from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _bootstrap import (
    DEFAULT_JAVA,
    DEFAULT_LIGAND_PREP,
    DEFAULT_NEXTFLOW_JAR,
    DEFAULT_OBABEL,
    DEFAULT_PHASE071_DIR,
    DEFAULT_PYTHON,
    DEFAULT_RECEPTOR_PREP,
    DEFAULT_VINA,
    env_default,
    path_arg,
)
from genoligand_phase07.audit_fix import extract_tool_versions


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=DEFAULT_PHASE071_DIR)
    parser.add_argument("--python", default=env_default("GENOLIGAND_PHASE07_PYTHON", DEFAULT_PYTHON if DEFAULT_PYTHON else sys.executable))
    parser.add_argument("--vina-bin", default=env_default("GENOLIGAND_PHASE07_VINA", DEFAULT_VINA))
    parser.add_argument("--ligand-prep-command", default=env_default("GENOLIGAND_PHASE07_LIGAND_PREP", DEFAULT_LIGAND_PREP))
    parser.add_argument("--receptor-prep-command", default=env_default("GENOLIGAND_PHASE07_RECEPTOR_PREP", DEFAULT_RECEPTOR_PREP))
    parser.add_argument("--obabel-bin", default=env_default("GENOLIGAND_PHASE07_OBABEL", DEFAULT_OBABEL))
    parser.add_argument("--java", default=env_default("JAVA_EXE", DEFAULT_JAVA))
    parser.add_argument("--nextflow-jar", default=env_default("NEXTFLOW_JAR", str(DEFAULT_NEXTFLOW_JAR)))
    parser.add_argument("--nextflow", default=env_default("NEXTFLOW_BIN", "nextflow"))
    args = parser.parse_args()
    raise SystemExit(
        extract_tool_versions(
            args.artifact_dir,
            args.python,
            args.vina_bin,
            args.ligand_prep_command,
            args.receptor_prep_command,
            args.obabel_bin,
            args.java,
            args.nextflow_jar,
            args.nextflow,
        )
    )
