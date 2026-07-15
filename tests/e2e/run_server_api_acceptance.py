from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import math
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_VERSION = "genoligand_server_api_acceptance_v1"
TERMINAL_STATES = {"completed", "blocked", "failed", "cancelled"}
REQUIRED_WORKERS = {
    "mmseqs_alignment",
    "foldseek_alignment",
    "docking",
    "md_summary",
}
REQUIRED_INPUT_KINDS = {
    "small_molecule",
    "peptide",
    "protein_ligand",
    "complex",
    "genome_or_metagenome",
}
REQUIRED_FORMAT_CASES = {"smiles", "fasta", "fastq", "genbank", "pdb"}
HEX_SHA256 = set("0123456789abcdef")
CANDIDATE_EXPORT_FIELDS = (
    ("rank", "排序"), ("candidate_id", "候选编号"), ("target_id", "目标编号"),
    ("target_name", "目标名称"), ("candidate_scope_label", "候选范围"), ("organism", "物种"),
    ("final_score", "综合分数"), ("known_database_score", "已知库分数"),
    ("metagenome_score", "宏基因组分数"), ("structure_score", "结构分数"),
    ("docking_score", "对接分数"), ("md_score", "动力学分数"),
    ("md_support_label", "动力学支持"), ("risk_note", "风险提示"),
    ("evidence_summary", "证据摘要"), ("status_label", "状态"), ("result_source", "输出材料"),
)
EVIDENCE_EXPORT_FIELDS = (
    ("evidence_type", "证据类型"), ("source", "来源"), ("status", "状态"),
    ("summary", "说明"), ("result_source", "输出材料"),
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


@dataclass(frozen=True)
class InputCase:
    label: str
    expected_kind: str
    manifest: dict[str, Any]
    acceptance_profile: str = ""


class ApiClient:
    def __init__(self, base_url: str, timeout: float) -> None:
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def request(
        self,
        method: str,
        path: str,
        payload: dict[str, Any] | None = None,
    ) -> tuple[int, str, bytes]:
        data = None
        headers = {"Accept": "application/json"}
        if payload is not None:
            data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
            headers["Content-Type"] = "application/json; charset=utf-8"
        request = urllib.request.Request(
            f"{self.base_url}{path}", data=data, headers=headers, method=method
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                return (
                    response.status,
                    response.headers.get("Content-Type", ""),
                    response.read(),
                )
        except urllib.error.HTTPError as error:
            return error.code, error.headers.get("Content-Type", ""), error.read()

    def json(
        self,
        method: str,
        path: str,
        payload: dict[str, Any] | None = None,
        expected_status: int = 200,
    ) -> dict[str, Any]:
        status, _, body = self.request(method, path, payload)
        require(status == expected_status, f"{method} {path} returned HTTP {status}: {body!r}")
        try:
            value = json.loads(body.decode("utf-8-sig"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise AssertionError(f"{method} {path} returned invalid JSON: {error}") from error
        require(isinstance(value, dict), f"{method} {path} must return a JSON object")
        return value


def file_manifest(project_name: str, field: str, path: Path) -> dict[str, Any]:
    return {
        "inputs": [{"field": "project_name", "value": project_name}],
        "files": [
            {
                "field": field,
                "name": path.name,
                "persisted_path": str(path.resolve()),
            }
        ],
    }


def write_input_fixtures(root: Path) -> list[InputCase]:
    root.mkdir(parents=True, exist_ok=True)
    acetazolamide_sdf = root / "acetazolamide-multirecord.sdf"
    acetazolamide_block = """Acetazolamide
  GenoLigand

 13 13  0  0  0  0  0  0  0  0999 V2000
    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
    1.5000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
    2.8000    0.0000    0.0000 O   0  0  0  0  0  0  0  0  0  0  0  0
    1.5000    1.5000    0.0000 N   0  0  0  0  0  0  0  0  0  0  0  0
    2.8000    2.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
    3.9000    1.2000    0.0000 N   0  0  0  0  0  0  0  0  0  0  0  0
    5.0000    2.0000    0.0000 N   0  0  0  0  0  0  0  0  0  0  0  0
    4.6000    3.3000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
    3.2000    3.3000    0.0000 S   0  0  0  0  0  0  0  0  0  0  0  0
    5.7000    4.2000    0.0000 S   0  0  0  0  0  0  0  0  0  0  0  0
    7.0000    3.5000    0.0000 O   0  0  0  0  0  0  0  0  0  0  0  0
    5.7000    5.7000    0.0000 O   0  0  0  0  0  0  0  0  0  0  0  0
    4.4000    4.9000    0.0000 N   0  0  0  0  0  0  0  0  0  0  0  0
  1  2  1  0  0  0  0
  2  3  2  0  0  0  0
  2  4  1  0  0  0  0
  4  5  1  0  0  0  0
  5  6  4  0  0  0  0
  6  7  4  0  0  0  0
  7  8  4  0  0  0  0
  8  9  4  0  0  0  0
  9  5  4  0  0  0  0
  8 10  1  0  0  0  0
 10 11  2  0  0  0  0
 10 12  2  0  0  0  0
 10 13  1  0  0  0  0
M  END
"""
    acetazolamide_sdf.write_text(
        acetazolamide_block.replace("Acetazolamide\n", "Acetazolamide record 1\n", 1)
        + "$$$$\n"
        + acetazolamide_block.replace("Acetazolamide\n", "Acetazolamide record 2\n", 1)
        + "$$$$\n",
        encoding="utf-8",
    )
    fasta = root / "protein.fasta"
    fasta.write_text(
        ">protein_1\nMKTAYIAKQRQISFVKSHFSRQDILDLWQ\n", encoding="utf-8"
    )
    fastq = root / "reads.fastq"
    fastq.write_text("@read_1\nATGCGTAA\n+\nFFFFFFFF\n", encoding="utf-8")
    genbank = root / "records.gbk"
    genbank.write_text(
        "LOCUS       FIRST\nORIGIN\n        1 atgcgtaa\n//\n"
        "LOCUS       SECOND\nORIGIN\n        1 acgtacgt\n//\n",
        encoding="utf-8",
    )
    pdb = root / "complex.pdb"
    pdb.write_text(
        "ATOM      1  N   GLY A   1       0.000   0.000   0.000  1.00  0.00           N  \n"
        "ATOM      2  CA  GLY A   1       1.458   0.000   0.000  1.00  0.00           C  \n"
        "HETATM    3  C1  LIG B   1       2.500   0.000   0.000  1.00  0.00           C  \n"
        "TER       4      GLY A   1\nEND\n",
        encoding="utf-8",
    )
    return [
        InputCase(
            "smiles",
            "small_molecule",
            {
                "inputs": [
                    {"field": "project_name", "value": "验收-SMILES"},
                    {
                        "field": "small_molecule",
                        "value": "CC(=O)Nc1nnc(s1)S(=O)(=O)N",
                    },
                ],
                "files": [
                    {
                        "field": "small_molecule",
                        "name": acetazolamide_sdf.name,
                        "persisted_path": str(acetazolamide_sdf.resolve()),
                    }
                ],
            },
            "candidate_identity_and_complex_md",
        ),
        InputCase(
            "peptide",
            "peptide",
            {
                "inputs": [
                    {"field": "project_name", "value": "验收-多肽"},
                    {
                        "field": "peptide",
                        "value": "GTGSFGRVMLVKHKETGNHYAMKILDKQK",
                    },
                ],
                "files": [],
            },
            "peptide_conformer",
        ),
        InputCase(
            "fasta",
            "protein_ligand",
            file_manifest("验收-FASTA", "protein_ligand", fasta),
        ),
        InputCase(
            "fastq",
            "genome_or_metagenome",
            file_manifest("验收-FASTQ", "genome_or_metagenome", fastq),
        ),
        InputCase(
            "genbank",
            "genome_or_metagenome",
            file_manifest("验收-GenBank", "genome_or_metagenome", genbank),
        ),
        InputCase("pdb", "complex", file_manifest("验收-PDB", "complex", pdb)),
    ]


def create_project(client: ApiClient) -> str:
    response = client.json("POST", "/api/user/projects", {})
    project = response.get("project")
    require(isinstance(project, dict), f"project creation failed: {response}")
    project_id = project.get("project_id")
    require(isinstance(project_id, str) and project_id, f"project id missing: {response}")
    return project_id


def inspect_and_save(
    client: ApiClient, project_id: str, manifest: dict[str, Any], expected_kind: str
) -> None:
    inspection = client.json("POST", "/api/user/inputs/inspect", manifest)
    require(inspection.get("usable") is True, f"input inspection failed: {inspection}")
    kinds = {
        row.get("canonical_kind")
        for row in inspection.get("inputs", [])
        if isinstance(row, dict) and row.get("usable") is True
    }
    require(expected_kind in kinds, f"expected {expected_kind}, inspection returned {kinds}")
    saved = client.json(
        "POST",
        f"/api/user/projects/{urllib.parse.quote(project_id)}/inputs",
        manifest,
    )
    require(saved.get("ok") is True, f"input save failed: {saved}")
    require(
        saved.get("inspection", {}).get("usable") is True,
        f"saved input inspection is inconsistent: {saved}",
    )


def require_preflight(client: ApiClient, project_id: str) -> None:
    preflight = client.json(
        "POST", f"/api/user/projects/{urllib.parse.quote(project_id)}/preflight", {}
    )
    require(preflight.get("ok") is True, f"project preflight failed: {preflight}")
    checks = preflight.get("checks")
    require(isinstance(checks, list) and checks, f"preflight checks missing: {preflight}")
    require(
        all(isinstance(row, dict) and row.get("ready") is True for row in checks),
        f"preflight contains an unready check: {preflight}",
    )


def start_run(client: ApiClient, project_id: str) -> tuple[str, dict[str, Any]]:
    response = client.json(
        "POST", f"/api/user/projects/{urllib.parse.quote(project_id)}/runs", {}
    )
    run = response.get("run")
    require(isinstance(run, dict), f"run response missing: {response}")
    run_id = run.get("run_id")
    require(isinstance(run_id, str) and run_id, f"run id missing: {response}")
    return run_id, response


def wait_for_state(
    client: ApiClient,
    run_id: str,
    expected: str,
    timeout: float,
    poll_interval: float,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout
    path = f"/api/user/runs/{urllib.parse.quote(run_id)}"
    last: dict[str, Any] = {}
    while time.monotonic() < deadline:
        last = client.json("GET", path)
        state = last.get("state")
        if state in TERMINAL_STATES:
            require(state == expected, f"run {run_id} reached {state}, expected {expected}: {last}")
            return last
        time.sleep(poll_interval)
    raise AssertionError(f"run {run_id} did not reach {expected}; last response: {last}")


def run_directory(artifact_dir: Path, project_id: str, run_id: str) -> Path:
    path = artifact_dir / "projects" / project_id / "runs" / run_id
    require(path.is_dir(), f"run directory is missing: {path}")
    return path


def read_trimmed(path: Path) -> str:
    return path.read_text(encoding="utf-8-sig").strip() if path.is_file() else ""


def artifact_map(value: Any) -> dict[str, bool]:
    rows = value if isinstance(value, list) else []
    return {
        str(row.get("id")): row.get("exists") is True
        for row in rows
        if isinstance(row, dict) and isinstance(row.get("id"), str)
    }


def valid_sha256(value: object) -> bool:
    text = str(value).strip().lower()
    return len(text) == 64 and set(text) <= HEX_SHA256


def valid_git_commit(value: object) -> bool:
    text = str(value).strip().lower()
    return len(text) == 40 and set(text) <= HEX_SHA256


def validate_input_capabilities(payload: dict[str, Any]) -> None:
    require(
        payload.get("schema_version") == "genoligand_input_capabilities_v1",
        f"input capability schema mismatch: {payload}",
    )
    rows = payload.get("input_types")
    require(isinstance(rows, list), f"input capability rows are invalid: {payload}")
    by_kind = {
        str(row.get("kind", "")): row
        for row in rows
        if isinstance(row, dict) and str(row.get("kind", ""))
    }
    require(
        len(by_kind) == len(rows) and set(by_kind) == REQUIRED_INPUT_KINDS,
        f"input capability kinds differ: {sorted(by_kind)}",
    )
    required_forms = {
        "small_molecule": ("text", "forms", "smiles"),
        "peptide": ("text", "forms", "amino_acid_sequence"),
        "protein_ligand": ("file", "extensions", "fasta"),
        "complex": ("file", "extensions", "pdb"),
        "genome_or_metagenome": ("file", "extensions", "fastq"),
    }
    for kind, (section, field, expected) in required_forms.items():
        values = by_kind[kind].get(section, {}).get(field, [])
        require(
            isinstance(values, list) and expected in values,
            f"input capability {kind} omits {expected}",
        )
    genome_extensions = by_kind["genome_or_metagenome"].get("file", {}).get(
        "extensions", []
    )
    require(
        isinstance(genome_extensions, list) and "gb" in genome_extensions,
        "input capability genome_or_metagenome omits gb",
    )


def parse_tsv(text: str, label: str) -> list[dict[str, str]]:
    reader = csv.DictReader(io.StringIO(text), delimiter="\t")
    require(reader.fieldnames is not None, f"{label} header is missing")
    return [
        {str(key): str(value or "").strip() for key, value in row.items() if key is not None}
        for row in reader
    ]


def read_tsv(path: Path) -> list[dict[str, str]]:
    require(path.is_file(), f"TSV artifact is missing: {path}")
    return parse_tsv(path.read_text(encoding="utf-8-sig"), str(path))


def load_json_file(path: Path, label: str) -> dict[str, Any]:
    require(path.is_file(), f"{label} is missing: {path}")
    try:
        payload = json.loads(path.read_text(encoding="utf-8-sig"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise AssertionError(f"{label} is invalid JSON: {path}: {error}") from error
    require(isinstance(payload, dict), f"{label} must be a JSON object: {path}")
    return payload


def draft_2020_12_validator(schema_name: str) -> Any:
    try:
        from jsonschema import Draft202012Validator
    except ImportError as error:
        raise AssertionError(
            "jsonschema with Draft 2020-12 support is required for worker artifact acceptance"
        ) from error
    schema = load_json_file(ROOT / "schemas" / schema_name, f"published {schema_name}")
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
        f"{schema_name} does not publish Draft 2020-12",
    )
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def validate_schema(validator: Any, payload: dict[str, Any], label: str) -> None:
    errors = sorted(
        validator.iter_errors(payload),
        key=lambda error: "/".join(str(part) for part in error.absolute_path),
    )
    require(
        not errors,
        f"{label} violates the published schema: "
        + "; ".join(
            f"{error.json_path}: {error.message}" for error in errors
        ),
    )


def validate_worker_contract_artifacts(run_dir: Path) -> dict[str, Any]:
    job_dir = run_dir / "worker_jobs"
    result_dir = run_dir / "worker_results"
    require(job_dir.is_dir(), f"completed run worker_jobs directory is missing: {run_dir}")
    require(result_dir.is_dir(), f"completed run worker_results directory is missing: {run_dir}")
    job_files = sorted(job_dir.glob("*.json"))
    result_files = sorted(result_dir.glob("*.json"))
    require(job_files, f"completed run has no real worker job files: {run_dir}")
    require(result_files, f"completed run has no real worker result files: {run_dir}")

    job_validator = draft_2020_12_validator("worker_job.schema.json")
    result_validator = draft_2020_12_validator("worker_result.schema.json")
    jobs: dict[str, dict[str, Any]] = {}
    results: dict[str, dict[str, Any]] = {}
    for path in job_files:
        payload = load_json_file(path, "worker job")
        validate_schema(job_validator, payload, f"real worker job {path.name}")
        job_id = str(payload.get("job_id", ""))
        require(path.stem == job_id, f"worker job filename does not match job_id: {path.name}")
        require(job_id not in jobs, f"duplicate worker job_id: {job_id}")
        require(payload.get("run_id") == run_dir.name, f"worker job run_id mismatch: {path.name}")
        jobs[job_id] = payload
    for path in result_files:
        payload = load_json_file(path, "worker result")
        validate_schema(result_validator, payload, f"real worker result {path.name}")
        job_id = str(payload.get("job_id", ""))
        require(path.stem == job_id, f"worker result filename does not match job_id: {path.name}")
        require(job_id not in results, f"duplicate worker result job_id: {job_id}")
        require(payload.get("run_id") == run_dir.name, f"worker result run_id mismatch: {path.name}")
        results[job_id] = payload
    require(
        set(jobs) == set(results),
        "real worker job/result file sets differ: "
        f"jobs_only={sorted(set(jobs) - set(results))},"
        f"results_only={sorted(set(results) - set(jobs))}",
    )

    coverage: dict[str, dict[str, Any]] = {}
    for job_id, job in jobs.items():
        result = results[job_id]
        for field in ("job_id", "worker", "run_id", "invocation_id"):
            require(
                result.get(field) == job.get(field),
                f"real worker job/result {field} mismatch: {job_id}",
            )
        for field in (
            "candidate_id",
            "candidate_digest",
            "input_id",
            "record_id",
            "input_digest",
            "source_digest",
            "receptor_digest",
        ):
            if field in job or field in result:
                require(
                    result.get(field) == job.get(field),
                    f"real worker job/result {field} binding mismatch: {job_id}",
                )
        for field in ("candidate_id", "input_id", "input_digest"):
            expected = f"expected_{field}"
            if expected in result:
                require(
                    result[expected] == job.get(field),
                    f"real worker result {expected} binding mismatch: {job_id}",
                )
        worker = str(job["worker"])
        require(worker in REQUIRED_WORKERS, f"unexpected real worker type: {worker}")
        stats = coverage.setdefault(
            worker,
            {
                "jobs": 0,
                "results": 0,
                "result_statuses": {},
                "execution_statuses": {},
                "contract_variants": {},
            },
        )
        stats["jobs"] += 1
        stats["results"] += 1
        for key, source_field in (
            ("result_statuses", "status"),
            ("execution_statuses", "execution_status"),
        ):
            value = str(result[source_field])
            stats[key][value] = int(stats[key].get(value, 0)) + 1
        execution_status = str(result["execution_status"])
        if str(result["engine_version"]).startswith("worker unavailable:"):
            variant = "rust_fallback_failure"
        elif execution_status == "failed":
            variant = "python_failure"
        else:
            variant = f"python_{execution_status}"
        stats["contract_variants"][variant] = (
            int(stats["contract_variants"].get(variant, 0)) + 1
        )
    return {
        "job_files": len(job_files),
        "result_files": len(result_files),
        "workers": coverage,
    }


def optional_worker_contract_artifacts(run_dir: Path) -> dict[str, Any]:
    jobs = list((run_dir / "worker_jobs").glob("*.json"))
    results = list((run_dir / "worker_results").glob("*.json"))
    if not jobs and not results:
        return {"job_files": 0, "result_files": 0, "workers": {}}
    return validate_worker_contract_artifacts(run_dir)


def require_fields(row: dict[str, Any], fields: tuple[str, ...], label: str) -> None:
    missing = [field for field in fields if not str(row.get(field, "")).strip()]
    require(not missing, f"{label} fields missing: {missing}")


def unit_score(value: object, label: str) -> None:
    try:
        number = float(str(value))
    except ValueError as error:
        raise AssertionError(f"{label} is not numeric") from error
    require(math.isfinite(number) and 0.0 <= number <= 1.0, f"{label} is out of range: {number}")


def evidence_value(text: str, key: str) -> str:
    for part in text.split(";"):
        name, separator, value = part.partition("=")
        if separator and name.strip() == key:
            return value.strip()
    return ""


def validate_search_outcomes(run_dir: Path) -> dict[str, int]:
    specs = {
        "known": ("known_database_matches.tsv", ("source_row_provenance", "current_run_query_id")),
        "metagenome": ("metagenome_potential_matches.tsv", ("source_database_id", "current_run_query_id")),
        "foldseek": (
            "tool_outputs/foldseek_structure_matches.tsv",
            ("source_database_id", "source_digest", "builder_version", "foldseek_version", "current_run_query_id", "query_digest", "tm_score"),
        ),
    }
    counts = {}
    base = ("input_id", "record_id", "source", "target_id", "match_score", "evidence")
    for name, (relative, extra) in specs.items():
        rows = read_tsv(run_dir / relative)
        counts[name] = len(rows)
        for line, row in enumerate(rows, 2):
            label = f"{relative}:{line}"
            require_fields(row, base + extra, label)
            unit_score(row["match_score"], label)
            if name == "foldseek":
                unit_score(row["tm_score"], label)
                require(valid_sha256(row["source_digest"]) and valid_sha256(row["query_digest"]), f"{label} digest invalid")
            else:
                digest = evidence_value(row["evidence"], "source_digest")
                require(valid_sha256(digest), f"{label} source digest invalid")
                if name == "metagenome":
                    require(all(evidence_value(row["evidence"], key) for key in ("builder_version", "mmseqs_version")), f"{label} tool provenance missing")
    return counts


def run_artifact(run_dir: Path, value: object, label: str) -> Path:
    raw = str(value).strip()
    require(raw, f"{label} path is missing")
    path = Path(raw)
    if not path.is_absolute():
        path = run_dir / path
    resolved = path.resolve(strict=False)
    try:
        resolved.relative_to(run_dir.resolve())
    except ValueError as error:
        raise AssertionError(f"{label} is outside the current run: {raw}") from error
    require(resolved.is_file(), f"{label} file is missing: {resolved}")
    return resolved


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json_sha256(value: object) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def json_artifact(run_dir: Path, value: object, label: str) -> dict[str, Any]:
    path = run_artifact(run_dir, value, label)
    try:
        payload = json.loads(path.read_text(encoding="utf-8-sig"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise AssertionError(f"{label} is invalid JSON: {path}: {error}") from error
    require(isinstance(payload, dict), f"{label} must be a JSON object: {path}")
    return payload


def require_identity(
    payload: dict[str, Any], entry: dict[str, Any], label: str, run_id: str
) -> None:
    require(payload.get("run_id") == run_id, f"{label} run identity mismatch")
    for field in (
        "candidate_id",
        "input_id",
        "record_id",
        "candidate_digest",
        "input_digest",
        "job_id",
        "invocation_id",
    ):
        require(payload.get(field) == entry.get(field), f"{label} {field} identity mismatch")
    source_digest = payload.get("source_digest", payload.get("source_content_sha256"))
    require(source_digest == entry.get("source_digest"), f"{label} source digest mismatch")


def verify_artifacts(run_dir: Path, rows: object, label: str, entry: dict[str, Any]) -> None:
    require(isinstance(rows, list) and rows, f"{label} artifact list is empty")
    seen: set[str] = set()
    for index, row in enumerate(rows):
        require(isinstance(row, dict), f"{label} artifact row {index} is invalid")
        require_identity(row, entry, f"{label} artifact row {index}", run_dir.name)
        raw_path = str(row.get("artifact_path", row.get("path", ""))).strip()
        digest = str(row.get("artifact_sha256", row.get("sha256", ""))).strip().lower()
        path = run_artifact(run_dir, raw_path, f"{label} artifact {index}")
        relative = path.relative_to(run_dir.resolve()).as_posix()
        require(relative not in seen, f"{label} duplicates artifact: {relative}")
        seen.add(relative)
        require(valid_sha256(digest), f"{label} digest is invalid: {relative}")
        require(sha256_file(path) == digest, f"{label} digest mismatch: {relative}")


def validate_manifest_index(
    run_dir: Path,
    kind: str,
    selected: dict[str, dict[str, str]],
) -> dict[str, int]:
    path = run_dir / f"tool_outputs/{kind}_manifest_index.json"
    require(path.is_file(), f"{kind} manifest index is missing for selected candidates")
    payload = json.loads(path.read_text(encoding="utf-8-sig"))
    require(isinstance(payload, dict), f"{kind} manifest index is invalid")
    require(
        payload.get("schema_version") == f"genoligand_{kind}_manifest_index_v1",
        f"{kind} manifest index schema mismatch",
    )
    require(payload.get("run_id") == run_dir.name, f"{kind} manifest index run mismatch")
    entries = payload.get("entries")
    require(isinstance(entries, list), f"{kind} manifest index entries are invalid")
    by_candidate = {str(entry.get("candidate_id", "")): entry for entry in entries if isinstance(entry, dict)}
    require(len(by_candidate) == len(entries) and "" not in by_candidate, f"{kind} candidate index duplicate")
    require(set(by_candidate) == set(selected), f"{kind} outcomes do not match top-N selection")

    completed = 0
    for candidate_id, entry in by_candidate.items():
        selection = selected[candidate_id]
        require_fields(entry, ("input_id", "record_id", "candidate_digest", "input_digest", "source_digest", "job_id", "invocation_id", "status", "reason"), f"{kind}:{candidate_id}")
        require(entry["input_id"] == selection["input_id"] and entry["record_id"] == selection["record_id"], f"{kind} selection identity mismatch: {candidate_id}")
        require(all(valid_sha256(entry[field]) for field in ("candidate_digest", "input_digest", "source_digest")), f"{kind} digest invalid: {candidate_id}")
        status = str(entry["status"])
        require(status in {"completed", "not_ready", "failed"}, f"{kind} outcome status invalid: {status}")
        manifest = None
        if str(entry.get("manifest", "")).strip():
            manifest = json_artifact(run_dir, entry["manifest"], f"{kind} manifest:{candidate_id}")
            require_identity(manifest, entry, f"{kind} manifest:{candidate_id}", run_dir.name)
        if status == "completed":
            completed += 1
            require(manifest is not None, f"completed {kind} manifest is missing: {candidate_id}")
            if kind == "docking":
                verify_artifacts(run_dir, manifest.get("artifacts"), f"docking:{candidate_id}", entry)
            else:
                provenance = json_artifact(run_dir, entry.get("artifact_provenance"), f"MD provenance:{candidate_id}")
                require_identity(provenance, entry, f"MD provenance:{candidate_id}", run_dir.name)
                require(valid_sha256(provenance.get("parameters_digest")), "MD parameter digest is invalid")
                require_fields(provenance, ("tool_name", "tool_version"), f"MD provenance:{candidate_id}")
                verify_artifacts(run_dir, provenance.get("artifacts"), f"md:{candidate_id}", entry)
    return {"outcomes": len(by_candidate), "completed": completed}


def validate_candidate_outcomes(run_dir: Path, candidate_ids: set[str]) -> dict[str, int]:
    selection_rows = read_tsv(run_dir / "complex_candidate_selection.tsv")
    require(len(selection_rows) <= 3, f"coordinate selection exceeds top-N=3: {len(selection_rows)}")
    selected: dict[str, dict[str, str]] = {}
    for index, row in enumerate(selection_rows, start=2):
        require_fields(row, ("candidate_id", "identity_namespace", "input_id", "record_id", "target_id", "source", "evidence", "selection_rank", "receptor_artifact", "coordinate_sha256"), f"complex_candidate_selection.tsv:{index}")
        candidate_id = row["candidate_id"]
        require(candidate_id not in selected, f"selected candidate is duplicated: {candidate_id}")
        require(candidate_id in candidate_ids, f"selected candidate is absent from ranking: {candidate_id}")
        require(valid_sha256(row["coordinate_sha256"]), f"coordinate digest invalid: {candidate_id}")
        receptor = run_artifact(run_dir, row["receptor_artifact"], f"receptor for {candidate_id}")
        require(sha256_file(receptor) == row["coordinate_sha256"].lower(), f"receptor digest mismatch: {candidate_id}")
        selected[candidate_id] = row
    if not selected:
        return {key: 0 for key in ("selected", "docking_outcomes", "docking_completed", "md_outcomes", "md_completed")}

    docking = validate_manifest_index(run_dir, "docking", selected)
    md = validate_manifest_index(run_dir, "md", selected)
    decomposition = read_tsv(run_dir / "score_decomposition.tsv")
    present = {
        (row.get("candidate_id", ""), row.get("component", ""))
        for row in decomposition
        if row.get("value") == "1.0000"
    }
    for candidate_id in selected:
        for component in ("coordinate_backed_candidate", "docking_outcome_present", "md_outcome_present"):
            require((candidate_id, component) in present, f"score decomposition misses {candidate_id}:{component}")
    return {
        "selected": len(selected),
        "docking_outcomes": docking["outcomes"],
        "docking_completed": docking["completed"],
        "md_outcomes": md["outcomes"],
        "md_completed": md["completed"],
    }


def manifest_index_entries(run_dir: Path, kind: str) -> list[dict[str, Any]]:
    payload = load_json_file(
        run_dir / "tool_outputs" / f"{kind}_manifest_index.json",
        f"{kind} manifest index",
    )
    rows = payload.get("entries")
    require(isinstance(rows, list), f"{kind} manifest index entries are invalid")
    require(all(isinstance(row, dict) for row in rows), f"{kind} manifest index row is invalid")
    return rows


def validate_candidate_identity_continuation(run_dir: Path) -> dict[str, Any]:
    conversion = read_tsv(run_dir / "input_conversion_manifest.tsv")
    sdf_rows = [
        row
        for row in conversion
        if row.get("format", "").lower() == "sdf"
        and row.get("conversion_status") == "converted"
    ]
    sdf_identities = {
        (row.get("input_id", ""), row.get("record_id", "")) for row in sdf_rows
    }
    require(
        len(sdf_rows) == 2
        and len(sdf_identities) == 2
        and len({input_id for input_id, _ in sdf_identities}) == 1,
        f"multi-record SDF did not preserve two records: {sdf_rows}",
    )

    known = read_tsv(run_dir / "known_database_matches.tsv")
    ranking = read_tsv(run_dir / "target_candidate_ranking.tsv")
    require(
        len({row.get("candidate_id", "") for row in ranking}) == len(ranking),
        "candidate ranking contains duplicate candidate ids",
    )
    sources_by_identity_target: dict[tuple[str, str, str], set[str]] = {}
    identities_by_target: dict[str, set[tuple[str, str]]] = {}
    for row in known:
        input_id = row.get("input_id", "")
        record_id = row.get("record_id", "")
        target_id = row.get("target_id", "")
        source = row.get("source", "").lower()
        if not input_id or not record_id or not target_id:
            continue
        source_class = (
            "chembl"
            if "chembl" in source
            else "bindingdb"
            if "bindingdb" in source
            else source
        )
        sources_by_identity_target.setdefault(
            (input_id, record_id, target_id), set()
        ).add(source_class)
        identities_by_target.setdefault(target_id, set()).add((input_id, record_id))

    multi_source = sorted(
        (
            (identity_target, sources)
            for identity_target, sources in sources_by_identity_target.items()
            if {"chembl", "bindingdb"} <= sources
        ),
        key=lambda item: item[0],
    )
    require(multi_source, "no real input record merged ChEMBL and BindingDB evidence")
    (source_input, source_record, source_target), source_names = multi_source[0]
    merged = [
        row
        for row in ranking
        if (
            row.get("input_id"),
            row.get("record_id"),
            row.get("target_id"),
        )
        == (source_input, source_record, source_target)
    ]
    require(
        len(merged) == 1,
        "same-record multi-source evidence did not merge into one candidate",
    )

    target_id, target_identities = next(
        (
            (target, identities)
            for target, identities in sorted(identities_by_target.items())
            if len({input_id for input_id, _ in identities}) >= 2
        ),
        ("", set()),
    )
    require(target_id, "no target was independently retained for two project inputs")
    same_target_candidates = [
        row
        for row in ranking
        if row.get("target_id") == target_id
        and (row.get("input_id", ""), row.get("record_id", "")) in target_identities
    ]
    by_identity: dict[tuple[str, str], list[dict[str, str]]] = {}
    for row in same_target_candidates:
        by_identity.setdefault(
            (row.get("input_id", ""), row.get("record_id", "")), []
        ).append(row)
    require(
        all(len(by_identity.get(identity, [])) == 1 for identity in target_identities)
        and len({rows[0]["candidate_id"] for rows in by_identity.values()})
        == len(target_identities),
        "same target across project inputs did not retain distinct candidate identities",
    )

    ranked_identities = {
        (row.get("input_id", ""), row.get("record_id", "")) for row in ranking
    }
    selected_rows = read_tsv(run_dir / "complex_candidate_selection.tsv")
    selected_identities = {
        (row.get("input_id", ""), row.get("record_id", "")) for row in selected_rows
    }
    require(sdf_identities <= ranked_identities, "an SDF record did not reach candidate ranking")
    require(sdf_identities <= selected_identities, "an SDF record did not reach top-N selection")
    selected_candidates = {
        row.get("candidate_id", "")
        for row in selected_rows
        if (row.get("input_id", ""), row.get("record_id", "")) in sdf_identities
    }
    for kind in ("docking", "md"):
        indexed = {str(row.get("candidate_id", "")) for row in manifest_index_entries(run_dir, kind)}
        require(
            selected_candidates <= indexed,
            f"an SDF record did not reach the {kind} worker outcome index",
        )
    return {
        "same_record_multi_source": {
            "input_id": source_input,
            "record_id": source_record,
            "target_id": source_target,
            "sources": sorted(source_names),
            "candidate_id": merged[0]["candidate_id"],
        },
        "same_target_project_inputs": {
            "target_id": target_id,
            "input_records": len(target_identities),
            "candidate_ids": sorted(rows[0]["candidate_id"] for rows in by_identity.values()),
        },
        "multi_record_sdf": {
            "records": sorted(f"{input_id}:{record_id}" for input_id, record_id in sdf_identities),
            "selected_candidates": sorted(selected_candidates),
            "continued_through": ["candidate_ranking", "top_n", "docking", "md"],
        },
    }


def completed_md_manifests(run_dir: Path) -> list[tuple[dict[str, Any], dict[str, Any]]]:
    completed = []
    for entry in manifest_index_entries(run_dir, "md"):
        if entry.get("status") != "completed":
            continue
        completed.append(
            (
                entry,
                json_artifact(
                    run_dir,
                    entry.get("manifest"),
                    f"completed MD manifest:{entry.get('candidate_id', '')}",
                ),
            )
        )
    require(completed, "input case has no completed MD semantic artifact")
    return completed


def validate_md_case_semantics(run_dir: Path, profile: str) -> dict[str, Any]:
    completed = completed_md_manifests(run_dir)
    if profile == "peptide_conformer":
        for entry, manifest in completed:
            require(
                manifest.get("artifact_schema_version")
                == "genoligand_peptide_conformer_v1"
                and manifest.get("method") == "rdkit_peptide_conformer_ensemble"
                and manifest.get("method_class") == "conformer_ensemble"
                and manifest.get("platform") == "RDKit"
                and manifest.get("has_receptor") is False
                and manifest.get("uses_openmm") is False
                and float(manifest.get("md_score", -1)) == 0.0,
                f"peptide MD semantics are invalid: {entry.get('candidate_id')}",
            )
            require(int(manifest.get("conformer_count", 0)) > 0, "peptide conformer count is empty")
            run_artifact(run_dir, manifest.get("coordinate_artifact"), "peptide coordinates")
            run_artifact(run_dir, manifest.get("ensemble_artifact"), "peptide ensemble")
            metrics = manifest.get("metric_artifacts")
            require(isinstance(metrics, list) and len(metrics) >= 2, "peptide metrics are incomplete")
            for path in metrics:
                run_artifact(run_dir, path, "peptide metric")
        require(not read_tsv(run_dir / "md_summary.tsv"), "peptide conformers entered complex MD scores")
        report = (run_dir / "research_report.md").read_text(encoding="utf-8-sig")
        require("OpenMM" not in report, "peptide report claims OpenMM complex dynamics")
        return {
            "method": "rdkit_peptide_conformer_ensemble",
            "completed_candidates": len(completed),
            "uses_openmm": False,
            "md_score": 0.0,
        }

    require(profile == "candidate_identity_and_complex_md", f"unknown MD profile: {profile}")
    facts = []
    for entry, manifest in completed:
        if manifest.get("method") != "openmm_receptor_ligand_complex_dynamics":
            continue
        platform = str(manifest.get("platform", ""))
        system = manifest.get("system")
        require(platform in {"CPU", "CUDA", "Reference"}, f"OpenMM platform is invalid: {platform}")
        require(
            isinstance(system, dict)
            and str(system.get("solvent_mode", ""))
            and str(system.get("nonbonded_method", ""))
            and int(system.get("total_atom_count", 0)) > 0,
            "complex OpenMM system facts are incomplete",
        )
        require(
            int(manifest.get("frame_count", 0)) > 0
            and int(manifest.get("state_row_count", 0)) > 0,
            "complex OpenMM trajectory facts are incomplete",
        )
        for field in (
            "receptor_coordinate",
            "ligand_coordinate",
            "complex_manifest",
            "protocol_artifact",
            "state_artifact",
            "trajectory",
        ):
            run_artifact(run_dir, manifest.get(field), f"complex OpenMM {field}")
        provenance = json_artifact(
            run_dir,
            entry.get("artifact_provenance"),
            f"complex OpenMM provenance:{entry.get('candidate_id', '')}",
        )
        require(provenance.get("tool_name") == "OpenMM", "complex MD provenance tool is not OpenMM")
        facts.append(
            {
                "candidate_id": entry.get("candidate_id"),
                "platform": platform,
                "solvent_mode": system["solvent_mode"],
                "nonbonded_method": system["nonbonded_method"],
                "total_atom_count": system["total_atom_count"],
                "frame_count": manifest["frame_count"],
                "state_row_count": manifest["state_row_count"],
            }
        )
    require(facts, "small-molecule case has no completed complex OpenMM calculation")
    return {"method": "openmm_receptor_ligand_complex_dynamics", "facts": facts}


def result_page(client: ApiClient, run_id: str, endpoint: str, row_key: str, page: int, page_size: int) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    query = urllib.parse.urlencode({"page": page, "page_size": page_size})
    payload = client.json(
        "GET", f"/api/user/runs/{urllib.parse.quote(run_id)}/{endpoint}?{query}"
    )
    rows = payload.get(row_key)
    pagination = payload.get("pagination")
    require(isinstance(rows, list), f"{endpoint} rows are invalid: {payload}")
    require(isinstance(pagination, dict), f"{endpoint} pagination is invalid: {payload}")
    require((pagination.get("page"), pagination.get("page_size"), pagination.get("returned_rows")) == (page, page_size, len(rows)), f"{endpoint} page metadata mismatch")
    require(payload.get("row_count") == pagination.get("total_rows") == pagination.get("filtered_rows"), f"{endpoint} total mismatch")
    require(all(isinstance(row, dict) for row in rows), f"{endpoint} contains invalid rows")
    return rows, pagination


def validate_paginated_results(
    client: ApiClient, run_id: str, endpoint: str, row_key: str
) -> list[dict[str, Any]]:
    page = 1
    all_rows: list[dict[str, Any]] = []
    total: int | None = None
    while True:
        rows, pagination = result_page(client, run_id, endpoint, row_key, page, 5000)
        if total is None:
            total = pagination["total_rows"]
            require(isinstance(total, int) and total > 0, f"completed run has no {endpoint} rows")
        require(pagination["total_rows"] == total, f"{endpoint} total changed between pages")
        all_rows.extend(rows)
        if pagination.get("has_next_page") is not True:
            break
        page += 1
    require(total == len(all_rows), f"{endpoint} full pagination lost rows")
    for singleton_page in sorted({1, min(2, total), total}):
        rows, pagination = result_page(client, run_id, endpoint, row_key, singleton_page, 1)
        require(rows == [all_rows[singleton_page - 1]], f"{endpoint} singleton page is inconsistent")
        require(
            pagination.get("has_previous_page") is (singleton_page > 1),
            f"{endpoint} previous-page flag is inconsistent",
        )
        require(
            pagination.get("has_next_page") is (singleton_page < total),
            f"{endpoint} next-page flag is inconsistent",
        )
    return all_rows


def validate_export(client: ApiClient, run_id: str, artifact: str, api_rows: list[dict[str, Any]], fields: tuple[tuple[str, str], ...]) -> int:
    status, content_type, body = client.request(
        "GET", f"/api/user/runs/{urllib.parse.quote(run_id)}/download/{artifact}"
    )
    require(status == 200, f"{artifact} export returned HTTP {status}")
    require("text/tab-separated-values" in content_type, f"{artifact} export type is {content_type}")
    exported = parse_tsv(body.decode("utf-8-sig"), artifact)
    require(len(exported) == len(api_rows), f"{artifact} export row count differs from API")
    for index, (api_row, export_row) in enumerate(zip(api_rows, exported), start=1):
        for api_field, export_field in fields:
            require(
                str(api_row.get(api_field, "")).strip() == export_row.get(export_field, "").strip(),
                f"{artifact} export mismatch at row {index}, field {api_field}",
            )
    return len(exported)


def validate_report_trace(
    run_dir: Path,
    project_id: str,
    run_id: str,
    report_text: str,
    candidates: list[dict[str, Any]],
) -> str:
    context_path = run_dir / "run_context.json"
    context = json.loads(context_path.read_text(encoding="utf-8-sig"))
    invocation = read_trimmed(run_dir / "pipeline_invocation_id.txt")
    require(invocation, "pipeline invocation identity is missing")
    require(context.get("project_id") == project_id, "run context project identity mismatch")
    require(context.get("run_id") == run_id, "run context run identity mismatch")
    require(context.get("pipeline_invocation_id") == invocation, "run context invocation mismatch")
    policy = context.get("ranking_policy")
    require(isinstance(policy, dict), "run context ranking policy is missing")
    policy_version = str(policy.get("schema_version", "")).strip()
    policy_digest = str(context.get("ranking_policy_digest", "")).strip().lower()
    require(policy_version, "ranking policy version is missing")
    require(valid_sha256(policy_digest), "ranking policy digest is invalid")
    require(
        canonical_json_sha256(policy) == policy_digest,
        "ranking policy digest does not match the policy payload",
    )
    context_rows = {
        row.get("field", ""): row.get("value", "")
        for row in read_tsv(run_dir / "run_context.tsv")
    }
    require(
        context_rows.get("ranking_policy_version") == policy_version
        and context_rows.get("ranking_policy_digest") == policy_digest,
        "JSON and TSV run contexts use different ranking policies",
    )
    decomposition = read_tsv(run_dir / "score_decomposition.tsv")
    require(decomposition, "score decomposition is empty")
    require(
        all(
            row.get("policy_version") == policy_version
            and row.get("policy_digest") == policy_digest
            for row in decomposition
        ),
        "score decomposition uses a different ranking policy",
    )
    require(
        policy_version in report_text and policy_digest in report_text,
        "report omits the bound ranking policy",
    )
    require(project_id in report_text and run_id in report_text, "report omits current run identity")
    first_target = str(candidates[0].get("target_id", "")).strip()
    require(first_target and first_target in report_text, "report top candidate differs from candidate API")

    source_map = read_tsv(run_dir / "artifact_source_map.tsv")
    require(source_map, "artifact source map is empty")
    require(
        all(row.get("project_id") == project_id and row.get("run_id") == run_id for row in source_map),
        "artifact source map contains another run identity",
    )
    report_rows = [row for row in source_map if row.get("artifact") == "research_report.md"]
    require(len(report_rows) == 1 and report_rows[0].get("status") == "available", "report source-map row is invalid")
    require(
        "candidate_detail.json" in report_rows[0].get("upstream_source", "")
        and "evidence_ledger.tsv" in report_rows[0].get("upstream_source", ""),
        "report source map omits candidate or evidence inputs",
    )
    gate = read_tsv(run_dir / "authenticity_gate.tsv")
    require(gate and all(row.get("status") == "passed" for row in gate), "authenticity gate did not pass")
    require(
        all(row.get("claim_id") and row.get("evidence_artifact") for row in gate),
        "authenticity gate contains an unbound claim",
    )
    return invocation


def validate_terminal_run(
    client: ApiClient,
    artifact_dir: Path,
    project_id: str,
    run_id: str,
    expected: str,
) -> dict[str, Any]:
    api_run = client.json("GET", f"/api/user/runs/{urllib.parse.quote(run_id)}")
    require(api_run.get("state") == expected, f"run API state mismatch: {api_run}")
    require(isinstance(api_run.get("state_label"), str), f"run state label missing: {api_run}")
    project = client.json(
        "GET", f"/api/user/projects/{urllib.parse.quote(project_id)}"
    )
    require(project.get("project_status") == expected, f"project state mismatch: {project}")

    run_dir = run_directory(artifact_dir, project_id, run_id)
    require(read_trimmed(run_dir / "run_state.txt") == expected, "run_state.txt disagrees with API")
    require(
        not (run_dir / "pipeline_done.txt").exists(),
        "direct server runs must not create the Nextflow pipeline_done marker",
    )
    failure_reason = read_trimmed(run_dir / "failure_reason.txt")
    if expected in {"blocked", "failed"}:
        require(failure_reason, f"{expected} run needs failure_reason.txt")
        require(f"state={expected}" in failure_reason, f"unstructured {expected} reason: {failure_reason}")
        require("repair_action=" in failure_reason, f"repair action missing: {failure_reason}")
    else:
        require(not failure_reason, f"{expected} run has a failure reason: {failure_reason}")
    if expected == "cancelled":
        require("state\tcancelled" in read_trimmed(run_dir / "run.log"), "cancel event missing")

    context_path = run_dir / "run_context.json"
    if expected != "cancelled":
        require(context_path.is_file(), f"{expected} run context is missing")
        context = json.loads(context_path.read_text(encoding="utf-8-sig"))
        require(context.get("state") == expected, f"run context state mismatch: {context}")

    artifacts = client.json("GET", f"/api/user/runs/{urllib.parse.quote(run_id)}/artifacts")
    endpoint_items = artifact_map(artifacts.get("items"))
    run_items = artifact_map(api_run.get("outputs", {}).get("items"))
    require(endpoint_items == run_items, "run and artifact APIs expose different outputs")
    candidates = client.json(
        "GET", f"/api/user/runs/{urllib.parse.quote(run_id)}/candidates"
    )
    evidence = client.json("GET", f"/api/user/runs/{urllib.parse.quote(run_id)}/evidence")
    candidate_count = candidates.get("row_count")
    evidence_count = evidence.get("row_count")
    require(isinstance(candidate_count, int), f"candidate count invalid: {candidates}")
    require(isinstance(evidence_count, int), f"evidence count invalid: {evidence}")
    require(candidates.get("has_output") is (candidate_count > 0), "candidate API mismatch")
    require(evidence.get("has_output") is (evidence_count > 0), "evidence API mismatch")

    report_path = run_dir / "research_report.md"
    report_visible = endpoint_items.get("research_report", False)
    download_status, content_type, report_body = client.request(
        "GET", f"/api/user/runs/{urllib.parse.quote(run_id)}/download/research_report"
    )
    scientific_outcomes: dict[str, Any] = {}
    worker_contracts: dict[str, Any] = {
        "job_files": 0,
        "result_files": 0,
        "workers": {},
    }
    if expected == "completed":
        require(candidate_count > 0, "completed run has no candidate rows")
        require(evidence_count > 0, "completed run has no evidence rows")
        require(report_path.is_file(), "completed run did not write research_report.md")
        require((run_dir / "authenticity_gate.tsv").is_file(), "authenticity gate is missing")
        require(report_visible is True, "completed result report is hidden")
        require(download_status == 200, f"visible report returned HTTP {download_status}")
        require("text/markdown" in content_type, f"report type is {content_type}")
        report_text = report_path.read_text(encoding="utf-8-sig")
        require(
            report_body.decode("utf-8-sig") == report_text,
            "downloaded report differs from the run artifact",
        )
        candidate_rows = validate_paginated_results(
            client, run_id, "candidates", "candidates"
        )
        evidence_rows = validate_paginated_results(client, run_id, "evidence", "evidence")
        require(len(candidate_rows) == candidate_count, "candidate pagination count mismatch")
        require(len(evidence_rows) == evidence_count, "evidence pagination count mismatch")
        candidate_export_rows = validate_export(
            client, run_id, "candidate_ranking", candidate_rows, CANDIDATE_EXPORT_FIELDS
        )
        evidence_export_rows = validate_export(
            client, run_id, "evidence", evidence_rows, EVIDENCE_EXPORT_FIELDS
        )
        search = validate_search_outcomes(run_dir)
        worker_contracts = validate_worker_contract_artifacts(run_dir)
        candidate_outcomes = validate_candidate_outcomes(
            run_dir,
            {
                str(row.get("candidate_id", "")).strip()
                for row in candidate_rows
                if str(row.get("candidate_id", "")).strip()
            },
        )
        invocation = validate_report_trace(
            run_dir, project_id, run_id, report_text, candidate_rows
        )
        scientific_outcomes = {
            "search": search,
            "worker_contracts": worker_contracts,
            "top_n": candidate_outcomes,
            "pipeline_invocation_id": invocation,
            "candidate_export_rows": candidate_export_rows,
            "evidence_export_rows": evidence_export_rows,
        }
    else:
        require(not report_visible, f"{expected} run exposes a report")
        require(download_status == 404, f"{expected} report download returned {download_status}")
        require(candidate_count == 0 and evidence_count == 0, f"{expected} run exposes results")
        if expected == "failed":
            worker_contracts = optional_worker_contract_artifacts(run_dir)

    return {
        "project_id": project_id,
        "run_id": run_id,
        "run_dir": str(run_dir),
        "state": expected,
        "failure_reason": failure_reason,
        "candidate_count": candidate_count,
        "evidence_count": evidence_count,
        "report_file": report_path.is_file(),
        "report_visible": report_visible,
        "done_marker": False,
        "worker_contracts": worker_contracts,
        "scientific_outcomes": scientific_outcomes,
    }


def run_completed_input_case(
    client: ApiClient,
    artifact_dir: Path,
    case: InputCase,
    timeout: float,
    poll_interval: float,
) -> dict[str, Any]:
    project_id = create_project(client)
    inspect_and_save(client, project_id, case.manifest, case.expected_kind)
    require_preflight(client, project_id)
    run_id, _ = start_run(client, project_id)
    wait_for_state(client, run_id, "completed", timeout, poll_interval)
    result = validate_terminal_run(client, artifact_dir, project_id, run_id, "completed")
    result["input_case"] = case.label
    result["canonical_kind"] = case.expected_kind
    if case.acceptance_profile:
        run_dir = Path(result["run_dir"])
        contracts: dict[str, Any] = {
            "md_semantics": validate_md_case_semantics(
                run_dir, case.acceptance_profile
            )
        }
        if case.acceptance_profile == "candidate_identity_and_complex_md":
            contracts["candidate_identity"] = validate_candidate_identity_continuation(
                run_dir
            )
        result["case_contracts"] = contracts
    return result


def validate_completed_aggregate(completed: list[dict[str, Any]]) -> dict[str, Any]:
    kinds = {str(row.get("canonical_kind", "")) for row in completed}
    labels = {str(row.get("input_case", "")) for row in completed}
    require(kinds == REQUIRED_INPUT_KINDS, f"completed input kinds differ: {sorted(kinds)}")
    require(REQUIRED_FORMAT_CASES <= labels, f"completed input formats differ: {sorted(labels)}")
    require(all(row.get("state") == "completed" for row in completed), "formal input did not complete")
    require(
        all(row.get("candidate_count", 0) > 0 and row.get("evidence_count", 0) > 0 for row in completed),
        "a formal completed input has no candidate or evidence rows",
    )

    search = {
        name: sum(
            int(row.get("scientific_outcomes", {}).get("search", {}).get(name, 0))
            for row in completed
        )
        for name in ("known", "metagenome", "foldseek")
    }
    require(all(count > 0 for count in search.values()), f"aggregate search outcomes missing: {search}")
    top_n = [row.get("scientific_outcomes", {}).get("top_n", {}) for row in completed]
    require(max((int(row.get("selected", 0)) for row in top_n), default=0) == 3, "no run exercised full top-N=3")
    docking_completed = sum(int(row.get("docking_completed", 0)) for row in top_n)
    md_completed = sum(int(row.get("md_completed", 0)) for row in top_n)
    require(docking_completed > 0, "aggregate acceptance has no completed docking outcome")
    require(md_completed > 0, "aggregate acceptance has no completed MD outcome")
    worker_coverage = {
        worker: {
            "jobs": 0,
            "results": 0,
            "result_statuses": {},
            "execution_statuses": {},
            "contract_variants": {},
        }
        for worker in sorted(REQUIRED_WORKERS)
    }
    for row in completed:
        observed = (
            row.get("scientific_outcomes", {})
            .get("worker_contracts", {})
            .get("workers", {})
        )
        require(isinstance(observed, dict), "completed run worker coverage is missing")
        for worker, stats in observed.items():
            require(
                worker in worker_coverage and isinstance(stats, dict),
                f"invalid worker coverage: {worker}",
            )
            aggregate = worker_coverage[worker]
            aggregate["jobs"] += int(stats.get("jobs", 0))
            aggregate["results"] += int(stats.get("results", 0))
            for key in ("result_statuses", "execution_statuses", "contract_variants"):
                values = stats.get(key, {})
                require(isinstance(values, dict), f"invalid {worker} {key} coverage")
                for value, count in values.items():
                    name = str(value)
                    aggregate[key][name] = int(aggregate[key].get(name, 0)) + int(count)
    require(
        all(stats["jobs"] > 0 and stats["results"] > 0 for stats in worker_coverage.values()),
        f"real worker Schema coverage is incomplete: {worker_coverage}",
    )
    require(
        all(
            int(stats["result_statuses"].get("PASS", 0)) > 0
            and int(stats["execution_statuses"].get("completed", 0)) > 0
            for stats in worker_coverage.values()
        ),
        f"real worker completed execution coverage is incomplete: {worker_coverage}",
    )
    provenance_runs = [
        row
        for row in completed
        if int(row.get("scientific_outcomes", {}).get("top_n", {}).get("docking_completed", 0))
        >= 2
        and int(row.get("scientific_outcomes", {}).get("top_n", {}).get("md_completed", 0))
        >= 1
    ]
    require(
        provenance_runs,
        "no completed run contains at least two Vina outcomes and one OpenMM outcome",
    )
    provenance_run = max(
        provenance_runs,
        key=lambda row: (
            int(row["scientific_outcomes"]["top_n"]["docking_completed"]),
            int(row["scientific_outcomes"]["top_n"]["md_completed"]),
        ),
    )
    return {
        "canonical_kinds": sorted(kinds),
        "format_cases": sorted(REQUIRED_FORMAT_CASES),
        "search_outcomes": search,
        "max_top_n": max(int(row.get("selected", 0)) for row in top_n),
        "docking_completed": docking_completed,
        "md_completed": md_completed,
        "worker_schema_coverage": worker_coverage,
        "runtime_provenance_attack_run": {
            "project_id": provenance_run["project_id"],
            "run_id": provenance_run["run_id"],
            "run_dir": provenance_run["run_dir"],
            "input_case": provenance_run["input_case"],
            "docking_completed": provenance_run["scientific_outcomes"]["top_n"][
                "docking_completed"
            ],
            "md_completed": provenance_run["scientific_outcomes"]["top_n"][
                "md_completed"
            ],
        },
    }


def summarize_worker_failure_and_fallback(
    runs: list[dict[str, Any]],
) -> dict[str, Any]:
    observed = {"python_failure": 0, "rust_fallback_failure": 0}
    for run in runs:
        contracts = run.get("worker_contracts", {})
        workers = contracts.get("workers", {}) if isinstance(contracts, dict) else {}
        if not isinstance(workers, dict):
            continue
        for stats in workers.values():
            variants = stats.get("contract_variants", {}) if isinstance(stats, dict) else {}
            if not isinstance(variants, dict):
                continue
            for name in observed:
                observed[name] += int(variants.get(name, 0))
    missing = [name for name, count in observed.items() if count == 0]
    return {
        "observed_real_job_results": observed,
        "missing_real_variants": missing,
        "schema_and_binding_validation": "executed_for_every_observed_job_result",
        "api_generation_status": (
            "complete"
            if not missing
            else "server_api_has_no_per_run_worker_fault_injection"
        ),
        "generation_note": (
            "A Python worker failure and Rust worker-unavailable fallback require "
            "process-level worker fault injection; the current user API exposes no such control."
        ),
    }


def run_blocked_case(
    client: ApiClient, artifact_dir: Path, timeout: float, poll_interval: float
) -> dict[str, Any]:
    project_id = create_project(client)
    run_id, _ = start_run(client, project_id)
    wait_for_state(client, run_id, "blocked", timeout, poll_interval)
    return validate_terminal_run(client, artifact_dir, project_id, run_id, "blocked")


def run_failed_case(
    client: ApiClient,
    artifact_dir: Path,
    fixture_root: Path,
    timeout: float,
    poll_interval: float,
) -> dict[str, Any]:
    source = fixture_root / "deleted_after_save.fasta"
    source.write_text(">deleted\nMKTAYIAKQRQISFVKSHFSRQDILDLWQ\n", encoding="utf-8")
    manifest = file_manifest("验收-失败终态", "protein_ligand", source)
    project_id = create_project(client)
    inspect_and_save(client, project_id, manifest, "protein_ligand")
    source.unlink()
    run_id, _ = start_run(client, project_id)
    wait_for_state(client, run_id, "failed", timeout, poll_interval)
    return validate_terminal_run(client, artifact_dir, project_id, run_id, "failed")


def run_cancelled_case(
    client: ApiClient,
    artifact_dir: Path,
    fixture_root: Path,
    timeout: float,
    poll_interval: float,
) -> dict[str, Any]:
    source = fixture_root / "cancel_after_start.fasta"
    source.write_text(">cancel\n" + "ACGT" * 65_536 + "\n", encoding="utf-8")
    manifest = file_manifest("验收-取消终态", "genome_or_metagenome", source)
    project_id = create_project(client)
    inspect_and_save(client, project_id, manifest, "genome_or_metagenome")
    require_preflight(client, project_id)
    run_id, started = start_run(client, project_id)
    require(started.get("run", {}).get("state") == "running", f"run finished before cancel: {started}")
    cancelled = client.json(
        "POST", f"/api/user/runs/{urllib.parse.quote(run_id)}/cancel", {}
    )
    require(cancelled.get("status") == "cancelled", f"cancel failed: {cancelled}")
    require(cancelled.get("process_stopped") is True, f"process remained active: {cancelled}")
    wait_for_state(client, run_id, "cancelled", timeout, poll_interval)
    return validate_terminal_run(client, artifact_dir, project_id, run_id, "cancelled")


def write_evidence(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = path.with_suffix(path.suffix + ".pending")
    pending.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    pending.replace(path)


def write_worker_contract_self_test(run_dir: Path) -> None:
    job_dir = run_dir / "worker_jobs"
    result_dir = run_dir / "worker_results"
    job_dir.mkdir(parents=True)
    result_dir.mkdir(parents=True)
    digest = "a" * 64
    for worker in sorted(REQUIRED_WORKERS):
        job_id = f"{worker}_self_test"
        invocation_id = f"invocation-{worker}"
        job: dict[str, Any] = {
            "schema_version": "genoligand_worker_current_v2",
            "pipeline_mode": "genoligand_scientific_pipeline",
            "job_id": job_id,
            "worker": worker,
            "stage": "self_test",
            "run_id": run_dir.name,
            "invocation_id": invocation_id,
            "rust_owned_decision_logic": True,
            "decision_owner": "GenoLigand Rust core",
            "input_manifest": str(run_dir / "input.json"),
            "run_dir": str(run_dir),
            "science_store": str(run_dir / "science_store"),
            "expected_result_schema": "genoligand_worker_current_v2",
        }
        if worker in {"docking", "md_summary"}:
            job.update(
                {
                    "candidate_id": "candidate-self-test",
                    "candidate_digest": digest,
                    "identity_namespace": "self-test",
                    "candidate_selection": "rank=1",
                    "input_id": "input-self-test",
                    "record_id": "record-self-test",
                    "input_digest": digest,
                    "source_digest": digest,
                    "receptor_artifact": str(run_dir / "receptor.pdb"),
                    "receptor_digest": digest,
                }
            )
        if worker == "md_summary":
            job["platform_policy"] = "required"
        result = {
            "schema_version": "genoligand_worker_current_v2",
            "job_id": job_id,
            "worker": worker,
            "run_id": run_dir.name,
            "invocation_id": invocation_id,
            "status": "FAIL",
            "engine_version": "self-test",
            "execution_status": "failed",
            "reason": "self_test",
            "outcomes": [
                {
                    "status": "failed",
                    "reason": "self_test",
                    "required": True,
                    "database_id": worker,
                    "row_count": 0,
                }
            ],
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
        (job_dir / f"{job_id}.json").write_text(
            json.dumps(job, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        (result_dir / f"{job_id}.json").write_text(
            json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        if worker == "mmseqs_alignment":
            fallback_job = dict(job)
            fallback_job["job_id"] = f"{worker}_rust_fallback_self_test"
            fallback_job["invocation_id"] = f"invocation-{worker}-rust-fallback"
            fallback_result = dict(result)
            fallback_result["job_id"] = fallback_job["job_id"]
            fallback_result["invocation_id"] = fallback_job["invocation_id"]
            fallback_result["engine_version"] = "worker unavailable: self_test"
            fallback_result["reason"] = "self_test_worker_unavailable"
            (job_dir / f"{fallback_job['job_id']}.json").write_text(
                json.dumps(fallback_job, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
            (result_dir / f"{fallback_result['job_id']}.json").write_text(
                json.dumps(fallback_result, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )


def write_case_contract_self_test(root: Path) -> None:
    identity = root / "identity-contracts"
    tool_outputs = identity / "tool_outputs"
    tool_outputs.mkdir(parents=True)
    (identity / "input_conversion_manifest.tsv").write_text(
        "input_id\trecord_id\tformat\tconversion_status\n"
        "input_2\trecord_1\tplain_text\tconverted\n"
        "input_3\tazm_1\tsdf\tconverted\n"
        "input_3\tazm_2\tsdf\tconverted\n",
        encoding="utf-8",
    )
    known_rows = []
    for input_id, record_id in (
        ("input_2", "record_1"),
        ("input_3", "azm_1"),
        ("input_3", "azm_2"),
    ):
        for source in ("ChEMBL", "BindingDB"):
            known_rows.append(
                f"{input_id}\t{source}\tP00918\tCA2\tHomo sapiens\tenzyme\t1.0\tligand\tbound\t{record_id}"
            )
    (identity / "known_database_matches.tsv").write_text(
        "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\n"
        + "\n".join(known_rows)
        + "\n",
        encoding="utf-8",
    )
    ranking_rows = [
        ("candidate_1", "input_2", "record_1"),
        ("candidate_2", "input_3", "azm_1"),
        ("candidate_3", "input_3", "azm_2"),
    ]
    (identity / "target_candidate_ranking.tsv").write_text(
        "candidate_id\ttarget_id\tinput_id\trecord_id\n"
        + "".join(
            f"{candidate}\tP00918\t{input_id}\t{record_id}\n"
            for candidate, input_id, record_id in ranking_rows
        ),
        encoding="utf-8",
    )
    (identity / "complex_candidate_selection.tsv").write_text(
        "candidate_id\tinput_id\trecord_id\n"
        "candidate_2\tinput_3\tazm_1\n"
        "candidate_3\tinput_3\tazm_2\n",
        encoding="utf-8",
    )
    indexed = {
        "entries": [
            {"candidate_id": "candidate_2"},
            {"candidate_id": "candidate_3"},
        ]
    }
    for kind in ("docking", "md"):
        (tool_outputs / f"{kind}_manifest_index.json").write_text(
            json.dumps(indexed) + "\n", encoding="utf-8"
        )
    identity_result = validate_candidate_identity_continuation(identity)
    require(
        identity_result["same_target_project_inputs"]["input_records"] == 3
        and len(identity_result["multi_record_sdf"]["selected_candidates"]) == 2,
        "candidate identity continuation self-test changed",
    )

    peptide = root / "peptide-contracts"
    (peptide / "tool_outputs").mkdir(parents=True)
    for name in (
        "peptide_conformer_coordinates.tsv",
        "peptide_conformer_ensemble.tsv",
        "peptide_conformer_summary.tsv",
        "peptide_conformer_metrics.tsv",
    ):
        (peptide / "tool_outputs" / name).write_text("fixture\n", encoding="utf-8")
    (peptide / "md_summary.tsv").write_text("candidate_id\tmd_stability_score\n", encoding="utf-8")
    (peptide / "research_report.md").write_text("RDKit conformer ensemble\n", encoding="utf-8")
    peptide_manifest = {
        "artifact_schema_version": "genoligand_peptide_conformer_v1",
        "method": "rdkit_peptide_conformer_ensemble",
        "method_class": "conformer_ensemble",
        "platform": "RDKit",
        "has_receptor": False,
        "uses_openmm": False,
        "md_score": 0.0,
        "conformer_count": 2,
        "coordinate_artifact": "tool_outputs/peptide_conformer_coordinates.tsv",
        "ensemble_artifact": "tool_outputs/peptide_conformer_ensemble.tsv",
        "metric_artifacts": [
            "tool_outputs/peptide_conformer_summary.tsv",
            "tool_outputs/peptide_conformer_metrics.tsv",
        ],
    }
    (peptide / "tool_outputs" / "peptide_manifest.json").write_text(
        json.dumps(peptide_manifest) + "\n", encoding="utf-8"
    )
    (peptide / "tool_outputs" / "md_manifest_index.json").write_text(
        json.dumps(
            {
                "entries": [
                    {
                        "candidate_id": "peptide_candidate",
                        "status": "completed",
                        "manifest": "tool_outputs/peptide_manifest.json",
                    }
                ]
            }
        )
        + "\n",
        encoding="utf-8",
    )
    peptide_result = validate_md_case_semantics(peptide, "peptide_conformer")
    require(peptide_result["uses_openmm"] is False, "peptide semantics self-test changed")

    complex_run = root / "complex-contracts"
    (complex_run / "tool_outputs").mkdir(parents=True)
    for name in (
        "receptor.pdb",
        "ligand.tsv",
        "complex.json",
        "protocol.tsv",
        "state.tsv",
        "trajectory.tsv",
    ):
        (complex_run / "tool_outputs" / name).write_text("fixture\n", encoding="utf-8")
    complex_manifest = {
        "method": "openmm_receptor_ligand_complex_dynamics",
        "platform": "CPU",
        "system": {
            "solvent_mode": "explicit",
            "nonbonded_method": "PME",
            "total_atom_count": 10,
        },
        "frame_count": 2,
        "state_row_count": 2,
        "receptor_coordinate": "tool_outputs/receptor.pdb",
        "ligand_coordinate": "tool_outputs/ligand.tsv",
        "complex_manifest": "tool_outputs/complex.json",
        "protocol_artifact": "tool_outputs/protocol.tsv",
        "state_artifact": "tool_outputs/state.tsv",
        "trajectory": "tool_outputs/trajectory.tsv",
    }
    (complex_run / "tool_outputs" / "complex_manifest.json").write_text(
        json.dumps(complex_manifest) + "\n", encoding="utf-8"
    )
    (complex_run / "tool_outputs" / "provenance.json").write_text(
        json.dumps({"tool_name": "OpenMM"}) + "\n", encoding="utf-8"
    )
    (complex_run / "tool_outputs" / "md_manifest_index.json").write_text(
        json.dumps(
            {
                "entries": [
                    {
                        "candidate_id": "complex_candidate",
                        "status": "completed",
                        "manifest": "tool_outputs/complex_manifest.json",
                        "artifact_provenance": "tool_outputs/provenance.json",
                    }
                ]
            }
        )
        + "\n",
        encoding="utf-8",
    )
    complex_result = validate_md_case_semantics(
        complex_run, "candidate_identity_and_complex_md"
    )
    require(len(complex_result["facts"]) == 1, "complex MD semantics self-test changed")


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="genoligand-api-acceptance-self-test-") as tmp:
        root = Path(tmp)
        cases = write_input_fixtures(root)
        labels = [case.label for case in cases]
        require(labels == ["smiles", "peptide", "fasta", "fastq", "genbank", "pdb"], "fixture labels changed")
        require({case.expected_kind for case in cases} == REQUIRED_INPUT_KINDS, "fixture kinds changed")
        require(REQUIRED_FORMAT_CASES <= set(labels), "required format coverage changed")
        for case in cases:
            require(isinstance(case.manifest.get("inputs"), list), f"{case.label} inputs missing")
            require(isinstance(case.manifest.get("files"), list), f"{case.label} files missing")
        small_molecule = cases[0]
        require(
            small_molecule.acceptance_profile == "candidate_identity_and_complex_md"
            and len(small_molecule.manifest["inputs"]) == 2
            and len(small_molecule.manifest["files"]) == 1,
            "small-molecule identity fixture changed",
        )
        sdf_path = Path(small_molecule.manifest["files"][0]["persisted_path"])
        require(
            sdf_path.read_text(encoding="utf-8").count("$$$$") == 2,
            "multi-record SDF fixture changed",
        )
        require(
            cases[1].acceptance_profile == "peptide_conformer",
            "peptide semantic profile changed",
        )
        require(parse_tsv("candidate_id\tscore\nC1\t1\n", "self-test") == [{"candidate_id": "C1", "score": "1"}], "TSV parser changed")
        require(
            canonical_json_sha256(
                {
                    "schema_version": "genoligand_target_ranking_v1",
                    "known_database_weight": 0.5,
                    "metagenome_weight": 0.25,
                    "structure_weight": 0.1,
                    "docking_weight": 0.05,
                    "md_weight": 0.1,
                }
            )
            == "aacee02f2226d92881a901dd96d3cf040694d2530ad62e1028e7f590fa7f713d",
            "ranking policy canonical digest changed",
        )
        for state in TERMINAL_STATES:
            run = root / state
            run.mkdir()
            (run / "run_state.txt").write_text(state + "\n", encoding="utf-8")
            require(read_trimmed(run / "run_state.txt") == state, f"state read failed: {state}")
        completed = root / "completed-worker-contracts"
        completed.mkdir()
        write_worker_contract_self_test(completed)
        worker_contracts = validate_worker_contract_artifacts(completed)
        require(
            set(worker_contracts["workers"]) == REQUIRED_WORKERS,
            "worker schema self-test coverage changed",
        )
        require(
            worker_contracts["workers"]["mmseqs_alignment"]["jobs"] == 2
            and worker_contracts["workers"]["mmseqs_alignment"]["results"] == 2
            and all(
                stats["jobs"] == 1 and stats["results"] == 1
                for worker, stats in worker_contracts["workers"].items()
                if worker != "mmseqs_alignment"
            ),
            "worker schema self-test file counts changed",
        )
        variants = worker_contracts["workers"]["mmseqs_alignment"]["contract_variants"]
        require(
            variants.get("python_failure") == 1
            and variants.get("rust_fallback_failure") == 1,
            "worker failure/fallback classification changed",
        )
        write_case_contract_self_test(root)
    print("server_api_acceptance_self_test\tPASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://127.0.0.1:19090")
    parser.add_argument("--artifact-dir", type=Path)
    parser.add_argument("--fixture-root", type=Path)
    parser.add_argument("--evidence-out", type=Path)
    parser.add_argument("--timeout-seconds", type=float, default=1800.0)
    parser.add_argument("--request-timeout-seconds", type=float, default=60.0)
    parser.add_argument("--poll-interval-seconds", type=float, default=0.25)
    parser.add_argument("--expected-version")
    parser.add_argument("--expected-commit")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    require(args.artifact_dir is not None, "--artifact-dir is required")
    require(bool(str(args.expected_version or "").strip()), "--expected-version is required")
    require(valid_git_commit(args.expected_commit), "--expected-commit must be a 40-character Git commit")
    artifact_dir = args.artifact_dir.resolve()
    require(artifact_dir.is_dir(), f"artifact directory does not exist: {artifact_dir}")
    fixture_root = (
        args.fixture_root.resolve()
        if args.fixture_root is not None
        else artifact_dir / "api-acceptance-inputs"
    )
    cases = write_input_fixtures(fixture_root)
    client = ApiClient(args.base_url, args.request_timeout_seconds)

    health = client.json("GET", "/api/user/health")
    require(health.get("status") == "ready", f"server health failed: {health}")
    readiness = health.get("science_store_readiness")
    require(
        isinstance(readiness, dict)
        and readiness.get("schema_version") == "genoligand_science_store_readiness_v1"
        and readiness.get("status") == "ready"
        and readiness.get("ready") is True
        and readiness.get("blockers") == []
        and valid_sha256(readiness.get("evidence_digest")),
        f"science store is not ready: {readiness}",
    )
    require(health.get("version") == args.expected_version, f"version mismatch: {health}")
    require(
        health.get("commit") == args.expected_commit
        and health.get("build_id") == args.expected_commit,
        f"commit mismatch: {health}",
    )
    runtime = health.get("runtime")
    require(
        isinstance(runtime, dict)
        and runtime.get("status") == "verified"
        and runtime.get("commit") == args.expected_commit
        and valid_sha256(runtime.get("manifest_digest")),
        f"runtime binary identity is not verified: {runtime}",
    )
    capabilities = client.json("GET", "/api/user/inputs/capabilities")
    validate_input_capabilities(capabilities)

    completed = [
        run_completed_input_case(
            client,
            artifact_dir,
            case,
            args.timeout_seconds,
            args.poll_interval_seconds,
        )
        for case in cases
    ]
    aggregate = validate_completed_aggregate(completed)
    states = {
        "blocked": run_blocked_case(
            client, artifact_dir, args.timeout_seconds, args.poll_interval_seconds
        ),
        "failed": run_failed_case(
            client,
            artifact_dir,
            fixture_root,
            args.timeout_seconds,
            args.poll_interval_seconds,
        ),
        "cancelled": run_cancelled_case(
            client,
            artifact_dir,
            fixture_root,
            args.timeout_seconds,
            args.poll_interval_seconds,
        ),
    }
    aggregate["worker_failure_and_fallback"] = summarize_worker_failure_and_fallback(
        [*completed, *states.values()]
    )
    evidence = {
        "schema_version": SCHEMA_VERSION,
        "base_url": args.base_url,
        "artifact_dir": str(artifact_dir),
        "fixture_root": str(fixture_root),
        "server": {
            "version": health.get("version"),
            "commit": health.get("commit"),
            "runtime": health.get("runtime"),
            "readiness_evidence_digest": readiness.get("evidence_digest"),
        },
        "completed_inputs": completed,
        "completed_aggregate": aggregate,
        "terminal_states": {"completed": completed[0], **states},
        "done_marker_policy": "nextflow_only",
    }
    evidence_out = (
        args.evidence_out.resolve()
        if args.evidence_out is not None
        else fixture_root / "server_api_acceptance_evidence.json"
    )
    write_evidence(evidence_out, evidence)
    print(f"server_api_acceptance\tPASS\tevidence={evidence_out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
