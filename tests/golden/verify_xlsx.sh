#!/usr/bin/env bash
# Write workbooks with the release CLI and check them with tests/golden/check_formulas.py.
# Usage: tests/golden/verify_xlsx.sh [PYTHON]   (PYTHON needs the `formulas` and `openpyxl` packages)
set -euo pipefail
cd "$(dirname "$0")/../.."
python=${1:-python}
cli=target/release/imr-chart-cli
out=target/verify
data=tests/golden/data
rm -rf "$out"
$cli $data/messy.csv -c CD34 --lsl 2e7 --formats xlsx -o $out/messy
$cli $data/part1.xlsx $data/part2.xlsx -c "QC3 CD34/Unit" --lsl 2e7 --formats xlsx -o $out/pooled
$cli $data/qc_summary.xlsx -c "QC3 CD34 Viability" --lsl 85% --rules tests/golden/rules/qc.json --formats xlsx -o $out/rules
$cli $data/dates.xlsx --sheet Runs -c Value --sigma 2.5 --formats xlsx -o $out/dates
$cli $data/noheader.xlsx -c "Column 2" --no-date --formats xlsx -o $out/rows
"$python" tests/golden/check_formulas.py $out/*/*.xlsx
