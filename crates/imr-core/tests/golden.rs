//! Parity tests against fixtures generated from the original Python implementation by
//! tests/golden/generate.py. Each test collects every mismatch before failing, so one run shows
//! them all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use imr_core::cell::{Cell, Column, DType};
use imr_core::export_html::render_page;
use imr_core::export_xlsx::{CfRule, SheetModel, XValue, column_letter, stored_number, workbook_model};
use imr_core::plot::{format_number, highlight_groups, looks_like_fraction, lsl_label, recent_mask};
use imr_core::proof::{StepValue, worked_steps};
use imr_core::report::{ChartJob, Settings, SourceTable, Value as PValue, resolve, safe_filename};
use imr_core::rules::{Rule, check_columns, evaluate, load_rules, rules_json};
use imr_core::{
    Error, ImrResult, VERSION, build_multi_job, build_sources_job, compute_imr, loader, parse_limit,
};
use serde_json::Value;

/// The fixtures, relative to this crate (cargo runs tests from the crate folder), written with
/// forward slashes so paths read the same on every OS.
const GOLDEN_REL: &str = "../../tests/golden";

fn golden(rel: &str) -> PathBuf {
    PathBuf::from(format!("{GOLDEN_REL}/{rel}"))
}

fn read(name: &str) -> Value {
    let text = std::fs::read_to_string(golden(name)).expect("golden file");
    serde_json::from_str(&text).expect("golden JSON")
}

fn cell(v: &Value) -> Cell {
    match v {
        Value::Null => Cell::Empty,
        Value::Bool(b) => Cell::Bool(*b),
        Value::Number(n) if n.is_f64() => Cell::Float(n.as_f64().unwrap()),
        Value::Number(n) => Cell::Int(n.as_i64().unwrap()),
        Value::String(s) => Cell::Text(s.clone()),
        Value::Object(o) if o.contains_key("d") => Cell::Date(date(&o["d"])),
        Value::Object(o) => match o["f"].as_str().unwrap() {
            "inf" => Cell::Float(f64::INFINITY),
            _ => Cell::Float(f64::NEG_INFINITY),
        },
        other => panic!("bad golden cell {other}"),
    }
}

fn date(v: &Value) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(v.as_str().unwrap(), "%Y-%m-%dT%H:%M:%S").unwrap()
}

fn cells(v: &Value) -> Vec<Cell> {
    v.as_array().unwrap().iter().map(cell).collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

fn bools(v: &Value) -> Vec<bool> {
    v.as_array().unwrap().iter().map(|b| b.as_bool().unwrap()).collect()
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

/// Same value, counting an int and a float with the same value as equal (Excel stores numbers as floats).
///
/// A cell of only spaces, written without Excel's keep-spaces flag (as openpyxl writes it), reads
/// as blank (calamine follows the XML rule); both count as blank everywhere.
fn same_number(a: &Cell, b: &Cell) -> bool {
    match (a, b) {
        (Cell::Int(i), Cell::Float(f)) | (Cell::Float(f), Cell::Int(i)) => *i as f64 == *f,
        (Cell::Empty, Cell::Text(s)) | (Cell::Text(s), Cell::Empty) => s.trim().is_empty() && !s.is_empty(),
        _ => a == b,
    }
}

fn same_cells(a: &[Cell], b: &[Cell]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_number(x, y))
}

fn num_cell(v: Option<f64>) -> Cell {
    v.map_or(Cell::Empty, Cell::float)
}

fn date_cell(v: Option<NaiveDateTime>) -> Cell {
    v.map_or(Cell::Empty, Cell::Date)
}

fn finish(problems: Vec<String>) {
    if !problems.is_empty() {
        panic!("{} mismatch(es):\n{}", problems.len(), problems.join("\n"));
    }
}

fn error_text(e: &Error) -> String {
    e.to_string()
}

/// The program's paths and version written the way the fixtures write them.
fn normalize(text: &str) -> String {
    let abs = resolve(Path::new(GOLDEN_REL)).display().to_string();
    text.replace(&abs, "{DIR}")
        .replace(GOLDEN_REL, "{DIR}")
        .replace(&format!("imr-chart {VERSION}"), "imr-chart {VERSION}")
        .replace('\\', "/")
}

// --- loader ------------------------------------------------------------------------

#[test]
fn loader_matches_python() {
    let doc = read("loader.json");
    let mut problems = Vec::new();
    for f in doc["files"].as_array().unwrap() {
        let file = f["file"].as_str().unwrap();
        let path = golden(&format!("data/{file}"));
        let sheets = loader::list_sheets(&path).unwrap();
        if sheets != strings(&f["sheets"]) {
            problems.push(format!("{file}: sheets {sheets:?} != {}", f["sheets"]));
        }
        if let Some(default) = f.get("default") {
            let got = loader::load_table(&path, None).unwrap().names();
            if got != strings(default) {
                problems.push(format!("{file}: default sheet columns {got:?} != {default}"));
            }
        }
        for (sheet, want) in f["tables"].as_object().unwrap() {
            let place = format!("{file} [{sheet}]");
            let got = loader::load_table(&path, if loader::is_csv(&path) { None } else { Some(sheet) });
            if let Some(err) = want.get("error") {
                match got {
                    Err(e) if normalize(&e.to_string()) == err.as_str().unwrap() => {}
                    other => problems.push(format!("{place}: want error {err}, got {other:?}")),
                }
                continue;
            }
            let frame = match got {
                Ok(f) => f,
                Err(e) => {
                    problems.push(format!("{place}: error {e}"));
                    continue;
                }
            };
            if frame.names() != strings(&want["columns"]) {
                problems.push(format!("{place}: columns {:?} != {}", frame.names(), want["columns"]));
                continue;
            }
            let index: Vec<i64> =
                want["index"].as_array().unwrap().iter().map(|i| i.as_i64().unwrap()).collect();
            if frame.index != index {
                problems.push(format!("{place}: index {:?} != {index:?}", frame.index));
            }
            for c in &frame.columns {
                let expect = cells(&want["cells"][&c.name]);
                if !same_cells(&c.cells, &expect) {
                    problems.push(format!("{place} {}: {:?} != {expect:?} ({:?})", c.name, c.cells, c.dtype));
                }
                let numbers: Vec<Cell> = loader::to_numeric(c).into_iter().map(num_cell).collect();
                let expect = cells(&want["to_numeric"][&c.name]);
                if !same_cells(&numbers, &expect) {
                    problems.push(format!("{place} to_numeric {}: {numbers:?} != {expect:?}", c.name));
                }
                if let Some(expect) = want["to_datetime"].get(&c.name).filter(|e| e.get("crash").is_none()) {
                    let got: Vec<Cell> = loader::to_datetime(c).into_iter().map(date_cell).collect();
                    if got != cells(expect) {
                        problems.push(format!("{place} to_datetime {}: {got:?} != {expect}", c.name));
                    }
                }
            }
            for (key, got) in [
                ("numeric_columns", loader::numeric_columns(&frame, 0.8)),
                ("date_columns", loader::date_columns(&frame)),
            ] {
                if got != strings(&want[key]) {
                    problems.push(format!("{place}: {key} {got:?} != {}", want[key]));
                }
            }
            if loader::pick_date_column(&frame) != opt_str(&want["pick_date"]) {
                problems.push(format!(
                    "{place}: pick_date {:?} != {}",
                    loader::pick_date_column(&frame),
                    want["pick_date"]
                ));
            }
            if loader::pick_label_column(&frame) != opt_str(&want["pick_label"]) {
                problems.push(format!("{place}: pick_label {:?}", loader::pick_label_column(&frame)));
            }
        }
    }
    match loader::load_table(golden("data/missing.xlsx"), None) {
        Err(e) if normalize(&e.to_string()) == doc["missing"]["error"].as_str().unwrap() => {}
        other => problems.push(format!("missing file: {other:?}")),
    }
    finish(problems);
}

// --- scalars -----------------------------------------------------------------------

#[test]
fn scalars_match_python() {
    let doc = read("scalars.json");
    let mut problems = Vec::new();
    for case in doc["parse_limit"].as_array().unwrap() {
        let input = case["in"].as_str().unwrap();
        match (parse_limit(input), case.get("ok")) {
            (Ok(got), Some(want)) => {
                let want =
                    if want.is_null() && input.trim() == "nan" { Some(f64::NAN) } else { cell_f64(want) };
                let same = match (got, want) {
                    (Some(a), Some(b)) => a == b || (a.is_nan() && b.is_nan()),
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    problems.push(format!("parse_limit({input:?}) = {got:?}, want {want:?}"));
                }
            }
            (Err(e), None) if e.to_string() == case["error"].as_str().unwrap() => {}
            (got, _) => problems.push(format!("parse_limit({input:?}) = {got:?}, want {case}")),
        }
    }
    for case in doc["to_numeric"].as_array().unwrap() {
        let col = Column::inferred("c", cells(&case["in"]));
        let got: Vec<Cell> = loader::to_numeric(&col).into_iter().map(num_cell).collect();
        if !same_cells(&got, &cells(&case["out"])) {
            problems.push(format!("to_numeric({}) = {got:?}, want {}", case["in"], case["out"]));
        }
    }
    for case in doc["to_datetime"].as_array().unwrap() {
        if case.get("crash").is_some() {
            continue; // the Python version crashed here; nothing to match
        }
        let col = Column::inferred("c", cells(&case["in"]));
        let got: Vec<Cell> = loader::to_datetime(&col).into_iter().map(date_cell).collect();
        if got != cells(&case["out"]) {
            problems.push(format!("to_datetime({}) = {got:?}, want {}", case["in"], case["out"]));
        }
    }
    for case in doc["format_number"].as_array().unwrap() {
        let x = case["x"].as_f64().unwrap();
        let p = case["percent"].as_bool().unwrap();
        let got = format_number(x, p);
        if got != case["out"].as_str().unwrap() {
            problems.push(format!("format_number({x}, {p}) = {got}, want {}", case["out"]));
        }
    }
    for case in doc["lsl_label"].as_array().unwrap() {
        let x = case["x"].as_f64().unwrap();
        let p = case["percent"].as_bool().unwrap();
        let got = lsl_label(x, p);
        if got != case["out"].as_str().unwrap() {
            problems.push(format!("lsl_label({x}, {p}) = {got}, want {}", case["out"]));
        }
    }
    for case in doc["looks_like_fraction"].as_array().unwrap() {
        let values: Vec<f64> =
            cells(&case["in"]).iter().map(|c| cell_f64_of(c).unwrap_or(f64::NAN)).collect();
        if looks_like_fraction(&values) != case["out"].as_bool().unwrap() {
            problems.push(format!("looks_like_fraction({})", case["in"]));
        }
    }
    for case in doc["safe_filename"].as_array().unwrap() {
        let got = safe_filename(case["in"].as_str().unwrap());
        if got != case["out"].as_str().unwrap() {
            problems.push(format!("safe_filename({}) = {got}", case["in"]));
        }
    }
    for case in doc["colors"].as_array().unwrap() {
        let c = case["in"].as_str().unwrap();
        if c.starts_with("xkcd:") {
            continue; // the xkcd color names are not supported (see the README)
        }
        match (Rule::new("", "c", "=", "", c, true, ""), case.get("ok")) {
            (Ok(r), Some(want)) if r.color == want.as_str().unwrap() => {}
            (Err(e), None) if e.to_string() == case["error"].as_str().unwrap() => {}
            (got, _) => problems.push(format!("color {c:?}: {got:?}, want {case}")),
        }
    }
    for case in doc["rules"].as_array().unwrap() {
        let i = &case["in"];
        let s = |k: &str| match &i[k] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let color = i.get("color").and_then(Value::as_str).unwrap_or("#9467bd");
        let got = Rule::new(s("name"), s("column"), s("op"), s("value"), color, true, s("sheet"));
        match (got, case.get("ok")) {
            (Ok(r), Some(want)) => {
                let got = [r.label(), r.describe(), r.color.clone(), r.value.clone(), r.sheet.clone()];
                let want = ["label", "describe", "color", "value", "sheet"]
                    .map(|k| want[k].as_str().unwrap().to_string());
                if got != want {
                    problems.push(format!("rule {i}: {got:?} != {want:?}"));
                }
            }
            (Err(e), None) if e.to_string() == case["error"].as_str().unwrap() => {}
            (got, _) => problems.push(format!("rule {i}: {got:?}, want {case}")),
        }
    }
    for case in doc["common_columns"].as_array().unwrap() {
        let lists: Vec<Vec<String>> = case["in"].as_array().unwrap().iter().map(strings).collect();
        if loader::common_columns(&lists) != strings(&case["out"]) {
            problems.push(format!("common_columns({})", case["in"]));
        }
    }
    finish(problems);
}

fn cell_f64(v: &Value) -> Option<f64> {
    cell_f64_of(&cell(v))
}

fn cell_f64_of(c: &Cell) -> Option<f64> {
    match c {
        Cell::Int(i) => Some(*i as f64),
        Cell::Float(f) => Some(*f),
        _ => None,
    }
}

// --- statistics --------------------------------------------------------------------

fn result_problems(place: &str, r: &ImrResult, want: &Value, problems: &mut Vec<String>) {
    let floats = |v: &[f64]| -> Vec<Cell> { v.iter().map(|f| Cell::float(*f)).collect() };
    if !same_cells(&floats(&r.values), &cells(&want["values"])) {
        problems.push(format!("{place}: values {:?}", r.values));
    }
    if !same_cells(&floats(&r.moving_ranges), &cells(&want["moving_ranges"])) {
        problems.push(format!("{place}: moving ranges {:?}", r.moving_ranges));
    }
    for (key, got) in [
        ("k", r.k),
        ("mean", r.mean),
        ("mr_bar", r.mr_bar),
        ("sigma", r.sigma),
        ("ucl", r.ucl),
        ("lcl", r.lcl),
        ("mr_ucl", r.mr_ucl),
        ("mr_lcl", r.mr_lcl),
    ] {
        if Some(got) != want[key].as_f64() {
            problems.push(format!("{place}: {key} {got:?} != {}", want[key]));
        }
    }
    if r.lsl != want["lsl"].as_f64() {
        problems.push(format!("{place}: lsl {:?} != {}", r.lsl, want["lsl"]));
    }
    for (key, got) in [
        ("above_ucl", &r.above_ucl),
        ("below_lcl", &r.below_lcl),
        ("below_lsl", &r.below_lsl),
        ("mr_above_ucl", &r.mr_above_ucl),
    ] {
        if *got != bools(&want[key]) {
            problems.push(format!("{place}: {key} {got:?} != {}", want[key]));
        }
    }
    if r.out_of_limits() != bools(&want["out_of_limits"]) {
        problems.push(format!("{place}: out_of_limits"));
    }
    let steps = worked_steps(r);
    let want_steps = want["worked"].as_array().unwrap();
    if steps.len() != want_steps.len() {
        problems.push(format!("{place}: {} worked steps, want {}", steps.len(), want_steps.len()));
    }
    for (s, w) in steps.iter().zip(want_steps) {
        let value = match s.value {
            StepValue::Int(i) => Cell::Int(i),
            StepValue::Float(f) => Cell::float(f),
        };
        if [s.quantity, s.formula, s.numbers.as_str()]
            != [w[0].as_str().unwrap(), w[1].as_str().unwrap(), w[2].as_str().unwrap()]
            || !same_number(&value, &cell(&w[3]))
        {
            problems.push(format!("{place}: worked step {s:?} != {w}"));
        }
    }
}

#[test]
fn statistics_match_python() {
    let mut problems = Vec::new();
    for case in read("stats.json").as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let values: Vec<f64> =
            case["values"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        let got = compute_imr(&values, case["k"].as_f64().unwrap(), case["lsl"].as_f64());
        match (got, case.get("result")) {
            (Ok(r), Some(want)) => result_problems(name, &r, want, &mut problems),
            (Err(e), None) if e.to_string() == case["error"].as_str().unwrap() => {}
            (got, _) => problems.push(format!("{name}: {got:?}")),
        }
    }
    finish(problems);
}

// --- highlight rules ---------------------------------------------------------------

fn rule_from(v: &Value) -> Rule {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let color = v.get("color").and_then(Value::as_str).unwrap_or("#9467bd");
    Rule::new(
        s("name"),
        s("column"),
        s("op"),
        s("value"),
        color,
        v.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        s("sheet"),
    )
    .unwrap()
}

#[test]
fn rules_match_python() {
    let doc = read("rules.json");
    let mut problems = Vec::new();
    let mut checked = 0;
    for case in doc["cases"].as_array().unwrap() {
        if case.get("crash").is_some() {
            continue; // the Python version crashed here; nothing to match
        }
        let series = &doc["series"][case["series"].as_str().unwrap()];
        let mut column = Column::inferred("c", cells(&series["cells"]));
        if series["datetime"].as_bool().unwrap() {
            column.dtype = DType::Date;
        }
        let frame = imr_core::Frame { index: (0..column.len() as i64).collect(), columns: vec![column] };
        let (op, value) = (case["op"].as_str().unwrap(), case["value"].as_str().unwrap());
        let rule = Rule::simple("", "c", op, value).unwrap();
        let place = format!("{} {op} {value:?}", case["series"]);
        match (rule.matches(&frame), case.get("hits")) {
            (Ok(got), Some(want)) if got == bools(want) => {}
            (Err(e), None) if e.to_string() == case["error"].as_str().unwrap() => {}
            (got, _) => problems
                .push(format!("{place}: {got:?}, want {}", case.get("hits").unwrap_or(&case["error"]))),
        }
        checked += 1;
    }
    assert!(checked > 1000, "only {checked} rule cases");

    let frame = imr_core::Frame {
        index: (1..=6).collect(),
        columns: vec![
            Column::inferred(
                "QC Lot",
                ["L3", "L1", "L2", "L4", "L5", "L6"].map(|s| Cell::Text(s.into())).to_vec(),
            ),
            Column::inferred(
                "Unit",
                vec![
                    Cell::Float(7e7),
                    Cell::Float(5e7),
                    Cell::Float(9e7),
                    Cell::Empty,
                    Cell::Text("bad".into()),
                    Cell::Float(1e7),
                ],
            ),
            Column::inferred(
                "Notes",
                vec![
                    Cell::Text("Retest".into()),
                    Cell::Text("ok".into()),
                    Cell::Text("RETEST later".into()),
                    Cell::Empty,
                    Cell::Text("".into()),
                    Cell::Text("fine".into()),
                ],
            ),
        ],
    };
    for case in doc["evaluate"].as_array().unwrap() {
        let rules: Vec<Rule> = case["rules"].as_array().unwrap().iter().map(rule_from).collect();
        let got = evaluate(&rules, &frame, case["sheet"].as_str());
        match (got, case.get("ok")) {
            (Ok(got), Some(want))
                if got
                    == want.as_array().unwrap().iter().map(|i| i.as_i64().unwrap()).collect::<Vec<_>>() => {}
            (Err(e), None) if e.to_string() == case["error"].as_str().unwrap() => {}
            (got, _) => problems.push(format!("evaluate {case}: {got:?}")),
        }
    }
    for case in doc["check_columns"].as_array().unwrap() {
        let rules: Vec<Rule> = case["rules"].as_array().unwrap().iter().map(rule_from).collect();
        let tables: Vec<(Option<String>, Vec<String>)> =
            case["tables"].as_array().unwrap().iter().map(|t| (opt_str(&t[0]), strings(&t[1]))).collect();
        match (check_columns(&rules, &tables), case.get("error")) {
            (Ok(()), None) => {}
            (Err(e), Some(want)) if e.to_string() == want.as_str().unwrap() => {}
            (got, _) => problems.push(format!("check_columns {case}: {got:?}")),
        }
    }
    let dir = tempfile::tempdir().unwrap();
    for case in doc["load"].as_array().unwrap() {
        let path = dir.path().join("rules.json");
        std::fs::write(&path, case["content"].as_str().unwrap()).unwrap();
        let name = case["name"].as_str().unwrap();
        match (load_rules(&path), case.get("ok")) {
            (Ok(got), Some(want)) => {
                let want: Vec<Rule> = want.as_array().unwrap().iter().map(rule_from).collect();
                if got != want {
                    problems.push(format!("load {name}: {got:?} != {want:?}"));
                }
            }
            (Err(e), None) => {
                let msg = e.to_string().replace(&path.display().to_string(), "{FILE}");
                let want = case["error"].as_str().unwrap();
                // JSON syntax errors are worded by each JSON reader; the start is what matters.
                let same = if name == "bad_json" {
                    msg.starts_with("Rules file {FILE} is not valid JSON: ")
                } else {
                    msg == want
                };
                if !same {
                    problems.push(format!("load {name}: {msg:?} != {want:?}"));
                }
            }
            (got, _) => problems.push(format!("load {name}: {got:?}")),
        }
    }
    let missing = dir.path().join("nope.json");
    match load_rules(&missing) {
        Err(e)
            if e.to_string().replace(&missing.display().to_string(), "{FILE}")
                == doc["missing"].as_str().unwrap() => {}
        other => problems.push(format!("missing rules file: {other:?}")),
    }
    let saved = rules_json(&[
        Rule::new("a", "Notes", "contains", "retest", "#123456", true, "").unwrap(),
        Rule::new("", "Unit", ">", "5e7", "#9467bd", false, "Data").unwrap(),
        Rule::new("Ünïcode › name", "Notes", "is blank", "", "#9467bd", true, "").unwrap(),
    ]);
    if saved != doc["saved"].as_str().unwrap() {
        problems.push(format!("saved rules:\n{saved}\nwant:\n{}", doc["saved"].as_str().unwrap()));
    }
    finish(problems);
}

// --- chart jobs --------------------------------------------------------------------

fn settings(spec: &Value) -> Settings {
    Settings {
        lsl: spec["lsl"].as_f64(),
        k: spec["k"].as_f64().unwrap_or(3.0),
        recent_days: spec["recent_days"].as_i64().unwrap_or(30),
        rules: spec
            .get("rules")
            .map_or(Vec::new(), |r| r.as_array().unwrap().iter().map(rule_from).collect()),
    }
}

/// Build the job the way generate.py did.
pub fn job_from_spec(spec: &Value) -> imr_core::Result<ChartJob> {
    let s = settings(spec);
    if spec["kind"] == "multi" {
        let sheet = spec["sheet"].as_str();
        let tables = spec["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let path = golden(&format!("data/{}", f.as_str().unwrap()));
                let frame = loader::load_table(&path, sheet)?;
                Ok((path, frame))
            })
            .collect::<imr_core::Result<Vec<_>>>()?;
        return build_multi_job(
            tables,
            spec["column"].as_str().unwrap(),
            spec["date"].as_str(),
            spec["label"].as_str(),
            sheet,
            &s,
        );
    }
    let sources = spec["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|src| {
            let path = golden(&format!("data/{}", src["file"].as_str().unwrap()));
            let frame = loader::load_table(&path, src["sheet"].as_str())?;
            Ok(SourceTable {
                path,
                frame,
                column: src["column"].as_str().unwrap().into(),
                date_column: opt_str(&src["date"]),
                sheet: opt_str(&src["sheet"]),
                label_column: opt_str(&src["label"]),
            })
        })
        .collect::<imr_core::Result<Vec<_>>>()?;
    build_sources_job(sources, &s)
}

fn text_cell(s: &str) -> Cell {
    Cell::Text(normalize(s))
}

fn opt_text(s: &Option<String>) -> Cell {
    s.as_deref().map_or(Cell::Empty, text_cell)
}

fn check_column(place: &str, key: &str, got: Vec<Cell>, want: &Value, problems: &mut Vec<String>) {
    let want = cells(&want[key]);
    if !same_cells(&got, &want) {
        problems.push(format!("{place} {key}: {got:?}\n    != {want:?}"));
    }
}

fn xvalue_cell(v: &XValue) -> Cell {
    match v {
        XValue::Empty => Cell::Empty,
        XValue::Int(i) => Cell::Int(*i),
        XValue::Number(f) => Cell::Float(stored_number(*f)),
        XValue::Bool(b) => Cell::Bool(*b),
        XValue::Text(s) | XValue::Formula(s) => text_cell(s),
        XValue::Date(d) => Cell::Date(d.and_hms_opt(0, 0, 0).unwrap()),
        XValue::DateTime(d) => Cell::Date(*d),
    }
}

fn hex6(golden: &Value) -> Option<u32> {
    golden.as_str().and_then(|s| u32::from_str_radix(&s[s.len() - 6..], 16).ok())
}

fn sheet_problems(place: &str, got: &SheetModel, want: &Value, problems: &mut Vec<String>) {
    let rows = want["rows"].as_array().unwrap();
    let (nr, nc) = (got.max_row().map_or(0, |r| r + 1), got.max_col().map_or(0, |c| c + 1));
    if nr as usize != rows.len() || rows.first().is_some_and(|r| r.as_array().unwrap().len() != nc as usize) {
        problems.push(format!(
            "{place}: {nr}x{nc} cells, want {}x{}",
            rows.len(),
            rows[0].as_array().unwrap().len()
        ));
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.as_array().unwrap().iter().enumerate() {
            let g = xvalue_cell(got.value(r as u32, c as u16));
            if !same_number(&g, &cell(v)) {
                problems.push(format!("{place} {}{}: {g:?} != {v}", column_letter(c as u16), r + 1));
            }
        }
    }
    // Styles: fills, fonts and number formats.
    let mut got_styles: BTreeMap<String, Value> = BTreeMap::new();
    for (&(r, c), (_, s)) in &got.cells {
        let mut m = serde_json::Map::new();
        if let Some(f) = s.fill {
            m.insert("fill".into(), Value::from(f));
        }
        if s.bold || s.italic || s.font_color.is_some() || s.font_size.is_some() {
            m.insert(
                "font".into(),
                serde_json::json!({"b": s.bold, "i": s.italic, "color": s.font_color, "sz": s.font_size}),
            );
        }
        if let Some(n) = s.num_format {
            m.insert("format".into(), Value::from(n));
        }
        if !m.is_empty() {
            got_styles.insert(format!("{}{}", column_letter(c), r + 1), Value::Object(m));
        }
    }
    let mut want_styles: BTreeMap<String, Value> = BTreeMap::new();
    for (k, s) in want["styles"].as_object().unwrap() {
        let mut m = serde_json::Map::new();
        if let Some(f) = s.get("fill") {
            m.insert("fill".into(), Value::from(hex6(f).unwrap()));
        }
        if let Some(f) = s.get("font") {
            m.insert(
                "font".into(),
                serde_json::json!({"b": f["b"], "i": f["i"], "color": hex6(&f["color"]), "sz": f["sz"]}),
            );
        }
        if let Some(n) = s.get("format") {
            m.insert("format".into(), n.clone());
        }
        want_styles.insert(k.clone(), Value::Object(m));
    }
    for (k, w) in &want_styles {
        if got_styles.get(k) != Some(w) {
            problems.push(format!("{place} {k}: style {:?} != {w}", got_styles.get(k)));
        }
    }
    for k in got_styles.keys().filter(|k| !want_styles.contains_key(*k)) {
        problems.push(format!("{place} {k}: extra style {}", got_styles[k]));
    }
    let widths: BTreeMap<String, f64> = got.widths.iter().map(|(c, w)| (column_letter(*c), *w)).collect();
    let want_widths: BTreeMap<String, f64> =
        want["widths"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap())).collect();
    // A column holding this folder's path is as wide as the path is long, which differs by checkout.
    let has_path = |letter: &str| {
        rows.iter().any(|r| {
            r.as_array().unwrap().iter().enumerate().any(|(c, v)| {
                column_letter(c as u16) == letter && v.as_str().is_some_and(|s| s.contains("{DIR}"))
            })
        })
    };
    for (letter, w) in &want_widths {
        if widths.get(letter) != Some(w) && !has_path(letter) {
            problems.push(format!("{place}: width of {letter} {:?} != {w}", widths.get(letter)));
        }
    }
    if widths.len() != want_widths.len() {
        problems.push(format!("{place}: widths {widths:?} != {want_widths:?}"));
    }
    if got.freeze != (want["freeze"] == "A2") {
        problems.push(format!("{place}: freeze {} != {}", got.freeze, want["freeze"]));
    }
    let want_cf = want["cf"].as_array().unwrap();
    if got.cf.len() != want_cf.len() {
        problems.push(format!("{place}: {} conditional formats, want {}", got.cf.len(), want_cf.len()));
    }
    for (g, w) in got.cf.iter().zip(want_cf) {
        let (r0, c0, r1, c1) = g.range;
        let range = if (r0, c0) == (r1, c1) {
            format!("{}{}", column_letter(c0), r0 + 1)
        } else {
            format!("{}{}:{}{}", column_letter(c0), r0 + 1, column_letter(c1), r1 + 1)
        };
        let (kind, formula) = match &g.rule {
            CfRule::Equal(v) => ("cellIs", v.clone()),
            CfRule::Expression(f) => ("expression", f.clone()),
        };
        if range != w["range"]
            || kind != w["type"]
            || formula != w["formula"][0]
            || Some(g.fill) != hex6(&w["fill"])
        {
            problems.push(format!("{place}: conditional format {g:?} != {w}"));
        }
    }
}

#[test]
fn jobs_match_python() {
    let mut problems = Vec::new();
    for case in read("jobs.json").as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let job = match (job_from_spec(&case["spec"]), case.get("error")) {
            (Ok(job), None) => job,
            (Err(e), Some(want)) => {
                if normalize(&error_text(&e)) != want.as_str().unwrap() {
                    problems.push(format!("{name}: error {e:?} != {want}"));
                }
                continue;
            }
            (got, _) => {
                problems.push(format!("{name}: {:?}", got.map(|j| j.result.n())));
                continue;
            }
        };
        let a = &job.audit;
        let audit = &case["audit"];
        check_column(name, "row", a.iter().map(|r| Cell::Int(r.row)).collect(), audit, &mut problems);
        check_column(name, "label", a.iter().map(|r| r.label.clone()).collect(), audit, &mut problems);
        check_column(name, "raw_date", a.iter().map(|r| r.raw_date.clone()).collect(), audit, &mut problems);
        check_column(
            name,
            "raw_value",
            a.iter().map(|r| r.raw_value.clone()).collect(),
            audit,
            &mut problems,
        );
        check_column(name, "date", a.iter().map(|r| date_cell(r.date)).collect(), audit, &mut problems);
        check_column(name, "value", a.iter().map(|r| num_cell(r.value)).collect(), audit, &mut problems);
        check_column(name, "file", a.iter().map(|r| text_cell(&r.file)).collect(), audit, &mut problems);
        check_column(
            name,
            "reason",
            a.iter().map(|r| Cell::Text(r.reason.clone())).collect(),
            audit,
            &mut problems,
        );
        check_column(
            name,
            "included",
            a.iter().map(|r| Cell::Bool(r.included())).collect(),
            audit,
            &mut problems,
        );
        if audit.get("rule").is_some() != job.has_rules() {
            problems.push(format!("{name}: has rules {}", job.has_rules()));
        }
        if audit.get("rule").is_some() {
            check_column(name, "rule", a.iter().map(|r| opt_text(&r.rule)).collect(), audit, &mut problems);
            check_column(
                name,
                "highlight",
                a.iter().map(|r| opt_text(&r.highlight)).collect(),
                audit,
                &mut problems,
            );
        }
        let d = &job.data;
        let data = &case["data"];
        check_column(name, "date", d.iter().map(|p| date_cell(p.date)).collect(), data, &mut problems);
        check_column(name, "label", d.iter().map(|p| p.label.clone()).collect(), data, &mut problems);
        check_column(name, "value", d.iter().map(|p| Cell::Float(p.value)).collect(), data, &mut problems);
        check_column(name, "row", d.iter().map(|p| Cell::Int(p.row)).collect(), data, &mut problems);
        check_column(name, "file", d.iter().map(|p| text_cell(&p.file)).collect(), data, &mut problems);
        if data.get("rule").is_some() {
            check_column(name, "rule", d.iter().map(|p| opt_text(&p.rule)).collect(), data, &mut problems);
            check_column(
                name,
                "highlight",
                d.iter().map(|p| opt_text(&p.highlight)).collect(),
                data,
                &mut problems,
            );
        }
        result_problems(name, &job.result, &case["result"], &mut problems);
        let scalars = [
            ("excluded", serde_json::json!(job.excluded)),
            ("column", serde_json::json!(job.column)),
            ("date_column", serde_json::json!(job.date_column)),
            ("sheet", serde_json::json!(job.sheet)),
            ("label_column", serde_json::json!(job.label_column)),
            ("highlighted", serde_json::json!(job.highlighted())),
            ("stem", serde_json::json!(job.default_stem())),
            ("sha256", serde_json::json!(job.sha256s)),
            ("percent", serde_json::json!(looks_like_fraction(&job.result.values))),
            ("recent", serde_json::json!(recent_mask(&job.data, job.recent_days))),
        ];
        for (key, got) in scalars {
            if got != case[key] {
                problems.push(format!("{name}: {key} {got} != {}", case[key]));
            }
        }
        // The Python version crashed counting duplicates of several files in row order.
        if case["duplicates"] != "crash" && serde_json::json!(job.duplicates()) != case["duplicates"] {
            problems.push(format!("{name}: duplicates {} != {}", job.duplicates(), case["duplicates"]));
        }
        let groups: Vec<Value> = highlight_groups(&job.data)
            .into_iter()
            .map(|g| serde_json::json!([g.label, g.color, g.mask]))
            .collect();
        if Value::Array(groups.clone()) != case["groups"] {
            problems.push(format!("{name}: groups {groups:?} != {}", case["groups"]));
        }
        let prov: Vec<Value> = job
            .provenance("{RUN_AT}")
            .into_iter()
            .map(|(k, v)| match v {
                PValue::Text(s) => serde_json::json!([k, normalize(&s)]),
                PValue::Int(i) => serde_json::json!([k, i]),
            })
            .collect();
        if Value::Array(prov.clone()) != case["provenance"] {
            problems.push(format!("{name}: provenance {prov:?}\n    != {}", case["provenance"]));
        }
        let sheets = workbook_model(&job, "{RUN_AT}");
        let want_sheets = case["xlsx"].as_array().unwrap();
        let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
        let want_names: Vec<&str> = want_sheets.iter().map(|s| s["name"].as_str().unwrap()).collect();
        if names != want_names {
            problems.push(format!("{name}: sheets {names:?} != {want_names:?}"));
        }
        for (got, want) in sheets.iter().zip(want_sheets) {
            sheet_problems(&format!("{name} [{}]", got.name), got, want, &mut problems);
        }
        let html = normalize_html(&render_page(&job, "{ID}", "{PLOTLYJS}"));
        let want = std::fs::read_to_string(golden(&format!("html/{name}.html"))).unwrap();
        if html != want {
            let at = html
                .bytes()
                .zip(want.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(html.len().min(want.len()));
            let from = at.saturating_sub(120);
            problems.push(format!(
                "{name}: HTML differs at byte {at}:\n    got  …{}\n    want …{}",
                html.get(from..(at + 120).min(html.len())).unwrap_or(""),
                want.get(from..(at + 120).min(want.len())).unwrap_or("")
            ));
        }
    }
    finish(problems);
}

/// Paths in the page (JSON-escaped) written the way the fixtures write them.
fn normalize_html(text: &str) -> String {
    let rel = GOLDEN_REL.replace('/', "\\u002f");
    text.replace(&rel, "{DIR}")
}
