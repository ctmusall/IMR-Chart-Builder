"""Record what the Python version's static chart (matplotlib) draws, for the Rust port's chart tests.

Run from the repo root after generate.py (it reuses its input files and chart jobs):

    .venv/bin/python tests/golden/generate_charts.py

For each chart job it writes, to charts.json: the figure size and margins, the title and note, and
for each panel its position, axis limits, ticks with their labels, axis labels, and every line,
scatter, shaded band, annotation and legend entry, with colors, sizes and drawing order. Dates are
matplotlib date numbers (days since 1970-01-01). Kept after the Python code is removed, for reference.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.dates as mdates  # noqa: E402
import numpy as np  # noqa: E402
from matplotlib.colors import to_hex  # noqa: E402

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent.parent))

from generate import JOBS, job_from_spec, normalize  # noqa: E402

CHART_JOBS = [name for name, spec in JOBS.items() if "error" not in name and name not in
              ("mixed_dates", "missing_column", "missing_rule_column", "too_few")]


def num(v):
    """A JSON number (dates as date numbers), or null for NaN / masked."""
    if v is None or np.ma.is_masked(v):
        return None
    if not isinstance(v, (int, float, np.number)):
        v = mdates.date2num(v)
    f = float(v)
    return None if np.isnan(f) else f


def xs(values):
    if np.issubdtype(np.asarray(values).dtype, np.datetime64):
        values = mdates.date2num(values)
    return [num(v) for v in np.asarray(values, dtype=float)]


def color(c):
    c = np.asarray(c, dtype=float)
    if c.size == 0:
        return None
    if c.ndim == 2:
        c = c[0]
    return None if c[3] == 0 else to_hex(c)


def colors(rgba):
    rgba = np.asarray(rgba, dtype=float)
    return [color(c) for c in rgba] if rgba.ndim == 2 else [color(rgba)]


def axes_doc(ax, fig):
    lo, hi = ax.get_xlim()
    ylo, yhi = ax.get_ylim()
    within = lambda t, a, b: a - 1e-9 * max(1, abs(a)) <= t <= b + 1e-9 * max(1, abs(b))  # noqa: E731
    xticks = [[float(t), lbl.get_text()] for t, lbl in zip(ax.get_xticks(), ax.get_xticklabels())
              if within(t, lo, hi)]
    yticks = [[float(t), lbl.get_text()] for t, lbl in zip(ax.get_yticks(), ax.get_yticklabels())
              if within(t, ylo, yhi)]
    lines = []
    for ln in ax.get_lines():
        hline = ln.get_transform() != ax.transData
        lines.append({"kind": "hline" if hline else "line", "x": [] if hline else xs(ln.get_xdata()),
                      "y": [num(v) for v in np.asarray(ln.get_ydata(), dtype=float)],
                      "color": to_hex(ln.get_color()), "width": ln.get_linewidth(), "zorder": ln.get_zorder()})
    scatters = []
    for col in ax.collections:
        off = col.get_offsets()
        scatters.append({
            "x": [num(v) for v in np.ma.getdata(off[:, 0]).astype(float)] if len(off) else [],
            "y": [num(v) if not np.ma.is_masked(off[i, 1]) else None for i, v in enumerate(np.ma.getdata(off[:, 1]))]
            if len(off) else [],
            "face": colors(col.get_facecolors()), "edge": colors(col.get_edgecolors()),
            "sizes": [float(s) for s in col.get_sizes()], "width": [float(w) for w in col.get_linewidths()],
            "zorder": col.get_zorder(), "label": col.get_label()})
    bands = []
    for p in ax.patches:
        bands.append({"x0": float(p.get_x()), "x1": float(p.get_x() + p.get_width()),
                      "color": to_hex(p.get_facecolor()), "zorder": p.get_zorder()})
    texts = []
    for t in ax.texts:
        texts.append({"text": t.get_text(), "xy": [num(v) for v in t.xy] if hasattr(t, "xy") else None,
                      "xycoords": list(t.xycoords) if isinstance(t.xycoords, tuple) else t.xycoords,
                      "offset": list(t.xyann) if hasattr(t, "xyann") else None,
                      "ha": t.get_ha(), "va": t.get_va(), "color": to_hex(t.get_color()),
                      "size": t.get_fontsize(), "weight": t.get_fontweight(), "box": t.get_bbox_patch() is not None,
                      "zorder": t.get_zorder()})
    legend = None
    if ax.get_legend() is not None:
        leg = ax.get_legend()
        box = leg.get_window_extent().transformed(ax.transAxes.inverted())
        legend = {"labels": [t.get_text() for t in leg.get_texts()],
                  "colors": [color(h.get_facecolor()) for h in leg.legend_handles],
                  "bbox": [box.x0, box.y0, box.x1, box.y1]}
    return {"position": list(ax.get_position().bounds), "xlim": [lo, hi], "ylim": [ylo, yhi],
            "xticks": xticks, "yticks": yticks, "xlabel": ax.get_xlabel(), "ylabel": ax.get_ylabel(),
            "xrotation": ax.get_xticklabels()[0].get_rotation() if ax.get_xticklabels() else 0,
            "xtick_labels_visible": any(lbl.get_visible() and lbl.get_text() for lbl in ax.get_xticklabels()),
            "lines": lines, "scatters": scatters, "bands": bands, "texts": texts, "legend": legend}


def chart_doc(job):
    fig = job.figure()
    fig.canvas.draw()
    ax_i, ax_mr = fig.axes
    note = next(t for t in fig.texts if t is not fig._suptitle)
    return {"size": list(fig.get_size_inches()), "dpi": fig.dpi,
            "title": fig._suptitle.get_text(), "title_size": fig._suptitle.get_fontsize(),
            "note": {"text": note.get_text(), "position": list(note.get_position()), "size": note.get_fontsize(),
                     "color": to_hex(note.get_color())},
            "margins": {**{k: getattr(fig.subplotpars, k) for k in ("left", "right", "top", "bottom")},
                        "hspace": ax_i.get_gridspec().hspace},
            "panels": [axes_doc(ax_i, fig), axes_doc(ax_mr, fig)]}


def main():
    out = []
    for name in CHART_JOBS:
        job = job_from_spec(JOBS[name])
        out.append({"name": name, "spec": JOBS[name], "chart": chart_doc(job)})
    text = normalize(json.dumps(out, indent=1, ensure_ascii=False))
    (HERE / "charts.json").write_text(text + "\n", encoding="utf-8")
    print("chart fixtures written for", len(out), "charts")


if __name__ == "__main__":
    main()
