import pandas as pd
import pytest

from imr_chart import loader
from imr_chart.rules import Rule, apply, evaluate, load_rules, save_rules

# Mirrors conftest ROWS: Excel-serial dates, a blank and a junk value, mixed-case text.
DF = pd.DataFrame({
    "QC Lot": ["L3", "L1", "L2", "L4", "L5", "L6"],
    "Date Processed": [45400, 45363, 45383, 45410, 45420, 45430],
    "Unit": [7.0e7, 5.0e7, 9.0e7, None, "bad", 1.0e7],
    "Viability": [0.90, 0.80, 0.95, 0.70, 0.40, 0.60],
    "Notes": ["Retest", "ok", "RETEST later", None, "", "fine"],
})


def hits(op, value="", column="Notes"):
    return list(DF["QC Lot"][Rule("", column, op, value).matches(DF).to_numpy(dtype=bool)])


@pytest.mark.parametrize("op, value, column, expected", [
    ("=", "l2", "QC Lot", ["L2"]),
    ("<>", "L2", "QC Lot", ["L3", "L1", "L4", "L5", "L6"]),
    (">", "5e7", "Unit", ["L3", "L2"]),
    (">=", "50,000,000", "Unit", ["L3", "L1", "L2"]),
    ("<", "70%", "Viability", ["L5", "L6"]),
    ("<=", "0.7", "Viability", ["L4", "L5", "L6"]),
    ("=", "7e7", "Unit", ["L3"]),
    (">=", "2024-04-18", "Date Processed", ["L3", "L4", "L5", "L6"]),
    ("<", "04/01/2024", "Date Processed", ["L1"]),
    ("contains", "retest", "Notes", ["L3", "L2"]),
    ("not contains", "retest", "Notes", ["L1", "L4", "L5", "L6"]),
    ("starts with", "re", "Notes", ["L3", "L2"]),
    ("ends with", "INE", "Notes", ["L6"]),
    ("is blank", "", "Notes", ["L4", "L5"]),
    ("is not blank", "", "Unit", ["L3", "L1", "L2", "L5", "L6"]),
])
def test_operators(op, value, column, expected):
    assert hits(op, value, column) == expected


def test_ordering_needs_number_or_date():
    with pytest.raises(ValueError, match="needs a number or date"):
        hits(">", "abc")


def test_first_enabled_match_wins():
    rules = [Rule("off", "Notes", "contains", "retest", enabled=False),
             Rule("high", "Unit", ">", "6e7", color="red"),
             Rule("retest", "Notes", "contains", "retest", color="blue")]
    assert list(evaluate(rules, DF)) == [1, -1, 1, -1, -1, -1]
    rules[0].enabled = True
    assert list(evaluate(rules, DF)) == [0, -1, 0, -1, -1, -1]
    labels, colors = apply(rules[1:], DF)
    assert list(labels) == ["high", "", "high", "", "", ""]
    assert list(colors) == ["#ff0000", "", "#ff0000", "", "", ""]


def test_label_defaults_to_description():
    assert Rule("", "Notes", "contains", "x").label == "Notes contains 'x'"
    assert Rule("", "Notes", "is blank").label == "Notes is blank"


@pytest.mark.parametrize("kw, msg", [
    ({"op": "~"}, "unknown operator"),
    ({"color": "notacolor"}, "not a color"),
    ({"column": ""}, "no column"),
])
def test_invalid_rule(kw, msg):
    with pytest.raises(ValueError, match=msg):
        Rule(**{"name": "r", "column": "Notes", "op": "=", "value": "x"} | kw)


def test_missing_column():
    with pytest.raises(KeyError, match="column 'Nope' not found"):
        evaluate([Rule("r", "Nope", "=", "x")], DF)


def test_json_round_trip(tmp_path):
    rules = [Rule("a", "Notes", "contains", "retest", "#123456"), Rule("", "Unit", ">", "5e7", enabled=False)]
    path = save_rules(rules, tmp_path / "r.json")
    assert load_rules(path) == rules


@pytest.mark.parametrize("text, msg", [
    ("{", "not valid JSON"),
    ('{"rules": 3}', "must hold a list"),
    ('{"rules": [{"name": "x"}]}', "needs a \"column\""),
])
def test_bad_rules_file(tmp_path, text, msg):
    path = tmp_path / "r.json"
    path.write_text(text)
    with pytest.raises(ValueError, match=msg):
        load_rules(path)
    with pytest.raises(ValueError, match="not found"):
        load_rules(tmp_path / "missing.json")


def test_audit_carries_rules(xlsx_with_header_tab):
    df = loader.load_table(xlsx_with_header_tab)
    rules = [Rule("note b", "Notes", "=", "b", "green"), Rule("", "QC3 CD34 Viability", ">", "85%", "orange")]
    data, _ = loader.prepare_series(df, "QC3 CD34/Unit", "Date Processed", "QC Lot", rules=rules)
    assert list(data["label"]) == ["L1", "L2", "L3", "L6"]
    assert list(data["rule"]) == ["note b", "QC3 CD34 Viability > '85%'", "QC3 CD34 Viability > '85%'", ""]
    assert list(data["highlight"]) == ["#008000", "#ffa500", "#ffa500", ""]
