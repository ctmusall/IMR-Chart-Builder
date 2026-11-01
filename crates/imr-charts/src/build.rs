//! What the chart shows, built from a chart job the way the Python version built its matplotlib
//! figure (`plot_static.build_figure`).

use imr_core::ChartJob;
use imr_core::plot::{
    CONNECTOR_COLOR, FLAG_COLOR, LIMIT_COLOR, LSL_COLOR, MEAN_COLOR, POINT_COLOR, RECENT_BAND_COLOR,
    RECENT_COLOR, format_number, highlight_groups, looks_like_fraction, lsl_label, recent_window, show_lcl,
};
use imr_core::pyfmt::py_format_g;

use crate::figure::{
    Annotation, Band, Coord, Figure, FigureText, HAlign, HLine, Legend, Line, Panel, Scatter, VAlign,
};
use crate::font::{LINE_HEIGHT, text_width};
use crate::ticks::{
    autoscale, date_label, date_ticks, date2num, max_n_ticks, scalar_labels, tick_space, visible,
};

/// The figure size in inches, and the resolution its layout is measured at.
pub const FIGSIZE: (f64, f64) = (11.0, 8.5);
pub const DPI: f64 = 100.0;
const NOTE_COLOR: &str = "#444444";

fn pick<T: Copy>(values: &[T], mask: &[bool]) -> Vec<T> {
    values.iter().zip(mask).filter(|(_, m)| **m).map(|(v, _)| *v).collect()
}

fn scatter(x: &[f64], y: &[f64], size: f64, color: &str, zorder: f64, label: Option<String>) -> Scatter {
    Scatter {
        x: x.to_vec(),
        y: y.to_vec(),
        face: Some(color.into()),
        edge: color.into(),
        size,
        width: 0.0,
        zorder,
        label,
    }
}

fn ring(x: &[f64], y: &[f64], label: Option<String>) -> Scatter {
    Scatter {
        x: x.to_vec(),
        y: y.to_vec(),
        face: None,
        edge: FLAG_COLOR.into(),
        size: 18.0,
        width: 0.9,
        zorder: 4.0,
        label,
    }
}

fn hline(y: f64, color: &str, width: f64) -> HLine {
    HLine { y, color: color.into(), width, zorder: 2.0 }
}

/// A limit's value just right of the panel.
fn tag(y: f64, text: String, color: &str) -> Annotation {
    Annotation {
        text,
        x: Coord::Axes(1.0),
        y: Coord::Data(y),
        offset: (4.0, 0.0),
        ha: HAlign::Left,
        va: VAlign::Center,
        color: color.into(),
        size: 8.0,
        bold: false,
        boxed: false,
        zorder: 3.0,
    }
}

/// One y axis's autoscaling, the lazy way matplotlib does it: data marks it stale; a horizontal
/// line first brings the view up to date, and marks it stale again only when the line falls
/// outside the view; the view is recomputed from all data (lines included) only while stale.
struct YScale {
    data: (f64, f64),
    view: (f64, f64),
    stale: bool,
    auto: bool,
}

impl YScale {
    fn new() -> YScale {
        YScale { data: (f64::INFINITY, f64::NEG_INFINITY), view: (0.0, 1.0), stale: false, auto: true }
    }

    fn include(&mut self, values: &[f64]) {
        let (lo, hi) = range(values.iter().copied());
        self.data = (self.data.0.min(lo), self.data.1.max(hi));
    }

    /// Points or a polyline (they always ask for a new view).
    fn add(&mut self, values: &[f64]) {
        self.include(values);
        self.stale = true;
    }

    fn view(&mut self) -> (f64, f64) {
        if self.stale && self.auto {
            self.view = autoscale(self.data.0, self.data.1, false);
            self.stale = false;
        }
        self.view
    }

    fn axhline(&mut self, y: f64) {
        let (lo, hi) = self.view();
        self.include(&[y]);
        if y < lo || y > hi {
            self.stale = true;
        }
    }

    /// `set_ylim(bottom=...)`: the top stays where the view is, and autoscaling stops.
    fn set_bottom(&mut self, bottom: f64) {
        let top = self.view().1;
        self.view = (bottom, top);
        self.auto = false;
    }
}

/// The finite range of some values.
fn range(values: impl IntoIterator<Item = f64>) -> (f64, f64) {
    values
        .into_iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
}

/// The two panels' positions: `subplots_adjust` margins, heights 3:2, a gap of 0.06 of a panel.
fn positions(bottom: f64) -> [[f64; 4]; 2] {
    let (left, right, top) = (0.12, 0.87, 0.93);
    let hspace = 0.06;
    let tot_h = top - bottom;
    let cell_h = tot_h / (2.0 + hspace);
    let sep = hspace * cell_h;
    let norm = cell_h * 2.0 / 5.0;
    let (h1, h2) = (3.0 * norm, 2.0 * norm);
    let top1 = top;
    let bottom1 = top1 - h1;
    let top2 = bottom1 - sep;
    let bottom2 = top2 - h2;
    [[left, bottom1, right - left, top1 - bottom1], [left, bottom2, right - left, top2 - bottom2]]
}

/// Build the two-panel I-MR figure.
pub fn figure(job: &ChartJob) -> Figure {
    let r = &job.result;
    let n = r.n();
    let percent = looks_like_fraction(&r.values);
    let has_dates = job.has_dates();
    let x: Vec<f64> = if has_dates {
        job.data.iter().map(|p| date2num(&p.date.expect("dated"))).collect()
    } else {
        (1..=n).map(|i| i as f64).collect()
    };
    let recent = recent_window(job);
    let mr = &r.moving_ranges;
    let flagged: Vec<bool> = (0..n).map(|i| r.above_ucl[i] || r.below_lcl[i]).collect();
    let show = show_lcl(r);
    let fmt = |v: f64| format_number(v, percent);
    let bottom = if has_dates { 0.17 } else { 0.12 };
    let [pos_i, pos_mr] = positions(bottom);

    // Individuals.
    let mut ax_i = Panel {
        position: pos_i,
        xlim: (0.0, 1.0),
        ylim: (0.0, 1.0),
        xticks: Vec::new(),
        yticks: Vec::new(),
        xlabel: String::new(),
        ylabel: job.column.clone(),
        show_xtick_labels: false,
        xrotation: 0.0,
        dates: has_dates,
        lines: vec![Line {
            x: x.clone(),
            y: r.values.clone(),
            color: CONNECTOR_COLOR.into(),
            width: 0.6,
            zorder: 1.0,
        }],
        hlines: Vec::new(),
        scatters: vec![scatter(&x, &r.values, 10.0, POINT_COLOR, 3.0, None)],
        bands: Vec::new(),
        texts: Vec::new(),
        legend: None,
    };
    let mut y_i = YScale::new();
    y_i.add(&r.values);
    if flagged.iter().any(|f| *f) {
        ax_i.scatters.push(ring(
            &pick(&x, &flagged),
            &pick(&r.values, &flagged),
            Some("Beyond limit".into()),
        ));
    }
    ax_i.hlines.push(hline(r.mean, MEAN_COLOR, 1.6));
    y_i.axhline(r.mean);
    ax_i.hlines.push(hline(r.ucl, LIMIT_COLOR, 1.4));
    y_i.axhline(r.ucl);
    if show {
        ax_i.hlines.push(hline(r.lcl, LIMIT_COLOR, 1.4));
        y_i.axhline(r.lcl);
    }
    if let Some(lsl) = r.lsl {
        ax_i.hlines.push(hline(lsl, LSL_COLOR, 1.0));
        y_i.axhline(lsl);
        ax_i.texts.push(Annotation {
            text: lsl_label(lsl, percent),
            x: Coord::Axes(0.0),
            y: Coord::Data(lsl),
            offset: (4.0, 2.0),
            ha: HAlign::Left,
            va: VAlign::Bottom,
            color: LSL_COLOR.into(),
            size: 9.0,
            bold: true,
            boxed: true,
            zorder: 5.0,
        });
    }
    // Without the LCL line, the axis starts just below the data (or zero).
    if !show {
        let low = r
            .values
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .min(r.lsl.unwrap_or(f64::INFINITY))
            .min(0.0);
        y_i.set_bottom(low - 0.03 * (r.ucl - low));
    }

    // Moving range.
    let mut ax_mr = Panel {
        position: pos_mr,
        ylabel: format!("Moving Range({})", job.column),
        show_xtick_labels: true,
        lines: vec![Line {
            x: x.clone(),
            y: mr.clone(),
            color: CONNECTOR_COLOR.into(),
            width: 0.6,
            zorder: 1.0,
        }],
        scatters: vec![scatter(&x, mr, 10.0, POINT_COLOR, 3.0, None)],
        ..ax_i.clone()
    };
    ax_mr.hlines = Vec::new();
    ax_mr.texts = Vec::new();
    let mut y_mr = YScale::new();
    y_mr.add(mr);
    if r.mr_above_ucl.iter().any(|f| *f) {
        ax_mr.scatters.push(ring(&pick(&x, &r.mr_above_ucl), &pick(mr, &r.mr_above_ucl), None));
    }
    for (y, color, width) in
        [(r.mr_bar, MEAN_COLOR, 1.6), (r.mr_ucl, LIMIT_COLOR, 1.4), (r.mr_lcl, LIMIT_COLOR, 1.4)]
    {
        ax_mr.hlines.push(hline(y, color, width));
        y_mr.axhline(y);
    }

    // Recent window: a shaded band plus recolored points (beyond-limit rings stay on top).
    if let Some(mask) = &recent {
        let start = pick(&x, mask).into_iter().fold(f64::INFINITY, f64::min);
        let end = *x.last().expect("points");
        for (ax, y, scale) in [(&mut ax_i, &r.values, &mut y_i), (&mut ax_mr, mr, &mut y_mr)] {
            ax.bands.push(Band { x0: start, x1: end, color: RECENT_BAND_COLOR.into(), zorder: 0.0 });
            ax.scatters.push(scatter(&pick(&x, mask), &pick(y, mask), 12.0, RECENT_COLOR, 3.5, None));
            scale.add(&pick(y, mask));
        }
        ax_i.texts.push(Annotation {
            text: format!("Last {} days", job.recent_days),
            x: Coord::Data(start),
            y: Coord::Axes(1.0),
            offset: (2.0, -2.0),
            ha: HAlign::Left,
            va: VAlign::Top,
            color: RECENT_COLOR.into(),
            size: 8.0,
            bold: true,
            boxed: false,
            zorder: 5.0,
        });
    }

    // Highlight rules: recolor matched points (beyond-limit rings stay on top).
    let groups = highlight_groups(&job.data);
    for g in &groups {
        let label = format!("{} ({})", g.label, g.count());
        ax_i.scatters.push(scatter(
            &pick(&x, &g.mask),
            &pick(&r.values, &g.mask),
            14.0,
            &g.color,
            3.7,
            Some(label),
        ));
        ax_mr.scatters.push(scatter(&pick(&x, &g.mask), &pick(mr, &g.mask), 14.0, &g.color, 3.7, None));
        y_i.add(&pick(&r.values, &g.mask));
        y_mr.add(&pick(mr, &g.mask));
    }
    ax_i.ylim = y_i.view();
    ax_mr.ylim = y_mr.view();

    // The shared x axis.
    let xlim = autoscale(range(x.iter().copied()).0, range(x.iter().copied()).1, has_dates);
    let width_pt = pos_mr[2] * FIGSIZE.0 * 72.0;
    let xticks: Vec<(f64, String)> = if has_dates {
        visible(date_ticks(xlim.0, xlim.1), xlim).into_iter().map(|t| (t, date_label(t))).collect()
    } else {
        let t = visible(max_n_ticks(xlim.0, xlim.1, tick_space(width_pt, true)), xlim);
        let labels = scalar_labels(&t, xlim);
        t.into_iter().zip(labels).collect()
    };
    for ax in [&mut ax_i, &mut ax_mr] {
        ax.xlim = xlim;
        ax.xticks = xticks.clone();
        let height_pt = ax.position[3] * FIGSIZE.1 * 72.0;
        ax.yticks = visible(max_n_ticks(ax.ylim.0, ax.ylim.1, tick_space(height_pt, false)), ax.ylim)
            .into_iter()
            .map(|t| (t, fmt(t)))
            .collect();
    }
    if has_dates {
        ax_mr.xrotation = 90.0;
        ax_mr.xlabel = job.date_column.clone().unwrap_or_else(|| "Observation".into());
    } else {
        ax_mr.xlabel = "Observation".into();
    }

    // Limit values on the right edge, so readers can see the numbers.
    ax_i.texts.push(tag(r.ucl, format!("UCL={}", fmt(r.ucl)), LIMIT_COLOR));
    ax_i.texts.push(tag(r.mean, format!("Avg={}", fmt(r.mean)), MEAN_COLOR));
    if show {
        ax_i.texts.push(tag(r.lcl, format!("LCL={}", fmt(r.lcl)), LIMIT_COLOR));
    }
    ax_mr.texts.push(tag(r.mr_ucl, format!("UCL={}", fmt(r.mr_ucl)), LIMIT_COLOR));
    ax_mr.texts.push(tag(r.mr_bar, format!("Avg={}", fmt(r.mr_bar)), MEAN_COLOR));

    if !groups.is_empty() {
        let entries: Vec<(String, String)> = ax_i
            .scatters
            .iter()
            .filter(|s| s.zorder == 3.7)
            .filter_map(|s| s.label.clone().map(|l| (l, s.edge.clone())))
            .collect();
        ax_i.legend = Some(Legend { bbox: best_legend(&ax_i, &entries), entries, size: 8.0 });
    }

    let count = |f: &[bool]| f.iter().filter(|b| **b).count();
    let mut note = format!(
        "n={n}   Limits: mean ± {}σ,  σ = MR̄/1.128 = {}   |   Beyond limits: {} individual, {} MR",
        py_format_g(r.k, 6),
        fmt(r.sigma),
        count(&flagged),
        count(&r.mr_above_ucl)
    );
    if r.lsl.is_some() {
        note += &format!(",  {} below LSL", count(&r.below_lsl));
    }
    if job.sources.len() > 1 {
        note += &format!("   |   Data from {} files", job.sources.len());
    }
    if !groups.is_empty() {
        note += &format!("   |   Highlighted by rules: {}", groups.iter().map(|g| g.count()).sum::<usize>());
    }
    if let Some(mask) = &recent {
        let start = pick(&x, mask).into_iter().fold(f64::INFINITY, f64::min);
        note += &format!("   |   Highlighted: last {} days (from {})", job.recent_days, date_label(start));
    }
    if job.excluded > 0 {
        note += &format!("\n{} rows excluded (blank or non-numeric).", job.excluded);
    }
    Figure {
        size: FIGSIZE,
        title: FigureText {
            text: format!("Individual & Moving Range chart of {}", job.column),
            x: 0.5,
            y: 0.98,
            ha: HAlign::Center,
            va: VAlign::Top,
            size: 14.0,
            bold: true,
            color: "#000000".into(),
        },
        note: FigureText {
            text: note,
            x: 0.01,
            y: 0.005,
            ha: HAlign::Left,
            va: VAlign::Bottom,
            size: 8.0,
            bold: false,
            color: NOTE_COLOR.into(),
        },
        panels: vec![ax_i, ax_mr],
    }
}

// --- the legend's "best" place (matplotlib's Legend._find_best_position) -----------------

/// A box in display pixels, y up.
#[derive(Debug, Clone, Copy)]
struct Bbox {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Bbox {
    fn contains_strict(&self, x: f64, y: f64) -> bool {
        self.x0 < x && x < self.x1 && self.y0 < y && y < self.y1
    }

    fn overlaps(&self, o: &Bbox) -> bool {
        !(o.x1 <= self.x0 || o.x0 >= self.x1 || o.y1 <= self.y0 || o.y0 >= self.y1)
    }
}

/// matplotlib's `segment_intersects_rectangle`.
fn segment_hits(x1: f64, y1: f64, x2: f64, y2: f64, b: &Bbox) -> bool {
    let (cx, cy) = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
    let (w, h) = (b.x1 - b.x0, b.y1 - b.y0);
    (x1 + x2 - 2.0 * cx).abs() < (x1 - x2).abs() + w
        && (y1 + y2 - 2.0 * cy).abs() < (y1 - y2).abs() + h
        && 2.0 * ((x1 - cx) * (y1 - y2) - (y1 - cy) * (x1 - x2)).abs()
            < w * (y1 - y2).abs() + h * (x1 - x2).abs()
}

fn path_hits(points: &[(f64, f64)], b: &Bbox) -> bool {
    points.windows(2).any(|w| {
        let ((x1, y1), (x2, y2)) = (w[0], w[1]);
        [x1, y1, x2, y2].iter().all(|v| v.is_finite()) && segment_hits(x1, y1, x2, y2, b)
    })
}

/// Where the legend goes, as fractions of the panel: the first of matplotlib's nine places that
/// covers nothing, else the one covering least (points, line vertices, line crossings, text and
/// the shaded band).
fn best_legend(ax: &Panel, entries: &[(String, String)]) -> [f64; 4] {
    let pt = DPI / 72.0;
    let fs = 8.0;
    let (fw, fh) = (FIGSIZE.0 * DPI, FIGSIZE.1 * DPI);
    let axes = Bbox {
        x0: ax.position[0] * fw,
        y0: ax.position[1] * fh,
        x1: (ax.position[0] + ax.position[2]) * fw,
        y1: (ax.position[1] + ax.position[3]) * fh,
    };
    let sx = |v: f64| axes.x0 + (v - ax.xlim.0) / (ax.xlim.1 - ax.xlim.0) * (axes.x1 - axes.x0);
    let sy = |v: f64| axes.y0 + (v - ax.ylim.0) / (ax.ylim.1 - ax.ylim.0) * (axes.y1 - axes.y0);
    let text_w = entries.iter().map(|(l, _)| text_width(l, fs, false)).fold(0.0, f64::max);
    let rows = entries.len() as f64;
    let width = (2.0 * 0.4 * fs + 2.0 * fs + 0.3 * fs + text_w) * pt;
    let height = (2.0 * 0.4 * fs + rows * LINE_HEIGHT * fs + (rows - 1.0).max(0.0) * 0.5 * fs) * pt;
    let pad = 0.5 * fs * pt;
    let c = Bbox { x0: axes.x0 + pad, y0: axes.y0 + pad, x1: axes.x1 - pad, y1: axes.y1 - pad };

    // What's in the panel, in display pixels.
    let mut lines: Vec<Vec<(f64, f64)>> = Vec::new();
    for l in &ax.lines {
        lines.push(l.x.iter().zip(&l.y).map(|(x, y)| (sx(*x), sy(*y))).collect());
    }
    for h in &ax.hlines {
        lines.push(vec![(axes.x0, sy(h.y)), (axes.x1, sy(h.y))]);
    }
    let offsets: Vec<(f64, f64)> = ax
        .scatters
        .iter()
        .flat_map(|s| s.x.iter().zip(&s.y).map(|(x, y)| (sx(*x), sy(*y))).collect::<Vec<_>>())
        .collect();
    let mut boxes: Vec<Bbox> =
        ax.bands.iter().map(|b| Bbox { x0: sx(b.x0), y0: axes.y0, x1: sx(b.x1), y1: axes.y1 }).collect();
    for t in &ax.texts {
        let x = match t.x {
            Coord::Data(v) => sx(v),
            Coord::Axes(f) => axes.x0 + f * (axes.x1 - axes.x0),
        } + t.offset.0 * pt;
        let y = match t.y {
            Coord::Data(v) => sy(v),
            Coord::Axes(f) => axes.y0 + f * (axes.y1 - axes.y0),
        } + t.offset.1 * pt;
        let (w, h) = (text_width(&t.text, t.size, t.bold) * pt, LINE_HEIGHT * t.size * pt);
        let x0 = if t.ha == HAlign::Left { x } else { x - w / 2.0 };
        let y0 = match t.va {
            VAlign::Bottom => y,
            VAlign::Center => y - h / 2.0,
            VAlign::Top => y - h,
        };
        boxes.push(Bbox { x0, y0, x1: x0 + w, y1: y0 + h });
    }

    let (mid_x, mid_y) = ((c.x0 + c.x1 - width) / 2.0, (c.y0 + c.y1 - height) / 2.0);
    let (left, right, low, high) = (c.x0, c.x1 - width, c.y0, c.y1 - height);
    // upper right, upper left, lower left, lower right, right, center left, center right,
    // lower center, upper center, center
    let spots = [
        (right, high),
        (left, high),
        (left, low),
        (right, low),
        (right, mid_y),
        (left, mid_y),
        (right, mid_y),
        (mid_x, low),
        (mid_x, high),
        (mid_x, mid_y),
    ];
    let mut best: Option<(usize, usize, Bbox)> = None;
    for (idx, (l, b)) in spots.into_iter().enumerate() {
        let lb = Bbox { x0: l, y0: b, x1: l + width, y1: b + height };
        let badness =
            lines.iter().map(|p| p.iter().filter(|(x, y)| lb.contains_strict(*x, *y)).count()).sum::<usize>()
                + offsets.iter().filter(|(x, y)| lb.contains_strict(*x, *y)).count()
                + boxes.iter().filter(|o| lb.overlaps(o)).count()
                + lines.iter().filter(|p| path_hits(p, &lb)).count();
        if best.is_none_or(|(bb, _, _)| badness < bb) {
            best = Some((badness, idx, lb));
        }
        if badness == 0 {
            break;
        }
    }
    let lb = best.expect("legend spots").2;
    let fx = |v: f64| (v - axes.x0) / (axes.x1 - axes.x0);
    let fy = |v: f64| (v - axes.y0) / (axes.y1 - axes.y0);
    [fx(lb.x0), fy(lb.y0), fx(lb.x1), fy(lb.y1)]
}
