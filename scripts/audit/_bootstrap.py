from __future__ import annotations

import os
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))


def path_arg(value: str) -> Path:
    return Path(value).resolve()


def env_default(name: str, default: str) -> str:
    return os.environ.get(name, default)


DEFAULT_PHASE07_DIR = ROOT / "acceptance_artifacts_phase_0_7_anti_hallucination_strict"
DEFAULT_PHASE071_DIR = ROOT / "acceptance_artifacts_phase_0_7_1_audit_fix_strict"
DEFAULT_VINA = r"D:\user\codex\tools\genoligand-phase07\vina_1.2.7_win.exe"
DEFAULT_PYTHON = r"D:\user\codex\tools\genoligand-phase07\python312-embed\python.exe"
DEFAULT_LIGAND_PREP = r"D:\user\codex\tools\genoligand-phase07\python312-embed\Scripts\mk_prepare_ligand.exe"
DEFAULT_RECEPTOR_PREP = r"D:\user\codex\tools\genoligand-phase07\python312-embed\Scripts\mk_prepare_receptor.exe"
DEFAULT_OBABEL = r"D:\user\codex\tools\genoligand-phase07\python312-embed\Scripts\obabel.exe"
DEFAULT_JAVA = r"D:\user\codex\tools\java\jdk-25.0.3+9\bin\java.exe"
DEFAULT_NEXTFLOW_JAR = ROOT / "acceptance_artifacts_phase_0_2/nextflow_rebuild/nextflow-patched.jar"
