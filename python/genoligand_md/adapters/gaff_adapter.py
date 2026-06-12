from __future__ import annotations

from pathlib import Path

from ..phase084 import parameterize_ligands


def parameterize(artifact_dir: Path) -> int:
    return parameterize_ligands(artifact_dir)
