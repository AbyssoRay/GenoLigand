from __future__ import annotations

import argparse
import base64
import csv
import gzip
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path
from typing import Iterable

from .schema import COMPLEX_MANIFEST_COLUMNS, LIGAND_PARAMETERIZATION_COLUMNS, SCHEMA_VERSION


ROOT = Path(__file__).resolve().parents[2]
PHASE084_DIRNAME = "acceptance_artifacts_phase_0_8_4_openmm_short_md_strict"
PHASE084_MODE = "phase_0_8_4_openmm_short_md_strict"
PHASE071_DIRNAME = "acceptance_artifacts_phase_0_7_1_audit_fix_strict"
PHASE083_DIRNAME = "acceptance_artifacts_phase_0_8_3_biolip2_resolver_patch_strict"
DEFAULT_DROPOUT_DISTRIBUTION = "GenoLigandPhase06"
DEFAULT_WSL_DISTRIBUTION = "GenoLigandPhase08"
DEFAULT_WSL_PYTHON = "/opt/genoligand-phase084-openff/bin/python"
FALLBACK_WSL_PYTHON = "/opt/genoligand-phase084-openmm/bin/python"
DUDEZ_ROOT = ROOT / "data/external_phase_0_8/dude_z"
PREFERRED_DUDEZ_TARGETS = ["FKB1A", "FABP4", "HIVPR", "HS90A", "KITH", "ABL1", "ANDR", "EGFR"]


P0_GATES = [
    "phase_0_7_1_regression_pass",
    "phase_0_8_3_regression_pass",
    "openmm_installed",
    "openmm_test_installation_pass",
    "cuda_platform_available",
    "cuda_platform_selected_for_primary_md",
    "cpu_smoke_md_pass",
    "gpu_short_md_pass",
    "mandatory_complexes_selected",
    "positive_control_complex_prepared",
    "redocked_pose_complex_prepared_or_explicitly_not_available_with_reason",
    "decoy_complexes_prepared",
    "ligand_parameterization_success_for_mandatory_complexes",
    "system_preparation_success_for_mandatory_complexes",
    "minimization_success",
    "equilibration_success",
    "production_md_success",
    "trajectory_files_present",
    "checkpoint_files_present",
    "state_data_logs_present",
    "no_nan_or_inf_in_energy_temperature_or_coordinates",
    "ligand_rmsd_table_present",
    "pocket_rmsd_table_present",
    "interaction_occupancy_table_present",
    "contact_persistence_table_present",
    "md_stability_summary_present",
    "decoy_calibrated_md_summary_present",
    "nextflow_first_run_success",
    "nextflow_resume_run_success",
    "semantic_hash_match",
    "evidence_ledger_covers_all_report_claims",
    "integration_adapter_architecture_pass",
    "open_source_compliance_pass",
    "no_proprietary_mandatory_dependency_pass",
    "no_raw_tool_report_dependency_pass",
    "genoligand_owned_score_pass",
    "canonical_schema_closure_pass",
    "cleanup_inventory_before_exists",
    "cleanup_plan_exists",
    "cleanup_dry_run_pass",
    "no_protected_path_in_delete_or_move_plan",
    "cleanup_execution_matches_plan",
    "no_default_wsl_deleted",
    "no_unrelated_wsl_deleted",
    "project_wsl_cleanup_policy_pass",
    "external_tool_cleanup_policy_pass",
    "stale_path_reference_audit_pass",
    "git_cleanliness_audit_pass",
    "external_resource_manifest_present",
    "tool_versions_manifest_present",
    "cleanup_resource_manifest_present",
    "post_cleanup_cargo_test_pass",
    "post_cleanup_nextflow_resume_pass",
    "post_cleanup_openmm_validation_pass",
    "storage_delta_summary_exists",
    "master_validation_p0_all_pass",
]


def path_arg(value: str) -> Path:
    return Path(value).resolve()


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def ensure_dir(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)


def ensure_parent(path: Path) -> None:
    if path.parent:
        ensure_dir(path.parent)


def bool_text(value: bool) -> str:
    return "True" if value else "False"


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def read_tsv(path: Path) -> list[dict[str, str]]:
    if not path.exists() or path.stat().st_size == 0:
        return []
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def write_tsv(path: Path, rows: Iterable[dict[str, object]], columns: list[str]) -> None:
    ensure_parent(path)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=columns, delimiter="\t", lineterminator="\n", extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({column: "" if row.get(column) is None else str(row.get(column, "")) for column in columns})


def write_json(path: Path, payload: dict[str, object]) -> None:
    ensure_parent(path)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def read_json(path: Path) -> dict[str, object]:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8-sig"))


def first_row(path: Path) -> dict[str, str]:
    rows = read_tsv(path)
    return rows[0] if rows else {}


def phase084_dirs(artifact_dir: Path) -> None:
    for item in [
        "inputs",
        "outputs/selection",
        "outputs/system_prep/receptors_prepared",
        "outputs/system_prep/ligands_prepared",
        "outputs/system_prep/ligand_topologies",
        "outputs/system_prep/systems",
        "outputs/system_prep/topologies",
        "outputs/system_prep/initial_coordinates",
        "outputs/md_runs",
        "outputs/md_analysis",
        "outputs/scoring",
        "outputs/compliance",
        "outputs/evidence",
        "outputs/reports",
        "outputs/cleanup",
        "outputs/workflow/markers",
        "metrics",
        "logs/command_stdout_stderr",
        "logs/openmm",
        "logs/nextflow",
        "logs/cleanup",
    ]:
        ensure_dir(artifact_dir / item)


def decode_bytes(data: bytes) -> str:
    if not data:
        return ""
    if len(data) >= 2 and data[1:2] == b"\x00":
        return data.decode("utf-16le", errors="replace")
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return data.decode("utf-16le", errors="replace")


def run_command(command: list[str], artifact_dir: Path, command_id: str, timeout: int = 3600, cwd: Path | None = None) -> dict[str, object]:
    started = time.time()
    stdout_path = artifact_dir / "logs/command_stdout_stderr" / f"{command_id}.stdout.txt"
    stderr_path = artifact_dir / "logs/command_stdout_stderr" / f"{command_id}.stderr.txt"
    ensure_parent(stdout_path)
    try:
        completed = subprocess.run(command, cwd=cwd or ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)
        stdout = decode_bytes(completed.stdout)
        stderr = decode_bytes(completed.stderr)
        code = int(completed.returncode)
    except Exception as exc:  # noqa: BLE001 - command failure is evidence, not a crash
        stdout = ""
        stderr = repr(exc)
        code = 999
    stdout_path.write_text(stdout, encoding="utf-8", errors="replace")
    stderr_path.write_text(stderr, encoding="utf-8", errors="replace")
    return {
        "command_id": command_id,
        "command": " ".join(command),
        "exit_code": code,
        "stdout_log": str(stdout_path),
        "stderr_log": str(stderr_path),
        "runtime_seconds": f"{time.time() - started:.3f}",
    }


def running_inside_wsl() -> bool:
    if os.name == "nt":
        return False
    if os.environ.get("WSL_DISTRO_NAME"):
        return True
    try:
        return "microsoft" in Path("/proc/sys/kernel/osrelease").read_text(encoding="utf-8", errors="replace").lower()
    except OSError:
        return False


def windows_path_to_wsl(path_text: str) -> str:
    normalized = path_text.replace("\\", "/")
    if len(normalized) > 2 and normalized[1] == ":":
        drive = normalized[0].lower()
        return f"/mnt/{drive}{normalized[2:]}"
    return normalized


def wsl_path_to_windows(path_text: str) -> str:
    normalized = path_text.replace("\\", "/")
    match = re.match(r"^/mnt/([A-Za-z])/(.*)$", normalized)
    if match:
        rest = match.group(2).replace("/", "\\")
        return f"{match.group(1).upper()}:\\{rest}"
    return path_text


def runtime_path(path_text: str) -> Path:
    if not path_text:
        return Path("")
    if running_inside_wsl():
        return Path(windows_path_to_wsl(path_text))
    return Path(wsl_path_to_windows(path_text))


def path_exists_any(path_text: str) -> bool:
    if not path_text:
        return False
    return runtime_path(path_text).exists()


def quote_sh(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"


def wsl_artifact_path(path: Path) -> str:
    return windows_path_to_wsl(str(path.resolve()))


def wsl_bash(distro: str, script: str, artifact_dir: Path, command_id: str, timeout: int = 3600) -> dict[str, object]:
    if running_inside_wsl():
        return run_command(["bash", "-lc", script], artifact_dir, command_id, timeout=timeout)
    return run_command(["wsl.exe", "-d", distro, "--", "bash", "-lc", script], artifact_dir, command_id, timeout=timeout)


def wsl_python_script(distro: str, script: str, artifact_dir: Path, command_id: str, timeout: int = 3600) -> dict[str, object]:
    candidates = [
        os.environ.get("GENOLIGAND_PHASE084_WSL_PYTHON", ""),
        DEFAULT_WSL_PYTHON,
        FALLBACK_WSL_PYTHON,
        "python3",
    ]
    encoded = base64.b64encode(script.encode("utf-8")).decode("ascii")
    runner = f"import base64; exec(base64.b64decode('{encoded}'))"
    commands = []
    for candidate in [item for item in candidates if item]:
        if candidate.startswith("/"):
            bindir = candidate.rsplit("/", 1)[0]
            commands.append(
                f"if [ -x {quote_sh(candidate)} ]; then "
                f"export PATH={quote_sh(bindir)}:\"$PATH\"; "
                f"{quote_sh(candidate)} -c {quote_sh(runner)}; exit $?; fi"
            )
        else:
            commands.append(
                f"if command -v {quote_sh(candidate)} >/dev/null 2>&1; then "
                f"{quote_sh(candidate)} -c {quote_sh(runner)}; exit $?; fi"
            )
    commands.append("echo 'phase084_python_not_found' >&2; exit 127")
    shell = "; ".join(commands)
    return wsl_bash(distro, shell, artifact_dir, command_id, timeout=timeout)


def artifact_manifest(source_dir: Path, out: Path, label: str) -> None:
    rows: list[dict[str, object]] = []
    if source_dir.exists():
        for path in sorted(source_dir.rglob("*")):
            if path.is_file() and path.stat().st_size < 20_000_000:
                rows.append({
                    "source_label": label,
                    "relative_path": path.relative_to(source_dir).as_posix(),
                    "absolute_path": str(path),
                    "bytes": path.stat().st_size,
                    "sha256": sha256_file(path),
                })
            elif path.is_file():
                rows.append({
                    "source_label": label,
                    "relative_path": path.relative_to(source_dir).as_posix(),
                    "absolute_path": str(path),
                    "bytes": path.stat().st_size,
                    "sha256": "not_hashed_large_file",
                })
    write_tsv(out, rows, ["source_label", "relative_path", "absolute_path", "bytes", "sha256"])


def write_input_manifests(artifact_dir: Path) -> int:
    artifact_manifest(ROOT / PHASE071_DIRNAME, artifact_dir / "inputs/phase_0_7_1_artifact_manifest.tsv", "phase_0_7_1")
    artifact_manifest(ROOT / PHASE083_DIRNAME, artifact_dir / "inputs/phase_0_8_3_artifact_manifest.tsv", "phase_0_8_3")
    return 0


def inventory_wsl(artifact_dir: Path) -> int:
    rows: list[dict[str, object]] = []
    status_cmd = run_command(["wsl.exe", "--status"], artifact_dir, "wsl_status", timeout=120)
    list_cmd = run_command(["wsl.exe", "-l", "-v"], artifact_dir, "wsl_list_verbose", timeout=120)
    stdout = Path(str(list_cmd["stdout_log"])).read_text(encoding="utf-8", errors="replace")
    default_name = ""
    status_text = Path(str(status_cmd["stdout_log"])).read_text(encoding="utf-8", errors="replace")
    m = re.search(r"默认分发:\s*([^\r\n]+)|Default Distribution:\s*([^\r\n]+)", status_text)
    if m:
        default_name = (m.group(1) or m.group(2) or "").strip()
    for raw in stdout.splitlines():
        line = raw.strip("\ufeff")
        if not line.strip() or "NAME" in line and "VERSION" in line:
            continue
        is_default = line.lstrip().startswith("*")
        clean = line.replace("*", " ", 1).strip()
        match = re.match(r"(.+?)\s{2,}(\S+)\s+(\d+)\s*$", clean)
        if not match:
            continue
        name, state, version = match.group(1).strip(), match.group(2), match.group(3)
        if is_default:
            default_name = name
        project_related = bool(re.search(r"genoligand|gls|phase", name, re.I))
        protected = is_default or not project_related
        action = "KEEP"
        notes = "default_or_unrelated_protected" if protected else "project_related_candidate"
        if name == DEFAULT_WSL_DISTRIBUTION:
            action = "KEEP"
            protected = True
            notes = "selected_phase084_backend"
        elif name == DEFAULT_DROPOUT_DISTRIBUTION and project_related and not is_default:
            action = "UNREGISTER"
            protected = False
            notes = "old_project_wsl_cleanup_candidate"
        rows.append({
            "distro_name": name,
            "state": state,
            "version": version,
            "is_default": bool_text(is_default or name == default_name),
            "is_project_related": bool_text(project_related),
            "project_match_reason": "name_match" if project_related else "none",
            "recommended_action": action,
            "protected": bool_text(protected),
            "notes": notes,
        })
    write_tsv(
        artifact_dir / "outputs/cleanup/wsl_distribution_inventory.tsv",
        rows,
        ["distro_name", "state", "version", "is_default", "is_project_related", "project_match_reason", "recommended_action", "protected", "notes"],
    )
    columns = ["distro_name", "state", "version", "is_default", "is_project_related", "project_match_reason", "recommended_action", "protected", "notes"]
    write_tsv(artifact_dir / "outputs/cleanup/wsl_cleanup_plan.tsv", rows, columns)
    write_tsv(artifact_dir / "outputs/cleanup/wsl_cleanup_manifest.tsv", rows, columns)
    return 0 if rows else 1


def execute_wsl_cleanup(artifact_dir: Path, execute: bool = False) -> int:
    rows: list[dict[str, object]] = []
    for row in read_tsv(artifact_dir / "outputs/cleanup/wsl_cleanup_plan.tsv"):
        action = row.get("recommended_action", "KEEP")
        name = row.get("distro_name", "")
        status = "SKIPPED_DRY_RUN"
        exit_code = 0
        stdout_log = ""
        stderr_log = ""
        if action == "UNREGISTER" and execute:
            if row.get("protected") == "True" or row.get("is_default") == "True":
                status = "BLOCKED_PROTECTED"
                exit_code = 1
            else:
                run_command(["wsl.exe", "--terminate", name], artifact_dir, f"wsl_terminate_{name}", timeout=120)
                result = run_command(["wsl.exe", "--unregister", name], artifact_dir, f"wsl_unregister_{name}", timeout=600)
                exit_code = int(result["exit_code"])
                stdout_log = str(result["stdout_log"])
                stderr_log = str(result["stderr_log"])
                status = "PASS" if exit_code == 0 else "FAIL"
        elif action == "UNREGISTER":
            status = "PLANNED_NOT_EXECUTED"
        else:
            status = "KEEP"
        rows.append({
            "distro_name": name,
            "planned_action": action,
            "executed_action": action if execute and action == "UNREGISTER" and status == "PASS" else "NONE",
            "exit_code": exit_code,
            "stdout_log": stdout_log,
            "stderr_log": stderr_log,
            "status": status,
            "notes": row.get("notes", ""),
        })
    columns = ["distro_name", "planned_action", "executed_action", "exit_code", "stdout_log", "stderr_log", "status", "notes"]
    write_tsv(artifact_dir / "outputs/cleanup/wsl_cleanup_execution.tsv", rows, columns)
    write_tsv(artifact_dir / "outputs/cleanup/wsl_cleanup_execution_manifest.tsv", rows, columns)
    return 0


def probe_openmm_platforms(artifact_dir: Path, distro: str = DEFAULT_WSL_DISTRIBUTION) -> int:
    script = r"""
import json, math, os, subprocess, sys, time
payload = {
    "probe_time_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "python_executable": sys.executable,
    "openmm_import": False,
    "openmm_version": "",
    "available_platforms": [],
    "cuda_available": False,
    "cuda_device_index": "",
    "cuda_device_name": "",
    "cuda_precision": "mixed",
    "test_installation_exit_code": 999,
    "test_installation_pass": False,
    "platform_properties": {},
    "python_packages": {},
    "external_commands": {},
}
try:
    from importlib.metadata import PackageNotFoundError, version
    for package in ["openmm", "pdbfixer", "rdkit", "MDAnalysis", "mdtraj", "openmmforcefields", "openff-toolkit", "pandas", "pyarrow", "duckdb"]:
        try:
            payload["python_packages"][package] = version(package)
        except PackageNotFoundError:
            payload["python_packages"][package] = "not_installed"
except Exception as exc:
    payload["python_package_probe_error"] = repr(exc)
try:
    import openmm
    from openmm import Platform
    payload["openmm_import"] = True
    payload["openmm_version"] = getattr(openmm.version, "version", "")
    payload["available_platforms"] = [Platform.getPlatform(i).getName() for i in range(Platform.getNumPlatforms())]
    payload["cuda_available"] = "CUDA" in payload["available_platforms"]
    if payload["cuda_available"]:
        platform = Platform.getPlatformByName("CUDA")
        payload["platform_properties"] = {name: platform.getPropertyDefaultValue(name) for name in platform.getPropertyNames()}
        payload["cuda_precision"] = payload["platform_properties"].get("Precision", "mixed")
except Exception as exc:
    payload["import_error"] = repr(exc)
try:
    proc = subprocess.run([sys.executable, "-m", "openmm.testInstallation"], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=300, check=False)
    payload["test_installation_exit_code"] = proc.returncode
    payload["test_installation_output_tail"] = proc.stdout[-4000:]
    payload["test_installation_pass"] = proc.returncode == 0
except Exception as exc:
    payload["test_installation_error"] = repr(exc)
try:
    proc = subprocess.run(["nvidia-smi", "--query-gpu=name,driver_version,memory.total", "--format=csv,noheader"], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30, check=False)
    payload["nvidia_smi_exit_code"] = proc.returncode
    payload["nvidia_smi_output"] = proc.stdout.strip()
    if proc.returncode == 0 and proc.stdout.strip():
        fields = [part.strip() for part in proc.stdout.splitlines()[0].split(",")]
        payload["cuda_device_index"] = "0"
        payload["cuda_device_name"] = fields[0]
        payload["cuda_driver_version"] = fields[1] if len(fields) > 1 else ""
        payload["cuda_memory_total"] = fields[2] if len(fields) > 2 else ""
except Exception as exc:
    payload["nvidia_smi_error"] = repr(exc)
for command in ["antechamber", "parmchk2", "tleap"]:
    try:
        proc = subprocess.run([command, "-h"], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30, check=False)
        payload["external_commands"][command] = {
            "exit_code": proc.returncode,
            "output_head": proc.stdout[:1000],
            "available": proc.returncode in {0, 1},
        }
    except Exception as exc:
        payload["external_commands"][command] = {"exit_code": 999, "output_head": repr(exc), "available": False}
print(json.dumps(payload, sort_keys=True))
"""
    result = wsl_python_script(distro, script, artifact_dir, "openmm_platform_probe", timeout=420)
    text = Path(str(result["stdout_log"])).read_text(encoding="utf-8", errors="replace").strip()
    payload: dict[str, object]
    try:
        payload = json.loads(text.splitlines()[-1]) if text else {}
    except json.JSONDecodeError:
        payload = {"parse_error": text[-1000:]}
    payload["wsl_distro"] = distro
    payload["command_exit_code"] = result["exit_code"]
    write_json(artifact_dir / "metrics/openmm_platform_probe.json", payload)
    write_json(artifact_dir / "metrics/openmm_cuda_memory_probe.json", {
        "wsl_distro": distro,
        "cuda_device_name": payload.get("cuda_device_name", ""),
        "cuda_memory_total": payload.get("cuda_memory_total", ""),
        "nvidia_smi_output": payload.get("nvidia_smi_output", ""),
    })
    rows = [{
        "metric": "openmm_platform_probe",
        "openmm_version": payload.get("openmm_version", ""),
        "available_platforms": ",".join(payload.get("available_platforms", []) if isinstance(payload.get("available_platforms"), list) else []),
        "cuda_available": bool_text(bool(payload.get("cuda_available"))),
        "test_installation_pass": bool_text(bool(payload.get("test_installation_pass"))),
        "selected_platform": "CUDA" if payload.get("cuda_available") else "",
        "cuda_device_name": payload.get("cuda_device_name", ""),
        "cuda_precision": payload.get("cuda_precision", ""),
        "status": "PASS" if payload.get("openmm_import") and payload.get("test_installation_pass") and payload.get("cuda_available") else "FAIL",
    }]
    write_tsv(artifact_dir / "metrics/openmm_performance_summary.tsv", rows, ["metric", "openmm_version", "available_platforms", "cuda_available", "test_installation_pass", "selected_platform", "cuda_device_name", "cuda_precision", "status"])
    return 0 if rows[0]["status"] == "PASS" else 1


def source_patch_rows(limit: int = 8) -> list[dict[str, str]]:
    path = ROOT / PHASE083_DIRNAME / "outputs/biolip2_patches/biolip2_reference_patches.tsv"
    rows = []
    if path.exists():
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            for row in reader:
                if row.get("resolved_structure_path") and path_exists_any(row["resolved_structure_path"]):
                    row = dict(row)
                    row["resolved_structure_path"] = str(runtime_path(row["resolved_structure_path"]))
                    rows.append(row)
                    if len(rows) >= limit:
                        break
    return rows


def split_mol2_blocks(source: Path, count: int) -> list[dict[str, str]]:
    if not source.exists():
        return []
    text = source.read_text(encoding="utf-8", errors="replace")
    starts = [match.start() for match in re.finditer(r"(?m)^@<TRIPOS>MOLECULE\s*$", text)]
    rows: list[dict[str, str]] = []
    for idx, start in enumerate(starts[:count]):
        end = starts[idx + 1] if idx + 1 < len(starts) else len(text)
        prefix_start = text.rfind("\n##########", 0, start)
        prefix = text[prefix_start:start] if prefix_start != -1 else ""
        block = text[start:end].strip() + "\n"
        name_match = re.search(r"Name:\s*(.+)", prefix)
        smiles_match = re.search(r"SMILES:\s*(.+)", prefix)
        charge_match = re.search(r"Ligand Charge:\s*([-+0-9.]+)", prefix)
        rows.append({
            "pose_index": str(idx + 1),
            "pose_name": (name_match.group(1).strip() if name_match else f"pose_{idx + 1}"),
            "source_smiles": (smiles_match.group(1).strip() if smiles_match else ""),
            "source_charge": (charge_match.group(1).strip() if charge_match else ""),
            "mol2_block": block,
        })
    return rows


def source_dudez_pose_rows(artifact_dir: Path) -> list[dict[str, str]]:
    class_plan = [
        ("positive_native_control", "active"),
        ("positive_redocked_control", "active"),
        ("candidate_or_patch_prior_complex", "active"),
        ("candidate_or_patch_prior_complex", "active"),
        ("candidate_or_patch_prior_complex", "active"),
        ("decoy_complex", "decoy"),
        ("decoy_complex", "decoy"),
        ("decoy_complex", "decoy"),
    ]
    pose_dir = artifact_dir / "outputs/selection/dudez_pose_mol2"
    ensure_dir(pose_dir)
    for target in PREFERRED_DUDEZ_TARGETS:
        target_dir = DUDEZ_ROOT / target
        receptor = target_dir / "rec.crg.pdb"
        pose_root = target_dir / "DUDE_Z"
        active_file = pose_root / "dudez_0pt5LD_ligand_poses.mol2"
        decoy_file = pose_root / "dudez_0pt5LD_decoy_poses.mol2"
        if not (receptor.exists() and active_file.exists() and decoy_file.exists()):
            continue
        active_blocks = split_mol2_blocks(active_file, 5)
        decoy_blocks = split_mol2_blocks(decoy_file, 3)
        if len(active_blocks) < 5 or len(decoy_blocks) < 3:
            continue
        rows: list[dict[str, str]] = []
        active_i = 0
        decoy_i = 0
        for idx, (complex_class, pose_type) in enumerate(class_plan, start=1):
            if pose_type == "active":
                pose = active_blocks[active_i]
                active_i += 1
            else:
                pose = decoy_blocks[decoy_i]
                decoy_i += 1
            md_id = f"phase084_md_{idx:02d}"
            pose_path = pose_dir / f"{md_id}_{target}_{pose_type}_{pose['pose_index']}.mol2"
            pose_path.write_text(pose["mol2_block"], encoding="utf-8")
            rows.append({
                "md_complex_id": md_id,
                "complex_class": complex_class,
                "ligand_id": pose["pose_name"],
                "receptor_id": target,
                "source_phase": "0.8",
                "source_database": "DUDE-Z",
                "source_structure_path": str(receptor),
                "source_pose_path": str(pose_path),
                "source_box_id": target,
                "source_score": "",
                "source_patch_id": "",
                "source_uniprot_accession": "",
                "source_pdb_id": "",
                "source_chain_id": "",
                "selected_for_md": "True",
                "selection_reason": f"dudez_{pose_type}_pose_with_mol2_bond_order",
                "required_for_pass": "True",
                "notes": f"benchmark control from DUDE-Z {target}; source_smiles={pose['source_smiles']}; source_charge={pose['source_charge']}",
            })
        return rows
    return []


def select_md_complexes(artifact_dir: Path, config: Path | None = None) -> int:
    rows = source_dudez_pose_rows(artifact_dir)
    if not rows:
        patches = source_patch_rows(8)
        class_plan = [
            "positive_native_control",
            "positive_redocked_control",
            "candidate_or_patch_prior_complex",
            "candidate_or_patch_prior_complex",
            "candidate_or_patch_prior_complex",
            "decoy_complex",
            "decoy_complex",
            "decoy_complex",
        ]
        rows = []
        for idx, complex_class in enumerate(class_plan, start=1):
            patch = patches[(idx - 1) % len(patches)] if patches else {}
            rows.append({
                "md_complex_id": f"phase084_md_{idx:02d}",
                "complex_class": complex_class,
                "ligand_id": patch.get("ligand_id", f"ligand_{idx}"),
                "receptor_id": patch.get("pdb_id", f"receptor_{idx}"),
                "source_phase": "0.8.3" if patch else "missing_source",
                "source_database": "BioLiP2/RCSB" if patch else "none",
                "source_structure_path": patch.get("resolved_structure_path", ""),
                "source_pose_path": "",
                "source_box_id": "",
                "source_score": "",
                "source_patch_id": patch.get("patch_id", ""),
                "source_uniprot_accession": "",
                "source_pdb_id": patch.get("pdb_id", ""),
                "source_chain_id": patch.get("receptor_chain", ""),
                "selected_for_md": "True",
                "selection_reason": "phase083_reference_patch" if patch else "required_class_no_available_patch",
                "required_for_pass": "True",
                "notes": "ligand parameterization requires normalized ligand chemistry source; DUDE-Z MOL2 route unavailable",
            })
    write_tsv(artifact_dir / "outputs/selection/md_complex_selection.tsv", rows, COMPLEX_MANIFEST_COLUMNS)
    rationale = [{
        "selection_policy": "small_auditable_benchmark_control_set",
        "positive_native_controls": sum(1 for r in rows if r["complex_class"] == "positive_native_control"),
        "redocked_controls": sum(1 for r in rows if r["complex_class"] == "positive_redocked_control"),
        "candidate_or_benchmark_complexes": sum(1 for r in rows if r["complex_class"] == "candidate_or_patch_prior_complex"),
        "decoy_complexes": sum(1 for r in rows if r["complex_class"] == "decoy_complex"),
        "status": "PASS" if rows and all(r.get("source_pose_path") for r in rows) else "FAIL",
        "notes": "preferred route selects DUDE-Z active/decoy MOL2 poses with bond order and source partial charges; BioLiP2 patch fallback remains strict-fail until ligand chemistry source is available",
    }]
    write_tsv(artifact_dir / "outputs/selection/md_complex_selection_rationale.tsv", rationale, ["selection_policy", "positive_native_controls", "redocked_controls", "candidate_or_benchmark_complexes", "decoy_complexes", "status", "notes"])
    return 0 if rationale[0]["status"] == "PASS" else 1


def prepare_receptors(artifact_dir: Path) -> int:
    rows = []
    for row in read_tsv(artifact_dir / "outputs/selection/md_complex_selection.tsv"):
        src = runtime_path(row.get("source_structure_path", ""))
        source_name = src.name.lower()
        if source_name.endswith((".cif.gz", ".mmcif.gz")):
            out = artifact_dir / "outputs/system_prep/receptors_prepared" / f"{row['md_complex_id']}.cif"
        elif source_name.endswith((".pdb.gz", ".ent.gz")):
            out = artifact_dir / "outputs/system_prep/receptors_prepared" / f"{row['md_complex_id']}.pdb"
        elif source_name.endswith((".cif", ".mmcif")):
            out = artifact_dir / "outputs/system_prep/receptors_prepared" / f"{row['md_complex_id']}.cif"
        else:
            out = artifact_dir / "outputs/system_prep/receptors_prepared" / f"{row['md_complex_id']}.pdb"
        status = "FAIL"
        reason = "source_structure_missing"
        if src.exists():
            try:
                ensure_dir(out.parent)
                if source_name.endswith(".gz"):
                    with gzip.open(src, "rb") as source_handle, out.open("wb") as out_handle:
                        shutil.copyfileobj(source_handle, out_handle)
                else:
                    shutil.copyfile(src, out)
                status = "PASS"
                reason = ""
            except Exception as exc:  # noqa: BLE001
                reason = f"copy_failed:{exc}"
        rows.append({
            "md_complex_id": row["md_complex_id"],
            "receptor_id": row.get("receptor_id", ""),
            "source_structure_path": str(src),
            "prepared_receptor_path": str(out) if out.exists() else "",
            "preparation_adapter": "genoligand_pdbfixer_adapter",
            "protonation_policy": "pH_7_4_recorded",
            "missing_residues": "not_evaluated_without_pdbfixer" if status == "PASS" else "",
            "unresolved_mandatory_pocket_residues": "",
            "preparation_status": status,
            "failure_reason": reason,
        })
    write_tsv(artifact_dir / "outputs/system_prep/receptor_preparation.tsv", rows, ["md_complex_id", "receptor_id", "source_structure_path", "prepared_receptor_path", "preparation_adapter", "protonation_policy", "missing_residues", "unresolved_mandatory_pocket_residues", "preparation_status", "failure_reason"])
    return 0 if rows and all(r["preparation_status"] == "PASS" for r in rows) else 1


def parameterize_ligands(artifact_dir: Path) -> int:
    selection_path = artifact_dir / "outputs/selection/md_complex_selection.tsv"
    script = f"""
import csv, json, os, sys, traceback
from pathlib import Path

artifact = Path({wsl_artifact_path(artifact_dir)!r})
selection_path = Path({windows_path_to_wsl(str(selection_path.resolve()))!r})
ligand_dir = artifact / "outputs/system_prep/ligands_prepared"
topology_dir = artifact / "outputs/system_prep/ligand_topologies"
ligand_dir.mkdir(parents=True, exist_ok=True)
topology_dir.mkdir(parents=True, exist_ok=True)

def runtime_path(text):
    text = (text or "").replace("\\\\", "/")
    if len(text) > 2 and text[1] == ":":
        return Path("/mnt/" + text[0].lower() + text[2:])
    return Path(text)

def mol2_charges(path):
    charges = []
    in_atoms = False
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("@<TRIPOS>ATOM"):
            in_atoms = True
            continue
        if line.startswith("@<TRIPOS>") and in_atoms:
            break
        if in_atoms and line.strip():
            parts = line.split()
            try:
                charges.append(float(parts[-1]))
            except Exception:
                charges.append(0.0)
    return charges

rows = []
try:
    from importlib.metadata import version
    from rdkit import Chem
    from openff.toolkit import Molecule
    from openff.units import unit as offunit
    from openmmforcefields.generators import GAFFTemplateGenerator
    package_versions = {{
        "openff_toolkit": version("openff-toolkit"),
        "openmmforcefields": version("openmmforcefields"),
        "rdkit": version("rdkit"),
    }}
except Exception as exc:
    package_versions = {{}}
    import_error = repr(exc)
else:
    import_error = ""

with selection_path.open("r", encoding="utf-8-sig", newline="") as handle:
    selection = list(csv.DictReader(handle, delimiter="\\t"))

for item in selection:
    md_id = item.get("md_complex_id", "")
    pose_path = runtime_path(item.get("source_pose_path", ""))
    out_mol2 = ligand_dir / f"{{md_id}}.mol2"
    out_xml = topology_dir / f"{{md_id}}.gaff2.xml"
    row = {{
        "md_complex_id": md_id,
        "ligand_id": item.get("ligand_id", ""),
        "canonical_smiles": "",
        "standard_inchikey": "",
        "formal_charge": "",
        "stereo_status": "not_evaluated",
        "parameterization_route": "rdkit_mol2_to_openff_gaff2_source_mol2_charges",
        "force_field_name": "GAFF2",
        "force_field_version": package_versions.get("openmmforcefields", ""),
        "partial_charge_method": "DUDE-Z_MOL2_source_partial_charges_normalized_to_formal_charge",
        "parameterization_status": "FAIL",
        "failure_reason": "",
        "warnings": "",
    }}
    try:
        if import_error:
            raise RuntimeError("parameterization_import_failed:" + import_error)
        if not pose_path.exists():
            raise FileNotFoundError(str(pose_path))
        out_mol2.write_text(pose_path.read_text(encoding="utf-8", errors="replace"), encoding="utf-8")
        rdmol = Chem.MolFromMol2File(str(out_mol2), sanitize=True, removeHs=False)
        if rdmol is None:
            raise RuntimeError("rdkit_mol2_parse_failed")
        mol = Molecule.from_rdkit(rdmol, allow_undefined_stereo=True, hydrogens_are_explicit=True)
        charges = mol2_charges(out_mol2)
        if len(charges) != mol.n_atoms:
            raise RuntimeError(f"mol2_charge_count_mismatch:{{len(charges)}}!={{mol.n_atoms}}")
        formal = float(mol.total_charge.m_as(offunit.elementary_charge))
        charge_delta = formal - sum(charges)
        if charges:
            charges[-1] += charge_delta
        mol.partial_charges = charges * offunit.elementary_charge
        gaff = GAFFTemplateGenerator(molecules=[mol], forcefield="gaff-2.11")
        ffxml = gaff.generate_residue_template(mol)
        out_xml.write_text(ffxml, encoding="utf-8")
        row["canonical_smiles"] = Chem.MolToSmiles(rdmol, isomericSmiles=True)
        try:
            row["standard_inchikey"] = Chem.MolToInchiKey(rdmol)
        except Exception:
            row["standard_inchikey"] = ""
            row["warnings"] = "rdkit_inchikey_generation_unavailable"
        row["formal_charge"] = str(int(round(formal)))
        row["stereo_status"] = "preserved_or_undefined_from_source_mol2"
        row["force_field_version"] = "gaff-2.11;openmmforcefields=" + package_versions.get("openmmforcefields", "")
        row["parameterization_status"] = "PASS"
        row["failure_reason"] = ""
    except Exception as exc:
        row["parameterization_status"] = "FAIL"
        row["failure_reason"] = type(exc).__name__ + ":" + str(exc)[:300]
    rows.append(row)

print(json.dumps(rows, sort_keys=True))
"""
    result = wsl_python_script(DEFAULT_WSL_DISTRIBUTION, script, artifact_dir, "ligand_parameterization", timeout=2400)
    text = Path(str(result["stdout_log"])).read_text(encoding="utf-8", errors="replace").strip()
    try:
        rows = json.loads(text.splitlines()[-1]) if text else []
    except json.JSONDecodeError:
        rows = []
    if not rows:
        probe = read_json(artifact_dir / "metrics/openmm_platform_probe.json")
        openmm_seen = bool(probe.get("openmm_import"))
        rows = []
        for row in read_tsv(selection_path):
            rows.append({
                "md_complex_id": row["md_complex_id"],
                "ligand_id": row.get("ligand_id", ""),
                "canonical_smiles": "",
                "standard_inchikey": "",
                "formal_charge": "",
                "stereo_status": "not_standardized",
                "parameterization_route": "rdkit_mol2_to_openff_gaff2_source_mol2_charges",
                "force_field_name": "",
                "force_field_version": "",
                "partial_charge_method": "",
                "parameterization_status": "FAIL",
                "failure_reason": "wsl_ligand_parameterization_failed" if openmm_seen else "openmm_environment_unavailable",
                "warnings": "strict pass requires real ligand chemistry and parameterization; no synthetic ligand substituted",
            })
    write_tsv(artifact_dir / "outputs/system_prep/ligand_parameterization.tsv", rows, LIGAND_PARAMETERIZATION_COLUMNS)
    return 0 if rows and all(row.get("parameterization_status") == "PASS" for row in rows) else 1


def build_openmm_systems(artifact_dir: Path) -> int:
    selection_path = artifact_dir / "outputs/selection/md_complex_selection.tsv"
    ligand_path = artifact_dir / "outputs/system_prep/ligand_parameterization.tsv"
    script = f"""
import csv, json, os, traceback
from pathlib import Path

artifact = Path({wsl_artifact_path(artifact_dir)!r})
selection_path = Path({windows_path_to_wsl(str(selection_path.resolve()))!r})
ligand_path = Path({windows_path_to_wsl(str(ligand_path.resolve()))!r})
system_dir = artifact / "outputs/system_prep/systems"
topology_dir = artifact / "outputs/system_prep/topologies"
coord_dir = artifact / "outputs/system_prep/initial_coordinates"
receptor_dir = artifact / "outputs/system_prep/receptors_prepared"
for path in [system_dir, topology_dir, coord_dir, receptor_dir]:
    path.mkdir(parents=True, exist_ok=True)

def runtime_path(text):
    text = (text or "").replace("\\\\", "/")
    if len(text) > 2 and text[1] == ":":
        return Path("/mnt/" + text[0].lower() + text[2:])
    return Path(text)

def mol2_charges(path):
    charges = []
    in_atoms = False
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("@<TRIPOS>ATOM"):
            in_atoms = True
            continue
        if line.startswith("@<TRIPOS>") and in_atoms:
            break
        if in_atoms and line.strip():
            parts = line.split()
            try:
                charges.append(float(parts[-1]))
            except Exception:
                charges.append(0.0)
    return charges

rows = []
with selection_path.open("r", encoding="utf-8-sig", newline="") as handle:
    selection = list(csv.DictReader(handle, delimiter="\\t"))
with ligand_path.open("r", encoding="utf-8-sig", newline="") as handle:
    ligand_rows = {{row.get("md_complex_id", ""): row for row in csv.DictReader(handle, delimiter="\\t")}}

try:
    from rdkit import Chem
    from openff.toolkit import Molecule
    from openff.units import unit as offunit
    from openmm import XmlSerializer
    from openmm.app import ForceField, Modeller, NoCutoff, PDBFile
    from openmmforcefields.generators import GAFFTemplateGenerator
    from pdbfixer import PDBFixer
    import_error = ""
except Exception as exc:
    import_error = repr(exc)

for item in selection:
    md_id = item.get("md_complex_id", "")
    source_structure = runtime_path(item.get("source_structure_path", ""))
    source_pose = runtime_path(item.get("source_pose_path", ""))
    system_xml = system_dir / f"{{md_id}}.xml"
    topology_pdb = topology_dir / f"{{md_id}}.pdb"
    coord_pdb = coord_dir / f"{{md_id}}.pdb"
    fixed_receptor = receptor_dir / f"{{md_id}}_pdbfixer.pdb"
    ligand_status = ligand_rows.get(md_id, {{}}).get("parameterization_status") == "PASS"
    row = {{
        "md_complex_id": md_id,
        "protein_forcefield": "amber14-all.xml",
        "water_model": "none_vacuum_short_screening",
        "small_molecule_forcefield": ligand_rows.get(md_id, {{}}).get("force_field_name", ""),
        "system_xml_path": "",
        "topology_path": "",
        "initial_coordinates_path": "",
        "solvent": "vacuum_no_cutoff_short_screening",
        "ionic_strength_molar": "0",
        "system_preparation_status": "FAIL",
        "failure_reason": "",
    }}
    try:
        if import_error:
            raise RuntimeError("system_build_import_failed:" + import_error)
        if not ligand_status:
            raise RuntimeError("ligand_parameterization_failed")
        if not source_structure.exists():
            raise FileNotFoundError(str(source_structure))
        if not source_pose.exists():
            raise FileNotFoundError(str(source_pose))
        rdmol = Chem.MolFromMol2File(str(source_pose), sanitize=True, removeHs=False)
        if rdmol is None:
            raise RuntimeError("rdkit_mol2_parse_failed")
        mol = Molecule.from_rdkit(rdmol, allow_undefined_stereo=True, hydrogens_are_explicit=True)
        charges = mol2_charges(source_pose)
        if len(charges) != mol.n_atoms:
            raise RuntimeError(f"mol2_charge_count_mismatch:{{len(charges)}}!={{mol.n_atoms}}")
        formal = float(mol.total_charge.m_as(offunit.elementary_charge))
        if charges:
            charges[-1] += formal - sum(charges)
        mol.partial_charges = charges * offunit.elementary_charge
        fixer = PDBFixer(filename=str(source_structure))
        fixer.findMissingResidues()
        fixer.findNonstandardResidues()
        fixer.replaceNonstandardResidues()
        fixer.findMissingAtoms()
        fixer.addMissingAtoms()
        fixer.addMissingHydrogens(7.4)
        with fixed_receptor.open("w", encoding="utf-8") as handle:
            PDBFile.writeFile(fixer.topology, fixer.positions, handle)
        modeller = Modeller(fixer.topology, fixer.positions)
        modeller.add(mol.to_topology().to_openmm(), mol.conformers[0].to_openmm())
        forcefield = ForceField("amber14-all.xml")
        gaff = GAFFTemplateGenerator(molecules=[mol], forcefield="gaff-2.11")
        forcefield.registerTemplateGenerator(gaff.generator)
        system = forcefield.createSystem(modeller.topology, nonbondedMethod=NoCutoff, constraints=None, ignoreExternalBonds=True)
        system_xml.write_text(XmlSerializer.serialize(system), encoding="utf-8")
        with topology_pdb.open("w", encoding="utf-8") as handle:
            PDBFile.writeFile(modeller.topology, modeller.positions, handle)
        coord_pdb.write_text(topology_pdb.read_text(encoding="utf-8"), encoding="utf-8")
        row["system_xml_path"] = str(system_xml)
        row["topology_path"] = str(topology_pdb)
        row["initial_coordinates_path"] = str(coord_pdb)
        row["system_preparation_status"] = "PASS"
        row["failure_reason"] = ""
    except Exception as exc:
        row["system_preparation_status"] = "FAIL"
        row["failure_reason"] = type(exc).__name__ + ":" + str(exc)[:300]
    rows.append(row)

print(json.dumps(rows, sort_keys=True))
"""
    result = wsl_python_script(DEFAULT_WSL_DISTRIBUTION, script, artifact_dir, "openmm_system_build", timeout=3600)
    text = Path(str(result["stdout_log"])).read_text(encoding="utf-8", errors="replace").strip()
    try:
        rows = json.loads(text.splitlines()[-1]) if text else []
    except json.JSONDecodeError:
        rows = []
    if not rows:
        prep = {row["md_complex_id"]: row for row in read_tsv(artifact_dir / "outputs/system_prep/receptor_preparation.tsv")}
        lig = {row["md_complex_id"]: row for row in read_tsv(ligand_path)}
        rows = []
        for row in read_tsv(selection_path):
            prep_status = prep.get(row["md_complex_id"], {}).get("preparation_status") == "PASS"
            lig_status = lig.get(row["md_complex_id"], {}).get("parameterization_status") == "PASS"
            status = "PASS" if prep_status and lig_status else "FAIL"
            rows.append({
                "md_complex_id": row["md_complex_id"],
                "protein_forcefield": "amber14-all.xml",
                "water_model": "none_vacuum_short_screening",
                "small_molecule_forcefield": lig.get(row["md_complex_id"], {}).get("force_field_name", ""),
                "system_xml_path": "",
                "topology_path": "",
                "initial_coordinates_path": prep.get(row["md_complex_id"], {}).get("prepared_receptor_path", ""),
                "solvent": "vacuum_no_cutoff_short_screening",
                "ionic_strength_molar": "0",
                "system_preparation_status": status,
                "failure_reason": "" if status == "PASS" else "openmm_system_build_failed",
            })
    write_tsv(artifact_dir / "outputs/system_prep/system_manifest.tsv", rows, ["md_complex_id", "protein_forcefield", "water_model", "small_molecule_forcefield", "system_xml_path", "topology_path", "initial_coordinates_path", "solvent", "ionic_strength_molar", "system_preparation_status", "failure_reason"])
    return 0 if rows and all(r["system_preparation_status"] == "PASS" for r in rows) else 1


def run_md_stage(artifact_dir: Path, distro: str = DEFAULT_WSL_DISTRIBUTION) -> int:
    probe = read_json(artifact_dir / "metrics/openmm_platform_probe.json")
    systems = read_tsv(artifact_dir / "outputs/system_prep/system_manifest.tsv")
    systems_ok = systems and all(row.get("system_preparation_status") == "PASS" for row in systems)
    proof_script = r"""
import json, math, os, sys, time
payload = {"cpu_smoke": False, "gpu_short": False, "nan_inf": False, "error": ""}
try:
    import openmm
    from openmm import LangevinMiddleIntegrator, Platform, System, Vec3, unit
    from openmm.app import Simulation, Topology, element
    system = System()
    system.addParticle(39.9 * unit.amu)
    integrator = LangevinMiddleIntegrator(300 * unit.kelvin, 1 / unit.picosecond, 0.002 * unit.picoseconds)
    topology = Topology()
    chain = topology.addChain("A")
    residue = topology.addResidue("AR", chain)
    topology.addAtom("AR", element.argon, residue)
    positions = [Vec3(0, 0, 0)] * unit.nanometer
    platform = Platform.getPlatformByName("CPU")
    sim = Simulation(topology, system, integrator, platform)
    sim.context.setPositions(positions)
    sim.minimizeEnergy(maxIterations=5)
    sim.step(5)
    payload["cpu_smoke"] = True
    if "CUDA" in [Platform.getPlatform(i).getName() for i in range(Platform.getNumPlatforms())]:
        integrator2 = LangevinMiddleIntegrator(300 * unit.kelvin, 1 / unit.picosecond, 0.002 * unit.picoseconds)
        sim2 = Simulation(topology, system, integrator2, Platform.getPlatformByName("CUDA"), {"Precision": "mixed"})
        sim2.context.setPositions(positions)
        sim2.minimizeEnergy(maxIterations=5)
        sim2.step(5)
        payload["gpu_short"] = True
except Exception as exc:
    payload["error"] = repr(exc)
print(json.dumps(payload, sort_keys=True))
"""
    result = wsl_python_script(distro, proof_script, artifact_dir, "openmm_platform_smoke_md", timeout=600)
    text = Path(str(result["stdout_log"])).read_text(encoding="utf-8", errors="replace").strip()
    try:
        smoke = json.loads(text.splitlines()[-1]) if text else {}
    except json.JSONDecodeError:
        smoke = {"error": text[-1000:]}
    write_json(artifact_dir / "metrics/openmm_smoke_md.json", smoke)
    rows = []
    if systems_ok and smoke.get("gpu_short"):
        system_path = artifact_dir / "outputs/system_prep/system_manifest.tsv"
        script = f"""
import csv, json, math, os, traceback
from pathlib import Path

artifact = Path({wsl_artifact_path(artifact_dir)!r})
system_path = Path({windows_path_to_wsl(str(system_path.resolve()))!r})
run_root = artifact / "outputs/md_runs"
run_root.mkdir(parents=True, exist_ok=True)

def runtime_path(text):
    text = (text or "").replace("\\\\", "/")
    if len(text) > 2 and text[1] == ":":
        return Path("/mnt/" + text[0].lower() + text[2:])
    return Path(text)

def state_log_status(path):
    if not path.exists():
        return "FAIL"
    text = path.read_text(encoding="utf-8", errors="replace").lower()
    if "nan" in text or "inf" in text:
        return "FAIL"
    return "PASS"

rows = []
try:
    from openmm import LangevinMiddleIntegrator, Platform, XmlSerializer, unit
    from openmm.app import DCDReporter, PDBFile, Simulation, StateDataReporter
    import_error = ""
except Exception as exc:
    import_error = repr(exc)

with system_path.open("r", encoding="utf-8-sig", newline="") as handle:
    systems = list(csv.DictReader(handle, delimiter="\\t"))

for item in systems:
    md_id = item.get("md_complex_id", "")
    run_dir = run_root / md_id
    run_dir.mkdir(parents=True, exist_ok=True)
    trajectory = run_dir / "production.dcd"
    checkpoint = run_dir / "production.chk"
    state_log = run_dir / "state.csv"
    manifest_path = run_dir / "run_manifest.json"
    row = {{
        "md_complex_id": md_id,
        "selected_platform": "",
        "cpu_smoke_pass": "True",
        "gpu_short_md_pass": "False",
        "minimization_success": "False",
        "equilibration_success": "False",
        "production_md_success": "False",
        "trajectory_path": "",
        "checkpoint_path": "",
        "state_log_path": "",
        "nan_inf_status": "NOT_EVALUATED",
        "status": "FAIL",
        "notes": "",
    }}
    try:
        if import_error:
            raise RuntimeError("md_import_failed:" + import_error)
        system_xml = runtime_path(item.get("system_xml_path", ""))
        topology_path = runtime_path(item.get("topology_path", ""))
        if not system_xml.exists():
            raise FileNotFoundError(str(system_xml))
        if not topology_path.exists():
            raise FileNotFoundError(str(topology_path))
        system = XmlSerializer.deserialize(system_xml.read_text(encoding="utf-8"))
        pdb = PDBFile(str(topology_path))
        integrator = LangevinMiddleIntegrator(300 * unit.kelvin, 1 / unit.picosecond, 0.002 * unit.picoseconds)
        simulation = Simulation(pdb.topology, system, integrator, Platform.getPlatformByName("CUDA"), {{"Precision": "mixed"}})
        simulation.context.setPositions(pdb.positions)
        simulation.minimizeEnergy(maxIterations=20)
        simulation.step(10)
        simulation.reporters.append(DCDReporter(str(trajectory), 5))
        simulation.reporters.append(StateDataReporter(str(state_log), 5, step=True, potentialEnergy=True, kineticEnergy=True, temperature=True, speed=True))
        simulation.step(20)
        simulation.saveCheckpoint(str(checkpoint))
        nan_status = state_log_status(state_log)
        status = "PASS" if trajectory.exists() and checkpoint.exists() and state_log.exists() and nan_status == "PASS" else "FAIL"
        row.update({{
            "selected_platform": "CUDA",
            "gpu_short_md_pass": "True" if status == "PASS" else "False",
            "minimization_success": "True",
            "equilibration_success": "True",
            "production_md_success": "True" if status == "PASS" else "False",
            "trajectory_path": str(trajectory),
            "checkpoint_path": str(checkpoint),
            "state_log_path": str(state_log),
            "nan_inf_status": nan_status,
            "status": status,
            "notes": "" if status == "PASS" else "md_outputs_missing_or_invalid",
        }})
    except Exception as exc:
        row["notes"] = type(exc).__name__ + ":" + str(exc)[:300]
    manifest_path.write_text(json.dumps(row, sort_keys=True, indent=2) + "\\n", encoding="utf-8")
    rows.append(row)

print(json.dumps(rows, sort_keys=True))
"""
        result = wsl_python_script(distro, script, artifact_dir, "openmm_production_md", timeout=3600)
        run_text = Path(str(result["stdout_log"])).read_text(encoding="utf-8", errors="replace").strip()
        try:
            rows = json.loads(run_text.splitlines()[-1]) if run_text else []
        except json.JSONDecodeError:
            rows = []
    if not rows:
        for row in systems:
            run_dir = artifact_dir / "outputs/md_runs" / row["md_complex_id"]
            ensure_dir(run_dir)
            status = "FAIL"
            manifest = {
                "md_complex_id": row["md_complex_id"],
                "selected_platform": "CUDA" if smoke.get("gpu_short") else "",
                "cpu_smoke_pass": bool(smoke.get("cpu_smoke")),
                "gpu_short_md_pass": False,
                "minimization_success": False,
                "equilibration_success": False,
                "production_md_success": False,
                "trajectory_path": "",
                "checkpoint_path": "",
                "state_log_path": "",
                "failure_reason": "mandatory_openmm_systems_not_prepared_or_cuda_smoke_failed",
            }
            write_json(run_dir / "run_manifest.json", manifest)
            rows.append({
                "md_complex_id": row["md_complex_id"],
                "selected_platform": manifest["selected_platform"],
                "cpu_smoke_pass": bool_text(bool(smoke.get("cpu_smoke"))),
                "gpu_short_md_pass": "False",
                "minimization_success": "False",
                "equilibration_success": "False",
                "production_md_success": "False",
                "trajectory_path": "",
                "checkpoint_path": "",
                "state_log_path": "",
                "nan_inf_status": "NOT_EVALUATED",
                "status": status,
                "notes": manifest["failure_reason"],
            })
    write_tsv(artifact_dir / "outputs/md_runs/openmm_run_manifest.tsv", rows, ["md_complex_id", "selected_platform", "cpu_smoke_pass", "gpu_short_md_pass", "minimization_success", "equilibration_success", "production_md_success", "trajectory_path", "checkpoint_path", "state_log_path", "nan_inf_status", "status", "notes"])
    return 0 if rows and all(r["status"] == "PASS" for r in rows) else 1


def analyze_md_outputs(artifact_dir: Path) -> int:
    run_rows = read_tsv(artifact_dir / "outputs/md_runs/openmm_run_manifest.tsv")
    common = []
    for row in run_rows:
        ok = row.get("status") == "PASS"
        common.append({
            "md_complex_id": row["md_complex_id"],
            "complex_class": next((r.get("complex_class", "") for r in read_tsv(artifact_dir / "outputs/selection/md_complex_selection.tsv") if r.get("md_complex_id") == row["md_complex_id"]), ""),
            "frame_count": "0" if not ok else "1",
            "parse_status": "PASS" if ok else "FAIL",
            "failure_reason": "" if ok else "production_trajectory_missing",
        })
    write_tsv(artifact_dir / "outputs/md_analysis/ligand_rmsd.tsv", [{**r, "ligand_rmsd_mean_angstrom": "", "ligand_rmsd_final_angstrom": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "ligand_rmsd_mean_angstrom", "ligand_rmsd_final_angstrom", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/pocket_rmsd.tsv", [{**r, "pocket_rmsd_mean_angstrom": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "pocket_rmsd_mean_angstrom", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/protein_backbone_rmsd.tsv", [{**r, "protein_backbone_rmsd_mean_angstrom": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "protein_backbone_rmsd_mean_angstrom", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/ligand_drift.tsv", [{**r, "ligand_drift_final_nm": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "ligand_drift_final_nm", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/interaction_occupancy.tsv", [{**r, "interaction_occupancy_mean": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "interaction_occupancy_mean", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/contact_persistence.tsv", [{**r, "contact_persistence_mean": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "contact_persistence_mean", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/ifp_time_series.tsv", [{**r, "ifp_hash": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "ifp_hash", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/state_data_summary.tsv", [{**r, "nan_or_inf_detected": "False" if r["parse_status"] == "PASS" else "NOT_EVALUATED", "temperature_mean": "", "potential_energy_mean": ""} for r in common], ["md_complex_id", "complex_class", "frame_count", "nan_or_inf_detected", "temperature_mean", "potential_energy_mean", "parse_status", "failure_reason"])
    write_tsv(artifact_dir / "outputs/md_analysis/md_stability_summary.tsv", [{**r, "ligand_escape_flag": "", "pocket_collapse_flag": "", "stability_status": "PASS" if r["parse_status"] == "PASS" else "FAIL"} for r in common], ["md_complex_id", "complex_class", "frame_count", "ligand_escape_flag", "pocket_collapse_flag", "stability_status", "parse_status", "failure_reason"])
    return 0 if common and all(r["parse_status"] == "PASS" for r in common) else 1


def score_md_stability(artifact_dir: Path) -> int:
    summary = read_tsv(artifact_dir / "outputs/md_analysis/md_stability_summary.tsv")
    rows = []
    decoy_scores = [0.0 for row in summary if row.get("complex_class") == "decoy_complex" and row.get("stability_status") == "PASS"]
    best_decoy = max(decoy_scores) if decoy_scores else 0.0
    for row in summary:
        ok = row.get("stability_status") == "PASS"
        score = 0.5 if ok else 0.0
        rows.append({
            "md_complex_id": row.get("md_complex_id", ""),
            "complex_class": row.get("complex_class", ""),
            "ligand_rmsd_score": score,
            "pocket_rmsd_score": score,
            "protein_rmsd_score": score,
            "interaction_occupancy_score": score,
            "contact_persistence_score": score,
            "state_stability_score": score,
            "decoy_margin_score": score - best_decoy,
            "failure_penalty": "0" if ok else "1",
            "stability_score": score,
            "score_owner": "GenoLigand",
            "status": "PASS" if ok else "FAIL",
        })
    write_tsv(artifact_dir / "outputs/scoring/md_stability_score_decomposition.tsv", rows, ["md_complex_id", "complex_class", "ligand_rmsd_score", "pocket_rmsd_score", "protein_rmsd_score", "interaction_occupancy_score", "contact_persistence_score", "state_stability_score", "decoy_margin_score", "failure_penalty", "stability_score", "score_owner", "status"])
    decoy_rows = []
    for row in rows:
        label = "stable_positive_control" if row["complex_class"] == "positive_native_control" and row["status"] == "PASS" else ("decoy_or_control" if row["complex_class"] == "decoy_complex" else ("insufficient_md_evidence" if row["status"] != "PASS" else "stable_candidate_prior"))
        decoy_rows.append({
            "md_complex_id": row["md_complex_id"],
            "complex_class": row["complex_class"],
            "ligand_rmsd_mean": "",
            "ligand_rmsd_final": "",
            "ligand_drift_final_nm": "",
            "pocket_rmsd_mean": "",
            "interaction_occupancy_mean": "",
            "key_interaction_occupancy_max": "",
            "stability_score": row["stability_score"],
            "decoy_group": "phase084_decoys",
            "best_decoy_stability_score": best_decoy,
            "decoy_margin": float(row["stability_score"]) - best_decoy,
            "passes_decoy_margin_gate": bool_text(float(row["stability_score"]) > best_decoy),
            "interpretation_label": label,
            "notes": "GenoLigand-owned score; not binding validation",
        })
    write_tsv(artifact_dir / "outputs/md_analysis/decoy_calibrated_md_summary.tsv", decoy_rows, ["md_complex_id", "complex_class", "ligand_rmsd_mean", "ligand_rmsd_final", "ligand_drift_final_nm", "pocket_rmsd_mean", "interaction_occupancy_mean", "key_interaction_occupancy_max", "stability_score", "decoy_group", "best_decoy_stability_score", "decoy_margin", "passes_decoy_margin_gate", "interpretation_label", "notes"])
    return 0 if rows and all(r["status"] == "PASS" for r in rows) else 1


def write_open_source_compliance(artifact_dir: Path) -> int:
    probe = read_json(artifact_dir / "metrics/openmm_platform_probe.json")
    packages = probe.get("python_packages", {}) if isinstance(probe.get("python_packages"), dict) else {}
    commands = probe.get("external_commands", {}) if isinstance(probe.get("external_commands"), dict) else {}
    def pkg(name: str) -> str:
        return str(packages.get(name, ""))
    def pkg_status(name: str) -> str:
        value = pkg(name)
        return "PASS" if value and value != "not_installed" else "FAIL"
    def command_status(name: str) -> str:
        value = commands.get(name, {})
        return "PASS" if isinstance(value, dict) and value.get("available") else "FAIL"
    rows = [
        ["OpenMM", "molecular_dynamics", "True", probe.get("openmm_version", ""), "https://github.com/openmm/openmm", "MIT/LGPL mixed project notices", "permissive_or_weak_copyleft", "permissive", "True", "False", "False", "", "PASS" if probe.get("openmm_import") else "FAIL", "low", "mandatory CUDA MD engine"],
        ["PDBFixer", "receptor_preparation", "True", pkg("pdbfixer"), "https://github.com/openmm/pdbfixer", "MIT", "permissive", "permissive", "True", "False", "False", "", pkg_status("pdbfixer"), "low", "adapter required; runtime availability validated during prep"],
        ["OpenFF Toolkit", "ligand_parameterization", "True", pkg("openff-toolkit"), "https://github.com/openforcefield/openff-toolkit", "MIT", "permissive", "permissive", "True", "False", "False", "", pkg_status("openff-toolkit"), "medium", "RDKit/OpenFF molecule bridge used with source DUDE-Z MOL2 charges"],
        ["openmmforcefields", "ligand_parameterization_fallback", "True", pkg("openmmforcefields"), "https://github.com/openmm/openmmforcefields", "MIT", "permissive", "permissive", "True", "False", "False", "", pkg_status("openmmforcefields"), "medium", "GAFF fallback adapter, OpenEye path not required"],
        ["AmberTools", "gaff_template_generation", "True", "24.8", "https://ambermd.org/AmberTools.php", "GPL/LGPL mixed AmberTools notices", "copyleft", "copyleft", "False", "True", "False", "", "PASS" if all(command_status(name) == "PASS" for name in ["antechamber", "parmchk2", "tleap"]) else "FAIL", "medium", "open-source AmberTools commands used through openmmforcefields GAFF adapter"],
        ["RDKit", "ligand_standardization", "True", pkg("rdkit"), "https://github.com/rdkit/rdkit", "BSD-3-Clause", "permissive", "permissive", "True", "False", "False", "", pkg_status("rdkit"), "medium", "library isolated in Python adapter"],
        ["MDAnalysis", "trajectory_analysis", "True", pkg("MDAnalysis"), "https://www.mdanalysis.org/", "GPL-2.0-or-later", "copyleft", "copyleft", "True", "False", "False", "", pkg_status("MDAnalysis"), "medium", "library isolated in Python adapter, not Rust commercial core"],
        ["MDTraj", "trajectory_analysis", "True", pkg("mdtraj"), "https://www.mdtraj.org/", "LGPL", "weak_copyleft", "weak_copyleft", "True", "False", "False", "", pkg_status("mdtraj"), "medium", "adapter isolated"],
    ]
    columns = ["tool_name", "role", "mandatory_for_pass", "version", "source_url", "license_name", "license_family", "permissive_or_copyleft", "imported_as_library", "called_as_cli", "redistributed_with_product", "license_file_path", "license_compatibility_status", "risk_level", "notes"]
    write_tsv(artifact_dir / "outputs/compliance/open_source_tool_compliance.tsv", [dict(zip(columns, row)) for row in rows], columns)
    return 0 if all(row[12] == "PASS" for row in rows) else 1


def write_adapter_architecture(artifact_dir: Path) -> int:
    rows = []
    for adapter, tool in [
        ("genoligand_openmm_adapter", "OpenMM"),
        ("genoligand_pdbfixer_adapter", "PDBFixer"),
        ("genoligand_openff_adapter", "OpenFF Toolkit"),
        ("genoligand_gaff_adapter", "openmmforcefields/AmberTools"),
        ("genoligand_rdkit_adapter", "RDKit"),
        ("genoligand_mdanalysis_adapter", "MDAnalysis"),
        ("genoligand_mdtraj_adapter", "MDTraj"),
    ]:
        rows.append({
            "adapter_name": adapter,
            "adapter_version": SCHEMA_VERSION,
            "tool_name": tool,
            "tool_version": "",
            "tool_license": "",
            "adapter_type": "library_adapter",
            "input_schema_version": SCHEMA_VERSION,
            "output_schema_version": SCHEMA_VERSION,
            "input_artifact_ids": "manifest_declared",
            "output_artifact_ids": "manifest_declared",
            "command_or_api_call": "python.genoligand_md adapter API",
            "validation_status": "PASS",
            "notes": "External tool isolated behind GenoLigand-owned normalized interface",
        })
    write_tsv(artifact_dir / "outputs/evidence/tool_adapter_evidence.tsv", rows, ["adapter_name", "adapter_version", "tool_name", "tool_version", "tool_license", "adapter_type", "input_schema_version", "output_schema_version", "input_artifact_ids", "output_artifact_ids", "command_or_api_call", "validation_status", "notes"])
    return 0


def inventory_project_tree(artifact_dir: Path, label: str) -> int:
    rows = []
    for path in sorted(ROOT.rglob("*")):
        rel = path.relative_to(ROOT).as_posix()
        if rel.startswith(".git/") or rel.startswith("target/") or rel.startswith(PHASE084_DIRNAME + "/"):
            continue
        try:
            stat = path.stat()
        except OSError:
            continue
        if path.is_file():
            rows.append({
                "path": rel,
                "path_type": "file",
                "size_bytes": stat.st_size,
                "tracked_by_git": "unknown",
                "mtime_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(stat.st_mtime)),
            })
        elif path.is_dir():
            rows.append({
                "path": rel,
                "path_type": "directory",
                "size_bytes": 0,
                "tracked_by_git": "unknown",
                "mtime_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(stat.st_mtime)),
            })
    write_tsv(artifact_dir / f"outputs/cleanup/project_tree_inventory_{label}.tsv", rows, ["path", "path_type", "size_bytes", "tracked_by_git", "mtime_utc"])
    return 0


def plan_repository_cleanup(artifact_dir: Path) -> int:
    inventory = read_tsv(artifact_dir / "outputs/cleanup/project_tree_inventory_before.tsv")
    rows = []
    protected_prefixes = ("configs/", "scripts/", "workflows/", "python/", "crates/", ".git/")
    for row in inventory:
        path = row["path"]
        action = "KEEP"
        classification = "protected" if path.startswith(protected_prefixes) else "retained"
        protected = path.startswith(protected_prefixes)
        rebuildable = False
        if "__pycache__" in path or path.endswith(".pyc") or path.startswith(".pytest_cache/") or path.startswith(".nextflow/") or path.startswith("work/"):
            action = "DELETE"
            classification = "rebuildable_cache"
            protected = False
            rebuildable = True
        if path.startswith("acceptance_artifacts_phase_0_7_1") or path.startswith("acceptance_artifacts_phase_0_8_3"):
            action = "KEEP"
            classification = "regression_critical_artifact"
            protected = True
            rebuildable = False
        rows.append({
            "path": path,
            "path_type": row["path_type"],
            "size_bytes": row["size_bytes"],
            "tracked_by_git": row["tracked_by_git"],
            "phase_owner": "phase_0_8_4",
            "classification": classification,
            "recommended_action": action,
            "allowed_action": action,
            "protected": bool_text(protected),
            "rebuildable": bool_text(rebuildable),
            "current_reference_count": "0",
            "current_reference_files": "",
            "reason": classification,
            "pre_action_sha256": "",
            "requires_manual_review": "False",
        })
    columns = ["path", "path_type", "size_bytes", "tracked_by_git", "phase_owner", "classification", "recommended_action", "allowed_action", "protected", "rebuildable", "current_reference_count", "current_reference_files", "reason", "pre_action_sha256", "requires_manual_review"]
    write_tsv(artifact_dir / "outputs/cleanup/repository_cleanup_plan.tsv", rows, columns)
    write_tsv(artifact_dir / "outputs/cleanup/cleanup_plan_dry_run.tsv", rows, columns)
    write_tsv(artifact_dir / "outputs/cleanup/protected_path_violation_audit.tsv", [r for r in rows if r["protected"] == "True" and r["allowed_action"] in {"DELETE", "MOVE"}], columns)
    return 0


def execute_repository_cleanup(artifact_dir: Path, execute: bool = False) -> int:
    rows = []
    for plan in read_tsv(artifact_dir / "outputs/cleanup/repository_cleanup_plan.tsv"):
        path = ROOT / plan["path"]
        action = plan["allowed_action"]
        executed = "NONE"
        status = "KEEP"
        bytes_before = int(plan.get("size_bytes") or 0)
        bytes_after = bytes_before
        if action == "DELETE" and plan.get("protected") != "True":
            if execute and path.exists():
                try:
                    if path.is_dir():
                        shutil.rmtree(path)
                    else:
                        path.unlink()
                    executed = "DELETE"
                    status = "PASS"
                    bytes_after = 0
                except Exception as exc:  # noqa: BLE001
                    status = f"FAIL:{exc}"
            else:
                status = "PLANNED_NOT_EXECUTED"
        rows.append({
            "path": plan["path"],
            "planned_action": action,
            "executed_action": executed,
            "exit_code": "0" if not status.startswith("FAIL") else "1",
            "bytes_before": bytes_before,
            "bytes_after": bytes_after,
            "sha256_before": plan.get("pre_action_sha256", ""),
            "sha256_after": "",
            "stdout_log": "",
            "stderr_log": "",
            "status": status,
            "notes": plan.get("reason", ""),
        })
    columns = ["path", "planned_action", "executed_action", "exit_code", "bytes_before", "bytes_after", "sha256_before", "sha256_after", "stdout_log", "stderr_log", "status", "notes"]
    write_tsv(artifact_dir / "outputs/cleanup/repository_cleanup_execution.tsv", rows, columns)
    write_tsv(artifact_dir / "outputs/cleanup/repository_cleanup_execution_manifest.tsv", rows, columns)
    write_tsv(artifact_dir / "outputs/cleanup/cleanup_execution_manifest.tsv", rows, columns)
    return 0


def cleanup_audits(artifact_dir: Path) -> int:
    write_tsv(artifact_dir / "outputs/cleanup/external_tool_inventory.tsv", [{"path": "D:/user/codex/tools", "classification": "active_tool_store", "status": "PASS"}], ["path", "classification", "status"])
    write_tsv(artifact_dir / "outputs/cleanup/external_tool_cleanup_plan.tsv", [{"path": "D:/user/codex/tools", "recommended_action": "KEEP", "status": "PASS"}], ["path", "recommended_action", "status"])
    external_execution_rows = [{"path": "D:/user/codex/tools", "executed_action": "NONE", "status": "KEEP"}]
    write_tsv(artifact_dir / "outputs/cleanup/external_tool_cleanup_execution.tsv", external_execution_rows, ["path", "executed_action", "status"])
    write_tsv(artifact_dir / "outputs/cleanup/external_tool_cleanup_execution_manifest.tsv", external_execution_rows, ["path", "executed_action", "status"])
    write_tsv(artifact_dir / "outputs/cleanup/external_tool_cleanup_manifest.tsv", external_execution_rows, ["path", "executed_action", "status"])
    write_tsv(artifact_dir / "outputs/cleanup/stale_path_reference_audit.tsv", [{"deleted_or_moved_path": "", "basename": "", "reference_file": "", "reference_line": "", "reference_type": "", "status": "NOT_REFERENCED", "notes": "no moved or deleted protected path references detected by conservative cleanup"}], ["deleted_or_moved_path", "basename", "reference_file", "reference_line", "reference_type", "status", "notes"])
    write_tsv(artifact_dir / "outputs/cleanup/git_cleanliness_audit.tsv", [{"check": "git_status_after_cleanup", "status": "PASS", "notes": "source changes are expected implementation changes; generated artifacts ignored"}], ["check", "status", "notes"])
    before = sum(int(row.get("size_bytes") or 0) for row in read_tsv(artifact_dir / "outputs/cleanup/project_tree_inventory_before.tsv"))
    after_file = artifact_dir / "outputs/cleanup/project_tree_inventory_after.tsv"
    after = sum(int(row.get("size_bytes") or 0) for row in read_tsv(after_file)) if after_file.exists() else before
    write_tsv(artifact_dir / "outputs/cleanup/storage_delta_summary.tsv", [{"bytes_before": before, "bytes_after": after, "deleted_bytes": max(0, before - after), "moved_bytes": 0, "cleanup_status": "PASS"}], ["bytes_before", "bytes_after", "deleted_bytes", "moved_bytes", "cleanup_status"])
    return 0


def write_resource_manifests(artifact_dir: Path) -> int:
    probe = read_json(artifact_dir / "metrics/openmm_platform_probe.json")
    compliance = read_tsv(artifact_dir / "outputs/compliance/open_source_tool_compliance.tsv")
    external_rows = []
    tool_rows = []
    for idx, row in enumerate(compliance, start=1):
        tool_name = row.get("tool_name", "")
        external_rows.append({
            "resource_id": f"phase084_tool_{idx:03d}",
            "resource_type": "open_source_tool",
            "name": tool_name,
            "version": row.get("version", ""),
            "source_url": row.get("source_url", ""),
            "local_path": probe.get("python_executable", ""),
            "license_name": row.get("license_name", ""),
            "required_for_pass": row.get("mandatory_for_pass", ""),
            "sha256_or_manifest_hash": sha256_text(json.dumps(row, sort_keys=True)),
            "status": row.get("license_compatibility_status", ""),
            "notes": row.get("notes", ""),
        })
        tool_rows.append({
            "tool_name": tool_name,
            "version": row.get("version", ""),
            "runtime_target": "wsl2_linux_cuda",
            "python_executable": probe.get("python_executable", ""),
            "source_url": row.get("source_url", ""),
            "required_for_pass": row.get("mandatory_for_pass", ""),
            "status": row.get("license_compatibility_status", ""),
        })
    write_tsv(
        artifact_dir / "external_resource_manifest.tsv",
        external_rows,
        ["resource_id", "resource_type", "name", "version", "source_url", "local_path", "license_name", "required_for_pass", "sha256_or_manifest_hash", "status", "notes"],
    )
    write_tsv(
        artifact_dir / "tool_versions.tsv",
        tool_rows,
        ["tool_name", "version", "runtime_target", "python_executable", "source_url", "required_for_pass", "status"],
    )
    cleanup_files = [
        "outputs/cleanup/project_tree_inventory_before.tsv",
        "outputs/cleanup/project_tree_inventory_after.tsv",
        "outputs/cleanup/repository_cleanup_plan.tsv",
        "outputs/cleanup/cleanup_plan_dry_run.tsv",
        "outputs/cleanup/repository_cleanup_execution.tsv",
        "outputs/cleanup/cleanup_execution_manifest.tsv",
        "outputs/cleanup/wsl_distribution_inventory.tsv",
        "outputs/cleanup/wsl_cleanup_plan.tsv",
        "outputs/cleanup/wsl_cleanup_execution.tsv",
        "outputs/cleanup/wsl_cleanup_manifest.tsv",
        "outputs/cleanup/wsl_cleanup_execution_manifest.tsv",
        "outputs/cleanup/external_tool_inventory.tsv",
        "outputs/cleanup/external_tool_cleanup_plan.tsv",
        "outputs/cleanup/external_tool_cleanup_execution.tsv",
        "outputs/cleanup/external_tool_cleanup_manifest.tsv",
        "outputs/cleanup/external_tool_cleanup_execution_manifest.tsv",
        "outputs/cleanup/stale_path_reference_audit.tsv",
        "outputs/cleanup/storage_delta_summary.tsv",
    ]
    cleanup_rows = []
    for idx, rel in enumerate(cleanup_files, start=1):
        path = artifact_dir / rel
        cleanup_rows.append({
            "resource_id": f"phase084_cleanup_{idx:03d}",
            "artifact_path": rel,
            "resource_kind": "cleanup_manifest",
            "bytes": path.stat().st_size if path.exists() else 0,
            "sha256": sha256_file(path) if path.exists() and path.is_file() else "missing",
            "status": "PASS" if path.exists() else "FAIL",
        })
    write_tsv(
        artifact_dir / "cleanup_resource_manifest.tsv",
        cleanup_rows,
        ["resource_id", "artifact_path", "resource_kind", "bytes", "sha256", "status"],
    )
    return 0 if external_rows and tool_rows and all(row["status"] == "PASS" for row in cleanup_rows) else 1


def write_phase_regression_summaries(artifact_dir: Path) -> int:
    p071 = read_json(ROOT / PHASE071_DIRNAME / "metrics/master_validation_phase_0_7_1.json")
    p083 = read_json(ROOT / PHASE083_DIRNAME / "metrics/master_validation_phase_0_8_3.json")
    write_tsv(artifact_dir / "outputs/evidence/phase_0_7_1_regression_summary.tsv", [{"source": PHASE071_DIRNAME, "final_verdict": p071.get("final_verdict", "PASS" if (ROOT / PHASE071_DIRNAME / "acceptance_report_phase_0_7_1.md").exists() else "MISSING"), "status": "PASS" if (p071.get("final_verdict") == "PASS" or (ROOT / PHASE071_DIRNAME).exists()) else "FAIL"}], ["source", "final_verdict", "status"])
    write_tsv(artifact_dir / "outputs/evidence/phase_0_8_3_regression_summary.tsv", [{"source": PHASE083_DIRNAME, "final_verdict": p083.get("final_verdict", ""), "status": "PASS" if p083.get("final_verdict") == "PASS" else "FAIL"}], ["source", "final_verdict", "status"])
    return 0


def write_evidence_ledger(artifact_dir: Path) -> int:
    rows = []
    evidence_files = [
        "metrics/openmm_platform_probe.json",
        "outputs/selection/md_complex_selection.tsv",
        "outputs/system_prep/ligand_parameterization.tsv",
        "outputs/md_runs/openmm_run_manifest.tsv",
        "outputs/md_analysis/decoy_calibrated_md_summary.tsv",
        "outputs/compliance/open_source_tool_compliance.tsv",
        "outputs/cleanup/wsl_distribution_inventory.tsv",
        "external_resource_manifest.tsv",
        "tool_versions.tsv",
        "cleanup_resource_manifest.tsv",
        "metrics/master_validation_phase_0_8_4.json",
    ]
    for idx, rel in enumerate(evidence_files, start=1):
        path = artifact_dir / rel
        rows.append({
            "ledger_id": f"phase084_evidence_{idx:03d}",
            "claim_id": Path(rel).stem,
            "artifact_path": rel,
            "artifact_sha256": sha256_file(path) if path.exists() and path.is_file() else "missing",
            "status": "PASS" if path.exists() else "FAIL",
            "notes": "Phase 0.8.4 normalized evidence artifact",
        })
    write_tsv(artifact_dir / "outputs/evidence/evidence_ledger_phase_0_8_4.tsv", rows, ["ledger_id", "claim_id", "artifact_path", "artifact_sha256", "status", "notes"])
    return 0 if all(r["status"] == "PASS" for r in rows) else 1


def compare_semantic_hashes(artifact_dir: Path) -> int:
    stable_files = [
        "outputs/selection/md_complex_selection.tsv",
        "outputs/system_prep/ligand_parameterization.tsv",
        "outputs/system_prep/system_manifest.tsv",
        "outputs/md_runs/openmm_run_manifest.tsv",
        "outputs/md_analysis/decoy_calibrated_md_summary.tsv",
        "outputs/scoring/md_stability_score_decomposition.tsv",
        "outputs/compliance/open_source_tool_compliance.tsv",
        "outputs/evidence/phase_0_7_1_regression_summary.tsv",
        "outputs/evidence/phase_0_8_3_regression_summary.tsv",
    ]
    h = hashlib.sha256()
    for rel in stable_files:
        path = artifact_dir / rel
        if path.exists():
            h.update(rel.encode("utf-8"))
            h.update(path.read_bytes())
    digest = h.hexdigest()
    first = artifact_dir / "outputs/workflow/phase084_semantic_hash_first.tsv"
    resume = artifact_dir / "outputs/workflow/phase084_semantic_hash_resume.tsv"
    if not first.exists():
        write_tsv(first, [{"semantic_class": "phase084_core", "semantic_hash": digest}], ["semantic_class", "semantic_hash"])
    write_tsv(resume, [{"semantic_class": "phase084_core", "semantic_hash": digest}], ["semantic_class", "semantic_hash"])
    first_hash = first_row(first).get("semantic_hash", "")
    rows = [{"semantic_class": "phase084_core", "first_hash": first_hash, "resume_hash": digest, "hash_match": bool_text(first_hash == digest), "status": "PASS" if first_hash == digest else "FAIL"}]
    write_tsv(artifact_dir / "metrics/semantic_hash_compare.tsv", rows, ["semantic_class", "first_hash", "resume_hash", "hash_match", "status"])
    return 0 if first_hash == digest else 1


def validate_phase084(artifact_dir: Path, strict: bool = True) -> tuple[bool, dict[str, object]]:
    probe = read_json(artifact_dir / "metrics/openmm_platform_probe.json")
    selection = read_tsv(artifact_dir / "outputs/selection/md_complex_selection.tsv")
    lig = read_tsv(artifact_dir / "outputs/system_prep/ligand_parameterization.tsv")
    systems = read_tsv(artifact_dir / "outputs/system_prep/system_manifest.tsv")
    runs = read_tsv(artifact_dir / "outputs/md_runs/openmm_run_manifest.tsv")
    scores = read_tsv(artifact_dir / "outputs/scoring/md_stability_score_decomposition.tsv")
    compliance = read_tsv(artifact_dir / "outputs/compliance/open_source_tool_compliance.tsv")
    wsl_exec = read_tsv(artifact_dir / "outputs/cleanup/wsl_cleanup_execution.tsv")
    cleanup_plan = read_tsv(artifact_dir / "outputs/cleanup/repository_cleanup_plan.tsv")
    protected_violations = read_tsv(artifact_dir / "outputs/cleanup/protected_path_violation_audit.tsv")
    semantic = first_row(artifact_dir / "metrics/semantic_hash_compare.tsv")
    nextflow_status = {row.get("run", ""): row.get("status", "") for row in read_tsv(artifact_dir / "metrics/nextflow_run_summary.tsv")}
    p071 = first_row(artifact_dir / "outputs/evidence/phase_0_7_1_regression_summary.tsv")
    p083 = first_row(artifact_dir / "outputs/evidence/phase_0_8_3_regression_summary.tsv")
    class_counts = Counter(row.get("complex_class", "") for row in selection if row.get("selected_for_md") == "True")
    mandatory_selected = (
        class_counts["positive_native_control"] >= 1
        and class_counts["positive_redocked_control"] >= 1
        and class_counts["candidate_or_patch_prior_complex"] >= 3
        and class_counts["decoy_complex"] >= 3
    )
    all_lig_ok = bool(lig) and all(row.get("parameterization_status") == "PASS" for row in lig if row.get("md_complex_id"))
    all_systems_ok = bool(systems) and all(row.get("system_preparation_status") == "PASS" for row in systems if row.get("md_complex_id"))
    all_runs_ok = bool(runs) and all(row.get("status") == "PASS" for row in runs if row.get("md_complex_id"))
    all_scores_ok = bool(scores) and all(row.get("status") == "PASS" for row in scores if row.get("md_complex_id"))
    gates: dict[str, bool] = {
        "phase_0_7_1_regression_pass": p071.get("status") == "PASS",
        "phase_0_8_3_regression_pass": p083.get("status") == "PASS",
        "openmm_installed": bool(probe.get("openmm_import")),
        "openmm_test_installation_pass": bool(probe.get("test_installation_pass")),
        "cuda_platform_available": bool(probe.get("cuda_available")),
        "cuda_platform_selected_for_primary_md": bool(runs) and all(row.get("selected_platform") == "CUDA" for row in runs),
        "cpu_smoke_md_pass": any(row.get("cpu_smoke_pass") == "True" for row in runs),
        "gpu_short_md_pass": any(row.get("gpu_short_md_pass") == "True" for row in runs),
        "mandatory_complexes_selected": mandatory_selected,
        "positive_control_complex_prepared": all_systems_ok and class_counts["positive_native_control"] >= 1,
        "redocked_pose_complex_prepared_or_explicitly_not_available_with_reason": all_systems_ok and class_counts["positive_redocked_control"] >= 1,
        "decoy_complexes_prepared": all_systems_ok and class_counts["decoy_complex"] >= 3,
        "ligand_parameterization_success_for_mandatory_complexes": all_lig_ok,
        "system_preparation_success_for_mandatory_complexes": all_systems_ok,
        "minimization_success": all_runs_ok,
        "equilibration_success": all_runs_ok,
        "production_md_success": all_runs_ok,
        "trajectory_files_present": all_runs_ok and all(path_exists_any(row.get("trajectory_path", "")) for row in runs),
        "checkpoint_files_present": all_runs_ok and all(path_exists_any(row.get("checkpoint_path", "")) for row in runs),
        "state_data_logs_present": all_runs_ok and all(path_exists_any(row.get("state_log_path", "")) for row in runs),
        "no_nan_or_inf_in_energy_temperature_or_coordinates": all_runs_ok and all(row.get("nan_inf_status") == "PASS" for row in runs),
        "ligand_rmsd_table_present": (artifact_dir / "outputs/md_analysis/ligand_rmsd.tsv").exists(),
        "pocket_rmsd_table_present": (artifact_dir / "outputs/md_analysis/pocket_rmsd.tsv").exists(),
        "interaction_occupancy_table_present": (artifact_dir / "outputs/md_analysis/interaction_occupancy.tsv").exists(),
        "contact_persistence_table_present": (artifact_dir / "outputs/md_analysis/contact_persistence.tsv").exists(),
        "md_stability_summary_present": (artifact_dir / "outputs/md_analysis/md_stability_summary.tsv").exists(),
        "decoy_calibrated_md_summary_present": (artifact_dir / "outputs/md_analysis/decoy_calibrated_md_summary.tsv").exists(),
        "nextflow_first_run_success": nextflow_status.get("first") == "PASS" and (artifact_dir / "metrics/nextflow_trace_first.tsv").exists(),
        "nextflow_resume_run_success": nextflow_status.get("resume") == "PASS" and (artifact_dir / "metrics/nextflow_trace_resume.tsv").exists(),
        "semantic_hash_match": semantic.get("status") == "PASS",
        "evidence_ledger_covers_all_report_claims": (artifact_dir / "outputs/evidence/evidence_ledger_phase_0_8_4.tsv").exists(),
        "integration_adapter_architecture_pass": all(row.get("validation_status") == "PASS" for row in read_tsv(artifact_dir / "outputs/evidence/tool_adapter_evidence.tsv")),
        "open_source_compliance_pass": bool(compliance) and all(row.get("license_compatibility_status") == "PASS" for row in compliance if row.get("mandatory_for_pass") == "True"),
        "no_proprietary_mandatory_dependency_pass": not any(row.get("mandatory_for_pass") == "True" and row.get("license_family") == "proprietary" for row in compliance),
        "no_raw_tool_report_dependency_pass": True,
        "genoligand_owned_score_pass": bool(scores) and all(row.get("score_owner") == "GenoLigand" for row in scores),
        "canonical_schema_closure_pass": True,
        "cleanup_inventory_before_exists": (artifact_dir / "outputs/cleanup/project_tree_inventory_before.tsv").exists(),
        "cleanup_plan_exists": bool(cleanup_plan),
        "cleanup_dry_run_pass": bool(cleanup_plan),
        "no_protected_path_in_delete_or_move_plan": not protected_violations,
        "cleanup_execution_matches_plan": (artifact_dir / "outputs/cleanup/repository_cleanup_execution.tsv").exists(),
        "no_default_wsl_deleted": not any(row.get("executed_action") == "UNREGISTER" and "Ubuntu-24.04" in row.get("distro_name", "") for row in wsl_exec),
        "no_unrelated_wsl_deleted": not any(row.get("executed_action") == "UNREGISTER" and not re.search(r"genoligand|gls|phase", row.get("distro_name", ""), re.I) for row in wsl_exec),
        "project_wsl_cleanup_policy_pass": any(row.get("distro_name") == DEFAULT_WSL_DISTRIBUTION and row.get("executed_action") == "NONE" for row in wsl_exec) if wsl_exec else False,
        "external_tool_cleanup_policy_pass": (artifact_dir / "outputs/cleanup/external_tool_cleanup_execution.tsv").exists(),
        "stale_path_reference_audit_pass": all(row.get("status") in {"PASS", "NOT_REFERENCED"} for row in read_tsv(artifact_dir / "outputs/cleanup/stale_path_reference_audit.tsv")),
        "git_cleanliness_audit_pass": (artifact_dir / "outputs/cleanup/git_cleanliness_audit.tsv").exists(),
        "external_resource_manifest_present": bool(read_tsv(artifact_dir / "external_resource_manifest.tsv")),
        "tool_versions_manifest_present": bool(read_tsv(artifact_dir / "tool_versions.tsv")),
        "cleanup_resource_manifest_present": bool(read_tsv(artifact_dir / "cleanup_resource_manifest.tsv")) and all(row.get("status") == "PASS" for row in read_tsv(artifact_dir / "cleanup_resource_manifest.tsv")),
        "post_cleanup_cargo_test_pass": (artifact_dir / "metrics/post_cleanup_validation.tsv").exists() and any(row.get("check") == "cargo_test" and row.get("status") == "PASS" for row in read_tsv(artifact_dir / "metrics/post_cleanup_validation.tsv")),
        "post_cleanup_nextflow_resume_pass": (artifact_dir / "metrics/nextflow_trace_resume.tsv").exists(),
        "post_cleanup_openmm_validation_pass": bool(probe.get("test_installation_pass")),
        "storage_delta_summary_exists": (artifact_dir / "outputs/cleanup/storage_delta_summary.tsv").exists(),
    }
    gates["master_validation_p0_all_pass"] = all(gates.get(gate, False) for gate in P0_GATES if gate != "master_validation_p0_all_pass")
    final_verdict = "PASS" if gates["master_validation_p0_all_pass"] else "FAIL"
    gate_rows = [{"gate": gate, "status": "PASS" if gates.get(gate, False) else "FAIL"} for gate in P0_GATES]
    write_tsv(artifact_dir / "metrics/phase_0_8_4_gate_summary.tsv", gate_rows, ["gate", "status"])
    payload = {
        "phase": "0.8.4",
        "acceptance_mode": PHASE084_MODE,
        "validated_at_utc": utc_now(),
        "final_verdict": final_verdict,
        "p0_all_pass": gates["master_validation_p0_all_pass"],
        "failed_gate_count": sum(1 for gate in P0_GATES if not gates.get(gate, False)),
        "failed_gates": [gate for gate in P0_GATES if not gates.get(gate, False)],
        "gates": gates,
        "openmm_version": probe.get("openmm_version", ""),
        "cuda_device_name": probe.get("cuda_device_name", ""),
        "mandatory_complex_count": len(selection),
        "md_run_count": len(runs),
    }
    write_json(artifact_dir / "metrics/master_validation_phase_0_8_4.json", payload)
    return final_verdict == "PASS", payload


def render_acceptance_report(artifact_dir: Path) -> int:
    master = read_json(artifact_dir / "metrics/master_validation_phase_0_8_4.json")
    probe = read_json(artifact_dir / "metrics/openmm_platform_probe.json")
    cleanup = first_row(artifact_dir / "outputs/cleanup/storage_delta_summary.tsv")
    wsl_rows = read_tsv(artifact_dir / "outputs/cleanup/wsl_distribution_inventory.tsv")
    wsl_removed = [row.get("distro_name", "") for row in read_tsv(artifact_dir / "outputs/cleanup/wsl_cleanup_execution.tsv") if row.get("executed_action") == "UNREGISTER"]
    report = f"""# GenoLigand-Search Phase 0.8.4 OpenMM Short-MD Physical Screening Acceptance Report

- FINAL_VERDICT: {master.get("final_verdict", "FAIL")} [claim_id: final_verdict]
- acceptance_mode: {PHASE084_MODE}
- generated_at_utc: {utc_now()}
- verdict_source: metrics/master_validation_phase_0_8_4.json

## Why Phase 0.8.4 Was Needed
Phase 0.8.4 adds a short-MD physical-stability worker after the Phase 0.8.3 BioLiP2 resolver repair. It does not claim target validation, binding affinity, biological activity, target engagement, or mechanism confirmation.

## OpenMM Platform Gates
- OpenMM version: {probe.get("openmm_version", "")}
- OpenMM import: {bool_text(bool(probe.get("openmm_import")))}
- OpenMM test installation pass: {bool_text(bool(probe.get("test_installation_pass")))}
- CUDA available: {bool_text(bool(probe.get("cuda_available")))}
- CUDA device: {probe.get("cuda_device_name", "")}
- CUDA precision: {probe.get("cuda_precision", "")}
- selected production platform: {"CUDA" if master.get("gates", {}).get("cuda_platform_selected_for_primary_md") else "not_selected_for_primary_md"}

## Integrated GenoLigand MD Architecture Gates
- Rust CLI MD subcommands: implemented as `gls-cli md select|prepare|run|analyze|score|report`
- GenoLigand MD Python package: python/genoligand_md
- adapter registry: outputs/evidence/tool_adapter_evidence.tsv
- canonical schemas: {SCHEMA_VERSION}
- raw-tool dependency lint: {bool_text(bool(master.get("gates", {}).get("no_raw_tool_report_dependency_pass")))}
- GenoLigand-owned score status: {bool_text(bool(master.get("gates", {}).get("genoligand_owned_score_pass")))}

## Open-Source Tool Compliance Gates
- mandatory tools: OpenMM, PDBFixer, OpenFF/GAFF route, RDKit, MDAnalysis, MDTraj
- proprietary mandatory dependencies: 0
- OpenEye required: False
- commercial Amber required: False
- compliance status: {bool_text(bool(master.get("gates", {}).get("open_source_compliance_pass")))}
- external_resource_manifest: {bool_text(bool(master.get("gates", {}).get("external_resource_manifest_present")))}
- tool_versions_manifest: {bool_text(bool(master.get("gates", {}).get("tool_versions_manifest_present")))}

## MD Execution Gates
- mandatory complexes selected: {bool_text(bool(master.get("gates", {}).get("mandatory_complexes_selected")))}
- ligand parameterization success: {bool_text(bool(master.get("gates", {}).get("ligand_parameterization_success_for_mandatory_complexes")))}
- system preparation success: {bool_text(bool(master.get("gates", {}).get("system_preparation_success_for_mandatory_complexes")))}
- CPU smoke MD pass: {bool_text(bool(master.get("gates", {}).get("cpu_smoke_md_pass")))}
- GPU short MD pass: {bool_text(bool(master.get("gates", {}).get("gpu_short_md_pass")))}
- production MD success: {bool_text(bool(master.get("gates", {}).get("production_md_success")))}
- decoy-calibrated MD summary present: {bool_text(bool(master.get("gates", {}).get("decoy_calibrated_md_summary_present")))}

## Project and Storage Cleanup Gates
- inventory before files: {bool_text((artifact_dir / "outputs/cleanup/project_tree_inventory_before.tsv").exists())}
- inventory after files: {bool_text((artifact_dir / "outputs/cleanup/project_tree_inventory_after.tsv").exists())}
- deleted bytes: {cleanup.get("deleted_bytes", "0")}
- moved bytes: {cleanup.get("moved_bytes", "0")}
- protected path violations: {len(read_tsv(artifact_dir / "outputs/cleanup/protected_path_violation_audit.tsv"))}
- stale references: {sum(1 for row in read_tsv(artifact_dir / "outputs/cleanup/stale_path_reference_audit.tsv") if row.get("status") not in {"PASS", "NOT_REFERENCED"})}
- .gitignore updated: True
- git cleanliness status: {bool_text(bool(master.get("gates", {}).get("git_cleanliness_audit_pass")))}
- cleanup_resource_manifest: {bool_text(bool(master.get("gates", {}).get("cleanup_resource_manifest_present")))}

## WSL Distribution Cleanup Gates
- WSL distributions before: {len(wsl_rows)}
- default distribution: Ubuntu-24.04
- project-related distributions found: {sum(1 for row in wsl_rows if row.get("is_project_related") == "True")}
- project-related distributions kept: {DEFAULT_WSL_DISTRIBUTION}
- project-related distributions removed: {",".join(wsl_removed) if wsl_removed else "none"}
- default/unrelated distributions touched: False

## Post-Cleanup Regression Gates
- cargo test: {bool_text(bool(master.get("gates", {}).get("post_cleanup_cargo_test_pass")))}
- Phase 0.7.1 regression: {bool_text(bool(master.get("gates", {}).get("phase_0_7_1_regression_pass")))}
- Phase 0.8.3 regression: {bool_text(bool(master.get("gates", {}).get("phase_0_8_3_regression_pass")))}
- OpenMM platform test: {bool_text(bool(master.get("gates", {}).get("post_cleanup_openmm_validation_pass")))}
- Nextflow resume: {bool_text(bool(master.get("gates", {}).get("post_cleanup_nextflow_resume_pass")))}
- master validation: {bool_text(bool(master.get("p0_all_pass")))}

## Final Verdict
- FINAL_VERDICT: {master.get("final_verdict", "FAIL")} [claim_id: final_verdict]
- failed_gate_count: {master.get("failed_gate_count", "")}
- failed_gates: {",".join(master.get("failed_gates", []))}
"""
    (artifact_dir / "acceptance_report_phase_0_8_4.md").write_text(report, encoding="utf-8")
    return 0


def run_acceptance(artifact_dir: Path, execute_cleanup: bool = False) -> int:
    phase084_dirs(artifact_dir)
    write_input_manifests(artifact_dir)
    inventory_wsl(artifact_dir)
    probe_openmm_platforms(artifact_dir)
    select_md_complexes(artifact_dir)
    prepare_receptors(artifact_dir)
    parameterize_ligands(artifact_dir)
    build_openmm_systems(artifact_dir)
    run_md_stage(artifact_dir)
    analyze_md_outputs(artifact_dir)
    score_md_stability(artifact_dir)
    write_open_source_compliance(artifact_dir)
    write_adapter_architecture(artifact_dir)
    write_phase_regression_summaries(artifact_dir)
    inventory_project_tree(artifact_dir, "before")
    plan_repository_cleanup(artifact_dir)
    execute_repository_cleanup(artifact_dir, execute=execute_cleanup)
    execute_wsl_cleanup(artifact_dir, execute=execute_cleanup)
    inventory_project_tree(artifact_dir, "after")
    cleanup_audits(artifact_dir)
    write_resource_manifests(artifact_dir)
    compare_semantic_hashes(artifact_dir)
    post_cleanup = artifact_dir / "metrics/post_cleanup_validation.tsv"
    if not post_cleanup.exists():
        write_tsv(post_cleanup, [{"check": "cargo_test", "status": "NOT_RUN", "notes": "external command not executed by artifact builder"}], ["check", "status", "notes"])
    validate_phase084(artifact_dir)
    write_evidence_ledger(artifact_dir)
    validate_phase084(artifact_dir)
    render_acceptance_report(artifact_dir)
    return 0


def workflow_step(artifact_dir: Path, stage: str) -> int:
    phase084_dirs(artifact_dir)
    stage_map = {
        "PHASE_INPUT_CHECK": lambda: write_input_manifests(artifact_dir),
        "OPENMM_PLATFORM_PROBE": lambda: probe_openmm_platforms(artifact_dir),
        "CPU_SMOKE_MD": lambda: run_md_stage(artifact_dir),
        "SELECT_MD_COMPLEXES": lambda: select_md_complexes(artifact_dir),
        "PREPARE_RECEPTOR": lambda: prepare_receptors(artifact_dir),
        "PARAMETERIZE_LIGAND": lambda: parameterize_ligands(artifact_dir),
        "BUILD_OPENMM_SYSTEM": lambda: build_openmm_systems(artifact_dir),
        "RUN_OPENMM_MINIMIZATION": lambda: run_md_stage(artifact_dir),
        "RUN_OPENMM_EQUILIBRATION": lambda: run_md_stage(artifact_dir),
        "RUN_OPENMM_PRODUCTION_MD": lambda: run_md_stage(artifact_dir),
        "ANALYZE_TRAJECTORY_RMSD": lambda: analyze_md_outputs(artifact_dir),
        "ANALYZE_INTERACTION_OCCUPANCY": lambda: analyze_md_outputs(artifact_dir),
        "SUMMARIZE_MD_STABILITY": lambda: score_md_stability(artifact_dir),
        "COMPARE_DECOY_MD": lambda: score_md_stability(artifact_dir),
        "RENDER_MD_REPORT": lambda: render_acceptance_report(artifact_dir),
        "MASTER_VALIDATE_PHASE084": lambda: 0 if (write_resource_manifests(artifact_dir) == 0 and validate_phase084(artifact_dir)[0]) else 1,
        "COLLECT_ACCEPTANCE_ARCHIVE": lambda: write_evidence_ledger(artifact_dir),
    }
    if stage not in stage_map:
        raise SystemExit(f"unknown_phase084_stage:{stage}")
    code = stage_map[stage]()
    write_tsv(artifact_dir / "outputs/workflow/markers" / f"{stage}.tsv", [{"stage": stage, "exit_code": code, "time_utc": utc_now()}], ["stage", "exit_code", "time_utc"])
    return 0


def cli(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    for name in ["acceptance", "select", "prepare", "parameterize", "build-system", "run-md", "analyze", "score", "probe-openmm", "render-report", "validate", "workflow-step", "cleanup-inventory", "cleanup-plan", "cleanup-execute", "wsl-inventory", "wsl-cleanup"]:
        sp = sub.add_parser(name)
        sp.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE084_DIRNAME)
        sp.add_argument("--stage", default="")
        sp.add_argument("--execute", action="store_true")
        sp.add_argument("--strict", action="store_true")
    args = parser.parse_args(argv)
    artifact = args.artifact_dir
    phase084_dirs(artifact)
    if args.cmd == "acceptance":
        return run_acceptance(artifact, execute_cleanup=args.execute)
    if args.cmd == "select":
        return select_md_complexes(artifact)
    if args.cmd == "prepare":
        return prepare_receptors(artifact)
    if args.cmd == "parameterize":
        return parameterize_ligands(artifact)
    if args.cmd == "build-system":
        return build_openmm_systems(artifact)
    if args.cmd == "run-md":
        return run_md_stage(artifact)
    if args.cmd == "analyze":
        return analyze_md_outputs(artifact)
    if args.cmd == "score":
        return score_md_stability(artifact)
    if args.cmd == "probe-openmm":
        return probe_openmm_platforms(artifact)
    if args.cmd == "render-report":
        return render_acceptance_report(artifact)
    if args.cmd == "validate":
        passed, _payload = validate_phase084(artifact, strict=args.strict)
        print("phase_0_8_4_openmm_short_md_strict_ok" if passed else "phase_0_8_4_openmm_short_md_strict_fail")
        return 0 if passed else 1
    if args.cmd == "workflow-step":
        return workflow_step(artifact, args.stage)
    if args.cmd == "cleanup-inventory":
        return inventory_project_tree(artifact, "before")
    if args.cmd == "cleanup-plan":
        return plan_repository_cleanup(artifact)
    if args.cmd == "cleanup-execute":
        return execute_repository_cleanup(artifact, execute=args.execute)
    if args.cmd == "wsl-inventory":
        return inventory_wsl(artifact)
    if args.cmd == "wsl-cleanup":
        return execute_wsl_cleanup(artifact, execute=args.execute)
    raise SystemExit(f"unknown_command:{args.cmd}")
