from __future__ import annotations

from inventory_external_tools import cli, sys

if __name__ == "__main__":
    raise SystemExit(cli(["acceptance", *sys.argv[1:]]))
