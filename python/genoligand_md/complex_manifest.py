from __future__ import annotations

from pathlib import Path

from .phase084 import ROOT, select_md_complexes


def build_complex_manifest(artifact_dir: Path, config: Path | None = None) -> int:
    return select_md_complexes(ROOT / artifact_dir if not artifact_dir.is_absolute() else artifact_dir, config)
