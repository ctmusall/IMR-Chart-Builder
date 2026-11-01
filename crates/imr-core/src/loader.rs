//! Load tabular QC data from Excel workbooks or CSV files.
//!
//! Column names come from, in order: a sibling `Column Headers (<sheet>)` tab; the first row when
//! it looks like a header; otherwise `Column 1`, `Column 2`, … Fully blank rows are dropped. Every
//! row keeps its line number in the source file (the Excel row number, or the CSV line number),
//! so every charted point can be traced back.

use std::path::Path;
use std::sync::LazyLock;

use calamine::{Data, DataType, Reader, open_workbook_auto};
use chrono::NaiveDateTime;
use regex::Regex;

use crate::cell::{Cell, Column, DType, Frame, infer_dtype};
use crate::dates;
use crate::error::{Error, Result};
use crate::pyfmt::{pd_numeric, py_float};

pub const HIDDEN_SHEETS: &[&str] = &["MC Tags"];
pub const PREFERRED_SHEET: &str = "Summary";
pub const PREFERRED_DATE_COLUMN: &str = "Date Processed";
pub const PREFERRED_LABEL_COLUMNS: &[&str] = &["QC Lot", "Lot Number", "ISBT DIN"];
const CSV_SUFFIXES: &[&str] = &["csv", "txt", "tsv"];

/// Text that pandas reads as a blank cell.
const NA_VALUES: &[&str] = &[
    "", "#N/A", "#N/A N/A", "#NA", "-1.#IND", "-1.#QNAN", "-NaN", "-nan", "1.#IND", "1.#QNAN", "<NA>", "N/A",
    "NA", "NULL", "NaN", "None", "n/a", "nan", "null",
];

static HEADER_TAB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Column Headers \((?P<sheet>.+)\)$").unwrap());

fn suffix(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_csv(path: impl AsRef<Path>) -> bool {
    CSV_SUFFIXES.contains(&suffix(path.as_ref()).as_str())
}

/// The file name without its extension (a CSV's only "sheet").
pub fn stem(path: impl AsRef<Path>) -> String {
    path.as_ref().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn not_found(path: &Path) -> Error {
    Error::NotFound(format!("File not found: {}", path.display()))
}

/// The data-bearing sheet names, the preferred sheet (`Summary`) first.
pub fn list_sheets(path: impl AsRef<Path>) -> Result<Vec<String>> {
    let path = path.as_ref();
    if is_csv(path) {
        return Ok(vec![stem(path)]);
    }
    if !path.exists() {
        return Err(not_found(path));
    }
    let wb = open_workbook_auto(path).map_err(|e| Error::msg(e.to_string()))?;
    let mut data: Vec<String> = wb
        .sheet_names()
        .into_iter()
        .filter(|n| !HEADER_TAB.is_match(n) && !HIDDEN_SHEETS.contains(&n.as_str()))
        .collect();
    if let Some(i) = data.iter().position(|n| n == PREFERRED_SHEET) {
        let s = data.remove(i);
        data.insert(0, s);
    }
    Ok(data)
}

/// Load a sheet (or CSV) with resolved column names. `sheet` None means the first data sheet.
///
/// The frame's index holds each row's line number in the source file.
pub fn load_table(path: impl AsRef<Path>, sheet: Option<&str>) -> Result<Frame> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(not_found(path));
    }
    if is_csv(path) {
        return load_csv(path);
    }
    let mut wb = open_workbook_auto(path).map_err(|e| Error::msg(e.to_string()))?;
    let all = wb.sheet_names();
    let sheet = match sheet {
        Some(s) => s.to_string(),
        None => {
            list_sheets(path)?.into_iter().next().ok_or_else(|| Error::msg("The workbook has no sheets"))?
        }
    };
    if !all.contains(&sheet) {
        return Err(Error::msg(format!("Sheet '{sheet}' not found. Available: {}", all.join(", "))));
    }
    let raw = read_grid(&mut wb, &sheet)?;
    let width = raw.iter().map(Vec::len).max().unwrap_or(0);
    let header_tab =
        all.iter().find(|n| HEADER_TAB.captures(n).is_some_and(|m| m["sheet"] == sheet)).cloned();

    // Columns as pandas reads them with header=None: dtype inferred over every row.
    let mut columns: Vec<Column> = (0..width)
        .map(|j| {
            let mut cells: Vec<Cell> = raw.iter().map(|r| r.get(j).cloned().unwrap_or(Cell::Empty)).collect();
            let dtype = infer_dtype(&mut cells, DType::Float);
            Column::new("", dtype, cells)
        })
        .collect();
    let mut index: Vec<i64> = (1..=raw.len() as i64).collect();
    let first_row =
        |columns: &[Column]| -> Vec<Cell> { columns.iter().map(|c| c.cells[0].clone()).collect() };

    let names: Vec<Option<String>>;
    if let Some(tab) = header_tab {
        let tab_grid = read_grid(&mut wb, &tab)?;
        let mut tab_names: Vec<Option<String>> =
            tab_grid.first().map(|r| r.iter().map(header_name).collect()).unwrap_or_default();
        while tab_names.len() < width {
            tab_names.push(None);
        }
        // reindex(columns=range(len(names))): extra data columns are dropped, missing ones added.
        columns.truncate(tab_names.len());
        while columns.len() < tab_names.len() {
            columns.push(Column::new("", DType::Float, vec![Cell::Empty; index.len()]));
        }
        names = tab_names;
        if !index.is_empty() && row_looks_like_header(&first_row(&columns)) {
            drop_first(&mut columns, &mut index);
        }
    } else if !index.is_empty() && row_looks_like_header(&first_row(&columns)) {
        names = first_row(&columns).iter().map(header_name).collect();
        drop_first(&mut columns, &mut index);
    } else {
        names = vec![None; width];
    }
    for (c, n) in columns.iter_mut().zip(dedupe(&names)) {
        c.name = n;
    }
    let mut frame = drop_blank_rows(Frame { columns, index });
    // infer_objects(): object columns whose remaining values all share a type take that type.
    for c in frame.columns.iter_mut().filter(|c| c.dtype == DType::Object) {
        c.dtype = infer_dtype(&mut c.cells, DType::Object);
    }
    Ok(frame)
}

fn drop_first(columns: &mut [Column], index: &mut Vec<i64>) {
    for c in columns.iter_mut() {
        c.cells.remove(0);
    }
    index.remove(0);
}

/// A sheet's cells from A1 to the last non-blank row and column, as pandas reads them.
fn read_grid<RS: std::io::Read + std::io::Seek>(
    wb: &mut calamine::Sheets<RS>,
    sheet: &str,
) -> Result<Vec<Vec<Cell>>> {
    let range = wb.worksheet_range(sheet).map_err(|e| Error::msg(e.to_string()))?;
    let Some((r0, c0)) = range.start() else { return Ok(Vec::new()) };
    let (r0, c0) = (r0 as usize, c0 as usize);
    let mut grid: Vec<Vec<Cell>> = vec![Vec::new(); r0];
    for row in range.rows() {
        let mut out = vec![Cell::Empty; c0];
        out.extend(row.iter().map(excel_cell));
        while out.last().is_some_and(Cell::is_empty) {
            out.pop();
        }
        grid.push(out);
    }
    while grid.last().is_some_and(|r| r.is_empty()) {
        grid.pop();
    }
    Ok(grid)
}

fn excel_cell(d: &Data) -> Cell {
    match d {
        Data::Empty | Data::Error(_) => Cell::Empty,
        Data::Int(i) => Cell::Int(*i),
        Data::Float(f) => number(*f),
        Data::Bool(b) => Cell::Bool(*b),
        Data::String(s) => text_cell(s),
        Data::DateTime(dt) => {
            if dt.is_duration() {
                number(dt.as_f64())
            } else {
                d.as_datetime().map(Cell::Date).unwrap_or_else(|| number(dt.as_f64()))
            }
        }
        Data::DateTimeIso(s) => d.as_datetime().map(Cell::Date).unwrap_or_else(|| text_cell(s)),
        Data::DurationIso(s) => text_cell(s),
    }
}

/// A number cell: whole numbers read as ints, as pandas does.
fn number(f: f64) -> Cell {
    if f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 { Cell::Int(f as i64) } else { Cell::float(f) }
}

fn text_cell(s: &str) -> Cell {
    if NA_VALUES.contains(&s) { Cell::Empty } else { Cell::Text(s.to_string()) }
}

fn header_name(c: &Cell) -> Option<String> {
    if c.is_empty() { None } else { Some(c.py_str()) }
}

/// At least 80% of the row's non-blank cells are text that isn't a number.
fn row_looks_like_header(row: &[Cell]) -> bool {
    let values: Vec<&Cell> = row.iter().filter(|c| !c.is_empty()).collect();
    if values.is_empty() {
        return false;
    }
    let strings = values.iter().filter(|c| matches!(c, Cell::Text(s) if pd_numeric(s).is_none())).count();
    strings as f64 / values.len() as f64 >= 0.8
}

/// Column names: blank ones become `Column N`, repeats `name (2)`, `name (3)`, …
pub fn dedupe(names: &[Option<String>]) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut name = match n.as_deref().map(str::trim) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => format!("Column {}", i + 1),
            };
            let count = seen.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                name = format!("{name} ({count})");
            }
            name
        })
        .collect()
}

fn drop_blank_rows(frame: Frame) -> Frame {
    let keep: Vec<usize> =
        (0..frame.len()).filter(|&i| frame.columns.iter().any(|c| !c.cells[i].is_empty())).collect();
    if keep.len() == frame.len() { frame } else { frame.take(&keep) }
}

// --- CSV ---------------------------------------------------------------------------

fn load_csv(path: &Path) -> Result<Frame> {
    let bytes = std::fs::read(path).map_err(|e| Error::msg(format!("{}: {e}", path.display())))?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let delimiter = if suffix(path) == "tsv" { b'\t' } else { sniff_delimiter(text) };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut header: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut index: Vec<i64> = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| Error::msg(format!("{}: {e}", path.display())))?;
        let fields: Vec<String> = record.iter().map(str::to_string).collect();
        if header.is_none() {
            header = Some(fields);
        } else {
            // The line the record starts on (counting blank lines, which pandas keeps as rows).
            let mut byte = record.position().map_or(0, |p| p.byte() as usize).min(text.len());
            while matches!(text.as_bytes().get(byte), Some(b'\n' | b'\r')) {
                byte += 1; // the reader's position includes the blank lines it skipped
            }
            index.push(
                1 + text.as_bytes()[..byte.min(text.len())].iter().filter(|b| **b == b'\n').count() as i64,
            );
            rows.push(fields);
        }
    }
    let names: Vec<Option<String>> =
        pandas_header(&header.unwrap_or_default()).into_iter().map(Some).collect();
    let columns = dedupe(&names)
        .into_iter()
        .enumerate()
        .map(|(j, name)| {
            let raw: Vec<Option<&str>> = rows
                .iter()
                .map(|r| r.get(j).map(String::as_str).filter(|s| !NA_VALUES.contains(s)))
                .collect();
            let (dtype, cells) = csv_column(&raw);
            Column::new(name, dtype, cells)
        })
        .collect();
    Ok(drop_blank_rows(Frame { columns, index }))
}

/// Header names as pandas' `read_csv` gives them: a blank one is `Unnamed: N`, and a repeated
/// one gets `.1`, `.2`, … (before the loader's own clean-up).
fn pandas_header(header: &[String]) -> Vec<String> {
    let mut names: Vec<String> = header
        .iter()
        .enumerate()
        .map(|(i, h)| if h.is_empty() { format!("Unnamed: {i}") } else { h.clone() })
        .collect();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for i in 0..names.len() {
        let name = names[i].clone();
        let mut n = *counts.get(&name).unwrap_or(&0);
        if n > 0 {
            let mut candidate = format!("{name}.{n}");
            while names.contains(&candidate) {
                n += 1;
                candidate = format!("{name}.{n}");
            }
            names[i] = candidate;
        }
        counts.insert(name, n + 1);
    }
    names
}

/// One CSV column's values as pandas types them: all ints, all numbers, all booleans, or text.
fn csv_column(raw: &[Option<&str>]) -> (DType, Vec<Cell>) {
    let values: Vec<&str> = raw.iter().flatten().copied().collect();
    let has_blank = values.len() < raw.len();
    let map = |f: &dyn Fn(&str) -> Cell| raw.iter().map(|v| v.map_or(Cell::Empty, f)).collect::<Vec<Cell>>();
    if values.is_empty() {
        return (DType::Float, vec![Cell::Empty; raw.len()]);
    }
    let is_int = |s: &str| {
        let t = s.trim();
        let t = t.strip_prefix(['+', '-']).unwrap_or(t);
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) && s.trim().parse::<i64>().is_ok()
    };
    if values.iter().all(|s| is_int(s)) {
        if has_blank {
            return (DType::Float, map(&|s| Cell::Float(s.trim().parse::<i64>().unwrap() as f64)));
        }
        return (DType::Int, map(&|s| Cell::Int(s.trim().parse().unwrap())));
    }
    let is_float = |s: &str| !s.contains('_') && py_float(s).is_some();
    if values.iter().all(|s| is_float(s)) {
        return (DType::Float, map(&|s| Cell::float(py_float(s).unwrap())));
    }
    let as_bool = |s: &str| match s {
        "True" | "TRUE" | "true" => Some(true),
        "False" | "FALSE" | "false" => Some(false),
        _ => None,
    };
    if values.iter().all(|s| as_bool(s).is_some()) {
        let dtype = if has_blank { DType::Object } else { DType::Bool };
        return (dtype, map(&|s| Cell::Bool(as_bool(s).unwrap())));
    }
    (DType::Object, map(&|s| Cell::Text(s.to_string())))
}

/// The delimiter that splits the first lines most consistently (like Python's `csv.Sniffer`).
fn sniff_delimiter(text: &str) -> u8 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(20).collect();
    let mut best = (b',', 0usize, false);
    for d in *b",\t;|:" {
        let counts: Vec<usize> = lines.iter().map(|l| l.bytes().filter(|b| *b == d).count()).collect();
        let Some(&first) = counts.first() else { continue };
        let consistent = first > 0 && counts.iter().all(|&c| c == first);
        if consistent && (!best.2 || first > best.1) || (!best.2 && first > best.1) {
            best = (d, first, consistent);
        }
    }
    best.0
}

// --- reading cells as numbers and dates --------------------------------------------

/// One cell as the loader reads numbers: `1,000`, `5e7`, ` 7 `, `50%` (-> 0.5); None otherwise.
pub fn cell_number(c: &Cell) -> Option<f64> {
    let f = match c {
        Cell::Empty | Cell::Date(_) => return None,
        Cell::Bool(b) => f64::from(u8::from(*b)),
        Cell::Int(i) => *i as f64,
        Cell::Float(f) => *f,
        Cell::Text(s) => {
            let s = s.trim().replace(',', "");
            match s.strip_suffix('%') {
                Some(n) => py_float(n)? / 100.0,
                None => pd_numeric(&s)?,
            }
        }
    };
    if f.is_nan() { None } else { Some(f) }
}

/// A column's cells as numbers (None where they aren't). A datetime column reads as microseconds
/// since 1970, as pandas does, and a blank in it as the smallest 64-bit integer (pandas' NaT).
pub fn to_numeric(col: &Column) -> Vec<Option<f64>> {
    if col.dtype == DType::Date {
        return col
            .cells
            .iter()
            .map(|c| match c {
                Cell::Date(d) => Some(d.and_utc().timestamp_micros() as f64),
                _ => Some(i64::MIN as f64),
            })
            .collect();
    }
    col.cells.iter().map(cell_number).collect()
}

/// pandas `to_numeric(errors="coerce")` of one cell: no thousands separators or percents.
fn pd_number(c: &Cell) -> Option<f64> {
    match c {
        Cell::Empty | Cell::Date(_) => None,
        Cell::Bool(b) => Some(f64::from(u8::from(*b))),
        Cell::Int(i) => Some(*i as f64),
        Cell::Float(f) => Some(*f),
        Cell::Text(s) => pd_numeric(s).filter(|f| !f.is_nan()),
    }
}

/// One cell as a timestamp: a date as it is, an Excel serial number, or a date written as text.
pub fn cell_datetime(c: &Cell) -> Option<NaiveDateTime> {
    match c {
        Cell::Date(d) => Some(*d),
        Cell::Empty => None,
        other => match pd_number(other) {
            Some(n) if (1.0..=dates::MAX_SERIAL).contains(&n) => dates::from_serial(n),
            _ => dates::parse_datetime(&other.py_str()),
        },
    }
}

/// A column's cells as timestamps (None where they aren't dates).
pub fn to_datetime(col: &Column) -> Vec<Option<NaiveDateTime>> {
    col.cells.iter().map(cell_datetime).collect()
}

/// Whether a cell is blank: empty, or text that is only whitespace.
pub fn is_blank(c: &Cell) -> bool {
    c.is_blank()
}

/// Columns where at least `threshold` of the non-blank values are numbers (date columns excluded).
pub fn numeric_columns(frame: &Frame, threshold: f64) -> Vec<String> {
    frame.columns.iter().filter(|c| is_numeric_column(c, threshold)).map(|c| c.name.clone()).collect()
}

fn is_numeric_column(c: &Column, threshold: f64) -> bool {
    if c.dtype == DType::Date {
        return false;
    }
    let nonblank: Vec<&Cell> = c.cells.iter().filter(|v| !v.is_blank()).collect();
    if nonblank.is_empty() {
        return false;
    }
    if c.dtype.is_numeric() {
        return true;
    }
    let numbers = nonblank.iter().filter(|v| cell_number(v).is_some()).count();
    numbers as f64 / nonblank.len() as f64 >= threshold
}

/// Columns that look like dates, by dtype or by name; `Date Processed` first.
pub fn date_columns(frame: &Frame) -> Vec<String> {
    let mut out: Vec<String> = frame
        .columns
        .iter()
        .filter(|c| c.dtype == DType::Date || c.name.to_lowercase().contains("date"))
        .map(|c| c.name.clone())
        .collect();
    if let Some(i) = out.iter().position(|n| n == PREFERRED_DATE_COLUMN) {
        let n = out.remove(i);
        out.insert(0, n);
    }
    out
}

pub fn pick_date_column(frame: &Frame) -> Option<String> {
    date_columns(frame).into_iter().next()
}

pub fn pick_label_column(frame: &Frame) -> Option<String> {
    PREFERRED_LABEL_COLUMNS.iter().find(|n| frame.column(n).is_some()).map(|n| n.to_string())
}

/// Names present in every list, in the first list's order.
pub fn common_columns(lists: &[Vec<String>]) -> Vec<String> {
    let Some((first, rest)) = lists.split_first() else { return Vec::new() };
    first.iter().filter(|n| rest.iter().all(|l| l.contains(n))).cloned().collect()
}

/// [`numeric_columns`] judged over all files' rows together, limited to columns every file has.
///
/// Pooling first means a short file with a junk cell or two doesn't hide a column.
pub fn pooled_numeric_columns(frames: &[&Frame]) -> Vec<String> {
    let common = common_columns(&frames.iter().map(|f| f.names()).collect::<Vec<_>>());
    common
        .into_iter()
        .filter(|name| {
            let parts: Vec<&Column> = frames.iter().filter_map(|f| f.column(name)).collect();
            let pooled = concat_columns(name, &parts);
            is_numeric_column(&pooled, 0.8)
        })
        .collect()
}

/// Columns stacked end to end, with the dtype pandas' `concat` gives them.
fn concat_columns(name: &str, parts: &[&Column]) -> Column {
    let cells: Vec<Cell> = parts.iter().flat_map(|c| c.cells.iter().cloned()).collect();
    let dtypes: Vec<DType> = parts.iter().filter(|c| !c.is_empty()).map(|c| c.dtype).collect();
    let dtype = match dtypes.first() {
        None => parts.first().map_or(DType::Object, |c| c.dtype),
        Some(&d) if dtypes.iter().all(|x| *x == d) => d,
        _ if dtypes.iter().all(|d| matches!(d, DType::Int | DType::Float)) => DType::Float,
        _ => DType::Object,
    };
    Column::new(name, dtype, cells)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_names() {
        let names = [Some("A".into()), None, Some(" A ".into()), Some("".into()), Some("B".into())];
        assert_eq!(dedupe(&names), ["A", "Column 2", "A (2)", "Column 4", "B"]);
    }

    #[test]
    fn csv_header_names() {
        let h: Vec<String> = ["A", "A", "", "B"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pandas_header(&h), ["A", "A.1", "Unnamed: 2", "B"]);
    }

    #[test]
    fn sniff() {
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n"), b';');
        assert_eq!(sniff_delimiter("a,b\n1,2\n"), b',');
        assert_eq!(sniff_delimiter("a\tb\n1\t2\n"), b'\t');
    }
}
