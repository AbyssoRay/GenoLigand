from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


PIPELINE_MODE = "genoligand_scientific_pipeline"
SCHEMA_VERSION = "genoligand_worker_current_v1"


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
    return job


def write_result(path: str | Path, job: dict[str, Any], status: str, version: str) -> None:
    result = {
        "schema_version": SCHEMA_VERSION,
        "job_id": job["job_id"],
        "worker": job["worker"],
        "status": status,
        "engine_version": version,
        "python_owns_final_score": False,
        "python_owns_report_semantics": False,
        "primitive_execution_only": True,
    }
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as handle:
        json.dump(result, handle, indent=2, sort_keys=True)
        handle.write("\n")
