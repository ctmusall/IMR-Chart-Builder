//! `imr-chart`: the I-MR Chart Builder app. Files given on the command line (or dropped on the
//! program) open in it; any `-option` runs the command line instead.

// No console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use imr_gui::{AppState, ImrApp, state::APP};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if !imr_cli::wants_gui(&args) {
        return imr_cli::main_with(std::env::args_os());
    }
    // The windowed build has no console: show anything that goes wrong in a message box.
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("Something went wrong: {info}");
        eprintln!("{msg}");
        rfd::MessageDialog::new()
            .set_title(APP)
            .set_description(msg)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("{APP} {}", imr_core::VERSION))
        .with_inner_size([1200.0, 900.0])
        .with_app_id("imr-chart")
        .with_drag_and_drop(true);
    if let Some(icon) = imr_gui::icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
    let result = eframe::run_native(
        APP,
        options,
        Box::new(move |cc| {
            imr_gui::setup_fonts(&cc.egui_ctx);
            let mut app = ImrApp::new(AppState::default());
            app.open_paths(&paths);
            Ok(Box::new(app))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title(APP)
                .set_description(format!("The window couldn't open: {e}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
            ExitCode::FAILURE
        }
    }
}
