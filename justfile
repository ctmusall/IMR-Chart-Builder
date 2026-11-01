# Developer tasks. Install `just` (https://just.systems) or read the recipes and run the commands.
# `just ci` runs the same checks as GitHub Actions' Rust jobs.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default: ci

# Format the code.
fmt:
    cargo fmt --all

# Formatting, lints and dependency checks, without changing anything.
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check

# Run the test suite.
test:
    cargo test --workspace

# Build the release executables into target/release/.
build:
    cargo build --release --workspace

# Run the built CLI on the smoke-test file, as CI does.
smoke: build
    ./target/release/imr-chart-cli --version
    ./target/release/imr-chart-cli tests/golden/smoke/smoke.csv -c Value --lsl 2e7 -o target/smoke

# Everything CI checks (verify-xlsx also needs Python with the `formulas` package).
ci: lint test smoke

# Have Excel formulas (Python's `formulas` package) recompute workbooks written by the Rust CLI.
verify-xlsx python=".venv/bin/python": build
    bash tests/golden/verify_xlsx.sh {{python}}

# Save the fixture charts as PNG into target/charts/ to look at.
charts:
    IMR_CHART_DIR=target/charts cargo test -p imr-charts --release --test render

# Regenerate the golden fixtures from the Python implementation (only while it still exists).
golden:
    .venv/bin/python tests/golden/generate.py
    .venv/bin/python tests/golden/generate_charts.py
