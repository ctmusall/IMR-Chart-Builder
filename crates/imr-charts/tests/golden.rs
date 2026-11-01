//! The chart against what matplotlib drew for the Python version (tests/golden/charts.json, by
//! generate_charts.py): panel positions, axis limits, ticks and labels, and every line, scatter,
//! band, annotation and legend entry. Numbers match to a relative 1e-9; the legend's place, which
//! depends on text widths, to 2% of the panel.

use std::path::PathBuf;

use imr_charts::figure::{Coord, Figure, HAlign, Panel, VAlign};
use imr_core::report::Settings;
use imr_core::rules::Rule;
use imr_core::{ChartJob, SourceTable, build_multi_job, build_sources_job, loader};
use serde_json::Value;

const GOLDEN_REL: &str = "../../tests/golden";

fn golden(rel: &str) -> PathBuf {
    PathBuf::from(format!("{GOLDEN_REL}/{rel}"))
}

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

fn job_from_spec(spec: &Value) -> ChartJob {
    let settings = Settings {
        lsl: spec["lsl"].as_f64(),
        k: spec["k"].as_f64().unwrap_or(3.0),
        recent_days: spec["recent_days"].as_i64().unwrap_or(30),
        rules: spec
            .get("rules")
            .map_or(Vec::new(), |r| r.as_array().unwrap().iter().map(rule_from).collect()),
    };
    if spec["kind"] == "multi" {
        let sheet = spec["sheet"].as_str();
        let tables = spec["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let path = golden(&format!("data/{}", f.as_str().unwrap()));
                let frame = loader::load_table(&path, sheet).unwrap();
                (path, frame)
            })
            .collect();
        return build_multi_job(
            tables,
            spec["column"].as_str().unwrap(),
            spec["date"].as_str(),
            spec["label"].as_str(),
            sheet,
            &settings,
        )
        .unwrap();
    }
    let sources = spec["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let path = golden(&format!("data/{}", s["file"].as_str().unwrap()));
            SourceTable {
                frame: loader::load_table(&path, s["sheet"].as_str()).unwrap(),
                path,
                column: s["column"].as_str().unwrap().into(),
                date_column: s["date"].as_str().map(str::to_string),
                sheet: s["sheet"].as_str().map(str::to_string),
                label_column: s["label"].as_str().map(str::to_string),
            }
        })
        .collect();
    build_sources_job(sources, &settings).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0) || (a.is_nan() && b.is_nan())
}

fn nums(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()
}

fn same_nums(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| close(*x, *y))
}

fn coord_name(c: &Coord) -> &'static str {
    match c {
        Coord::Data(_) => "data",
        Coord::Axes(_) => "axes fraction",
    }
}

fn coord_value(c: &Coord) -> f64 {
    match c {
        Coord::Data(v) | Coord::Axes(v) => *v,
    }
}

fn panel_problems(place: &str, p: &Panel, w: &Value, problems: &mut Vec<String>) {
    let mut check = |what: &str, ok: bool, got: String| {
        if !ok {
            problems.push(format!("{place}: {what} {got}"));
        }
    };
    check(
        "position",
        same_nums(&p.position, &nums(&w["position"])),
        format!("{:?} != {}", p.position, w["position"]),
    );
    check(
        "xlim",
        same_nums(&[p.xlim.0, p.xlim.1], &nums(&w["xlim"])),
        format!("{:?} != {}", p.xlim, w["xlim"]),
    );
    check(
        "ylim",
        same_nums(&[p.ylim.0, p.ylim.1], &nums(&w["ylim"])),
        format!("{:?} != {}", p.ylim, w["ylim"]),
    );
    let ticks = |t: &[(f64, String)]| t.iter().map(|(v, l)| serde_json::json!([v, l])).collect::<Vec<_>>();
    let same_ticks = |got: &[(f64, String)], want: &Value| {
        let want = want.as_array().unwrap();
        got.len() == want.len()
            && got
                .iter()
                .zip(want)
                .all(|((v, l), w)| close(*v, w[0].as_f64().unwrap()) && l == w[1].as_str().unwrap())
    };
    // The upper panel's x tick labels are hidden (shared axis); matplotlib reports none there.
    if !w["xticks"].as_array().unwrap().is_empty() {
        check(
            "xticks",
            same_ticks(&p.xticks, &w["xticks"]),
            format!("{:?} != {}", ticks(&p.xticks), w["xticks"]),
        );
    }
    check(
        "yticks",
        same_ticks(&p.yticks, &w["yticks"]),
        format!("{:?} != {}", ticks(&p.yticks), w["yticks"]),
    );
    check("xlabel", p.xlabel == w["xlabel"], format!("{:?} != {}", p.xlabel, w["xlabel"]));
    check("ylabel", p.ylabel == w["ylabel"], format!("{:?} != {}", p.ylabel, w["ylabel"]));
    check("x rotation", p.xrotation == w["xrotation"].as_f64().unwrap(), format!("{}", p.xrotation));
    check(
        "x tick labels shown",
        p.show_xtick_labels == w["xtick_labels_visible"].as_bool().unwrap(),
        format!("{}", p.show_xtick_labels),
    );

    let want_lines: Vec<&Value> =
        w["lines"].as_array().unwrap().iter().filter(|l| l["kind"] == "line").collect();
    let want_hlines: Vec<&Value> =
        w["lines"].as_array().unwrap().iter().filter(|l| l["kind"] == "hline").collect();
    check("lines", p.lines.len() == want_lines.len(), format!("{} != {}", p.lines.len(), want_lines.len()));
    for (l, wl) in p.lines.iter().zip(&want_lines) {
        let ok = same_nums(&l.x, &nums(&wl["x"]))
            && same_nums(&l.y, &nums(&wl["y"]))
            && l.color == wl["color"]
            && close(l.width, wl["width"].as_f64().unwrap())
            && l.zorder == wl["zorder"].as_f64().unwrap();
        check("line", ok, format!("{l:?}\n    != {wl}"));
    }
    check(
        "hlines",
        p.hlines.len() == want_hlines.len(),
        format!("{} != {}", p.hlines.len(), want_hlines.len()),
    );
    for (l, wl) in p.hlines.iter().zip(&want_hlines) {
        let ok = close(l.y, wl["y"][0].as_f64().unwrap())
            && l.color == wl["color"]
            && close(l.width, wl["width"].as_f64().unwrap())
            && l.zorder == wl["zorder"].as_f64().unwrap();
        check("hline", ok, format!("{l:?} != {wl}"));
    }
    let want_sc = w["scatters"].as_array().unwrap();
    check(
        "scatters",
        p.scatters.len() == want_sc.len(),
        format!("{} != {}", p.scatters.len(), want_sc.len()),
    );
    for (s, ws) in p.scatters.iter().zip(want_sc) {
        let face = ws["face"].as_array().unwrap().first().and_then(Value::as_str).map(str::to_string);
        let label = ws["label"].as_str().filter(|l| !l.starts_with('_')).map(str::to_string);
        let ok = same_nums(&s.x, &nums(&ws["x"]))
            && same_nums(&s.y, &nums(&ws["y"]))
            && s.face == face
            && Some(s.edge.as_str()) == ws["edge"][0].as_str()
            && close(s.size, ws["sizes"][0].as_f64().unwrap())
            && close(s.width, ws["width"][0].as_f64().unwrap())
            && s.zorder == ws["zorder"].as_f64().unwrap()
            && s.label == label;
        check("scatter", ok, format!("{s:?}\n    != {ws}"));
    }
    let want_bands = w["bands"].as_array().unwrap();
    check("bands", p.bands.len() == want_bands.len(), format!("{} != {}", p.bands.len(), want_bands.len()));
    for (b, wb) in p.bands.iter().zip(want_bands) {
        let ok = close(b.x0, wb["x0"].as_f64().unwrap())
            && close(b.x1, wb["x1"].as_f64().unwrap())
            && b.color == wb["color"];
        check("band", ok, format!("{b:?} != {wb}"));
    }
    let want_texts = w["texts"].as_array().unwrap();
    check("texts", p.texts.len() == want_texts.len(), format!("{} != {}", p.texts.len(), want_texts.len()));
    for (t, wt) in p.texts.iter().zip(want_texts) {
        let ha = match t.ha {
            HAlign::Left => "left",
            HAlign::Center => "center",
        };
        let va = match t.va {
            VAlign::Top => "top",
            VAlign::Center => "center",
            VAlign::Bottom => "bottom",
        };
        let ok = t.text == wt["text"]
            && close(coord_value(&t.x), wt["xy"][0].as_f64().unwrap())
            && close(coord_value(&t.y), wt["xy"][1].as_f64().unwrap())
            && coord_name(&t.x) == wt["xycoords"][0]
            && coord_name(&t.y) == wt["xycoords"][1]
            && same_nums(&[t.offset.0, t.offset.1], &nums(&wt["offset"]))
            && ha == wt["ha"]
            && va == wt["va"]
            && t.color == wt["color"]
            && t.size == wt["size"].as_f64().unwrap()
            && (if t.bold { "bold" } else { "normal" }) == wt["weight"]
            && t.boxed == wt["box"].as_bool().unwrap()
            && t.zorder == wt["zorder"].as_f64().unwrap();
        check("text", ok, format!("{t:?}\n    != {wt}"));
    }
    match (&p.legend, w["legend"].is_null()) {
        (None, true) => {}
        (Some(l), false) => {
            let labels: Vec<&str> = l.entries.iter().map(|(t, _)| t.as_str()).collect();
            let colors: Vec<&str> = l.entries.iter().map(|(_, c)| c.as_str()).collect();
            let wl = &w["legend"];
            check(
                "legend labels",
                serde_json::json!(labels) == wl["labels"],
                format!("{labels:?} != {}", wl["labels"]),
            );
            check(
                "legend colors",
                serde_json::json!(colors) == wl["colors"],
                format!("{colors:?} != {}", wl["colors"]),
            );
            let wb = nums(&wl["bbox"]);
            let near = l.bbox.iter().zip(&wb).all(|(a, b)| (a - b).abs() < 0.02);
            check("legend place", near, format!("{:?} != {wb:?}", l.bbox));
        }
        (got, _) => check("legend", false, format!("{got:?} != {}", w["legend"])),
    }
}

fn figure_problems(name: &str, f: &Figure, w: &Value, problems: &mut Vec<String>) {
    if f.title.text != w["title"] || f.title.size != w["title_size"].as_f64().unwrap() {
        problems.push(format!("{name}: title {:?}", f.title.text));
    }
    let note = &w["note"];
    if f.note.text != note["text"]
        || f.note.size != note["size"].as_f64().unwrap()
        || f.note.color != note["color"]
    {
        problems.push(format!("{name}: note {:?}\n    != {}", f.note.text, note["text"]));
    }
    if !same_nums(&[f.size.0, f.size.1], &nums(&w["size"])) {
        problems.push(format!("{name}: size {:?}", f.size));
    }
    for (i, (p, wp)) in f.panels.iter().zip(w["panels"].as_array().unwrap()).enumerate() {
        panel_problems(&format!("{name} panel {}", i + 1), p, wp, problems);
    }
}

#[test]
fn charts_match_matplotlib() {
    let text = std::fs::read_to_string(golden("charts.json")).unwrap();
    let cases: Vec<Value> = serde_json::from_str(&text).unwrap();
    let mut problems = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let job = job_from_spec(&case["spec"]);
        figure_problems(name, &imr_charts::figure(&job), &case["chart"], &mut problems);
    }
    assert!(cases.len() >= 20);
    if !problems.is_empty() {
        panic!("{} mismatch(es):\n{}", problems.len(), problems.join("\n"));
    }
}
