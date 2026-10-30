import matplotlib.colors
import numpy as np
import pandas as pd
import pytest

from imr_chart import plot_html, plot_static
from imr_chart.stats import compute_imr


def make_data(values, dates=True):
    n = len(values)
    return pd.DataFrame(
        {
            "date": pd.date_range("2024-01-01", periods=n) if dates else pd.Series([pd.NaT] * n),
            "label": [f"L{i}" for i in range(n)],
            "value": values,
        }
    )


def hline_values(ax):
    return sorted(round(line.get_ydata()[0], 6) for line in ax.get_lines()
                  if len(set(line.get_ydata())) == 1 and len(line.get_ydata()) == 2)


@pytest.fixture
def big():
    rng = np.random.default_rng(1)
    vals = np.abs(rng.normal(6e7, 3e7, 120))
    return make_data(vals), compute_imr(vals, lsl=2e7)


def test_static_two_panels_and_limit_lines(big):
    data, r = big
    fig = plot_static.build_figure(data, r, "QC3 CD34/Unit", excluded=3)
    ax_i, ax_mr = fig.axes
    lines_i = hline_values(ax_i)
    assert round(r.mean, 6) in lines_i and round(r.ucl, 6) in lines_i and round(r.lsl, 6) in lines_i
    assert round(r.lcl, 6) not in lines_i  # negative LCL hidden for non-negative data
    assert round(r.mr_ucl, 6) in hline_values(ax_mr)
    assert fig._suptitle.get_text() == "Individual & Moving Range chart of QC3 CD34/Unit"
    texts = " ".join(t.get_text() for t in fig.texts)
    assert "3 rows excluded" in texts
    assert any(t.get_text() == "LSL (2e7)" for t in ax_i.texts)


def test_static_lcl_drawn_when_meaningful():
    vals = [0.9, 0.85, 0.95, 0.88, 0.2, 0.92, 0.9]
    r = compute_imr(vals, lsl=0.5)
    fig = plot_static.build_figure(make_data(vals), r, "Viability")
    ax_i = fig.axes[0]
    assert round(r.lcl, 6) in hline_values(ax_i)
    assert ax_i.yaxis.get_major_formatter()(0.5, 0) == "50%"
    assert any(t.get_text() == "LSL (50%)" for t in ax_i.texts)


def test_static_without_dates_and_without_lsl():
    vals = [5.0, 6.0, 7.0, 6.5]
    r = compute_imr(vals)
    fig = plot_static.build_figure(make_data(vals, dates=False), r, "X", percent=False)
    assert fig.axes[1].get_xlabel() == "Observation"
    assert not any(t.get_text().startswith("LSL") for t in fig.axes[0].texts)


def test_save_png(tmp_path, big):
    data, r = big
    out = plot_static.save_png(plot_static.build_figure(data, r, "C"), tmp_path / "sub" / "c.png", dpi=50)
    assert out.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"


@pytest.mark.parametrize("x,percent,expected", [(2.5e8, False, "2.50e+8"), (0, False, "0"), (12.5, False, "12.5"),
                                                (0.5, True, "50%"), (1e-4, False, "1.00e-4")])
def test_format_number(x, percent, expected):
    assert plot_static.format_number(x, percent) == expected


@pytest.mark.parametrize("lsl,percent,expected", [(2e7, False, "LSL (2e7)"), (0.5, True, "LSL (50%)"),
                                                  (10, False, "LSL (10)")])
def test_lsl_label(lsl, percent, expected):
    assert plot_static.lsl_label(lsl, percent) == expected


def test_looks_like_fraction():
    assert plot_static.looks_like_fraction([0.1, 0.99])
    assert not plot_static.looks_like_fraction([1e7, 2e7])
    assert not plot_static.looks_like_fraction([-0.1, 0.5])


def test_html_figure(big, tmp_path):
    data, r = big
    fig = plot_html.build_html_figure(data, r, "QC3 CD34/Unit", excluded=2)
    assert len(fig.data) == 2
    ys = sorted(round(s["y0"], 6) for s in fig.layout.shapes)
    assert round(r.ucl, 6) in ys and round(r.mr_ucl, 6) in ys and round(r.lsl, 6) in ys
    assert list(fig.data[0].customdata[:, 0][:2]) == ["L0", "L1"]
    out = plot_html.save_html(fig, tmp_path / "c.html")
    html = out.read_text(encoding="utf-8")
    assert "<script src=\"http" not in html  # self-contained, works offline
    assert "Individual &amp; Moving Range" in html or "Individual & Moving Range" in html


def test_html_percent_no_dates():
    vals = [0.9, 0.2, 0.95, 0.88]
    r = compute_imr(vals)
    fig = plot_html.build_html_figure(make_data(vals, dates=False), r, "V")
    assert fig.layout.yaxis.tickformat == ".0%"
    assert fig.layout.xaxis2.title.text == "Observation"


def test_recent_mask():
    data = make_data(list(range(60)))  # daily from 2024-01-01
    mask = plot_static.recent_mask(data, 30)
    assert mask.sum() == 31 and mask[-31:].all()  # latest date and the 30 days before it
    assert plot_static.recent_mask(data, 0) is None
    assert plot_static.recent_mask(make_data([1.0, 2.0], dates=False), 30) is None


def band_count(ax):
    from matplotlib.patches import Polygon, Rectangle
    return sum(isinstance(p, (Polygon, Rectangle)) and p.get_facecolor()[:3] == matplotlib.colors.to_rgb(
        plot_static.RECENT_BAND_COLOR) for p in ax.patches)


def test_static_recent_highlight(big):
    data, r = big
    fig = plot_static.build_figure(data, r, "C")
    ax_i, ax_mr = fig.axes
    assert band_count(ax_i) == 1 and band_count(ax_mr) == 1
    recent_rgb = matplotlib.colors.to_rgba(plot_static.RECENT_COLOR)
    pts = [c for c in ax_i.collections if tuple(c.get_facecolor()[0]) == recent_rgb]
    assert len(pts) == 1 and len(pts[0].get_offsets()) == 31
    assert any(t.get_text() == "Last 30 days" for t in ax_i.texts)
    assert "Highlighted: last 30 days" in " ".join(t.get_text() for t in fig.texts)


def test_static_recent_off(big):
    data, r = big
    for fig in (plot_static.build_figure(data, r, "C", recent_days=0),
                plot_static.build_figure(make_data(r.values, dates=False), r, "C")):
        assert band_count(fig.axes[0]) == 0
        assert not any(t.get_text().startswith("Last") for t in fig.axes[0].texts)


def test_html_recent_highlight():
    vals = [5.0] * 40 + [50.0] + [5.0] * 19  # one flagged spike inside the last 30 days
    r = compute_imr(vals)
    data = make_data(vals)
    fig = plot_html.build_html_figure(data, r, "C", percent=False)
    colors = list(fig.data[0].marker.color)
    assert colors[0] == plot_static.POINT_COLOR
    assert colors[-1] == plot_static.RECENT_COLOR
    assert colors[40] == plot_static.LIMIT_COLOR  # flagged wins over recent
    assert any(s.type == "rect" and s.fillcolor == plot_static.RECENT_BAND_COLOR for s in fig.layout.shapes)
    off = plot_html.build_html_figure(data, r, "C", percent=False, recent_days=0)
    assert plot_static.RECENT_COLOR not in list(off.data[0].marker.color)


def ruled(values):
    data = make_data(values)
    data["rule"] = ["hot" if i % 3 == 0 else "" for i in range(len(values))]
    data["highlight"] = np.where(data["rule"] != "", "#ff00ff", "")
    return data


def test_static_rule_highlight():
    vals = [5.0, 6.0, 5.5, 50.0, 5.2, 5.8, 6.1]
    fig = plot_static.build_figure(ruled(vals), compute_imr(vals), "V", recent_days=0)
    ax_i, ax_mr = fig.axes
    for ax in (ax_i, ax_mr):
        magenta = [c for c in ax.collections
                   if len(c.get_facecolors()) and matplotlib.colors.to_hex(c.get_facecolors()[0]) == "#ff00ff"]
        assert len(magenta) == 1 and len(magenta[0].get_offsets()) == 3
    assert [t.get_text() for t in ax_i.get_legend().get_texts()] == ["hot (3)"]
    assert any("Highlighted by rules: 3" in t.get_text() for t in fig.texts)


def test_html_rule_highlight():
    vals = [5.0, 6.0, 5.5, 50.0, 5.2, 5.8, 6.1]
    r = compute_imr(vals, k=1)
    fig = plot_html.build_html_figure(ruled(vals), r, "V", recent_days=0)
    colors = list(fig.data[0].marker.color)
    assert colors.count("#ff00ff") == 3 and colors[1] == plot_static.POINT_COLOR
    flagged = r.above_ucl | r.below_lcl
    assert flagged[3] and fig.data[0].marker.line.color[3] == plot_static.LIMIT_COLOR  # ring kept
    assert fig.layout.showlegend and fig.data[2].name == "hot (3)" and fig.data[2].showlegend
    assert "Rule: hot" in fig.data[0].customdata[0, -1]


def test_no_rules_no_legend(big):
    data, r = big
    assert plot_static.build_figure(data, r, "V").axes[0].get_legend() is None
    assert not plot_html.build_html_figure(data, r, "V").layout.showlegend
