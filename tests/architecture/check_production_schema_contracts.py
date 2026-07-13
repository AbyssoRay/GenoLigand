from __future__ import annotations

import copy
import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_PATHS = (
    "schemas/project.schema.json",
    "schemas/run.schema.json",
    "schemas/artifact.schema.json",
    "schemas/install_status.schema.json",
)


def load_json(relative_path: str) -> dict[str, object]:
    return json.loads((ROOT / relative_path).read_text(encoding="utf-8"))


def local_schema_registry() -> Registry:
    registry = Registry()
    for schema_path in SCHEMA_PATHS:
        schema = load_json(schema_path)
        schema_id = schema.get("$id")
        if not isinstance(schema_id, str):
            raise AssertionError(f"schema has no string $id: {schema_path}")
        registry = registry.with_resource(schema_id, Resource.from_contents(schema))
    return registry


def load_installer() -> ModuleType:
    path = ROOT / "scripts" / "setup" / "install_complete_scientific.py"
    spec = importlib.util.spec_from_file_location("install_complete_scientific", path)
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot import installer: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def production_project() -> dict[str, object]:
    # Faithful to gls-server read_project.
    return {
        "project_id": "project-1710000000000",
        "project_name": "CDK2 ligand screen",
        "project_status": "completed",
        "latest_run_id": "run-1710000000001",
    }


def production_artifact() -> dict[str, object]:
    # Faithful to one artifact_index item.
    return {
        "id": "candidate_ranking",
        "label": "Candidate ranking",
        "exists": True,
    }


def production_run() -> dict[str, object]:
    # Faithful to run_json plus the fields added by user_run_json.
    artifact = production_artifact()
    return {
        "run_id": "run-1710000000001",
        "state": "completed",
        "current_stage": "Evidence and report",
        "failure_summary": "",
        "trace": {
            "project_label": "CDK2 ligand screen",
            "run_label": "Current run",
            "input_status": "User input saved",
            "database_status": "Science store ready",
            "output_status": "Output generated",
            "output_count": 1,
            "candidate_status": "Candidate ranking generated",
            "candidate_count": 1,
            "evidence_status": "Evidence ledger generated",
            "evidence_count": 1,
        },
        "outputs": {"items": [artifact]},
        "state_label": "Completed",
        "stages": [
            {
                "id": "evidence",
                "label": "Evidence and report",
                "status": "ready",
                "status_label": "Ready",
                "reason": "",
            }
        ],
    }


def production_install_status() -> dict[str, object]:
    installer = load_installer()
    payload = {
        "status": "ready",
        "install_ready": True,
        "summary": "Science store is ready.",
        "next_action": "Open WebUI",
        "access_url": "http://127.0.0.1:19151/",
        "progress": {
            "status": "ready",
            "stage": "Final checks",
            "current_resource": "Installation",
            "completed_steps": 6,
            "total_steps": 6,
            "failed_steps": 0,
            "bytes_downloaded": 1024,
            "bytes_total": 1024,
            "percent": 100,
            "message": "Science store is ready.",
            "updated_at": 1710000000,
        },
        "downloads": {
            "status": "ready",
            "ready": 8,
            "failed": 0,
            "message": "Downloads completed.",
            "next_action": "Continue",
        },
        "external_sources": {
            "status": "ready",
            "ready": 2,
            "total": 2,
            "needs_action": 0,
            "message": "External sources checked.",
            "next_action": "Continue",
            "items": [
                {
                    "label": "PLINDER",
                    "status": "Ready",
                    "ready": True,
                    "next_action": "Continue",
                }
            ],
        },
        "resources": {
            "status": "ready",
            "ready": 8,
            "total": 8,
            "needs_action": 0,
            "items": [
                {
                    "label": "ChEMBL",
                    "status": "Ready",
                    "ready": True,
                    "next_action": "Continue",
                }
            ],
        },
        "source_manifests": {
            "total": 8,
            "ready": 8,
            "hash_ready": 8,
            "license_ready": 8,
        },
        "tool_environment": {"status": "ready", "ready": 6, "total": 6},
        "cuda_toolkit": {
            "status": "ready",
            "path": "C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v12.8",
            "nvcc": "present",
            "notes": "CUDA Toolkit path detected",
        },
        "alignment_databases": {
            "status": "ready",
            "ready": 4,
            "deferred": 0,
            "total": 4,
        },
        "forcefields": {"status": "ready"},
        "runtime_binaries": {
            "gls_cli": {"status": "ready"},
            "gls_server": {"status": "ready"},
        },
        "launch_environment": {"launcher": "C:/GenoLigand/start-genoligand-webui.cmd"},
    }
    return installer.install_user_summary(payload)


def existing_install_fixture() -> dict[str, object]:
    installer = load_installer()
    return installer.install_user_summary(
        {
            "status": "ready",
            "install_ready": True,
            "summary": "ready",
            "next_action": "continue",
            "source_manifests": {
                "total": 1,
                "ready": 1,
                "hash_ready": 1,
                "license_ready": 1,
            },
            "tool_environment": {"status": "ready", "ready": 12, "total": 12},
            "runtime_binaries": {
                "gls_cli": {"status": "ready"},
                "gls_server": {"status": "ready"},
            },
            "launch_environment": {
                "launcher": "D:/hidden/start-genoligand-webui.cmd"
            },
            "alignment_databases": {
                "status": "needs_repair",
                "ready": 2,
                "deferred": 1,
                "total": 3,
            },
        }
    )


def validate_contract(schema_path: str, instance: dict[str, object]) -> None:
    schema = load_json(schema_path)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, registry=local_schema_registry())
    errors = sorted(validator.iter_errors(instance), key=lambda error: list(error.path))
    if errors:
        details = "; ".join(error.message for error in errors)
        raise AssertionError(f"{schema_path} rejects its production object: {details}")

    unexpected = copy.deepcopy(instance)
    unexpected["unexpected_contract_field"] = True
    if not list(validator.iter_errors(unexpected)):
        raise AssertionError(f"{schema_path} must reject unknown top-level fields")


def main() -> int:
    contracts = [
        ("schemas/project.schema.json", production_project()),
        ("schemas/run.schema.json", production_run()),
        ("schemas/artifact.schema.json", production_artifact()),
        ("schemas/install_status.schema.json", production_install_status()),
        ("schemas/install_status.schema.json", existing_install_fixture()),
    ]
    for schema_path, instance in contracts:
        validate_contract(schema_path, instance)
    print("production_schema_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
