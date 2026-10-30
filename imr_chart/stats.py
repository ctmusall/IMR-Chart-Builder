"""Individual & Moving Range (I-MR) control chart statistics.

Matches JMP's defaults: sigma is estimated from the average moving range
(sigma = MRbar / d2, d2 = 1.128 for span 2). Individual limits are
mean +/- k*sigma; the moving-range upper limit is MRbar + k*d3*sigma
(d3 = 0.8525), which equals D4*MRbar = 3.267*MRbar when k = 3.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

D2 = 1.128
D3 = 0.8525


@dataclass
class ImrResult:
    values: np.ndarray
    moving_ranges: np.ndarray  # length n; first entry is NaN
    k: float
    mean: float
    mr_bar: float
    sigma: float
    ucl: float
    lcl: float
    mr_ucl: float
    mr_lcl: float
    lsl: float | None = None
    above_ucl: np.ndarray = field(default=None)
    below_lcl: np.ndarray = field(default=None)
    below_lsl: np.ndarray = field(default=None)
    mr_above_ucl: np.ndarray = field(default=None)

    @property
    def n(self) -> int:
        return len(self.values)

    @property
    def out_of_limits(self) -> np.ndarray:
        return self.above_ucl | self.below_lcl | self.below_lsl | self.mr_above_ucl


def compute_imr(values, k: float = 3.0, lsl: float | None = None) -> ImrResult:
    x = np.asarray(values, dtype=float)
    if x.ndim != 1:
        raise ValueError("values must be one-dimensional")
    if np.isnan(x).any():
        raise ValueError("values must not contain NaN")
    if len(x) < 2:
        raise ValueError("At least 2 data points are required for an I-MR chart")
    if k <= 0:
        raise ValueError("Sigma multiplier must be positive")

    mr = np.empty_like(x)
    mr[0] = np.nan
    mr[1:] = np.abs(np.diff(x))

    mean = float(x.mean())
    mr_bar = float(np.nanmean(mr))
    sigma = mr_bar / D2
    ucl = mean + k * sigma
    lcl = mean - k * sigma
    mr_ucl = mr_bar + k * D3 * sigma
    mr_lcl = max(0.0, mr_bar - k * D3 * sigma)

    with np.errstate(invalid="ignore"):
        mr_above = np.nan_to_num(mr, nan=-np.inf) > mr_ucl
    return ImrResult(
        values=x,
        moving_ranges=mr,
        k=k,
        mean=mean,
        mr_bar=mr_bar,
        sigma=sigma,
        ucl=ucl,
        lcl=lcl,
        mr_ucl=mr_ucl,
        mr_lcl=mr_lcl,
        lsl=lsl,
        above_ucl=x > ucl,
        below_lcl=x < lcl,
        below_lsl=(x < lsl) if lsl is not None else np.zeros(len(x), dtype=bool),
        mr_above_ucl=mr_above,
    )


def parse_limit(text: str | float | None) -> float | None:
    """Parse '2e7', '20,000,000', '50%' (-> 0.5). Blank -> None."""
    if text is None:
        return None
    if isinstance(text, (int, float)):
        return float(text)
    s = str(text).strip().replace(",", "")
    if not s:
        return None
    try:
        if s.endswith("%"):
            return float(s[:-1]) / 100.0
        return float(s)
    except ValueError:
        raise ValueError(f"Invalid limit value: '{text}'") from None
