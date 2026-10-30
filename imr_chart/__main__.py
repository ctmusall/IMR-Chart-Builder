"""Entry point.

No arguments, or a single file path (e.g. a file dropped onto the .exe), opens
the GUI; anything else runs the CLI.
"""

import sys


def wants_gui(argv: list[str]) -> bool:
    return len(argv) == 0 or (len(argv) == 1 and not argv[0].startswith("-"))


def main() -> int:
    if not wants_gui(sys.argv[1:]):
        from .cli import main as cli_main

        return cli_main(sys.argv[1:])
    from .gui import main as gui_main

    return gui_main()


if __name__ == "__main__":
    sys.exit(main())
