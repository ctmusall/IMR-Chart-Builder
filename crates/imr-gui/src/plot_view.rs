//! The chart in the window, drawn with egui_plot from the same [`Figure`] the PNG is drawn from:
//! drag to pan, scroll to zoom (both panels together), double-click to reset; hovering a point
//! shows its lot, date, value and source file.

use egui::{Align2, Color32, Id, RichText, Ui};
use egui_plot::{GridMark, HLine, Legend, Line, MarkerShape, Plot, PlotPoint, PlotPoints, Points, Text};
use imr_charts::figure::{Coord, Figure, Panel};
use imr_charts::ticks::{date_label, date_ticks};
use imr_core::ChartJob;
use imr_core::plot::{format_number, looks_like_fraction};

/// `#rrggbb` as a color (black when it isn't one).
pub fn color32(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    match u32::from_str_radix(h, 16) {
        Ok(v) if h.len() == 6 => Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
        _ => Color32::BLACK,
    }
}

/// A chart color as drawn in the window. The chart's colors are matplotlib's, chosen for white
/// paper; on a dark background the near-black points turn light, the grey connector lighter, and
/// the pale recent-days band a faint orange tint. Saved files always use the originals.
fn shown(hex: &str, dark: bool) -> Color32 {
    let c = color32(hex);
    if !dark {
        return c;
    }
    match hex {
        imr_core::plot::POINT_COLOR => Color32::from_gray(225),
        imr_core::plot::CONNECTOR_COLOR => Color32::from_gray(150),
        imr_core::plot::RECENT_BAND_COLOR => Color32::from_rgba_unmultiplied(255, 127, 14, 36),
        _ => c,
    }
}

fn pt_px(points: f64) -> f32 {
    (points * 96.0 / 72.0) as f32
}

/// What hovering point `i` shows, as in the HTML chart.
fn tooltip(job: &ChartJob, i: usize, mr: bool, percent: bool) -> String {
    let p = &job.data[i];
    let r = &job.result;
    let mut lines = vec![if p.label.is_empty() { "(no lot)".to_string() } else { p.label.py_str() }];
    lines.push(match p.date {
        Some(d) if job.has_dates() => d.format("%m/%d/%Y").to_string(),
        _ => format!("Obs {}", i + 1),
    });
    if job.sources.len() > 1 {
        lines.push(p.file.clone());
    }
    if let Some(rule) = p.rule.as_deref().filter(|r| !r.is_empty()) {
        lines.push(format!("Rule: {rule}"));
    }
    let v = if mr { r.moving_ranges[i] } else { r.values[i] };
    lines.push(format!("{}: {}", if mr { "MR" } else { "Value" }, format_number(v, percent)));
    lines.join("\n")
}

/// Draw the chart: title, the two linked panels, and the note.
pub fn chart_view(ui: &mut Ui, fig: &Figure, job: &ChartJob, id: Id, height: f32) {
    ui.vertical_centered(|ui| ui.label(RichText::new(&fig.title.text).strong().size(16.0)));
    let total = height.max(420.0) - 90.0;
    let heights = [total * 0.6, total * 0.4];
    let link = id.with("x");
    let percent = looks_like_fraction(&job.result.values);
    for (k, (panel, h)) in fig.panels.iter().zip(heights).enumerate() {
        panel_view(ui, panel, job, id.with(k), link, h, k == 1, percent);
    }
    ui.label(RichText::new(&fig.note.text).small().color(Color32::from_gray(90)));
}

#[allow(clippy::too_many_arguments)]
fn panel_view(
    ui: &mut Ui,
    p: &Panel,
    job: &ChartJob,
    id: Id,
    link: Id,
    height: f32,
    mr: bool,
    percent: bool,
) {
    let mut plot = Plot::new(id)
        .height(height)
        .y_axis_label(p.ylabel.clone())
        .link_axis(link, [true, false])
        .link_cursor(link, [true, false])
        .include_x(p.xlim.0)
        .include_x(p.xlim.1)
        .include_y(p.ylim.0)
        .include_y(p.ylim.1)
        .show_x(false)
        .show_y(false)
        .show_grid([false, true])
        .y_axis_min_width(60.0)
        .y_axis_formatter(move |mark, _| format_number(mark.value, percent));
    if !p.xlabel.is_empty() {
        plot = plot.x_axis_label(p.xlabel.clone());
    }
    if p.legend.is_some() {
        plot = plot.legend(Legend::default().position(egui_plot::Corner::LeftTop));
    }
    if p.dates {
        plot = plot
            .x_grid_spacer(|input| {
                let (lo, hi) = input.bounds;
                let ticks = date_ticks(lo, hi);
                let step = if ticks.len() > 1 { ticks[1] - ticks[0] } else { 1.0 };
                ticks.into_iter().map(|value| GridMark { value, step_size: step }).collect()
            })
            .x_axis_formatter(|mark, _| date_label(mark.value));
    }
    let dark = ui.visuals().dark_mode;
    let resp = plot.show(ui, |pu| {
        for b in &p.bands {
            pu.span(egui_plot::Span::new("", b.x0..=b.x1).fill(shown(&b.color, dark)));
        }
        for l in &p.lines {
            // NaN breaks the line.
            let mut seg: Vec<[f64; 2]> = Vec::new();
            let flush = |seg: &mut Vec<[f64; 2]>, pu: &mut egui_plot::PlotUi<'_>| {
                if seg.len() > 1 {
                    pu.line(
                        Line::new("", PlotPoints::from(std::mem::take(seg)))
                            .color(shown(&l.color, dark))
                            .width(1.0)
                            // Draw every segment (no aggregating dense points into a band).
                            .allow_band(false)
                            .allow_hover(false),
                    );
                }
                seg.clear();
            };
            for (x, y) in l.x.iter().zip(&l.y) {
                if x.is_finite() && y.is_finite() {
                    seg.push([*x, *y]);
                } else {
                    flush(&mut seg, pu);
                }
            }
            flush(&mut seg, pu);
        }
        for h in &p.hlines {
            pu.hline(
                HLine::new("", h.y).color(shown(&h.color, dark)).width(pt_px(h.width)).allow_hover(false),
            );
        }
        let mut scatters: Vec<&imr_charts::figure::Scatter> = p.scatters.iter().collect();
        scatters.sort_by(|a, b| a.zorder.total_cmp(&b.zorder));
        for s in scatters {
            let pts: Vec<[f64; 2]> =
                s.x.iter()
                    .zip(&s.y)
                    .filter(|(x, y)| x.is_finite() && y.is_finite())
                    .map(|(x, y)| [*x, *y])
                    .collect();
            let radius = pt_px(s.size.sqrt()) / 2.0 + if s.face.is_none() { 1.0 } else { 0.0 };
            let name = s.label.clone().unwrap_or_default();
            let name = if s.label.as_deref() == Some("Beyond limit") { String::new() } else { name };
            pu.points(
                Points::new(name, PlotPoints::from(pts))
                    .shape(MarkerShape::Circle)
                    .radius(radius)
                    .filled(s.face.is_some())
                    .color(shown(s.face.as_deref().unwrap_or(&s.edge), dark))
                    .allow_hover(false),
            );
        }
        // Limit values at the data's right edge, the LSL label at its left, "Last N days" at the
        // band's top (placing them at the plot's edge would widen the automatic bounds every frame).
        for t in &p.texts {
            let y = match t.y {
                Coord::Data(v) => v,
                Coord::Axes(f) => p.ylim.0 + f * (p.ylim.1 - p.ylim.0),
            };
            let (x, align) = match t.x {
                Coord::Axes(f) if f >= 1.0 => (p.xlim.1, Align2::RIGHT_BOTTOM),
                Coord::Axes(_) => (p.xlim.0, Align2::LEFT_BOTTOM),
                Coord::Data(v) => (v, Align2::LEFT_TOP),
            };
            let mut text = RichText::new(&t.text).color(shown(&t.color, dark)).size(11.0);
            if t.bold {
                text = text.strong();
            }
            pu.text(Text::new("", PlotPoint::new(x, y), text).anchor(align).allow_hover(false));
        }
    });

    // Hover: the nearest point within 10 px.
    if let Some(pos) = resp.response.hover_pos() {
        let fig_x = &p.lines[0].x;
        let ys = &p.lines[0].y;
        let best = fig_x
            .iter()
            .zip(ys)
            .enumerate()
            .filter(|(_, (x, y))| x.is_finite() && y.is_finite())
            .map(|(i, (x, y))| (i, resp.transform.position_from_point(&PlotPoint::new(*x, *y)).distance(pos)))
            .filter(|(_, d)| *d <= 10.0)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = best {
            let text = tooltip(job, i, mr, percent);
            resp.response.on_hover_ui_at_pointer(|ui| {
                ui.label(text);
            });
        }
    }
}
