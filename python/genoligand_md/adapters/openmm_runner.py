from __future__ import annotations

from pathlib import Path

from ..phase084 import probe_openmm_platforms, run_md_stage


def probe(artifact_dir: Path) -> int:
    return probe_openmm_platforms(artifact_dir)


def run_short_md(artifact_dir: Path) -> int:
    return run_md_stage(artifact_dir)
