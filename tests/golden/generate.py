"""Generate the golden fixtures the Rust port is tested against, from the Python implementation.

Run from the repo root while the Python package still exists:

    .venv/bin/python tests/golden/generate.py

It writes input files under tests/golden/data/ and tests/golden/smoke/, and the expected results
as JSON (plus normalized HTML) under tests/golden/. Cells are encoded as JSON: null for blank,
true/false, integers, floats (always with a decimal point or exponent), strings,
{"d": "YYYY-MM-DDTHH:MM:SS"} for timestamps and {"f": "inf"} / {"f": "-inf"}. Paths under this
folder are written as {DIR}, the run time as {RUN_AT} and the program version as {VERSION}.
Kept after the Python code is removed, for reference.
"""

from __future__ import annotations

import contextlib
import io
import json
import math
import re
import shutil
import sys
import tempfile
from datetime import date, datetime
from pathlib import Path

import numpy as np
import pandas as pd

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT))

import openpyxl  # noqa: E402

from imr_chart import __version__, cli, loader, plot_html, plot_static, proof, report  # noqa: E402
from imr_chart.rules import OPERATORS, Rule, check_columns, evaluate, load_rules, save_rules  # noqa: E402
from imr_chart.stats import compute_imr, parse_limit  # noqa: E402

DATA = HERE / "data"
SMOKE = HERE / "smoke"
HTML = HERE / "html"


def tag(v):
    if v is None or v is pd.NaT or v is pd.NA:
        return None
    if isinstance(v, (bool, np.bool_)):
        return bool(v)
    if isinstance(v, (int, np.integer)):
        return int(v)
    if isinstance(v, (float, np.floating)):
        f = float(v)
        if math.isnan(f):
            return None
        if math.isinf(f):
            return {"f": "inf" if f > 0 else "-inf"}
        return f
    if isinstance(v, str):
        return v
    if isinstance(v, (pd.Timestamp, datetime, np.datetime64)):
        if pd.isna(v):
            return None
        return {"d": pd.Timestamp(v).strftime("%Y-%m-%dT%H:%M:%S")}
    if isinstance(v, date):
        return {"d": v.strftime("%Y-%m-%dT00:00:00")}
    raise TypeError(f"can't tag {type(v)}: {v!r}")


def cells(values) -> list:
    if isinstance(values, pd.Series):
        values = values.tolist()
    return [tag(v) for v in values]


def dump(name: str, doc):
    (HERE / name).write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")


def normalize(text: str, run_at: str | None = None) -> str:
    if run_at:
        text = text.replace(run_at, "{RUN_AT}")
    return (text.replace(str(HERE) + "/", "{DIR}/").replace(str(HERE), "{DIR}")
            .replace(f"imr-chart {__version__}", "imr-chart {VERSION}"))


def attempt(fn, *args, **kw):
    try:
        return {"ok": fn(*args, **kw)}
    except (ValueError, KeyError, FileNotFoundError) as exc:
        return {"error": exc.args[0] if isinstance(exc, KeyError) and exc.args else str(exc),
                "kind": type(exc).__name__}


# --- input files ---------------------------------------------------------------------

HEADERS = ["QC Lot", "Date Processed", "QC3 CD34/Unit", "QC3 CD34 Viability", "Notes"]
ROWS = [
    ["L3", 45400, 7.0e7, 0.90, "a"],
    ["L1", 45363, 5.0e7, 0.80, "b"],
    ["L2", 45383, 9.0e7, 0.95, "c"],
    ["L4", 45410, None, 0.70, "d"],
    ["L5", 45420, "bad", 0.40, "e"],
    ["L6", 45430, 1.0e7, 0.60, "f"],
]


def summary_workbook(path: Path, rows):
    with pd.ExcelWriter(path) as xw:
        pd.DataFrame(rows).to_excel(xw, sheet_name="Summary", header=False, index=False)
        pd.DataFrame([HEADERS]).to_excel(xw, sheet_name="Column Headers (Summary)", header=False, index=False)


def qc_frame(seed: int, n: int, start: str, lot_prefix: str) -> pd.DataFrame:
    """A realistic QC export: datetime dates (a few blank), large counts, fractions, notes."""
    rng = np.random.default_rng(seed)
    days = np.sort(rng.integers(0, 3 * n, n))
    dates = pd.Timestamp(start) + pd.to_timedelta(days, unit="D")
    cd34 = np.round(rng.normal(6e7, 1.8e7, n), -3)
    cd34[rng.integers(0, n)] = 2.4e8  # one far above the limits
    viab = np.round(rng.uniform(0.82, 0.99, n), 4)
    notes = rng.choice(["", "ok", "Retest", "retest pending", "QC hold", None], n)
    df = pd.DataFrame({
        "QC Lot": [f"{lot_prefix}-{1000 + i}" for i in range(n)],
        "ISBT DIN": [f"W4375{24000700 + 3 * i}" for i in range(n)],
        "Date Processed": list(dates),
        "Quarter": [f"Q{(d.month - 1) // 3 + 1}" for d in dates],
        "QC3 CD34/Unit": cd34.astype(object),
        "QC3 CD34 Viability": viab,
        "QC2 CFU (GM&GEMM)": rng.integers(20, 90, n),
        "Notes": notes,
    })
    df.loc[4, "Date Processed"] = None
    df.loc[7, "QC3 CD34/Unit"] = "pending"
    df.loc[9, "QC3 CD34/Unit"] = None
    df.loc[11, "QC3 CD34/Unit"] = "5.5E7"
    return df


def write_inputs():
    if DATA.exists():
        shutil.rmtree(DATA)
    (DATA / "other").mkdir(parents=True)
    SMOKE.mkdir(parents=True, exist_ok=True)
    with pd.ExcelWriter(DATA / "with_header_tab.xlsx") as xw:
        pd.DataFrame([["tag"]]).to_excel(xw, sheet_name="MC Tags", header=False, index=False)
        pd.DataFrame([["x", "y"], [1, 2]]).to_excel(xw, sheet_name="Query", header=False, index=False)
        pd.DataFrame(ROWS).to_excel(xw, sheet_name="Summary", header=False, index=False)
        pd.DataFrame([HEADERS]).to_excel(xw, sheet_name="Column Headers (Summary)", header=False, index=False)
    pd.DataFrame(ROWS, columns=HEADERS).to_excel(DATA / "inline.xlsx", sheet_name="Data", index=False)
    pd.DataFrame([[1, 2.5], [2, 3.5], [3, 1.5]]).to_excel(DATA / "noheader.xlsx", header=False, index=False)
    df = pd.DataFrame(ROWS, columns=HEADERS)
    df["Date Processed"] = ["2024-04-18", "2024-03-12", "2024-04-01", "2024-04-28", "2024-05-08", "2024-05-18"]
    df.to_csv(DATA / "data.csv", index=False)
    summary_workbook(DATA / "part1.xlsx", ROWS[:3])
    summary_workbook(DATA / "part2.xlsx", ROWS[3:])
    summary_workbook(DATA / "other" / "part1.xlsx", [["L7", 45440, 6.0e7, 0.85, "g"], ["L8", 45450, 6.5e7, 0.9, "h"]])
    (DATA / "messy.csv").write_text(
        "QC Lot,Date Processed,CD34\n"
        "L3,2024-04-18,7.0e7\nL1,2024-03-12,5.0e7\n\nL2,2024-04-01,9.0e7\nL4,2024-04-28,\n"
        "L5,2024-05-08,bad\nL6,2024-05-18,1.0e7\nL7,,4.0e7\nL8,2024-05-20,50%\n"
        'L9,2024-05-21,"1,000"\nL10,2024-05-22,2.6e8\n', encoding="utf-8")
    (DATA / "tabbed.tsv").write_text(
        "Lot\tRun Date\tValue\tNotes\nA\t01/05/2024\t10.5\tx\nB\t01/06/2024\t11\t\n"
        "C\t01/09/2024\t 9.75 \tretest\nD\t\t12\ty\nE\t01/12/2024\t13.25\tz\n", encoding="utf-8")
    (DATA / "semicolon.csv").write_text(
        "Lot;Date;Value\nA;2024-02-01;1.5\nB;2024-02-03;2.5\nC;2024-02-02;2.0\n", encoding="utf-8")
    (DATA / "edge.csv").write_text(
        "A,A,,B\n1,2,3,4\n5,6,7,8\n,,,\n9,10,11,x\n", encoding="utf-8")
    with pd.ExcelWriter(DATA / "dates.xlsx") as xw:
        pd.DataFrame({
            "Lot": ["D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "D9", "D10", "D11", "D12"],
            "Run Date": [datetime(2024, 1, 5), 45300, "2024-01-07", "1/8/2024", "Jan 9, 2024", "2024-01-10 13:00",
                         "pending", None, "  ", "2024-01-14T08:30:00", "01/15/24", 45306.5],
            "Value": [1, 2.5, "3", " 4 ", "50%", "1,000", "abc", 8, None, "", "9e1", -2],
            "Flag": [True, False, True, True, False, True, False, True, False, True, False, True],
        }).to_excel(xw, sheet_name="Runs", index=False)
        pd.DataFrame({"Only": ["x", None, "y"]}).to_excel(xw, sheet_name="Text only", index=False)
        pd.DataFrame().to_excel(xw, sheet_name="Empty", index=False)
    with pd.ExcelWriter(DATA / "qc_summary.xlsx") as xw:
        main = qc_frame(7, 60, "2025-06-02", "QC")
        main.to_excel(xw, sheet_name="Summary", header=False, index=False)
        pd.DataFrame([list(main.columns)]).to_excel(xw, sheet_name="Column Headers (Summary)", header=False,
                                                    index=False)
        arch = qc_frame(11, 25, "2025-01-06", "AR").rename(columns={
            "QC Lot": "Lot Number", "Date Processed": "Processing Date", "QC3 CD34/Unit": "CD34 per unit"})
        arch.to_excel(xw, sheet_name="Archive", index=False)
        pd.DataFrame([["tag"]]).to_excel(xw, sheet_name="MC Tags", header=False, index=False)
    a = qc_frame(3, 20, "2025-03-03", "DP")
    with pd.ExcelWriter(DATA / "dup_a.xlsx") as xw:
        a.iloc[:14].to_excel(xw, sheet_name="Summary", index=False)
    with pd.ExcelWriter(DATA / "dup_b.xlsx") as xw:
        a.iloc[10:].to_excel(xw, sheet_name="Summary", index=False)
    (SMOKE / "smoke.csv").write_text("Lot,Date,Value\nA,2024-01-01,5e7\nB,2024-01-02,6e7\nC,2024-01-03,4e7\n"
                                     "D,2024-01-04,7e7\n", encoding="utf-8")


LOADER_FILES = ["with_header_tab.xlsx", "inline.xlsx", "noheader.xlsx", "data.csv", "part1.xlsx", "part2.xlsx",
                "other/part1.xlsx", "messy.csv", "tabbed.tsv", "semicolon.csv", "edge.csv", "dates.xlsx",
                "qc_summary.xlsx", "dup_a.xlsx", "dup_b.xlsx"]


def frame_doc(df: pd.DataFrame) -> dict:
    return {"columns": [str(c) for c in df.columns], "index": [int(i) for i in df.index],
            "cells": {str(c): cells(df[c]) for c in df.columns},
            "numeric_columns": loader.numeric_columns(df), "date_columns": loader.date_columns(df),
            "pick_date": loader.pick_date_column(df), "pick_label": loader.pick_label_column(df),
            "to_numeric": {str(c): cells(loader.to_numeric(df[c])) for c in df.columns},
            "to_datetime": {str(c): datetime_cells(df[c]) for c in df.columns
                            if str(c) in loader.date_columns(df) or str(c) in ("Value", "Lot")}}


def datetime_cells(series: pd.Series):
    try:
        return cells(loader.to_datetime(series))
    except Exception as exc:  # noqa: BLE001 - the Python version crashed here; nothing to match
        return {"crash": type(exc).__name__}


def loader_golden():
    out = []
    for name in LOADER_FILES:
        path = DATA / name
        sheets = loader.list_sheets(path)
        doc = {"file": name, "sheets": sheets, "tables": {}}
        targets = list(sheets)
        if name == "with_header_tab.xlsx":
            targets += ["MC Tags", "Column Headers (Summary)", "Nope"]
        for sheet in targets:
            try:
                doc["tables"][sheet] = frame_doc(loader.load_table(path, None if loader.is_csv(path) else sheet))
            except (ValueError, KeyError) as exc:
                doc["tables"][sheet] = {"error": normalize(str(exc))}
        if not loader.is_csv(path):
            doc["default"] = [str(c) for c in loader.load_table(path).columns]
        out.append(doc)
    missing = attempt(loader.load_table, DATA / "missing.xlsx")
    dump("loader.json", {"files": out, "missing": {"error": normalize(missing["error"])}})


# --- scalars -------------------------------------------------------------------------

def datetime_case(values):
    try:
        return {"in": cells(values), "out": cells(loader.to_datetime(pd.Series(values)))}
    except Exception as exc:  # noqa: BLE001 - the Python version crashed here; nothing to match
        return {"in": cells(values), "crash": type(exc).__name__}


def scalar_golden():
    limits = ["2e7", "20,000,000", "50%", " 7 ", "", "   ", "abc", "1e", "-3.5", "1_000", "inf", "nan", "12%%",
              "5 %", "0x10", "+4", ".5", "1.", "1,2,3", "%"]
    numbers = [[1, 2.5, "3", " 4 ", "50%", "1,000", "abc", None, "", "9e1", "-2", "1_0", "5 %", "12.5%", " 7%",
                "inf", "nan", "1e400", True]]
    numbers += [[1, 2, 3], [1.5, None, 3.0], ["a", "b"], [None, None]]
    dates_in = [["2024-01-05", "1/8/2024", "Jan 9, 2024", "2024-01-10 13:00", "pending", None, "  ", 45300,
                 45306.5, "45307", "0", 0, -1, 2958466, "2024-01-14T08:30:00", "01/15/24", "9 Jan 2024",
                 "2024/02/03", "20240105", "13/01/2024", "2024-13-01", "Monday, January 8, 2024",
                 "2024-01-05 1:30 PM", "January 2024", "2024"]]
    dates_in += [[45300, 45301], [datetime(2024, 1, 5), None, "2024-02-01"]]
    numbers_fmt = [0, 1, -1, 0.5, 1234.5678, 12345.678, 99999, 1e5, 123456, -123456, 6.01103869e7, 1e-3, 9.99e-4,
                   -0.0005, 2.5e-7, 1e10, 3.14159, 100, 0.123456, 1e5 - 0.01, 1.005e8, 9.995e7, 0.95, 0.9049, 1.5,
                   12.5, 1234567.0]
    lsls = [2e7, 2e5, 1e5, 99999, 0.5, 0.001, 0.0009, 5e-4, 0, 1.5e7, 2.5e8, -2e7, 123456, 0.75, 10, 1e10, 3e-5]
    fractions = [[0.5, 0.9], [0, 1.5], [0, 1.51], [-0.1, 0.5], [], [5e7, 6e7], [1.0], [0.2, float("nan"), 0.4]]
    colors = ["#9467bd", "red", "blue", "#FF00AA", "#abc", "#ABCD", "#11223344", "tab:orange", "C1", "r", "k",
              "0.5", "darkgreen", "DarkGreen", "xkcd:sky blue", "notacolor", "#12345", "", "  red  ", "grey",
              "rebeccapurple", "none", "1.5", "#1234567"]
    names = ["QC3 CD34/Unit", "a  b", "__x__", "", "/", "Viability (%)", "naïve", "a.b-c_d", "...", "CD34 % ok"]
    rule_cases = []
    for kw in [{"name": "r", "column": "Notes", "op": "=", "value": "x"},
               {"name": "", "column": "Notes", "op": "contains", "value": "x"},
               {"name": "", "column": "Notes", "op": "is blank", "value": "ignored"},
               {"name": "", "column": "Notes", "op": "=", "value": None, "sheet": " Summary "},
               {"name": "", "column": "Unit", "op": ">", "value": 5, "sheet": None},
               {"name": "", "column": "Notes", "op": "~"},
               {"name": "r", "column": "Notes", "op": "~"},
               {"name": "r", "column": "", "op": "="},
               {"name": "", "column": "", "op": "="},
               {"name": "r", "column": "Notes", "op": "=", "color": "notacolor"},
               {"name": "", "column": "Notes", "op": "=", "value": "v", "color": "bad", "sheet": "S"}]:
        try:
            r = Rule(**kw)
            rule_cases.append({"in": kw, "ok": {"label": r.label, "describe": r.describe(), "color": r.color,
                                                "value": r.value, "sheet": r.sheet}})
        except ValueError as exc:
            rule_cases.append({"in": kw, "error": str(exc)})
    doc = {
        "parse_limit": [{"in": s, **{k: tag(v) if k == "ok" else v for k, v in attempt(parse_limit, s).items()}}
                        for s in limits],
        "to_numeric": [{"in": cells(v), "out": cells(loader.to_numeric(pd.Series(v)))} for v in numbers],
        "to_datetime": [datetime_case(v) for v in dates_in + [[x] for x in dates_in[0]]],
        "format_number": [{"x": x, "percent": p, "out": plot_static.format_number(x, p)}
                          for x in numbers_fmt for p in (False, True)],
        "lsl_label": [{"x": x, "percent": p, "out": plot_static.lsl_label(x, p)} for x in lsls for p in (False, True)],
        "looks_like_fraction": [{"in": cells(v), "out": bool(plot_static.looks_like_fraction(v))} for v in fractions],
        "safe_filename": [{"in": s, "out": report.safe_filename(s)} for s in names],
        "colors": [{"in": c, **attempt(lambda c: Rule("", "c", "=", "", color=c).color, c)} for c in colors],
        "rules": rule_cases,
        "common_columns": [{"in": v, "out": loader.common_columns(v)}
                           for v in [[], [["a", "b", "c"]], [["a", "b", "c"], ["c", "a"]], [["a"], ["b"]]]],
    }
    dump("scalars.json", doc)


# --- statistics ----------------------------------------------------------------------

def result_doc(r) -> dict:
    return {"n": r.n, "values": cells(r.values), "moving_ranges": cells(r.moving_ranges), "k": r.k,
            "mean": r.mean, "mr_bar": r.mr_bar, "sigma": r.sigma, "ucl": r.ucl, "lcl": r.lcl, "mr_ucl": r.mr_ucl,
            "mr_lcl": r.mr_lcl, "lsl": r.lsl, "above_ucl": [bool(b) for b in r.above_ucl],
            "below_lcl": [bool(b) for b in r.below_lcl], "below_lsl": [bool(b) for b in r.below_lsl],
            "mr_above_ucl": [bool(b) for b in r.mr_above_ucl], "out_of_limits": [bool(b) for b in r.out_of_limits],
            "worked": [[q, f, num, tag(v)] for q, f, num, v in proof.worked_steps(r)]}


def stats_golden():
    rng = np.random.default_rng(1)
    series = {
        "five": ([10, 12, 11, 15, 9], 3.0, None),
        "flags": ([1, 1, 1, 1, 1, 1, 1, 1, 10, 1], 3.0, 0.5),
        "lsl": ([5e7, 6e7, 7e7, 1e7, 9e7], 3.0, 2e7),
        "k2": ([0.9, 0.8, 0.95, 0.7, 0.4, 0.6], 2.0, 0.5),
        "two": ([1.0, 2.0], 3.0, None),
        "constant": ([3.0, 3.0, 3.0], 3.0, None),
        "long": (list(np.round(rng.normal(6e7, 2e7, 300), 3)), 3.0, 2e7),
        "long_odd": (list(rng.normal(0.9, 0.05, 137)), 2.5, 0.8),
        "pairwise": (list(rng.uniform(0, 1e9, 1000) * 1.000000001), 3.0, None),
        "k_frac": ([1.1, 2.2, 3.3, 2.2, 1.1, 0.1], 1.5, 0.0),
        "one": ([1.0], 3.0, None),
        "nan": ([1.0, float("nan"), 2.0], 3.0, None),
        "k0": ([1.0, 2.0], 0.0, None),
        "kneg": ([1.0, 2.0], -1.0, None),
    }
    out = []
    for name, (values, k, lsl) in series.items():
        case = {"name": name, "values": cells(values), "k": k, "lsl": lsl}
        try:
            case["result"] = result_doc(compute_imr(values, k=k, lsl=lsl))
        except ValueError as exc:
            case["error"] = str(exc)
        out.append(case)
    dump("stats.json", out)


# --- highlight rules -----------------------------------------------------------------

SERIES = {
    "TEXT": ["Retest", "ok", "RETEST later", None, "", "fine", "  Spaced  ", "Straße", "1,000", "50%"],
    "NUM": [7.0e7, 5.0e7, 9.0e7, None, "bad", 1.0e7, "6e7", "50%", " 3 ", 0],
    "FRAC": [0.9, 0.8, 0.95, 0.7, 0.4, 0.6],
    "INT": [1, 2, 3, 10, 100, 45400],
    "SERIAL": [45400, 45363, 45383, 45410, 45420, 45430],
    "DATES": [datetime(2024, 4, 18), datetime(2024, 3, 12), None, datetime(2024, 4, 1, 13, 30)],
    "DATESTR": ["2024-04-18", "03/12/2024", "pending", None, "Apr 1, 2024", "2024-04-01 13:00"],
    "BOOL": [True, False, True],
}
RULE_VALUES = ["", "l2", "retest", "RE", "ine", "5e7", "50,000,000", "70%", "0.7", "2024-04-18", "04/01/2024",
               "abc", "ok", "1,000", "0", "45400", "true", "straße", "STRASSE", " spaced ", "1", "Apr 1, 2024",
               "2024-04-01 13:00", "nan", "-1", "%"]


def rules_golden():
    cases = []
    for sname, values in SERIES.items():
        df = pd.DataFrame({"c": pd.Series(values)})
        if sname == "DATES":
            df = pd.DataFrame({"c": pd.Series(pd.to_datetime(values))})
        for op in OPERATORS:
            for value in RULE_VALUES:
                if op in ("is blank", "is not blank") and value:
                    continue
                try:
                    hits = [bool(b) for b in Rule("", "c", op, value).matches(df).fillna(False)]
                    cases.append({"series": sname, "op": op, "value": value, "hits": hits})
                except ValueError as exc:
                    cases.append({"series": sname, "op": op, "value": value, "error": str(exc)})
                except Exception as exc:  # noqa: BLE001 - the Python version crashed here; nothing to match
                    cases.append({"series": sname, "op": op, "value": value, "crash": type(exc).__name__})
    dfs = {"DATES": "datetime"}
    series = {k: {"cells": cells(pd.Series(v)) if k not in dfs else cells(pd.Series(pd.to_datetime(v))),
                  "datetime": k in dfs} for k, v in SERIES.items()}

    df = pd.DataFrame({
        "QC Lot": ["L3", "L1", "L2", "L4", "L5", "L6"],
        "Unit": [7.0e7, 5.0e7, 9.0e7, None, "bad", 1.0e7],
        "Notes": ["Retest", "ok", "RETEST later", None, "", "fine"],
    })
    rules = [Rule("off", "Notes", "contains", "retest", enabled=False),
             Rule("high", "Unit", ">", "6e7", color="red"),
             Rule("retest", "Notes", "contains", "retest", color="blue"),
             Rule("", "Missing", "=", "x", sheet="Other"),
             Rule("", "Missing", "=", "x")]
    evaluations = []
    for sheet in (None, "Summary", "Other"):
        rs = rules if sheet else rules[:3]
        evaluations.append({"sheet": sheet, "rules": [rule_dict(r) for r in rs], **attempt(
            lambda: [int(i) for i in evaluate(rs, df, sheet)])})
    evaluations.append({"sheet": None, "rules": [rule_dict(rules[4])],
                        **attempt(lambda: [int(i) for i in evaluate([rules[4]], df)])})
    checks = []
    tables = [("Summary", ["QC Lot", "Notes"]), ("Data", ["QC Lot", "Unit"])]
    for rs in ([Rule("", "Notes", "=", "x")], [Rule("", "Nope", "=", "x")], [Rule("", "Nope", "=", "x", sheet="Data")],
               [Rule("", "Notes", "=", "x", sheet="Data")], [Rule("", "Nope", "=", "x", sheet="Elsewhere")],
               [Rule("n", "Nope", "=", "x", enabled=False)], [Rule("named", "Nope", "is blank", sheet="Summary")]):
        checks.append({"rules": [rule_dict(r) for r in rs], "tables": tables,
                       **attempt(lambda: check_columns(rs, tables))})

    files = {
        "full": {"rules": [{"name": "a", "column": "Notes", "op": "contains", "value": "retest", "color": "#123456"},
                           {"column": "Unit", "op": ">", "value": "5e7", "enabled": False, "extra": 1}]},
        "list": [{"column": "Notes", "op": "is blank", "sheet": "Summary", "color": "red"}],
        "numbers": {"rules": [{"column": "Unit", "op": ">", "value": 5}]},
        "not_list": {"rules": {"column": "x"}},
        "not_object": {"rules": ["x"]},
        "missing_op": {"rules": [{"column": "x"}]},
        "bad_op": {"rules": [{"column": "x", "op": "~"}]},
        "bad_json": "{not json",
        "empty": {"rules": []},
        "string": "\"just text\"",
    }
    loads = []
    tmp = Path(tempfile.mkdtemp())
    for name, content in files.items():
        p = tmp / f"{name}.json"
        p.write_text(content if isinstance(content, str) else json.dumps(content), encoding="utf-8")
        res = attempt(lambda: [rule_dict(r) for r in load_rules(p)])
        if "error" in res:
            res["error"] = res["error"].replace(str(p), "{FILE}")
        loads.append({"name": name, "content": p.read_text(encoding="utf-8"), **res})
    missing = attempt(load_rules, tmp / "nope.json")
    out = tmp / "saved.json"
    save_rules([Rule("a", "Notes", "contains", "retest", "#123456"),
                Rule("", "Unit", ">", "5e7", enabled=False, sheet="Data"),
                Rule("Ünïcode › name", "Notes", "is blank")], out)
    saved = out.read_text(encoding="utf-8")
    shutil.rmtree(tmp)
    dump("rules.json", {"series": series, "values": RULE_VALUES, "cases": cases, "evaluate": evaluations,
                        "check_columns": checks, "load": loads,
                        "missing": missing["error"].replace(str(tmp / "nope.json"), "{FILE}"), "saved": saved})


def rule_dict(r: Rule) -> dict:
    return {"name": r.name, "column": r.column, "op": r.op, "value": r.value, "color": r.color,
            "enabled": r.enabled, "sheet": r.sheet}


# --- chart jobs ----------------------------------------------------------------------

def src(file, column, date="Date Processed", sheet=None, label=None):
    return {"file": file, "column": column, "date": date, "sheet": sheet, "label": label}


QC_RULES = [
    {"name": "Retest", "column": "Notes", "op": "contains", "value": "retest", "color": "tab:orange",
     "enabled": True, "sheet": ""},
    {"name": "", "column": "QC3 CD34 Viability", "op": "<", "value": "85%", "color": "#2ca02c", "enabled": True,
     "sheet": "Summary"},
    {"name": "Late", "column": "Date Processed", "op": ">=", "value": "2025-09-01", "color": "red",
     "enabled": True, "sheet": ""},
    {"name": "off", "column": "Notes", "op": "is blank", "value": "", "color": "#000000", "enabled": False,
     "sheet": ""},
]

JOBS = {
    # build_job / build_multi_job: same sheet and columns everywhere.
    "header_tab": {"kind": "multi", "files": ["with_header_tab.xlsx"], "sheet": "Summary",
                   "column": "QC3 CD34/Unit", "date": "Date Processed", "lsl": 2e7},
    "viability": {"kind": "multi", "files": ["inline.xlsx"], "sheet": "Data", "column": "QC3 CD34 Viability",
                  "date": "Date Processed", "lsl": 0.5, "k": 2.0},
    "no_date": {"kind": "multi", "files": ["noheader.xlsx"], "column": "Column 2", "date": None},
    "csv_dates": {"kind": "multi", "files": ["data.csv"], "column": "QC3 CD34/Unit", "date": "Date Processed",
                  "lsl": 2e7, "recent_days": 0},
    "messy": {"kind": "multi", "files": ["messy.csv"], "column": "CD34", "date": "Date Processed", "lsl": 2e7},
    "tsv": {"kind": "multi", "files": ["tabbed.tsv"], "column": "Value", "date": "Run Date", "recent_days": 5},
    "dates_mixed": {"kind": "multi", "files": ["dates.xlsx"], "sheet": "Runs", "column": "Value",
                    "date": "Run Date", "recent_days": 3},
    "dates_rows": {"kind": "multi", "files": ["dates.xlsx"], "sheet": "Runs", "column": "Value", "date": None,
                   "label": "Lot"},
    "split": {"kind": "multi", "files": ["part1.xlsx", "part2.xlsx"], "sheet": "Summary",
              "column": "QC3 CD34/Unit", "date": "Date Processed", "lsl": 2e7},
    "split_rows": {"kind": "multi", "files": ["part1.xlsx", "part2.xlsx"], "column": "QC3 CD34/Unit",
                   "date": None},
    "same_name": {"kind": "multi", "files": ["part1.xlsx", "other/part1.xlsx"], "column": "QC3 CD34/Unit",
                  "date": "Date Processed"},
    "duplicates": {"kind": "multi", "files": ["dup_a.xlsx", "dup_b.xlsx"], "column": "QC3 CD34/Unit",
                   "date": "Date Processed", "lsl": 2e7, "recent_days": 14},
    "qc_rules": {"kind": "multi", "files": ["qc_summary.xlsx"], "sheet": "Summary", "column": "QC3 CD34/Unit",
                 "date": "Date Processed", "lsl": 2e7, "k": 2.5, "rules": QC_RULES},
    "qc_viability_rules": {"kind": "multi", "files": ["qc_summary.xlsx"], "column": "QC3 CD34 Viability",
                           "date": "Date Processed", "lsl": 0.85, "rules": QC_RULES[:1], "recent_days": 60},
    "qc_cfu": {"kind": "multi", "files": ["qc_summary.xlsx"], "column": "QC2 CFU (GM&GEMM)", "date": None,
               "recent_days": 30},
    # build_sources_job: each file with its own sheet and columns.
    "sources": {"kind": "sources", "sources": [src("with_header_tab.xlsx", "QC3 CD34/Unit", sheet="Summary"),
                                               src("inline.xlsx", "QC3 CD34/Unit", sheet="Data")], "lsl": 2e7},
    "sources_archive": {"kind": "sources", "rules": QC_RULES, "lsl": 2e7, "sources": [
        src("qc_summary.xlsx", "QC3 CD34/Unit", sheet="Summary"),
        src("qc_summary.xlsx", "CD34 per unit", date="Processing Date", sheet="Archive", label="Lot Number")]},
    "sources_csv": {"kind": "sources", "sources": [src("data.csv", "QC3 CD34/Unit"),
                                                   src("part1.xlsx", "QC3 CD34/Unit", sheet="Summary")]},
    "sources_rows": {"kind": "sources", "sources": [src("data.csv", "QC3 CD34/Unit", date=None),
                                                    src("noheader.xlsx", "Column 2", date=None, sheet="Sheet1")]},
    # Errors.
    "mixed_dates": {"kind": "sources", "sources": [src("data.csv", "QC3 CD34/Unit"),
                                                   src("noheader.xlsx", "Column 2", date=None, sheet="Sheet1")]},
    "missing_column": {"kind": "multi", "files": ["part1.xlsx", "data.csv"], "column": "Notes",
                       "date": "Nope"},
    "missing_rule_column": {"kind": "multi", "files": ["part1.xlsx"], "column": "QC3 CD34/Unit",
                            "date": "Date Processed",
                            "rules": [{"name": "", "column": "Nope", "op": "=", "value": "x"}]},
    "too_few": {"kind": "multi", "files": ["semicolon.csv"], "column": "Lot", "date": "Date"},
    "semicolon": {"kind": "multi", "files": ["semicolon.csv"], "column": "Value", "date": "Date", "k": 3.5},
}


def job_from_spec(spec):
    rules = [Rule(**r) for r in spec.get("rules", [])]
    common = {"lsl": spec.get("lsl"), "k": spec.get("k", 3.0), "recent_days": spec.get("recent_days", 30),
              "rules": rules}
    if spec["kind"] == "multi":
        tables = [(DATA / f, loader.load_table(DATA / f, spec.get("sheet"))) for f in spec["files"]]
        return report.build_multi_job(tables, spec["column"], spec["date"], label_column=spec.get("label"),
                                      sheet=spec.get("sheet"), **common)
    sources = [report.SourceTable(DATA / s["file"], loader.load_table(DATA / s["file"], s["sheet"]), s["column"],
                                  s["date"], s["sheet"], s["label"]) for s in spec["sources"]]
    return report.build_sources_job(sources, **common)


def frame_cells(df: pd.DataFrame) -> dict:
    return {str(c): [normalize(v) if isinstance(v, str) else v for v in cells(df[c])] for c in df.columns}


def font_doc(f):
    return {"b": bool(f.b), "i": bool(f.i), "color": f.color.rgb if f.color is not None and f.color.type == "rgb"
            else None, "sz": float(f.sz) if f.sz else None}


def workbook_doc(path: Path, run_at: str) -> list:
    wb = openpyxl.load_workbook(path)
    sheets = []
    for ws in wb.worksheets:
        rows = []
        for r in ws.iter_rows():
            row = []
            for c in r:
                v = c.value
                row.append(normalize(v, run_at) if isinstance(v, str) else tag(v))
            rows.append(row)
        styles = {}
        for r in ws.iter_rows():
            for c in r:
                s = {}
                if c.fill.fill_type == "solid":
                    s["fill"] = c.fill.fgColor.rgb
                f = font_doc(c.font)
                if f["b"] or f["i"] or f["color"] or (f["sz"] and f["sz"] != 11.0):
                    s["font"] = f
                if c.number_format != "General":
                    s["format"] = c.number_format
                if c.border.left.style:
                    s["border"] = c.border.left.style
                if c.alignment.horizontal:
                    s["align"] = [c.alignment.horizontal, c.alignment.vertical]
                if s:
                    styles[c.coordinate] = s
        widths = {k: v.width for k, v in ws.column_dimensions.items() if v.width}
        cf = []
        for rng in ws.conditional_formatting:
            for rule in rng.rules:
                cf.append({"range": str(rng.sqref), "type": rule.type, "operator": rule.operator,
                           "formula": list(rule.formula), "fill": rule.dxf.fill.fgColor.rgb if rule.dxf and
                           rule.dxf.fill else None, "bg": rule.dxf.fill.bgColor.rgb if rule.dxf and rule.dxf.fill
                           else None})
        sheets.append({"name": ws.title, "rows": rows, "styles": styles, "widths": widths,
                       "freeze": ws.freeze_panes, "cf": cf})
    return sheets


UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")


def normalize_html(text: str) -> str:
    start = text.index("<script>/**\n* plotly.js")
    end = text.index("</script>", start)
    text = text[:start] + "<script>{PLOTLYJS}" + text[end:]
    text = text.replace(str(HERE).replace("/", "\\u002f"), "{DIR}")
    return UUID.sub("{ID}", text)


def jobs_golden():
    HTML.mkdir(exist_ok=True)
    for old in HTML.glob("*"):
        old.unlink()
    out = []
    tmp = Path(tempfile.mkdtemp())
    for name, spec in JOBS.items():
        case = {"name": name, "spec": spec}
        try:
            job = job_from_spec(spec)
        except (ValueError, KeyError) as exc:
            case["error"] = normalize(exc.args[0] if isinstance(exc, KeyError) else str(exc))
            out.append(case)
            continue
        prov = job.provenance()
        run_at = str(prov["Generated"])
        case["audit"] = frame_cells(job.audit)
        case["data"] = frame_cells(job.data)
        case["excluded"] = job.excluded
        case["result"] = result_doc(job.result)
        case["column"] = job.column
        case["date_column"] = job.date_column
        case["sheet"] = job.sheet
        case["label_column"] = job.label_column
        try:
            case["duplicates"] = job.duplicates
        except TypeError:  # the Python version crashed here (several files in row order)
            case["duplicates"] = "crash"
        case["highlighted"] = job.highlighted
        case["stem"] = job.default_stem()
        case["sha256"] = job.sha256s
        case["provenance"] = [[k, normalize(v, run_at) if isinstance(v, str) else v] for k, v in prov.items()]
        recent = plot_static.recent_mask(job.data, job.recent_days)
        case["recent"] = None if recent is None else [bool(b) for b in recent]
        case["groups"] = [[label, color, [bool(b) for b in mask]]
                          for label, color, mask in plot_static.highlight_groups(job.data)]
        case["percent"] = bool(plot_static.looks_like_fraction(job.result.values))
        # Workbook: save it, read it back, and keep only the description.
        xlsx = tmp / f"{name}.xlsx"
        job.save_xlsx(xlsx)
        # provenance() is called again inside save_xlsx; its time may differ by a second.
        stamp = openpyxl.load_workbook(xlsx)["Summary stats"]
        gen = next(r[1].value for r in stamp.iter_rows() if r[0].value == "Generated")
        case["xlsx"] = workbook_doc(xlsx, gen)
        html = tmp / f"{name}.html"
        job.save_html(html)
        (HTML / f"{name}.html").write_text(normalize_html(html.read_text(encoding="utf-8")), encoding="utf-8")
        out.append(case)
    shutil.rmtree(tmp)
    dump("jobs.json", out)


# --- command line --------------------------------------------------------------------

CLI_CASES = {
    "list_columns": ["with_header_tab.xlsx", "--list-columns"],
    "list_columns_multi": ["part1.xlsx", "part2.xlsx", "data.csv", "--list-columns"],
    "list_columns_qc": ["qc_summary.xlsx", "--list-columns", "--sheet", "Archive"],
    "one": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--lsl", "2e7", "--formats", "xlsx,html"],
    "two_columns": ["data.csv", "-c", "QC3 CD34/Unit", "--lsl", "2e7", "-c", "QC3 CD34 Viability", "--lsl", "50%",
                    "--formats", "html"],
    "one_lsl_two_columns": ["data.csv", "-c", "QC3 CD34/Unit", "-c", "QC3 CD34 Viability", "--lsl", "0",
                            "--formats", " XLSX , "],
    "no_date": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--no-date", "--formats", "xlsx", "--sigma", "2"],
    "multi": ["part1.xlsx", "part2.xlsx", "-c", "QC3 CD34/Unit", "--lsl", "2e7", "--formats", "xlsx"],
    "dups": ["dup_a.xlsx", "dup_b.xlsx", "-c", "QC3 CD34/Unit", "--formats", "xlsx", "--recent-days", "0"],
    "rules": ["qc_summary.xlsx", "-c", "QC3 CD34/Unit", "--rules", "{RULES}", "--formats", "xlsx",
              "--label-column", "ISBT DIN", "--date-column", "Date Processed"],
    "rules_sheet": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--rules", "{RULES_SHEET}", "--formats", "xlsx"],
    "no_column": ["with_header_tab.xlsx"],
    "bad_format": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--formats", "png,pdf,svg"],
    "negative_days": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--recent-days", "-1"],
    "lsl_count": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--lsl", "1", "--lsl", "2", "--lsl", "3"],
    "bad_lsl": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--lsl", "abc"],
    "missing_file": ["nope.xlsx", "-c", "x"],
    "missing_sheet": ["part1.xlsx", "part2.xlsx", "--sheet", "Nope", "-c", "x"],
    "missing_column": ["with_header_tab.xlsx", "-c", "Nope"],
    "missing_in_second": ["part1.xlsx", "messy.csv", "-c", "QC3 CD34/Unit"],
    "too_few": ["semicolon.csv", "-c", "Lot", "--formats", "xlsx"],
    "bad_rules": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--rules", "{BAD_RULES}"],
    "missing_rules": ["with_header_tab.xlsx", "-c", "QC3 CD34/Unit", "--rules", "nope.json"],
}


def cli_golden():
    out = []
    tmp = Path(tempfile.mkdtemp())
    rules_dir = HERE / "rules"
    rules_dir.mkdir(exist_ok=True)
    (rules_dir / "qc.json").write_text(json.dumps({"rules": QC_RULES}, indent=2) + "\n", encoding="utf-8")
    (rules_dir / "sheet.json").write_text(json.dumps({"rules": [
        {"column": "Notes", "op": "is not blank", "sheet": "Elsewhere"},
        {"column": "Notes", "op": "=", "value": "a", "sheet": "Summary", "color": "blue"}]}) + "\n", encoding="utf-8")
    (rules_dir / "bad.json").write_text('{"rules": [{"column": "Notes"}]}\n', encoding="utf-8")
    subs = {"{RULES}": "rules/qc.json", "{RULES_SHEET}": "rules/sheet.json", "{BAD_RULES}": "rules/bad.json"}
    for name, argv in CLI_CASES.items():
        out_dir = tmp / name
        args = []
        for a in argv:
            a = subs.get(a, a)
            if a.endswith((".xlsx", ".csv", ".json", ".tsv")) and not a.startswith("rules/"):
                a = str(DATA / a)
            elif a.startswith("rules/"):
                a = str(HERE / a)
            args.append(a)
        args += ["-o", str(out_dir)]
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = cli.main(args)
        files = sorted(p.name for p in out_dir.glob("*")) if out_dir.exists() else []
        norm = lambda s: normalize(s.replace(str(out_dir), "{OUT}"))  # noqa: E731
        out.append({"name": name, "args": [a.replace(str(HERE), "{DIR}") for a in args[:-2]] + ["-o", "{OUT}"],
                    "code": code, "stdout": norm(stdout.getvalue()), "stderr": norm(stderr.getvalue()),
                    "files": files})
    shutil.rmtree(tmp)
    dump("cli.json", out)


if __name__ == "__main__":
    write_inputs()
    loader_golden()
    scalar_golden()
    stats_golden()
    rules_golden()
    jobs_golden()
    cli_golden()
    print("golden fixtures written to", HERE)
