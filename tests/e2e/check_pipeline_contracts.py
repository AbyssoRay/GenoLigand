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
    science_api = read("crates/gls-science/src/lib.rs")
    science_conversion = read("crates/gls-science/src/input_conversion.rs")
    legacy_module = "_".join(["input", "recognition"])
    legacy_path = ROOT / "crates" / "gls-server" / "src" / f"{legacy_module}.rs"

    require("pipeline run" in workflow, "workflow must call full pipeline")
    require("pipeline" in cli and "run" in cli, "gls-cli must expose pipeline run")
    require(
        "inspect_input_manifest" in server,
        "server must use authoritative science inspection",
    )
    require(
        "pub fn inspect_input_manifest" in science_api,
        "science must expose authoritative input inspection",
    )
    require(
        "convert_inputs_from_manifest" in science_api,
        "science inspection must use manifest input conversion",
    )
    require(not legacy_path.exists(), "legacy server recognizer module must be removed")
    require(
        f"mod {legacy_module}" not in server,
        "server must not declare the legacy recognizer module",
    )
    for input_key in ["small_molecule", "peptide", "protein_ligand", "complex", "genome_or_metagenome"]:
        require(
            f'"{input_key}"' in science_conversion,
            f"science resolver must preserve input type: {input_key}",
        )

    print("pipeline_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
