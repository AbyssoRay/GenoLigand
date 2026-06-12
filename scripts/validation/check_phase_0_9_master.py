from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_ARTIFACT = ROOT / "acceptance_artifacts_phase_0_9_webui_local_beta_strict"
SERVER = ROOT / "target" / "debug" / ("gls-server.exe" if sys.platform.startswith("win") else "gls-server")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", default=str(DEFAULT_ARTIFACT))
    parser.add_argument("--workspace-root", default=str(ROOT))
    args = parser.parse_args()
    artifact_dir = Path(args.artifact_dir)
    command = [
        str(SERVER),
        "validate-phase-0-9",
        "--artifact-dir",
        str(artifact_dir),
        "--workspace-root",
        str(Path(args.workspace_root)),
    ]
    result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (artifact_dir / "logs").mkdir(parents=True, exist_ok=True)
    (artifact_dir / "logs" / "check_phase_0_9_master.stdout.txt").write_text(result.stdout, encoding="utf-8")
    (artifact_dir / "logs" / "check_phase_0_9_master.stderr.txt").write_text(result.stderr, encoding="utf-8")
    if result.returncode != 0:
        return result.returncode
    master_path = artifact_dir / "metrics" / "master_validation_phase_0_9.json"
    master = json.loads(master_path.read_text(encoding="utf-8"))
    return 0 if master.get("final_verdict") == "PASS" and master.get("master_validation_p0_all_pass") else 1


if __name__ == "__main__":
    raise SystemExit(main())
