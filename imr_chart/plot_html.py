"""Interactive (plotly) I-MR chart, saved as a self-contained HTML file."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from .plot_static import (
    DEFAULT_RECENT_DAYS,
    LIMIT_COLOR,
    LSL_COLOR,
    MEAN_COLOR,
    POINT_COLOR,
    RECENT_BAND_COLOR,
    RECENT_COLOR,
    _show_lcl,
    format_number,
    looks_like_fraction,
    lsl_label,
    recent_mask,
)
from .stats import ImrResult


def build_html_figure(
    data: pd.DataFrame,
    result: ImrResult,
    column: str,
    date_label: str = "Date Processed",
    excluded: int = 0,
    percent: bool | None = None,
    recent_days: int = DEFAULT_RECENT_DAYS,
    sources: int = 1,
) -> go.Figure:
    if percent is None:
        percent = looks_like_fraction(result.values)
    has_dates = "date" in data and data["date"].notna().all()
    x = data["date"] if has_dates else np.arange(1, result.n + 1)
    labels = data["label"] if "label" in data else pd.Series(range(1, result.n + 1)).astype(str)

    fig = make_subplots(rows=2, cols=1, shared_xaxes=True, vertical_spacing=0.04, row_heights=[0.6, 0.4])
    vfmt = ".1%" if percent else ".4~s"
    date_part = "%{x|%m/%d/%Y}" if has_dates else "Obs %{x}"
    custom = [labels]
    if sources > 1 and "file" in data:
        custom.append(data["file"])
        date_part += "<br>%{customdata[1]}"
    mr = result.moving_ranges
    recent = recent_mask(data, recent_days)
    if recent is not None and not recent.any():
        recent = None
    base = np.where(recent, RECENT_COLOR, POINT_COLOR) if recent is not None else POINT_COLOR

    def colors(flags):
        return np.where(flags, LIMIT_COLOR, base)

    fig.add_trace(
        go.Scatter(
            x=x, y=result.values, mode="lines+markers", name=column,
            line={"color": "#9a9a9a", "width": 1}, marker={"size": 6, "color": colors(result.above_ucl | result.below_lcl)},
            customdata=np.column_stack(custom),
            hovertemplate=f"<b>%{{customdata[0]}}</b><br>{date_part}<br>Value: %{{y:{vfmt}}}<extra></extra>",
        ),
        row=1, col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=x, y=mr, mode="lines+markers", name="Moving Range",
            line={"color": "#9a9a9a", "width": 1}, marker={"size": 6, "color": colors(result.mr_above_ucl)},
            customdata=np.column_stack(custom),
            hovertemplate=f"<b>%{{customdata[0]}}</b><br>{date_part}<br>MR: %{{y:{vfmt}}}<extra></extra>",
        ),
        row=2, col=1,
    )

    # Added after the traces: row="all" skips subplots that are still empty.
    if recent is not None:
        start, end = data["date"][recent].min(), data["date"].iloc[-1]
        fig.add_vrect(x0=start, x1=end, fillcolor=RECENT_BAND_COLOR, opacity=1, layer="below",
                      line_width=0, row="all", col=1)
        fig.add_annotation(x=start, y=1, xref="x", yref="y domain", text=f"<b>Last {recent_days} days</b>",
                           showarrow=False, xanchor="left", yanchor="top", font={"color": RECENT_COLOR, "size": 11})

    def hline(y, color, text, row, width=2):
        fig.add_hline(y=y, line={"color": color, "width": width}, row=row, col=1,
                      annotation_text=text, annotation_position="right",
                      annotation_font={"color": color, "size": 11})

    hline(result.ucl, LIMIT_COLOR, f"UCL={format_number(result.ucl, percent)}", 1)
    hline(result.mean, MEAN_COLOR, f"Avg={format_number(result.mean, percent)}", 1)
    if _show_lcl(result):
        hline(result.lcl, LIMIT_COLOR, f"LCL={format_number(result.lcl, percent)}", 1)
    if result.lsl is not None:
        fig.add_hline(y=result.lsl, line={"color": LSL_COLOR, "width": 1.5}, row=1, col=1,
                      annotation_text=lsl_label(result.lsl, percent), annotation_position="top left",
                      annotation_font={"color": LSL_COLOR, "size": 12})
    hline(result.mr_ucl, LIMIT_COLOR, f"UCL={format_number(result.mr_ucl, percent)}", 2)
    hline(result.mr_bar, MEAN_COLOR, f"Avg={format_number(result.mr_bar, percent)}", 2)
    hline(result.mr_lcl, LIMIT_COLOR, "", 2)

    tickformat = ".0%" if percent else ".2e"
    fig.update_yaxes(title_text=column, tickformat=tickformat, row=1, col=1)
    fig.update_yaxes(title_text=f"Moving Range({column})", tickformat=tickformat, row=2, col=1)
    fig.update_xaxes(title_text=date_label if has_dates else "Observation", row=2, col=1)
    if has_dates:
        fig.update_xaxes(tickformat="%m/%d/%Y", tickangle=-90)

    subtitle = f"n={result.n}, limits = mean ± {result.k:g}σ (σ = MR̄/1.128)"
    if recent is not None:
        subtitle += f" · highlighted: last {recent_days} days (from {start:%m/%d/%Y})"
    if sources > 1:
        subtitle += f" · data from {sources} files"
    if excluded:
        subtitle += f" · {excluded} rows excluded"
    fig.update_layout(
        title={"text": f"<b>Individual & Moving Range chart of {column}</b><br><sup>{subtitle}</sup>", "x": 0.5},
        template="plotly_white", showlegend=False, hovermode="closest",
        height=800, margin={"r": 120},
    )
    return fig


def save_html(fig: go.Figure, path: str | Path) -> Path:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fig.write_html(path, include_plotlyjs=True, full_html=True)
    return path
