import pandas as pd
import pytest

from imr_chart import loader


def test_list_sheets_hides_helpers_and_prefers_summary(xlsx_with_header_tab):
    assert loader.list_sheets(xlsx_with_header_tab) == ["Summary", "Query"]


def test_list_sheets_csv(csv_file):
    assert loader.list_sheets(csv_file) == ["data"]


def test_header_tab_resolution(xlsx_with_header_tab):
    df = loader.load_table(xlsx_with_header_tab)
    assert list(df.columns) == ["QC Lot", "Date Processed", "QC3 CD34/Unit", "QC3 CD34 Viability", "Notes"]
    assert len(df) == 6
    assert df.iloc[0]["QC Lot"] == "L3"


def test_inline_header(xlsx_inline_header):
    df = loader.load_table(xlsx_inline_header, "Data")
    assert "QC3 CD34/Unit" in df.columns
    assert len(df) == 6


def test_no_header_gets_generic_names(xlsx_no_header):
    df = loader.load_table(xlsx_no_header)
    assert list(df.columns) == ["Column 1", "Column 2"]
    assert len(df) == 3


def test_csv(csv_file):
    df = loader.load_table(csv_file)
    assert len(df) == 6 and "Date Processed" in df.columns


def test_missing_file(tmp_path):
    with pytest.raises(FileNotFoundError):
        loader.load_table(tmp_path / "nope.xlsx")


def test_missing_sheet(xlsx_with_header_tab):
    with pytest.raises(ValueError, match="not found"):
        loader.load_table(xlsx_with_header_tab, "Nope")


def test_dedupe():
    assert loader._dedupe(["a", "a", None, " "]) == ["a", "a (2)", "Column 3", "Column 4"]


def test_numeric_and_date_columns(xlsx_with_header_tab):
    df = loader.load_table(xlsx_with_header_tab)
    num = loader.numeric_columns(df)
    assert "QC3 CD34/Unit" in num and "QC3 CD34 Viability" in num
    assert "Notes" not in num and "QC Lot" not in num
    assert loader.pick_date_column(df) == "Date Processed"
    assert loader.pick_label_column(df) == "QC Lot"


def test_pick_columns_none():
    df = pd.DataFrame({"a": [1, 2]})
    assert loader.pick_date_column(df) is None
    assert loader.pick_label_column(df) is None


def test_to_numeric_percent_and_junk():
    s = loader.to_numeric(pd.Series(["50%", "1,000", "x", None, 3, "bad%"]))
    assert s.iloc[0] == 0.5 and s.iloc[1] == 1000 and s.iloc[4] == 3
    assert s.iloc[[2, 3, 5]].isna().all()


def test_to_datetime_serial_and_strings():
    s = loader.to_datetime(pd.Series([45363, "2024-04-01", None, "garbage"]))
    assert s.iloc[0] == pd.Timestamp("2024-03-12")
    assert s.iloc[1] == pd.Timestamp("2024-04-01")
    assert s.iloc[[2, 3]].isna().all()


def test_to_datetime_passthrough():
    s = pd.Series(pd.to_datetime(["2024-01-01"]))
    assert loader.to_datetime(s) is s


def test_prepare_series_excludes_and_sorts(xlsx_with_header_tab):
    df = loader.load_table(xlsx_with_header_tab)
    data, excluded = loader.prepare_series(df, "QC3 CD34/Unit", "Date Processed", "QC Lot")
    assert excluded == 2  # blank + "bad"
    assert list(data["label"]) == ["L1", "L2", "L3", "L6"]
    assert data["date"].is_monotonic_increasing
    assert list(data["value"]) == [5e7, 9e7, 7e7, 1e7]


def test_prepare_series_csv_dates(csv_file):
    df = loader.load_table(csv_file)
    data, excluded = loader.prepare_series(df, "QC3 CD34 Viability", "Date Processed", "QC Lot")
    assert excluded == 0
    assert data["label"].iloc[0] == "L1"


def test_prepare_series_without_date_keeps_order(xlsx_with_header_tab):
    df = loader.load_table(xlsx_with_header_tab)
    data, _ = loader.prepare_series(df, "QC3 CD34 Viability")
    assert list(data["label"]) == ["1", "2", "3", "4", "5", "6"]
    assert data["date"].isna().all()


@pytest.mark.parametrize("kw", [{"value_col": "nope"}, {"value_col": "Notes", "date_col": "nope"},
                                {"value_col": "Notes", "label_col": "nope"}])
def test_prepare_series_bad_columns(xlsx_with_header_tab, kw):
    df = loader.load_table(xlsx_with_header_tab)
    with pytest.raises(KeyError):
        loader.prepare_series(df, **kw)


def test_real_workbook(real_sheet):
    assert loader.list_sheets(real_sheet)[0] == "Summary"
    df = loader.load_table(real_sheet)
    assert len(df) > 1000
    assert {"QC Lot", "Date Processed", "QC3 CD34/Unit", "QC3 CD34 Viability"} <= set(df.columns)
    data, excluded = loader.prepare_series(df, "QC3 CD34/Unit", "Date Processed", "QC Lot")
    assert len(data) + excluded == len(df)
    assert data["date"].min() >= pd.Timestamp("2020-01-01")
