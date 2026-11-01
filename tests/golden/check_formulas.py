"""Check that a workbook written by the Rust port proves itself: Excel's formulas recompute every
number and pass, and changing one charted value makes the right checks fail.

    python tests/golden/check_formulas.py WORKBOOK.xlsx [WORKBOOK.xlsx ...]

Needs the `formulas` package (pip install formulas), which calculates a workbook the way Excel
does. Used by `just verify-xlsx` and CI.
"""

from __future__ import annotations

import shutil
import sys
import tempfile
from pathlib import Path

import formulas
import openpyxl


def evaluate(path: Path) -> dict:
    """Calculate a workbook like Excel would; returns {(sheet, cell): value}."""
    sol = formulas.ExcelModel().loads(str(path)).finish().calculate()
    out = {}
    for key, value in sol.items():
        if not hasattr(value, "value"):
            continue  # an internal entry of the formulas package, not a cell
        sheet, _, cell = str(key).rpartition("!")
        if ":" not in cell:
            out[(sheet.rsplit("]", 1)[-1].strip("'").upper(), cell)] = value.value[0, 0]
    return out


def checks(values: dict) -> dict:
    rows = sorted(int(c[1:]) for (s, c) in values if s == "VERIFICATION" and c.startswith("E") and c[1:].isdigit()
                  and int(c[1:]) > 11)
    return {values[("VERIFICATION", f"A{r}")]: values[("VERIFICATION", f"E{r}")] for r in rows
            if ("VERIFICATION", f"A{r}") in values}


def check(path: Path) -> list[str]:
    problems = []
    values = evaluate(path)
    overall = values[("VERIFICATION", "B1")]
    results = checks(values)
    if not str(overall).startswith("ALL ") or set(results.values()) != {"PASS"}:
        problems.append(f"{path}: {overall}; failing: {[k for k, v in results.items() if v != 'PASS']}")
    # Tamper with one charted value: the checks that depend on it must fail, the others still pass.
    with tempfile.TemporaryDirectory() as tmp:
        tampered = Path(tmp) / "tampered.xlsx"
        shutil.copy(path, tampered)
        wb = openpyxl.load_workbook(tampered)
        wb["Data"]["C3"] = wb["Data"]["C3"].value * 1.01
        wb.save(tampered)
        values = evaluate(tampered)
        results = checks(values)
        if values[("VERIFICATION", "B1")] != "CHECK FAILED":
            problems.append(f"{path}: tampering not detected ({values[('VERIFICATION', 'B1')]})")
        for name in ("Charted values that differ from their source row", "Mean (Individuals center line)"):
            if results.get(name) != "FAIL":
                problems.append(f"{path}: after tampering, '{name}' is {results.get(name)}")
        if results.get("Rows read from source") != "PASS":
            problems.append(f"{path}: after tampering, 'Rows read from source' is {results.get('Rows read from source')}")
    return problems


if __name__ == "__main__":
    problems = [p for arg in sys.argv[1:] for p in check(Path(arg))]
    for p in problems:
        print(p)
    print("formula check:", "FAILED" if problems else f"all {len(sys.argv) - 1} workbook(s) pass")
    sys.exit(1 if problems else 0)
