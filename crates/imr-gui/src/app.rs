//! The window: files and their column choices, chart settings, the highlight rules table, the
//! action buttons, and the chart. All logic lives in [`AppState`]; this draws it.

use std::path::PathBuf;

use egui::{Color32, Id, RichText, Ui};
use imr_core::rules::OPERATORS;

use crate::plot_view::{chart_view, color32};
use crate::state::{AppState, Format, RuleRow};

const DATA_FILTER: &[&str] = &["xlsx", "xlsm", "xls", "csv", "tsv", "txt"];
/// Workbook sections shown before the list scrolls.
const VISIBLE_SOURCES: f32 = 3.0;

pub struct ImrApp {
    pub state: AppState,
}

/// A combo box over `options`, named for screen readers and tests. Returns the new choice.
fn combo(ui: &mut Ui, id: Id, name: &str, value: &str, options: &[String], width: f32) -> Option<String> {
    let mut chosen = None;
    let inner =
        egui::ComboBox::from_id_salt(id).selected_text(value).width(width).height(320.0).show_ui(ui, |ui| {
            for o in options {
                if ui.selectable_label(o == value, o.as_str()).clicked() && o != value {
                    chosen = Some(o.clone());
                }
            }
        });
    let (label, enabled) = (format!("{name}: {value}"), ui.is_enabled());
    inner.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, &label));
    chosen
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

impl ImrApp {
    pub fn new(state: AppState) -> ImrApp {
        ImrApp { state }
    }

    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        let existing: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
        if !existing.is_empty() {
            self.state.open_paths(&existing);
        }
    }

    /// Draw the whole window into `ui`.
    pub fn draw(&mut self, ui: &mut Ui) {
        let dropped: Vec<PathBuf> =
            ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.state.open_paths(&dropped);
        }
        let arrow = if self.state.settings_open { "▼" } else { "▶" };
        let toggle = ui.add(egui::Button::new(format!("{arrow} Data & rules")).frame(false));
        toggle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Data & rules"));
        if toggle.clicked() {
            self.state.settings_open = !self.state.settings_open;
        }
        if self.state.settings_open {
            self.settings(ui);
            ui.separator();
            self.rules_table(ui);
        }
        ui.separator();
        self.actions(ui);
        if !self.state.stats.is_empty() {
            ui.add(egui::Label::new(RichText::new(&self.state.stats).monospace()).selectable(true));
        }
        if !self.state.status.is_empty() {
            ui.label(RichText::new(&self.state.status).small().weak());
        }
        if let (Some(fig), Some(job)) = (&self.state.figure, &self.state.job) {
            // The chart takes the room left (fold the settings away for more).
            let height = ui.available_height().max(480.0);
            chart_view(ui, fig, job, Id::unique("chart"), height);
        }
        self.error_modal(ui);
    }

    fn settings(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Files:");
            if ui.button("Open file(s)…").clicked()
                && let Some(paths) = rfd::FileDialog::new()
                    .set_title("Open data file(s)")
                    .add_filter("Data files", DATA_FILTER)
                    .pick_files()
            {
                self.state.open_paths(&paths);
            }
            let tip = self
                .state
                .sources
                .iter()
                .map(|s| s.path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            let label = ui.label(self.state.files_label());
            if !tip.is_empty() {
                label.on_hover_text(tip);
            }
        });
        if !self.state.sources.is_empty() {
            let row_h = ui.spacing().interact_size.y + 30.0;
            egui::ScrollArea::vertical().max_height(row_h * VISIBLE_SOURCES).auto_shrink([false, true]).show(
                ui,
                |ui| {
                    for i in 0..self.state.sources.len() {
                        self.source_section(ui, i);
                    }
                },
            );
        }
        let loaded = self.state.can_generate();
        ui.add_enabled_ui(loaded, |ui| {
            egui::Grid::new("settings").num_columns(2).show(ui, |ui| {
                ui.label("LSL:");
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.state.lsl)
                        .hint_text("e.g. 2e7 or 50%  (blank = none)")
                        .desired_width(240.0),
                );
                edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "LSL"));
                if edit.changed() {
                    self.state.invalidate();
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.state.generate();
                }
                ui.end_row();
                ui.label("Sigma multiplier (k):");
                let sigma = ui.add(
                    egui::DragValue::new(&mut self.state.sigma)
                        .range(0.5..=6.0)
                        .speed(0.05)
                        .fixed_decimals(2),
                );
                sigma.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Sigma multiplier (k)")
                });
                if sigma.changed() {
                    self.state.invalidate();
                }
                ui.end_row();
                ui.label("Highlight recent:");
                let recent = egui::DragValue::new(&mut self.state.recent_days)
                    .range(0..=3650)
                    .custom_formatter(|v, _| if v == 0.0 { "Off".into() } else { format!("{v:.0} days") })
                    .custom_parser(|s| {
                        let s = s.trim().trim_end_matches("days").trim();
                        if s.eq_ignore_ascii_case("off") { Some(0.0) } else { s.parse().ok() }
                    });
                let recent = ui.add(recent);
                recent.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Highlight recent")
                });
                if recent.changed() {
                    self.state.invalidate();
                }
                ui.end_row();
            });
        });
    }

    fn source_section(&mut self, ui: &mut Ui, i: usize) {
        let s = &self.state.sources[i];
        let (name, path, csv) = (s.name(), s.path.display().to_string(), s.is_csv());
        let (sheets, sheet) = (s.sheets.clone(), s.sheet.clone());
        let (columns, column) = (s.columns.clone(), s.column.clone());
        let (dates, date) = (s.dates.clone(), s.date.clone());
        ui.group(|ui| {
            ui.label(RichText::new(&name).strong()).on_hover_text(path);
            ui.horizontal(|ui| {
                ui.label("Sheet:");
                let picked = ui
                    .add_enabled_ui(!csv, |ui| {
                        combo(ui, Id::unique(("sheet", i)), "Sheet", &sheet, &sheets, 160.0)
                    })
                    .inner;
                if let Some(sh) = picked {
                    self.state.load_sheet(i, &sh);
                }
                ui.label("Value column:");
                if let Some(c) =
                    combo(ui, Id::unique(("column", i)), "Value column", &column, &columns, 260.0)
                {
                    self.state.choose_column(i, &c);
                }
                ui.label("Date column:");
                if let Some(d) = combo(ui, Id::unique(("date", i)), "Date column", &date, &dates, 180.0) {
                    self.state.choose_date(i, &d);
                }
            });
        });
    }

    fn rules_table(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Highlight rules (the first matching rule colors the point)").strong());
        let sheets = self.state.rule_sheets();
        let ops: Vec<String> = OPERATORS.iter().map(|s| s.to_string()).collect();
        let mut changed = false;
        egui::ScrollArea::vertical().id_salt("rules").max_height(150.0).auto_shrink([false, true]).show(
            ui,
            |ui| {
                egui::Grid::new("rules_grid").num_columns(8).striped(true).show(ui, |ui| {
                    for h in ["", "On", "Name", "Sheet", "Column", "Operator", "Value", "Color"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for i in 0..self.state.rules.len() {
                        let selected = self.state.selected_rule == Some(i);
                        if ui.selectable_label(selected, format!("{}", i + 1)).clicked() {
                            self.state.selected_rule = Some(i);
                        }
                        let columns = self.state.columns_for(&self.state.rules[i].sheet);
                        let row: &mut RuleRow = &mut self.state.rules[i];
                        let named = |r: egui::Response, role: egui::Role, name: &str| {
                            let name = format!("Rule {} {name}", i + 1);
                            r.widget_info(|| egui::WidgetInfo::labeled(role, true, &name));
                            r.changed()
                        };
                        let mut touched =
                            named(ui.checkbox(&mut row.enabled, ""), egui::Role::CheckBox, "on");
                        touched |= named(
                            ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut row.name)),
                            egui::Role::TextInput,
                            "name",
                        );
                        if let Some(s) = combo(
                            ui,
                            Id::unique(("rule_sheet", i)),
                            "Rule sheet",
                            &row.sheet.clone(),
                            &sheets,
                            120.0,
                        ) {
                            row.sheet = s;
                            touched = true;
                        }
                        if let Some(c) = combo(
                            ui,
                            Id::unique(("rule_column", i)),
                            "Rule column",
                            &row.column.clone(),
                            &columns,
                            180.0,
                        ) {
                            row.column = c;
                            touched = true;
                        }
                        if let Some(o) = combo(
                            ui,
                            Id::unique(("rule_op", i)),
                            "Rule operator",
                            &row.op.clone(),
                            &ops,
                            100.0,
                        ) {
                            row.op = o;
                            touched = true;
                        }
                        touched |= named(
                            ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(&mut row.value)),
                            egui::Role::TextInput,
                            "value",
                        );
                        let mut c = color32(&row.color);
                        if egui::color_picker::color_edit_button_srgba(
                            ui,
                            &mut c,
                            egui::color_picker::Alpha::Opaque,
                        )
                        .changed()
                        {
                            row.color = hex(c);
                            touched = true;
                        }
                        if touched {
                            self.state.selected_rule = Some(i);
                            changed = true;
                        }
                        ui.end_row();
                    }
                });
            },
        );
        if changed {
            self.state.invalidate();
        }
        ui.horizontal(|ui| {
            if ui.button("Add").clicked() {
                self.state.add_rule(RuleRow::default());
            }
            if ui.button("Remove").clicked() {
                self.state.remove_rule();
            }
            if ui.button("Up").clicked() {
                self.state.move_rule(-1);
            }
            if ui.button("Down").clicked() {
                self.state.move_rule(1);
            }
            ui.separator();
            if ui.button("Load rules…").clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .set_title("Load highlight rules")
                    .add_filter("Highlight rules", &["json"])
                    .pick_file()
            {
                self.state.load_rules(&p);
            }
            if ui.button("Save rules…").clicked() {
                if let Err(e) = self.state.checked_rules() {
                    self.state.errors.push(format!("Could not save rules: {e}"));
                } else if let Some(p) = rfd::FileDialog::new()
                    .set_title("Save highlight rules")
                    .set_file_name("highlight_rules.json")
                    .add_filter("Highlight rules", &["json"])
                    .save_file()
                {
                    self.state.save_rules(&p);
                }
            }
        });
    }

    fn actions(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.add_enabled(self.state.can_generate(), egui::Button::new("Generate chart")).clicked() {
                self.state.generate();
            }
            let ready = self.state.job.is_some();
            for (format, label) in [
                (Format::Png, "Save PNG…"),
                (Format::Html, "Save HTML…"),
                (Format::Xlsx, "Save Excel stats…"),
            ] {
                if ui.add_enabled(ready, egui::Button::new(label)).clicked() {
                    self.save_dialog(format);
                }
            }
        });
    }

    fn save_dialog(&mut self, format: Format) {
        let Some(start) = self.state.default_save_path(format) else { return };
        let mut dialog = rfd::FileDialog::new()
            .set_title(format!("Save {}", format.extension().to_uppercase()))
            .add_filter(format.filter_name(), &[format.extension()]);
        if let Some(dir) = start.parent().filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = start.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(path) = dialog.save_file() {
            self.state.save(format, &path);
        }
    }

    /// The oldest problem, in a box the user closes.
    fn error_modal(&mut self, ui: &mut Ui) {
        let Some(msg) = self.state.errors.first().cloned() else { return };
        let modal = egui::Modal::new(Id::unique("error")).show(ui.ctx(), |ui| {
            ui.set_max_width(480.0);
            ui.heading("Something went wrong");
            ui.label(&msg);
            ui.add_space(8.0);
            ui.button("OK").clicked()
        });
        if modal.inner || modal.should_close() {
            self.state.errors.remove(0);
        }
    }
}

impl eframe::App for ImrApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| self.draw(ui));
    }
}
