import numpy as np
import pytest

from imr_chart.stats import D2, D3, compute_imr, parse_limit


def test_hand_computed_series():
    r = compute_imr([1, 3, 2, 5])
    assert r.mean == pytest.approx(2.75)
    np.testing.assert_allclose(r.moving_ranges[1:], [2, 1, 3])
    assert np.isnan(r.moving_ranges[0])
    assert r.mr_bar == pytest.approx(2.0)
    assert r.sigma == pytest.approx(2.0 / 1.128)
    assert r.ucl == pytest.approx(2.75 + 3 * 2.0 / 1.128)
    assert r.lcl == pytest.approx(2.75 - 3 * 2.0 / 1.128)
    assert r.mr_lcl == 0.0
    assert r.n == 4


def test_mr_ucl_equals_d4_times_mrbar_for_k3():
    rng = np.random.default_rng(0)
    r = compute_imr(rng.normal(100, 5, 200))
    assert r.mr_ucl == pytest.approx(3.267 * r.mr_bar, rel=1e-3)


def test_sigma_multiplier_scales_limits():
    data = [10, 12, 11, 15, 9, 13]
    r3 = compute_imr(data, k=3)
    r2 = compute_imr(data, k=2)
    assert r2.ucl - r2.mean == pytest.approx((r3.ucl - r3.mean) * 2 / 3)
    assert r2.mr_ucl == pytest.approx(r2.mr_bar + 2 * D3 * r2.mr_bar / D2)


def test_flags():
    data = [10, 10, 10, 10, 10, 10, 10, 10, 10, 30]
    r = compute_imr(data, lsl=10.5)
    assert r.above_ucl[-1]
    assert not r.above_ucl[:-1].any()
    assert not r.below_lcl.any()
    assert r.below_lsl[:-1].all() and not r.below_lsl[-1]
    assert r.mr_above_ucl[-1] and not r.mr_above_ucl[0]
    assert r.out_of_limits.sum() == 10


def test_no_lsl_means_no_lsl_flags():
    r = compute_imr([1, 2, 3])
    assert r.lsl is None
    assert not r.below_lsl.any()


def test_constant_series():
    r = compute_imr([5, 5, 5])
    assert r.sigma == 0
    assert r.ucl == r.lcl == 5
    assert not r.out_of_limits.any()


@pytest.mark.parametrize("bad", [[1], [], [[1, 2], [3, 4]], [1, float("nan")]])
def test_invalid_inputs(bad):
    with pytest.raises(ValueError):
        compute_imr(bad)


def test_nonpositive_k():
    with pytest.raises(ValueError):
        compute_imr([1, 2, 3], k=0)


@pytest.mark.parametrize(
    "text,expected",
    [("2e7", 2e7), ("20,000,000", 2e7), ("50%", 0.5), (" 0.5 ", 0.5), ("", None), (None, None), (3, 3.0)],
)
def test_parse_limit(text, expected):
    assert parse_limit(text) == expected


def test_parse_limit_invalid():
    with pytest.raises(ValueError, match="Invalid limit"):
        parse_limit("abc")
