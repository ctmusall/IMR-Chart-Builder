"""Evidence that a chart matches its input: file fingerprint and worked calculations."""

from __future__ import annotations

import hashlib
from pathlib import Path

from .stats import D2, D3, ImrResult

CONSTANTS_SOURCE = ("d₂ = 1.128 and d₃ = 0.8525 are the standard control chart constants for a moving range of "
                    "span 2 (e.g. Montgomery, Introduction to Statistical Quality Control, Appendix VI; "
                    "ASTM E2587). They are the same constants JMP uses by default.")


def file_sha256(path: str | Path) -> str | None:
    """SHA-256 of the file's bytes, or None when the file can't be read."""
    try:
        h = hashlib.sha256()
        with open(path, "rb") as f:
            for block in iter(lambda: f.read(1 << 20), b""):
                h.update(block)
        return h.hexdigest()
    except OSError:
        return None


def _g(x: float) -> str:
    return f"{x:.6g}"


def worked_steps(result: ImrResult) -> list[tuple[str, str, str, float]]:
    """(quantity, formula, formula with this data's numbers, result) for every chart statistic."""
    r = result
    n = r.n
    mr_sum = float(sum(r.moving_ranges[1:]))
    k = r.k
    steps = [
        ("Points charted (n)", "count of included rows", f"{n}", n),
        ("Mean, x̄ (Individuals center line)", "x̄ = Σxᵢ / n",
         f"{_g(float(r.values.sum()))} / {n}", r.mean),
        ("Moving range, MRᵢ", "MRᵢ = |xᵢ − xᵢ₋₁|,  i = 2…n",
         f"{n - 1} values (first point has none); see the Data sheet", float("nan")),
        ("Average moving range, MR̄ (MR center line)", "MR̄ = ΣMRᵢ / (n − 1)",
         f"{_g(mr_sum)} / {n - 1}", r.mr_bar),
        ("Sigma estimate, σ̂", "σ̂ = MR̄ / d₂",
         f"{_g(r.mr_bar)} / {D2}", r.sigma),
        ("Individuals UCL", "UCL = x̄ + k·σ̂",
         f"{_g(r.mean)} + {k:g} × {_g(r.sigma)}", r.ucl),
        ("Individuals LCL", "LCL = x̄ − k·σ̂",
         f"{_g(r.mean)} − {k:g} × {_g(r.sigma)}", r.lcl),
        ("Moving range UCL", "MR UCL = MR̄ + k·d₃·σ̂",
         f"{_g(r.mr_bar)} + {k:g} × {D3} × {_g(r.sigma)}", r.mr_ucl),
        ("Moving range LCL", "MR LCL = max(0, MR̄ − k·d₃·σ̂)",
         f"max(0, {_g(r.mr_bar)} − {k:g} × {D3} × {_g(r.sigma)})", r.mr_lcl),
    ]
    if k == 3:
        steps.append(("Check: MR UCL at k = 3", "MR UCL = D₄·MR̄,  D₄ = 1 + 3·d₃/d₂ = 3.267",
                      f"{1 + 3 * D3 / D2:.4f} × {_g(r.mr_bar)}", (1 + 3 * D3 / D2) * r.mr_bar))
    steps += [
        ("Points above UCL", "count of xᵢ > UCL", f"xᵢ > {_g(r.ucl)}", int(r.above_ucl.sum())),
        ("Points below LCL", "count of xᵢ < LCL", f"xᵢ < {_g(r.lcl)}", int(r.below_lcl.sum())),
    ]
    if r.lsl is not None:
        steps.append(("Points below LSL", "count of xᵢ < LSL", f"xᵢ < {_g(r.lsl)}", int(r.below_lsl.sum())))
    steps.append(("Moving ranges above MR UCL", "count of MRᵢ > MR UCL", f"MRᵢ > {_g(r.mr_ucl)}",
                  int(r.mr_above_ucl.sum())))
    return steps
