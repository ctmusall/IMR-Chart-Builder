"""Highlight rules: color chart points whose source row matches a condition.

A rule tests one column of the source row (any column, not only the charted
one), e.g. ``Notes contains "retest"`` or ``QC3 CD34 Viability < 50%``. Rules
form an ordered list; the first enabled rule a row matches decides its color.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, fields
from pathlib import Path

import numpy as np
import pandas as pd
from matplotlib.colors import is_color_like, to_hex

from . import loader
from .stats import parse_limit

OPERATORS = ("=", "<>", "<", "<=", ">", ">=", "contains", "not contains", "starts with", "ends with",
             "is blank", "is not blank")
NO_VALUE_OPERATORS = ("is blank", "is not blank")
DEFAULT_COLOR = "#9467bd"

_COMPARE = {
    "=": lambda a, b: a == b,
    "<>": lambda a, b: a != b,
    "<": lambda a, b: a < b,
    "<=": lambda a, b: a <= b,
    ">": lambda a, b: a > b,
    ">=": lambda a, b: a >= b,
}


@dataclass
class Rule:
    name: str
    column: str
    op: str
    value: str = ""
    color: str = DEFAULT_COLOR
    enabled: bool = True

    def __post_init__(self):
        self.value = "" if self.value is None else str(self.value)
        if self.op not in OPERATORS:
            raise ValueError(f"Rule '{self.label}': unknown operator '{self.op}' (use one of {', '.join(OPERATORS)})")
        if not self.column:
            raise ValueError(f"Rule '{self.label}': no column chosen")
        if not is_color_like(self.color):
            raise ValueError(f"Rule '{self.label}': '{self.color}' is not a color")
        self.color = to_hex(self.color)

    @property
    def label(self) -> str:
        return self.name or self.describe()

    def describe(self) -> str:
        if self.op in NO_VALUE_OPERATORS:
            return f"{self.column} {self.op}"
        return f"{self.column} {self.op} '{self.value}'"

    def matches(self, df: pd.DataFrame) -> pd.Series:
        """Boolean per row of ``df``; cells that can't be compared never match."""
        if self.column not in df.columns:
            raise KeyError(f"Rule '{self.label}': column '{self.column}' not found")
        s = df[self.column]
        blank = loader._is_blank(s)
        if self.op == "is blank":
            return blank
        if self.op == "is not blank":
            return ~blank
        if self.op in _COMPARE:
            return self._compare(s, blank)
        text = s.astype(str).str.strip().str.casefold().where(~blank, "")
        needle = self.value.strip().casefold()
        if self.op == "contains":
            return text.str.contains(needle, regex=False)
        if self.op == "not contains":
            return ~text.str.contains(needle, regex=False)
        if self.op == "starts with":
            return text.str.startswith(needle)
        return text.str.endswith(needle)

    def _compare(self, s: pd.Series, blank: pd.Series) -> pd.Series:
        cmp = _COMPARE[self.op]
        try:
            number = parse_limit(self.value)
        except ValueError:
            number = None
        if number is not None:
            left = loader.to_numeric(s)
            return cmp(left, number) & left.notna()
        when = pd.to_datetime(self.value, errors="coerce") if self.value.strip() else pd.NaT
        if not pd.isna(when):
            left = loader.to_datetime(s)
            return cmp(left, when) & left.notna()
        if self.op not in ("=", "<>"):
            raise ValueError(f"Rule '{self.label}': '{self.op}' needs a number or date, not '{self.value}'")
        text = s.astype(str).str.strip().str.casefold().where(~blank, "")
        return cmp(text, self.value.strip().casefold())


def evaluate(rules: list[Rule] | None, df: pd.DataFrame) -> pd.Series:
    """Index into ``rules`` of the first enabled rule each row matches; -1 for none."""
    out = pd.Series(-1, index=df.index, dtype=int)
    for i, rule in enumerate(rules or []):
        if not rule.enabled:
            continue
        hit = rule.matches(df).fillna(False).to_numpy(dtype=bool)
        out[hit & (out.to_numpy() == -1)] = i
    return out


def apply(rules: list[Rule] | None, df: pd.DataFrame) -> tuple[pd.Series, pd.Series]:
    """Per row of ``df``: the matching rule's label and color ("" when none)."""
    idx = evaluate(rules, df).to_numpy()
    labels = np.array([""] + [r.label for r in rules or []], dtype=object)
    colors = np.array([""] + [r.color for r in rules or []], dtype=object)
    return pd.Series(labels[idx + 1], index=df.index), pd.Series(colors[idx + 1], index=df.index)


def load_rules(path: str | Path) -> list[Rule]:
    try:
        doc = json.loads(Path(path).read_text(encoding="utf-8"))
    except FileNotFoundError:
        raise ValueError(f"Rules file not found: {path}") from None
    except json.JSONDecodeError as exc:
        raise ValueError(f"Rules file {path} is not valid JSON: {exc}") from None
    items = doc.get("rules") if isinstance(doc, dict) else doc
    if not isinstance(items, list):
        raise ValueError(f"Rules file {path} must hold a list under \"rules\"")
    known = {f.name for f in fields(Rule)}
    out = []
    for item in items:
        if not isinstance(item, dict):
            raise ValueError(f"Rules file {path}: each rule must be an object")
        try:
            out.append(Rule(**{"name": "", **{k: v for k, v in item.items() if k in known}}))
        except TypeError:
            raise ValueError(f"Rules file {path}: each rule needs a \"column\" and an \"op\"") from None
    return out


def save_rules(rules: list[Rule], path: str | Path) -> Path:
    path = Path(path)
    path.write_text(json.dumps({"rules": [asdict(r) for r in rules]}, indent=2) + "\n", encoding="utf-8")
    return path
