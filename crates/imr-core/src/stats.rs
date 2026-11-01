//! Individual & Moving Range (I-MR) control chart statistics.
//!
//! Matches JMP's defaults: sigma is estimated from the average moving range
//! (sigma = MRbar / d2, d2 = 1.128 for span 2). Individual limits are
//! mean +/- k*sigma; the moving-range upper limit is MRbar + k*d3*sigma
//! (d3 = 0.8525), which equals D4*MRbar = 3.267*MRbar when k = 3.

use crate::error::{Error, Result};
use crate::pyfmt::py_float;

pub const D2: f64 = 1.128;
pub const D3: f64 = 0.8525;

/// The chart's numbers. `moving_ranges[0]` is NaN (the first point has no moving range).
#[derive(Debug, Clone, PartialEq)]
pub struct ImrResult {
    pub values: Vec<f64>,
    pub moving_ranges: Vec<f64>,
    pub k: f64,
    pub mean: f64,
    pub mr_bar: f64,
    pub sigma: f64,
    pub ucl: f64,
    pub lcl: f64,
    pub mr_ucl: f64,
    pub mr_lcl: f64,
    pub lsl: Option<f64>,
    pub above_ucl: Vec<bool>,
    pub below_lcl: Vec<bool>,
    pub below_lsl: Vec<bool>,
    pub mr_above_ucl: Vec<bool>,
}

impl ImrResult {
    pub fn n(&self) -> usize {
        self.values.len()
    }

    /// Points beyond any limit (individual, LSL or moving range).
    pub fn out_of_limits(&self) -> Vec<bool> {
        (0..self.n())
            .map(|i| self.above_ucl[i] || self.below_lcl[i] || self.below_lsl[i] || self.mr_above_ucl[i])
            .collect()
    }

    pub fn count(flags: &[bool]) -> usize {
        flags.iter().filter(|b| **b).count()
    }
}

/// numpy's pairwise summation of float64 (what `ndarray.sum()` / `mean()` use), so results match
/// to the last bit.
fn pairwise(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = -0.0;
        for &x in a {
            res += x;
        }
        res
    } else if n <= 128 {
        let mut r = [0.0; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - n % 8 {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise(&a[..n2]) + pairwise(&a[n2..])
    }
}

/// `np.sum(a)` of a float64 array.
pub fn np_sum(a: &[f64]) -> f64 {
    if a.is_empty() { 0.0 } else { pairwise(a) }
}

/// Python's built-in `sum()`: left to right, starting from 0.
pub fn py_sum(a: &[f64]) -> f64 {
    a.iter().fold(0.0, |acc, x| acc + x)
}

pub fn compute_imr(values: &[f64], k: f64, lsl: Option<f64>) -> Result<ImrResult> {
    let x = values.to_vec();
    if x.iter().any(|v| v.is_nan()) {
        return Err(Error::msg("values must not contain NaN"));
    }
    if x.len() < 2 {
        return Err(Error::msg("At least 2 data points are required for an I-MR chart"));
    }
    if k <= 0.0 || k.is_nan() {
        return Err(Error::msg("Sigma multiplier must be positive"));
    }
    let n = x.len();
    let mut mr = vec![f64::NAN; n];
    for i in 1..n {
        mr[i] = (x[i] - x[i - 1]).abs();
    }
    let mean = np_sum(&x) / n as f64;
    // np.nanmean: NaN counted as 0 in the sum, and left out of the count.
    let with_zero: Vec<f64> = mr.iter().map(|v| if v.is_nan() { 0.0 } else { *v }).collect();
    let mr_bar = np_sum(&with_zero) / (n - 1) as f64;
    let sigma = mr_bar / D2;
    let ucl = mean + k * sigma;
    let lcl = mean - k * sigma;
    let mr_ucl = mr_bar + k * D3 * sigma;
    let mr_lcl = 0f64.max(mr_bar - k * D3 * sigma);
    Ok(ImrResult {
        above_ucl: x.iter().map(|v| *v > ucl).collect(),
        below_lcl: x.iter().map(|v| *v < lcl).collect(),
        below_lsl: x.iter().map(|v| lsl.is_some_and(|l| *v < l)).collect(),
        mr_above_ucl: mr.iter().map(|v| !v.is_nan() && *v > mr_ucl).collect(),
        values: x,
        moving_ranges: mr,
        k,
        mean,
        mr_bar,
        sigma,
        ucl,
        lcl,
        mr_ucl,
        mr_lcl,
        lsl,
    })
}

/// Parse `2e7`, `20,000,000`, `50%` (-> 0.5). Blank -> None.
pub fn parse_limit(text: &str) -> Result<Option<f64>> {
    let s = text.trim().replace(',', "");
    if s.is_empty() {
        return Ok(None);
    }
    let parsed = match s.strip_suffix('%') {
        Some(n) => py_float(n).map(|f| f / 100.0),
        None => py_float(&s),
    };
    match parsed {
        Some(f) => Ok(Some(f)),
        None => Err(Error::msg(format!("Invalid limit value: '{text}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_computed() {
        let r = compute_imr(&[10.0, 12.0, 11.0, 15.0, 9.0], 3.0, None).unwrap();
        assert_eq!(r.mean, 11.4);
        assert_eq!(r.mr_bar, 3.25);
        assert!((r.mr_ucl - 3.25 * (1.0 + 3.0 * D3 / D2)).abs() < 1e-12);
    }

    #[test]
    fn limits() {
        assert_eq!(parse_limit("2e7").unwrap(), Some(2e7));
        assert_eq!(parse_limit("50%").unwrap(), Some(0.5));
        assert_eq!(parse_limit(" ").unwrap(), None);
        assert!(parse_limit("abc").is_err());
    }
}
