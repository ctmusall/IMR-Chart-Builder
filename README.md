# I-MR Chart Builder

Generates **Individual & Moving Range (I-MR) control charts** from Excel or CSV QC exports, in the same style as JMP's Control Chart Builder:

- black points, green center line (mean), red control limits
- a blue **LSL** line (for example `LSL (2e7)`)
- a moving-range chart underneath, plotted against Date Processed
- the **most recent 30 days** (counted back from the latest date in the file) shaded and drawn in orange, so new results stand out. Control limits still use all the data.

**Several files at once.** Give it one file or many exports with the same layout (same sheet name and columns), and every row from all of them goes on one chart. Points are sorted by date across files; same-date ties keep the order the files were given. Control limits use all the pooled points. In the HTML, hovering a point shows which file it came from. If the same lot, date and value turns up in more than one file (overlapping exports), you get a warning. Those points are still charted.

Outputs: **PNG** image, **interactive HTML** (hover a point to see its lot number, date, and value), and an **Excel stats** workbook. The workbook proves that the chart's numbers match the input (see below).

## Download

Download the latest build from the [Releases page](https://github.com/ctmusall/IMR-Chart-Builder/releases/latest). No GitHub account is needed.

- **Windows:** [imr-chart.exe](https://github.com/ctmusall/IMR-Chart-Builder/releases/latest/download/imr-chart.exe). Double-click to run. Windows may show a SmartScreen warning because the exe isn't code-signed: click *More info* → *Run anyway*.
- **Linux:** [imr-chart-linux](https://github.com/ctmusall/IMR-Chart-Builder/releases/latest/download/imr-chart-linux). Run `chmod +x imr-chart-linux` once, then run it.

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
| **Summary stats** | Which input was charted. Includes each source file's **SHA-256 fingerprint**, path, sheet, columns, settings, program version and timestamp. Re-hash the file (`certutil -hashfile FILE SHA256` on Windows, `sha256sum FILE` on Linux) to confirm the input is the same file. |
| **Verification** | Excel recomputes every number with its own formulas (n, mean, MR̄, σ, all limits, every flag count) and compares each with the program's value. Every check shows PASS or FAIL, and the top cell shows **ALL CHECKS PASS** only when every check agrees. If you open the file from email and the cells are blank, click *Enable Editing* so Excel can calculate. |
| **Recompute** | The per-point formulas behind Verification: each value looked up in its source file and row, each moving range, and each flag. |
| **Worked formulas** | Each calculation written out with this data's numbers, e.g. `UCL = 6.01104e+07 + 3 × 2.97635e+07`. Includes the source of the constants d₂ = 1.128 and d₃ = 0.8525. |
| **Input audit** | Every row of each source sheet: its file and row number in the original file, raw and parsed date and value, and whether it was charted. If it was excluded, the reason is given (blank or non-numeric value, blank or unreadable date). |
| **Data** | Each charted point, with its **Source row** and **Source file**, so you can find it in the original file. |

The test suite checks that the Verification sheet evaluates to ALL CHECKS PASS. It also checks that changing a single value makes it fail.

## Input files

- `.xlsx` / `.xlsm` / `.xls` / `.csv` / `.tsv`
- **Headers.** If the workbook has a `Column Headers (<Sheet>)` tab, it supplies the column names for `<Sheet>` (this is how `Summary` is laid out). Otherwise the first row is used as the header.
- **Defaults.** The `Summary` sheet, the `Date Processed` column for the x-axis, and `QC Lot` to label points.
- Any numeric column can be charted. LSL accepts `2e7`, `20000000`, or `50%`.

## Using the app

**GUI:**
1. Run `imr-chart.exe` (Windows) or `./imr-chart` (Linux). You can also drag one or more files onto the exe.
2. Open a file, or select several files with the same layout (Ctrl/Shift-click in the Open dialog), and choose the sheet, column, and LSL. Columns and sheets that all the files share are listed.
3. Click **Generate chart**, then **Save PNG… / Save HTML… / Save Excel stats…**.

**Command line** (`imr-chart-cli`):
```
imr-chart-cli sheet.xlsx --list-columns
imr-chart-cli sheet.xlsx -c "QC3 CD34/Unit" --lsl 2e7 -o charts/
imr-chart-cli sheet.xlsx -c "QC3 CD34/Unit" --lsl 2e7 -c "QC3 CD34 Viability" --lsl 50% --formats png,html
imr-chart-cli data.csv -c Value --sigma 2 --date-column "Run Date"
imr-chart-cli jan.xlsx feb.xlsx mar.xlsx -c "QC3 CD34/Unit" --lsl 2e7   # one chart from all three files
```
| Option | Effect |
|---|---|
| `--sheet` | Choose a worksheet (the same name in every file) |
| `--no-date` | Plot in row order instead of by date |
| `--label-column` | Choose the hover/lot column |
| `--recent-days N` | Highlight the last N days (default 30; `0` turns it off). In the GUI, use **Highlight recent** |
| `--rules FILE` | Color points by highlight rules from a JSON file (see below) |

### Highlight rules

A highlight rule colors every point whose source row matches a condition. A rule can test any column of the row, not only the charted one, e.g. `Notes contains retest` or `QC3 CD34 Viability < 90%`. Rules are checked top to bottom and the **first matching rule** decides a point's color. Points beyond the control limits keep their red ring. The chart legend shows how many points each rule matched, and the Excel **Data** sheet has a **Highlight rule** column.

| Operator | Matches when the cell… |
|---|---|
| `=` `<>` `<` `<=` `>` `>=` | compares with a number (`2e7`, `1,000`, `50%`) or a date (`2025-01-01`, `04/01/2024`). `=` and `<>` also compare text (ignoring case) |
| `contains`, `not contains`, `starts with`, `ends with` | holds the text, ignoring case |
| `is blank`, `is not blank` | is empty, or not |

In the GUI, edit the **Highlight rules** table (Add / Remove / Up / Down, click a color to change it) and use **Save rules…** to write a file that `--rules` can read:
```json
{"rules": [
  {"name": "Retest", "column": "Notes", "op": "contains", "value": "retest", "color": "#ff00ff"},
  {"name": "", "column": "QC3 CD34 Viability", "op": "<", "value": "90%", "color": "#2ca02c", "enabled": true}
]}
```
A blank `name` shows the condition itself in the legend.

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
| **GitHub Actions** | Push to `main`. `.github/workflows/build.yml` tests and builds both platforms, then publishes them as a public release named after the version in `imr_chart/__init__.py` | The [Releases page](https://github.com/ctmusall/IMR-Chart-Builder/releases/latest) |

Both build scripts run the full test suite before building. `sheet.xlsx` holds real QC data, so keep it out of public repositories. The tests that use it skip automatically when it's absent.
