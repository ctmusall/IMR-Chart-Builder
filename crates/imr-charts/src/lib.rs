//! The I-MR chart as an image, laid out as the Python version's matplotlib chart was: two panels
//! (individuals over moving ranges) on an 11 × 8.5 inch figure, saved as a PNG at 200 dpi.
//!
//! [`build::figure`] turns a chart job into a [`Figure`] (what is drawn where, independent of how);
//! [`svg::render_svg`] draws it, and [`save_png`] rasterizes that with resvg. The app draws the
//! same figure interactively.

pub mod build;
pub mod figure;
pub mod font;
pub mod svg;
pub mod ticks;

use std::path::Path;

use imr_core::{ChartJob, Error};
use resvg::{tiny_skia, usvg};

pub use build::figure;
pub use figure::Figure;

/// Pixels per inch of saved PNG files (the Python version's `savefig(dpi=200)`).
pub const PNG_DPI: f64 = 200.0;

fn options() -> usvg::Options<'static> {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_font_data(font::REGULAR.to_vec());
    opt.fontdb_mut().load_font_data(font::BOLD.to_vec());
    opt.font_family = font::FAMILY.into();
    opt.dpi = 100.0;
    opt
}

/// An SVG drawn as PNG bytes at `dpi` (the SVG is at 100 units per inch).
pub fn svg_to_png(svg: &str, dpi: f64) -> Result<Vec<u8>, Error> {
    let tree = usvg::Tree::from_str(svg, &options())
        .map_err(|e| Error::msg(format!("Couldn't draw the chart: {e}")))?;
    let scale = (dpi / 100.0) as f32;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).round() as u32, (size.height() * scale).round() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))
        .ok_or_else(|| Error::msg("The chart is too large to draw"))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| Error::msg(format!("Couldn't encode the PNG: {e}")))
}

/// The chart as PNG bytes.
pub fn png_bytes(job: &ChartJob) -> Result<Vec<u8>, Error> {
    svg_to_png(&svg::render_svg(&figure(job)), PNG_DPI)
}

/// Save the chart as a PNG image.
pub fn save_png(job: &ChartJob, path: &Path) -> Result<(), Error> {
    let bytes = png_bytes(job)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::msg(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| Error::msg(format!("{}: {e}", path.display())))
}
