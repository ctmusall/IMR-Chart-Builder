"""Glue between loading, statistics and exports, shared by the CLI and GUI."""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

import pandas as pd

from . import export_xlsx, loader, plot_html, plot_static
from .stats import ImrResult, compute_imr

FORMATS = ("png", "html", "xlsx")


@dataclass
class ChartJob:
    source: Path
    column: str
    date_column: str | None
    data: pd.DataFrame
    result: ImrResult
    excluded: int

    def figure(self):
        return plot_static.build_figure(self.data, self.result, self.column,
                                        date_label=self.date_column or "Observation", excluded=self.excluded)

    def save_png(self, path) -> Path:
        return plot_static.save_png(self.figure(), path)

    def save_html(self, path) -> Path:
        fig = plot_html.build_html_figure(self.data, self.result, self.column,
                                          date_label=self.date_column or "Observation", excluded=self.excluded)
        return plot_html.save_html(fig, path)

    def save_xlsx(self, path) -> Path:
        return export_xlsx.save_xlsx(self.data, self.result, self.column, path,
                                     source=self.source.name, excluded=self.excluded)

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
) -> ChartJob:
    if label_column is None:
        label_column = loader.pick_label_column(df)
    data, excluded = loader.prepare_series(df, column, date_column, label_column)
    result = compute_imr(data["value"].to_numpy(), k=k, lsl=lsl)
    return ChartJob(Path(source), column, date_column, data, result, excluded)
