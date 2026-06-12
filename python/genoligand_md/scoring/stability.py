from __future__ import annotations

from pathlib import Path

from ..phase084 import score_md_stability


def score(artifact_dir: Path) -> int:
    return score_md_stability(artifact_dir)
