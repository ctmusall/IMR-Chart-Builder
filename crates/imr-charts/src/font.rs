//! The chart's font, DejaVu Sans (matplotlib's default), embedded so charts look the same
//! everywhere, and text measurement for laying the chart out.

use std::collections::HashMap;
use std::sync::LazyLock;

pub const REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
pub const BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
pub const FAMILY: &str = "DejaVu Sans";

/// A line of text's height and descent, as fractions of the font size (matplotlib's measured
/// line box for DejaVu Sans at screen resolution).
pub const LINE_HEIGHT: f64 = 1.05;
pub const DESCENT: f64 = 0.22;

struct Metrics {
    units_per_em: f64,
    advances: HashMap<char, f64>,
    kerning: HashMap<(char, char), f64>,
    fallback: f64,
}

fn metrics(data: &[u8]) -> Metrics {
    let face = ttf_parser::Face::parse(data, 0).expect("embedded font parses");
    let units_per_em = face.units_per_em() as f64;
    let chars: Vec<char> = (' '..='~').chain("−—·…±σ̂̄₀₁₂₃₄ᵢ›→°µ²³×".chars()).collect();
    let glyph = |c: char| face.glyph_index(c);
    let advances = chars
        .iter()
        .filter_map(|&c| glyph(c).and_then(|g| face.glyph_hor_advance(g)).map(|a| (c, a as f64)))
        .collect();
    let mut kerning = HashMap::new();
    if let Some(kern) = face.tables().kern {
        for sub in kern.subtables.into_iter().filter(|s| s.horizontal && !s.variable) {
            for &a in &chars {
                for &b in &chars {
                    if let (Some(ga), Some(gb)) = (glyph(a), glyph(b))
                        && let Some(k) = sub.glyphs_kerning(ga, gb)
                    {
                        kerning.insert((a, b), k as f64);
                    }
                }
            }
        }
    }
    let fallback = glyph('n').and_then(|g| face.glyph_hor_advance(g)).map_or(units_per_em * 0.6, f64::from);
    Metrics { units_per_em, advances, kerning, fallback }
}

static REGULAR_METRICS: LazyLock<Metrics> = LazyLock::new(|| metrics(REGULAR));
static BOLD_METRICS: LazyLock<Metrics> = LazyLock::new(|| metrics(BOLD));

/// The width of one line of `text` at `size` (any unit), with kerning.
pub fn text_width(text: &str, size: f64, bold: bool) -> f64 {
    let m = if bold { &*BOLD_METRICS } else { &*REGULAR_METRICS };
    let chars: Vec<char> = text.chars().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect();
    let mut units: f64 = chars.iter().map(|c| m.advances.get(c).copied().unwrap_or(m.fallback)).sum();
    for pair in chars.windows(2) {
        units += m.kerning.get(&(pair[0], pair[1])).copied().unwrap_or(0.0);
    }
    units * size / m.units_per_em
}
