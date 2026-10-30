"""Desktop GUI (Qt / PySide6) for generating and saving I-MR charts."""

from __future__ import annotations

import sys
from pathlib import Path

import matplotlib

matplotlib.use("QtAgg")
from matplotlib.backends.backend_qtagg import FigureCanvasQTAgg, NavigationToolbar2QT  # noqa: E402
from PySide6.QtCore import Qt  # noqa: E402
from PySide6.QtGui import QColor  # noqa: E402
from PySide6.QtWidgets import (  # noqa: E402
    QAbstractItemView,
    QApplication,
    QColorDialog,
    QComboBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QSpinBox,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

from . import __version__, loader, rules  # noqa: E402
from .plot_static import DEFAULT_RECENT_DAYS  # noqa: E402
from .report import ChartJob, build_multi_job  # noqa: E402
from .stats import parse_limit  # noqa: E402

NO_DATE = "(row order)"
FILE_FILTER = "Data files (*.xlsx *.xlsm *.xls *.csv *.tsv *.txt);;All files (*)"
RULES_FILTER = "Highlight rules (*.json);;All files (*)"
RULE_ON, RULE_NAME, RULE_COLUMN, RULE_OP, RULE_VALUE, RULE_COLOR = range(6)


class RulesEditor(QGroupBox):
    """Ordered table of highlight rules; the first rule a row matches colors its point."""

    def __init__(self, parent=None):
        super().__init__("Highlight rules (the first matching rule colors the point)", parent)
        self.columns: list[str] = []
        self.table = QTableWidget(0, 6)
        self.table.setHorizontalHeaderLabels(["On", "Name", "Column", "Operator", "Value", "Color"])
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.SingleSelection)
        self.table.verticalHeader().setVisible(False)
        header = self.table.horizontalHeader()
        for col in (RULE_ON, RULE_OP, RULE_COLOR):
            header.setSectionResizeMode(col, QHeaderView.ResizeToContents)
        for col in (RULE_NAME, RULE_COLUMN, RULE_VALUE):
            header.setSectionResizeMode(col, QHeaderView.Stretch)
        self.table.setMaximumHeight(150)

        self.add_btn = QPushButton("Add")
        self.remove_btn = QPushButton("Remove")
        self.up_btn = QPushButton("Up")
        self.down_btn = QPushButton("Down")
        self.load_btn = QPushButton("Load rules…")
        self.save_btn = QPushButton("Save rules…")
        buttons = QHBoxLayout()
        for b in (self.add_btn, self.remove_btn, self.up_btn, self.down_btn):
            buttons.addWidget(b)
        buttons.addStretch(1)
        buttons.addWidget(self.load_btn)
        buttons.addWidget(self.save_btn)

        layout = QVBoxLayout()
        layout.addWidget(self.table)
        layout.addLayout(buttons)
        self.setLayout(layout)

        self.add_btn.clicked.connect(lambda: self.add_rule())
        self.remove_btn.clicked.connect(self.remove_rule)
        self.up_btn.clicked.connect(lambda: self.move_rule(-1))
        self.down_btn.clicked.connect(lambda: self.move_rule(1))
        self.load_btn.clicked.connect(self.choose_load)
        self.save_btn.clicked.connect(self.choose_save)

    # -- rows ------------------------------------------------------------
    def add_rule(self, rule: rules.Rule | dict | None = None, row: int | None = None):
        """Insert a row from a Rule (or a dict of its fields, possibly incomplete)."""
        values = {"name": "", "column": "", "op": "=", "value": "", "color": rules.DEFAULT_COLOR, "enabled": True}
        values |= vars(rule) if isinstance(rule, rules.Rule) else (rule or {})
        row = self.table.rowCount() if row is None else row
        self.table.insertRow(row)
        on = QTableWidgetItem()
        on.setFlags(Qt.ItemIsUserCheckable | Qt.ItemIsEnabled | Qt.ItemIsSelectable)
        on.setCheckState(Qt.Checked if values["enabled"] else Qt.Unchecked)
        self.table.setItem(row, RULE_ON, on)
        self.table.setItem(row, RULE_NAME, QTableWidgetItem(values["name"]))
        column = QComboBox()
        column.setEditable(True)
        column.addItems(self.columns)
        column.setCurrentText(values["column"])
        self.table.setCellWidget(row, RULE_COLUMN, column)
        op = QComboBox()
        op.addItems(rules.OPERATORS)
        op.setCurrentText(values["op"])
        self.table.setCellWidget(row, RULE_OP, op)
        self.table.setItem(row, RULE_VALUE, QTableWidgetItem(values["value"]))
        color = QPushButton()
        color.clicked.connect(lambda: self._pick_color(color))
        self._set_color(color, values["color"])
        self.table.setCellWidget(row, RULE_COLOR, color)
        self.table.selectRow(row)

    def remove_rule(self):
        row = self.table.currentRow()
        if row >= 0:
            self.table.removeRow(row)

    def move_rule(self, step: int):
        row = self.table.currentRow()
        target = row + step
        if row < 0 or not 0 <= target < self.table.rowCount():
            return
        values = self._row_values(row)
        self.table.removeRow(row)
        self.add_rule(values, target)

    @staticmethod
    def _set_color(button: QPushButton, color: str):
        button.setProperty("color", color)
        button.setText(color)
        text = "black" if QColor(color).lightness() > 128 else "white"
        button.setStyleSheet(f"background-color: {color}; color: {text}")

    def _pick_color(self, button: QPushButton):
        color = QColorDialog.getColor(QColor(button.property("color")), self, "Rule color")
        if color.isValid():
            self._set_color(button, color.name())

    def _row_values(self, row: int) -> dict:
        return {
            "name": self.table.item(row, RULE_NAME).text().strip(),
            "column": self.table.cellWidget(row, RULE_COLUMN).currentText().strip(),
            "op": self.table.cellWidget(row, RULE_OP).currentText(),
            "value": self.table.item(row, RULE_VALUE).text(),
            "color": self.table.cellWidget(row, RULE_COLOR).property("color"),
            "enabled": self.table.item(row, RULE_ON).checkState() == Qt.Checked,
        }

    # -- public ----------------------------------------------------------
    def rules(self) -> list[rules.Rule]:
        """The table's rules, in order; raises ValueError naming the first incomplete row."""
        out = []
        for row in range(self.table.rowCount()):
            try:
                out.append(rules.Rule(**self._row_values(row)))
            except ValueError as exc:
                raise ValueError(f"Highlight rule {row + 1}: {exc}") from None
        return out

    def set_rules(self, items: list[rules.Rule]):
        self.table.setRowCount(0)
        for rule in items:
            self.add_rule(rule)

    def set_columns(self, columns: list[str]):
        """Offer ``columns`` in every row's column box, keeping what each row already names."""
        self.columns = [str(c) for c in columns]
        for row in range(self.table.rowCount()):
            box = self.table.cellWidget(row, RULE_COLUMN)
            current = box.currentText()
            box.clear()
            box.addItems(self.columns)
            box.setCurrentText(current)

    def choose_load(self):
        path, _ = QFileDialog.getOpenFileName(self, "Load highlight rules", "", RULES_FILTER)
        if path:
            try:
                self.set_rules(rules.load_rules(path))
            except (ValueError, OSError) as exc:
                QMessageBox.critical(self, "Could not load rules", str(exc))

    def choose_save(self):
        try:
            items = self.rules()
        except ValueError as exc:
            QMessageBox.critical(self, "Could not save rules", str(exc))
            return
        path, _ = QFileDialog.getSaveFileName(self, "Save highlight rules", "highlight_rules.json", RULES_FILTER)
        if path:
            try:
                rules.save_rules(items, path if path.lower().endswith(".json") else path + ".json")
            except OSError as exc:
                QMessageBox.critical(self, "Could not save rules", str(exc))


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(f"I-MR Chart Builder {__version__}")
        self.resize(1200, 900)
        self.paths: list[Path] = []
        self.tables: list[tuple[Path, object]] | None = None  # (file, DataFrame) per input file
        self.job: ChartJob | None = None

        self.file_label = QLabel("No file loaded")
        self.open_btn = QPushButton("Open file(s)…")
        self.sheet_box = QComboBox()
        self.column_box = QComboBox()
        self.date_box = QComboBox()
        self.lsl_edit = QLineEdit()
        self.lsl_edit.setPlaceholderText("e.g. 2e7 or 50%  (blank = none)")
        self.sigma_spin = QDoubleSpinBox()
        self.sigma_spin.setRange(0.5, 6.0)
        self.sigma_spin.setSingleStep(0.5)
        self.sigma_spin.setValue(3.0)
        self.recent_spin = QSpinBox()
        self.recent_spin.setRange(0, 3650)
        self.recent_spin.setValue(DEFAULT_RECENT_DAYS)
        self.recent_spin.setSuffix(" days")
        self.recent_spin.setSpecialValueText("Off")
        self.generate_btn = QPushButton("Generate chart")
        self.save_png_btn = QPushButton("Save PNG…")
        self.save_html_btn = QPushButton("Save HTML…")
        self.save_xlsx_btn = QPushButton("Save Excel stats…")
        self.rules_editor = RulesEditor()
        self.stats_label = QLabel("")
        self.stats_label.setTextInteractionFlags(Qt.TextSelectableByMouse)

        form = QFormLayout()
        file_row = QHBoxLayout()
        file_row.addWidget(self.open_btn)
        file_row.addWidget(self.file_label, 1)
        form.addRow("Files:", file_row)
        form.addRow("Sheet:", self.sheet_box)
        form.addRow("Value column:", self.column_box)
        form.addRow("Date column:", self.date_box)
        form.addRow("LSL:", self.lsl_edit)
        form.addRow("Sigma multiplier (k):", self.sigma_spin)
        form.addRow("Highlight recent:", self.recent_spin)

        buttons = QHBoxLayout()
        for b in (self.generate_btn, self.save_png_btn, self.save_html_btn, self.save_xlsx_btn):
            buttons.addWidget(b)
        buttons.addStretch(1)

        self.canvas_holder = QVBoxLayout()
        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.canvas = None
        self.toolbar = None

        root = QVBoxLayout()
        root.addLayout(form)
        root.addWidget(self.rules_editor)
        root.addLayout(buttons)
        root.addWidget(self.stats_label)
        root.addLayout(self.canvas_holder)
        root.addWidget(self.scroll, 1)
        central = QWidget()
        central.setLayout(root)
        self.setCentralWidget(central)

        self.open_btn.clicked.connect(self.choose_file)
        self.sheet_box.currentTextChanged.connect(self.load_sheet)
        self.generate_btn.clicked.connect(self.generate)
        self.lsl_edit.returnPressed.connect(self.generate)
        self.save_png_btn.clicked.connect(lambda: self.save("png", "PNG image (*.png)"))
        self.save_html_btn.clicked.connect(lambda: self.save("html", "Interactive HTML (*.html)"))
        self.save_xlsx_btn.clicked.connect(lambda: self.save("xlsx", "Excel workbook (*.xlsx)"))
        self._update_enabled()

    # -- state -----------------------------------------------------------
    def _update_enabled(self):
        loaded = self.tables is not None
        for w in (self.sheet_box, self.column_box, self.date_box, self.lsl_edit, self.sigma_spin, self.recent_spin,
                  self.generate_btn):
            w.setEnabled(loaded)
        for w in (self.save_png_btn, self.save_html_btn, self.save_xlsx_btn):
            w.setEnabled(self.job is not None)

    def error(self, title: str, exc: Exception | str):
        msg = exc.args[0] if isinstance(exc, KeyError) and exc.args else str(exc)
        QMessageBox.critical(self, title, msg)

    # -- actions ---------------------------------------------------------
    def choose_file(self):
        paths, _ = QFileDialog.getOpenFileNames(self, "Open data file(s) with the same layout", "", FILE_FILTER)
        if paths:
            self.open_paths(paths)

    def open_path(self, path: str | Path) -> bool:
        return self.open_paths([path])

    def open_paths(self, paths: list[str | Path]) -> bool:
        """Open one or more files with the same layout; their rows are charted together."""
        paths = [Path(p) for p in paths]
        try:
            sheets = loader.common_columns([loader.list_sheets(p) for p in paths if not loader.is_csv(p)])
            if all(loader.is_csv(p) for p in paths):
                sheets = [paths[0].stem]
            elif not sheets:
                raise ValueError("These workbooks have no sheet name in common.")
        except Exception as exc:  # noqa: BLE001 - surface any read error to the user
            self.error("Could not open file", exc)
            return False
        self.paths = paths
        if len(paths) == 1:
            self.file_label.setText(str(paths[0]))
        else:
            names = ", ".join(p.name for p in paths[:3]) + (", …" if len(paths) > 3 else "")
            self.file_label.setText(f"{len(paths)} files: {names}")
        self.file_label.setToolTip("\n".join(str(p) for p in paths))
        self.sheet_box.blockSignals(True)
        self.sheet_box.clear()
        self.sheet_box.addItems(sheets)
        self.sheet_box.blockSignals(False)
        return self.load_sheet(sheets[0] if sheets else None)

    def load_sheet(self, sheet: str | None) -> bool:
        if not self.paths or not sheet:
            return False
        tables = []
        for path in self.paths:
            try:
                tables.append((path, loader.load_table(path, None if loader.is_csv(path) else sheet)))
            except Exception as exc:  # noqa: BLE001
                self.tables = None
                self._update_enabled()
                self.error("Could not read sheet", f"{path.name}: {exc}" if len(self.paths) > 1 else exc)
                return False
        self.tables = tables
        frames = [df for _, df in tables]
        prev_col = self.column_box.currentText()
        self.column_box.clear()
        numeric = loader.pooled_numeric_columns(frames)
        self.column_box.addItems(numeric)
        if prev_col in numeric:
            self.column_box.setCurrentText(prev_col)
        self.date_box.clear()
        self.date_box.addItems(loader.common_columns([loader.date_columns(df) for df in frames]) + [NO_DATE])
        self.rules_editor.set_columns(loader.common_columns([list(df.columns) for df in frames]))
        self.job = None
        self._update_enabled()
        return True

    def generate(self) -> bool:
        if self.tables is None:
            return False
        column = self.column_box.currentText()
        if not column:
            self.error("No column", "This sheet has no numeric columns to chart.")
            return False
        date_col = self.date_box.currentText()
        date_col = None if date_col in ("", NO_DATE) else date_col
        try:
            lsl = parse_limit(self.lsl_edit.text())
            excel = not all(loader.is_csv(p) for p in self.paths)
            self.job = build_multi_job(self.tables, column, date_col, lsl=lsl, k=self.sigma_spin.value(),
                                       recent_days=self.recent_spin.value(), rules=self.rules_editor.rules(),
                                       sheet=self.sheet_box.currentText() if excel else None)
        except Exception as exc:  # noqa: BLE001
            self.job = None
            self._update_enabled()
            self.error("Could not build chart", exc)
            return False
        self._show_figure()
        r = self.job.result
        files = f"files={len(self.job.sources)}  " if len(self.job.sources) > 1 else ""
        dupes = self.job.duplicates
        self.stats_label.setText(
            f"{files}n={r.n}  excluded={self.job.excluded}  mean={r.mean:.4g}  σ={r.sigma:.4g}  "
            f"UCL={r.ucl:.4g}  LCL={r.lcl:.4g}  MR̄={r.mr_bar:.4g}  MR UCL={r.mr_ucl:.4g}  "
            f"beyond limits={int((r.above_ucl | r.below_lcl).sum())}"
            + (f"  below LSL={int(r.below_lsl.sum())}" if r.lsl is not None else "")
            + (f"  highlighted={self.job.highlighted}" if self.job.rules else "")
            + (f"  ⚠ {dupes} points repeated across files" if dupes else "")
        )
        self._update_enabled()
        return True

    def _show_figure(self):
        if self.toolbar is not None:
            self.canvas_holder.removeWidget(self.toolbar)
            self.toolbar.hide()
            self.toolbar.deleteLater()
        self.canvas = FigureCanvasQTAgg(self.job.figure())
        # Below this size the footnote and date labels collide; scroll instead.
        self.canvas.setMinimumSize(950, 760)
        self.toolbar = NavigationToolbar2QT(self.canvas, self)
        self.canvas_holder.addWidget(self.toolbar)
        self.scroll.setWidget(self.canvas)  # deletes the previous canvas

    def save(self, fmt: str, filt: str, path: str | None = None) -> Path | None:
        if self.job is None:
            return None
        if path is None:
            start = (self.paths[0].parent if self.paths else Path.cwd()) / f"{self.job.default_stem()}.{fmt}"
            path, _ = QFileDialog.getSaveFileName(self, f"Save {fmt.upper()}", str(start), filt)
            if not path:
                return None
        if not path.lower().endswith(f".{fmt}"):
            path += f".{fmt}"
        try:
            saved = {"png": self.job.save_png, "html": self.job.save_html, "xlsx": self.job.save_xlsx}[fmt](path)
        except Exception as exc:  # noqa: BLE001
            self.error("Could not save", exc)
            return None
        self.statusBar().showMessage(f"Saved {saved}", 8000)
        return saved


def main() -> int:
    app = QApplication.instance() or QApplication(sys.argv)
    win = MainWindow()
    win.show()
    paths = [p for p in sys.argv[1:] if Path(p).exists()]
    if paths:
        win.open_paths(paths)
    return app.exec()
