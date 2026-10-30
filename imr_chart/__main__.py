"""Entry point.

No arguments, or a single file path (e.g. a file dropped onto the .exe), opens
the GUI; anything else runs the CLI.
"""

import sys


def wants_gui(argv: list[str]) -> bool:
    # Bare file paths (e.g. files dropped onto the exe) open the GUI; any option means the CLI.
    return not any(a.startswith("-") for a in argv)


def main() -> int:
    if not wants_gui(sys.argv[1:]):
        from .cli import main as cli_main

        return cli_main(sys.argv[1:])
    from .gui import main as gui_main

    return gui_main()


if __name__ == "__main__":
    sys.exit(main())
