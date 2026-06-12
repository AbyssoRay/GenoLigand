from __future__ import annotations

from pathlib import Path

from ..phase084 import analyze_md_outputs


def compute(artifact_dir: Path) -> int:
    return analyze_md_outputs(artifact_dir)
