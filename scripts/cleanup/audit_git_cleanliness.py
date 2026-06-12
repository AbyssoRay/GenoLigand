from __future__ import annotations

from audit_stale_path_references import cli, sys

if __name__ == "__main__":
    raise SystemExit(cli(["acceptance", *sys.argv[1:]]))
