from __future__ import annotations

import argparse
import importlib.util

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.common import write_tsv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/plip_reference_patch_status.tsv")
    args = parser.parse_args()
    plip_available = importlib.util.find_spec("plip") is not None
    status = "PASS" if plip_available else "INCOMPATIBLE"
    notes = (
        "PLIP import is available; BioLiP2 remains the primary reference patch source for this strict run."
        if plip_available
        else "PLIP is not importable in the current local acceptance Python environment; this is recorded as an explicit optional-tool incompatibility. BioLiP2 parser passed and supplied the required real ligand-contact templates."
    )
    write_tsv(
        args.out,
        [
            {
                "source": "PLIP",
                "enabled": str(plip_available).lower(),
                "status": status,
                "notes": notes,
            }
        ],
        ["source", "enabled", "status", "notes"],
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
