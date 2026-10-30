"""Excel export of I-MR statistics, per-point data and out-of-limit rows.

Besides the program's results, the workbook carries the evidence for them:

- **Verification** recomputes every statistic with ordinary Excel formulas and
  compares it with the program's value (PASS/FAIL), so a reviewer can check the
  chart without trusting this program's arithmetic.
- **Recompute** does the per-point work behind that sheet (moving ranges, flags,
  and a lookup of each charted value back to its source row).
- **Worked formulas** writes each calculation out with this data's numbers.
- **Input audit** lists every source row with its raw cell values and why it
  was or wasn't charted.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pandas as pd
from openpyxl.formatting.rule import CellIsRule, FormulaRule
from openpyxl.styles import Font, PatternFill
from openpyxl.utils import get_column_letter

from .proof import CONSTANTS_SOURCE, worked_steps
from .stats import D2, D3, ImrResult

HEADER_FONT = Font(bold=True, color="FFFFFF")
HEADER_FILL = PatternFill("solid", fgColor="305496")
FLAG_FILL = PatternFill("solid", fgColor="F8CBAD")
PASS_FILL = PatternFill("solid", fgColor="C6EFCE")
FAIL_FILL = PatternFill("solid", fgColor="FFC7CE")
NOTE_FONT = Font(italic=True, color="555555")
# Relative tolerance for "same number": allows for Excel summing in a different order.
TOLERANCE = "1E-9"


def stats_rows(result: ImrResult, column: str, source: str = "", excluded: int = 0,
               provenance: dict | None = None) -> list[tuple[str, object]]:
    rows = [
        ("Source file", source),
        *(provenance or {}).items(),
        ("Column", column),
        ("Points (n)", result.n),
        ("Rows excluded", excluded),
        ("Mean (center line)", result.mean),
        ("Average moving range (MR̄)", result.mr_bar),
        ("Sigma (MR̄/1.128)", result.sigma),
        ("Sigma multiplier (k)", result.k),
        ("Individuals UCL", result.ucl),
        ("Individuals LCL", result.lcl),
        ("Moving range UCL", result.mr_ucl),
        ("Moving range LCL", result.mr_lcl),
        ("LSL", result.lsl if result.lsl is not None else "—"),
        ("Points above UCL", int(result.above_ucl.sum())),
        ("Points below LCL", int(result.below_lcl.sum())),
        ("Points below LSL", int(result.below_lsl.sum()) if result.lsl is not None else "—"),
        ("Moving ranges above MR UCL", int(result.mr_above_ucl.sum())),
    ]
    return rows


def _source_rows(data: pd.DataFrame, n: int) -> np.ndarray:
    return data["row"].to_numpy() if "row" in data else np.arange(1, n + 1)


def points_frame(data: pd.DataFrame, result: ImrResult) -> pd.DataFrame:
    return pd.DataFrame(
        {
            "Lot": data["label"].to_numpy() if "label" in data else range(1, result.n + 1),
            "Date": data["date"].dt.date.to_numpy() if "date" in data and data["date"].notna().all() else None,
            "Value": result.values,
            "Moving Range": result.moving_ranges,
            "Above UCL": result.above_ucl,
            "Below LCL": result.below_lcl,
            "Below LSL": result.below_lsl,
            "MR above UCL": result.mr_above_ucl,
            "Source row": _source_rows(data, result.n),
        }
    )


def audit_frame(audit: pd.DataFrame | None, data: pd.DataFrame, result: ImrResult) -> pd.DataFrame:
    """The Input audit sheet. Without a loader audit, every charted point is listed as included."""
    if audit is None:
        dates = data["date"] if "date" in data else pd.Series(pd.NaT, index=range(result.n))
        audit = pd.DataFrame({"row": _source_rows(data, result.n), "label": points_frame(data, result)["Lot"],
                              "raw_date": dates.to_numpy(), "raw_value": result.values, "date": dates.to_numpy(),
                              "value": result.values, "included": True, "reason": ""})
    return pd.DataFrame(
        {
            "Source row": audit["row"].to_numpy(),
            "Lot": audit["label"].to_numpy(),
            "Raw date": audit["raw_date"].to_numpy(),
            "Raw value": audit["raw_value"].to_numpy(),
            "Parsed date": pd.to_datetime(audit["date"]).dt.date.to_numpy(),
            "Parsed value": audit["value"].to_numpy(),
            "Included": np.where(audit["included"], "Yes", "No"),
            "Reason excluded": audit["reason"].to_numpy(),
        }
    )


def _style(ws, flag_cols: list[int] | None = None):
    for cell in ws[1]:
        cell.font = HEADER_FONT
        cell.fill = HEADER_FILL
    for i, col in enumerate(ws.iter_cols(min_row=1, max_row=min(ws.max_row, 200)), start=1):
        width = max(len(str(c.value)) if c.value is not None else 0 for c in col)
        ws.column_dimensions[get_column_letter(i)].width = min(max(width + 2, 10), 45)
    ws.freeze_panes = "A2"
    if flag_cols:
        for row in ws.iter_rows(min_row=2):
            if any(row[c - 1].value is True for c in flag_cols):
                for cell in row:
                    cell.fill = FLAG_FILL


def _same(a: str, b: str) -> str:
    return f"ABS({a}-{b})<={TOLERANCE}*MAX(1,ABS({b}))"


def _write_recompute(ws, n: int, n_audit: int, cells: dict[str, str], has_lsl: bool):
    """Per-point Excel formulas. Limits come from the Verification sheet's recomputed column."""
    headers = ["Point", "Value (Data)", "Value in source row", "Value matches source", "Moving range (Excel)",
               "MR matches Data", "Above UCL", "Below LCL", "Below LSL", "MR above UCL", "Flags match Data"]
    ws.append(headers)
    aud = f"'Input audit'!$A$2:$A${n_audit + 1}"
    raw = f"'Input audit'!$D$2:$D${n_audit + 1}"
    for i in range(1, n + 1):
        r = i + 1
        src = f"C{r}"
        parsed = f'NUMBERVALUE({src},".",",")'
        ws.append([
            i,
            f"=Data!C{r}",
            f"=INDEX({raw},MATCH(Data!I{r},{aud},0))",
            # Text cells ("50%", "1,000") are parsed the way the loader does, independent of locale.
            f'=IF(ISNUMBER({src}),{_same(src, f"B{r}")},'
            f'IFERROR({_same(parsed, f"B{r}")},FALSE))',
            "" if i == 1 else f"=ABS(B{r}-B{r - 1})",
            f"=ISBLANK(Data!D{r})" if i == 1 else f"={_same(f'E{r}', f'Data!D{r}')}",
            f"=B{r}>{cells['ucl']}",
            f"=B{r}<{cells['lcl']}",
            f"=B{r}<{cells['lsl']}" if has_lsl else "=FALSE",
            "=FALSE" if i == 1 else f"=E{r}>{cells['mr_ucl']}",
            f"=AND(G{r}=Data!E{r},H{r}=Data!F{r},I{r}=Data!G{r},J{r}=Data!H{r})",
        ])
    _style(ws)


def _write_verification(ws, result: ImrResult, excluded: int, n_audit: int) -> dict[str, str]:
    """Write the check table; returns the addresses of the Excel-recomputed limits."""
    n = result.n
    last = n + 1
    rc = lambda col, first=2: f"Recompute!{col}{first}:{col}{last}"  # noqa: E731
    audit_col = lambda col: f"'Input audit'!{col}2:{col}{n_audit + 1}"  # noqa: E731

    ws["A1"] = "Overall result"
    ws["A1"].font = Font(bold=True, size=14)
    ws["A2"] = ("Column C is calculated by Excel, using ordinary formulas on the Data and Input audit sheets. "
                "Column B is what the program calculated and drew on the chart. A check passes when they agree "
                f"to within a relative {TOLERANCE}.")
    ws["A3"] = ("If column C or the overall result is blank, click 'Enable Editing' so Excel can calculate. "
                "Each value in Data can be traced to its row in the source file through 'Source row'.")
    for c in ("A2", "A3"):
        ws[c].font = NOTE_FONT

    ws["A5"], ws["B5"] = "Inputs", "Value"
    params = [("Sigma multiplier (k)", result.k), ("LSL", result.lsl if result.lsl is not None else "none"),
              ("d₂ (moving range span 2)", D2), ("d₃ (moving range span 2)", D3)]
    for i, (name, value) in enumerate(params, start=6):
        ws[f"A{i}"], ws[f"B{i}"] = name, value
    k, lsl, d2, d3 = "Verification!$B$6", "Verification!$B$7", "Verification!$B$8", "Verification!$B$9"

    header_row = 11
    for col, text in zip("ABCDE", ["Check", "Program value", "Excel recomputed", "Difference", "Result"]):
        ws[f"{col}{header_row}"] = text
    checks: list[tuple[str, str, object, str]] = []  # (key, label, program value, excel formula)
    row_of: dict[str, int] = {}

    def add(key, label, program, formula):
        row_of[key] = header_row + 1 + len(checks)
        checks.append((key, label, program, formula))

    def c(key):
        return f"Verification!$C${row_of[key]}"

    add("rows", "Rows read from source", n_audit, f"=COUNT({audit_col('A')})")
    add("excluded", "Rows excluded", excluded, f'=COUNTIF({audit_col("G")},"No")')
    add("n", "Points charted (n)", n, f"=COUNT({rc('B')})")
    add("included", "Included rows in Input audit", n, f'=COUNTIF({audit_col("G")},"Yes")')
    add("src", "Charted values that differ from their source row", 0, f"=COUNTIF({rc('D')},FALSE)")
    add("mean", "Mean (Individuals center line)", result.mean, f"=AVERAGE({rc('B')})")
    add("mr_bad", "Moving ranges that differ from Data", 0, f"=COUNTIF({rc('F')},FALSE)")
    add("mr_bar", "Average moving range (MR̄)", result.mr_bar, f"=AVERAGE({rc('E', 3)})")
    add("sigma", "Sigma = MR̄ / d₂", result.sigma, f"={c('mr_bar')}/{d2}")
    add("ucl", "Individuals UCL = mean + k·σ", result.ucl, f"={c('mean')}+{k}*{c('sigma')}")
    add("lcl", "Individuals LCL = mean − k·σ", result.lcl, f"={c('mean')}-{k}*{c('sigma')}")
    add("mr_ucl", "Moving range UCL = MR̄ + k·d₃·σ", result.mr_ucl, f"={c('mr_bar')}+{k}*{d3}*{c('sigma')}")
    add("mr_lcl", "Moving range LCL = max(0, MR̄ − k·d₃·σ)", result.mr_lcl,
        f"=MAX(0,{c('mr_bar')}-{k}*{d3}*{c('sigma')})")
    add("above", "Points above UCL", int(result.above_ucl.sum()), f"=COUNTIF({rc('G')},TRUE)")
    add("below", "Points below LCL", int(result.below_lcl.sum()), f"=COUNTIF({rc('H')},TRUE)")
    if result.lsl is not None:
        add("below_lsl", "Points below LSL", int(result.below_lsl.sum()), f"=COUNTIF({rc('I')},TRUE)")
    add("mr_above", "Moving ranges above MR UCL", int(result.mr_above_ucl.sum()), f"=COUNTIF({rc('J')},TRUE)")
    add("flags", "Points whose flags differ from Data", 0, f"=COUNTIF({rc('K')},FALSE)")

    for key, label, program, formula in checks:
        r = row_of[key]
        ws[f"A{r}"], ws[f"B{r}"], ws[f"C{r}"] = label, program, formula
        ws[f"D{r}"] = f"=C{r}-B{r}"
        ws[f"E{r}"] = f'=IF({_same(f"C{r}", f"B{r}")},"PASS","FAIL")'
    first, end = header_row + 1, header_row + len(checks)
    results = f"E{first}:E{end}"
    ws["B1"] = (f'=IF(AND(COUNTIF({results},"FAIL")=0,COUNTIF({results},"PASS")={len(checks)}),'
                f'"ALL {len(checks)} CHECKS PASS","CHECK FAILED")')
    ws["B1"].font = Font(bold=True, size=14)

    ws.conditional_formatting.add(results, CellIsRule(operator="equal", formula=['"PASS"'], fill=PASS_FILL))
    ws.conditional_formatting.add(results, CellIsRule(operator="equal", formula=['"FAIL"'], fill=FAIL_FILL))
    ws.conditional_formatting.add("B1", FormulaRule(formula=['LEFT(B1,3)="ALL"'], fill=PASS_FILL))
    ws.conditional_formatting.add("B1", FormulaRule(formula=['B1="CHECK FAILED"'], fill=FAIL_FILL))
    for cell in ws[5] + ws[header_row]:
        if cell.value is not None:
            cell.font, cell.fill = HEADER_FONT, HEADER_FILL
    for col, width in zip("ABCDE", (48, 20, 20, 14, 10)):
        ws.column_dimensions[col].width = width
    for row in ws.iter_rows(min_row=first, max_row=end, min_col=2, max_col=4):
        for cell in row:
            cell.number_format = "0.000000E+00"
    return {key: c(key) for key in ("ucl", "lcl", "mr_ucl")} | {"lsl": lsl}


def _write_worked(ws, result: ImrResult):
    ws.append(["Quantity", "Formula", "With this data's numbers", "Result"])
    for quantity, formula, numbers, value in worked_steps(result):
        ws.append([quantity, formula, numbers, None if isinstance(value, float) and np.isnan(value) else value])
    ws.append([])
    ws.append(["Notes"])
    notes = [
        f"k (sigma multiplier) = {result.k:g}. Points and moving ranges are flagged when strictly beyond a limit.",
        CONSTANTS_SOURCE,
        "Limits are calculated from every charted point, including the highlighted recent window.",
        "The Verification sheet recomputes each result above with Excel formulas.",
        "Numbers are stored to 16 significant digits (Excel displays 15), so a source value may differ "
        "in its 17th digit.",
    ]
    for note in notes:
        ws.append([note])
    _style(ws)
    ws.column_dimensions["A"].width = 42


def save_xlsx(
    data: pd.DataFrame,
    result: ImrResult,
    column: str,
    path: str | Path,
    source: str = "",
    excluded: int = 0,
    audit: pd.DataFrame | None = None,
    provenance: dict | None = None,
) -> Path:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    points = points_frame(data, result)
    flagged = points[result.out_of_limits]
    stats = pd.DataFrame(stats_rows(result, column, source, excluded, provenance), columns=["Statistic", "Value"])
    audit_sheet = audit_frame(audit, data, result)
    with pd.ExcelWriter(path, engine="openpyxl") as xw:
        stats.to_excel(xw, sheet_name="Summary stats", index=False)
        cells = _write_verification(xw.book.create_sheet("Verification"), result, excluded, len(audit_sheet))
        _write_worked(xw.book.create_sheet("Worked formulas"), result)
        points.to_excel(xw, sheet_name="Data", index=False)
        _write_recompute(xw.book.create_sheet("Recompute"), result.n, len(audit_sheet), cells,
                         result.lsl is not None)
        audit_sheet.to_excel(xw, sheet_name="Input audit", index=False)
        flagged.to_excel(xw, sheet_name="Out of limits", index=False)
        _style(xw.sheets["Summary stats"])
        _style(xw.sheets["Data"], flag_cols=[5, 6, 7, 8])
        _style(xw.sheets["Input audit"])
        _style(xw.sheets["Out of limits"])
        xw.book.calculation.fullCalcOnLoad = True
    return path
