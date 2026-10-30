import pytest

from imr_chart import __main__ as entry
from imr_chart.cli import main


def test_list_columns(xlsx_with_header_tab, capsys):
    assert main([str(xlsx_with_header_tab), "--list-columns"]) == 0
    out = capsys.readouterr().out
    assert "Summary" in out and "QC3 CD34/Unit" in out and "Date Processed" in out


def test_generate_all_formats(xlsx_with_header_tab, tmp_path, capsys):
    rc = main([str(xlsx_with_header_tab), "-c", "QC3 CD34/Unit", "--lsl", "2e7", "-o", str(tmp_path)])
    assert rc == 0
    for ext in ("png", "html", "xlsx"):
        assert (tmp_path / f"IMR_QC3_CD34_Unit.{ext}").stat().st_size > 0
    assert "excluded=2" in capsys.readouterr().out


def test_multiple_columns_and_formats(csv_file, tmp_path):
    rc = main([str(csv_file), "-c", "QC3 CD34/Unit", "-c", "QC3 CD34 Viability", "--lsl", "2e7", "--lsl", "50%",
               "--formats", "png", "--sigma", "2", "-o", str(tmp_path / "out")])
    assert rc == 0
    assert sorted(p.name for p in (tmp_path / "out").iterdir()) == ["IMR_QC3_CD34_Unit.png", "IMR_QC3_CD34_Viability.png"]


def test_no_date(xlsx_with_header_tab, tmp_path):
    assert main([str(xlsx_with_header_tab), "-c", "QC3 CD34 Viability", "--no-date", "--formats", "xlsx",
                 "-o", str(tmp_path)]) == 0


@pytest.mark.parametrize(
    "extra,code,msg",
    [
        ([], 2, "--column is required"),
        (["-c", "Nope"], 1, "Column 'Nope' not found"),
        (["-c", "QC3 CD34/Unit", "--formats", "gif"], 2, "unknown format"),
        (["-c", "QC3 CD34/Unit", "--lsl", "abc"], 1, "Invalid limit"),
        (["-c", "QC3 CD34/Unit", "-c", "QC3 CD34 Viability", "--lsl", "1", "--lsl", "2", "--lsl", "3"], 2,
         "one per --column"),
        (["--sheet", "Nope", "--list-columns"], 1, "not found"),
    ],
)
def test_errors(xlsx_with_header_tab, tmp_path, capsys, extra, code, msg):
    assert main([str(xlsx_with_header_tab), "-o", str(tmp_path), *extra]) == code
    assert msg in capsys.readouterr().err


def test_missing_file(tmp_path, capsys):
    assert main([str(tmp_path / "x.xlsx"), "--list-columns"]) == 1
    assert "File not found" in capsys.readouterr().err


def test_real_workbook_end_to_end(real_sheet, tmp_path, capsys):
    rc = main([str(real_sheet), "-c", "QC3 CD34/Unit", "--lsl", "2e7", "-o", str(tmp_path)])
    assert rc == 0
    assert len(list(tmp_path.iterdir())) == 3


def test_entry_routing(monkeypatch):
    assert entry.wants_gui([])
    assert entry.wants_gui(["file.xlsx"])
    assert not entry.wants_gui(["file.xlsx", "-c", "X"])
    assert not entry.wants_gui(["--help"])

    called = {}
    monkeypatch.setattr("sys.argv", ["imr-chart", "f.xlsx", "--list-columns"])
    monkeypatch.setattr("imr_chart.cli.main", lambda argv: called.setdefault("cli", argv) and 0)
    entry.main()
    assert called["cli"] == ["f.xlsx", "--list-columns"]
