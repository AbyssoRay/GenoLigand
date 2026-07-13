from __future__ import annotations

import sys

from geno_plm.cli import validate_manifest


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 1:
        raise SystemExit("usage: check_plm_embedding_manifest.py MANIFEST")
    validate_manifest(args[0])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
