from __future__ import annotations

import argparse
import hashlib
import json
import os
from collections.abc import Mapping
from pathlib import Path
from typing import Any


PIPELINE_MODE = "genoligand_scientific_pipeline"
SCHEMA_VERSION = "genoligand_worker_current_v2"
CANDIDATE_SELECTION_DIGEST_FIELDS = (
    "candidate_id",
    "identity_namespace",
    "input_id",
    "record_id",
    "target_id",
    "pdb_id",
    "selection_rank",
    "receptor_artifact",
    "coordinate_sha256",
)


def candidate_selection_digest(selection: Mapping[str, Any]) -> str:
    payload = {
        "schema_version": "genoligand_candidate_selection_digest_v1",
        **{
            field: str(selection.get(field, "") or "").strip()
            for field in CANDIDATE_SELECTION_DIGEST_FIELDS
        },
    }
    encoded = json.dumps(
        payload,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def runtime_artifact_path(value: str | Path) -> Path:
    raw = str(value)
    path = Path(raw)
    if path.is_absolute():
        return path
    if os.name != "nt" and len(raw) >= 3 and raw[1] == ":" and raw[2] in "\\/":
        drive = raw[0].lower()
        rest = raw[3:].replace("\\", "/")
        return Path(f"/mnt/{drive}/{rest}")
    return Path(raw.replace("\\", "/"))


def parse_worker_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--job", required=True)
    parser.add_argument("--out", required=True)
    return parser.parse_args()


def read_job(path: str | Path) -> dict[str, Any]:
    with Path(path).open("r", encoding="utf-8-sig") as handle:
        job = json.load(handle)
    if job.get("schema_version") != SCHEMA_VERSION:
        raise ValueError("unsupported worker job schema_version")
    if job.get("pipeline_mode") != PIPELINE_MODE:
        raise ValueError("unsupported pipeline_mode")
    if job.get("rust_owned_decision_logic") is not True:
        raise ValueError("worker job must declare Rust-owned decision logic")
    for field_name in ("run_id", "invocation_id"):
        if not str(job.get(field_name, "")).strip():
            raise ValueError(f"worker job must declare {field_name}")
    if job.get("worker") == "md_summary" and job.get("platform_policy") not in {
        "required",
        "preferred",
        "disabled",
    }:
        raise ValueError("md_summary job must declare platform_policy")
    return job


def write_result(
    path: str | Path,
    job: dict[str, Any],
    status: str,
    version: str,
    *,
    execution_status: str | None = None,
    reason: str | None = None,
    outcomes: list[dict[str, Any]] | None = None,
) -> None:
    if execution_status is None:
        execution_status = "completed" if status == "PASS" else "failed"
    if reason is None:
        reason = execution_status
    if outcomes is None:
        outcomes = [
            {
                "status": execution_status,
                "reason": reason,
                "required": True,
                "database_id": str(job["worker"]),
                "row_count": 0,
            }
        ]
    result = {
        "schema_version": SCHEMA_VERSION,
        "job_id": job["job_id"],
        "worker": job["worker"],
        "run_id": str(job.get("run_id", "")).strip() or Path(str(job["run_dir"])).name,
        "invocation_id": str(job.get("invocation_id", "")).strip(),
        "status": status,
        "engine_version": version,
        "execution_status": execution_status,
        "reason": reason,
        "outcomes": outcomes,
        "python_owns_final_score": False,
        "python_owns_report_semantics": False,
        "primitive_execution_only": True,
    }
    for field_name in (
        "candidate_id",
        "candidate_digest",
        "input_id",
        "record_id",
        "input_digest",
        "source_digest",
        "receptor_digest",
    ):
        value = str(job.get(field_name, "")).strip()
        if value:
            result[field_name] = value
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as handle:
        json.dump(result, handle, indent=2, sort_keys=True)
        handle.write("\n")
