//! The command line against runs of the Python version (tests/golden/cli.json), plus its own checks.

use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

const GOLDEN_REL: &str = "../../tests/golden";

fn cli() -> Command {
    Command::cargo_bin("imr-chart-cli").unwrap()
}

#[test]
fn runs_match_python() {
    let text = std::fs::read_to_string(format!("{GOLDEN_REL}/cli.json")).unwrap();
    let cases: Vec<Value> = serde_json::from_str(&text).unwrap();
    let mut problems = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let out = tempfile::tempdir().unwrap();
        let out_dir = out.path().join(name);
        let args: Vec<String> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                a.as_str()
                    .unwrap()
                    .replace("{DIR}", GOLDEN_REL)
                    .replace("{OUT}", &out_dir.display().to_string())
            })
            .collect();
        let output = cli().args(&args).output().unwrap();
        let norm = |b: &[u8]| {
            String::from_utf8_lossy(b)
                .replace(&out_dir.display().to_string(), "{OUT}")
                .replace(GOLDEN_REL, "{DIR}")
                .replace(&format!("imr-chart {}", imr_core::VERSION), "imr-chart {VERSION}")
                .replace('\\', "/")
        };
        let code = output.status.code().unwrap_or(-1);
        if code != case["code"].as_i64().unwrap() as i32 {
            problems.push(format!("{name}: exit {code}, want {} ({})", case["code"], norm(&output.stderr)));
        }
        for (key, got) in [("stdout", norm(&output.stdout)), ("stderr", norm(&output.stderr))] {
            if got != case[key].as_str().unwrap() {
                problems.push(format!("{name}: {key}\n{got}\nwant:\n{}", case[key].as_str().unwrap()));
            }
        }
        let mut files: Vec<String> = std::fs::read_dir(&out_dir)
            .map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        files.sort();
        let want: Vec<String> =
            case["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().into()).collect();
        if files != want {
            problems.push(format!("{name}: files {files:?} != {want:?}"));
        }
    }
    if !problems.is_empty() {
        panic!("{} mismatch(es):\n{}", problems.len(), problems.join("\n"));
    }
}

#[test]
fn version_and_usage_errors() {
    let out = cli().arg("--version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("imr-chart {}", imr_core::VERSION));
    // No input file: argparse and clap both exit with 2.
    assert_eq!(cli().args(["-c", "x"]).output().unwrap().status.code(), Some(2));
}

#[test]
fn smoke_csv_writes_files() {
    let dir = tempfile::tempdir().unwrap();
    let smoke = Path::new(GOLDEN_REL).join("smoke/smoke.csv");
    cli()
        .arg(&smoke)
        .args(["-c", "Value", "--lsl", "2e7", "--formats", "xlsx,html", "-o"])
        .arg(dir.path())
        .assert()
        .success();
    for ext in ["xlsx", "html"] {
        let path = dir.path().join(format!("IMR_Value.{ext}"));
        assert!(std::fs::metadata(&path).unwrap().len() > 1000, "{ext} written");
    }
    let html = std::fs::read_to_string(dir.path().join("IMR_Value.html")).unwrap();
    assert!(html.contains("plotly.js v4.1.1"));
}

#[test]
fn gui_routing() {
    use std::ffi::OsString;
    let os = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
    assert!(imr_cli::wants_gui(&os(&[])));
    assert!(imr_cli::wants_gui(&os(&["a.xlsx", "b.xlsx"])));
    assert!(!imr_cli::wants_gui(&os(&["a.xlsx", "-c", "x"])));
}
