# PyInstaller spec: builds two single-file executables from one analysis.
#   imr-chart      - windowed GUI (double-click; drop a file onto it to open it)
#   imr-chart-cli  - console build for scripting / batch use
# Build with:  pyinstaller --noconfirm imr_chart.spec

from PyInstaller.utils.hooks import collect_data_files

datas = collect_data_files("plotly", includes=["package_data/**/*", "validators/**/*"])

# Qt modules the app never uses; excluding them keeps the executable smaller.
excludes = [
    "tkinter", "PySide6.QtWebEngineCore", "PySide6.QtWebEngineWidgets", "PySide6.Qt3DCore",
    "PySide6.QtQuick", "PySide6.QtQml", "PySide6.QtMultimedia", "PySide6.QtPdf", "PySide6.QtCharts",
    "PySide6.QtDataVisualization", "PySide6.QtBluetooth", "PySide6.QtNetworkAuth", "PySide6.QtSql",
    "PySide6.QtTest", "PyQt5", "PyQt6", "IPython", "pytest",
    # Test-only (the "formulas" Excel evaluator and what it pulls in); pandas would otherwise bundle scipy.
    "formulas", "schedula", "scipy",
]


def make_analysis(script, gui):
    hidden = ["openpyxl", "matplotlib.backends.backend_agg"]
    if gui:
        hidden.append("matplotlib.backends.backend_qtagg")
    return Analysis(
        [script],
        pathex=["."],
        datas=datas,
        hiddenimports=hidden,
        # The console build never imports Qt; leaving it out saves ~100 MB.
        excludes=excludes if gui else excludes + ["PySide6", "shiboken6"],
        noarchive=False,
    )


def make_exe(a, name, console):
    pyz = PYZ(a.pure)
    return EXE(
        pyz, a.scripts, a.binaries, a.datas, [],
        name=name,
        console=console,
        upx=False,
        strip=False,
        debug=False,
    )


gui = make_analysis("scripts/run_gui.py", gui=True)
make_exe(gui, "imr-chart", console=False)

cli = make_analysis("scripts/run_cli.py", gui=False)
make_exe(cli, "imr-chart-cli", console=True)
