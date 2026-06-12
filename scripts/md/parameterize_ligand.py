from __future__ import annotations

from md_cli import cli

if __name__ == "__main__":
    raise SystemExit(cli(["parameterize", *(__import__("sys").argv[1:])]))
