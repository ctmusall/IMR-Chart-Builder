# I-MR Chart Builder

Generates **Individual & Moving Range (I-MR) control charts** from Excel or CSV QC exports, in the same style as JMP's Control Chart Builder:

- black points, green center line (mean), red control limits
- a blue **LSL** line (for example `LSL (2e7)`)
- a moving-range chart underneath, plotted against Date Processed

Outputs: **PNG** image, **interactive HTML** (hover a point to see its lot number, date, and value), and an **Excel stats** workbook.

## Calculations (same as JMP's defaults)

| | Formula |
|---|---|
| σ (sigma) | MR̄ / 1.128 (average moving range ÷ d₂) |
| Individuals center / UCL / LCL | mean, mean + kσ, mean − kσ |
| Moving range center / UCL / LCL | MR̄, MR̄ + k·0.8525σ (= 3.267·MR̄ at k = 3), 0 |

`k` (the sigma multiplier) defaults to 3 and can be changed. A negative LCL is not drawn when the data can't go below zero, matching the JMP view of CD34/Unit. Rows with a blank or non-numeric value are excluded, and the chart footnote shows how many.

## Input files

- `.xlsx` / `.xlsm` / `.xls` / `.csv` / `.tsv`
- **Headers.** If the workbook has a `Column Headers (<Sheet>)` tab, it supplies the column names for `<Sheet>` (this is how `Summary` is laid out). Otherwise the first row is used as the header.
- **Defaults.** The `Summary` sheet, the `Date Processed` column for the x-axis, and `QC Lot` to label points.
- Any numeric column can be charted. LSL accepts `2e7`, `20000000`, or `50%`.

## Using the app

**GUI:**
1. Run `imr-chart.exe` (Windows) or `./imr-chart` (Linux). You can also drag a file onto the exe.
2. Open a file and choose the sheet, column, and LSL.
3. Click **Generate chart**, then **Save PNG… / Save HTML… / Save Excel stats…**.

**Command line** (`imr-chart-cli`):
```
imr-chart-cli sheet.xlsx --list-columns
imr-chart-cli sheet.xlsx -c "QC3 CD34/Unit" --lsl 2e7 -o charts/
imr-chart-cli sheet.xlsx -c "QC3 CD34/Unit" --lsl 2e7 -c "QC3 CD34 Viability" --lsl 50% --formats png,html
imr-chart-cli data.csv -c Value --sigma 2 --date-column "Run Date"
```
| Option | Effect |
|---|---|
| `--sheet` | Choose a worksheet |
| `--no-date` | Plot in row order instead of by date |
| `--label-column` | Choose the hover/lot column |

## Development

```
python3 -m venv .venv && .venv/bin/pip install -e '.[dev]'
QT_QPA_PLATFORM=offscreen .venv/bin/pytest --cov=imr_chart
.venv/bin/python -m imr_chart          # GUI
.venv/bin/python -m imr_chart FILE ... # CLI
```

## Building executables

PyInstaller cannot cross-compile, so build each OS's executable on that OS.

| Platform | How to build | Output |
|---|---|---|
| **Linux** | `./build.sh` | `dist/imr-chart` (GUI) and `dist/imr-chart-cli` |
| **Windows** | Install Python 3.10+ from python.org, then run `powershell -ExecutionPolicy Bypass -File build.ps1` | `dist\imr-chart.exe` and `dist\imr-chart-cli.exe` |
| **GitHub Actions** | Push to GitHub; `.github/workflows/build.yml` tests and builds both platforms | Download the exes from the run's artifacts |

Both build scripts run the full test suite before building. `sheet.xlsx` holds real QC data, so keep it out of public repositories. The tests that use it skip automatically when it's absent.
