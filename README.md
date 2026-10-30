# I-MR Chart Builder

Generates **Individual & Moving Range (I-MR) control charts** from Excel or CSV QC exports, in the same style as JMP's Control Chart Builder:

- black points, green center line (mean), red control limits
- a blue **LSL** line (for example `LSL (2e7)`)
- a moving-range chart underneath, plotted against Date Processed
- the **most recent 30 days** (counted back from the latest date in the file) shaded and drawn in orange, so new results stand out. Control limits still use all the data.

Outputs: **PNG** image, **interactive HTML** (hover a point to see its lot number, date, and value), and an **Excel stats** workbook. The workbook proves that the chart's numbers match the input (see below).

## Download

Download the latest build from the [Releases page](https://github.com/ctmusall/kelseys_tool/releases/latest). No GitHub account is needed.

- **Windows:** [imr-chart.exe](https://github.com/ctmusall/kelseys_tool/releases/latest/download/imr-chart.exe). Double-click to run. Windows may show a SmartScreen warning because the exe isn't code-signed: click *More info* → *Run anyway*.
- **Linux:** [imr-chart-linux](https://github.com/ctmusall/kelseys_tool/releases/latest/download/imr-chart-linux). Run `chmod +x imr-chart-linux` once, then run it.

Each push to `main` that passes the tests publishes a new release automatically.

## Calculations (same as JMP's defaults)

| | Formula |
|---|---|
| σ (sigma) | MR̄ / 1.128 (average moving range ÷ d₂) |
| Individuals center / UCL / LCL | mean, mean + kσ, mean − kσ |
| Moving range center / UCL / LCL | MR̄, MR̄ + k·0.8525σ (= 3.267·MR̄ at k = 3), 0 |

`k` (the sigma multiplier) defaults to 3 and can be changed. A negative LCL is not drawn when the data can't go below zero, matching the JMP view of CD34/Unit. Rows with a blank or non-numeric value are excluded, and the chart footnote shows how many.

## Proof that the chart matches the input

The Excel stats workbook (`.xlsx`) carries the evidence alongside the results:

| Sheet | What it proves |
|---|---|
| **Summary stats** | Which input was charted. Includes the source file's **SHA-256 fingerprint**, path, sheet, columns, settings, program version and timestamp. Re-hash the file (`certutil -hashfile FILE SHA256` on Windows, `sha256sum FILE` on Linux) to confirm the input is the same file. |
| **Verification** | Excel recomputes every number with its own formulas (n, mean, MR̄, σ, all limits, every flag count) and compares each with the program's value. Every check shows PASS or FAIL, and the top cell shows **ALL CHECKS PASS** only when every check agrees. If you open the file from email and the cells are blank, click *Enable Editing* so Excel can calculate. |
| **Recompute** | The per-point formulas behind Verification: each value looked up in its source row, each moving range, and each flag. |
| **Worked formulas** | Each calculation written out with this data's numbers, e.g. `UCL = 6.01104e+07 + 3 × 2.97635e+07`. Includes the source of the constants d₂ = 1.128 and d₃ = 0.8525. |
| **Input audit** | Every row of the source sheet: its row number in the original file, raw and parsed date and value, and whether it was charted. If it was excluded, the reason is given (blank or non-numeric value, blank or unreadable date). |
| **Data** | Each charted point, with its **Source row**, so you can find it in the original file. |

The test suite checks that the Verification sheet evaluates to ALL CHECKS PASS. It also checks that changing a single value makes it fail.

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
| `--recent-days N` | Highlight the last N days (default 30; `0` turns it off). In the GUI, use **Highlight recent** |

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
| **GitHub Actions** | Push to `main`. `.github/workflows/build.yml` tests and builds both platforms, then publishes them as a public release named after the version in `imr_chart/__init__.py` | The [Releases page](https://github.com/ctmusall/kelseys_tool/releases/latest) |

Both build scripts run the full test suite before building. `sheet.xlsx` holds real QC data, so keep it out of public repositories. The tests that use it skip automatically when it's absent.
