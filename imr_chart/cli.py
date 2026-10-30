"""Command-line interface: ``imr-chart FILE [FILE ...] --column COL [--lsl 2e7] ...``."""

from __future__ import annotations

import argparse
import sys

from . import __version__, loader, rules
from .plot_static import DEFAULT_RECENT_DAYS
from .report import FORMATS, build_multi_job
from .stats import parse_limit


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="imr-chart",
        description="Generate Individual & Moving Range (I-MR) control charts from Excel or CSV files.",
    )
    p.add_argument("files", nargs="+", metavar="FILE",
                   help="Input .xlsx/.xlsm/.xls or .csv file; give several files with the same layout "
                        "to chart all their rows together")
    p.add_argument("--sheet", help="Worksheet name, the same in every file "
                                   "(default: 'Summary' if present, else the first data sheet)")
    p.add_argument("-c", "--column", action="append", dest="columns", metavar="COLUMN",
                   help="Numeric column to chart; repeat for several charts")
    p.add_argument("--date-column", help="Date column for the x-axis (default: 'Date Processed' if present)")
    p.add_argument("--no-date", action="store_true", help="Plot in row order instead of by date")
    p.add_argument("--label-column", help="Column used to identify points (default: 'QC Lot' if present)")
    p.add_argument("--lsl", action="append", dest="lsls", metavar="VALUE",
                   help="Lower spec limit, e.g. 2e7 or 50%%; repeat once per --column (or give one for all)")
    p.add_argument("--sigma", type=float, default=3.0, help="Sigma multiplier for control limits (default 3)")
    p.add_argument("--recent-days", type=int, default=DEFAULT_RECENT_DAYS, metavar="N",
                   help=f"Highlight the last N days of data (default {DEFAULT_RECENT_DAYS}; 0 disables)")
    p.add_argument("--rules", metavar="FILE",
                   help="JSON file of highlight rules: points whose row matches a rule are drawn in its color "
                        "(save one from the GUI, or see the README)")
    p.add_argument("-o", "--out", default=".", help="Output directory (default: current directory)")
    p.add_argument("--formats", default=",".join(FORMATS),
                   help=f"Comma-separated output formats from {', '.join(FORMATS)} (default: all)")
    p.add_argument("--list-columns", action="store_true", help="List sheets and numeric columns, then exit")
    p.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    return p


def _load(path: str, sheet: str | None):
    """Load one input; errors name the file when several are given."""
    try:
        return loader.load_table(path, sheet)
    except ValueError as exc:
        raise ValueError(f"{path}: {exc}") from None


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        tables = [(path, _load(path, args.sheet)) for path in args.files]
        frames = [df for _, df in tables]

        if args.list_columns:
            print("Sheets:", ", ".join(loader.common_columns([loader.list_sheets(f) for f in args.files])))
            print("Date columns:", ", ".join(loader.common_columns([loader.date_columns(df) for df in frames]))
                  or "(none)")
            print("Numeric columns:")
            for c in loader.pooled_numeric_columns(frames):
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
        if args.recent_days < 0:
            print("error: --recent-days must be 0 or more", file=sys.stderr)
            return 2

        lsls = [parse_limit(v) for v in (args.lsls or [])]
        if len(lsls) > 1 and len(lsls) != len(args.columns):
            print("error: give one --lsl for all columns, or one per --column", file=sys.stderr)
            return 2
        if len(lsls) <= 1:
            lsls = (lsls or [None]) * len(args.columns)

        highlight_rules = rules.load_rules(args.rules) if args.rules else []
        sheets = sorted({loader.list_sheets(f)[0] if args.sheet is None else args.sheet
                         for f in args.files if not loader.is_csv(f)})
        sheet = ", ".join(sheets) or None
        date_col = None if args.no_date else (args.date_column or loader.pick_date_column(frames[0]))
        for column, lsl in zip(args.columns, lsls):
            job = build_multi_job(tables, column, date_col, lsl=lsl, k=args.sigma, label_column=args.label_column,
                                  recent_days=args.recent_days, sheet=sheet, rules=highlight_rules)
            r = job.result
            files = f"files={len(job.sources)} " if len(job.sources) > 1 else ""
            print(f"{column}: {files}n={r.n} excluded={job.excluded} mean={r.mean:.6g} "
                  f"UCL={r.ucl:.6g} LCL={r.lcl:.6g} MR-UCL={r.mr_ucl:.6g}"
                  + (f" highlighted={job.highlighted}" if highlight_rules else ""))
            if job.duplicates:
                print(f"  warning: {job.duplicates} points have the same lot, date and value in more than one file "
                      "(overlapping exports?); all are charted", file=sys.stderr)
            for path in job.save_all(args.out, formats):
                print(f"  wrote {path}")
        return 0
    except (FileNotFoundError, KeyError, ValueError) as exc:
        msg = exc.args[0] if isinstance(exc, KeyError) and exc.args else exc
        print(f"error: {msg}", file=sys.stderr)
        return 1
