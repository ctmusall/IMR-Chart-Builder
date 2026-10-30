from pathlib import Path

import matplotlib
import pandas as pd
import pytest

matplotlib.use("Agg")

ROOT = Path(__file__).resolve().parent.parent
REAL_SHEET = ROOT / "sheet.xlsx"

HEADERS = ["QC Lot", "Date Processed", "QC3 CD34/Unit", "QC3 CD34 Viability", "Notes"]
# Deliberately out of date order, with one blank and one junk value.
ROWS = [
    ["L3", 45400, 7.0e7, 0.90, "a"],
    ["L1", 45363, 5.0e7, 0.80, "b"],
    ["L2", 45383, 9.0e7, 0.95, "c"],
    ["L4", 45410, None, 0.70, "d"],
    ["L5", 45420, "bad", 0.40, "e"],
    ["L6", 45430, 1.0e7, 0.60, "f"],
]


@pytest.fixture
def xlsx_with_header_tab(tmp_path) -> Path:
    """Mimics the real workbook: data tab without header + 'Column Headers (Summary)'."""
    path = tmp_path / "with_header_tab.xlsx"
    with pd.ExcelWriter(path) as xw:
        pd.DataFrame([["tag"]]).to_excel(xw, sheet_name="MC Tags", header=False, index=False)
        pd.DataFrame([["x", "y"], [1, 2]]).to_excel(xw, sheet_name="Query", header=False, index=False)
        pd.DataFrame(ROWS).to_excel(xw, sheet_name="Summary", header=False, index=False)
        pd.DataFrame([HEADERS]).to_excel(xw, sheet_name="Column Headers (Summary)", header=False, index=False)
    return path


@pytest.fixture
def xlsx_inline_header(tmp_path) -> Path:
    path = tmp_path / "inline.xlsx"
    pd.DataFrame(ROWS, columns=HEADERS).to_excel(path, sheet_name="Data", index=False)
    return path


@pytest.fixture
def xlsx_no_header(tmp_path) -> Path:
    path = tmp_path / "noheader.xlsx"
    pd.DataFrame([[1, 2.5], [2, 3.5], [3, 1.5]]).to_excel(path, header=False, index=False)
    return path


@pytest.fixture
def csv_file(tmp_path) -> Path:
    path = tmp_path / "data.csv"
    df = pd.DataFrame(ROWS, columns=HEADERS)
    df["Date Processed"] = ["2024-04-18", "2024-03-12", "2024-04-01", "2024-04-28", "2024-05-08", "2024-05-18"]
    df.to_csv(path, index=False)
    return path


@pytest.fixture
def real_sheet() -> Path:
    if not REAL_SHEET.exists():
        pytest.skip("sheet.xlsx sample not present")
    return REAL_SHEET


def _summary_workbook(path: Path, rows) -> Path:
    with pd.ExcelWriter(path) as xw:
        pd.DataFrame(rows).to_excel(xw, sheet_name="Summary", header=False, index=False)
        pd.DataFrame([HEADERS]).to_excel(xw, sheet_name="Column Headers (Summary)", header=False, index=False)
    return path


@pytest.fixture
def xlsx_split(tmp_path) -> list[Path]:
    """ROWS split across two workbooks with the same layout, so both use Excel rows 1-3."""
    return [_summary_workbook(tmp_path / "part1.xlsx", ROWS[:3]),
            _summary_workbook(tmp_path / "part2.xlsx", ROWS[3:])]
