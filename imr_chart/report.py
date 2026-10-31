"""Glue between loading, statistics and exports, shared by the CLI and GUI."""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

import pandas as pd

from . import __version__, export_xlsx, loader, plot_html, plot_static
from . import rules as rules_mod
from .proof import file_sha256
from .rules import Rule
from .stats import ImrResult, compute_imr

FORMATS = ("png", "html", "xlsx")


@dataclass
class SourceTable:
    """One input file's frame and the columns to chart from it."""

    path: str | Path
    df: pd.DataFrame
    column: str
    date_column: str | None = None
    sheet: str | None = None
    label_column: str | None = None


def _joined(values) -> str | None:
    """Distinct non-empty values in first-seen order, joined for display; None when there are none."""
    unique = list(dict.fromkeys(v for v in values if v))
    return " / ".join(map(str, unique)) or None


@dataclass
class ChartJob:
    sources: list[Path]
    column: str  # for display; per-source values are in ``columns`` etc.
    date_column: str | None
    data: pd.DataFrame
    result: ImrResult
    excluded: int
    recent_days: int = plot_static.DEFAULT_RECENT_DAYS
    audit: pd.DataFrame | None = None
    sheet: str | None = None
    label_column: str | None = None
    sha256s: list[str | None] = field(default_factory=list, repr=False)
    rules: list[Rule] = field(default_factory=list)
    columns: list[str] = field(default_factory=list)
    date_columns: list[str | None] = field(default_factory=list)
    sheets: list[str | None] = field(default_factory=list)

    @property
    def source(self) -> Path:
        return self.sources[0]

    @property
    def duplicates(self) -> int:
        """Charted points whose lot, date and value also appear in another file (e.g. overlapping exports)."""
        if len(self.sources) < 2:
            return 0
        key = self.data[["label", "date", "value"]].astype(str).agg("|".join, axis=1)
        files_per_key = self.data.groupby(key)["file"].transform("nunique")
        return int((files_per_key > 1).sum())

    @property
    def highlighted(self) -> int:
        """Charted points colored by a highlight rule."""
        return int((self.data["highlight"] != "").sum()) if "highlight" in self.data else 0

    def figure(self):
        return plot_static.build_figure(self.data, self.result, self.column,
                                        date_label=self.date_column or "Observation", excluded=self.excluded,
                                        recent_days=self.recent_days, sources=len(self.sources))

    def save_png(self, path) -> Path:
        return plot_static.save_png(self.figure(), path)

    def save_html(self, path) -> Path:
        fig = plot_html.build_html_figure(self.data, self.result, self.column,
                                          date_label=self.date_column or "Observation", excluded=self.excluded,
                                          recent_days=self.recent_days, sources=len(self.sources))
        return plot_html.save_html(fig, path)

    def save_xlsx(self, path) -> Path:
        name = self.source.name if len(self.sources) == 1 else f"{len(self.sources)} files"
        return export_xlsx.save_xlsx(self.data, self.result, self.column, path,
                                     source=name, excluded=self.excluded,
                                     audit=self.audit, provenance=self.provenance())

    def provenance(self) -> dict[str, object]:
        """What the chart was made from and with, so it can be tied to the exact input."""
        files: dict[str, object] = {}
        if len(self.sources) == 1:
            files["Source path"] = str(self.source.resolve())
            files["Source SHA-256"] = self.sha256s[0] or "(file not readable)"
        else:
            files["Source files"] = len(self.sources)
            for i, (src, sha) in enumerate(zip(self.sources, self.sha256s), start=1):
                files[f"Source {i} path"] = str(src.resolve())
                files[f"Source {i} SHA-256"] = sha or "(file not readable)"
                if self.columns:
                    if not loader.is_csv(src):
                        files[f"Source {i} sheet"] = self.sheets[i - 1] or "(default)"
                    files[f"Source {i} column"] = self.columns[i - 1]
                    files[f"Source {i} date column"] = self.date_columns[i - 1] or "(row order)"
        return files | {
            "Sheet": self.sheet or "(default)",
            "Date column": self.date_column or "(row order)",
            "Label column": self.label_column or "(row number)",
            "Highlight rules": "; ".join(f"{r.describe()} → {r.color}" + ("" if r.enabled else " (off)")
                                         for r in self.rules) or "(none)",
            "Recent highlight (days)": self.recent_days or "off",
            "Generated": datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S %Z"),
            "Program version": f"imr-chart {__version__}",
        }

    def default_stem(self) -> str:
        return f"IMR_{safe_filename(self.column)}"

    def save_all(self, out_dir, formats=FORMATS) -> list[Path]:
        out_dir = Path(out_dir)
        savers = {"png": self.save_png, "html": self.save_html, "xlsx": self.save_xlsx}
        return [savers[f](out_dir / f"{self.default_stem()}.{f}") for f in formats]


def safe_filename(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", text).strip("_") or "chart"


def build_job(
    df: pd.DataFrame,
    source: str | Path,
    column: str,
    date_column: str | None = None,
    lsl: float | None = None,
    k: float = 3.0,
    label_column: str | None = None,
    recent_days: int = plot_static.DEFAULT_RECENT_DAYS,
    sheet: str | None = None,
    rules: list[Rule] | None = None,
) -> ChartJob:
    return build_multi_job([(source, df)], column, date_column, lsl=lsl, k=k, label_column=label_column,
                           recent_days=recent_days, sheet=sheet, rules=rules)


def build_multi_job(
    tables: list[tuple[str | Path, pd.DataFrame]],
    column: str,
    date_column: str | None = None,
    lsl: float | None = None,
    k: float = 3.0,
    label_column: str | None = None,
    recent_days: int = plot_static.DEFAULT_RECENT_DAYS,
    sheet: str | None = None,
    rules: list[Rule] | None = None,
) -> ChartJob:
    """One chart pooling every row of several same-layout files (sorted by date, or kept in file order)."""
    if not tables:
        raise ValueError("No input files")
    if label_column is None:
        label_column = loader.pick_label_column(tables[0][1])
    sources = [SourceTable(src, df, column, date_column, sheet, label_column) for src, df in tables]
    job = build_sources_job(sources, lsl=lsl, k=k, recent_days=recent_days, rules=rules)
    job.sheet = sheet
    return job


def build_sources_job(
    sources: list[SourceTable],
    lsl: float | None = None,
    k: float = 3.0,
    recent_days: int = plot_static.DEFAULT_RECENT_DAYS,
    rules: list[Rule] | None = None,
) -> ChartJob:
    """One chart pooling the rows of several files, each with its own sheet, value column and date column.

    Points are sorted by date across files, or kept in file order when no source uses a date column."""
    if not sources:
        raise ValueError("No input files")
    dated = [s.date_column is not None for s in sources]
    if any(dated) and not all(dated):
        raise ValueError("Choose a date column for every workbook, or (row order) for all of them.")
    paths = [Path(s.path) for s in sources]
    names = [p.name for p in paths]
    if len(set(names)) < len(names):  # same file name in different folders: keep them apart
        names = [str(p) for p in paths]
    labels = [s.label_column if s.label_column is not None else loader.pick_label_column(s.df) for s in sources]
    rules_mod.check_columns(rules, [(s.sheet, list(s.df.columns)) for s in sources])
    audit = loader.audit_sources([(name, s.df, s.column, s.date_column, label, s.sheet)
                                  for name, s, label in zip(names, sources, labels)], rules)
    data, excluded = loader.included_series(audit, by_date=all(dated))
    result = compute_imr(data["value"].to_numpy(), k=k, lsl=lsl)
    return ChartJob(paths, _joined(s.column for s in sources), _joined(s.date_column for s in sources),
                    data, result, excluded, recent_days, audit=audit,
                    sheet=_joined(s.sheet for s in sources if not loader.is_csv(s.path)),  # a CSV has no sheet
                    label_column=_joined(labels), sha256s=[file_sha256(p) for p in paths], rules=list(rules or []),
                    columns=[s.column for s in sources], date_columns=[s.date_column for s in sources],
                    sheets=[s.sheet for s in sources])
