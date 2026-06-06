from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.common import write_tsv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/plip_reference_patch_status.tsv")
    args = parser.parse_args()
    write_tsv(
        args.out,
        [
            {
                "source": "PLIP",
                "enabled": "false",
                "status": "SKIP",
                "notes": "BioLiP2 residue annotations are the required real ligand-contact source for this strict acceptance run.",
            }
        ],
        ["source", "enabled", "status", "notes"],
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
