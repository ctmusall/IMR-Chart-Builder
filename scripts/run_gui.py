"""PyInstaller entry point for the windowed build (GUI, or CLI when given options)."""

import sys

from imr_chart.__main__ import main

sys.exit(main())
