from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.common import write_tsv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/afdb_reference_subset_status.tsv")
    args = parser.parse_args()
    write_tsv(
        args.out,
        [
            {
                "source": "AlphaFoldDB",
                "enabled": "false",
                "status": "SKIP",
                "notes": "Phase 0.6 strict run uses RCSB PDB plus BioLiP2 as the real external reference set.",
            }
        ],
        ["source", "enabled", "status", "notes"],
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
