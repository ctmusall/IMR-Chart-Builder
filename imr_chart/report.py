"""Glue between loading, statistics and exports, shared by the CLI and GUI."""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

import pandas as pd

from . import __version__, export_xlsx, loader, plot_html, plot_static
from .proof import file_sha256
from .stats import ImrResult, compute_imr

FORMATS = ("png", "html", "xlsx")


@dataclass
class ChartJob:
    sources: list[Path]
    column: str
    date_column: str | None
    data: pd.DataFrame
    result: ImrResult
    excluded: int
    recent_days: int = plot_static.DEFAULT_RECENT_DAYS
    audit: pd.DataFrame | None = None
    sheet: str | None = None
    label_column: str | None = None
    sha256s: list[str | None] = field(default_factory=list, repr=False)

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
        return files | {
            "Sheet": self.sheet or "(default)",
            "Date column": self.date_column or "(row order)",
            "Label column": self.label_column or "(row number)",
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
) -> ChartJob:
    return build_multi_job([(source, df)], column, date_column, lsl=lsl, k=k, label_column=label_column,
                           recent_days=recent_days, sheet=sheet)


def build_multi_job(
    tables: list[tuple[str | Path, pd.DataFrame]],
    column: str,
    date_column: str | None = None,
    lsl: float | None = None,
    k: float = 3.0,
    label_column: str | None = None,
    recent_days: int = plot_static.DEFAULT_RECENT_DAYS,
    sheet: str | None = None,
) -> ChartJob:
    """One chart pooling every row of several same-layout files (sorted by date, or kept in file order)."""
    if not tables:
        raise ValueError("No input files")
    sources = [Path(src) for src, _ in tables]
    if label_column is None:
        label_column = loader.pick_label_column(tables[0][1])
    names = [src.name for src in sources]
    if len(set(names)) < len(names):  # same file name in different folders: keep them apart
        names = [str(src) for src in sources]
    audit = loader.audit_files([(name, df) for name, (_, df) in zip(names, tables)],
                               column, date_column, label_column)
    data, excluded = loader.included_series(audit, by_date=date_column is not None)
    result = compute_imr(data["value"].to_numpy(), k=k, lsl=lsl)
    return ChartJob(sources, column, date_column, data, result, excluded, recent_days,
                    audit=audit, sheet=sheet, label_column=label_column,
                    sha256s=[file_sha256(src) for src in sources])
