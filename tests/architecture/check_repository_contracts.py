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
    require("gls_cli" in workflow and "pipeline run" in workflow, "main.nf must dispatch gls-cli pipeline run")
    require("scripts/" not in workflow.replace("scripts/setup/", ""), "main.nf must not call scripts directly")

    server = read("crates/gls-server/src/lib.rs")
    require(
        "scripts/" not in server.replace("scripts/setup/", ""),
        "gls-server must enter scientific work through product APIs",
    )
    require("install_complete_scientific.py" in server, "installer exception must be explicit")

    require((ROOT / "docs/architecture.md").is_file(), "architecture responsibility map is required")

    for schema in [
        "schemas/project.schema.json",
        "schemas/run.schema.json",
        "schemas/artifact.schema.json",
        "schemas/diagnostics.schema.json",
        "schemas/install_status.schema.json",
        "schemas/report_manifest.schema.json",
        "schemas/project_package.schema.json",
    ]:
        require((ROOT / schema).is_file(), f"missing schema: {schema}")

    print("repository_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
