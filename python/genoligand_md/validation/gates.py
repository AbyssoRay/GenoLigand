from __future__ import annotations

from pathlib import Path

from ..phase084 import validate_phase084


def validate(artifact_dir: Path, strict: bool = True) -> tuple[bool, dict[str, object]]:
    return validate_phase084(artifact_dir, strict=strict)
