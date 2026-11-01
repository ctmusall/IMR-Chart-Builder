//! The real window, driven headless: click buttons and check what happens.

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use imr_gui::{AppState, ImrApp};

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/data").join(name)
}

fn harness(app: ImrApp) -> Harness<'static, ImrApp> {
    Harness::builder()
        .with_size([1200.0, 900.0])
        .with_context_setup(imr_gui::setup_fonts)
        .build_ui_state(|ui, app: &mut ImrApp| app.draw(ui), app)
}

fn opened(names: &[&str]) -> ImrApp {
    let mut app = ImrApp::new(AppState::default());
    app.open_paths(&names.iter().map(|n| data(n)).collect::<Vec<_>>());
    app
}

#[test]
fn generate_button_draws_the_chart() {
    let mut app = opened(&["with_header_tab.xlsx"]);
    app.state.choose_column(0, "QC3 CD34/Unit");
    let mut h = harness(app);
    h.run();
    h.get_by_label("Generate chart").click();
    h.run();
    let s = &h.state().state;
    assert!(s.errors.is_empty(), "{:?}", s.errors);
    assert!(s.figure.is_some());
    assert!(s.stats.starts_with("n=4"), "{}", s.stats);
    h.get_by_label("Save PNG…");
}

#[test]
fn settings_fold_away() {
    let mut h = harness(opened(&["with_header_tab.xlsx"]));
    h.run();
    assert!(h.query_by_label("LSL").is_some());
    h.get_by_label("Data & rules").click();
    h.run();
    assert!(!h.state().state.settings_open);
    assert!(h.query_by_label("LSL").is_none());
    assert!(h.query_by_label("Generate chart").is_some());
}

#[test]
fn rule_buttons_edit_the_table() {
    let mut h = harness(opened(&["with_header_tab.xlsx"]));
    h.run();
    h.get_by_label("Add").click();
    h.run();
    h.get_by_label("Add").click();
    h.run();
    assert_eq!(h.state().state.rules.len(), 2);
    h.get_by_label("Remove").click();
    h.run();
    assert_eq!(h.state().state.rules.len(), 1);
}

#[test]
fn errors_show_until_closed() {
    let mut app = opened(&["with_header_tab.xlsx"]);
    app.state.lsl = "abc".into();
    let mut h = harness(app);
    h.run();
    h.get_by_label("Generate chart").click();
    h.run();
    assert_eq!(h.state().state.errors.len(), 1);
    h.get_by_label("OK").click();
    h.run();
    assert!(h.state().state.errors.is_empty());
}

/// Set IMR_UI_DIR to render screenshots of the window with a chart there, in the light and dark
/// themes (needs a GPU or a software Vulkan/GL driver).
#[test]
fn screenshot() {
    let Some(dir) = std::env::var_os("IMR_UI_DIR").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, name) in [(egui::Theme::Light, "window-light.png"), (egui::Theme::Dark, "window-dark.png")] {
        let mut app = opened(&["qc_summary.xlsx"]);
        app.state.choose_column(0, "QC3 CD34/Unit");
        app.state.lsl = "2e7".into();
        app.state
            .load_rules(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/rules/qc.json"));
        assert!(app.state.generate());
        let mut h = Harness::builder()
            .with_size([1400.0, 1000.0])
            .with_theme(theme)
            .with_context_setup(imr_gui::setup_fonts)
            .wgpu()
            .build_ui_state(|ui, app: &mut ImrApp| app.draw(ui), app);
        h.run();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}
