//! Every fixture chart drawn to PNG. Set IMR_CHART_DIR to keep the images to look at
//! (`just charts` saves them to target/charts/).

use std::path::PathBuf;

use imr_core::report::Settings;
use imr_core::{build_multi_job, loader};

#[test]
fn charts_draw_as_png() {
    let keep = std::env::var_os("IMR_CHART_DIR").map(PathBuf::from);
    let dir = tempfile::tempdir().unwrap();
    let out = keep.clone().unwrap_or_else(|| dir.path().to_path_buf());
    let data = PathBuf::from("../../tests/golden/data");
    type Case<'a> = (&'a str, &'a [&'a str], &'a str, Option<&'a str>, Option<f64>);
    let cases: [Case; 4] = [
        ("header_tab", &["with_header_tab.xlsx"], "QC3 CD34/Unit", Some("Date Processed"), Some(2e7)),
        ("viability", &["inline.xlsx"], "QC3 CD34 Viability", Some("Date Processed"), Some(0.5)),
        ("rows", &["noheader.xlsx"], "Column 2", None, None),
        ("pooled", &["dup_a.xlsx", "dup_b.xlsx"], "QC3 CD34/Unit", Some("Date Processed"), Some(2e7)),
    ];
    for (name, files, column, date, lsl) in cases {
        let tables =
            files.iter().map(|f| (data.join(f), loader::load_table(data.join(f), None).unwrap())).collect();
        let settings = Settings { lsl, ..Settings::default() };
        let job = build_multi_job(tables, column, date, None, None, &settings).unwrap();
        let path = out.join(format!("{name}.png"));
        imr_charts::save_png(&job, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"), "{name} is a PNG");
        // 11 x 8.5 inches at 200 dpi.
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 2200, "{name} width");
        assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 1700, "{name} height");
    }
}
