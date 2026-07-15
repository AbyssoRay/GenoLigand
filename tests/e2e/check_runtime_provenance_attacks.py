from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable


SCHEMA_VERSION = "genoligand_runtime_provenance_attacks_v1"
COMPLETION_OUTPUTS = (
    "target_candidate_ranking.tsv",
    "candidate_detail.json",
    "evidence_ledger.tsv",
    "research_report.md",
    "score_decomposition.tsv",
    "artifact_source_map.tsv",
)
TARGET_RANK_ENTRYPOINT = (
    "gls-cli target rank -> ensure_final_artifacts_authentic -> read_docking_scores"
)
DYNAMICS_ENTRYPOINT = (
    "gls-cli dynamics screen -> materialize_indexed_md_summary -> "
    "verified_indexed_md_computations"
)


@dataclass(frozen=True)
class DockingArtifact:
    candidate_id: str
    job_id: str
    invocation_id: str
    manifest: str
    pose_sdf: str


@dataclass(frozen=True)
class MdArtifact:
    candidate_id: str
    job_id: str
    invocation_id: str
    manifest: str
    provenance: str
    md_summary: str
    md_rmsd: str
    md_hbonds: str


@dataclass(frozen=True)
class RunContract:
    run_id: str
    project_id: str
    input_manifest: Path
    docking: tuple[DockingArtifact, ...]
    md: MdArtifact


@dataclass(frozen=True)
class Rejection:
    stage: tuple[str, str]
    entrypoint: str
    cli_reason: str
    provenance_reason: str
    gate_artifact: str = ""
    gate_marker: str = ""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def load_json(path: Path, label: str) -> dict[str, Any]:
    require(path.is_file(), f"missing {label}: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"invalid {label}: {path}: {error}") from error
    require(isinstance(value, dict), f"{label} must be a JSON object: {path}")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def valid_sha256(value: object) -> bool:
    text = str(value)
    return len(text) == 64 and all(character in "0123456789abcdefABCDEF" for character in text)


def valid_git_commit(value: object) -> bool:
    text = str(value)
    return len(text) == 40 and all(character in "0123456789abcdefABCDEF" for character in text)


def tree_snapshot(root: Path) -> dict[str, Any]:
    digest = hashlib.sha256()
    files = sorted(path for path in root.rglob("*") if path.is_file())
    total = 0
    for path in files:
        relative = path.relative_to(root).as_posix()
        size = path.stat().st_size
        content = sha256_file(path)
        total += size
        digest.update(relative.encode("utf-8"))
        digest.update(str(size).encode("ascii"))
        digest.update(content.encode("ascii"))
    return {
        "file_count": len(files),
        "bytes": total,
        "sha256": digest.hexdigest(),
    }


def read_tsv(path: Path, label: str) -> tuple[list[str], list[dict[str, str]]]:
    require(path.is_file(), f"missing {label}: {path}")
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            fieldnames = list(reader.fieldnames or [])
            rows = [dict(row) for row in reader]
    except (OSError, UnicodeError, csv.Error) as error:
        raise RuntimeError(f"invalid {label}: {path}: {error}") from error
    require(fieldnames, f"{label} has no header: {path}")
    return fieldnames, rows


def write_tsv(path: Path, fieldnames: list[str], rows: list[dict[str, str]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            fieldnames=fieldnames,
            lineterminator="\n",
            extrasaction="ignore",
        )
        writer.writeheader()
        writer.writerows(rows)


def mutate_tsv_candidate(
    path: Path,
    candidate_id: str,
    changes: dict[str, str],
) -> dict[str, str]:
    fieldnames, rows = read_tsv(path, path.name)
    require(set(changes) <= set(fieldnames), f"TSV fields are missing in {path}: {sorted(changes)}")
    matches = [row for row in rows if row.get("candidate_id") == candidate_id]
    require(len(matches) == 1, f"expected one {candidate_id} row in {path}")
    previous = {field: str(matches[0].get(field, "")) for field in changes}
    matches[0].update(changes)
    write_tsv(path, fieldnames, rows)
    return previous


def resolve_run_artifact(run_dir: Path, raw: object, label: str) -> tuple[Path, str]:
    text = str(raw).strip()
    require(text, f"{label} path is empty")
    candidate = Path(text)
    path = candidate.resolve() if candidate.is_absolute() else (run_dir / candidate).resolve()
    try:
        relative = path.relative_to(run_dir).as_posix()
    except ValueError as error:
        raise RuntimeError(f"{label} is outside completed run: {text}") from error
    require(path.is_file(), f"missing {label}: {path}")
    return path, relative


def safe_component(value: object, fallback: str) -> str:
    text = re.sub(r"[^A-Za-z0-9_.-]+", "_", str(value).strip()).strip("._-")
    return text[:80] or fallback


def result_for_entry(run_dir: Path, entry: dict[str, Any], kind: str) -> dict[str, Any]:
    job_id = str(entry.get("job_id", "")).strip()
    require(job_id, f"{kind} manifest index job_id is missing")
    return load_json(run_dir / "worker_results" / f"{job_id}.json", f"{kind} worker result")


def manifest_entries(run_dir: Path, kind: str) -> list[dict[str, Any]]:
    index = load_json(
        run_dir / "tool_outputs" / f"{kind}_manifest_index.json",
        f"{kind} manifest index",
    )
    require(
        index.get("schema_version") == f"genoligand_{kind}_manifest_index_v1",
        f"{kind} manifest index schema differs",
    )
    require(index.get("run_id") == run_dir.name, f"{kind} manifest index run_id differs")
    entries = index.get("entries")
    require(isinstance(entries, list) and entries, f"{kind} manifest index entries are empty")
    require(all(isinstance(entry, dict) for entry in entries), f"{kind} manifest entry is invalid")
    candidate_ids = [str(entry.get("candidate_id", "")).strip() for entry in entries]
    job_ids = [str(entry.get("job_id", "")).strip() for entry in entries]
    require(
        all(candidate_ids) and len(set(candidate_ids)) == len(candidate_ids),
        f"{kind} manifest index candidate identities are missing or duplicated",
    )
    require(
        all(job_ids) and len(set(job_ids)) == len(job_ids),
        f"{kind} manifest index job identities are missing or duplicated",
    )
    return entries


def validate_docking_contract(run_dir: Path) -> tuple[DockingArtifact, ...]:
    _, summary_rows = read_tsv(run_dir / "docking_summary.tsv", "docking summary")
    summary_candidates = {
        str(row.get("candidate_id", ""))
        for row in summary_rows
        if str(row.get("docking_status", row.get("status", ""))).lower() == "completed"
        and "vina" in str(row.get("docking_engine", row.get("engine", ""))).lower()
    }
    completed: list[DockingArtifact] = []
    for entry in manifest_entries(run_dir, "docking"):
        if entry.get("status") != "completed":
            continue
        candidate_id = str(entry.get("candidate_id", "")).strip()
        job_id = str(entry.get("job_id", "")).strip()
        invocation_id = str(entry.get("invocation_id", "")).strip()
        require(candidate_id and job_id and invocation_id, "completed docking index identity is incomplete")
        result = result_for_entry(run_dir, entry, "docking")
        require(
            result.get("status") == "PASS"
            and result.get("execution_status") == "completed"
            and "vina" in str(result.get("engine_version", "")).lower(),
            f"docking result is not a real completed Vina result: {candidate_id}",
        )
        for field, expected in (
            ("candidate_id", candidate_id),
            ("job_id", job_id),
            ("run_id", run_dir.name),
            ("invocation_id", invocation_id),
        ):
            require(result.get(field) == expected, f"docking result {field} differs: {candidate_id}")
        manifest_path, manifest_relative = resolve_run_artifact(
            run_dir, entry.get("manifest"), f"docking manifest {candidate_id}"
        )
        manifest = load_json(manifest_path, f"docking manifest {candidate_id}")
        require(result.get("manifest") == manifest_relative, f"docking result manifest differs: {candidate_id}")
        require(
            manifest.get("artifact_schema_version") == "genoligand_docking_artifacts_v1"
            and manifest.get("status") == "completed"
            and manifest.get("method") == "autodock_vina",
            f"docking manifest is not real completed Vina output: {candidate_id}",
        )
        require(manifest.get("candidate_id") == candidate_id, f"docking manifest candidate differs: {candidate_id}")
        artifacts = manifest.get("artifacts")
        require(isinstance(artifacts, list) and artifacts, f"docking manifest artifacts are empty: {candidate_id}")
        pose_sdf = ""
        for row in artifacts:
            require(isinstance(row, dict), f"docking artifact row is invalid: {candidate_id}")
            path, relative = resolve_run_artifact(
                run_dir, row.get("artifact_path"), f"docking artifact {candidate_id}"
            )
            expected = str(row.get("artifact_sha256", ""))
            require(valid_sha256(expected), f"docking artifact digest is invalid: {relative}")
            require(sha256_file(path).lower() == expected.lower(), f"docking artifact digest differs: {relative}")
            if row.get("artifact_name") == "pose_sdf":
                pose_sdf = relative
        require(pose_sdf, f"completed Vina manifest has no pose_sdf: {candidate_id}")
        require(candidate_id in summary_candidates, f"completed Vina candidate is absent from summary: {candidate_id}")
        completed.append(
            DockingArtifact(
                candidate_id=candidate_id,
                job_id=job_id,
                invocation_id=invocation_id,
                manifest=manifest_relative,
                pose_sdf=pose_sdf,
            )
        )
    completed.sort(key=lambda item: item.candidate_id)
    require(
        len(completed) >= 2,
        "runtime provenance attack gate requires at least two real completed Vina candidates",
    )
    return tuple(completed)


def artifact_by_name(
    run_dir: Path,
    rows: list[dict[str, Any]],
    candidate_id: str,
    name: str,
) -> str:
    matches = []
    for row in rows:
        if Path(str(row.get("artifact_path", ""))).name != name:
            continue
        path, relative = resolve_run_artifact(
            run_dir, row.get("artifact_path"), f"MD {name} {candidate_id}"
        )
        expected = str(row.get("artifact_sha256", ""))
        require(valid_sha256(expected), f"MD artifact digest is invalid: {relative}")
        require(sha256_file(path).lower() == expected.lower(), f"MD artifact digest differs: {relative}")
        matches.append(relative)
    require(len(matches) == 1, f"completed OpenMM provenance must bind one {name}: {candidate_id}")
    return matches[0]


def validate_md_contract(run_dir: Path) -> MdArtifact:
    completed: list[MdArtifact] = []
    for entry in manifest_entries(run_dir, "md"):
        if entry.get("status") != "completed":
            continue
        candidate_id = str(entry.get("candidate_id", "")).strip()
        job_id = str(entry.get("job_id", "")).strip()
        invocation_id = str(entry.get("invocation_id", "")).strip()
        require(candidate_id and job_id and invocation_id, "completed MD index identity is incomplete")
        manifest_path, manifest_relative = resolve_run_artifact(
            run_dir, entry.get("manifest"), f"MD manifest {candidate_id}"
        )
        manifest = load_json(manifest_path, f"MD manifest {candidate_id}")
        if manifest.get("method") != "openmm_receptor_ligand_complex_dynamics":
            continue
        result = result_for_entry(run_dir, entry, "MD")
        require(
            result.get("status") == "PASS"
            and result.get("execution_status") == "completed"
            and "openmm" in str(result.get("engine_version", "")).lower(),
            f"MD result is not a real completed OpenMM result: {candidate_id}",
        )
        for field, expected in (
            ("candidate_id", candidate_id),
            ("job_id", job_id),
            ("run_id", run_dir.name),
            ("invocation_id", invocation_id),
        ):
            require(result.get(field) == expected, f"MD result {field} differs: {candidate_id}")
        provenance_path, provenance_relative = resolve_run_artifact(
            run_dir, entry.get("artifact_provenance"), f"MD provenance {candidate_id}"
        )
        provenance = load_json(provenance_path, f"MD provenance {candidate_id}")
        require(
            provenance.get("schema_version") == "genoligand_md_artifact_provenance_v1"
            and str(provenance.get("tool_name", "")).lower() == "openmm",
            f"MD provenance is not real OpenMM output: {candidate_id}",
        )
        for field, expected in (
            ("candidate_id", candidate_id),
            ("job_id", job_id),
            ("invocation_id", invocation_id),
        ):
            require(provenance.get(field) == expected, f"MD provenance {field} differs: {candidate_id}")
        rows = provenance.get("artifacts")
        require(isinstance(rows, list) and rows, f"MD provenance artifacts are empty: {candidate_id}")
        require(all(isinstance(row, dict) for row in rows), f"MD provenance artifact row is invalid: {candidate_id}")
        require(
            result.get("manifest") == manifest_relative
            and result.get("artifact_provenance") == provenance_relative
            and manifest.get("artifact_provenance") == provenance_relative,
            f"MD result/manifest provenance binding differs: {candidate_id}",
        )
        for row in rows:
            artifact_path, relative = resolve_run_artifact(
                run_dir, row.get("artifact_path"), f"MD provenance artifact {candidate_id}"
            )
            expected_digest = str(row.get("artifact_sha256", ""))
            require(valid_sha256(expected_digest), f"MD artifact digest is invalid: {relative}")
            require(
                sha256_file(artifact_path).lower() == expected_digest.lower(),
                f"MD artifact digest differs: {relative}",
            )
            for field, expected in (
                ("candidate_id", candidate_id),
                ("job_id", job_id),
                ("invocation_id", invocation_id),
            ):
                require(row.get(field) == expected, f"MD artifact {field} differs: {relative}")
        completed.append(
            MdArtifact(
                candidate_id=candidate_id,
                job_id=job_id,
                invocation_id=invocation_id,
                manifest=manifest_relative,
                provenance=provenance_relative,
                md_summary=artifact_by_name(run_dir, rows, candidate_id, "md_summary.tsv"),
                md_rmsd=artifact_by_name(run_dir, rows, candidate_id, "md_rmsd.tsv"),
                md_hbonds=artifact_by_name(run_dir, rows, candidate_id, "md_hbonds.tsv"),
            )
        )
    completed.sort(key=lambda item: item.candidate_id)
    require(completed, "runtime provenance attack gate requires a real completed OpenMM manifest")
    require((run_dir / "tool_outputs" / "md_summary.tsv").is_file(), "consolidated MD summary is missing")
    return completed[0]


def derive_input_manifest(run_dir: Path, workspace_root: Path) -> tuple[Path, dict[str, Any]]:
    context = load_json(run_dir / "run_context.json", "completed run context")
    require(context.get("state") == "completed", "run_context.json is not completed")
    require(context.get("run_id") == run_dir.name, "run context run_id differs")
    invocation_id = str(context.get("pipeline_invocation_id", "")).strip()
    require(invocation_id, "run context pipeline invocation is missing")
    require(
        (run_dir / "pipeline_invocation_id.txt").read_text(encoding="utf-8-sig").strip()
        == invocation_id,
        "run context pipeline invocation differs from the run artifact",
    )
    raw = context.get("input_manifest")
    path_value = raw.get("path") if isinstance(raw, dict) else raw
    require(str(path_value or "").strip(), "run context input manifest path is missing")
    candidate = Path(str(path_value))
    path = candidate.resolve() if candidate.is_absolute() else (workspace_root / candidate).resolve()
    require(path.is_file(), f"completed run input manifest is missing: {path}")
    return path, context


def validate_completed_run(run_dir: Path, workspace_root: Path) -> RunContract:
    require(run_dir.is_dir(), f"completed run does not exist: {run_dir}")
    state_path = run_dir / "run_state.txt"
    require(state_path.is_file(), f"run_state.txt is missing: {run_dir}")
    require(state_path.read_text(encoding="utf-8-sig").strip() == "completed", "run is not completed")
    input_manifest, context = derive_input_manifest(run_dir, workspace_root)
    project_id = safe_component(context.get("project_id"), "project")
    return RunContract(
        run_id=run_dir.name,
        project_id=project_id,
        input_manifest=input_manifest,
        docking=validate_docking_contract(run_dir),
        md=validate_md_contract(run_dir),
    )


def rebase_worker_jobs(run_dir: Path) -> None:
    job_dir = run_dir / "worker_jobs"
    require(job_dir.is_dir(), f"worker_jobs is missing from copied run: {run_dir}")
    for path in sorted(job_dir.glob("*.json")):
        value = load_json(path, "copied worker job")
        value["run_dir"] = str(run_dir.resolve())
        write_json(path, value)


def copy_case_run(
    session_root: Path,
    label: str,
    original_run: Path,
    contract: RunContract,
) -> tuple[Path, Path]:
    case_root = session_root / safe_component(label, "case")
    copied_run = case_root / contract.project_id / "runs" / contract.run_id
    copied_run.parent.mkdir(parents=True)
    shutil.copytree(original_run, copied_run)
    rebase_worker_jobs(copied_run)
    return case_root, copied_run


def remove_tree(path: Path) -> None:
    def retry(function: Callable[..., Any], name: str, _: Any) -> None:
        os.chmod(name, stat.S_IREAD | stat.S_IWRITE | stat.S_IEXEC)
        function(name)

    if path.exists():
        shutil.rmtree(path, onerror=retry)


def remove_completion_outputs(run_dir: Path) -> None:
    for relative in (*COMPLETION_OUTPUTS, "authenticity_gate.tsv"):
        path = run_dir / relative
        if path.exists():
            require(path.is_file(), f"completion output is not a file: {path}")
            path.unlink()


def completion_outputs(run_dir: Path) -> list[str]:
    return [relative for relative in COMPLETION_OUTPUTS if (run_dir / relative).exists()]


def cli_command(
    gls_cli: Path,
    stage: tuple[str, str],
    workspace_root: Path,
    science_store: Path,
    input_manifest: Path,
    run_dir: Path,
) -> list[str]:
    return [
        str(gls_cli),
        stage[0],
        stage[1],
        "--workspace-root",
        str(workspace_root),
        "--science-store",
        str(science_store),
        "--input-manifest",
        str(input_manifest),
        "--run-dir",
        str(run_dir),
    ]


def execute_cli(
    gls_cli: Path,
    stage: tuple[str, str],
    workspace_root: Path,
    science_store: Path,
    input_manifest: Path,
    run_dir: Path,
    timeout_seconds: float,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cli_command(gls_cli, stage, workspace_root, science_store, input_manifest, run_dir),
        cwd=workspace_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout_seconds,
        check=False,
    )


def validate_cli_identity(
    gls_cli: Path,
    expected_version: str,
    expected_commit: str,
    timeout_seconds: float,
) -> dict[str, str]:
    completed = subprocess.run(
        [str(gls_cli), "--version"],
        cwd=gls_cli.parent,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout_seconds,
        check=False,
    )
    expected = f"gls-cli {expected_version} commit {expected_commit}"
    require(
        completed.returncode == 0 and completed.stdout.strip() == expected,
        f"gls-cli build identity differs: {completed.stdout.strip()} {completed.stderr.strip()}",
    )
    return {
        "version": expected_version,
        "commit": expected_commit,
        "sha256": sha256_file(gls_cli),
    }


def run_isolated(
    session_root: Path,
    label: str,
    original_run: Path,
    contract: RunContract,
    callback: Callable[[Path, Path], dict[str, Any]],
) -> dict[str, Any]:
    case_root, copied_run = copy_case_run(session_root, label, original_run, contract)
    try:
        result = callback(case_root, copied_run)
    finally:
        remove_tree(case_root)
    require(not case_root.exists(), f"isolated run copy was not cleaned: {case_root}")
    result["copy_cleaned"] = True
    return result


def run_baseline_case(
    session_root: Path,
    label: str,
    stage: tuple[str, str],
    original_run: Path,
    contract: RunContract,
    gls_cli: Path,
    workspace_root: Path,
    science_store: Path,
    timeout_seconds: float,
) -> dict[str, Any]:
    def callback(_: Path, copied_run: Path) -> dict[str, Any]:
        completed = execute_cli(
            gls_cli,
            stage,
            workspace_root,
            science_store,
            contract.input_manifest,
            copied_run,
            timeout_seconds,
        )
        require(
            completed.returncode == 0,
            f"clean copied run failed at {' '.join(stage)}: {completed.stderr.strip()}",
        )
        return {
            "id": label,
            "stage": " ".join(stage),
            "exit_code": completed.returncode,
            "stdout": completed.stdout.strip(),
        }

    return run_isolated(session_root, label, original_run, contract, callback)


def authenticity_rejection(run_dir: Path) -> dict[str, str]:
    _, rows = read_tsv(run_dir / "authenticity_gate.tsv", "attack authenticity gate")
    blocked = [row for row in rows if row.get("status") == "blocked"]
    require(len(blocked) == 1, f"expected one blocked authenticity row, observed {len(blocked)}")
    return blocked[0]


def run_attack_case(
    session_root: Path,
    label: str,
    original_run: Path,
    contract: RunContract,
    gls_cli: Path,
    workspace_root: Path,
    science_store: Path,
    timeout_seconds: float,
    mutate: Callable[[Path, Path], Rejection],
) -> dict[str, Any]:
    def callback(case_root: Path, copied_run: Path) -> dict[str, Any]:
        remove_completion_outputs(copied_run)
        rejection = mutate(case_root, copied_run)
        completed = execute_cli(
            gls_cli,
            rejection.stage,
            workspace_root,
            science_store,
            contract.input_manifest,
            copied_run,
            timeout_seconds,
        )
        stderr = completed.stderr.strip()
        require(completed.returncode != 0, f"attack {label} was accepted via {rejection.entrypoint}")
        require(
            stderr == rejection.cli_reason,
            f"attack {label} returned a different CLI reason via {rejection.entrypoint}: {stderr}",
        )
        gate: dict[str, str] | None = None
        if rejection.gate_marker:
            gate = authenticity_rejection(copied_run)
            require(gate.get("artifact") == rejection.gate_artifact, f"attack {label} gate artifact differs")
            require(gate.get("marker") == rejection.gate_marker, f"attack {label} gate marker differs")
            require(
                gate.get("reason") == rejection.provenance_reason,
                f"attack {label} gate reason differs: {gate.get('reason', '')}",
            )
        else:
            require(
                rejection.provenance_reason == stderr,
                f"attack {label} did not expose its precise provenance reason",
            )
        created = completion_outputs(copied_run)
        require(not created, f"attack {label} created completed ranking/report outputs: {created}")
        return {
            "id": label,
            "stage": " ".join(rejection.stage),
            "entrypoint": rejection.entrypoint,
            "exit_code": completed.returncode,
            "cli_reason": stderr,
            "provenance_reason": rejection.provenance_reason,
            "authenticity_gate": gate,
            "completion_outputs_created": created,
        }

    return run_isolated(session_root, label, original_run, contract, callback)


def target_rank_rejection(reason: str) -> Rejection:
    return Rejection(
        stage=("target", "rank"),
        entrypoint=TARGET_RANK_ENTRYPOINT,
        cli_reason="authenticity_gate_failed:forbidden_final_artifact_rows=1",
        provenance_reason=reason,
        gate_artifact="docking_summary.tsv",
        gate_marker="invalid_docking_provenance",
    )


def external_pose_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    selected = contract.docking[0]

    def mutate(case_root: Path, run_dir: Path) -> Rejection:
        outside = case_root / "outside_pose.sdf"
        outside.write_text("external forged pose\n", encoding="utf-8")
        mutate_tsv_candidate(
            run_dir / "docking_summary.tsv",
            selected.candidate_id,
            {"notes": f"pose={outside.resolve()};attack=external_pose"},
        )
        return target_rank_rejection(f"docking_pose_outside_current_run:{selected.candidate_id}")

    return mutate


def tampered_pose_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    selected = contract.docking[0]

    def mutate(_: Path, run_dir: Path) -> Rejection:
        pose = run_dir / selected.pose_sdf
        with pose.open("ab") as handle:
            handle.write(b"\nGENOLIGAND_TAMPERED_POSE\n")
        return target_rank_rejection(
            f"docking_artifact_binding_invalid:{selected.candidate_id}:pose_sdf"
        )

    return mutate


def cross_candidate_manifest_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    source, victim = contract.docking[:2]

    def mutate(_: Path, run_dir: Path) -> Rejection:
        index_path = run_dir / "tool_outputs" / "docking_manifest_index.json"
        index = load_json(index_path, "copied docking manifest index")
        entries = index.get("entries")
        require(isinstance(entries, list), "copied docking entries are invalid")
        victim_entry = next(
            (entry for entry in entries if entry.get("candidate_id") == victim.candidate_id),
            None,
        )
        require(isinstance(victim_entry, dict), "cross-candidate victim entry is missing")
        victim_entry["manifest"] = source.manifest
        write_json(index_path, index)
        result_path = run_dir / "worker_results" / f"{victim.job_id}.json"
        result = load_json(result_path, "cross-candidate worker result")
        result["manifest"] = source.manifest
        write_json(result_path, result)
        return target_rank_rejection(
            f"docking_manifest_identity_mismatch:{victim.candidate_id}:job_id"
        )

    return mutate


def stale_invocation_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    selected = contract.docking[0]

    def mutate(_: Path, run_dir: Path) -> Rejection:
        stale = "invocation_from_previous_run"
        for directory in ("worker_jobs", "worker_results"):
            path = run_dir / directory / f"{selected.job_id}.json"
            value = load_json(path, f"stale invocation {directory}")
            value["invocation_id"] = stale
            write_json(path, value)
        return target_rank_rejection(
            f"docking_candidate_job_binding_invalid:{selected.candidate_id}"
        )

    return mutate


def refresh_provenance_digest(run_dir: Path, md: MdArtifact, artifact_relative: str) -> None:
    path = run_dir / md.provenance
    provenance = load_json(path, "copied MD provenance")
    rows = provenance.get("artifacts")
    require(isinstance(rows, list), "copied MD provenance artifacts are invalid")
    matching = [
        row
        for row in rows
        if isinstance(row, dict)
        and resolve_run_artifact(run_dir, row.get("artifact_path"), "copied MD artifact")[1]
        == artifact_relative
    ]
    require(len(matching) == 1, f"MD provenance row is missing: {artifact_relative}")
    matching[0]["artifact_sha256"] = sha256_file(run_dir / artifact_relative)
    write_json(path, provenance)


def md_rejection(reason: str) -> Rejection:
    return Rejection(
        stage=("dynamics", "screen"),
        entrypoint=DYNAMICS_ENTRYPOINT,
        cli_reason=reason,
        provenance_reason=reason,
    )


def md_artifact_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    md = contract.md

    def mutate(_: Path, run_dir: Path) -> Rejection:
        artifact = run_dir / md.md_hbonds
        with artifact.open("ab") as handle:
            handle.write(b"\nGENOLIGAND_FORGED_MD_ARTIFACT\n")
        return md_rejection(
            f"md_provenance_artifact_digest_mismatch:{md.candidate_id}:{md.md_hbonds}"
        )

    return mutate


def md_unit_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    md = contract.md

    def mutate(_: Path, run_dir: Path) -> Rejection:
        mutate_tsv_candidate(
            run_dir / md.md_rmsd,
            md.candidate_id,
            {"unit": "nanometer"},
        )
        refresh_provenance_digest(run_dir, md, md.md_rmsd)
        return md_rejection(f"md_metric_binding_invalid:{md.candidate_id}:md_rmsd.tsv")

    return mutate


def md_candidate_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    md = contract.md

    def mutate(_: Path, run_dir: Path) -> Rejection:
        mutate_tsv_candidate(
            run_dir / md.md_rmsd,
            md.candidate_id,
            {"candidate_id": "forged_candidate_not_in_current_run"},
        )
        refresh_provenance_digest(run_dir, md, md.md_rmsd)
        return md_rejection(f"md_metric_binding_invalid:{md.candidate_id}:md_rmsd.tsv")

    return mutate


def md_score_attack(contract: RunContract) -> Callable[[Path, Path], Rejection]:
    md = contract.md

    def mutate(_: Path, run_dir: Path) -> Rejection:
        previous = mutate_tsv_candidate(
            run_dir / md.md_summary,
            md.candidate_id,
            {"md_stability_score": "__pending__"},
        )
        try:
            original = float(previous["md_stability_score"])
        except ValueError as error:
            raise RuntimeError("real MD stability score is not numeric") from error
        forged = "0.0000" if original > 0.25 else "0.9900"
        mutate_tsv_candidate(
            run_dir / md.md_summary,
            md.candidate_id,
            {"md_stability_score": forged},
        )
        mutate_tsv_candidate(
            run_dir / "tool_outputs" / "md_summary.tsv",
            md.candidate_id,
            {"md_stability_score": forged},
        )
        refresh_provenance_digest(run_dir, md, md.md_summary)
        return md_rejection(f"md_stability_score_mismatch:{md.candidate_id}")

    return mutate


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        with pending.open("x", encoding="utf-8") as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(pending, path)
    finally:
        pending.unlink(missing_ok=True)


def validate_work_root(workspace_root: Path, raw: Path | None) -> Path:
    allowed = (workspace_root / "work").resolve()
    work_root = raw.resolve() if raw is not None else allowed
    try:
        work_root.relative_to(allowed)
    except ValueError as error:
        raise RuntimeError(f"work root must stay under {allowed}: {work_root}") from error
    work_root.mkdir(parents=True, exist_ok=True)
    return work_root


def path_is_within(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def onsite_gate(
    completed_run: Path,
    workspace_root: Path,
    science_store: Path,
    gls_cli: Path,
    work_root: Path,
    timeout_seconds: float,
    expected_version: str,
    expected_commit: str,
) -> dict[str, Any]:
    original = completed_run.resolve()
    workspace = workspace_root.resolve()
    store = science_store.resolve()
    cli = gls_cli.resolve()
    require(workspace.is_dir(), f"workspace does not exist: {workspace}")
    require(store.is_dir(), f"science store does not exist: {store}")
    require(cli.is_file(), f"gls-cli does not exist: {cli}")
    require(timeout_seconds > 0, "timeout must be positive")
    cli_identity = validate_cli_identity(
        cli, expected_version, expected_commit, timeout_seconds
    )
    before = tree_snapshot(original)
    after: dict[str, Any] = {}
    payload: dict[str, Any]
    try:
        contract = validate_completed_run(original, workspace)
        baselines: list[dict[str, Any]] = []
        attacks: list[dict[str, Any]] = []
        with tempfile.TemporaryDirectory(
            prefix="runtime-provenance-attacks-", dir=work_root
        ) as temporary:
            session_root = Path(temporary)
            baselines.append(
                run_baseline_case(
                    session_root,
                    "baseline_dynamics_screen",
                    ("dynamics", "screen"),
                    original,
                    contract,
                    cli,
                    workspace,
                    store,
                    timeout_seconds,
                )
            )
            baselines.append(
                run_baseline_case(
                    session_root,
                    "baseline_target_rank",
                    ("target", "rank"),
                    original,
                    contract,
                    cli,
                    workspace,
                    store,
                    timeout_seconds,
                )
            )
            for label, mutation in (
                ("docking_external_pose_path", external_pose_attack(contract)),
                ("docking_pose_content_tamper", tampered_pose_attack(contract)),
                ("docking_cross_candidate_manifest", cross_candidate_manifest_attack(contract)),
                ("docking_stale_invocation", stale_invocation_attack(contract)),
                ("md_artifact_content_forgery", md_artifact_attack(contract)),
                ("md_unit_forgery", md_unit_attack(contract)),
                ("md_candidate_forgery", md_candidate_attack(contract)),
                ("md_score_forgery", md_score_attack(contract)),
            ):
                attacks.append(
                    run_attack_case(
                        session_root,
                        label,
                        original,
                        contract,
                        cli,
                        workspace,
                        store,
                        timeout_seconds,
                        mutation,
                    )
                )
        payload = {
            "schema_version": SCHEMA_VERSION,
            "status": "passed",
            "mode": "onsite_real_artifacts",
            "completed_run": str(original),
            "workspace_root": str(workspace),
            "science_store": str(store),
            "gls_cli": str(cli),
            "preflight": {
                "run_id": contract.run_id,
                "project_id": contract.project_id,
                "input_manifest": str(contract.input_manifest),
                "gls_cli_identity": cli_identity,
                "real_completed_vina_candidates": [item.candidate_id for item in contract.docking],
                "real_completed_openmm_candidate": contract.md.candidate_id,
                "baselines": baselines,
            },
            "attacks": attacks,
            "copies_cleaned": True,
            "original_run": {"before": before},
        }
    finally:
        after = tree_snapshot(original)
        require(before == after, f"original completed run changed during attack gate: {original}")
    payload["original_run"]["after"] = after
    payload["original_run"]["unchanged"] = True
    return payload


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="runtime-provenance-self-test-") as temporary:
        root = Path(temporary)
        run = root / "project" / "runs" / "run_self_test"
        (run / "worker_jobs").mkdir(parents=True)
        job = run / "worker_jobs" / "job.json"
        write_json(job, {"job_id": "job", "run_dir": "stale"})
        rebase_worker_jobs(run)
        require(load_json(job, "self-test job")["run_dir"] == str(run.resolve()), "job rebase failed")
        table = run / "metric.tsv"
        write_tsv(
            table,
            ["candidate_id", "value", "unit"],
            [{"candidate_id": "C1", "value": "1.0", "unit": "angstrom"}],
        )
        mutate_tsv_candidate(table, "C1", {"unit": "nanometer"})
        _, rows = read_tsv(table, "self-test metric")
        require(rows[0]["unit"] == "nanometer", "TSV mutation failed")
        snapshot = tree_snapshot(run)
        require(snapshot["file_count"] == 2 and valid_sha256(snapshot["sha256"]), "tree snapshot failed")
        evidence = root / "self_test_evidence.json"
        atomic_json(
            evidence,
            {
                "schema_version": SCHEMA_VERSION,
                "mode": "contract_only_self_test",
                "onsite_cli_executed": False,
            },
        )
        require(load_json(evidence, "self-test evidence")["onsite_cli_executed"] is False, "atomic evidence failed")
    print("runtime_provenance_attacks_self_test PASS mode=contract_only onsite_cli_executed=false")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run provenance attacks against copied real Vina/OpenMM completed artifacts."
    )
    parser.add_argument("--completed-run", type=Path)
    parser.add_argument("--workspace-root", type=Path)
    parser.add_argument("--science-store", type=Path)
    parser.add_argument("--gls-cli", type=Path)
    parser.add_argument("--work-root", type=Path)
    parser.add_argument("--evidence-out", type=Path)
    parser.add_argument("--timeout-seconds", type=float, default=300.0)
    parser.add_argument("--expected-version")
    parser.add_argument("--expected-commit")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    require(args.completed_run is not None, "--completed-run is required")
    require(args.workspace_root is not None, "--workspace-root is required")
    require(args.science_store is not None, "--science-store is required")
    require(args.gls_cli is not None, "--gls-cli is required")
    require(bool(str(args.expected_version or "").strip()), "--expected-version is required")
    require(valid_git_commit(args.expected_commit), "--expected-commit must be a 40-character Git commit")
    workspace = args.workspace_root.resolve()
    work_root = validate_work_root(workspace, args.work_root)
    require(
        not path_is_within(work_root, args.completed_run),
        "work root must stay outside the original completed run",
    )
    evidence = (
        args.evidence_out.resolve()
        if args.evidence_out is not None
        else work_root / "runtime_provenance_attacks_evidence.json"
    )
    require(
        not path_is_within(evidence, args.completed_run),
        "evidence output must stay outside the original completed run",
    )
    payload = onsite_gate(
        args.completed_run,
        workspace,
        args.science_store,
        args.gls_cli,
        work_root,
        args.timeout_seconds,
        args.expected_version,
        args.expected_commit,
    )
    atomic_json(evidence, payload)
    print(f"runtime_provenance_attacks PASS evidence={evidence}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"runtime_provenance_attacks FAIL {error}", file=sys.stderr)
        raise SystemExit(1)
