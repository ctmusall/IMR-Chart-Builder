//! What both chart outputs (the static image and the interactive HTML) show besides the numbers:
//! the recent-days window, highlight-rule groups, and how values and limits are written.

use chrono::{Duration, NaiveDateTime};

use crate::pyfmt::{py_format_e, py_format_f, py_format_g, py_format_g_sep};
use crate::report::{ChartJob, Point};
use crate::stats::ImrResult;

pub const POINT_COLOR: &str = "#111111";
pub const CONNECTOR_COLOR: &str = "#9a9a9a";
pub const MEAN_COLOR: &str = "#2e9e3e";
pub const LIMIT_COLOR: &str = "#d62728";
pub const LSL_COLOR: &str = "#3a5fcd";
pub const FLAG_COLOR: &str = "#d62728";
pub const RECENT_COLOR: &str = "#ff7f0e";
pub const RECENT_BAND_COLOR: &str = "#fff1e0";

/// Points within `days` of the latest date, or None when there is no window (no days, or a point
/// without a date).
pub fn recent_mask(data: &[Point], days: i64) -> Option<Vec<bool>> {
    if days == 0 || data.is_empty() || data.iter().any(|p| p.date.is_none()) {
        return None;
    }
    let latest = data.iter().filter_map(|p| p.date).max()?;
    let start = latest - Duration::days(days);
    Some(data.iter().map(|p| p.date.is_some_and(|d| d >= start)).collect())
}

/// The recent window as drawn: None when it holds no point.
pub fn recent_window(job: &ChartJob) -> Option<Vec<bool>> {
    recent_mask(&job.data, job.recent_days).filter(|m| m.iter().any(|b| *b))
}

/// The first date in the recent window.
pub fn recent_start(data: &[Point], mask: &[bool]) -> Option<NaiveDateTime> {
    data.iter().zip(mask).filter(|(_, m)| **m).filter_map(|(p, _)| p.date).min()
}

/// One highlight rule's points.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub label: String,
    pub color: String,
    pub mask: Vec<bool>,
}

impl Group {
    pub fn count(&self) -> usize {
        self.mask.iter().filter(|b| **b).count()
    }
}

/// `(rule label, color, point mask)` for each highlight rule that matched a charted point.
pub fn highlight_groups(data: &[Point]) -> Vec<Group> {
    if data.first().is_none_or(|p| p.highlight.is_none()) {
        return Vec::new();
    }
    let pair = |p: &Point| (p.rule.clone().unwrap_or_default(), p.highlight.clone().unwrap_or_default());
    let mut seen: Vec<(String, String)> = Vec::new();
    for p in data {
        let key = pair(p);
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
    seen.into_iter()
        .filter(|(_, color)| !color.is_empty())
        .map(|(label, color)| {
            let mask = data.iter().map(|p| pair(p) == (label.clone(), color.clone())).collect();
            Group { label, color, mask }
        })
        .collect()
}

/// True when the data look like proportions (e.g. viability 0.89 = 89%).
pub fn looks_like_fraction(values: &[f64]) -> bool {
    let finite: Vec<f64> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    if finite.is_empty() {
        return false; // empty, or nanmin of all-NaN (NaN compares false)
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    min >= 0.0 && max <= 1.5
}

/// A value as the chart writes it: `6.01e+7`, `1,235`, `89%`.
pub fn format_number(x: f64, percent: bool) -> String {
    if percent {
        return format!("{}%", py_format_f(x * 100.0, 0));
    }
    if x != 0.0 && (x.abs() >= 1e5 || x.abs() < 1e-3) {
        let s = py_format_e(x, 2);
        let (mant, exp) = s.split_once('e').expect("exponent");
        let exp: i32 = exp.parse().expect("exponent number");
        return format!("{mant}e{exp:+}");
    }
    py_format_g_sep(x, 4, true)
}

/// The LSL line's label: `LSL (2e7)`, `LSL (50%)`, `LSL (12.5)`.
pub fn lsl_label(lsl: f64, percent: bool) -> String {
    if percent {
        return format!("LSL ({}%)", py_format_g(lsl * 100.0, 6));
    }
    if lsl != 0.0 && (lsl.abs() >= 1e5 || lsl.abs() < 1e-3) {
        return format!("LSL ({})", py_format_e(lsl, 0)).replace("e+0", "e").replace("e+", "e");
    }
    format!("LSL ({})", py_format_g(lsl, 6))
}

/// Whether to draw the individuals LCL. Like JMP's view of the CD34/Unit chart: a negative LCL on
/// data that can never go negative carries no information, so it is not drawn.
pub fn show_lcl(result: &ImrResult) -> bool {
    let min = result.values.iter().copied().fold(f64::INFINITY, f64::min);
    !(result.lcl < 0.0 && 0.0 <= min)
}
