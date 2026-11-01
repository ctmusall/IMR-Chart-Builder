//! Interactive I-MR chart, saved as a self-contained HTML file.
//!
//! The page embeds plotly.js (the same build the Python version inlined) and the same figure:
//! traces, limit lines and annotations, with JSON written the way plotly's Python library
//! writes it (typed arrays as base64, `<`, `>` and `/` escaped), so the file is identical apart
//! from the plot's random element id.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::NaiveDateTime;
use serde_json::{Map, Value, json};

use crate::error::{Error, Result};
use crate::plot::{
    LIMIT_COLOR, LSL_COLOR, MEAN_COLOR, POINT_COLOR, RECENT_BAND_COLOR, RECENT_COLOR, format_number,
    highlight_groups, looks_like_fraction, lsl_label, recent_mask, show_lcl,
};
use crate::pyfmt::{py_float_repr, py_format_g};
use crate::report::ChartJob;

/// plotly.js, as the Python version embedded it.
pub const PLOTLY_JS: &str = include_str!("../assets/plotly.min.js");
/// The `plotly_white` template, serialized as plotly writes it.
const TEMPLATE: &str = include_str!("../assets/plotly_white.json");

// --- JSON as plotly's Python library writes it -------------------------------------

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '/' => out.push_str("\\u002f"),
            c if (c as u32) < 0x20 || !c.is_ascii() => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_json(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => out.push_str(&i.to_string()),
            (_, Some(u)) => out.push_str(&u.to_string()),
            _ => out.push_str(&py_float_repr(n.as_f64().unwrap_or(f64::NAN))),
        },
        Value::String(s) => write_str(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(out, x);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(out, k);
                out.push(':');
                if k == "template" && x.is_null() {
                    out.push_str(TEMPLATE);
                } else {
                    write_json(out, x);
                }
            }
            out.push('}');
        }
    }
}

/// Plotly JSON text of a value.
pub fn plotly_json(v: &Value) -> String {
    let mut out = String::new();
    write_json(&mut out, v);
    out
}

/// A float, written as plotly writes Python floats (`55000000.0`); NaN as null.
fn float(f: f64) -> Value {
    serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
}

/// A float64 array as a plotly typed array (`{"dtype": "f8", "bdata": ...}`).
fn f8(values: &[f64]) -> Value {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    json!({"dtype": "f8", "bdata": STANDARD.encode(bytes)})
}

/// An int64 array as plotly stores it: in the smallest of int8 / int16 / int32 that fits.
fn ints(values: &[i64]) -> Value {
    let (min, max) = (values.iter().copied().min().unwrap_or(0), values.iter().copied().max().unwrap_or(0));
    let (dtype, bytes): (&str, Vec<u8>) = if min >= i8::MIN as i64 && max <= i8::MAX as i64 {
        ("i1", values.iter().flat_map(|v| (*v as i8).to_le_bytes()).collect())
    } else if min >= i16::MIN as i64 && max <= i16::MAX as i64 {
        ("i2", values.iter().flat_map(|v| (*v as i16).to_le_bytes()).collect())
    } else if min >= i32::MIN as i64 && max <= i32::MAX as i64 {
        ("i4", values.iter().flat_map(|v| (*v as i32).to_le_bytes()).collect())
    } else {
        return Value::Array(values.iter().map(|v| json!(v)).collect());
    };
    json!({"dtype": dtype, "bdata": STANDARD.encode(bytes)})
}

/// A pandas timestamp as plotly writes one on its own (`2024-04-18T00:00:00`).
fn timestamp(d: &NaiveDateTime) -> String {
    let s = d.format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
    s.strip_suffix(".000000").map(str::to_string).unwrap_or(s)
}

/// A timestamp in a date array: `2024-04-18T00:00:00.000000` for microsecond dates,
/// `2024-04-18T00:00:00.000000000` for nanosecond ones.
fn timestamp_array(d: &NaiveDateTime, micros: bool) -> String {
    if micros {
        d.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    } else {
        d.format("%Y-%m-%dT%H:%M:%S%.9f").to_string()
    }
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

// --- the figure --------------------------------------------------------------------

/// The plotly figure: `(data, layout)`. The layout's template is written separately.
pub fn build_figure(job: &ChartJob) -> (Value, Value) {
    let result = &job.result;
    let data = &job.data;
    let n = result.n();
    let percent = looks_like_fraction(&result.values);
    let has_dates = job.has_dates();
    let x: Value = if has_dates {
        Value::Array(
            data.iter()
                .map(|p| Value::String(timestamp_array(&p.date.expect("date"), job.dates_in_us)))
                .collect(),
        )
    } else {
        ints(&(1..=n as i64).collect::<Vec<_>>())
    };
    let vfmt = if percent { ".1%" } else { ".4~s" };
    let mut date_part = if has_dates { "%{x|%m/%d/%Y}".to_string() } else { "Obs %{x}".to_string() };
    let mut custom: Vec<Vec<Value>> = vec![data.iter().map(|p| label_value(&p.label)).collect()];
    if job.sources.len() > 1 {
        custom.push(data.iter().map(|p| Value::String(p.file.clone())).collect());
        date_part += &format!("<br>%{{customdata[{}]}}", custom.len() - 1);
    }
    let groups = highlight_groups(data);
    if !groups.is_empty() {
        custom.push(
            data.iter()
                .map(|p| {
                    let rule = p.rule.as_deref().unwrap_or("");
                    Value::String(if rule.is_empty() { String::new() } else { format!("<br>Rule: {rule}") })
                })
                .collect(),
        );
        date_part += &format!("%{{customdata[{}]}}", custom.len() - 1);
    }
    let customdata =
        Value::Array((0..n).map(|i| Value::Array(custom.iter().map(|c| c[i].clone()).collect())).collect());
    let mr = &result.moving_ranges;
    let recent = recent_mask(data, job.recent_days).filter(|m| m.iter().any(|b| *b));
    let base = |i: usize| -> &str {
        match &recent {
            Some(m) if m[i] => RECENT_COLOR,
            _ => POINT_COLOR,
        }
    };
    let marker = |flags: &[bool]| -> Value {
        if groups.is_empty() {
            let colors = (0..n).map(|i| Value::String(if flags[i] { LIMIT_COLOR } else { base(i) }.into()));
            return obj(vec![("color", Value::Array(colors.collect())), ("size", json!(6))]);
        }
        // Rule colors fill the point, so beyond-limit points are ringed in red instead of filled.
        let fill = (0..n).map(|i| {
            let h = data[i].highlight.as_deref().unwrap_or("");
            Value::String(if h.is_empty() { base(i).to_string() } else { h.to_string() })
        });
        let ring = (0..n).map(|i| Value::String(if flags[i] { LIMIT_COLOR } else { "rgba(0,0,0,0)" }.into()));
        let sizes: Vec<i64> = flags.iter().map(|f| if *f { 9 } else { 6 }).collect();
        obj(vec![
            ("color", Value::Array(fill.collect())),
            ("line", obj(vec![("color", Value::Array(ring.collect())), ("width", json!(2))])),
            ("size", ints(&sizes)),
        ])
    };
    let flagged: Vec<bool> = (0..n).map(|i| result.above_ucl[i] || result.below_lcl[i]).collect();
    let trace = |name: &str, y: Value, flags: &[bool], what: &str, axes: (&str, &str)| -> Value {
        obj(vec![
            ("customdata", customdata.clone()),
            (
                "hovertemplate",
                Value::String(format!(
                    "<b>%{{customdata[0]}}</b><br>{date_part}<br>{what}: %{{y:{vfmt}}}<extra></extra>"
                )),
            ),
            ("line", obj(vec![("color", json!("#9a9a9a")), ("width", json!(1))])),
            ("marker", marker(flags)),
            ("mode", json!("lines+markers")),
            ("name", Value::String(name.into())),
            ("showlegend", json!(false)),
            ("x", x.clone()),
            ("y", y),
            ("type", json!("scatter")),
            ("xaxis", Value::String(axes.0.into())),
            ("yaxis", Value::String(axes.1.into())),
        ])
    };
    let mut traces = vec![
        trace(&job.column, f8(&result.values), &flagged, "Value", ("x", "y")),
        trace("Moving Range", f8(mr), &result.mr_above_ucl, "MR", ("x2", "y2")),
    ];
    for g in &groups {
        traces.push(obj(vec![
            ("hoverinfo", json!("skip")),
            ("marker", obj(vec![("color", Value::String(g.color.clone())), ("size", json!(8))])),
            ("mode", json!("markers")),
            ("name", Value::String(format!("{} ({})", g.label, g.count()))),
            ("showlegend", json!(true)),
            ("x", json!([null])),
            ("y", json!([null])),
            ("type", json!("scatter")),
            ("xaxis", json!("x")),
            ("yaxis", json!("y")),
        ]));
    }

    let mut shapes: Vec<Value> = Vec::new();
    let mut annotations: Vec<Value> = Vec::new();
    let mut start: Option<NaiveDateTime> = None;
    if let Some(m) = &recent {
        let s =
            data.iter().zip(m).filter(|(_, r)| **r).filter_map(|(p, _)| p.date).min().expect("recent point");
        let end = data.last().and_then(|p| p.date).expect("dated");
        start = Some(s);
        for (xref, yref) in [("x", "y domain"), ("x2", "y2 domain")] {
            shapes.push(obj(vec![
                ("fillcolor", json!(RECENT_BAND_COLOR)),
                ("layer", json!("below")),
                ("line", obj(vec![("width", json!(0))])),
                ("opacity", json!(1)),
                ("type", json!("rect")),
                ("x0", Value::String(timestamp(&s))),
                ("x1", Value::String(timestamp(&end))),
                ("xref", json!(xref)),
                ("y0", json!(0)),
                ("y1", json!(1)),
                ("yref", json!(yref)),
            ]));
        }
        annotations.push(obj(vec![
            ("font", obj(vec![("color", json!(RECENT_COLOR)), ("size", json!(11))])),
            ("showarrow", json!(false)),
            ("text", Value::String(format!("<b>Last {} days</b>", job.recent_days))),
            ("x", Value::String(timestamp(&s))),
            ("xanchor", json!("left")),
            ("xref", json!("x")),
            ("y", json!(1)),
            ("yanchor", json!("top")),
            ("yref", json!("y domain")),
        ]));
    }
    let mut hline = |y: f64, color: &str, width: Value, text: String, row: u8, lsl: bool| {
        let (xref, yref) = if row == 1 { ("x domain", "y") } else { ("x2 domain", "y2") };
        shapes.push(obj(vec![
            ("line", obj(vec![("color", json!(color)), ("width", width)])),
            ("type", json!("line")),
            ("x0", json!(0)),
            ("x1", json!(1)),
            ("xref", json!(xref)),
            ("y0", float(y)),
            ("y1", float(y)),
            ("yref", json!(yref)),
        ]));
        annotations.push(obj(vec![
            ("font", obj(vec![("color", json!(color)), ("size", json!(if lsl { 12 } else { 11 }))])),
            ("showarrow", json!(false)),
            ("text", Value::String(text)),
            ("x", json!(if lsl { 0 } else { 1 })),
            ("xanchor", json!("left")),
            ("xref", json!(xref)),
            ("y", float(y)),
            ("yanchor", json!(if lsl { "bottom" } else { "middle" })),
            ("yref", json!(yref)),
        ]));
    };
    let two = || json!(2);
    hline(result.ucl, LIMIT_COLOR, two(), format!("UCL={}", format_number(result.ucl, percent)), 1, false);
    hline(result.mean, MEAN_COLOR, two(), format!("Avg={}", format_number(result.mean, percent)), 1, false);
    if show_lcl(result) {
        hline(
            result.lcl,
            LIMIT_COLOR,
            two(),
            format!("LCL={}", format_number(result.lcl, percent)),
            1,
            false,
        );
    }
    if let Some(lsl) = result.lsl {
        hline(lsl, LSL_COLOR, float(1.5), lsl_label(lsl, percent), 1, true);
    }
    hline(
        result.mr_ucl,
        LIMIT_COLOR,
        two(),
        format!("UCL={}", format_number(result.mr_ucl, percent)),
        2,
        false,
    );
    hline(
        result.mr_bar,
        MEAN_COLOR,
        two(),
        format!("Avg={}", format_number(result.mr_bar, percent)),
        2,
        false,
    );
    hline(result.mr_lcl, LIMIT_COLOR, two(), String::new(), 2, false);

    let tickformat = if percent { ".0%" } else { ".2e" };
    let date_axis = |pairs: &mut Vec<(&str, Value)>| {
        if has_dates {
            pairs.push(("tickformat", json!("%m/%d/%Y")));
            pairs.push(("tickangle", json!(-90)));
        }
    };
    let mut xaxis = vec![
        ("anchor", json!("y")),
        ("domain", json!([0.0, 1.0])),
        ("matches", json!("x2")),
        ("showticklabels", json!(false)),
    ];
    date_axis(&mut xaxis);
    let mut xaxis2 = vec![
        ("anchor", json!("y2")),
        ("domain", json!([0.0, 1.0])),
        (
            "title",
            obj(vec![(
                "text",
                Value::String(if has_dates {
                    job.date_column.clone().unwrap_or_default()
                } else {
                    "Observation".into()
                }),
            )]),
        ),
    ];
    date_axis(&mut xaxis2);
    let yaxis = |domain: Value, title: String| {
        obj(vec![
            ("anchor", json!(if domain[0] == json!(0.0) { "x2" } else { "x" })),
            ("domain", domain),
            ("title", obj(vec![("text", Value::String(title))])),
            ("tickformat", json!(tickformat)),
        ])
    };

    let mut subtitle = format!("n={n}, limits = mean ± {}σ (σ = MR̄/1.128)", py_format_g(result.k, 6));
    if let Some(s) = start {
        subtitle += &format!(" · highlighted: last {} days (from {})", job.recent_days, s.format("%m/%d/%Y"));
    }
    if job.sources.len() > 1 {
        subtitle += &format!(" · data from {} files", job.sources.len());
    }
    if !groups.is_empty() {
        subtitle +=
            &format!(" · {} points highlighted by rules", groups.iter().map(|g| g.count()).sum::<usize>());
    }
    if job.excluded > 0 {
        subtitle += &format!(" · {} rows excluded", job.excluded);
    }
    let layout = obj(vec![
        ("template", Value::Null),
        ("xaxis", obj(xaxis)),
        ("yaxis", yaxis(json!([0.424, 1.0]), job.column.clone())),
        ("xaxis2", obj(xaxis2)),
        ("yaxis2", yaxis(json!([0.0, 0.384]), format!("Moving Range({})", job.column))),
        ("shapes", Value::Array(shapes)),
        ("annotations", Value::Array(annotations)),
        (
            "title",
            obj(vec![
                (
                    "text",
                    Value::String(format!(
                        "<b>Individual & Moving Range chart of {}</b><br><sup>{subtitle}</sup>",
                        job.column
                    )),
                ),
                ("x", json!(0.5)),
            ]),
        ),
        (
            "legend",
            obj(vec![
                ("orientation", json!("h")),
                ("yanchor", json!("bottom")),
                ("y", json!(1.0)),
                ("xanchor", json!("right")),
                ("x", json!(1)),
            ]),
        ),
        ("margin", obj(vec![("r", json!(120))])),
        ("showlegend", json!(!groups.is_empty())),
        ("hovermode", json!("closest")),
        ("height", json!(800)),
    ]);
    (Value::Array(traces), layout)
}

fn label_value(c: &crate::cell::Cell) -> Value {
    if c.is_empty() { Value::Null } else { Value::String(c.py_str()) }
}

/// A random version-4 UUID for the plot's element id (as plotly uses).
fn element_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()),
        );
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// The whole HTML page, with `plotly_js` embedded and the plot in element `id`.
pub fn render_page(job: &ChartJob, id: &str, plotly_js: &str) -> String {
    let (data, layout) = build_figure(job);
    format!(
        "<!doctype html>\n<html>\n<head>\n    <meta charset=\"utf-8\" />\n    <style>html, body {{height: 100%;}}</style>\n</head>\n<body>\n    \
         <div style=\"height:800px; width:100%;\">                        \
         <script>window.PlotlyConfig = {{MathJaxConfig: 'local'}};</script>\n        \
         <script>{plotly_js}</script>                \
         <div id=\"{id}\" class=\"plotly-graph-div\" style=\"height:100%; width:100%;\"></div>            \
         <script>                window.PLOTLYENV=window.PLOTLYENV || {{}};                                \
         if (document.getElementById(\"{id}\")) {{                    Plotly.newPlot(                        \
         \"{id}\",                        {},                        {},                        \
         {{\"responsive\": true}}                    )                }};            </script>        </div>\n\
         </body>\n</html>",
        plotly_json(&data),
        plotly_json(&layout),
    )
}

/// Save the interactive chart.
pub fn save_html(job: &ChartJob, path: &std::path::Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::msg(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, render_page(job, &element_id(), PLOTLY_JS))
        .map_err(|e| Error::msg(format!("{}: {e}", path.display())))
}
