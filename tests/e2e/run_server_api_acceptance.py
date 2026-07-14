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


SCHEMA_VERSION = "genoligand_server_api_acceptance_v1"
TERMINAL_STATES = {"completed", "blocked", "failed", "cancelled"}
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
                    {"field": "small_molecule", "value": "CCO"},
                ],
                "files": [],
            },
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
    require(valid_sha256(context.get("ranking_policy_digest")), "ranking policy digest is invalid")
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
            "top_n": candidate_outcomes,
            "pipeline_invocation_id": invocation,
            "candidate_export_rows": candidate_export_rows,
            "evidence_export_rows": evidence_export_rows,
        }
    else:
        require(not report_visible, f"{expected} run exposes a report")
        require(download_status == 404, f"{expected} report download returned {download_status}")
        require(candidate_count == 0 and evidence_count == 0, f"{expected} run exposes results")

    return {
        "project_id": project_id,
        "run_id": run_id,
        "state": expected,
        "failure_reason": failure_reason,
        "candidate_count": candidate_count,
        "evidence_count": evidence_count,
        "report_file": report_path.is_file(),
        "report_visible": report_visible,
        "done_marker": False,
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
    return {
        "canonical_kinds": sorted(kinds),
        "format_cases": sorted(REQUIRED_FORMAT_CASES),
        "search_outcomes": search,
        "max_top_n": max(int(row.get("selected", 0)) for row in top_n),
        "docking_completed": docking_completed,
        "md_completed": md_completed,
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
        require(parse_tsv("candidate_id\tscore\nC1\t1\n", "self-test") == [{"candidate_id": "C1", "score": "1"}], "TSV parser changed")
        for state in TERMINAL_STATES:
            run = root / state
            run.mkdir()
            (run / "run_state.txt").write_text(state + "\n", encoding="utf-8")
            require(read_trimmed(run / "run_state.txt") == state, f"state read failed: {state}")
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
    require(isinstance(readiness, dict) and readiness.get("ready") is True, f"science store is not ready: {readiness}")
    if args.expected_version:
        require(health.get("version") == args.expected_version, f"version mismatch: {health}")
    if args.expected_commit:
        require(health.get("commit") == args.expected_commit, f"commit mismatch: {health}")
    capabilities = client.json("GET", "/api/user/inputs/capabilities")
    capability_text = json.dumps(capabilities, ensure_ascii=False)
    for expected in (*sorted(REQUIRED_INPUT_KINDS), "fasta", "fastq", "gb", "pdb"):
        require(expected in capability_text, f"input capability missing: {expected}")

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
