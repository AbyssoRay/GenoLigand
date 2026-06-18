from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    workflow = read("workflows/main.nf")
    cli = read("crates/gls-cli/src/main.rs")
    server = read("crates/gls-server/src/lib.rs")

    require("pipeline run" in workflow, "workflow must call full pipeline")
    require("pipeline" in cli and "run" in cli, "gls-cli must expose pipeline run")
    for input_key in ["small_molecule", "peptide", "protein_ligand", "complex", "genome_or_metagenome"]:
        require(input_key in server, f"server must preserve input type: {input_key}")

    print("pipeline_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
