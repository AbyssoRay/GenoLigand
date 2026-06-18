from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    forbidden_roots = ["outputs", ".genoligand", "acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict"]
    for rel in forbidden_roots:
        require(not (ROOT / rel).exists(), f"release root must stay clean: {rel}")

    transient_files = list(ROOT.glob(".nextflow.log*")) + list(ROOT.glob("*.tmp"))
    require(not transient_files, "transient files must be cleaned before release")

    print("clean_release_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
