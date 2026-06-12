from __future__ import annotations

from pathlib import Path

from ..phase084 import render_acceptance_report


def render(artifact_dir: Path) -> int:
    return render_acceptance_report(artifact_dir)
