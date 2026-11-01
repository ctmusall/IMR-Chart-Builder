//! Axis limits and tick positions, computed the way matplotlib does for the chart: autoscaling
//! with 5% margins, `MaxNLocator` for numbers, `AutoDateLocator(minticks=6, maxticks=12)` for
//! dates, and `ScalarFormatter` labels for observation numbers.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

/// Days since 1970-01-01 (matplotlib's date numbers).
pub fn date2num(d: &NaiveDateTime) -> f64 {
    d.and_utc().timestamp_micros() as f64 / 86_400e6
}

/// A date number as a timestamp, to the microsecond.
pub fn num2date(x: f64) -> NaiveDateTime {
    chrono::DateTime::UNIX_EPOCH.naive_utc() + Duration::microseconds((x * 86_400e6).round() as i64)
}

/// Python's float floor division and modulo (`divmod`).
fn py_divmod(x: f64, y: f64) -> (f64, f64) {
    let mut m = x % y;
    let mut div = (x - m) / y;
    if m != 0.0 && ((y < 0.0) != (m < 0.0)) {
        m += y;
        div -= 1.0;
    }
    let mut floordiv;
    if div != 0.0 {
        floordiv = div.floor();
        if div - floordiv > 0.5 {
            floordiv += 1.0;
        }
    } else {
        floordiv = 0f64.copysign(x / y);
    }
    if m == 0.0 {
        m = 0f64.copysign(y);
    }
    (floordiv, m)
}

/// `matplotlib.transforms.nonsingular`.
pub fn nonsingular(vmin: f64, vmax: f64, expander: f64, tiny: f64) -> (f64, f64) {
    if !vmin.is_finite() || !vmax.is_finite() {
        return (-expander, expander);
    }
    let (mut vmin, mut vmax) = if vmax < vmin { (vmax, vmin) } else { (vmin, vmax) };
    let maxabs = vmin.abs().max(vmax.abs());
    if maxabs < (1e6 / tiny) * f64::MIN_POSITIVE {
        vmin = -expander;
        vmax = expander;
    } else if vmax - vmin <= maxabs * tiny {
        if vmax == 0.0 && vmin == 0.0 {
            vmin = -expander;
            vmax = expander;
        } else {
            vmin -= expander * vmin.abs();
            vmax += expander * vmax.abs();
        }
    }
    (vmin, vmax)
}

/// Autoscaled limits for data from `lo` to `hi`: matplotlib's nonsingular check, then 5% margins.
pub fn autoscale(lo: f64, hi: f64, dates: bool) -> (f64, f64) {
    let (lo, hi) = if !lo.is_finite() || !hi.is_finite() {
        if dates { (0.0, 1.0) } else { (-0.05, 0.05) }
    } else if dates {
        if lo == hi { (lo - 365.25 * 2.0, hi + 365.25 * 2.0) } else { (lo, hi) }
    } else {
        nonsingular(lo, hi, 0.05, 1e-15)
    };
    let delta = (hi - lo) * 0.05;
    let (lo, hi) = (lo - delta, hi + delta);
    if dates { (lo, hi) } else { nonsingular(lo, hi, 0.05, 1e-15) }
}

/// `scale_range(vmin, vmax, n)` from matplotlib.ticker.
fn scale_range(vmin: f64, vmax: f64, n: f64) -> (f64, f64) {
    let dv = (vmax - vmin).abs();
    let meanv = (vmax + vmin) / 2.0;
    let offset = if meanv.abs() / dv < 100.0 {
        0.0
    } else {
        10f64.powf(py_divmod(meanv.abs().log10(), 1.0).0).copysign(meanv)
    };
    let scale = 10f64.powf(py_divmod((dv / n).log10(), 1.0).0);
    (scale, offset)
}

struct EdgeInteger {
    step: f64,
    offset: f64,
}

impl EdgeInteger {
    fn closeto(&self, ms: f64, edge: f64) -> bool {
        let tol = if self.offset > 0.0 {
            let digits = (self.offset / self.step).log10();
            (10f64.powf(digits - 12.0)).clamp(1e-10, 0.4999)
        } else {
            1e-10
        };
        (ms - edge).abs() < tol
    }

    fn le(&self, x: f64) -> f64 {
        let (d, m) = py_divmod(x, self.step);
        if self.closeto(m / self.step, 1.0) { d + 1.0 } else { d }
    }

    fn ge(&self, x: f64) -> f64 {
        let (d, m) = py_divmod(x, self.step);
        if self.closeto(m / self.step, 0.0) { d } else { d + 1.0 }
    }
}

/// The number of ticks matplotlib aims for on an axis `length_pt` points long, with 10 pt labels
/// (`get_tick_space`: label size × 3 across, × 2 up), between 1 and 9.
pub fn tick_space(length_pt: f64, horizontal: bool) -> f64 {
    let size = 10.0 * if horizontal { 3.0 } else { 2.0 };
    (length_pt / size).floor().clamp(1.0, 9.0)
}

/// `MaxNLocator(nbins, steps=[1, 2, 2.5, 5, 10]).tick_values(vmin, vmax)` (matplotlib's AutoLocator).
pub fn max_n_ticks(vmin: f64, vmax: f64, nbins: f64) -> Vec<f64> {
    let (vmin, vmax) = nonsingular(vmin, vmax, 1e-13, 1e-14);
    let (scale, offset) = scale_range(vmin, vmax, nbins);
    let (vmin_, vmax_) = (vmin - offset, vmax - offset);
    let steps: Vec<f64> =
        [0.1, 0.2, 0.25, 0.5, 1.0, 2.0, 2.5, 5.0, 10.0, 20.0].iter().map(|s| s * scale).collect();
    let raw_step = (vmax_ - vmin_) / nbins;
    let istep = steps.iter().position(|&s| s >= raw_step).unwrap_or(steps.len() - 1);
    let mut ticks = Vec::new();
    for &step in steps[..=istep].iter().rev() {
        let best_vmin = py_divmod(vmin_, step).0 * step;
        let edge = EdgeInteger { step, offset };
        let low = edge.le(vmin_ - best_vmin);
        let high = edge.ge(vmax_ - best_vmin);
        ticks = Vec::new();
        let mut k = low;
        while k <= high {
            ticks.push(k * step + best_vmin);
            k += 1.0;
        }
        let nticks = ticks.iter().filter(|t| **t <= vmax_ && **t >= vmin_).count();
        if nticks >= 2 {
            break;
        }
    }
    ticks.into_iter().map(|t| t + offset).collect()
}

/// `ScalarFormatter` labels (no offset or scientific notation: observation numbers are small).
pub fn scalar_labels(ticks: &[f64], view: (f64, f64)) -> Vec<String> {
    let mut locs: Vec<f64> = ticks.to_vec();
    if locs.len() < 2 {
        locs.extend([view.0, view.1]);
    }
    let (lo, hi) = locs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    let mut range = hi - lo;
    if range == 0.0 {
        range = locs.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    }
    if range == 0.0 {
        range = 1.0;
    }
    let locs = if ticks.len() < 2 { &locs[..locs.len() - 2] } else { &locs[..] };
    let oom = range.log10().floor() as i32;
    let mut sigfigs = (3 - oom).max(0);
    let thresh = 1e-3 * 10f64.powi(oom);
    while sigfigs >= 0 {
        let p = 10f64.powi(sigfigs);
        let worst = locs.iter().map(|v| (v - (v * p).round_ties_even() / p).abs()).fold(0.0, f64::max);
        if worst < thresh {
            sigfigs -= 1;
        } else {
            break;
        }
    }
    let decimals = (sigfigs + 1) as usize;
    ticks
        .iter()
        .map(|t| {
            let s = format!("{t:.decimals$}");
            match s.strip_prefix('-') {
                Some(rest) if rest.trim_start_matches(['0', '.']).is_empty() => rest.to_string(),
                Some(rest) => format!("\u{2212}{rest}"),
                None => s,
            }
        })
        .collect()
}

// --- dates -------------------------------------------------------------------------

fn add_months(d: NaiveDateTime, months: i32) -> NaiveDateTime {
    let total = d.year() * 12 + d.month0() as i32 + months;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    let last = (28..=31).rev().find(|day| NaiveDate::from_ymd_opt(y, m, *day).is_some()).unwrap_or(28);
    NaiveDate::from_ymd_opt(y, m, d.day().min(last)).expect("valid date").and_time(d.time())
}

/// dateutil's `relativedelta(dmax, dmin)`: whole months, then days, hours, minutes.
fn relativedelta(dmax: NaiveDateTime, dmin: NaiveDateTime) -> (i32, i32, i64, i64, i64) {
    let mut months = (dmax.year() - dmin.year()) * 12 + dmax.month() as i32 - dmin.month() as i32;
    while dmax < add_months(dmin, months) {
        months -= 1;
    }
    let rest = dmax - add_months(dmin, months);
    let secs = rest.num_seconds();
    (
        months.div_euclid(12),
        months.rem_euclid(12),
        secs.div_euclid(86_400),
        secs.rem_euclid(86_400) / 3600,
        secs.rem_euclid(3600) / 60,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Freq {
    Yearly,
    Monthly,
    Daily,
    Hourly,
    Minutely,
    Secondly,
}

/// Every datetime from `start` to `end` (inclusive) whose fields are in the given sets (None: any).
#[allow(clippy::too_many_arguments)]
fn rrule_between(
    freq: Freq,
    start: NaiveDateTime,
    end: NaiveDateTime,
    bymonth: Option<&[u32]>,
    bymonthday: Option<&[u32]>,
    byhour: Option<&[u32]>,
    byminute: Option<&[u32]>,
    bysecond: Option<&[u32]>,
) -> Vec<NaiveDateTime> {
    let ok = |set: Option<&[u32]>, v: u32| set.is_none_or(|s| s.contains(&v));
    let step = match freq {
        Freq::Yearly | Freq::Monthly | Freq::Daily => Duration::days(1),
        Freq::Hourly => Duration::hours(1),
        Freq::Minutely => Duration::minutes(1),
        Freq::Secondly => Duration::seconds(1),
    };
    // Start at the first whole unit at or before `start`.
    let mut t = match freq {
        Freq::Yearly | Freq::Monthly | Freq::Daily => start.date().and_time(NaiveTime::MIN),
        Freq::Hourly => start.date().and_hms_opt(start.hour(), 0, 0).expect("valid time"),
        Freq::Minutely => start.date().and_hms_opt(start.hour(), start.minute(), 0).expect("valid time"),
        Freq::Secondly => start.with_nanosecond(0).expect("valid time"),
    };
    let mut out = Vec::new();
    while t <= end && out.len() < 10_000 {
        if t >= start
            && ok(bymonth, t.month())
            && ok(bymonthday, t.day())
            && ok(byhour, t.hour())
            && ok(byminute, t.minute())
            && ok(bysecond, t.second())
        {
            out.push(t);
        }
        t += step;
    }
    out
}

/// Tick positions for a date axis from `vmin` to `vmax` (date numbers), as
/// `AutoDateLocator(minticks=6, maxticks=12)` places them.
pub fn date_ticks(vmin: f64, vmax: f64) -> Vec<f64> {
    let (dmin, dmax) = (num2date(vmin.min(vmax)), num2date(vmin.max(vmax)));
    let (years, months, _, hours, minutes) = relativedelta(dmax, dmin);
    let tdelta = dmax - dmin;
    let num_years = years as f64;
    let num_months = num_years * 12.0 + months as f64;
    let num_days = tdelta.num_days() as f64;
    let num_hours = num_days * 24.0 + hours as f64;
    let num_minutes = num_hours * 60.0 + minutes as f64;
    let num_seconds = (tdelta.num_microseconds().unwrap_or(i64::MAX) as f64 / 1e6).floor();
    let freqs = [Freq::Yearly, Freq::Monthly, Freq::Daily, Freq::Hourly, Freq::Minutely, Freq::Secondly];
    let nums = [num_years, num_months, num_days, num_hours, num_minutes, num_seconds];
    let intervals: [&[u32]; 6] = [
        &[1, 2, 4, 5, 10, 20, 40, 50, 100, 200, 400, 500, 1000, 2000, 4000, 5000, 10000],
        &[1, 2, 3, 4, 6],
        &[1, 2, 4, 7, 14],
        &[1, 2, 3, 4, 6, 12],
        &[1, 5, 10, 15, 30],
        &[1, 5, 10, 15, 30],
    ];
    let full: [Vec<u32>; 6] = [
        Vec::new(),
        (1..13).collect(),
        (1..32).collect(),
        (0..24).collect(),
        (0..60).collect(),
        (0..60).collect(),
    ];
    // [unused, bymonth, bymonthday, byhour, byminute, bysecond]
    let mut by: [Option<Vec<u32>>; 6] =
        [None, Some(vec![1]), Some(vec![1]), Some(vec![0]), Some(vec![0]), Some(vec![0])];
    let mut chosen = (Freq::Secondly, 1u32);
    let mut found = false;
    for (i, (freq, num)) in freqs.iter().zip(nums).enumerate() {
        if num < 6.0 {
            by[i] = None;
            continue;
        }
        let list = intervals[i];
        let interval =
            *list.iter().find(|iv| num <= **iv as f64 * 11.0).unwrap_or(list.last().expect("intervals"));
        if i > 0 {
            let mut range: Vec<u32> = full[i].iter().copied().step_by(interval as usize).collect();
            // matplotlib compares the index with dateutil's DAILY / WEEKLY constants (3 and 2),
            // so this applies to days (index 2) and hours (index 3).
            if i == 2 || i == 3 {
                if interval == 14 {
                    range = vec![1, 15];
                } else if interval == 7 {
                    range = vec![1, 8, 15, 22];
                }
            }
            by[i] = Some(range);
            chosen = (*freq, 1);
        } else {
            chosen = (*freq, interval);
        }
        found = true;
        break;
    }
    if !found {
        chosen = (Freq::Secondly, 1);
    }
    let ticks: Vec<NaiveDateTime> = if chosen.0 == Freq::Yearly {
        let base = chosen.1 as f64;
        let edge = EdgeInteger { step: base, offset: 0.0 };
        let ymin = (edge.le(dmin.year() as f64) * base).max(1.0) as i32;
        let ymax = (edge.ge(dmax.year() as f64) * base).min(9999.0) as i32;
        (0..)
            .map(|k| ymin + k * chosen.1 as i32)
            .take_while(|y| *y <= ymax)
            .filter_map(|y| NaiveDate::from_ymd_opt(y, 1, 1))
            .map(|d| d.and_time(NaiveTime::MIN))
            .filter(|d| *d >= dmin && *d <= dmax)
            .collect()
    } else {
        rrule_between(
            chosen.0,
            dmin,
            dmax,
            by[1].as_deref(),
            by[2].as_deref(),
            by[3].as_deref(),
            by[4].as_deref(),
            by[5].as_deref(),
        )
    };
    if ticks.is_empty() {
        return vec![vmin, vmax];
    }
    ticks.iter().map(date2num).collect()
}

/// A date number written as `%m/%d/%Y`.
pub fn date_label(x: f64) -> String {
    num2date(x).format("%m/%d/%Y").to_string()
}

/// The ticks that are drawn: those within the view limits.
pub fn visible(ticks: Vec<f64>, view: (f64, f64)) -> Vec<f64> {
    let (lo, hi) = (view.0.min(view.1), view.0.max(view.1));
    let tol = |v: f64| 1e-9 * v.abs().max(1.0);
    ticks.into_iter().filter(|t| *t >= lo - tol(lo) && *t <= hi + tol(hi)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divmod_like_python() {
        assert_eq!(py_divmod(7.0, 2.0), (3.0, 1.0));
        assert_eq!(py_divmod(-7.0, 2.0), (-4.0, 1.0));
        assert_eq!(py_divmod(7.5, -2.0), (-4.0, -0.5));
    }

    #[test]
    fn max_n() {
        let view = (-2.29e6, 2.515e8);
        assert_eq!(
            visible(max_n_ticks(view.0, view.1, 9.0), view),
            (0..=5).map(|i| i as f64 * 5e7).collect::<Vec<_>>()
        );
    }

    #[test]
    fn scalar() {
        assert_eq!(scalar_labels(&[1.0, 1.25, 1.5], (0.9, 3.1)), ["1.00", "1.25", "1.50"]);
        assert_eq!(scalar_labels(&[0.0, 10.0, 20.0], (-2.0, 62.0)), ["0", "10", "20"]);
    }
}
