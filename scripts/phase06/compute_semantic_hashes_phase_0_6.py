from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.pipeline import semantic_hashes


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR)
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/hashes/semantic_hashes.tsv")
    args = parser.parse_args()
    return semantic_hashes(args.artifact_dir, args.out)


if __name__ == "__main__":
    raise SystemExit(main())
