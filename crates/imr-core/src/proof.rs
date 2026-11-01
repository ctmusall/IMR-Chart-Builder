//! Evidence that a chart matches its input: file fingerprints and worked calculations.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::pyfmt::{py_float_repr, py_format_g};
use crate::stats::{D2, D3, ImrResult, np_sum, py_sum};

/// SHA-256 of the file's bytes as lowercase hex, or None when the file can't be read.
pub fn file_sha256(path: impl AsRef<Path>) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub const CONSTANTS_SOURCE: &str = "d₂ = 1.128 and d₃ = 0.8525 are the standard control chart constants for a \
moving range of span 2 (e.g. Montgomery, Introduction to Statistical Quality Control, Appendix VI; ASTM E2587). \
They are the same constants JMP uses by default.";

/// A worked step's result: a count, or a number (NaN when the step has no single result).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepValue {
    Int(i64),
    Float(f64),
}

/// One calculation written out: the quantity, its formula, the formula with this data's numbers,
/// and the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub quantity: &'static str,
    pub formula: &'static str,
    pub numbers: String,
    pub value: StepValue,
}

fn g(x: f64) -> String {
    py_format_g(x, 6)
}

/// Every chart statistic, worked through with this data's numbers.
pub fn worked_steps(r: &ImrResult) -> Vec<Step> {
    let n = r.n();
    let mr_sum = py_sum(&r.moving_ranges[1..]);
    let k = py_format_g(r.k, 6);
    let step = |quantity, formula, numbers: String, value| Step { quantity, formula, numbers, value };
    let count = |flags: &[bool]| StepValue::Int(flags.iter().filter(|b| **b).count() as i64);
    let mut steps = vec![
        step("Points charted (n)", "count of included rows", n.to_string(), StepValue::Int(n as i64)),
        step(
            "Mean, x̄ (Individuals center line)",
            "x̄ = Σxᵢ / n",
            format!("{} / {n}", g(np_sum(&r.values))),
            StepValue::Float(r.mean),
        ),
        step(
            "Moving range, MRᵢ",
            "MRᵢ = |xᵢ − xᵢ₋₁|,  i = 2…n",
            format!("{} values (first point has none); see the Data sheet", n - 1),
            StepValue::Float(f64::NAN),
        ),
        step(
            "Average moving range, MR̄ (MR center line)",
            "MR̄ = ΣMRᵢ / (n − 1)",
            format!("{} / {}", g(mr_sum), n - 1),
            StepValue::Float(r.mr_bar),
        ),
        step(
            "Sigma estimate, σ̂",
            "σ̂ = MR̄ / d₂",
            format!("{} / {}", g(r.mr_bar), py_float_repr(D2)),
            StepValue::Float(r.sigma),
        ),
        step(
            "Individuals UCL",
            "UCL = x̄ + k·σ̂",
            format!("{} + {k} × {}", g(r.mean), g(r.sigma)),
            StepValue::Float(r.ucl),
        ),
        step(
            "Individuals LCL",
            "LCL = x̄ − k·σ̂",
            format!("{} − {k} × {}", g(r.mean), g(r.sigma)),
            StepValue::Float(r.lcl),
        ),
        step(
            "Moving range UCL",
            "MR UCL = MR̄ + k·d₃·σ̂",
            format!("{} + {k} × {} × {}", g(r.mr_bar), py_float_repr(D3), g(r.sigma)),
            StepValue::Float(r.mr_ucl),
        ),
        step(
            "Moving range LCL",
            "MR LCL = max(0, MR̄ − k·d₃·σ̂)",
            format!("max(0, {} − {k} × {} × {})", g(r.mr_bar), py_float_repr(D3), g(r.sigma)),
            StepValue::Float(r.mr_lcl),
        ),
    ];
    if r.k == 3.0 {
        let d4 = 1.0 + 3.0 * D3 / D2;
        steps.push(step(
            "Check: MR UCL at k = 3",
            "MR UCL = D₄·MR̄,  D₄ = 1 + 3·d₃/d₂ = 3.267",
            format!("{d4:.4} × {}", g(r.mr_bar)),
            StepValue::Float(d4 * r.mr_bar),
        ));
    }
    steps.push(step(
        "Points above UCL",
        "count of xᵢ > UCL",
        format!("xᵢ > {}", g(r.ucl)),
        count(&r.above_ucl),
    ));
    steps.push(step(
        "Points below LCL",
        "count of xᵢ < LCL",
        format!("xᵢ < {}", g(r.lcl)),
        count(&r.below_lcl),
    ));
    if let Some(lsl) = r.lsl {
        steps.push(step(
            "Points below LSL",
            "count of xᵢ < LSL",
            format!("xᵢ < {}", g(lsl)),
            count(&r.below_lsl),
        ));
    }
    steps.push(step(
        "Moving ranges above MR UCL",
        "count of MRᵢ > MR UCL",
        format!("MRᵢ > {}", g(r.mr_ucl)),
        count(&r.mr_above_ucl),
    ));
    steps
}
