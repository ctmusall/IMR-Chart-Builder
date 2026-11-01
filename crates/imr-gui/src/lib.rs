//! The I-MR Chart Builder desktop app (egui).

pub mod app;
pub mod plot_view;
pub mod state;

pub use app::ImrApp;
pub use state::AppState;

/// The look: a light theme (the chart's colors are matplotlib's, chosen for white paper), and
/// fonts: egui's defaults, with Hack (arrows ▼ ▶) and Noto Emoji (⚠) as fallbacks for symbols the
/// UI font lacks.
pub fn setup_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "NotoEmoji".into(),
        Arc::new(
            egui::FontData::from_static(epaint_default_fonts::NOTO_EMOJI_REGULAR)
                .tweak(egui::FontTweak { scale: 0.81, ..Default::default() }),
        ),
    );
    for (family, fallbacks) in [
        (egui::FontFamily::Proportional, ["Hack", "NotoEmoji"]),
        (egui::FontFamily::Monospace, ["Ubuntu-Light", "NotoEmoji"]),
    ] {
        let list = fonts.families.entry(family).or_default();
        for name in fallbacks {
            if !list.iter().any(|n| n == name) {
                list.push(name.into());
            }
        }
    }
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Light);
}

/// The window icon.
pub fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory_with_format(
        include_bytes!("../../../assets/icon.png"),
        image::ImageFormat::Png,
    )
    .ok()?
    .into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}
