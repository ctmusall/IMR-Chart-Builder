import os

import pytest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
pytest.importorskip("PySide6")
pytest.importorskip("pytestqt")

from PySide6.QtWidgets import QMessageBox  # noqa: E402

from imr_chart.gui import NO_DATE, MainWindow  # noqa: E402


def pick(box, text):
    """Choose ``text`` in a combo box the way a user does (which also emits textActivated)."""
    box.setCurrentText(text)
    box.textActivated.emit(text)


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
    assert win.panels[0].sheet_box.currentText() == "Summary"
    assert "QC3 CD34/Unit" in [win.panels[0].column_box.itemText(i) for i in range(win.panels[0].column_box.count())]
    assert win.panels[0].date_box.currentText() == "Date Processed"

    win.panels[0].column_box.setCurrentText("QC3 CD34/Unit")
    win.lsl_edit.setText("2e7")
    assert win.generate()
    assert win.canvas is not None
    assert "n=4" in win.stats_label.text() and "below LSL" in win.stats_label.text()
    assert win.save_png_btn.isEnabled()

    for fmt in ("png", "html", "xlsx"):
        out = win.save(fmt, "", path=str(tmp_path / f"chart_{fmt}"))
        assert out is not None and out.suffix == f".{fmt}" and out.stat().st_size > 0

    # Regenerating replaces the canvas without error.
    win.panels[0].date_box.setCurrentText(NO_DATE)
    assert win.generate()
    assert win.errors == []


def test_reload_sheet_keeps_column_when_present(win, xlsx_with_header_tab):
    win.open_path(xlsx_with_header_tab)
    win.panels[0].column_box.setCurrentText("QC3 CD34 Viability")
    assert win.load_sheet("Summary")
    assert win.panels[0].column_box.currentText() == "QC3 CD34 Viability"
    win.panels[0].sheet_box.setCurrentText("Query")
    assert win.panels[0].column_box.currentText() == "x"


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
    pick(win.panels[0].column_box, "QC3 CD34/Unit")
    # Copied to the second file even though that column is mostly junk there (so not offered by default).
    assert win.panels[1].column == "QC3 CD34/Unit"
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
    win.panels[0].column_box.setCurrentText("QC3 CD34/Unit")
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


def test_collapse_settings(win):
    win.show()
    assert win.settings_panel.isVisible()
    win.settings_toggle.click()
    assert not win.settings_panel.isVisible() and win.generate_btn.isVisible()
    win.set_settings_visible(True)
    assert win.settings_panel.isVisible() and win.settings_toggle.isChecked()


def test_workbooks_with_different_sheets(win, xlsx_with_header_tab, xlsx_inline_header, tmp_path):
    from imr_chart.rules import Rule

    assert win.open_paths([xlsx_with_header_tab, xlsx_inline_header])
    first, second = win.panels
    assert (first.sheet, second.sheet) == ("Summary", "Data")
    pick(first.column_box, "QC3 CD34/Unit")
    assert second.column == "QC3 CD34/Unit"
    pick(second.column_box, "QC3 CD34 Viability")  # the second file's choice is its own
    assert first.column == "QC3 CD34/Unit"

    ed = win.rules_editor
    assert set(ed.columns_by_sheet) == {"Summary", "Data"}
    ed.add_rule(Rule("data b", "Notes", "=", "b", sheet="Data"))
    assert ed.rules()[0].sheet == "Data"
    assert win.generate()
    assert win.job.columns == ["QC3 CD34/Unit", "QC3 CD34 Viability"]
    assert win.job.highlighted == 1  # only the Data sheet's L1 row, not Summary's
    assert "files=2" in win.stats_label.text() and "n=10" in win.stats_label.text()
    assert win.save("xlsx", "", path=str(tmp_path / "mixed")) is not None

    win.panels[1].date_box.setCurrentText(NO_DATE)
    assert win.generate() is False
    assert "date column for every workbook" in win.errors[-1]
    assert len(win.errors) == 1


def test_rule_sheet_limits_columns(win, xlsx_with_header_tab):
    from imr_chart.gui import ANY_SHEET, RULE_COLUMN, RULE_SHEET

    win.open_path(xlsx_with_header_tab)
    ed = win.rules_editor
    ed.add_rule({"column": "Notes", "op": "is blank"})
    sheet, column = ed.table.cellWidget(0, RULE_SHEET), ed.table.cellWidget(0, RULE_COLUMN)
    assert sheet.currentText() == ANY_SHEET
    sheet.setCurrentText("Summary")
    assert "Notes" in [column.itemText(i) for i in range(column.count())]
    assert column.currentText() == "Notes"
    win.panels[0].sheet_box.setCurrentText("Query")  # Summary no longer loaded: its rules keep their sheet
    assert sheet.currentText() == "Summary" and ed.columns == ["x", "y"]
    assert ed.rules()[0].sheet == "Summary"


def test_app_icon_is_packaged():
    from PySide6.QtGui import QIcon

    from imr_chart.gui import ICON

    assert ICON.exists() and ICON.with_suffix(".ico").exists()
    assert not QIcon(str(ICON)).isNull()
