//! Glue between loading, statistics and exports, shared by the CLI and the app.
//!
//! A [`ChartJob`] holds one chart's data: every input row with why it was or wasn't charted
//! (the audit), the charted points, the statistics, and what it was made from.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::NaiveDateTime;
use regex::Regex;

use crate::cell::{Cell, Column, Frame, is_midnight};
use crate::error::{Error, Result};
use crate::loader;
use crate::proof::file_sha256;
use crate::pyfmt::{py_float_repr, py_str_datetime};
use crate::rules::{self, Rule};
use crate::stats::{ImrResult, compute_imr};

pub const FORMATS: &[&str] = &["png", "html", "xlsx"];
pub const DEFAULT_RECENT_DAYS: i64 = 30;

/// One input file's table and the columns to chart from it.
#[derive(Debug, Clone)]
pub struct SourceTable {
    pub path: PathBuf,
    pub frame: Frame,
    pub column: String,
    pub date_column: Option<String>,
    pub sheet: Option<String>,
    pub label_column: Option<String>,
}

/// One input row: its raw and parsed values, and whether it is charted (and why not).
#[derive(Debug, Clone, PartialEq)]
pub struct AuditRow {
    /// The row's line number in its source file.
    pub row: i64,
    /// The label cell as text (blank when the label cell is blank).
    pub label: Cell,
    pub raw_date: Cell,
    pub raw_value: Cell,
    pub date: Option<NaiveDateTime>,
    pub value: Option<f64>,
    pub file: String,
    /// Why the row is not charted; "" when it is.
    pub reason: String,
    /// The first matching highlight rule's label and color ("" for none); None without rules.
    pub rule: Option<String>,
    pub highlight: Option<String>,
}

impl AuditRow {
    pub fn included(&self) -> bool {
        self.reason.is_empty()
    }
}

/// One charted point.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub date: Option<NaiveDateTime>,
    pub label: Cell,
    pub value: f64,
    pub row: i64,
    pub file: String,
    pub rule: Option<String>,
    pub highlight: Option<String>,
}

/// pandas `astype(str)` of a column, keeping blanks blank.
fn text_cells(col: &Column) -> Vec<Cell> {
    col.as_text()
        .into_iter()
        .zip(&col.cells)
        .map(|(t, c)| if c.is_empty() { Cell::Empty } else { Cell::Text(t) })
        .collect()
}

/// One row per input row: raw and parsed values, and whether it is charted and why not.
///
/// With highlight `rules`, also each row's first matching rule. `sheet` names the sheet `frame`
/// came from, so rules for other sheets are skipped.
pub fn audit_rows(
    frame: &Frame,
    value_col: &str,
    date_col: Option<&str>,
    label_col: Option<&str>,
    source: &str,
    rules: &[Rule],
    sheet: Option<&str>,
) -> Result<Vec<AuditRow>> {
    let value =
        frame.column(value_col).ok_or_else(|| Error::key(format!("Column '{value_col}' not found")))?;
    let date = match date_col {
        Some(d) => Some(frame.column(d).ok_or_else(|| Error::key(format!("Date column '{d}' not found")))?),
        None => None,
    };
    let labels = match label_col {
        Some(l) => {
            text_cells(frame.column(l).ok_or_else(|| Error::key(format!("Label column '{l}' not found")))?)
        }
        None => (1..=frame.len()).map(|i| Cell::Text(i.to_string())).collect(),
    };
    let numbers = loader::to_numeric(value);
    let dates = date.map(loader::to_datetime);
    let (rule_labels, colors) = if rules.is_empty() {
        (None, None)
    } else {
        let (l, c) = rules::apply(rules, frame, sheet)?;
        (Some(l), Some(c))
    };
    Ok((0..frame.len())
        .map(|i| {
            let raw_date = date.map_or(Cell::Empty, |d| d.cells[i].clone());
            let parsed_date = dates.as_ref().and_then(|d| d[i]);
            let raw_value = value.cells[i].clone();
            let mut reason = "";
            if date.is_some() {
                if parsed_date.is_none() {
                    reason = "Date not readable";
                }
                if raw_date.is_blank() {
                    reason = "Date blank";
                }
            }
            if numbers[i].is_none() {
                reason = "Value not numeric";
            }
            if raw_value.is_blank() {
                reason = "Value blank";
            }
            AuditRow {
                row: frame.index[i],
                label: labels[i].clone(),
                raw_date,
                raw_value,
                date: parsed_date,
                value: numbers[i],
                file: source.to_string(),
                reason: reason.to_string(),
                rule: rule_labels.as_ref().map(|l| l[i].clone()),
                highlight: colors.as_ref().map(|c| c[i].clone()),
            }
        })
        .collect())
}

/// The charted points (sorted by date, stable, when `by_date`) and the number of excluded rows.
pub fn included_series(audit: &[AuditRow], by_date: bool) -> (Vec<Point>, usize) {
    let mut out: Vec<Point> = audit
        .iter()
        .filter(|r| r.included())
        .map(|r| Point {
            date: r.date,
            label: r.label.clone(),
            value: r.value.unwrap_or(f64::NAN),
            row: r.row,
            file: r.file.clone(),
            rule: r.rule.clone(),
            highlight: r.highlight.clone(),
        })
        .collect();
    if by_date {
        out.sort_by_key(|p| p.date);
    }
    let excluded = audit.iter().filter(|r| !r.included()).count();
    (out, excluded)
}

/// Distinct non-empty values in first-seen order, joined for display; None when there are none.
fn joined<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    for v in values.into_iter().flatten() {
        if !v.is_empty() && !seen.contains(&v) {
            seen.push(v);
        }
    }
    if seen.is_empty() { None } else { Some(seen.join(" / ")) }
}

/// A provenance or statistics value: text or a whole number.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Int(i64),
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Text(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Text(s)
    }
}

#[derive(Debug, Clone)]
pub struct ChartJob {
    pub sources: Vec<PathBuf>,
    /// For display; per-source values are in `columns` etc.
    pub column: String,
    pub date_column: Option<String>,
    pub data: Vec<Point>,
    pub result: ImrResult,
    pub excluded: usize,
    pub recent_days: i64,
    pub audit: Vec<AuditRow>,
    pub sheet: Option<String>,
    pub label_column: Option<String>,
    pub sha256s: Vec<Option<String>>,
    pub rules: Vec<Rule>,
    pub columns: Vec<String>,
    pub date_columns: Vec<Option<String>>,
    pub sheets: Vec<Option<String>>,
    /// Whether the dates are held to the microsecond (every source's date column was read as
    /// dates) rather than the nanosecond (dates parsed from text or serial numbers), as pandas
    /// holds them; the HTML chart writes them that way.
    pub dates_in_us: bool,
}

/// An absolute path with symlinks resolved, as Python's `Path.resolve()` gives it.
pub fn resolve(path: &Path) -> PathBuf {
    let p = std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    // Windows: drop the \\?\ prefix canonicalize adds, so the path reads as people write it.
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p,
    }
}

impl ChartJob {
    pub fn source(&self) -> &Path {
        &self.sources[0]
    }

    /// Whether every charted point has a date (the x-axis is then dates, not observation numbers).
    pub fn has_dates(&self) -> bool {
        self.data.iter().all(|p| p.date.is_some())
    }

    /// Charted points whose lot, date and value also appear in another file (e.g. overlapping exports).
    pub fn duplicates(&self) -> usize {
        if self.sources.len() < 2 {
            return 0;
        }
        let date_only = self.data.iter().all(|p| p.date.is_none_or(|d| is_midnight(&d)));
        let keys: Vec<String> = self
            .data
            .iter()
            .map(|p| {
                let label = if p.label.is_empty() { "nan".to_string() } else { p.label.py_str() };
                let date = match p.date {
                    None => "NaT".to_string(),
                    Some(d) if date_only => d.format("%Y-%m-%d").to_string(),
                    Some(d) => py_str_datetime(&d),
                };
                format!("{label}|{date}|{}", py_float_repr(p.value))
            })
            .collect();
        let mut files: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (k, p) in keys.iter().zip(&self.data) {
            files.entry(k).or_default().insert(&p.file);
        }
        keys.iter().filter(|k| files[k.as_str()].len() > 1).count()
    }

    /// Charted points colored by a highlight rule.
    pub fn highlighted(&self) -> usize {
        self.data.iter().filter(|p| p.highlight.as_deref().is_some_and(|h| !h.is_empty())).count()
    }

    /// Whether highlight rules were applied (the audit and data then carry each row's rule).
    pub fn has_rules(&self) -> bool {
        self.audit.first().is_some_and(|r| r.rule.is_some())
    }

    /// What the chart was made from and with, so it can be tied to the exact input. `generated` is
    /// the time to record.
    pub fn provenance(&self, generated: &str) -> Vec<(String, Value)> {
        let mut out: Vec<(String, Value)> = Vec::new();
        let sha = |s: &Option<String>| s.clone().unwrap_or_else(|| "(file not readable)".into());
        if self.sources.len() == 1 {
            out.push(("Source path".into(), resolve(self.source()).display().to_string().into()));
            out.push(("Source SHA-256".into(), sha(&self.sha256s[0]).into()));
        } else {
            out.push(("Source files".into(), Value::Int(self.sources.len() as i64)));
            for (i, (src, s)) in self.sources.iter().zip(&self.sha256s).enumerate() {
                let n = i + 1;
                out.push((format!("Source {n} path"), resolve(src).display().to_string().into()));
                out.push((format!("Source {n} SHA-256"), sha(s).into()));
                if !self.columns.is_empty() {
                    if !loader::is_csv(src) {
                        out.push((
                            format!("Source {n} sheet"),
                            self.sheets[i].clone().unwrap_or_else(|| "(default)".into()).into(),
                        ));
                    }
                    out.push((format!("Source {n} column"), self.columns[i].clone().into()));
                    out.push((
                        format!("Source {n} date column"),
                        self.date_columns[i].clone().unwrap_or_else(|| "(row order)".into()).into(),
                    ));
                }
            }
        }
        let rules = self
            .rules
            .iter()
            .map(|r| format!("{} → {}{}", r.describe(), r.color, if r.enabled { "" } else { " (off)" }))
            .collect::<Vec<_>>()
            .join("; ");
        out.push(("Sheet".into(), self.sheet.clone().unwrap_or_else(|| "(default)".into()).into()));
        out.push((
            "Date column".into(),
            self.date_column.clone().unwrap_or_else(|| "(row order)".into()).into(),
        ));
        out.push((
            "Label column".into(),
            self.label_column.clone().unwrap_or_else(|| "(row number)".into()).into(),
        ));
        out.push(("Highlight rules".into(), if rules.is_empty() { "(none)".into() } else { rules }.into()));
        out.push((
            "Recent highlight (days)".into(),
            if self.recent_days == 0 { "off".into() } else { Value::Int(self.recent_days) },
        ));
        out.push(("Generated".into(), generated.into()));
        out.push(("Program version".into(), format!("imr-chart {}", crate::VERSION).into()));
        out
    }

    /// The output file name without extension: `IMR_<column>`.
    pub fn default_stem(&self) -> String {
        format!("IMR_{}", safe_filename(&self.column))
    }
}

/// The current local time as the provenance records it.
pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string()
}

static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9._-]+").unwrap());

/// Text as a file name: runs of other characters become `_` (`QC3 CD34/Unit` -> `QC3_CD34_Unit`).
pub fn safe_filename(text: &str) -> String {
    let s = UNSAFE.replace_all(text, "_");
    let s = s.trim_matches('_');
    if s.is_empty() { "chart".into() } else { s.to_string() }
}

/// Chart settings shared by every source.
#[derive(Debug, Clone)]
pub struct Settings {
    pub lsl: Option<f64>,
    pub k: f64,
    pub recent_days: i64,
    pub rules: Vec<Rule>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { lsl: None, k: 3.0, recent_days: DEFAULT_RECENT_DAYS, rules: Vec::new() }
    }
}

/// One chart pooling every row of several same-layout files (sorted by date, or kept in file order).
pub fn build_multi_job(
    tables: Vec<(PathBuf, Frame)>,
    column: &str,
    date_column: Option<&str>,
    label_column: Option<&str>,
    sheet: Option<&str>,
    settings: &Settings,
) -> Result<ChartJob> {
    if tables.is_empty() {
        return Err(Error::msg("No input files"));
    }
    let label = label_column.map(str::to_string).or_else(|| loader::pick_label_column(&tables[0].1));
    let sources = tables
        .into_iter()
        .map(|(path, frame)| SourceTable {
            path,
            frame,
            column: column.to_string(),
            date_column: date_column.map(str::to_string),
            sheet: sheet.map(str::to_string),
            label_column: label.clone(),
        })
        .collect();
    let mut job = build_sources_job(sources, settings)?;
    job.sheet = sheet.map(str::to_string);
    Ok(job)
}

/// One chart pooling the rows of several files, each with its own sheet, value column and date column.
///
/// Points are sorted by date across files, or kept in file order when no source uses a date column.
pub fn build_sources_job(sources: Vec<SourceTable>, settings: &Settings) -> Result<ChartJob> {
    if sources.is_empty() {
        return Err(Error::msg("No input files"));
    }
    let dated: Vec<bool> = sources.iter().map(|s| s.date_column.is_some()).collect();
    if dated.iter().any(|d| *d) && !dated.iter().all(|d| *d) {
        return Err(Error::msg("Choose a date column for every workbook, or (row order) for all of them."));
    }
    let paths: Vec<PathBuf> = sources.iter().map(|s| s.path.clone()).collect();
    let mut names: Vec<String> = paths
        .iter()
        .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        .collect();
    if names.iter().collect::<HashSet<_>>().len() < names.len() {
        // The same file name in different folders: keep them apart.
        names = paths.iter().map(|p| p.display().to_string()).collect();
    }
    let labels: Vec<Option<String>> = sources
        .iter()
        .map(|s| s.label_column.clone().or_else(|| loader::pick_label_column(&s.frame)))
        .collect();
    let tables: Vec<(Option<String>, Vec<String>)> =
        sources.iter().map(|s| (s.sheet.clone(), s.frame.names())).collect();
    rules::check_columns(&settings.rules, &tables)?;
    let mut audit = Vec::new();
    for ((name, s), label) in names.iter().zip(&sources).zip(&labels) {
        let rows = audit_rows(
            &s.frame,
            &s.column,
            s.date_column.as_deref(),
            label.as_deref(),
            name,
            &settings.rules,
            s.sheet.as_deref(),
        )
        .map_err(|e| match e {
            Error::Key(m) => Error::Key(format!("{m} in {name}")),
            other => other,
        })?;
        audit.extend(rows);
    }
    let (data, excluded) = included_series(&audit, dated.iter().all(|d| *d));
    let values: Vec<f64> = data.iter().map(|p| p.value).collect();
    let result = compute_imr(&values, settings.k, settings.lsl)?;
    Ok(ChartJob {
        column: joined(sources.iter().map(|s| Some(s.column.as_str()))).unwrap_or_default(),
        date_column: joined(sources.iter().map(|s| s.date_column.as_deref())),
        sheet: joined(sources.iter().filter(|s| !loader::is_csv(&s.path)).map(|s| s.sheet.as_deref())),
        label_column: joined(labels.iter().map(|l| l.as_deref())),
        sha256s: paths.iter().map(file_sha256).collect(),
        rules: settings.rules.clone(),
        columns: sources.iter().map(|s| s.column.clone()).collect(),
        date_columns: sources.iter().map(|s| s.date_column.clone()).collect(),
        sheets: sources.iter().map(|s| s.sheet.clone()).collect(),
        dates_in_us: sources.iter().all(|s| {
            s.date_column
                .as_deref()
                .and_then(|d| s.frame.column(d))
                .is_some_and(|c| c.dtype == crate::cell::DType::Date)
        }),
        sources: paths,
        data,
        result,
        excluded,
        recent_days: settings.recent_days,
        audit,
    })
}
