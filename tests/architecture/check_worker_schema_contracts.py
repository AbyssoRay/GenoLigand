from __future__ import annotations

import copy
import json
import sys
import tempfile
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[2]
CAPTURE_ROOT = ROOT / "tests" / "architecture" / "fixtures" / "worker_contracts"
sys.path.insert(0, str(ROOT / "python"))

from genoligand_workers.worker_io import write_result  # noqa: E402


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"{path} is not a JSON object")
    return value


def load_capture(name: str) -> dict[str, Any]:
    return load_json(CAPTURE_ROOT / name)


def validator(name: str) -> Draft202012Validator:
    schema = load_json(ROOT / "schemas" / name)
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        raise AssertionError(f"{name} does not declare JSON Schema 2020-12")
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def validation_errors(contract: Draft202012Validator, payload: object) -> list[str]:
    return [
        f"{error.json_path}: {error.message}"
        for error in sorted(contract.iter_errors(payload), key=lambda item: list(item.path))
    ]


def assert_valid(contract: Draft202012Validator, payload: object, label: str) -> None:
    errors = validation_errors(contract, payload)
    if errors:
        raise AssertionError(f"{label} rejected: {'; '.join(errors)}")


def assert_invalid(contract: Draft202012Validator, payload: object, label: str) -> None:
    if contract.is_valid(payload):
        raise AssertionError(f"{label} was accepted")


def source_has_symbols(source: str, language: str, symbols: list[str]) -> None:
    path = ROOT / source
    text = path.read_text(encoding="utf-8")
    prefix = "def " if language == "Python" else "fn "
    for symbol in symbols:
        if f"{prefix}{symbol}(" not in text:
            raise AssertionError(f"capture producer {source} no longer defines {symbol}")


def assert_declared_producer(producer: dict[str, Any]) -> None:
    language = producer.get("language")
    source = producer.get("source")
    symbols = producer.get("symbols")
    if (
        language not in {"Python", "Rust"}
        or not isinstance(source, str)
        or not isinstance(symbols, list)
        or not all(isinstance(symbol, str) for symbol in symbols)
    ):
        raise AssertionError("capture producer metadata is incomplete")
    source_has_symbols(source, language, symbols)
    for postprocessor in producer.get("postprocessors", []):
        source_name, separator, symbol = str(postprocessor).partition(":")
        if not separator:
            raise AssertionError(f"invalid capture postprocessor: {postprocessor}")
        source_has_symbols(source_name, "Python", [symbol])


def captured_jobs(capture: dict[str, Any], field: str) -> list[dict[str, Any]]:
    value = capture.get(field)
    if not isinstance(value, list) or not all(isinstance(item, dict) for item in value):
        raise AssertionError(f"capture field {field} is not a job list")
    return value


def validate_job_captures(
    contract: Draft202012Validator,
) -> dict[str, dict[str, Any]]:
    alignment_capture = load_capture("alignment_jobs.capture.json")
    assert_declared_producer(alignment_capture.get("producer", {}))
    alignment_jobs = captured_jobs(alignment_capture, "jobs")

    top_n_capture = load_capture("top_n_jobs.capture.json")
    top_n_producer = top_n_capture.get("producer", {})
    if (
        top_n_producer.get("crate") != "gls-science"
        or top_n_producer.get("job_builder") != "candidate_worker_job_value"
    ):
        raise AssertionError("top-N capture lost its Rust producer identity")
    source_has_symbols(
        "crates/gls-science/src/lib.rs", "Rust", ["candidate_worker_job_value"]
    )
    docking_jobs = captured_jobs(top_n_capture, "docking_jobs")
    md_jobs = captured_jobs(top_n_capture, "md_jobs")
    if len(docking_jobs) != 3 or len(md_jobs) != 3:
        raise AssertionError("Rust top-N capture must contain three docking and MD jobs")

    bound_capture = load_capture("md_job_with_docking_binding.capture.json")
    bound_producer = bound_capture.get("producer", {})
    bound_md_job = bound_capture.get("job")
    if (
        bound_producer.get("job_builder") != "candidate_worker_job_value"
        or not isinstance(bound_md_job, dict)
    ):
        raise AssertionError("bound MD capture lost its Rust producer identity")

    jobs = alignment_jobs + docking_jobs + md_jobs + [bound_md_job]
    for index, job in enumerate(jobs, start=1):
        label = f"Rust production job capture {index} ({job.get('worker')})"
        assert_valid(contract, job, label)
        missing_invocation = copy.deepcopy(job)
        missing_invocation.pop("invocation_id", None)
        assert_invalid(contract, missing_invocation, f"{label} without invocation_id")
        unknown = copy.deepcopy(job)
        unknown["unknown_contract_field"] = True
        assert_invalid(contract, unknown, f"{label} with unknown field")

    docking_candidates = [job.get("candidate_id") for job in docking_jobs]
    md_candidates = [job.get("candidate_id") for job in md_jobs]
    if len(set(docking_candidates)) != 3 or docking_candidates != md_candidates:
        raise AssertionError("captured docking and MD jobs lost their top-N identities")
    for worker, worker_jobs, prefix in (
        ("docking", docking_jobs, "autodock_vina_complex_docking__"),
        ("md_summary", md_jobs, "md_summary_ligand_dynamics__"),
    ):
        if len({job.get("job_id") for job in worker_jobs}) != 3:
            raise AssertionError(f"{worker} jobs do not have independent job IDs")
        if len({job.get("invocation_id") for job in worker_jobs}) != 3:
            raise AssertionError(f"{worker} jobs do not have independent invocations")
        for job in worker_jobs:
            candidate_id = str(job.get("candidate_id", ""))
            if (
                job.get("worker") != worker
                or job.get("job_id") != f"{prefix}{candidate_id}"
                or job.get("identity_namespace") != "uniprot"
                or candidate_id not in str(job.get("receptor_artifact", ""))
            ):
                raise AssertionError(f"captured {worker} job lost candidate binding")
            for field in (
                "candidate_id",
                "candidate_digest",
                "input_id",
                "record_id",
                "input_digest",
                "source_digest",
                "receptor_artifact",
                "receptor_digest",
            ):
                invalid = copy.deepcopy(job)
                invalid.pop(field)
                assert_invalid(contract, invalid, f"{worker} job without {field}")

    candidate_id = str(bound_md_job.get("candidate_id", ""))
    docking_invocation = str(bound_md_job.get("docking_invocation_id", ""))
    docking_manifest = str(bound_md_job.get("docking_manifest", "")).replace("\\", "/")
    if (
        candidate_id not in md_candidates
        or bound_md_job.get("docking_job_id")
        != f"autodock_vina_complex_docking__{candidate_id}"
        or docking_invocation == bound_md_job.get("invocation_id")
        or f"/{candidate_id}/{docking_invocation}/docking_manifest.json"
        not in f"/{docking_manifest}"
    ):
        raise AssertionError("captured MD job lost docking producer binding")
    for field in (
        "docking_manifest_digest",
        "docking_job_id",
        "docking_invocation_id",
    ):
        invalid = copy.deepcopy(bound_md_job)
        invalid.pop(field)
        assert_invalid(contract, invalid, f"bound MD job without {field}")
    invalid_policy = copy.deepcopy(bound_md_job)
    invalid_policy["platform_policy"] = "auto"
    assert_invalid(contract, invalid_policy, "MD job with ambiguous platform policy")

    return {
        "mmseqs_alignment": alignment_jobs[0],
        "foldseek_alignment": alignment_jobs[1],
        "docking": docking_jobs[0],
        "md_summary": bound_md_job,
    }


def reproduce_python_result_core(
    payload: dict[str, Any], job: dict[str, Any], label: str
) -> None:
    with tempfile.TemporaryDirectory() as temporary:
        output = Path(temporary) / "result.json"
        write_result(
            output,
            job,
            str(payload["status"]),
            str(payload["engine_version"]),
            execution_status=str(payload["execution_status"]),
            reason=str(payload["reason"]),
            outcomes=payload["outcomes"],
        )
        reproduced = load_json(output)
    for field, value in reproduced.items():
        if payload.get(field) != value:
            raise AssertionError(f"{label} differs from worker_io.write_result at {field}")


def validate_result_captures(
    contract: Draft202012Validator, jobs: dict[str, dict[str, Any]]
) -> None:
    capture = load_capture("worker_results.capture.json")
    producers = capture.get("producers")
    if not isinstance(producers, list):
        raise AssertionError("result capture producer list is missing")
    producer_ids: set[str] = set()
    for producer in producers:
        if not isinstance(producer, dict) or not isinstance(producer.get("id"), str):
            raise AssertionError("result capture producer metadata is invalid")
        assert_declared_producer(producer)
        producer_ids.add(producer["id"])
    if producer_ids != {"python", "rust_fallback"}:
        raise AssertionError("result captures must declare Python and Rust fallback producers")

    captures = capture.get("captures")
    if not isinstance(captures, list):
        raise AssertionError("result capture list is missing")
    seen: dict[tuple[str, str, str], dict[str, Any]] = {}
    for item in captures:
        if not isinstance(item, dict) or not isinstance(item.get("payload"), dict):
            raise AssertionError("result capture entry is invalid")
        payload = item["payload"]
        label = str(item.get("case", "unnamed result capture"))
        producer = str(item.get("producer", ""))
        assert_valid(contract, payload, label)
        unknown = copy.deepcopy(payload)
        unknown["unknown_contract_field"] = True
        assert_invalid(contract, unknown, f"{label} with unknown field")
        missing_outcomes = copy.deepcopy(payload)
        missing_outcomes.pop("outcomes", None)
        assert_invalid(contract, missing_outcomes, f"{label} without outcomes")
        worker = str(payload.get("worker", ""))
        status = str(payload.get("status", ""))
        key = (producer, worker, status)
        if key in seen:
            raise AssertionError(f"duplicate result capture {key}")
        seen[key] = payload
        if producer == "python":
            reproduce_python_result_core(payload, jobs[worker], label)
        elif producer == "rust_fallback":
            optional_bindings = (
                "candidate_id",
                "candidate_digest",
                "input_id",
                "record_id",
                "input_digest",
                "source_digest",
                "receptor_digest",
            )
            if any(field in payload and not str(payload[field]).strip() for field in optional_bindings):
                raise AssertionError(f"{label} serialized an empty optional binding")
        else:
            raise AssertionError(f"{label} has unknown producer {producer}")

    workers = {"mmseqs_alignment", "foldseek_alignment", "docking", "md_summary"}
    expected = {
        *(('python', worker, status) for worker in workers for status in ("PASS", "FAIL")),
        *(('rust_fallback', worker, "FAIL") for worker in workers),
    }
    if set(seen) != expected:
        raise AssertionError(f"result capture matrix incomplete: {expected - set(seen)}")

    docking_success = seen[("python", "docking", "PASS")]
    md_success = seen[("python", "md_summary", "PASS")]
    for field in (
        "candidate_id",
        "candidate_digest",
        "input_id",
        "record_id",
        "input_digest",
        "source_digest",
        "receptor_digest",
        "manifest",
    ):
        invalid = copy.deepcopy(docking_success)
        invalid.pop(field)
        assert_invalid(contract, invalid, f"docking success without {field}")
    for field in (
        "candidate_id",
        "candidate_digest",
        "input_id",
        "record_id",
        "input_digest",
        "source_digest",
        "artifact_path",
        "manifest",
        "artifact_provenance",
    ):
        invalid = copy.deepcopy(md_success)
        invalid.pop(field)
        assert_invalid(contract, invalid, f"MD success without {field}")


def main() -> int:
    job_contract = validator("worker_job.schema.json")
    result_contract = validator("worker_result.schema.json")
    jobs = validate_job_captures(job_contract)
    validate_result_captures(result_contract, jobs)
    print("worker_schema_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
