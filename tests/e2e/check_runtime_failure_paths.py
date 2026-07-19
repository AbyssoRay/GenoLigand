from __future__ import annotations

import argparse
import importlib.util
import json
import os
import stat
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PATH = ROOT / "tests/e2e/prepare_cli_pipeline_fixture.py"
JOB_ID = "mmseqs_onsite_sequence_alignment"
SCHEMA_VERSION = "genoligand_runtime_failure_paths_v1"


PYTHON_WORKER_FAILURE = r'''from __future__ import annotations

import argparse
import json


parser = argparse.ArgumentParser()
parser.add_argument("--job", required=True)
parser.add_argument("--out", required=True)
args = parser.parse_args()
with open(args.job, "r", encoding="utf-8-sig") as handle:
    job = json.load(handle)

reason = "python_worker_fault_injected"
result = {
    "schema_version": "genoligand_worker_current_v2",
    "job_id": job["job_id"],
    "worker": job["worker"],
    "run_id": job["run_id"],
    "invocation_id": job["invocation_id"],
    "status": "FAIL",
    "engine_version": "fault-injection-python-worker;exit=17",
    "execution_status": "failed",
    "reason": reason,
    "outcomes": [{
        "status": "failed",
        "reason": reason,
        "required": True,
        "database_id": job["worker"],
        "row_count": 0,
    }],
    "python_owns_final_score": False,
    "python_owns_report_semantics": False,
    "primitive_execution_only": True,
}
for field in (
    "candidate_id",
    "candidate_digest",
    "input_id",
    "record_id",
    "input_digest",
    "source_digest",
    "receptor_digest",
):
    if field in job:
        result[field] = job[field]
with open(args.out, "w", encoding="utf-8") as handle:
    json.dump(result, handle, sort_keys=True)
raise SystemExit(17)
'''


@dataclass(frozen=True)
class FailureCase:
    name: str
    fault: str
    stage: str
    last_successful_stage: str
    error_code: str
    worker_variant: str = ""


CASES = (
    FailureCase("input_failure", "missing_input_file", "input_normalize", "none", "input_conversion_failed"),
    FailureCase(
        "database_failure",
        "database_output_locked",
        "database_match_known",
        "onsite_sequence_alignment",
        "create_failed",
    ),
    FailureCase(
        "python_worker_failure",
        "python_worker_failure",
        "onsite_sequence_alignment",
        "features_compute",
        "mmseqs_alignment_fail",
        "python_failure",
    ),
    FailureCase(
        "worker_unavailable_query_structure",
        "worker_unavailable",
        "query_structure_prepare",
        "input_normalize",
        "query_structure_prepare_launch_failed",
    ),
    FailureCase(
        "sorting_failure",
        "ranking_output_locked",
        "target_rank",
        "dynamics_screen",
        "write_failed",
    ),
    FailureCase(
        "report_failure",
        "report_output_locked",
        "report_render",
        "evidence_build",
        "write_failed",
    ),
)


class LockedOutput:
    def __init__(self, path: Path) -> None:
        path.write_text("", encoding="utf-8")
        self.path = path
        self.handle: int | None = None
        if os.name == "nt":
            import ctypes
            from ctypes import wintypes

            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            create_file = kernel32.CreateFileW
            create_file.argtypes = (
                wintypes.LPCWSTR,
                wintypes.DWORD,
                wintypes.DWORD,
                wintypes.LPVOID,
                wintypes.DWORD,
                wintypes.DWORD,
                wintypes.HANDLE,
            )
            create_file.restype = wintypes.HANDLE
            handle = create_file(str(path), 0x80000000, 0x1, None, 3, 0x80, None)
            invalid_handle = ctypes.c_void_p(-1).value
            if handle == invalid_handle:
                raise OSError(ctypes.get_last_error(), f"failed to lock output: {path}")
            self.handle = handle
        else:
            path.chmod(stat.S_IRUSR | stat.S_IRGRP | stat.S_IROTH)

    def close(self) -> None:
        if self.handle is not None:
            import ctypes
            from ctypes import wintypes

            close_handle = ctypes.WinDLL("kernel32", use_last_error=True).CloseHandle
            close_handle.argtypes = (wintypes.HANDLE,)
            close_handle.restype = wintypes.BOOL
            if not close_handle(self.handle):
                raise OSError(ctypes.get_last_error(), f"failed to unlock output: {self.path}")
            self.handle = None
        elif self.path.exists():
            self.path.chmod(stat.S_IRUSR | stat.S_IWUSR)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8-sig"))
    require(isinstance(value, dict), f"JSON object required: {path}")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def load_fixture_module() -> Any:
    spec = importlib.util.spec_from_file_location("runtime_failure_fixture", FIXTURE_PATH)
    if spec is None or spec.loader is None:
        raise AssertionError("pipeline fixture module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def draft_2020_12_validator(schema_name: str) -> Any:
    try:
        from jsonschema import Draft202012Validator
    except ImportError as error:
        raise AssertionError("jsonschema with Draft 2020-12 support is required") from error
    schema = load_json(ROOT / "schemas" / schema_name)
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
        f"{schema_name} is not Draft 2020-12",
    )
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def validate_schema(validator: Any, value: dict[str, Any], label: str) -> None:
    errors = sorted(validator.iter_errors(value), key=lambda error: list(error.absolute_path))
    require(
        not errors,
        f"{label} schema errors: "
        + " | ".join(f"{'.'.join(map(str, error.absolute_path)) or '<root>'}: {error.message}" for error in errors),
    )


def parse_failure(path: Path) -> dict[str, str]:
    rows: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        key, separator, value = line.partition("=")
        if separator:
            rows[key] = value
    return rows


def prepare_case(
    fixture: Any,
    root: Path,
    case: FailureCase,
    python: Path,
) -> tuple[dict[str, str], LockedOutput | None]:
    fixture.prepare(root, "ready")
    run_dir = root / "run"
    run_dir.mkdir()
    environment = os.environ.copy()
    environment["GENOLIGAND_PYTHON"] = str(python)

    locked_output = None
    if case.fault == "missing_input_file":
        write_json(
            root / "input_manifest.json",
            {
                "inputs": [{"field": "project_name", "value": "failure-path"}],
                "files": [
                    {
                        "field": "protein_ligand",
                        "name": "deleted-before-run.fasta",
                        "persisted_path": str(root / "deleted-before-run.fasta"),
                    }
                ],
            },
        )
    elif case.fault == "database_output_locked":
        locked_output = LockedOutput(run_dir / "known_database_matches.tsv")
    elif case.fault == "python_worker_failure":
        (root / "python/genoligand_workers/alignment_worker.py").write_text(
            PYTHON_WORKER_FAILURE,
            encoding="utf-8",
        )
    elif case.fault == "worker_unavailable":
        environment["GENOLIGAND_PYTHON"] = str(root / "missing-python.exe")
    elif case.fault == "ranking_output_locked":
        locked_output = LockedOutput(run_dir / "target_candidate_ranking.tsv")
    elif case.fault == "report_output_locked":
        locked_output = LockedOutput(run_dir / "research_report.md")
    else:
        raise AssertionError(f"unsupported fault: {case.fault}")
    return environment, locked_output


def validate_worker_pair(
    run_dir: Path,
    variant: str,
    job_validator: Any,
    result_validator: Any,
) -> dict[str, Any]:
    job_path = run_dir / "worker_jobs" / f"{JOB_ID}.json"
    result_path = run_dir / "worker_results" / f"{JOB_ID}.json"
    require(job_path.is_file(), f"worker job missing: {job_path}")
    require(result_path.is_file(), f"worker result missing: {result_path}")
    job = load_json(job_path)
    result = load_json(result_path)
    validate_schema(job_validator, job, f"{variant} job")
    validate_schema(result_validator, result, f"{variant} result")
    for field in ("job_id", "worker", "run_id", "invocation_id"):
        require(result.get(field) == job.get(field), f"{variant} binding mismatch: {field}")
    require(result.get("status") == "FAIL", f"{variant} result must fail")
    require(result.get("execution_status") == "failed", f"{variant} execution status")
    engine = str(result.get("engine_version", ""))
    if variant == "python_failure":
        require(engine == "fault-injection-python-worker;exit=17", "Python worker process evidence missing")
        require(result.get("reason") == "python_worker_fault_injected", "Python failure reason changed")
    else:
        require(engine.startswith("worker unavailable:"), "Rust worker-unavailable fallback evidence missing")
        require("launch_failed" in str(result.get("reason", "")), "fallback launch reason missing")
    return {
        "variant": variant,
        "job": str(job_path),
        "result": str(result_path),
        "worker": job["worker"],
        "job_id": job["job_id"],
        "invocation_id": job["invocation_id"],
        "engine_version": engine,
        "reason": result["reason"],
        "draft_2020_12": "PASS",
        "binding": "PASS",
    }


def run_case(
    cli: Path,
    python: Path,
    artifact_root: Path,
    fixture: Any,
    case: FailureCase,
    job_validator: Any,
    result_validator: Any,
) -> dict[str, Any]:
    root = artifact_root / "cases" / case.name
    root.mkdir(parents=True)
    environment, locked_output = prepare_case(fixture, root, case, python)
    run_dir = root / "run"
    command = [
        str(cli),
        "pipeline",
        "run",
        "--workspace-root",
        str(root),
        "--science-store",
        str(root / "science_store"),
        "--input-manifest",
        str(root / "input_manifest.json"),
        "--run-dir",
        str(run_dir),
    ]
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=120,
            check=False,
        )
    finally:
        if locked_output is not None:
            locked_output.close()
    (root / "cli.stdout.txt").write_text(completed.stdout, encoding="utf-8")
    (root / "cli.stderr.txt").write_text(completed.stderr, encoding="utf-8")
    require(completed.returncode != 0, f"{case.name} unexpectedly exited zero")
    require((run_dir / "run_state.txt").read_text(encoding="utf-8") == "failed\n", f"{case.name} state")
    failure = parse_failure(run_dir / "failure_reason.txt")
    require(failure.get("schema_version") == "genoligand_pipeline_failure_v1", f"{case.name} failure schema")
    require(failure.get("state") == "failed", f"{case.name} failure state")
    require(failure.get("error_code") == case.error_code, f"{case.name} error code: {failure}")
    require(failure.get("stage") == case.stage, f"{case.name} stage: {failure}")
    require(
        failure.get("last_successful_stage") == case.last_successful_stage,
        f"{case.name} last successful stage: {failure}",
    )
    require(bool(failure.get("invocation_id")), f"{case.name} invocation missing")
    require(bool(failure.get("repair_action")), f"{case.name} repair action missing")
    require(not (run_dir / "pipeline_done.txt").exists(), f"{case.name} created done marker")
    context = load_json(run_dir / "run_context.json")
    require(context.get("state") == "failed", f"{case.name} run context state")
    require(
        context.get("pipeline_invocation_id") == failure["invocation_id"],
        f"{case.name} invocation binding",
    )
    worker = None
    if case.worker_variant:
        worker = validate_worker_pair(run_dir, case.worker_variant, job_validator, result_validator)
    return {
        "case": case.name,
        "exit_code": completed.returncode,
        "state": failure["state"],
        "error_code": failure["error_code"],
        "stage": failure["stage"],
        "last_successful_stage": failure["last_successful_stage"],
        "invocation_id": failure["invocation_id"],
        "repair_action": failure["repair_action"],
        "done_marker": False,
        "stdout": str(root / "cli.stdout.txt"),
        "stderr": str(root / "cli.stderr.txt"),
        "failure_reason": str(run_dir / "failure_reason.txt"),
        "worker_contract": worker,
    }


def select_cli(value: Path | None) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    candidates = [value] if value is not None else [
        ROOT / f"target/release/gls-cli{suffix}",
        ROOT / f"target/debug/gls-cli{suffix}",
    ]
    selected = next((path.resolve() for path in candidates if path is not None and path.is_file()), None)
    if selected is None:
        raise AssertionError("built gls-cli is required; pass --gls-cli")
    return selected


def execute(cli: Path, python: Path, artifact_root: Path) -> dict[str, Any]:
    fixture = load_fixture_module()
    job_validator = draft_2020_12_validator("worker_job.schema.json")
    result_validator = draft_2020_12_validator("worker_result.schema.json")
    results = [
        run_case(cli, python, artifact_root, fixture, case, job_validator, result_validator)
        for case in CASES
    ]
    evidence = {
        "schema_version": SCHEMA_VERSION,
        "status": "PASS",
        "cli": str(cli),
        "python": str(python),
        "cases": results,
        "covered_stages": sorted({result["stage"] for result in results}),
        "worker_contract_variants": sorted(
            result["worker_contract"]["variant"]
            for result in results
            if result["worker_contract"] is not None
        ),
    }
    write_json(artifact_root / "runtime_failure_paths.json", evidence)
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--gls-cli", type=Path)
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--artifact-dir", type=Path, default=ROOT / "work/runtime-failure-paths")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    cli = select_cli(args.gls_cli)
    python = args.python.resolve()
    require(python.is_file(), f"Python executable missing: {python}")
    if args.self_test:
        with tempfile.TemporaryDirectory(prefix="genoligand-runtime-failures-") as tmp:
            evidence = execute(cli, python, Path(tmp))
    else:
        artifact_root = args.artifact_dir.resolve()
        require(not artifact_root.exists(), f"artifact directory already exists: {artifact_root}")
        artifact_root.mkdir(parents=True)
        evidence = execute(cli, python, artifact_root)
    print(
        "runtime_failure_paths\tPASS\t"
        f"cases={len(evidence['cases'])}\t"
        f"stages={','.join(evidence['covered_stages'])}\t"
        f"worker_variants={','.join(evidence['worker_contract_variants'])}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
