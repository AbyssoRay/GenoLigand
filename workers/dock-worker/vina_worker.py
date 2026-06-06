#!/usr/bin/env python3
"""Persistent AutoDock Vina worker adapter.

Expected DOCK request fields:
  DOCK<TAB>candidate_id|profile_id|patch_score[<TAB>...]

Required environment:
  GENOLIGAND_VINA_COMMAND=/path/to/vina
  GENOLIGAND_DOCK_INPUT_DIR=/path/to/pdbqt_inputs

Optional:
  GENOLIGAND_VINA_ARGS="--exhaustiveness 8 --cpu 4"
"""

import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

if hasattr(sys.stdin, "reconfigure"):
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")


def respond_ok(payload: str = "") -> None:
    sys.stdout.write(f"OK\t{payload}\n" if payload else "OK\n")
    sys.stdout.flush()


def respond_err(reason: str) -> None:
    sys.stdout.write(f"ERR\t{reason}\n")
    sys.stdout.flush()


def safe_id(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", value)


def parse_vina_score(output: str) -> float:
    for raw_line in output.splitlines():
        line = raw_line.strip()
        if line.startswith("REMARK VINA RESULT:"):
            return float(line.split()[3])
        parts = line.split()
        if len(parts) >= 2 and parts[0] == "1":
            return float(parts[1])
    raise RuntimeError("vina_result_missing_score")


def run_vina(candidate_id: str) -> float:
    vina = os.environ.get("GENOLIGAND_VINA_COMMAND")
    input_dir = os.environ.get("GENOLIGAND_DOCK_INPUT_DIR")
    if not vina:
        raise RuntimeError("vina_command_not_configured")
    if not input_dir:
        raise RuntimeError("dock_input_dir_not_configured")
    stem = safe_id(candidate_id)
    receptor = Path(input_dir) / f"receptor_{stem}.pdbqt"
    ligand = Path(input_dir) / f"ligand_{stem}.pdbqt"
    if not receptor.exists():
        raise RuntimeError(f"vina_receptor_missing:{receptor}")
    if not ligand.exists():
        raise RuntimeError(f"vina_ligand_missing:{ligand}")
    command = [
        vina,
        "--receptor",
        str(receptor),
        "--ligand",
        str(ligand),
        *shlex.split(os.environ.get("GENOLIGAND_VINA_ARGS", "")),
    ]
    completed = subprocess.run(
        command,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    output = completed.stdout + "\n" + completed.stderr
    if completed.returncode != 0:
        raise RuntimeError(f"vina_command_failed:{completed.returncode}:{completed.stderr.strip()}")
    return parse_vina_score(output)


def handle(line: str) -> str:
    fields = line.lstrip("\ufeff").rstrip("\r\n").split("\t")
    if not fields or fields[0] != "DOCK":
        raise RuntimeError("dock_worker_bad_request")
    scores = []
    for entry in fields[1:]:
        parts = entry.split("|")
        if len(parts) != 3:
            raise RuntimeError("dock_worker_bad_entry")
        candidate_id = parts[0]
        scores.append(f"{run_vina(candidate_id):.6f}")
    return ";".join(scores)


def main() -> int:
    for line in sys.stdin:
        try:
            respond_ok(handle(line))
        except Exception as exc:  # noqa: BLE001
            respond_err(str(exc))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
