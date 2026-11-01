//! Python tests the golden fixtures don't cover, ported by hand: files the exports write, rule
//! files round-tripping, and the real sample workbook when it is present.

use std::path::Path;

use calamine::{Reader, open_workbook_auto};
use imr_core::report::Settings;
use imr_core::rules::{Rule, load_rules, save_rules};
use imr_core::{build_multi_job, export_html, export_xlsx, loader};

const GOLDEN_REL: &str = "../../tests/golden";

fn header_tab_job(settings: &Settings) -> imr_core::ChartJob {
    let path = Path::new(GOLDEN_REL).join("data/with_header_tab.xlsx");
    let frame = loader::load_table(&path, None).unwrap();
    build_multi_job(vec![(path, frame)], "QC3 CD34/Unit", Some("Date Processed"), None, None, settings)
        .unwrap()
}

#[test]
fn xlsx_file_has_every_sheet() {
    let dir = tempfile::tempdir().unwrap();
    let job = header_tab_job(&Settings { lsl: Some(2e7), ..Settings::default() });
    let path = dir.path().join("out").join("stats.xlsx");
    export_xlsx::save_xlsx(&job, &path).unwrap();
    let mut wb = open_workbook_auto(&path).unwrap();
    assert_eq!(
        wb.sheet_names(),
        [
            "Summary stats",
            "Verification",
            "Worked formulas",
            "Data",
            "Recompute",
            "Input audit",
            "Out of limits"
        ]
    );
    let data = wb.worksheet_range("Data").unwrap();
    assert_eq!(data.get_size(), (5, 11));
    let formulas = wb.worksheet_formula("Recompute").unwrap();
    let text: Vec<String> = formulas.rows().flatten().map(|f| f.to_string()).collect();
    assert!(text.iter().any(|f| f.contains("_xlfn.NUMBERVALUE(")), "NUMBERVALUE carries its prefix");
}

#[test]
fn html_file_embeds_plotly() {
    let dir = tempfile::tempdir().unwrap();
    let job = header_tab_job(&Settings::default());
    let path = dir.path().join("chart.html");
    export_html::save_html(&job, &path).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains(export_html::PLOTLY_JS));
    // A fresh element id each time, used in all three places.
    let id = html.split("<div id=\"").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(id.len(), 36);
    assert_eq!(html.matches(id).count(), 3);
}

#[test]
fn rules_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let rules = vec![
        Rule::new("a", "Notes", "contains", "retest", "#123456", true, "").unwrap(),
        Rule::new("", "Unit", ">", "5e7", "red", false, "Summary").unwrap(),
    ];
    let path = dir.path().join("rules.json");
    save_rules(&rules, &path).unwrap();
    assert_eq!(load_rules(&path).unwrap(), rules);
}

#[test]
fn real_workbook_when_present() {
    let path = Path::new("../../sheet.xlsx");
    if !path.exists() {
        return; // the local sample isn't in version control
    }
    let frame = loader::load_table(path, None).unwrap();
    assert!(loader::numeric_columns(&frame, 0.8).contains(&"QC3 CD34/Unit".to_string()));
    let job = build_multi_job(
        vec![(path.to_path_buf(), frame)],
        "QC3 CD34/Unit",
        Some("Date Processed"),
        None,
        Some("Summary"),
        &Settings { lsl: Some(2e7), ..Settings::default() },
    )
    .unwrap();
    assert!(job.result.n() > 100);
}
