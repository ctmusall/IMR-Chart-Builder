//! Excel export of I-MR statistics, per-point data and out-of-limit rows.
//!
//! Besides the program's results, the workbook carries the evidence for them:
//!
//! - **Verification** recomputes every statistic with ordinary Excel formulas and compares it with
//!   the program's value (PASS/FAIL), so a reviewer can check the chart without trusting this
//!   program's arithmetic.
//! - **Recompute** does the per-point work behind that sheet (moving ranges, flags, and a lookup of
//!   each charted value back to its source row).
//! - **Worked formulas** writes each calculation out with this data's numbers.
//! - **Input audit** lists every source row with its raw cell values and why it was or wasn't charted.
//!
//! The workbook is first laid out as [`SheetModel`]s (values, styles, widths), then written; tests
//! check the layout without reading styles back from the file.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};
use rust_xlsxwriter::{
    Color, ConditionalFormatCell, ConditionalFormatCellRule, ConditionalFormatFormula, Format, FormatPattern,
    Workbook,
};

use crate::cell::Cell;
use crate::error::{Error, Result};
use crate::proof::{CONSTANTS_SOURCE, worked_steps};
use crate::pyfmt::{py_float_repr, py_format_g, py_str_datetime};
use crate::report::{AuditRow, ChartJob, Value};
use crate::stats::{D2, D3, ImrResult};

const HEADER_FILL: u32 = 0x305496;
const HEADER_FONT_COLOR: u32 = 0xFFFFFF;
const FLAG_FILL: u32 = 0xF8CBAD;
const PASS_FILL: u32 = 0xC6EFCE;
const FAIL_FILL: u32 = 0xFFC7CE;
const NOTE_COLOR: u32 = 0x555555;
/// Relative tolerance for "same number": allows for Excel summing in a different order.
const TOLERANCE: &str = "1E-9";
const DATE_FORMAT: &str = "YYYY-MM-DD";
const DATETIME_FORMAT: &str = "YYYY-MM-DD HH:MM:SS";
const SCI_FORMAT: &str = "0.000000E+00";

/// A cell's value as it is written.
#[derive(Debug, Clone, PartialEq)]
pub enum XValue {
    Empty,
    Int(i64),
    Number(f64),
    Bool(bool),
    Text(String),
    /// Formula text, starting with `=`.
    Formula(String),
    Date(NaiveDate),
    DateTime(NaiveDateTime),
}

impl XValue {
    fn number(f: f64) -> XValue {
        if f.is_nan() { XValue::Empty } else { XValue::Number(f) }
    }

    fn text(s: &str) -> XValue {
        if s.is_empty() { XValue::Empty } else { XValue::Text(s.to_string()) }
    }

    /// Python `str()` of the value openpyxl holds, which sets the column width.
    fn width_text(&self) -> Option<String> {
        match self {
            XValue::Empty => None,
            XValue::Int(i) => Some(i.to_string()),
            XValue::Number(f) => Some(py_float_repr(*f)),
            XValue::Bool(true) => Some("True".into()),
            XValue::Bool(false) => Some("False".into()),
            XValue::Text(s) | XValue::Formula(s) => Some(s.clone()),
            XValue::Date(d) => Some(d.format("%Y-%m-%d").to_string()),
            XValue::DateTime(d) => Some(py_str_datetime(d)),
        }
    }
}

/// A number as the file stores it: 16 significant digits, as openpyxl writes them (the
/// Worked formulas notes say so). Column widths use the full value, as the Python version did.
pub fn stored_number(f: f64) -> f64 {
    format!("{f:.15e}").parse().unwrap_or(f)
}

/// A raw input cell as pandas writes it.
fn cell_value(c: &Cell) -> XValue {
    match c {
        Cell::Empty => XValue::Empty,
        Cell::Bool(b) => XValue::Bool(*b),
        Cell::Int(i) => XValue::Int(*i),
        Cell::Float(f) => XValue::number(*f),
        Cell::Text(s) => XValue::text(s),
        Cell::Date(d) => XValue::DateTime(*d),
    }
}

/// How a cell looks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XStyle {
    pub bold: bool,
    pub italic: bool,
    pub font_color: Option<u32>,
    pub font_size: Option<f64>,
    pub fill: Option<u32>,
    pub num_format: Option<&'static str>,
}

impl XStyle {
    fn header() -> XStyle {
        XStyle {
            bold: true,
            font_color: Some(HEADER_FONT_COLOR),
            fill: Some(HEADER_FILL),
            ..XStyle::default()
        }
    }

    fn title() -> XStyle {
        XStyle { bold: true, font_size: Some(14.0), ..XStyle::default() }
    }

    fn note() -> XStyle {
        XStyle { italic: true, font_color: Some(NOTE_COLOR), ..XStyle::default() }
    }
}

/// A conditional format: cells equal to a value, or a formula that is true.
#[derive(Debug, Clone, PartialEq)]
pub enum CfRule {
    Equal(String),
    Expression(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CondFormat {
    /// `(first row, first col, last row, last col)`, zero-based.
    pub range: (u32, u16, u32, u16),
    pub rule: CfRule,
    pub fill: u32,
}

/// One worksheet as it will be written.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SheetModel {
    pub name: String,
    /// Cells by (row, column), zero-based.
    pub cells: BTreeMap<(u32, u16), (XValue, XStyle)>,
    pub widths: BTreeMap<u16, f64>,
    /// Freeze the header row.
    pub freeze: bool,
    pub cf: Vec<CondFormat>,
}

impl SheetModel {
    fn new(name: &str) -> SheetModel {
        SheetModel { name: name.into(), ..SheetModel::default() }
    }

    fn set(&mut self, row: u32, col: u16, value: XValue) {
        self.cells.entry((row, col)).or_insert((XValue::Empty, XStyle::default())).0 = value;
    }

    fn style(&mut self, row: u32, col: u16) -> &mut XStyle {
        &mut self.cells.entry((row, col)).or_insert((XValue::Empty, XStyle::default())).1
    }

    /// Append a row after the last one (openpyxl's `ws.append`).
    fn append(&mut self, values: Vec<XValue>) {
        let row = self.max_row().map_or(0, |r| r + 1);
        self.append_at(row, values);
    }

    fn append_at(&mut self, row: u32, values: Vec<XValue>) {
        for (j, v) in values.into_iter().enumerate() {
            self.set(row, j as u16, v);
        }
        // An empty row still counts (openpyxl keeps it), so the next append goes below it.
        self.cells.entry((row, 0)).or_insert((XValue::Empty, XStyle::default()));
    }

    pub fn max_row(&self) -> Option<u32> {
        self.cells.keys().map(|(r, _)| *r).max()
    }

    pub fn max_col(&self) -> Option<u16> {
        self.cells.keys().map(|(_, c)| *c).max()
    }

    pub fn value(&self, row: u32, col: u16) -> &XValue {
        self.cells.get(&(row, col)).map_or(&XValue::Empty, |(v, _)| v)
    }

    /// A table written the way pandas' `to_excel` does: a header row, then one row per record
    /// (formats for dates and timestamps).
    fn table(name: &str, headers: &[&str], rows: Vec<Vec<XValue>>) -> SheetModel {
        let mut ws = SheetModel::new(name);
        ws.append_at(0, headers.iter().map(|h| XValue::Text((*h).into())).collect());
        for (i, row) in rows.into_iter().enumerate() {
            for (j, v) in row.into_iter().enumerate() {
                let format = match &v {
                    XValue::Date(_) => Some(DATE_FORMAT),
                    XValue::DateTime(_) => Some(DATETIME_FORMAT),
                    _ => None,
                };
                ws.set(i as u32 + 1, j as u16, v);
                if format.is_some() {
                    ws.style(i as u32 + 1, j as u16).num_format = format;
                }
            }
        }
        ws
    }

    /// Header row colors, widths from the first 200 rows (10 to 45), header row frozen; with
    /// `flag_cols`, rows with a true flag are filled.
    fn style_table(&mut self, flag_cols: &[u16]) {
        let Some(last_col) = self.max_col() else { return };
        let last_row = self.max_row().unwrap_or(0);
        for c in 0..=last_col {
            let s = self.style(0, c);
            s.bold = true;
            s.font_color = Some(HEADER_FONT_COLOR);
            s.fill = Some(HEADER_FILL);
        }
        for c in 0..=last_col {
            let longest = (0..=last_row.min(199))
                .filter_map(|r| self.value(r, c).width_text())
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0);
            self.widths.insert(c, 45f64.min(10f64.max(longest as f64 + 2.0)));
        }
        self.freeze = true;
        if !flag_cols.is_empty() {
            for r in 1..=last_row {
                if flag_cols.iter().any(|c| *self.value(r, *c) == XValue::Bool(true)) {
                    for c in 0..=last_col {
                        self.style(r, c).fill = Some(FLAG_FILL);
                    }
                }
            }
        }
    }
}

fn col_letter(col: u16) -> String {
    let mut n = col as u32 + 1;
    let mut s = String::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        s.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

fn count(flags: &[bool]) -> i64 {
    flags.iter().filter(|b| **b).count() as i64
}

/// The Summary stats rows: statistic and value.
pub fn stats_rows(
    result: &ImrResult,
    column: &str,
    source: &str,
    excluded: usize,
    provenance: &[(String, Value)],
) -> Vec<(String, XValue)> {
    let mut rows: Vec<(String, XValue)> = vec![("Source file".into(), XValue::text(source))];
    for (k, v) in provenance {
        rows.push((
            k.clone(),
            match v {
                Value::Text(s) => XValue::text(s),
                Value::Int(i) => XValue::Int(*i),
            },
        ));
    }
    let dash = || XValue::Text("—".into());
    rows.extend([
        ("Column".into(), XValue::text(column)),
        ("Points (n)".into(), XValue::Int(result.n() as i64)),
        ("Rows excluded".into(), XValue::Int(excluded as i64)),
        ("Mean (center line)".into(), XValue::number(result.mean)),
        ("Average moving range (MR̄)".into(), XValue::number(result.mr_bar)),
        ("Sigma (MR̄/1.128)".into(), XValue::number(result.sigma)),
        ("Sigma multiplier (k)".into(), XValue::number(result.k)),
        ("Individuals UCL".into(), XValue::number(result.ucl)),
        ("Individuals LCL".into(), XValue::number(result.lcl)),
        ("Moving range UCL".into(), XValue::number(result.mr_ucl)),
        ("Moving range LCL".into(), XValue::number(result.mr_lcl)),
        ("LSL".into(), result.lsl.map_or_else(dash, XValue::number)),
        ("Points above UCL".into(), XValue::Int(count(&result.above_ucl))),
        ("Points below LCL".into(), XValue::Int(count(&result.below_lcl))),
        (
            "Points below LSL".into(),
            if result.lsl.is_some() { XValue::Int(count(&result.below_lsl)) } else { dash() },
        ),
        ("Moving ranges above MR UCL".into(), XValue::Int(count(&result.mr_above_ucl))),
    ]);
    rows
}

const POINT_HEADERS: &[&str] = &[
    "Lot",
    "Date",
    "Value",
    "Moving Range",
    "Above UCL",
    "Below LCL",
    "Below LSL",
    "MR above UCL",
    "Source row",
    "Source file",
    "Highlight rule",
];

/// The Data sheet's rows, one per charted point.
fn point_rows(job: &ChartJob) -> Vec<Vec<XValue>> {
    let r = &job.result;
    let dated = job.has_dates();
    job.data
        .iter()
        .enumerate()
        .map(|(i, p)| {
            vec![
                cell_value(&p.label),
                if dated { p.date.map_or(XValue::Empty, |d| XValue::Date(d.date())) } else { XValue::Empty },
                XValue::number(r.values[i]),
                XValue::number(r.moving_ranges[i]),
                XValue::Bool(r.above_ucl[i]),
                XValue::Bool(r.below_lcl[i]),
                XValue::Bool(r.below_lsl[i]),
                XValue::Bool(r.mr_above_ucl[i]),
                XValue::Int(p.row),
                XValue::text(&p.file),
                XValue::text(p.rule.as_deref().unwrap_or("")),
            ]
        })
        .collect()
}

const AUDIT_HEADERS: &[&str] = &[
    "Source row",
    "Lot",
    "Raw date",
    "Raw value",
    "Parsed date",
    "Parsed value",
    "Included",
    "Reason excluded",
    "Source file",
];

fn audit_row(a: &AuditRow) -> Vec<XValue> {
    vec![
        XValue::Int(a.row),
        cell_value(&a.label),
        cell_value(&a.raw_date),
        cell_value(&a.raw_value),
        a.date.map_or(XValue::Empty, |d| XValue::Date(d.date())),
        a.value.map_or(XValue::Empty, XValue::number),
        XValue::Text(if a.included() { "Yes" } else { "No" }.into()),
        XValue::text(&a.reason),
        XValue::text(&a.file),
    ]
}

fn same(a: &str, b: &str) -> String {
    format!("ABS({a}-{b})<={TOLERANCE}*MAX(1,ABS({b}))")
}

/// Addresses of the Excel-recomputed limits on the Verification sheet.
struct LimitCells {
    ucl: String,
    lcl: String,
    mr_ucl: String,
    lsl: String,
}

fn verification(result: &ImrResult, excluded: usize, n_audit: usize) -> (SheetModel, LimitCells) {
    let mut ws = SheetModel::new("Verification");
    let n = result.n();
    let last = n + 1;
    let rc = |col: &str, first: usize| format!("Recompute!{col}{first}:{col}{last}");
    let audit_col = |col: &str| format!("'Input audit'!{col}2:{col}{}", n_audit + 1);

    ws.set(0, 0, XValue::Text("Overall result".into()));
    *ws.style(0, 0) = XStyle::title();
    ws.set(
        1,
        0,
        XValue::Text(format!(
            "Column C is calculated by Excel, using ordinary formulas on the Data and Input audit sheets. \
             Column B is what the program calculated and drew on the chart. A check passes when they agree \
             to within a relative {TOLERANCE}."
        )),
    );
    ws.set(
        2,
        0,
        XValue::Text(
            "If column C or the overall result is blank, click 'Enable Editing' so Excel can calculate. \
             Each value in Data can be traced to its row in the source file through 'Source file' and \
             'Source row'."
                .into(),
        ),
    );
    *ws.style(1, 0) = XStyle::note();
    *ws.style(2, 0) = XStyle::note();

    ws.set(4, 0, XValue::Text("Inputs".into()));
    ws.set(4, 1, XValue::Text("Value".into()));
    let params = [
        ("Sigma multiplier (k)", XValue::number(result.k)),
        ("LSL", result.lsl.map_or(XValue::Text("none".into()), XValue::number)),
        ("d₂ (moving range span 2)", XValue::Number(D2)),
        ("d₃ (moving range span 2)", XValue::Number(D3)),
    ];
    for (i, (name, value)) in params.into_iter().enumerate() {
        ws.set(5 + i as u32, 0, XValue::Text(name.into()));
        ws.set(5 + i as u32, 1, value);
    }
    let (k, lsl, d2, d3) =
        ("Verification!$B$6", "Verification!$B$7", "Verification!$B$8", "Verification!$B$9");

    let header_row = 11; // Excel row number
    for (j, text) in ["Check", "Program value", "Excel recomputed", "Difference", "Result"].iter().enumerate()
    {
        ws.set(header_row - 1, j as u16, XValue::Text((*text).into()));
    }
    // (key, label, program value, Excel formula)
    let mut checks: Vec<(&str, &str, XValue, String)> = Vec::new();
    let row_of = |checks: &[(&str, &str, XValue, String)], key: &str| -> usize {
        header_row as usize + 1 + checks.iter().position(|c| c.0 == key).expect("check key")
    };
    let c = |checks: &[(&str, &str, XValue, String)], key: &str| {
        format!("Verification!$C${}", row_of(checks, key))
    };
    let int = |v: usize| XValue::Int(v as i64);

    checks.push(("rows", "Rows read from source", int(n_audit), format!("=COUNT({})", audit_col("A"))));
    checks.push(("excluded", "Rows excluded", int(excluded), format!("=COUNTIF({},\"No\")", audit_col("G"))));
    checks.push(("n", "Points charted (n)", int(n), format!("=COUNT({})", rc("B", 2))));
    checks.push((
        "included",
        "Included rows in Input audit",
        int(n),
        format!("=COUNTIF({},\"Yes\")", audit_col("G")),
    ));
    checks.push((
        "src",
        "Charted values that differ from their source row",
        int(0),
        format!("=COUNTIF({},FALSE)", rc("D", 2)),
    ));
    checks.push((
        "mean",
        "Mean (Individuals center line)",
        XValue::number(result.mean),
        format!("=AVERAGE({})", rc("B", 2)),
    ));
    checks.push((
        "mr_bad",
        "Moving ranges that differ from Data",
        int(0),
        format!("=COUNTIF({},FALSE)", rc("F", 2)),
    ));
    checks.push((
        "mr_bar",
        "Average moving range (MR̄)",
        XValue::number(result.mr_bar),
        format!("=AVERAGE({})", rc("E", 3)),
    ));
    let f = format!("={}/{d2}", c(&checks, "mr_bar"));
    checks.push(("sigma", "Sigma = MR̄ / d₂", XValue::number(result.sigma), f));
    let f = format!("={}+{k}*{}", c(&checks, "mean"), c(&checks, "sigma"));
    checks.push(("ucl", "Individuals UCL = mean + k·σ", XValue::number(result.ucl), f));
    let f = format!("={}-{k}*{}", c(&checks, "mean"), c(&checks, "sigma"));
    checks.push(("lcl", "Individuals LCL = mean − k·σ", XValue::number(result.lcl), f));
    let f = format!("={}+{k}*{d3}*{}", c(&checks, "mr_bar"), c(&checks, "sigma"));
    checks.push(("mr_ucl", "Moving range UCL = MR̄ + k·d₃·σ", XValue::number(result.mr_ucl), f));
    let f = format!("=MAX(0,{}-{k}*{d3}*{})", c(&checks, "mr_bar"), c(&checks, "sigma"));
    checks.push(("mr_lcl", "Moving range LCL = max(0, MR̄ − k·d₃·σ)", XValue::number(result.mr_lcl), f));
    checks.push((
        "above",
        "Points above UCL",
        XValue::Int(count(&result.above_ucl)),
        format!("=COUNTIF({},TRUE)", rc("G", 2)),
    ));
    checks.push((
        "below",
        "Points below LCL",
        XValue::Int(count(&result.below_lcl)),
        format!("=COUNTIF({},TRUE)", rc("H", 2)),
    ));
    if result.lsl.is_some() {
        checks.push((
            "below_lsl",
            "Points below LSL",
            XValue::Int(count(&result.below_lsl)),
            format!("=COUNTIF({},TRUE)", rc("I", 2)),
        ));
    }
    checks.push((
        "mr_above",
        "Moving ranges above MR UCL",
        XValue::Int(count(&result.mr_above_ucl)),
        format!("=COUNTIF({},TRUE)", rc("J", 2)),
    ));
    checks.push((
        "flags",
        "Points whose flags differ from Data",
        int(0),
        format!("=COUNTIF({},FALSE)", rc("K", 2)),
    ));

    let first = header_row + 1;
    let end = header_row + checks.len() as u32;
    for (i, (_, label, program, formula)) in checks.iter().enumerate() {
        let r = first + i as u32; // Excel row number
        ws.set(r - 1, 0, XValue::Text((*label).into()));
        ws.set(r - 1, 1, program.clone());
        ws.set(r - 1, 2, XValue::Formula(formula.clone()));
        ws.set(r - 1, 3, XValue::Formula(format!("=C{r}-B{r}")));
        ws.set(
            r - 1,
            4,
            XValue::Formula(format!("=IF({},\"PASS\",\"FAIL\")", same(&format!("C{r}"), &format!("B{r}")))),
        );
    }
    let results = format!("E{first}:E{end}");
    let n_checks = checks.len();
    ws.set(
        0,
        1,
        XValue::Formula(format!(
            "=IF(AND(COUNTIF({results},\"FAIL\")=0,COUNTIF({results},\"PASS\")={n_checks}),\
             \"ALL {n_checks} CHECKS PASS\",\"CHECK FAILED\")"
        )),
    );
    *ws.style(0, 1) = XStyle::title();

    let e = (first - 1, 4, end - 1, 4);
    ws.cf.push(CondFormat { range: e, rule: CfRule::Equal("\"PASS\"".into()), fill: PASS_FILL });
    ws.cf.push(CondFormat { range: e, rule: CfRule::Equal("\"FAIL\"".into()), fill: FAIL_FILL });
    ws.cf.push(CondFormat {
        range: (0, 1, 0, 1),
        rule: CfRule::Expression("LEFT(B1,3)=\"ALL\"".into()),
        fill: PASS_FILL,
    });
    ws.cf.push(CondFormat {
        range: (0, 1, 0, 1),
        rule: CfRule::Expression("B1=\"CHECK FAILED\"".into()),
        fill: FAIL_FILL,
    });
    for row in [4, header_row - 1] {
        for col in 0..5 {
            if *ws.value(row, col) != XValue::Empty {
                *ws.style(row, col) = XStyle::header();
            }
        }
    }
    for (col, width) in [48.0, 20.0, 20.0, 14.0, 10.0].into_iter().enumerate() {
        ws.widths.insert(col as u16, width);
    }
    for r in first..=end {
        for col in 1..4 {
            ws.style(r - 1, col).num_format = Some(SCI_FORMAT);
        }
    }
    let limits = LimitCells {
        ucl: c(&checks, "ucl"),
        lcl: c(&checks, "lcl"),
        mr_ucl: c(&checks, "mr_ucl"),
        lsl: lsl.to_string(),
    };
    (ws, limits)
}

fn worked(result: &ImrResult) -> SheetModel {
    let mut ws = SheetModel::new("Worked formulas");
    ws.append(
        ["Quantity", "Formula", "With this data's numbers", "Result"]
            .map(|s| XValue::Text(s.into()))
            .to_vec(),
    );
    for step in worked_steps(result) {
        ws.append(vec![
            XValue::Text(step.quantity.into()),
            XValue::Text(step.formula.into()),
            XValue::Text(step.numbers),
            match step.value {
                crate::proof::StepValue::Int(i) => XValue::Int(i),
                crate::proof::StepValue::Float(f) => XValue::number(f),
            },
        ]);
    }
    ws.append(Vec::new());
    ws.append(vec![XValue::Text("Notes".into())]);
    let notes = [
        format!(
            "k (sigma multiplier) = {}. Points and moving ranges are flagged when strictly beyond a limit.",
            py_format_g(result.k, 6)
        ),
        CONSTANTS_SOURCE.to_string(),
        "Limits are calculated from every charted point, including the highlighted recent window.".into(),
        "The Verification sheet recomputes each result above with Excel formulas.".into(),
        "Numbers are stored to 16 significant digits (Excel displays 15), so a source value may differ in its \
         17th digit."
            .into(),
    ];
    for note in notes {
        ws.append(vec![XValue::Text(note)]);
    }
    ws.style_table(&[]);
    ws.widths.insert(0, 42.0);
    ws
}

fn recompute(n: usize, n_audit: usize, cells: &LimitCells, has_lsl: bool) -> SheetModel {
    let mut ws = SheetModel::new("Recompute");
    let headers = [
        "Point",
        "Value (Data)",
        "Value in source row",
        "Value matches source",
        "Moving range (Excel)",
        "MR matches Data",
        "Above UCL",
        "Below LCL",
        "Below LSL",
        "MR above UCL",
        "Flags match Data",
    ];
    ws.append(headers.map(|h| XValue::Text(h.into())).to_vec());
    let key = format!("'Input audit'!$J$2:$J${}", n_audit + 1);
    let raw = format!("'Input audit'!$D$2:$D${}", n_audit + 1);
    let f = |s: String| XValue::Formula(s);
    for i in 1..=n {
        let r = i + 1;
        let src = format!("C{r}");
        let parsed = format!("NUMBERVALUE({src},\".\",\",\")");
        let b = format!("B{r}");
        ws.append(vec![
            XValue::Int(i as i64),
            f(format!("=Data!C{r}")),
            // Source file + row identifies the input row, even when several files share row numbers.
            f(format!("=INDEX({raw},MATCH(Data!J{r}&\"|\"&Data!I{r},{key},0))")),
            // Text cells ("50%", "1,000") are parsed the way the loader does, independent of locale.
            f(format!("=IF(ISNUMBER({src}),{},IFERROR({},FALSE))", same(&src, &b), same(&parsed, &b))),
            if i == 1 { XValue::Empty } else { f(format!("=ABS(B{r}-B{})", r - 1)) },
            if i == 1 {
                f(format!("=ISBLANK(Data!D{r})"))
            } else {
                f(format!("={}", same(&format!("E{r}"), &format!("Data!D{r}"))))
            },
            f(format!("=B{r}>{}", cells.ucl)),
            f(format!("=B{r}<{}", cells.lcl)),
            if has_lsl { f(format!("=B{r}<{}", cells.lsl)) } else { f("=FALSE".into()) },
            if i == 1 { f("=FALSE".into()) } else { f(format!("=E{r}>{}", cells.mr_ucl)) },
            f(format!("=AND(G{r}=Data!E{r},H{r}=Data!F{r},I{r}=Data!G{r},J{r}=Data!H{r})")),
        ]);
    }
    ws.style_table(&[]);
    ws
}

/// The workbook's sheets, in order.
pub fn workbook_model(job: &ChartJob, generated: &str) -> Vec<SheetModel> {
    let r = &job.result;
    let source = if job.sources.len() == 1 {
        job.source().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        format!("{} files", job.sources.len())
    };
    let stats_rows = stats_rows(r, &job.column, &source, job.excluded, &job.provenance(generated));
    let mut stats = SheetModel::table(
        "Summary stats",
        &["Statistic", "Value"],
        stats_rows.into_iter().map(|(k, v)| vec![XValue::Text(k), v]).collect(),
    );
    let n_audit = job.audit.len();
    let (verify, cells) = verification(r, job.excluded, n_audit);
    let worked = worked(r);
    let points = point_rows(job);
    let out_rows: Vec<Vec<XValue>> =
        points.iter().zip(r.out_of_limits()).filter(|(_, o)| *o).map(|(p, _)| p.clone()).collect();
    let mut data = SheetModel::table("Data", POINT_HEADERS, points);
    let recompute = recompute(r.n(), n_audit, &cells, r.lsl.is_some());
    let mut audit =
        SheetModel::table("Input audit", AUDIT_HEADERS, job.audit.iter().map(audit_row).collect());
    audit.set(0, 9, XValue::Text("Lookup key".into()));
    for row in 2..n_audit + 2 {
        audit.set(row as u32 - 1, 9, XValue::Formula(format!("=I{row}&\"|\"&A{row}")));
    }
    let mut out = SheetModel::table("Out of limits", POINT_HEADERS, out_rows);
    stats.style_table(&[]);
    data.style_table(&[4, 5, 6, 7]);
    for (i, p) in job.data.iter().enumerate() {
        if let Some(color) = p.highlight.as_deref().filter(|c| !c.is_empty()) {
            let rgb = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0);
            data.style(i as u32 + 1, 10).fill = Some(rgb);
        }
    }
    audit.style_table(&[]);
    out.style_table(&[]);
    vec![stats, verify, worked, data, recompute, audit, out]
}

/// The column letter of a zero-based column (0 -> A).
pub fn column_letter(col: u16) -> String {
    col_letter(col)
}

fn format_for(style: &XStyle) -> Format {
    let mut f = Format::new();
    if style.bold {
        f = f.set_bold();
    }
    if style.italic {
        f = f.set_italic();
    }
    if let Some(c) = style.font_color {
        f = f.set_font_color(Color::RGB(c));
    }
    if let Some(s) = style.font_size {
        f = f.set_font_size(s);
    }
    if let Some(c) = style.fill {
        f = f.set_pattern(FormatPattern::Solid).set_background_color(Color::RGB(c));
    }
    if let Some(n) = style.num_format {
        f = f.set_num_format(n);
    }
    f
}

fn xlsx_error(e: rust_xlsxwriter::XlsxError) -> Error {
    Error::msg(e.to_string())
}

/// Write the model to a workbook file.
pub fn write_model(sheets: &[SheetModel], path: &Path) -> Result<()> {
    let mut wb = Workbook::new();
    for model in sheets {
        let ws = wb.add_worksheet();
        ws.set_name(&model.name).map_err(xlsx_error)?;
        for (&(row, col), (value, style)) in &model.cells {
            let fmt = format_for(style);
            let res = match value {
                XValue::Empty => {
                    if *style == XStyle::default() {
                        continue;
                    }
                    ws.write_blank(row, col, &fmt).map(|_| ())
                }
                XValue::Int(i) => ws.write_number_with_format(row, col, *i as f64, &fmt).map(|_| ()),
                XValue::Number(n) => {
                    ws.write_number_with_format(row, col, stored_number(*n), &fmt).map(|_| ())
                }
                XValue::Bool(b) => ws.write_boolean_with_format(row, col, *b, &fmt).map(|_| ()),
                XValue::Text(s) => ws.write_string_with_format(row, col, s, &fmt).map(|_| ()),
                // Excel 2013+ functions (NUMBERVALUE) need their file-format prefix.
                XValue::Formula(s) => ws
                    .write_formula_with_format(
                        row,
                        col,
                        s.replace("NUMBERVALUE(", "_xlfn.NUMBERVALUE(").as_str(),
                        &fmt,
                    )
                    .map(|_| ()),
                XValue::Date(d) => ws.write_datetime_with_format(row, col, d, &fmt).map(|_| ()),
                XValue::DateTime(d) => ws.write_datetime_with_format(row, col, d, &fmt).map(|_| ()),
            };
            res.map_err(xlsx_error)?;
        }
        for (&col, &width) in &model.widths {
            ws.set_column_width(col, width).map_err(xlsx_error)?;
        }
        if model.freeze {
            ws.set_freeze_panes(1, 0).map_err(xlsx_error)?;
        }
        for cf in &model.cf {
            let fill = Format::new().set_background_color(Color::RGB(cf.fill));
            let (r0, c0, r1, c1) = cf.range;
            match &cf.rule {
                CfRule::Equal(v) => {
                    let rule = ConditionalFormatCell::new()
                        .set_rule(ConditionalFormatCellRule::EqualTo(v.as_str()))
                        .set_format(fill);
                    ws.add_conditional_format(r0, c0, r1, c1, &rule).map_err(xlsx_error)?;
                }
                CfRule::Expression(f) => {
                    let rule = ConditionalFormatFormula::new().set_rule(f.as_str()).set_format(fill);
                    ws.add_conditional_format(r0, c0, r1, c1, &rule).map_err(xlsx_error)?;
                }
            }
        }
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::msg(format!("{}: {e}", dir.display())))?;
    }
    wb.save(path).map_err(xlsx_error)
}

/// Save the chart's statistics workbook.
pub fn save_xlsx(job: &ChartJob, path: &Path) -> Result<()> {
    write_model(&workbook_model(job, &crate::report::now_stamp()), path)
}
