//! Drawing a [`Figure`] as SVG, at 100 units per inch (matplotlib's figure DPI), styled like the
//! Python version's matplotlib chart: 10 pt tick and axis labels, left and bottom spines only, a
//! light horizontal grid under the data.

use std::fmt::Write as _;

use crate::figure::{Coord, Figure, FigureText, HAlign, Panel, VAlign};
use crate::font::{DESCENT, FAMILY, LINE_HEIGHT, text_width};

const DPI: f64 = 100.0;

fn pt(v: f64) -> f64 {
    v * DPI / 72.0
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

struct Svg {
    out: String,
    height: f64,
    clips: usize,
}

impl Svg {
    /// Flip y: figure coordinates run upward, SVG downward.
    fn y(&self, y: f64) -> f64 {
        self.height - y
    }

    /// One line of text whose box (by `ha` / `va`) is at `(x, y)` (figure pixels, y up).
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        x: f64,
        y: f64,
        text: &str,
        size_pt: f64,
        bold: bool,
        color: &str,
        ha: HAlign,
        va: VAlign,
        rotate: f64,
    ) {
        let size = pt(size_pt);
        let h = LINE_HEIGHT * size;
        let bottom = match va {
            VAlign::Bottom => y,
            VAlign::Center => y - h / 2.0,
            VAlign::Top => y - h,
        };
        let baseline = self.y(bottom + DESCENT * size);
        let anchor = match ha {
            HAlign::Left => "start",
            HAlign::Center => "middle",
        };
        let weight = if bold { r#" font-weight="bold""# } else { "" };
        let rot = if rotate != 0.0 {
            format!(r#" transform="rotate({:.1} {x:.2} {:.2})""#, -rotate, self.y(y))
        } else {
            String::new()
        };
        let _ = writeln!(
            self.out,
            r#"<text xml:space="preserve" x="{x:.2}" y="{baseline:.2}" font-size="{size:.2}" text-anchor="{anchor}" fill="{color}"{weight}{rot}>{}</text>"#,
            esc(text)
        );
    }

    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: &str, width_pt: f64) {
        let _ = writeln!(
            self.out,
            r#"<line x1="{x1:.2}" y1="{:.2}" x2="{x2:.2}" y2="{:.2}" stroke="{color}" stroke-width="{:.3}" stroke-linecap="butt"/>"#,
            self.y(y1),
            self.y(y2),
            pt(width_pt)
        );
    }

    fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, extra: &str) {
        let _ = writeln!(
            self.out,
            r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}"{extra}/>"#,
            x0.min(x1),
            self.y(y0.max(y1)),
            (x1 - x0).abs(),
            (y1 - y0).abs()
        );
    }
}

/// The chart as an SVG document `size` inches at 100 units per inch.
pub fn render_svg(fig: &Figure) -> String {
    let (w, h) = (fig.size.0 * DPI, fig.size.1 * DPI);
    let mut svg = Svg { out: String::new(), height: h, clips: 0 };
    let _ = writeln!(
        svg.out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="{FAMILY}, sans-serif">"#
    );
    svg.rect(0.0, 0.0, w, h, r##" fill="#ffffff""##);
    for p in &fig.panels {
        draw_panel(&mut svg, p, w, h);
    }
    figure_text(&mut svg, &fig.title, w, h);
    figure_text(&mut svg, &fig.note, w, h);
    svg.out.push_str("</svg>\n");
    svg.out
}

fn figure_text(svg: &mut Svg, t: &FigureText, w: f64, h: f64) {
    let lines: Vec<&str> = t.text.split('\n').collect();
    let step = LINE_HEIGHT * pt(t.size) * 1.2;
    let (x, y) = (t.x * w, t.y * h);
    let n = lines.len() as f64;
    for (i, line) in lines.iter().enumerate() {
        // The block's top (va top) or bottom (va bottom) is at y; lines go down from the first.
        let ly = match t.va {
            VAlign::Top => y - i as f64 * step,
            _ => y + (n - 1.0 - i as f64) * step,
        };
        svg.text(x, ly, line, t.size, t.bold, &t.color, t.ha, t.va, 0.0);
    }
}

fn draw_panel(svg: &mut Svg, p: &Panel, fw: f64, fh: f64) {
    let (x0, y0) = (p.position[0] * fw, p.position[1] * fh);
    let (x1, y1) = (x0 + p.position[2] * fw, y0 + p.position[3] * fh);
    let sx = |v: f64| x0 + (v - p.xlim.0) / (p.xlim.1 - p.xlim.0) * (x1 - x0);
    let sy = |v: f64| y0 + (v - p.ylim.0) / (p.ylim.1 - p.ylim.0) * (y1 - y0);
    let at = |c: Coord, horizontal: bool| match (c, horizontal) {
        (Coord::Data(v), true) => sx(v),
        (Coord::Data(v), false) => sy(v),
        (Coord::Axes(f), true) => x0 + f * (x1 - x0),
        (Coord::Axes(f), false) => y0 + f * (y1 - y0),
    };

    svg.clips += 1;
    let id = svg.clips;
    let _ = writeln!(
        svg.out,
        r#"<clipPath id="c{id}"><rect x="{x0:.2}" y="{:.2}" width="{:.2}" height="{:.2}"/></clipPath>"#,
        svg.y(y1),
        x1 - x0,
        y1 - y0
    );
    // Everything inside the panel, by drawing order (zorder); the grid sits under the data.
    enum Item<'a> {
        Band(&'a crate::figure::Band),
        Grid,
        Line(&'a crate::figure::Line),
        HLine(&'a crate::figure::HLine),
        Scatter(&'a crate::figure::Scatter),
    }
    let mut items: Vec<(f64, Item)> = vec![(0.5, Item::Grid)];
    items.extend(p.bands.iter().map(|b| (b.zorder, Item::Band(b))));
    items.extend(p.lines.iter().map(|l| (l.zorder, Item::Line(l))));
    items.extend(p.hlines.iter().map(|l| (l.zorder, Item::HLine(l))));
    items.extend(p.scatters.iter().map(|s| (s.zorder, Item::Scatter(s))));
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let _ = writeln!(svg.out, r#"<g clip-path="url(#c{id})">"#);
    for (_, item) in items {
        match item {
            Item::Band(b) => svg.rect(sx(b.x0), y0, sx(b.x1), y1, &format!(r#" fill="{}""#, b.color)),
            Item::Grid => {
                for (v, _) in &p.yticks {
                    svg.line(x0, sy(*v), x1, sy(*v), "#e6e6e6", 0.6);
                }
            }
            Item::Line(l) => {
                let mut d = String::new();
                let mut pen = false;
                for (x, y) in l.x.iter().zip(&l.y) {
                    if !(x.is_finite() && y.is_finite()) {
                        pen = false;
                        continue;
                    }
                    let _ = write!(d, "{}{:.2},{:.2} ", if pen { "L" } else { "M" }, sx(*x), svg.y(sy(*y)));
                    pen = true;
                }
                if !d.is_empty() {
                    let _ = writeln!(
                        svg.out,
                        r#"<path d="{d}" fill="none" stroke="{}" stroke-width="{:.3}" stroke-linejoin="round"/>"#,
                        l.color,
                        pt(l.width)
                    );
                }
            }
            Item::HLine(l) => svg.line(x0, sy(l.y), x1, sy(l.y), &l.color, l.width),
            Item::Scatter(s) => {
                let r = pt(s.size.sqrt()) / 2.0;
                let style = match &s.face {
                    Some(face) if s.width == 0.0 => format!(r#" fill="{face}""#),
                    Some(face) => {
                        format!(r#" fill="{face}" stroke="{}" stroke-width="{:.3}""#, s.edge, pt(s.width))
                    }
                    None => format!(r#" fill="none" stroke="{}" stroke-width="{:.3}""#, s.edge, pt(s.width)),
                };
                for (x, y) in s.x.iter().zip(&s.y).filter(|(x, y)| x.is_finite() && y.is_finite()) {
                    let _ = writeln!(
                        svg.out,
                        r#"<circle cx="{:.2}" cy="{:.2}" r="{r:.2}"{style}/>"#,
                        sx(*x),
                        svg.y(sy(*y))
                    );
                }
            }
        }
    }
    svg.out.push_str("</g>\n");

    // Spines: left and bottom only.
    svg.line(x0, y0, x0, y1, "#000000", 0.8);
    svg.line(x0, y0, x1, y0, "#000000", 0.8);
    // Ticks and their labels (3.5 pt ticks, labels 3.5 pt beyond them).
    let gap = pt(3.5 + 3.5);
    for (v, label) in &p.yticks {
        svg.line(x0 - pt(3.5), sy(*v), x0, sy(*v), "#000000", 0.8);
        let tw = text_width(label, pt(10.0), false);
        svg.text(x0 - gap - tw, sy(*v), label, 10.0, false, "#000000", HAlign::Left, VAlign::Center, 0.0);
    }
    let mut depth: f64 = 0.0;
    for (v, label) in &p.xticks {
        svg.line(sx(*v), y0 - pt(3.5), sx(*v), y0, "#000000", 0.8);
        if !p.show_xtick_labels {
            continue;
        }
        if p.xrotation == 90.0 {
            // Reading upward, the label's right end just below the tick.
            let tw = text_width(label, pt(10.0), false);
            depth = depth.max(tw);
            svg.text(
                sx(*v),
                y0 - gap - tw,
                label,
                10.0,
                false,
                "#000000",
                HAlign::Left,
                VAlign::Center,
                90.0,
            );
        } else {
            depth = depth.max(LINE_HEIGHT * pt(10.0));
            svg.text(sx(*v), y0 - gap, label, 10.0, false, "#000000", HAlign::Center, VAlign::Top, 0.0);
        }
    }
    if !p.xlabel.is_empty() {
        svg.text(
            (x0 + x1) / 2.0,
            y0 - gap - depth - pt(4.0),
            &p.xlabel,
            10.0,
            false,
            "#000000",
            HAlign::Center,
            VAlign::Top,
            0.0,
        );
    }
    if !p.ylabel.is_empty() {
        let widest = p.yticks.iter().map(|(_, l)| text_width(l, pt(10.0), false)).fold(0.0, f64::max);
        let x = x0 - gap - widest - pt(4.0) - LINE_HEIGHT * pt(10.0) / 2.0;
        svg.text(x, (y0 + y1) / 2.0, &p.ylabel, 10.0, false, "#000000", HAlign::Center, VAlign::Center, 90.0);
    }

    // Annotations, above the data; ones at a data position outside the panel are not drawn.
    let mut texts: Vec<&crate::figure::Annotation> = p.texts.iter().collect();
    texts.sort_by(|a, b| a.zorder.total_cmp(&b.zorder));
    for t in texts {
        let (mut x, mut y) = (at(t.x, true), at(t.y, false));
        if let Coord::Data(_) = t.y
            && !(y0 - 0.5..=y1 + 0.5).contains(&y)
        {
            continue;
        }
        x += pt(t.offset.0);
        y += pt(t.offset.1);
        if t.boxed {
            let size = pt(t.size);
            let (tw, th) = (text_width(&t.text, size, t.bold), LINE_HEIGHT * size);
            let pad = 0.15 * size;
            let by0 = match t.va {
                VAlign::Bottom => y,
                VAlign::Center => y - th / 2.0,
                VAlign::Top => y - th,
            };
            svg.rect(
                x - pad,
                by0 - pad,
                x + tw + pad,
                by0 + th + pad,
                r##" fill="#ffffff" fill-opacity="0.8" rx="2""##,
            );
        }
        svg.text(x, y, &t.text, t.size, t.bold, &t.color, t.ha, t.va, 0.0);
    }

    if let Some(legend) = &p.legend {
        let bx = |f: f64| x0 + f * (x1 - x0);
        let by = |f: f64| y0 + f * (y1 - y0);
        let [l, b, r, t] = legend.bbox;
        svg.rect(
            bx(l),
            by(b),
            bx(r),
            by(t),
            r##" fill="#ffffff" fill-opacity="0.85" stroke="#cccccc" stroke-width="1" rx="3""##,
        );
        let fs = legend.size;
        let row = LINE_HEIGHT * pt(fs);
        let mut cy = by(t) - pt(0.4 * fs) - row / 2.0;
        for (label, color) in &legend.entries {
            let hx = bx(l) + pt(0.4 * fs) + pt(fs);
            let _ = writeln!(
                svg.out,
                r#"<circle cx="{hx:.2}" cy="{:.2}" r="{:.2}" fill="{color}"/>"#,
                svg.y(cy),
                pt(14f64.sqrt()) / 2.0
            );
            let tx = bx(l) + pt(0.4 * fs) + pt(2.0 * fs) + pt(0.3 * fs);
            svg.text(tx, cy, label, fs, false, "#000000", HAlign::Left, VAlign::Center, 0.0);
            cy -= row + pt(0.5 * fs);
        }
    }
}
