from __future__ import annotations

import os
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

DEFAULT_ARTIFACT_DIR = ROOT / "acceptance_artifacts_phase_0_7_anti_hallucination_strict"
DEFAULT_PHASE06_DIR = ROOT / "acceptance_artifacts_phase_0_6_structure_patch_strict"
DEFAULT_PHASE05_DIR = ROOT / "acceptance_artifacts_phase_0_5_gpu_strict"
DEFAULT_LIGAND_INPUT = ROOT / "inputs/phase_0_7/ligands/ligands.tsv"
DEFAULT_DOCKING_CONFIG = ROOT / "configs/phase_0_7_docking.yml"
DEFAULT_CONTROLS_CONFIG = ROOT / "configs/phase_0_7_controls.yml"
DEFAULT_IFP_CONFIG = ROOT / "configs/phase_0_7_interaction_fingerprints.yml"
DEFAULT_ANTI_HALLUCINATION_CONFIG = ROOT / "configs/phase_0_7_anti_hallucination.yml"


def path_arg(value: str) -> Path:
    return Path(value).expanduser()


def env_default(name: str, fallback: str) -> str:
    return os.environ.get(name, fallback)
