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
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from . import __version__, loader, rules  # noqa: E402
from .plot_static import DEFAULT_RECENT_DAYS  # noqa: E402
from .report import ChartJob, SourceTable, build_sources_job  # noqa: E402
from .stats import parse_limit  # noqa: E402

NO_DATE = "(row order)"
FILE_FILTER = "Data files (*.xlsx *.xlsm *.xls *.csv *.tsv *.txt);;All files (*)"
RULES_FILTER = "Highlight rules (*.json);;All files (*)"
RULE_ON, RULE_NAME, RULE_SHEET, RULE_COLUMN, RULE_OP, RULE_VALUE, RULE_COLOR = range(7)
ANY_SHEET = "(any sheet)"
VISIBLE_SOURCES = 3  # workbook sections shown before the list scrolls


class RulesEditor(QGroupBox):
    """Ordered table of highlight rules; the first rule a row matches colors its point."""

    def __init__(self, parent=None):
        super().__init__("Highlight rules (the first matching rule colors the point)", parent)
        self.columns: list[str] = []  # every loaded sheet's columns
        self.columns_by_sheet: dict[str, list[str]] = {}
        self.table = QTableWidget(0, 7)
        self.table.setHorizontalHeaderLabels(["On", "Name", "Sheet", "Column", "Operator", "Value", "Color"])
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.SingleSelection)
        self.table.verticalHeader().setVisible(False)
        header = self.table.horizontalHeader()
        for col in (RULE_ON, RULE_OP, RULE_COLOR):
            header.setSectionResizeMode(col, QHeaderView.ResizeToContents)
        for col in (RULE_NAME, RULE_SHEET, RULE_COLUMN, RULE_VALUE):
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
        values = {"name": "", "sheet": "", "column": "", "op": "=", "value": "", "color": rules.DEFAULT_COLOR,
                  "enabled": True}
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
        column.setCurrentText(values["column"])
        sheet = QComboBox()
        sheet.setEditable(True)  # a rules file may name a sheet that isn't loaded
        self._fill_sheets(sheet, values["sheet"] or ANY_SHEET)
        self._fill_columns(column, sheet.currentText())
        sheet.currentTextChanged.connect(lambda text: self._fill_columns(column, text))
        self.table.setCellWidget(row, RULE_SHEET, sheet)
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

    def _fill_sheets(self, box: QComboBox, current: str):
        box.blockSignals(True)
        box.clear()
        box.addItems([ANY_SHEET, *self.columns_by_sheet])
        box.setCurrentText(current)
        box.blockSignals(False)

    def columns_for(self, sheet: str) -> list[str]:
        """Columns offered for a rule on ``sheet``: that sheet's, or every sheet's for any sheet."""
        if sheet.strip() in ("", ANY_SHEET):
            return self.columns
        return self.columns_by_sheet.get(sheet.strip(), [])

    def _fill_columns(self, box: QComboBox, sheet: str):
        current = box.currentText()
        box.clear()
        box.addItems(self.columns_for(sheet))
        box.setCurrentText(current)

    def _row_values(self, row: int) -> dict:
        sheet = self.table.cellWidget(row, RULE_SHEET).currentText().strip()
        return {
            "name": self.table.item(row, RULE_NAME).text().strip(),
            "sheet": "" if sheet == ANY_SHEET else sheet,
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

    def set_sheets(self, columns_by_sheet: dict[str, list[str]]):
        """Offer these sheets, and each one's columns, in every row, keeping what each row already names."""
        self.columns_by_sheet = {str(k): [str(c) for c in v] for k, v in columns_by_sheet.items()}
        self.columns = list(dict.fromkeys(c for cols in self.columns_by_sheet.values() for c in cols))
        for row in range(self.table.rowCount()):
            sheet = self.table.cellWidget(row, RULE_SHEET)
            self._fill_sheets(sheet, sheet.currentText())
            self._fill_columns(self.table.cellWidget(row, RULE_COLUMN), sheet.currentText())

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


class SourcePanel(QGroupBox):
    """One input file's section: which sheet, value column and date column to chart from it."""

    def __init__(self, path: Path, parent=None):
        super().__init__(path.name, parent)
        self.setToolTip(str(path))
        self.path = path
        self.df = None
        self.sheet_box = QComboBox()
        self.column_box = QComboBox()
        self.date_box = QComboBox()
        self.sheet_box.addItems(loader.list_sheets(path))
        if not self.sheet_box.count():
            raise ValueError("This workbook has no data sheets.")
        self.sheet_box.setEnabled(not loader.is_csv(path))
        row = QHBoxLayout(self)
        for label, box, stretch in (("Sheet:", self.sheet_box, 1), ("Value column:", self.column_box, 2),
                                    ("Date column:", self.date_box, 1)):
            row.addWidget(QLabel(label))
            row.addWidget(box, stretch)

    @property
    def sheet(self) -> str:
        return self.sheet_box.currentText()

    @property
    def column(self) -> str:
        return self.column_box.currentText()

    @property
    def date_column(self) -> str | None:
        date_col = self.date_box.currentText()
        return None if date_col in ("", NO_DATE) else date_col

    def load_sheet(self, sheet: str | None = None):
        """Read ``sheet`` (default: the selected one) and offer its columns, keeping the choices still there."""
        sheet = sheet or self.sheet
        self.df = None
        df = loader.load_table(self.path, None if loader.is_csv(self.path) else sheet)
        if self.sheet != sheet:
            self.sheet_box.blockSignals(True)
            self.sheet_box.setCurrentText(sheet)
            self.sheet_box.blockSignals(False)
        prev_col, prev_date = self.column, self.date_box.currentText()
        self.df = df
        self.column_box.clear()
        self.column_box.addItems(loader.numeric_columns(df))
        self.date_box.clear()
        self.date_box.addItems(loader.date_columns(df) + [NO_DATE])
        for box, prev in ((self.column_box, prev_col), (self.date_box, prev_date)):
            if prev and box.findText(prev) >= 0:
                box.setCurrentText(prev)

    def _select(self, box: QComboBox, name: str) -> bool:
        """Choose ``name`` if this sheet has it, adding it to the list when it wasn't offered
        (e.g. a column that is numeric in another file but mostly junk in this one)."""
        if self.df is None or not name:
            return False
        if box.findText(name) < 0:
            if name not in self.df.columns:
                return False
            box.insertItem(box.count() - (box is self.date_box), name)  # dates: keep (row order) last
        box.setCurrentText(name)
        return True

    def select_column(self, name: str) -> bool:
        return self._select(self.column_box, name)

    def select_date(self, name: str) -> bool:
        if name == NO_DATE:
            self.date_box.setCurrentText(NO_DATE)
            return True
        return self._select(self.date_box, name)

    def source_table(self) -> SourceTable:
        return SourceTable(self.path, self.df, self.column, self.date_column, self.sheet)


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(f"I-MR Chart Builder {__version__}")
        self.resize(1200, 900)
        self.paths: list[Path] = []
        self.panels: list[SourcePanel] = []  # one section per input file
        self.job: ChartJob | None = None

        self.file_label = QLabel("No file loaded")
        self.open_btn = QPushButton("Open file(s)…")
        self.sources_layout = QVBoxLayout()
        self.sources_layout.setContentsMargins(0, 0, 0, 0)
        sources_widget = QWidget()
        sources_widget.setLayout(self.sources_layout)
        self.sources_scroll = QScrollArea()
        self.sources_scroll.setWidgetResizable(True)
        self.sources_scroll.setWidget(sources_widget)
        self.sources_scroll.setFrameShape(QScrollArea.NoFrame)
        self.sources_scroll.hide()
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
        form.addRow(self.sources_scroll)
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

        # Everything above the action buttons can be folded away to give the chart the room.
        self.settings_panel = QWidget()
        settings = QVBoxLayout(self.settings_panel)
        settings.setContentsMargins(0, 0, 0, 0)
        settings.addLayout(form)
        settings.addWidget(self.rules_editor)
        self.settings_toggle = QToolButton()
        self.settings_toggle.setText("Data && rules")
        self.settings_toggle.setToolButtonStyle(Qt.ToolButtonTextBesideIcon)
        self.settings_toggle.setCheckable(True)
        self.settings_toggle.setChecked(True)
        self.settings_toggle.setArrowType(Qt.DownArrow)
        self.settings_toggle.setAutoRaise(True)
        self.settings_toggle.toggled.connect(self.set_settings_visible)

        root = QVBoxLayout()
        root.addWidget(self.settings_toggle)
        root.addWidget(self.settings_panel)
        root.addLayout(buttons)
        root.addWidget(self.stats_label)
        root.addLayout(self.canvas_holder)
        root.addWidget(self.scroll, 1)
        central = QWidget()
        central.setLayout(root)
        self.setCentralWidget(central)

        self.open_btn.clicked.connect(self.choose_file)
        self.generate_btn.clicked.connect(self.generate)
        self.lsl_edit.returnPressed.connect(self.generate)
        self.save_png_btn.clicked.connect(lambda: self.save("png", "PNG image (*.png)"))
        self.save_html_btn.clicked.connect(lambda: self.save("html", "Interactive HTML (*.html)"))
        self.save_xlsx_btn.clicked.connect(lambda: self.save("xlsx", "Excel workbook (*.xlsx)"))
        self._update_enabled()

    # -- state -----------------------------------------------------------
    def set_settings_visible(self, visible: bool):
        """Show or fold away the file/column/limit settings and the highlight rules."""
        self.settings_toggle.blockSignals(True)
        self.settings_toggle.setChecked(visible)
        self.settings_toggle.blockSignals(False)
        self.settings_toggle.setArrowType(Qt.DownArrow if visible else Qt.RightArrow)
        self.settings_panel.setVisible(visible)

    def _update_enabled(self):
        loaded = bool(self.panels)
        for w in (self.lsl_edit, self.sigma_spin, self.recent_spin, self.generate_btn):
            w.setEnabled(loaded)
        for w in (self.save_png_btn, self.save_html_btn, self.save_xlsx_btn):
            w.setEnabled(self.job is not None)

    def error(self, title: str, exc: Exception | str):
        msg = exc.args[0] if isinstance(exc, KeyError) and exc.args else str(exc)
        QMessageBox.critical(self, title, msg)

    # -- actions ---------------------------------------------------------
    def choose_file(self):
        paths, _ = QFileDialog.getOpenFileNames(self, "Open data file(s)", "", FILE_FILTER)
        if paths:
            self.open_paths(paths)

    def open_path(self, path: str | Path) -> bool:
        return self.open_paths([path])

    def open_paths(self, paths: list[str | Path]) -> bool:
        """Open one or more files; each gets its own sheet/column choices and their rows are charted together."""
        paths = [Path(p) for p in paths]
        panels = []
        for path in paths:
            try:
                panel = SourcePanel(path)
                panel.load_sheet()
            except Exception as exc:  # noqa: BLE001 - surface any read error to the user
                self.error("Could not open file", f"{path.name}: {exc}" if len(paths) > 1 else exc)
                return False
            panels.append(panel)
        for old in self.panels:
            self.sources_layout.removeWidget(old)
            old.deleteLater()
        self.paths, self.panels = paths, panels
        if len(paths) == 1:
            self.file_label.setText(str(paths[0]))
        else:
            names = ", ".join(p.name for p in paths[:3]) + (", …" if len(paths) > 3 else "")
            self.file_label.setText(f"{len(paths)} files: {names}")
        self.file_label.setToolTip("\n".join(str(p) for p in paths))
        for panel in panels:
            self.sources_layout.addWidget(panel)
            panel.sheet_box.currentTextChanged.connect(lambda sheet, p=panel: self.load_sheet(sheet, p))
            panel.column_box.currentTextChanged.connect(self._invalidate)
            panel.date_box.currentTextChanged.connect(self._invalidate)
        # Choosing a column in the first workbook picks it in the others too, where they have it.
        first = panels[0]
        first.column_box.textActivated.connect(lambda name: self.copy_choice("column", name))
        first.date_box.textActivated.connect(lambda name: self.copy_choice("date", name))
        self.copy_choice("column", first.column)
        self.copy_choice("date", first.date_box.currentText())
        self._fit_sources()
        self.sources_scroll.show()
        self._refresh_rule_sheets()
        self._invalidate()
        return True

    def _fit_sources(self):
        """Show up to VISIBLE_SOURCES workbook sections; scroll for the rest."""
        shown = self.panels[:VISIBLE_SOURCES]
        spacing = self.sources_layout.spacing()
        height = sum(p.sizeHint().height() for p in shown) + spacing * max(len(shown) - 1, 0)
        self.sources_scroll.setFixedHeight(height + 2)

    def copy_choice(self, kind: str, name: str):
        """Select value (``kind="column"``) or date column ``name`` in every workbook after the first that has it."""
        for panel in self.panels[1:]:
            (panel.select_column if kind == "column" else panel.select_date)(name)

    def _invalidate(self, *_):
        self.job = None
        self._update_enabled()

    def _refresh_rule_sheets(self):
        columns: dict[str, list[str]] = {}
        for panel in self.panels:
            if panel.df is not None:
                columns[panel.sheet] = list(dict.fromkeys([*columns.get(panel.sheet, []), *map(str, panel.df.columns)]))
        self.rules_editor.set_sheets(columns)

    def load_sheet(self, sheet: str | None, panel: SourcePanel | None = None) -> bool:
        """Read ``sheet`` of ``panel``'s workbook (default: the first workbook)."""
        panel = panel or (self.panels[0] if self.panels else None)
        if panel is None or not sheet:
            return False
        try:
            panel.load_sheet(sheet)
            ok = True
        except Exception as exc:  # noqa: BLE001
            self.error("Could not read sheet", f"{panel.path.name}: {exc}" if len(self.panels) > 1 else exc)
            ok = False
        self._refresh_rule_sheets()
        self._invalidate()
        return ok

    def generate(self) -> bool:
        if not self.panels:
            return False
        many = len(self.panels) > 1
        for panel in self.panels:
            where = f"{panel.path.name}: " if many else ""
            if panel.df is None:
                self.error("Sheet not loaded", f"{where}Could not read sheet '{panel.sheet}'.")
                return False
            if not panel.column:
                self.error("No column", f"{where}This sheet has no numeric columns to chart.")
                return False
        try:
            lsl = parse_limit(self.lsl_edit.text())
            self.job = build_sources_job([p.source_table() for p in self.panels], lsl=lsl, k=self.sigma_spin.value(),
                                         recent_days=self.recent_spin.value(), rules=self.rules_editor.rules())
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
