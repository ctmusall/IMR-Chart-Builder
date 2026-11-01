//! The Python GUI smoke tests (tests/test_gui_smoke.py), ported to the app's state.

use std::path::{Path, PathBuf};

use imr_core::rules::Rule;
use imr_gui::AppState;
use imr_gui::state::{ANY_SHEET, Format, NO_DATE, RuleRow};

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/data").join(name)
}

fn opened(names: &[&str]) -> AppState {
    let mut s = AppState::default();
    assert!(s.open_paths(&names.iter().map(|n| data(n)).collect::<Vec<_>>()), "{:?}", s.errors);
    s
}

#[test]
fn initial_state() {
    let mut s = AppState::default();
    assert!(!s.can_generate());
    assert!(!s.generate());
    assert!(s.save(Format::Png, Path::new("x")).is_none());
    assert_eq!(s.files_label(), "No file loaded");
}

#[test]
fn load_generate_save() {
    let mut s = opened(&["with_header_tab.xlsx"]);
    let src = &s.sources[0];
    assert_eq!(src.sheet, "Summary");
    assert!(src.columns.contains(&"QC3 CD34/Unit".to_string()));
    assert_eq!(src.date, "Date Processed");
    s.choose_column(0, "QC3 CD34/Unit");
    s.lsl = "2e7".into();
    assert!(s.generate(), "{:?}", s.errors);
    assert!(s.figure.is_some());
    assert!(s.stats.contains("n=4") && s.stats.contains("below LSL"), "{}", s.stats);
    let dir = tempfile::tempdir().unwrap();
    for f in [Format::Png, Format::Html, Format::Xlsx] {
        let out = s.save(f, &dir.path().join(format!("chart_{}", f.extension()))).unwrap();
        assert_eq!(out.extension().unwrap(), f.extension());
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
    }
    let start = s.default_save_path(Format::Png).unwrap();
    assert_eq!(start.file_name().unwrap(), "IMR_QC3_CD34_Unit.png");
    assert_eq!(start.parent().unwrap(), data("with_header_tab.xlsx").parent().unwrap());
    s.choose_date(0, NO_DATE);
    assert!(s.job.is_none(), "a new choice clears the chart");
    assert!(s.generate());
    assert!(s.errors.is_empty(), "{:?}", s.errors);
}

#[test]
fn reload_sheet_keeps_column_when_present() {
    let mut s = opened(&["with_header_tab.xlsx"]);
    s.choose_column(0, "QC3 CD34 Viability");
    assert!(s.load_sheet(0, "Summary"));
    assert_eq!(s.sources[0].column, "QC3 CD34 Viability");
    assert!(s.load_sheet(0, "Query"));
    assert_eq!(s.sources[0].column, "x");
}

#[test]
fn errors_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.xlsx");
    std::fs::write(&bad, "not a workbook").unwrap();
    let mut s = AppState::default();
    assert!(!s.open_paths(&[bad]));
    assert!(!s.errors.is_empty());
    let mut s = opened(&["with_header_tab.xlsx"]);
    s.lsl = "abc".into();
    assert!(!s.generate());
    assert!(s.errors.last().unwrap().contains("Invalid limit"));
    assert!(s.job.is_none());
}

#[test]
fn csv_and_no_numeric_columns() {
    let mut s = opened(&["data.csv"]);
    assert!(s.generate());
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("text.csv");
    std::fs::write(&p, "a,b\nx,y\nz,w\n").unwrap();
    let mut s = AppState::default();
    assert!(s.open_paths(&[p]));
    assert!(!s.generate());
    assert!(s.errors.last().unwrap().contains("no numeric columns"));
}

#[test]
fn multiple_files() {
    let mut s = opened(&["part1.xlsx", "part2.xlsx"]);
    assert!(s.files_label().starts_with("2 files: part1.xlsx"));
    s.choose_column(0, "QC3 CD34/Unit");
    // Copied to the second file even though that column is mostly junk there (so not offered by default).
    assert_eq!(s.sources[1].column, "QC3 CD34/Unit");
    assert!(s.generate(), "{:?}", s.errors);
    assert!(s.stats.contains("files=2") && s.stats.contains("n=4"), "{}", s.stats);
    let dir = tempfile::tempdir().unwrap();
    for f in [Format::Png, Format::Html, Format::Xlsx] {
        assert!(s.save(f, &dir.path().join("pooled")).is_some());
    }
    assert!(s.errors.is_empty(), "{:?}", s.errors);
}

#[test]
fn highlight_rules() {
    let mut s = opened(&["with_header_tab.xlsx"]);
    assert!(s.columns_for(ANY_SHEET).contains(&"Notes".to_string()));
    s.add_rule(RuleRow::from(&Rule::new("note b", "Notes", "=", "b", "#ff00ff", true, "").unwrap()));
    s.add_rule(RuleRow { column: "QC Lot".into(), value: "L2".into(), ..RuleRow::default() });
    s.move_rule(-1); // second row (selected) moves to the top
    let labels: Vec<String> = s.checked_rules().unwrap().iter().map(Rule::label).collect();
    assert_eq!(labels, ["QC Lot = 'L2'", "note b"]);
    s.choose_column(0, "QC3 CD34/Unit");
    assert!(s.generate(), "{:?}", s.errors);
    assert_eq!(s.job.as_ref().unwrap().highlighted(), 2);
    assert!(s.stats.contains("highlighted=2"));

    let dir = tempfile::tempdir().unwrap();
    let path = s.save_rules(&dir.path().join("r")).unwrap();
    assert_eq!(path.extension().unwrap(), "json");
    s.set_rules(&[]);
    assert!(s.checked_rules().unwrap().is_empty());
    assert!(s.load_rules(&path));
    let colors: Vec<String> = s.checked_rules().unwrap().iter().map(|r| r.color.clone()).collect();
    assert_eq!(colors, ["#9467bd", "#ff00ff"]);

    s.add_rule(RuleRow::default()); // blank column: generating names the incomplete rule
    assert!(!s.generate());
    assert!(s.errors.last().unwrap().contains("Highlight rule 3"), "{:?}", s.errors);
}

#[test]
fn workbooks_with_different_sheets() {
    let mut s = opened(&["with_header_tab.xlsx", "inline.xlsx"]);
    assert_eq!((s.sources[0].sheet.as_str(), s.sources[1].sheet.as_str()), ("Summary", "Data"));
    s.choose_column(0, "QC3 CD34/Unit");
    assert_eq!(s.sources[1].column, "QC3 CD34/Unit");
    s.choose_column(1, "QC3 CD34 Viability"); // the second file's choice is its own
    assert_eq!(s.sources[0].column, "QC3 CD34/Unit");
    let sheets: Vec<String> = s.columns_by_sheet().into_iter().map(|(n, _)| n).collect();
    assert_eq!(sheets, ["Summary", "Data"]);
    s.add_rule(RuleRow::from(&Rule::new("data b", "Notes", "=", "b", "#9467bd", true, "Data").unwrap()));
    assert_eq!(s.checked_rules().unwrap()[0].sheet, "Data");
    assert!(s.generate(), "{:?}", s.errors);
    let job = s.job.as_ref().unwrap();
    assert_eq!(job.columns, ["QC3 CD34/Unit", "QC3 CD34 Viability"]);
    assert_eq!(job.highlighted(), 1); // only the Data sheet's L1 row, not Summary's
    assert!(s.stats.contains("files=2") && s.stats.contains("n=10"), "{}", s.stats);
    let dir = tempfile::tempdir().unwrap();
    assert!(s.save(Format::Xlsx, &dir.path().join("mixed")).is_some());

    s.choose_date(1, NO_DATE);
    assert!(!s.generate());
    assert!(s.errors.last().unwrap().contains("date column for every workbook"));
    assert_eq!(s.errors.len(), 1);
}

#[test]
fn rule_sheet_limits_columns() {
    let mut s = opened(&["with_header_tab.xlsx"]);
    s.add_rule(RuleRow { column: "Notes".into(), op: "is blank".into(), ..RuleRow::default() });
    assert_eq!(s.rules[0].sheet, ANY_SHEET);
    s.rules[0].sheet = "Summary".into();
    assert!(s.columns_for("Summary").contains(&"Notes".to_string()));
    assert!(s.load_sheet(0, "Query")); // Summary no longer loaded: its rules keep their sheet
    assert_eq!(s.rules[0].sheet, "Summary");
    assert_eq!(s.columns_for(ANY_SHEET), ["x", "y"]);
    assert_eq!(s.checked_rules().unwrap()[0].sheet, "Summary");
}

#[test]
fn app_icon_is_packaged() {
    assert!(imr_gui::icon().is_some());
    assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon.ico").exists());
}
