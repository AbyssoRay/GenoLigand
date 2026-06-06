from __future__ import annotations

import os
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

DEFAULT_ARTIFACT_DIR = ROOT / "acceptance_artifacts_phase_0_6_structure_patch_strict"
DEFAULT_PHASE05_DIR = ROOT / "acceptance_artifacts_phase_0_5_gpu_strict"
DEFAULT_BIOLIP_URL = "https://zhanggroup.org/BioLiP/download/BioLiP.txt.gz"
DEFAULT_RCSB_PDB_URL = "https://files.rcsb.org/download/{pdb_id}.pdb"


def env_or_default(name: str, default: str = "") -> str:
    return os.environ.get(name, default)


def path_arg(value: str) -> Path:
    return Path(value).expanduser()
