"""Static (matplotlib) Individual & Moving Range chart, styled after JMP."""

from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg", force=False)
import matplotlib.dates as mdates  # noqa: E402
import numpy as np  # noqa: E402
import pandas as pd  # noqa: E402
from matplotlib.figure import Figure  # noqa: E402
from matplotlib.ticker import FuncFormatter  # noqa: E402

from .stats import ImrResult  # noqa: E402

POINT_COLOR = "#111111"
CONNECTOR_COLOR = "#9a9a9a"
MEAN_COLOR = "#2e9e3e"
LIMIT_COLOR = "#d62728"
LSL_COLOR = "#3a5fcd"
FLAG_COLOR = "#d62728"
RECENT_COLOR = "#ff7f0e"
RECENT_BAND_COLOR = "#fff1e0"
DEFAULT_RECENT_DAYS = 30


def recent_mask(data: pd.DataFrame, days: int | None) -> np.ndarray | None:
    """Points within ``days`` of the latest date, or None when there is no window."""
    if not days or "date" not in data or not data["date"].notna().all() or data.empty:
        return None
    dates = data["date"]
    return (dates >= dates.max() - pd.Timedelta(days=days)).to_numpy()


def looks_like_fraction(values) -> bool:
    """True when the data look like proportions (e.g. viability 0.89 = 89%)."""
    v = np.asarray(values, dtype=float)
    return v.size > 0 and np.nanmin(v) >= 0 and np.nanmax(v) <= 1.5


def format_number(x: float, percent: bool = False) -> str:
    if percent:
        return f"{x * 100:.0f}%"
    if x != 0 and (abs(x) >= 1e5 or abs(x) < 1e-3):
        mant, exp = f"{x:.2e}".split("e")
        return f"{mant}e{int(exp):+d}"
    return f"{x:,.4g}"


def lsl_label(lsl: float, percent: bool) -> str:
    if percent:
        return f"LSL ({lsl * 100:g}%)"
    if lsl != 0 and (abs(lsl) >= 1e5 or abs(lsl) < 1e-3):
        return f"LSL ({lsl:.0e})".replace("e+0", "e").replace("e+", "e")
    return f"LSL ({lsl:g})"


def _show_lcl(result: ImrResult) -> bool:
    # Like JMP's view of the CD34/Unit chart: a negative LCL on data that can
    # never go negative carries no information, so it is not drawn.
    return not (result.lcl < 0 <= result.values.min())


def build_figure(
    data: pd.DataFrame,
    result: ImrResult,
    column: str,
    date_label: str = "Date Processed",
    excluded: int = 0,
    percent: bool | None = None,
    figsize=(11, 8.5),
    recent_days: int = DEFAULT_RECENT_DAYS,
    sources: int = 1,
) -> Figure:
    """Build the two-panel I-MR figure. ``data`` must have ``date`` and ``value``."""
    if percent is None:
        percent = looks_like_fraction(result.values)
    has_dates = "date" in data and data["date"].notna().all()
    x = data["date"].to_numpy() if has_dates else np.arange(1, result.n + 1)
    recent = recent_mask(data, recent_days)
    if recent is not None and not recent.any():
        recent = None

    fig = Figure(figsize=figsize, dpi=100)
    ax_i, ax_mr = fig.subplots(2, 1, sharex=True, gridspec_kw={"height_ratios": [3, 2], "hspace": 0.06})
    fig.suptitle(f"Individual & Moving Range chart of {column}", fontsize=14, fontweight="bold")

    fmt = FuncFormatter(lambda v, _pos: format_number(v, percent))

    # Individuals
    ax_i.plot(x, result.values, color=CONNECTOR_COLOR, linewidth=0.6, zorder=1)
    ax_i.scatter(x, result.values, s=10, color=POINT_COLOR, zorder=3, linewidths=0)
    flagged = result.above_ucl | result.below_lcl
    if flagged.any():
        ax_i.scatter(np.asarray(x)[flagged], result.values[flagged], s=18, facecolors="none",
                     edgecolors=FLAG_COLOR, linewidths=0.9, zorder=4, label="Beyond limit")
    ax_i.axhline(result.mean, color=MEAN_COLOR, linewidth=1.6, zorder=2)
    ax_i.axhline(result.ucl, color=LIMIT_COLOR, linewidth=1.4, zorder=2)
    if _show_lcl(result):
        ax_i.axhline(result.lcl, color=LIMIT_COLOR, linewidth=1.4, zorder=2)
    if result.lsl is not None:
        ax_i.axhline(result.lsl, color=LSL_COLOR, linewidth=1.0, zorder=2)
        ax_i.annotate(lsl_label(result.lsl, percent), xy=(0, result.lsl), xycoords=("axes fraction", "data"),
                      xytext=(4, 2), textcoords="offset points", ha="left", va="bottom",
                      color=LSL_COLOR, fontsize=9, fontweight="bold",
                      bbox={"boxstyle": "round,pad=0.15", "fc": "white", "ec": "none", "alpha": 0.8}, zorder=5)
    ax_i.set_ylabel(column)
    ax_i.yaxis.set_major_formatter(fmt)
    if not _show_lcl(result):
        lo = min(result.values.min(), result.lsl if result.lsl is not None else np.inf, 0)
        ax_i.set_ylim(bottom=lo - 0.03 * (result.ucl - lo))

    # Moving range
    mr = result.moving_ranges
    ax_mr.plot(x, mr, color=CONNECTOR_COLOR, linewidth=0.6, zorder=1)
    ax_mr.scatter(x, mr, s=10, color=POINT_COLOR, zorder=3, linewidths=0)
    if result.mr_above_ucl.any():
        ax_mr.scatter(np.asarray(x)[result.mr_above_ucl], mr[result.mr_above_ucl], s=18, facecolors="none",
                      edgecolors=FLAG_COLOR, linewidths=0.9, zorder=4)
    ax_mr.axhline(result.mr_bar, color=MEAN_COLOR, linewidth=1.6, zorder=2)
    ax_mr.axhline(result.mr_ucl, color=LIMIT_COLOR, linewidth=1.4, zorder=2)
    ax_mr.axhline(result.mr_lcl, color=LIMIT_COLOR, linewidth=1.4, zorder=2)
    ax_mr.set_ylabel(f"Moving Range({column})")
    ax_mr.yaxis.set_major_formatter(fmt)

    # Recent window: shaded band plus recolored points (beyond-limit rings stay on top).
    if recent is not None:
        start, end = x[recent].min(), x[-1]
        for ax, y in ((ax_i, result.values), (ax_mr, mr)):
            ax.axvspan(start, end, color=RECENT_BAND_COLOR, zorder=0, linewidth=0)
            ax.scatter(x[recent], y[recent], s=12, color=RECENT_COLOR, zorder=3.5, linewidths=0)
        ax_i.annotate(f"Last {recent_days} days", xy=(start, 1), xycoords=("data", "axes fraction"),
                      xytext=(2, -2), textcoords="offset points", ha="left", va="top",
                      color=RECENT_COLOR, fontsize=8, fontweight="bold", zorder=5)

    for ax in (ax_i, ax_mr):
        ax.grid(axis="y", color="#e6e6e6", linewidth=0.6)
        ax.set_axisbelow(True)
        for side in ("top", "right"):
            ax.spines[side].set_visible(False)

    if has_dates:
        locator = mdates.AutoDateLocator(minticks=6, maxticks=12)
        ax_mr.xaxis.set_major_locator(locator)
        ax_mr.xaxis.set_major_formatter(mdates.DateFormatter("%m/%d/%Y"))
        for lbl in ax_mr.get_xticklabels():
            lbl.set_rotation(90)
        ax_mr.set_xlabel(date_label)
    else:
        ax_mr.set_xlabel("Observation")

    # Limit values on the right edge, so readers can see the numbers.
    def tag(ax, y, text, color):
        ax.annotate(text, xy=(1, y), xycoords=("axes fraction", "data"), xytext=(4, 0),
                    textcoords="offset points", va="center", ha="left", fontsize=8, color=color)

    tag(ax_i, result.ucl, f"UCL={format_number(result.ucl, percent)}", LIMIT_COLOR)
    tag(ax_i, result.mean, f"Avg={format_number(result.mean, percent)}", MEAN_COLOR)
    if _show_lcl(result):
        tag(ax_i, result.lcl, f"LCL={format_number(result.lcl, percent)}", LIMIT_COLOR)
    tag(ax_mr, result.mr_ucl, f"UCL={format_number(result.mr_ucl, percent)}", LIMIT_COLOR)
    tag(ax_mr, result.mr_bar, f"Avg={format_number(result.mr_bar, percent)}", MEAN_COLOR)

    note = (f"n={result.n}   Limits: mean ± {result.k:g}σ,  σ = MR̄/1.128 = {format_number(result.sigma, percent)}"
            f"   |   Beyond limits: {int(flagged.sum())} individual, {int(result.mr_above_ucl.sum())} MR")
    if result.lsl is not None:
        note += f",  {int(result.below_lsl.sum())} below LSL"
    if sources > 1:
        note += f"   |   Data from {sources} files"
    if recent is not None:
        note += f"   |   Highlighted: last {recent_days} days (from {pd.Timestamp(x[recent].min()):%m/%d/%Y})"
    if excluded:
        note += f"\n{excluded} rows excluded (blank or non-numeric)."
    fig.text(0.01, 0.005, note, fontsize=8, color="#444444", ha="left", va="bottom")

    fig.subplots_adjust(left=0.12, right=0.87, top=0.93, bottom=0.17 if has_dates else 0.12)
    return fig


def save_png(fig: Figure, path: str | Path, dpi: int = 200) -> Path:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(path, dpi=dpi, facecolor="white")
    return path
