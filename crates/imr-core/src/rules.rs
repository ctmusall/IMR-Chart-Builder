//! Highlight rules: color chart points whose source row matches a condition.
//!
//! A rule tests one column of the source row (any column, not only the charted one), e.g.
//! `Notes contains "retest"` or `QC3 CD34 Viability < 50%`. Rules form an ordered list; the first
//! enabled rule a row matches decides its color. A rule may name a sheet; it then tests only rows
//! read from a sheet of that name.

use std::cmp::Ordering;
use std::path::Path;

use serde_json::{Map, Value};

use crate::cell::{Cell, Column, Frame};
use crate::colors::normalize_color;
use crate::dates;
use crate::error::{Error, Result};
use crate::loader;
use crate::pyfmt::{casefold, json_ensure_ascii, py_float_repr};
use crate::stats::parse_limit;

pub const OPERATORS: &[&str] = &[
    "=",
    "<>",
    "<",
    "<=",
    ">",
    ">=",
    "contains",
    "not contains",
    "starts with",
    "ends with",
    "is blank",
    "is not blank",
];
pub const NO_VALUE_OPERATORS: &[&str] = &["is blank", "is not blank"];
pub const DEFAULT_COLOR: &str = "#9467bd";

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: String,
    pub column: String,
    pub op: String,
    pub value: String,
    /// Lower-case `#rrggbb`.
    pub color: String,
    pub enabled: bool,
    /// "" = rows from any sheet.
    pub sheet: String,
}

impl Rule {
    /// A checked rule: a known operator, a column, and a color (normalized to `#rrggbb`).
    pub fn new(
        name: impl Into<String>,
        column: impl Into<String>,
        op: impl Into<String>,
        value: impl Into<String>,
        color: &str,
        enabled: bool,
        sheet: impl Into<String>,
    ) -> Result<Rule> {
        let mut rule = Rule {
            name: name.into(),
            column: column.into(),
            op: op.into(),
            value: value.into(),
            color: color.to_string(),
            enabled,
            sheet: sheet.into().trim().to_string(),
        };
        if !OPERATORS.contains(&rule.op.as_str()) {
            return Err(Error::msg(format!(
                "Rule '{}': unknown operator '{}' (use one of {})",
                rule.label(),
                rule.op,
                OPERATORS.join(", ")
            )));
        }
        if rule.column.is_empty() {
            return Err(Error::msg(format!("Rule '{}': no column chosen", rule.label())));
        }
        match normalize_color(color) {
            Some(hex) => rule.color = hex,
            None => return Err(Error::msg(format!("Rule '{}': '{color}' is not a color", rule.label()))),
        }
        Ok(rule)
    }

    /// A rule with the default color, enabled, for any sheet.
    pub fn simple(name: &str, column: &str, op: &str, value: &str) -> Result<Rule> {
        Rule::new(name, column, op, value, DEFAULT_COLOR, true, "")
    }

    pub fn label(&self) -> String {
        if self.name.is_empty() { self.describe() } else { self.name.clone() }
    }

    pub fn describe(&self) -> String {
        let place = if self.sheet.is_empty() { String::new() } else { format!("{} › ", self.sheet) };
        if NO_VALUE_OPERATORS.contains(&self.op.as_str()) {
            format!("{place}{} {}", self.column, self.op)
        } else {
            format!("{place}{} {} '{}'", self.column, self.op, self.value)
        }
    }

    /// Whether this rule tests rows read from `sheet`.
    pub fn applies_to(&self, sheet: Option<&str>) -> bool {
        self.sheet.is_empty() || Some(self.sheet.as_str()) == sheet
    }

    /// One flag per row of `frame`; cells that can't be compared never match.
    pub fn matches(&self, frame: &Frame) -> Result<Vec<bool>> {
        let Some(col) = frame.column(&self.column) else {
            return Err(Error::key(format!("Rule '{}': column '{}' not found", self.label(), self.column)));
        };
        let blank: Vec<bool> = col.cells.iter().map(Cell::is_blank).collect();
        match self.op.as_str() {
            "is blank" => return Ok(blank),
            "is not blank" => return Ok(blank.iter().map(|b| !b).collect()),
            "=" | "<>" | "<" | "<=" | ">" | ">=" => return self.compare(col, &blank),
            _ => {}
        }
        let text = folded_text(col, &blank);
        let needle = casefold(self.value.trim());
        Ok(text
            .iter()
            .map(|t| match self.op.as_str() {
                "contains" => t.contains(&needle),
                "not contains" => !t.contains(&needle),
                "starts with" => t.starts_with(&needle),
                _ => t.ends_with(&needle),
            })
            .collect())
    }

    fn compare(&self, col: &Column, blank: &[bool]) -> Result<Vec<bool>> {
        let test = |ord: Option<Ordering>| -> bool {
            let Some(ord) = ord else { return self.op == "<>" };
            match self.op.as_str() {
                "=" => ord == Ordering::Equal,
                "<>" => ord != Ordering::Equal,
                "<" => ord == Ordering::Less,
                "<=" => ord != Ordering::Greater,
                ">" => ord == Ordering::Greater,
                _ => ord != Ordering::Less,
            }
        };
        if let Ok(Some(number)) = parse_limit(&self.value) {
            // A blank cell never matches; NaN compares unequal to everything (so `<>` holds).
            return Ok(loader::to_numeric(col)
                .iter()
                .map(|v| v.is_some_and(|v| test(v.partial_cmp(&number))))
                .collect());
        }
        let when = if self.value.trim().is_empty() { None } else { dates::parse_datetime(&self.value) };
        if let Some(when) = when {
            return Ok(loader::to_datetime(col)
                .iter()
                .map(|d| d.is_some_and(|d| test(Some(d.cmp(&when)))))
                .collect());
        }
        if self.op != "=" && self.op != "<>" {
            return Err(Error::msg(format!(
                "Rule '{}': '{}' needs a number or date, not '{}'",
                self.label(),
                self.op,
                self.value
            )));
        }
        let needle = casefold(self.value.trim());
        Ok(folded_text(col, blank).iter().map(|t| test(Some(t.as_str().cmp(needle.as_str())))).collect())
    }
}

/// Each cell as stripped, case-folded text; "" for blank cells.
fn folded_text(col: &Column, blank: &[bool]) -> Vec<String> {
    col.as_text()
        .iter()
        .zip(blank)
        .map(|(t, b)| if *b { String::new() } else { casefold(t.trim()) })
        .collect()
}

/// Index into `rules` of the first enabled rule each row matches; -1 for none.
///
/// `sheet` names the sheet `frame` was read from: rules for another sheet are skipped. So is an
/// any-sheet rule whose column `frame` lacks, when other sheets are involved (see
/// [`check_columns`]); without `sheet` a missing column is an error.
pub fn evaluate(rules: &[Rule], frame: &Frame, sheet: Option<&str>) -> Result<Vec<i64>> {
    let mut out = vec![-1i64; frame.len()];
    for (i, rule) in rules.iter().enumerate() {
        if !rule.enabled || !rule.applies_to(sheet) {
            continue;
        }
        if sheet.is_some() && rule.sheet.is_empty() && frame.column(&rule.column).is_none() {
            continue;
        }
        let hits = rule.matches(frame)?;
        for (o, hit) in out.iter_mut().zip(hits) {
            if hit && *o == -1 {
                *o = i as i64;
            }
        }
    }
    Ok(out)
}

/// Per row of `frame`: the matching rule's label and color ("" when none).
pub fn apply(rules: &[Rule], frame: &Frame, sheet: Option<&str>) -> Result<(Vec<String>, Vec<String>)> {
    let idx = evaluate(rules, frame, sheet)?;
    let labels = idx.iter().map(|&i| if i < 0 { String::new() } else { rules[i as usize].label() }).collect();
    let colors =
        idx.iter().map(|&i| if i < 0 { String::new() } else { rules[i as usize].color.clone() }).collect();
    Ok((labels, colors))
}

/// A Key error for an enabled rule whose column is in none of the `(sheet, columns)` it applies to.
///
/// A rule for a sheet that isn't loaded matches nothing and is not an error.
pub fn check_columns(rules: &[Rule], tables: &[(Option<String>, Vec<String>)]) -> Result<()> {
    for rule in rules.iter().filter(|r| r.enabled) {
        let targets: Vec<&Vec<String>> =
            tables.iter().filter(|(sheet, _)| rule.applies_to(sheet.as_deref())).map(|(_, c)| c).collect();
        if !targets.is_empty() && !targets.iter().any(|cols| cols.contains(&rule.column)) {
            let place =
                if rule.sheet.is_empty() { String::new() } else { format!(" in sheet '{}'", rule.sheet) };
            return Err(Error::key(format!(
                "Rule '{}': column '{}' not found{place}",
                rule.label(),
                rule.column
            )));
        }
    }
    Ok(())
}

/// Python `str()` of a JSON value, as a rule's value or sheet reads it.
fn json_str(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) if n.is_f64() => py_float_repr(n.as_f64().unwrap_or(f64::NAN)),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Read rules saved by [`save_rules`] (or written by hand: `{"rules": [...]}` or a bare list).
pub fn load_rules(path: impl AsRef<Path>) -> Result<Vec<Rule>> {
    let path = path.as_ref();
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::msg(format!("Rules file not found: {}", path.display())));
        }
        Err(e) => return Err(Error::msg(format!("{}: {e}", path.display()))),
    };
    let doc: Value = serde_json::from_str(&text)
        .map_err(|e| Error::msg(format!("Rules file {} is not valid JSON: {e}", path.display())))?;
    let items = match &doc {
        Value::Object(o) => o.get("rules"),
        other => Some(other),
    };
    let Some(Value::Array(items)) = items else {
        return Err(Error::msg(format!("Rules file {} must hold a list under \"rules\"", path.display())));
    };
    items
        .iter()
        .map(|item| {
            let Value::Object(o) = item else {
                return Err(Error::msg(format!(
                    "Rules file {}: each rule must be an object",
                    path.display()
                )));
            };
            let (Some(column), Some(op)) = (o.get("column"), o.get("op")) else {
                return Err(Error::msg(format!(
                    "Rules file {}: each rule needs a \"column\" and an \"op\"",
                    path.display()
                )));
            };
            let color = o.get("color").map_or(DEFAULT_COLOR.to_string(), json_str);
            Rule::new(
                o.get("name").map(json_str).unwrap_or_default(),
                json_str(column),
                json_str(op),
                o.get("value").map(json_str).unwrap_or_default(),
                &color,
                o.get("enabled").is_none_or(truthy),
                o.get("sheet").map(json_str).unwrap_or_default(),
            )
        })
        .collect()
}

/// The rules as JSON, exactly as the Python version wrote them (`json.dumps(..., indent=2)`).
pub fn rules_json(rules: &[Rule]) -> String {
    let list: Vec<Value> = rules
        .iter()
        .map(|r| {
            let mut m = Map::new();
            m.insert("name".into(), r.name.clone().into());
            m.insert("column".into(), r.column.clone().into());
            m.insert("op".into(), r.op.clone().into());
            m.insert("value".into(), r.value.clone().into());
            m.insert("color".into(), r.color.clone().into());
            m.insert("enabled".into(), r.enabled.into());
            m.insert("sheet".into(), r.sheet.clone().into());
            Value::Object(m)
        })
        .collect();
    let mut doc = Map::new();
    doc.insert("rules".into(), Value::Array(list));
    let text = serde_json::to_string_pretty(&Value::Object(doc)).expect("JSON");
    json_ensure_ascii(&text) + "\n"
}

pub fn save_rules(rules: &[Rule], path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    std::fs::write(path, rules_json(rules)).map_err(|e| Error::msg(format!("{}: {e}", path.display())))
}
