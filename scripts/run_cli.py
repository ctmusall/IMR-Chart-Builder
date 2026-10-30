"""PyInstaller entry point for the console build (always the CLI)."""

import sys

from imr_chart.cli import main

sys.exit(main())
