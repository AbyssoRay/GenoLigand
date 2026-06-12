from __future__ import annotations

from pathlib import Path

from ..phase084 import prepare_receptors


def prepare(artifact_dir: Path) -> int:
    return prepare_receptors(artifact_dir)
