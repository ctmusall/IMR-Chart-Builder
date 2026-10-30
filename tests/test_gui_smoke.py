import os

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
pytest.importorskip("PySide6")
pytest.importorskip("pytestqt")

from PySide6.QtWidgets import QMessageBox  # noqa: E402

from imr_chart.gui import NO_DATE, MainWindow  # noqa: E402


@pytest.fixture
def win(qtbot, monkeypatch):
    errors = []
    monkeypatch.setattr(QMessageBox, "critical", lambda *a, **k: errors.append(a[2]))
    w = MainWindow()
    w.errors = errors
    qtbot.addWidget(w)
    return w


def test_initial_state(win):
    assert not win.generate_btn.isEnabled()
    assert not win.save_png_btn.isEnabled()
    assert win.generate() is False
    assert win.save("png", "") is None


def test_load_generate_save(win, xlsx_with_header_tab, tmp_path):
    assert win.open_path(xlsx_with_header_tab)
    assert win.sheet_box.currentText() == "Summary"
    assert "QC3 CD34/Unit" in [win.column_box.itemText(i) for i in range(win.column_box.count())]
    assert win.date_box.currentText() == "Date Processed"

    win.column_box.setCurrentText("QC3 CD34/Unit")
    win.lsl_edit.setText("2e7")
    assert win.generate()
    assert win.canvas is not None
    assert "n=4" in win.stats_label.text() and "below LSL" in win.stats_label.text()
    assert win.save_png_btn.isEnabled()

    for fmt in ("png", "html", "xlsx"):
        out = win.save(fmt, "", path=str(tmp_path / f"chart_{fmt}"))
        assert out is not None and out.suffix == f".{fmt}" and out.stat().st_size > 0

    # Regenerating replaces the canvas without error.
    win.date_box.setCurrentText(NO_DATE)
    assert win.generate()
    assert win.errors == []


def test_reload_sheet_keeps_column_when_present(win, xlsx_with_header_tab):
    win.open_path(xlsx_with_header_tab)
    win.column_box.setCurrentText("QC3 CD34 Viability")
    assert win.load_sheet("Summary")
    assert win.column_box.currentText() == "QC3 CD34 Viability"
    win.sheet_box.setCurrentText("Query")
    assert win.column_box.currentText() == "x"


def test_errors_are_reported(win, xlsx_with_header_tab, tmp_path):
    bad = tmp_path / "bad.xlsx"
    bad.write_text("not a workbook")
    assert win.open_path(bad) is False
    assert win.errors

    win.open_path(xlsx_with_header_tab)
    win.lsl_edit.setText("abc")
    assert win.generate() is False
    assert "Invalid limit" in win.errors[-1]
    assert not win.save_png_btn.isEnabled()


def test_csv(win, csv_file):
    assert win.open_path(csv_file)
    assert win.generate()


def test_no_numeric_columns(win, tmp_path):
    p = tmp_path / "text.csv"
    p.write_text("a,b\nx,y\nz,w\n")
    assert win.open_path(p)
    assert win.generate() is False
    assert "no numeric columns" in win.errors[-1]


def test_multiple_files(win, xlsx_split, tmp_path):
    assert win.open_paths(xlsx_split)
    assert win.file_label.text().startswith("2 files: part1.xlsx")
    win.column_box.setCurrentText("QC3 CD34/Unit")
    assert win.generate()
    assert "files=2" in win.stats_label.text() and "n=4" in win.stats_label.text()
    for fmt in ("png", "html", "xlsx"):
        assert win.save(fmt, "", path=str(tmp_path / f"pooled_{fmt}")) is not None
    assert win.errors == []


def test_highlight_rules(win, xlsx_with_header_tab, tmp_path):
    from imr_chart.rules import Rule, load_rules, save_rules

    win.open_path(xlsx_with_header_tab)
    ed = win.rules_editor
    assert "Notes" in ed.columns
    ed.add_rule(Rule("note b", "Notes", "=", "b", "#ff00ff"))
    ed.add_rule({"column": "QC Lot", "op": "=", "value": "L2"})
    ed.move_rule(-1)  # second row (selected) moves to the top
    assert [r.label for r in ed.rules()] == ["QC Lot = 'L2'", "note b"]
    win.column_box.setCurrentText("QC3 CD34/Unit")
    assert win.generate()
    assert win.job.highlighted == 2 and "highlighted=2" in win.stats_label.text()

    path = save_rules(ed.rules(), tmp_path / "r.json")
    ed.set_rules([])
    assert ed.rules() == []
    ed.set_rules(load_rules(path))
    assert [r.color for r in ed.rules()] == ["#9467bd", "#ff00ff"]

    ed.add_rule()  # blank column: generating reports which rule is incomplete
    assert win.generate() is False
    assert "Highlight rule 3" in win.errors[-1]
