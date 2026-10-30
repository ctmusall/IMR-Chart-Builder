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
