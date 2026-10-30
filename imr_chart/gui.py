"""Desktop GUI (Qt / PySide6) for generating and saving I-MR charts."""

from __future__ import annotations

import sys
from pathlib import Path

import matplotlib

matplotlib.use("QtAgg")
from matplotlib.backends.backend_qtagg import FigureCanvasQTAgg, NavigationToolbar2QT  # noqa: E402
from PySide6.QtCore import Qt  # noqa: E402
from PySide6.QtWidgets import (  # noqa: E402
    QApplication,
    QComboBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QVBoxLayout,
    QWidget,
)

from . import __version__, loader  # noqa: E402
from .report import ChartJob, build_job  # noqa: E402
from .stats import parse_limit  # noqa: E402

NO_DATE = "(row order)"
FILE_FILTER = "Data files (*.xlsx *.xlsm *.xls *.csv *.tsv *.txt);;All files (*)"


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(f"I-MR Chart Builder {__version__}")
        self.resize(1200, 900)
        self.path: Path | None = None
        self.df = None
        self.job: ChartJob | None = None

        self.file_label = QLabel("No file loaded")
        self.open_btn = QPushButton("Open file…")
        self.sheet_box = QComboBox()
        self.column_box = QComboBox()
        self.date_box = QComboBox()
        self.lsl_edit = QLineEdit()
        self.lsl_edit.setPlaceholderText("e.g. 2e7 or 50%  (blank = none)")
        self.sigma_spin = QDoubleSpinBox()
        self.sigma_spin.setRange(0.5, 6.0)
        self.sigma_spin.setSingleStep(0.5)
        self.sigma_spin.setValue(3.0)
        self.generate_btn = QPushButton("Generate chart")
        self.save_png_btn = QPushButton("Save PNG…")
        self.save_html_btn = QPushButton("Save HTML…")
        self.save_xlsx_btn = QPushButton("Save Excel stats…")
        self.stats_label = QLabel("")
        self.stats_label.setTextInteractionFlags(Qt.TextSelectableByMouse)

        form = QFormLayout()
        file_row = QHBoxLayout()
        file_row.addWidget(self.open_btn)
        file_row.addWidget(self.file_label, 1)
        form.addRow("File:", file_row)
        form.addRow("Sheet:", self.sheet_box)
        form.addRow("Value column:", self.column_box)
        form.addRow("Date column:", self.date_box)
        form.addRow("LSL:", self.lsl_edit)
        form.addRow("Sigma multiplier (k):", self.sigma_spin)

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
        loaded = self.df is not None
        for w in (self.sheet_box, self.column_box, self.date_box, self.lsl_edit, self.sigma_spin, self.generate_btn):
            w.setEnabled(loaded)
        for w in (self.save_png_btn, self.save_html_btn, self.save_xlsx_btn):
            w.setEnabled(self.job is not None)

    def error(self, title: str, exc: Exception | str):
        msg = exc.args[0] if isinstance(exc, KeyError) and exc.args else str(exc)
        QMessageBox.critical(self, title, msg)

    # -- actions ---------------------------------------------------------
    def choose_file(self):
        path, _ = QFileDialog.getOpenFileName(self, "Open data file", "", FILE_FILTER)
        if path:
            self.open_path(path)

    def open_path(self, path: str | Path) -> bool:
        try:
            sheets = loader.list_sheets(path)
        except Exception as exc:  # noqa: BLE001 - surface any read error to the user
            self.error("Could not open file", exc)
            return False
        self.path = Path(path)
        self.file_label.setText(str(self.path))
        self.sheet_box.blockSignals(True)
        self.sheet_box.clear()
        self.sheet_box.addItems(sheets)
        self.sheet_box.blockSignals(False)
        return self.load_sheet(sheets[0] if sheets else None)

    def load_sheet(self, sheet: str | None) -> bool:
        if self.path is None or not sheet:
            return False
        try:
            self.df = loader.load_table(self.path, None if loader.is_csv(self.path) else sheet)
        except Exception as exc:  # noqa: BLE001
            self.df = None
            self._update_enabled()
            self.error("Could not read sheet", exc)
            return False
        prev_col = self.column_box.currentText()
        self.column_box.clear()
        numeric = loader.numeric_columns(self.df)
        self.column_box.addItems(numeric)
        if prev_col in numeric:
            self.column_box.setCurrentText(prev_col)
        self.date_box.clear()
        self.date_box.addItems(loader.date_columns(self.df) + [NO_DATE])
        self.job = None
        self._update_enabled()
        return True

    def generate(self) -> bool:
        if self.df is None:
            return False
        column = self.column_box.currentText()
        if not column:
            self.error("No column", "This sheet has no numeric columns to chart.")
            return False
        date_col = self.date_box.currentText()
        date_col = None if date_col in ("", NO_DATE) else date_col
        try:
            lsl = parse_limit(self.lsl_edit.text())
            self.job = build_job(self.df, self.path, column, date_col, lsl=lsl, k=self.sigma_spin.value())
        except Exception as exc:  # noqa: BLE001
            self.job = None
            self._update_enabled()
            self.error("Could not build chart", exc)
            return False
        self._show_figure()
        r = self.job.result
        self.stats_label.setText(
            f"n={r.n}  excluded={self.job.excluded}  mean={r.mean:.4g}  σ={r.sigma:.4g}  "
            f"UCL={r.ucl:.4g}  LCL={r.lcl:.4g}  MR̄={r.mr_bar:.4g}  MR UCL={r.mr_ucl:.4g}  "
            f"beyond limits={int((r.above_ucl | r.below_lcl).sum())}"
            + (f"  below LSL={int(r.below_lsl.sum())}" if r.lsl is not None else "")
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
            start = (self.path.parent if self.path else Path.cwd()) / f"{self.job.default_stem()}.{fmt}"
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
    if len(sys.argv) > 1 and Path(sys.argv[1]).exists():
        win.open_path(sys.argv[1])
    return app.exec()
