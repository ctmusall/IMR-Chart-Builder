import openpyxl
import pandas as pd

from imr_chart.export_xlsx import save_xlsx
from imr_chart.stats import compute_imr


def test_xlsx_export(tmp_path):
    vals = [10, 10, 10, 10, 10, 10, 10, 10, 10, 30]
    data = pd.DataFrame({"date": pd.date_range("2024-01-01", periods=10),
                         "label": [f"L{i}" for i in range(10)], "value": vals})
    r = compute_imr(vals, lsl=5)
    path = save_xlsx(data, r, "My Col", tmp_path / "x" / "out.xlsx", source="in.xlsx", excluded=4)

    wb = openpyxl.load_workbook(path)
    assert wb.sheetnames == ["Summary stats", "Data", "Out of limits"]
    stats = {row[0]: row[1] for row in wb["Summary stats"].iter_rows(min_row=2, values_only=True)}
    assert stats["Column"] == "My Col"
    assert stats["Source file"] == "in.xlsx"
    assert stats["Points (n)"] == 10
    assert stats["Rows excluded"] == 4
    assert stats["Mean (center line)"] == r.mean
    assert stats["Individuals UCL"] == r.ucl
    assert stats["LSL"] == 5
    assert stats["Points above UCL"] == 1
    assert stats["Points below LSL"] == 0

    data_ws = wb["Data"]
    assert data_ws.max_row == 11
    assert [c.value for c in data_ws[1]][:4] == ["Lot", "Date", "Value", "Moving Range"]
    oob = list(wb["Out of limits"].iter_rows(min_row=2, values_only=True))
    assert len(oob) == 1 and oob[0][0] == "L9"


def test_xlsx_export_without_lsl_or_dates(tmp_path):
    vals = [1.0, 2.0, 3.0]
    data = pd.DataFrame({"date": [pd.NaT] * 3, "label": ["a", "b", "c"], "value": vals})
    path = save_xlsx(data, compute_imr(vals), "C", tmp_path / "o.xlsx")
    stats = {row[0]: row[1] for row in openpyxl.load_workbook(path)["Summary stats"].iter_rows(min_row=2, values_only=True)}
    assert stats["LSL"] == "—"
    assert stats["Points below LSL"] == "—"
