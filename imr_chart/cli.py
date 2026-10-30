"""Command-line interface: ``imr-chart FILE --column COL [--lsl 2e7] ...``."""

from __future__ import annotations

import argparse
import sys

from . import __version__, loader
from .report import FORMATS, build_job
from .stats import parse_limit


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="imr-chart",
        description="Generate Individual & Moving Range (I-MR) control charts from Excel or CSV files.",
    )
    p.add_argument("file", help="Input .xlsx/.xlsm/.xls or .csv file")
    p.add_argument("--sheet", help="Worksheet name (default: 'Summary' if present, else the first data sheet)")
    p.add_argument("-c", "--column", action="append", dest="columns", metavar="COLUMN",
                   help="Numeric column to chart; repeat for several charts")
    p.add_argument("--date-column", help="Date column for the x-axis (default: 'Date Processed' if present)")
    p.add_argument("--no-date", action="store_true", help="Plot in row order instead of by date")
    p.add_argument("--label-column", help="Column used to identify points (default: 'QC Lot' if present)")
    p.add_argument("--lsl", action="append", dest="lsls", metavar="VALUE",
                   help="Lower spec limit, e.g. 2e7 or 50%%; repeat once per --column (or give one for all)")
    p.add_argument("--sigma", type=float, default=3.0, help="Sigma multiplier for control limits (default 3)")
    p.add_argument("-o", "--out", default=".", help="Output directory (default: current directory)")
    p.add_argument("--formats", default=",".join(FORMATS),
                   help=f"Comma-separated output formats from {', '.join(FORMATS)} (default: all)")
    p.add_argument("--list-columns", action="store_true", help="List sheets and numeric columns, then exit")
    p.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        df = loader.load_table(args.file, args.sheet)

        if args.list_columns:
            print("Sheets:", ", ".join(loader.list_sheets(args.file)))
            print("Date columns:", ", ".join(loader.date_columns(df)) or "(none)")
            print("Numeric columns:")
            for c in loader.numeric_columns(df):
                print(f"  {c}")
            return 0

        if not args.columns:
            print("error: at least one --column is required (use --list-columns to see options)", file=sys.stderr)
            return 2

        formats = [f.strip().lower() for f in args.formats.split(",") if f.strip()]
        bad = [f for f in formats if f not in FORMATS]
        if bad:
            print(f"error: unknown format(s): {', '.join(bad)}", file=sys.stderr)
            return 2

        lsls = [parse_limit(v) for v in (args.lsls or [])]
        if len(lsls) > 1 and len(lsls) != len(args.columns):
            print("error: give one --lsl for all columns, or one per --column", file=sys.stderr)
            return 2
        if len(lsls) <= 1:
            lsls = (lsls or [None]) * len(args.columns)

        date_col = None if args.no_date else (args.date_column or loader.pick_date_column(df))
        for column, lsl in zip(args.columns, lsls):
            job = build_job(df, args.file, column, date_col, lsl=lsl, k=args.sigma, label_column=args.label_column)
            r = job.result
            print(f"{column}: n={r.n} excluded={job.excluded} mean={r.mean:.6g} "
                  f"UCL={r.ucl:.6g} LCL={r.lcl:.6g} MR-UCL={r.mr_ucl:.6g}")
            for path in job.save_all(args.out, formats):
                print(f"  wrote {path}")
        return 0
    except (FileNotFoundError, KeyError, ValueError) as exc:
        msg = exc.args[0] if isinstance(exc, KeyError) and exc.args else exc
        print(f"error: {msg}", file=sys.stderr)
        return 1
