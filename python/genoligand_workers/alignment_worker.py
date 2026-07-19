from __future__ import annotations

import csv
import hashlib
import json
import math
import os
import shutil
import subprocess
from pathlib import Path
from typing import Any, NamedTuple

from genoligand_workers.common import WorkerExecution, WorkerOutcome
from genoligand_workers.query_structure_worker import (
    ARTIFACT_SCHEMA_VERSION as QUERY_STRUCTURE_ARTIFACT_SCHEMA_VERSION,
    FEATURE_SCHEMA_VERSION as QUERY_STRUCTURE_FEATURE_SCHEMA_VERSION,
    INDEX_HEADER as QUERY_STRUCTURE_INDEX_HEADER,
    METHOD_VERSION as QUERY_STRUCTURE_METHOD_VERSION,
)
from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


FOLDSEEK_MATCH_HEADER = (
    "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\t"
    "matched_features\tevidence\trecord_id\tsource_database_id\tsource_digest\t"
    "builder_version\tfoldseek_version\tcurrent_run_query_id\tquery_digest\ttm_score\n"
)
ALIGNMENT_MANIFEST_SCHEMA_VERSION = "genoligand_alignment_database_manifest_v2"
FOLDSEEK_STRUCTURE_SUFFIXES = {".pdb", ".ent", ".cif", ".mmcif"}
QUERY_STRUCTURE_FORMATS = {"pdb", "ent", "cif", "mmcif"}
ALIGNMENT_MANIFEST_REQUIRED_FIELDS = {
    "source_id",
    "manifest_schema_version",
    "source_namespace",
    "source_class",
    "source_digest",
    "source_files",
    "source_bytes",
    "builder_version",
    "db_path",
    "status",
}

MMSEQS_MATCH_HEADER = (
    "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\t"
    "matched_features\tevidence\tsource_database_id\tsource_namespace\tsource_class\t"
    "source_digest\tbuilder_version\tmmseqs_version\tcurrent_run_query_id\trecord_id\t"
    "query_kind\talignment_length\tquery_coverage\ttarget_coverage\talignment_cigar\n"
)


class SequenceInput(NamedTuple):
    query_id: str
    input_id: str
    record_id: str
    canonical_kind: str
    sequence: str


class QueryBatch(NamedTuple):
    name: str
    dbtype: str
    search_type: str
    records: list[SequenceInput]


class TargetDatabase(NamedTuple):
    database_id: str
    namespace: str
    source_class: str
    source_digest: str
    builder_version: str
    path: Path


class FoldseekTargetDatabase(NamedTuple):
    database_id: str
    source_digest: str
    source_files: int
    source_bytes: int
    builder_version: str
    path: Path


class StructureQuery(NamedTuple):
    path: Path
    input_id: str
    record_id: str
    query_id: str
    query_digest: str


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        execution = run_alignment_job(job)
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        reason = safe_note(str(exc)) or "alignment_worker_failed"
        execution = WorkerExecution(
            "failed",
            reason,
            f"alignment worker failed: {reason}",
            (WorkerOutcome("failed", reason, True, str(job.get("worker", "alignment")), 0),),
        )
        try:
            run_dir = runtime_path(str(job["run_dir"]))
            tool_outputs(run_dir).mkdir(parents=True, exist_ok=True)
            if str(job.get("worker", "")).startswith("foldseek"):
                write_status(tool_outputs(run_dir) / "foldseek_alignment_status.tsv", "failed", reason, 0)
            else:
                write_status(tool_outputs(run_dir) / "mmseqs_alignment_status.tsv", "failed", reason, 0)
        except Exception:
            pass
    write_result(
        args.out,
        job,
        execution.result_status,
        execution.engine_version,
        execution_status=execution.status,
        reason=execution.reason,
        outcomes=execution.outcome_dicts(),
    )
    return 0 if execution.status in {"completed", "not_applicable"} else 2


def run_alignment_job(job: dict[str, Any]) -> WorkerExecution:
    worker = str(job.get("worker", ""))
    run_dir = runtime_path(str(job["run_dir"]))
    science_store = runtime_path(str(job["science_store"]))
    if worker == "foldseek_alignment":
        return run_foldseek_alignment(run_dir, science_store)
    return run_mmseqs_alignment(run_dir, science_store)


def run_mmseqs_alignment(run_dir: Path, science_store: Path) -> WorkerExecution:
    out_dir = tool_outputs(run_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    matches_path = out_dir / "mmseqs_alignment_matches.tsv"
    status_path = out_dir / "mmseqs_alignment_status.tsv"
    write_text(matches_path, MMSEQS_MATCH_HEADER)

    sequences = read_sequence_inputs(run_dir)
    if not sequences:
        reason = "no_sequence_input"
        write_status(status_path, "not_applicable", reason, 0)
        return WorkerExecution(
            "not_applicable",
            reason,
            "MMseqs2 not executed",
            (WorkerOutcome("not_applicable", reason, False, "mmseqs", 0),),
        )

    targets = mmseqs_target_databases(science_store)
    if not targets:
        reason = "no_persistent_mmseqs_databases"
        write_status(status_path, "not_ready", reason, 0)
        return WorkerExecution(
            "not_ready",
            reason,
            "MMseqs2 not executed",
            (WorkerOutcome("not_ready", reason, True, "mmseqs_target_manifest", 0),),
        )

    mmseqs = shutil.which("mmseqs")
    if not mmseqs:
        reason = "missing_mmseqs"
        write_status(status_path, "not_ready", reason, 0)
        return WorkerExecution(
            "not_ready",
            reason,
            "MMseqs2 executable missing",
            tuple(
                WorkerOutcome("not_ready", reason, True, target.database_id, 0)
                for target in targets
            ),
        )

    mmseqs_version = executable_version(mmseqs, timeout=30)
    batches = sequence_query_batches(sequences)
    query_databases: list[tuple[QueryBatch, Path]] = []
    raw_dir = out_dir / "mmseqs_raw"
    raw_dir.mkdir(parents=True, exist_ok=True)
    tmp_root = out_dir / "mmseqs_tmp"
    tmp_root.mkdir(parents=True, exist_ok=True)
    timeout = int(os.environ.get("GENOLIGAND_ALIGNMENT_TIMEOUT_SECONDS", "900"))
    for batch in batches:
        query = out_dir / f"mmseqs_query_{batch.name}.fasta"
        query_db = out_dir / f"mmseqs_query_{batch.name}_db"
        write_fasta(query, batch.records)
        run_command(
            [mmseqs, "createdb", str(query), str(query_db), "--dbtype", batch.dbtype],
            timeout,
            f"mmseqs_createdb_{batch.name}",
        )
        query_databases.append((batch, query_db))

    rows: list[str] = []
    outcomes: list[WorkerOutcome] = []
    for target_index, target in enumerate(targets, start=1):
        target_rows: list[str] = []
        failure_reason = ""
        for batch, query_db in query_databases:
            stem = f"target_{target_index}_{batch.name}"
            raw = raw_dir / f"{stem}.m8"
            tmp = tmp_root / stem
            result_db = raw_dir / f"{stem}_result"
            command = [
                mmseqs,
                "search",
                str(query_db),
                str(target.path),
                str(result_db),
                str(tmp),
                "-a",
                "--search-type",
                batch.search_type,
                *mmseqs_prefilter_options(target),
                "--threads",
                os.environ.get("GENOLIGAND_ALIGNMENT_THREADS", "1"),
            ]
            completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
            if completed.returncode != 0:
                failure_reason = (
                    safe_note(first_line(completed.stderr or completed.stdout))
                    or f"mmseqs_search_failed_{batch.name}"
                )
                break
            command = [
                mmseqs,
                "convertalis",
                str(query_db),
                str(target.path),
                str(result_db),
                str(raw),
                "--format-output",
                "query,target,pident,alnlen,qcov,tcov,evalue,bits,qstart,qend,tstart,tend,cigar",
            ]
            completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
            if completed.returncode != 0:
                failure_reason = (
                    safe_note(first_line(completed.stderr or completed.stdout))
                    or f"mmseqs_convertalis_failed_{batch.name}"
                )
                break
            identities = {record.query_id: record for record in batch.records}
            target_rows.extend(mmseqs_rows(raw, target, identities, mmseqs_version))
        rows.extend(target_rows)
        if failure_reason:
            outcomes.append(
                WorkerOutcome("failed", failure_reason, True, target.database_id, len(target_rows))
            )
        else:
            outcomes.append(
                WorkerOutcome(
                    "completed", "search_completed", True, target.database_id, len(target_rows)
                )
            )
    write_text(matches_path, MMSEQS_MATCH_HEADER + "".join(rows))
    failures = [outcome for outcome in outcomes if outcome.status == "failed"]
    version = f"{mmseqs_version}; targets={len(targets)}; matches={len(rows)}"
    if failures:
        reason = ";".join(f"{outcome.database_id}:{outcome.reason}" for outcome in failures[:4])
        write_status(status_path, "failed", reason, len(rows))
        return WorkerExecution("failed", reason, version, tuple(outcomes))
    reason = "search_completed" if rows else "search_completed_no_hits"
    write_status(status_path, "completed", reason, len(rows))
    return WorkerExecution("completed", reason, version, tuple(outcomes))


def run_foldseek_alignment(run_dir: Path, science_store: Path) -> WorkerExecution:
    out_dir = tool_outputs(run_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    matches_path = out_dir / "foldseek_structure_matches.tsv"
    status_path = out_dir / "foldseek_alignment_status.tsv"
    write_text(matches_path, FOLDSEEK_MATCH_HEADER)

    queries = structure_queries(run_dir)
    if not queries:
        reason = "no_structure_query"
        write_status(status_path, "not_applicable", reason, 0)
        return WorkerExecution(
            "not_applicable",
            reason,
            "Foldseek not executed",
            (WorkerOutcome("not_applicable", reason, False, "foldseek", 0),),
        )

    targets = foldseek_target_databases(science_store)
    if not targets:
        reason = "no_persistent_foldseek_database"
        write_status(status_path, "not_ready", reason, 0)
        return WorkerExecution(
            "not_ready",
            reason,
            "Foldseek not executed",
            (WorkerOutcome("not_ready", reason, True, "foldseek_target_manifest", 0),),
        )
    foldseek = shutil.which("foldseek")
    if not foldseek:
        reason = "missing_foldseek"
        write_status(status_path, "not_ready", reason, 0)
        return WorkerExecution(
            "not_ready",
            reason,
            "Foldseek executable missing",
            tuple(
                WorkerOutcome("not_ready", reason, True, target.database_id, 0)
                for target in targets
            ),
        )

    foldseek_version = executable_version(foldseek, timeout=30)
    raw_dir = out_dir / "foldseek_raw"
    raw_dir.mkdir(parents=True, exist_ok=True)
    tmp_root = out_dir / "foldseek_tmp"
    tmp_root.mkdir(parents=True, exist_ok=True)
    query_db = out_dir / "foldseek_query_db"
    timeout = int(os.environ.get("GENOLIGAND_ALIGNMENT_TIMEOUT_SECONDS", "900"))
    query_sources = prepare_foldseek_query_sources(run_dir, tmp_root, queries)
    run_command(
        [foldseek, "createdb", str(query_sources), str(query_db)],
        timeout,
        "foldseek_createdb",
    )
    query_identities = foldseek_query_identities(query_db, queries)
    rows: list[str] = []
    outcomes: list[WorkerOutcome] = []
    for index, target in enumerate(targets, start=1):
        raw = raw_dir / f"target_{index}.m8"
        tmp = tmp_root / f"target_{index}"
        result_db = raw_dir / f"target_{index}_result"
        command = [
            foldseek,
            "search",
            str(query_db),
            str(target.path),
            str(result_db),
            str(tmp),
            "-a",
            "--threads",
            os.environ.get("GENOLIGAND_ALIGNMENT_THREADS", "1"),
        ]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        if completed.returncode != 0:
            reason = safe_note(first_line(completed.stderr or completed.stdout)) or "foldseek_search_failed"
            outcomes.append(WorkerOutcome("failed", reason, True, target.database_id, 0))
            continue
        command = [
            foldseek,
            "convertalis",
            str(query_db),
            str(target.path),
            str(result_db),
            str(raw),
            "--format-output",
            "query,target,pident,alntmscore,evalue,bits",
        ]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        if completed.returncode != 0:
            reason = safe_note(first_line(completed.stderr or completed.stdout)) or "foldseek_convertalis_failed"
            outcomes.append(WorkerOutcome("failed", reason, True, target.database_id, 0))
            continue
        target_rows = foldseek_rows(
            raw,
            target,
            foldseek_version,
            query_identities,
        )
        rows.extend(target_rows)
        outcomes.append(
            WorkerOutcome("completed", "search_completed", True, target.database_id, len(target_rows))
        )
    write_text(matches_path, FOLDSEEK_MATCH_HEADER + "".join(rows))
    failures = [outcome for outcome in outcomes if outcome.status == "failed"]
    if failures:
        reason = ";".join(f"{outcome.database_id}:{outcome.reason}" for outcome in failures[:4])
        write_status(status_path, "failed", reason, len(rows))
        return WorkerExecution(
            "failed",
            reason,
            f"{foldseek_version}; targets={len(targets)}; matches={len(rows)}",
            tuple(outcomes),
        )
    reason = "search_completed" if rows else "search_completed_no_hits"
    write_status(status_path, "completed", reason, len(rows))
    return WorkerExecution(
        "completed",
        reason,
        f"{foldseek_version}; targets={len(targets)}; matches={len(rows)}",
        tuple(outcomes),
    )


def runtime_path(value: str) -> Path:
    path = Path(value)
    if path.exists():
        return path
    if len(value) >= 3 and value[1] == ":" and value[2] in "\\/":
        drive = value[0].lower()
        rest = value[3:].replace("\\", "/")
        return Path(f"/mnt/{drive}/{rest}")
    return path


def run_command(command: list[str], timeout: int, label: str) -> None:
    completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if completed.returncode != 0:
        detail = first_line(completed.stderr or completed.stdout) or "command failed"
        raise ValueError(f"{label}_failed:{detail}")


def executable_version(command: str, timeout: int) -> str:
    completed = subprocess.run([command, "version"], capture_output=True, text=True, timeout=timeout)
    if completed.returncode != 0:
        detail = first_line(completed.stderr or completed.stdout)
        raise ValueError(f"mmseqs_version_failed:{detail}")
    version = next(
        (line.strip() for line in (completed.stdout or completed.stderr).splitlines() if line.strip()),
        "",
    )
    if not version:
        raise ValueError("mmseqs_version_missing")
    return safe_note(version)


def tool_outputs(run_dir: Path) -> Path:
    return run_dir / "tool_outputs"


def read_sequence_inputs(run_dir: Path) -> list[SequenceInput]:
    path = run_dir / "input_conversion_manifest.tsv"
    if not path.exists():
        return []
    rows: list[SequenceInput] = []
    query_identities: dict[str, tuple[str, str, str]] = {}
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            if str(row.get("conversion_status", "")).strip() != "converted":
                continue
            kind = str(row.get("canonical_kind", "")).strip()
            if kind not in {"peptide", "protein_ligand", "genome_or_metagenome"}:
                continue
            input_format = str(row.get("format", "")).strip().lower()
            if input_format not in {
                "plain_text",
                "fasta",
                "fa",
                "faa",
                "fna",
                "fastq",
                "fq",
                "gb",
                "gbk",
            }:
                continue
            value = decode_tsv_cell(str(row.get("canonical_value", ""))).strip().upper()
            sequence = "".join(ch for ch in value if ch.isalpha())
            if len(sequence) < 2:
                continue
            input_id = str(row.get("input_id", "input_1")).strip() or "input_1"
            record_id = str(row.get("record_id", "record_1")).strip() or "record_1"
            query_id = sequence_query_id(input_id, record_id)
            identity = (input_id, record_id, kind)
            if query_id in query_identities and query_identities[query_id] != identity:
                raise ValueError(f"sequence_query_identity_collision:{query_id}")
            query_identities[query_id] = identity
            rows.append(SequenceInput(query_id, input_id, record_id, kind, sequence))
    return rows


def sequence_query_id(input_id: str, record_id: str, variant: str = "") -> str:
    identity = [input_id, record_id]
    if variant:
        identity.append(variant)
    payload = json.dumps(
        identity, ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    return f"query_{hashlib.sha256(payload).hexdigest()}"


def sequence_query_batches(sequences: list[SequenceInput]) -> list[QueryBatch]:
    protein = [
        record
        for record in sequences
        if record.canonical_kind in {"peptide", "protein_ligand"}
    ]
    nucleotide: list[SequenceInput] = []
    translated: list[SequenceInput] = []
    for record in sequences:
        if record.canonical_kind != "genome_or_metagenome":
            continue
        sequence = record.sequence.replace("U", "T")
        nucleotide.append(
            SequenceInput(
                sequence_query_id(record.input_id, record.record_id, "nucleotide"),
                record.input_id,
                record.record_id,
                "nucleotide",
                sequence,
            )
        )
        for strand, frame, start, peptide in six_frame_orfs(sequence):
            variant = f"translated_protein:{strand}:{frame}:{start}:{len(peptide)}"
            translated.append(
                SequenceInput(
                    sequence_query_id(record.input_id, record.record_id, variant),
                    record.input_id,
                    record.record_id,
                    "translated_protein",
                    peptide,
                )
            )
    batches = [
        QueryBatch("protein", "1", "1", protein),
        QueryBatch("nucleotide", "2", "2", nucleotide),
        QueryBatch("translated_protein", "1", "1", translated),
    ]
    return [batch for batch in batches if batch.records]


CODON_TABLE = {
    "TTT": "F", "TTC": "F", "TTA": "L", "TTG": "L",
    "TCT": "S", "TCC": "S", "TCA": "S", "TCG": "S",
    "TAT": "Y", "TAC": "Y", "TAA": "*", "TAG": "*",
    "TGT": "C", "TGC": "C", "TGA": "*", "TGG": "W",
    "CTT": "L", "CTC": "L", "CTA": "L", "CTG": "L",
    "CCT": "P", "CCC": "P", "CCA": "P", "CCG": "P",
    "CAT": "H", "CAC": "H", "CAA": "Q", "CAG": "Q",
    "CGT": "R", "CGC": "R", "CGA": "R", "CGG": "R",
    "ATT": "I", "ATC": "I", "ATA": "I", "ATG": "M",
    "ACT": "T", "ACC": "T", "ACA": "T", "ACG": "T",
    "AAT": "N", "AAC": "N", "AAA": "K", "AAG": "K",
    "AGT": "S", "AGC": "S", "AGA": "R", "AGG": "R",
    "GTT": "V", "GTC": "V", "GTA": "V", "GTG": "V",
    "GCT": "A", "GCC": "A", "GCA": "A", "GCG": "A",
    "GAT": "D", "GAC": "D", "GAA": "E", "GAG": "E",
    "GGT": "G", "GGC": "G", "GGA": "G", "GGG": "G",
}


def six_frame_orfs(sequence: str, minimum_amino_acids: int = 3) -> list[tuple[str, int, int, str]]:
    dna = sequence.upper().replace("U", "T")
    reverse = dna.translate(str.maketrans("ACGTN", "TGCAN"))[::-1]
    out: list[tuple[str, int, int, str]] = []
    for strand, oriented in (("forward", dna), ("reverse", reverse)):
        for frame in range(3):
            peptide: list[str] = []
            peptide_start = frame
            for offset in range(frame, len(oriented) - 2, 3):
                amino_acid = CODON_TABLE.get(oriented[offset : offset + 3], "X")
                if amino_acid == "*":
                    if len(peptide) >= minimum_amino_acids:
                        out.append((strand, frame, peptide_start, "".join(peptide)))
                    peptide = []
                    peptide_start = offset + 3
                else:
                    peptide.append(amino_acid)
            if len(peptide) >= minimum_amino_acids:
                out.append((strand, frame, peptide_start, "".join(peptide)))
    return out


def mmseqs_target_databases(science_store: Path) -> list[TargetDatabase]:
    manifest = science_store / "indexes" / "mmseqs" / "mmseqs_target_databases.tsv"
    targets: list[TargetDatabase] = []
    for row in validated_alignment_manifest_rows(manifest, science_store, "mmseqs"):
        database_id = row["source_id"]
        path = resolved_manifest_database_path(science_store, row["db_path"])
        source_class = row["source_class"].lower()
        if source_class not in {"known", "metagenome"}:
            raise ValueError(f"mmseqs_target_source_class_invalid:{source_class}")
        if not mmseqs_database_ready(path):
            raise ValueError(f"mmseqs_target_database_not_ready:{database_id}")
        targets.append(
            TargetDatabase(
                database_id,
                row["source_namespace"],
                source_class,
                row["source_digest"].lower(),
                row["builder_version"],
                path,
            )
        )
    return sorted(dedupe_target_databases(targets), key=lambda target: str(target.path))


def foldseek_target_databases(science_store: Path) -> list[FoldseekTargetDatabase]:
    manifest = science_store / "indexes" / "foldseek" / "foldseek_target_databases.tsv"
    targets: list[FoldseekTargetDatabase] = []
    for row in validated_alignment_manifest_rows(manifest, science_store, "foldseek"):
        database_id = row["source_id"]
        path = resolved_manifest_database_path(science_store, row["db_path"])
        if not foldseek_database_ready(path):
            raise ValueError(f"foldseek_target_database_not_ready:{database_id}")
        targets.append(
            FoldseekTargetDatabase(
                database_id,
                row["source_digest"].lower(),
                int(row["source_files"]),
                int(row["source_bytes"]),
                row["builder_version"],
                path,
            )
        )
    deduped: dict[str, FoldseekTargetDatabase] = {}
    for target in targets:
        key = str(target.path.resolve())
        if key in deduped:
            raise ValueError(f"foldseek_target_database_duplicate:{target.database_id}")
        deduped[key] = target
    return sorted(deduped.values(), key=lambda target: (target.database_id, str(target.path)))


def validated_alignment_manifest_rows(
    manifest: Path,
    science_store: Path,
    tool: str,
) -> list[dict[str, str]]:
    if not manifest.is_file():
        return []
    with manifest.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        missing_headers = sorted(
            ALIGNMENT_MANIFEST_REQUIRED_FIELDS - set(reader.fieldnames or [])
        )
        if missing_headers:
            raise ValueError(
                f"{tool}_target_manifest_fields_missing:{','.join(missing_headers)}"
            )
        rows = [
            {str(key): str(value or "").strip() for key, value in row.items()}
            for row in reader
        ]
    for row in rows:
        database_id = row["source_id"]
        missing = sorted(
            field
            for field in ALIGNMENT_MANIFEST_REQUIRED_FIELDS
            if not row.get(field, "")
        )
        if missing:
            raise ValueError(
                f"{tool}_target_manifest_fields_missing:{database_id}:{','.join(missing)}"
            )
        if row["manifest_schema_version"] != ALIGNMENT_MANIFEST_SCHEMA_VERSION:
            raise ValueError(f"{tool}_target_manifest_schema_invalid:{database_id}")
        if row["status"] != "ready":
            raise ValueError(f"{tool}_target_database_not_ready:{database_id}")
        if not valid_digest(row["source_digest"].lower()):
            raise ValueError(f"{tool}_target_source_digest_invalid:{database_id}")
        try:
            source_files = int(row["source_files"])
            source_bytes = int(row["source_bytes"])
        except ValueError as exc:
            raise ValueError(f"{tool}_target_provenance_invalid:{database_id}") from exc
        if source_files <= 0 or source_bytes <= 0:
            raise ValueError(f"{tool}_target_provenance_invalid:{database_id}")
        resolved_manifest_database_path(science_store, row["db_path"])
    return rows


def resolved_manifest_database_path(science_store: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else science_store / path


def foldseek_target_database(science_store: Path) -> tuple[str, Path] | None:
    targets = foldseek_target_databases(science_store)
    if len(targets) != 1:
        return None
    return targets[0].database_id, targets[0].path


def dedupe_target_databases(targets: list[TargetDatabase]) -> list[TargetDatabase]:
    seen: set[str] = set()
    out: list[TargetDatabase] = []
    for target in targets:
        key = str(target.path)
        if key not in seen:
            seen.add(key)
            out.append(target)
    return out


def foldseek_database_ready(prefix: Path) -> bool:
    companions = [
        prefix,
        Path(f"{prefix}.dbtype"),
        Path(f"{prefix}.index"),
        Path(f"{prefix}.lookup"),
    ]
    try:
        if any(not path.is_file() or path.stat().st_size <= 0 for path in companions):
            return False
        if Path(f"{prefix}.dbtype").stat().st_size != 4:
            return False
        index_companions = {
            Path(f"{prefix}.idx"),
            Path(f"{prefix}.idx.dbtype"),
            Path(f"{prefix}.idx.index"),
        }
        present = {path for path in prefix.parent.glob(f"{prefix.name}.idx*") if path.is_file()}
        if present and present != index_companions:
            return False
        return not present or (
            all(path.stat().st_size > 0 for path in index_companions)
            and Path(f"{prefix}.idx.dbtype").stat().st_size == 4
        )
    except OSError:
        return False


def mmseqs_database_ready(prefix: Path) -> bool:
    dbtype = Path(str(prefix) + ".dbtype")
    try:
        return prefix.is_file() and prefix.stat().st_size > 0 and dbtype.is_file() and dbtype.stat().st_size > 0
    except OSError:
        return False


def mmseqs_prefilter_options(target: TargetDatabase) -> list[str]:
    if target.source_class == "metagenome":
        return ["--prefilter-mode", "1"]
    return []


def write_fasta(path: Path, rows: list[SequenceInput]) -> None:
    text = "".join(
        f">{record.query_id}\n{record.sequence}\n"
        for record in rows
    )
    write_text(path, text)


def mmseqs_rows(
    path: Path,
    target_database: TargetDatabase,
    query_identities: dict[str, SequenceInput],
    mmseqs_version: str,
) -> list[str]:
    if not path.exists():
        return []
    rows: list[str] = []
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            cols = line.rstrip("\n").split("\t")
            if len(cols) < 13:
                raise ValueError("mmseqs_alignment_backtrace_missing")
            (
                query,
                target,
                pident,
                alnlen,
                qcov,
                tcov,
                evalue,
                bits,
                qstart,
                qend,
                tstart,
                tend,
                cigar,
            ) = cols[:13]
            validate_mmseqs_alignment(
                pident, alnlen, qcov, tcov, evalue, bits, qstart, qend, tstart, tend, cigar
            )
            identity = query_identities.get(query)
            if identity is None:
                raise ValueError(f"mmseqs_query_identity_missing:{safe_note(query)}")
            score = alignment_score(pident, bits, qcov, tcov, alnlen)
            target_id = safe_id(target.split()[0])
            target_class = (
                "known_protein"
                if target_database.source_class == "known"
                else "potential_metagenome_sequence"
            )
            evidence = (
                f"mmseqs2_search;evalue={safe_note(evalue)};bits={safe_note(bits)};"
                f"pident={safe_note(pident)};alnlen={safe_note(alnlen)};"
                f"qcov={safe_note(qcov)};tcov={safe_note(tcov)};cigar={safe_note(cigar)};"
                f"target_database={safe_note(target_database.database_id)};"
                f"source_namespace={safe_note(target_database.namespace)};"
                f"source_class={safe_note(target_database.source_class)};"
                f"source_digest={safe_note(target_database.source_digest)};"
                f"builder_version={safe_note(target_database.builder_version)};"
                f"mmseqs_version={safe_note(mmseqs_version)};"
                f"query_id={safe_note(query)};record_id={safe_note(identity.record_id)}"
            )
            rows.append(
                f"{clean_cell(identity.input_id)}\t{clean_cell(target_database.namespace)}\t"
                f"{target_id}\t{clean_cell(target)}\tunknown\t{target_class}\t{score:.4f}\t"
                f"{clean_cell(target)}\t{evidence}\t{clean_cell(target_database.database_id)}\t"
                f"{clean_cell(target_database.namespace)}\t{target_database.source_class}\t"
                f"{target_database.source_digest}\t{clean_cell(target_database.builder_version)}\t"
                f"{clean_cell(mmseqs_version)}\t{clean_cell(query)}\t{clean_cell(identity.record_id)}\t"
                f"{clean_cell(identity.canonical_kind)}\t{clean_cell(alnlen)}\t"
                f"{clean_cell(qcov)}\t{clean_cell(tcov)}\t{clean_cell(cigar)}\n"
            )
    return rows


def validate_mmseqs_alignment(
    pident: str,
    alnlen: str,
    qcov: str,
    tcov: str,
    evalue: str,
    bits: str,
    qstart: str,
    qend: str,
    tstart: str,
    tend: str,
    cigar: str,
) -> None:
    try:
        identity = float(pident)
        alignment_length = int(alnlen)
        query_coverage = float(qcov)
        target_coverage = float(tcov)
        expectation = float(evalue)
        bit_score = float(bits)
        query_start, query_end = int(qstart), int(qend)
        target_start, target_end = int(tstart), int(tend)
    except ValueError as exc:
        raise ValueError("mmseqs_alignment_backtrace_invalid:numeric") from exc
    finite = (identity, query_coverage, target_coverage, expectation, bit_score)
    if (
        not all(math.isfinite(value) for value in finite)
        or not 0.0 <= identity <= 100.0
        or alignment_length <= 0
        or not 0.0 <= query_coverage <= 1.0
        or not 0.0 <= target_coverage <= 1.0
        or query_start <= 0
        or target_start <= 0
        or target_end <= 0
        or not cigar.strip()
        or cigar.strip() == "*"
    ):
        raise ValueError("mmseqs_alignment_backtrace_invalid:range")


def foldseek_rows(
    path: Path,
    target_database: FoldseekTargetDatabase,
    foldseek_version: str,
    query_identities: dict[str, StructureQuery],
) -> list[str]:
    if not path.exists():
        return []
    rows: list[str] = []
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            cols = line.rstrip("\n").split("\t")
            if len(cols) < 6:
                raise ValueError("foldseek_alignment_row_invalid")
            query, target, pident, tm_score, evalue, bits = cols[:6]
            query_identity = query_identities.get(query)
            if query_identity is None:
                raise ValueError(f"foldseek_query_identity_mismatch:{safe_note(query)}")
            try:
                identity_value = float(pident)
                tm_value = float(tm_score)
                evalue_value = float(evalue)
                bits_value = float(bits)
            except ValueError as exc:
                raise ValueError("foldseek_alignment_row_invalid") from exc
            if (
                not all(math.isfinite(value) for value in (identity_value, tm_value, evalue_value, bits_value))
                or not 0.0 <= identity_value <= 100.0
                or not 0.0 <= tm_value <= 1.0
                or evalue_value < 0.0
            ):
                raise ValueError("foldseek_alignment_row_invalid")
            score = max(0.0, min(1.0, 0.40 * alignment_score(pident, bits) + 0.60 * tm_value))
            target_id = safe_id(Path(target).stem or target.split()[0])
            evidence = (
                f"foldseek_search;evalue={safe_note(evalue)};bits={safe_note(bits)};"
                f"pident={safe_note(pident)};alntmscore={safe_note(tm_score)};"
                f"source_database_id={safe_note(target_database.database_id)};"
                f"source_digest={target_database.source_digest};"
                f"builder_version={safe_note(target_database.builder_version)};"
                f"foldseek_version={safe_note(foldseek_version)};"
                f"query_id={query_identity.query_id};query_digest={query_identity.query_digest};"
                f"input_id={safe_note(query_identity.input_id)};record_id={safe_note(query_identity.record_id)}"
            )
            rows.append(
                f"{clean_cell(query_identity.input_id)}\tFoldseek\t{target_id}\t{clean_cell(target)}\tunknown\t"
                f"onsite_structure_alignment\t{score:.4f}\t{clean_cell(target)}\t{evidence}\t"
                f"{clean_cell(query_identity.record_id)}\t{clean_cell(target_database.database_id)}\t"
                f"{target_database.source_digest}\t{clean_cell(target_database.builder_version)}\t"
                f"{clean_cell(foldseek_version)}\t{query_identity.query_id}\t"
                f"{query_identity.query_digest}\t{tm_value:.6f}\n"
            )
    return rows


def foldseek_lookup_names(database: Path) -> set[str]:
    lookup = next(
        (
            candidate
            for candidate in (Path(f"{database}.lookup"), Path(f"{database}.source"))
            if candidate.is_file() and candidate.stat().st_size > 0
        ),
        None,
    )
    if lookup is None:
        raise ValueError("foldseek_query_lookup_missing")
    names: set[str] = set()
    with lookup.open("r", encoding="utf-8", errors="strict") as handle:
        for line in handle:
            columns = line.rstrip("\r\n").split("\t")
            if len(columns) < 2 or not columns[0].isdigit() or not columns[1].strip():
                raise ValueError("foldseek_query_lookup_invalid")
            names.add(columns[1].strip())
    if not names:
        raise ValueError("foldseek_query_lookup_missing")
    return names


def foldseek_query_identities(
    database: Path,
    queries: list[StructureQuery],
) -> dict[str, StructureQuery]:
    by_query_id: dict[str, StructureQuery] = {}
    for query in queries:
        if query.query_id in by_query_id:
            raise ValueError(f"foldseek_query_id_collision:{query.query_id}")
        by_query_id[query.query_id] = query
    identities: dict[str, StructureQuery] = {}
    observed: set[str] = set()
    for native_name in foldseek_lookup_names(database):
        name = native_name.replace("\\", "/").rsplit("/", 1)[-1]
        matches = [
            query
            for query_id, query in by_query_id.items()
            if name == query_id
            or name.startswith(f"{query_id}_")
            or name.startswith(f"{query_id}.")
        ]
        if len(matches) != 1:
            raise ValueError(f"foldseek_query_identity_mismatch:{safe_note(native_name)}")
        identities[native_name] = matches[0]
        observed.add(matches[0].query_id)
    missing = set(by_query_id).difference(observed)
    if missing:
        raise ValueError(f"foldseek_query_lookup_incomplete:{safe_note(sorted(missing)[0])}")
    return identities


def prepare_foldseek_query_sources(
    run_dir: Path,
    tmp_root: Path,
    queries: list[StructureQuery],
) -> Path:
    root = run_dir.resolve(strict=True)
    resolved_tmp = tmp_root.resolve(strict=True)
    try:
        resolved_tmp.relative_to(root)
    except ValueError as exc:
        raise ValueError("foldseek_query_tmp_outside_run") from exc
    sources = resolved_tmp / "query_sources"
    if sources.exists() or sources.is_symlink():
        if is_link_like(sources) or not sources.is_dir():
            raise ValueError("foldseek_query_source_directory_invalid")
        shutil.rmtree(sources)
    sources.mkdir()
    for query in queries:
        suffix = structure_suffix(query.path)
        reference = sources / f"{query.query_id}{suffix}"
        try:
            os.link(query.path, reference)
        except OSError as hardlink_error:
            try:
                reference.symlink_to(query.path, target_is_directory=False)
            except OSError as symlink_error:
                raise ValueError(
                    f"foldseek_query_reference_create_failed:{safe_note(str(hardlink_error))}:"
                    f"{safe_note(str(symlink_error))}"
                ) from symlink_error
        if not reference.is_file() or not os.path.samefile(reference, query.path):
            raise ValueError("foldseek_query_reference_binding_invalid")
    return sources


def structure_queries(run_dir: Path) -> list[StructureQuery]:
    prepared_manifest = run_dir / "query_structure_manifest.tsv"
    if prepared_manifest.exists() or prepared_manifest.is_symlink():
        return prepared_structure_queries(run_dir, prepared_manifest)
    return legacy_structure_queries(run_dir)


def structure_query(run_dir: Path) -> StructureQuery | None:
    queries = structure_queries(run_dir)
    if not queries:
        return None
    if len(queries) != 1:
        raise ValueError("foldseek_query_identity_missing")
    return queries[0]


def legacy_structure_queries(run_dir: Path) -> list[StructureQuery]:
    manifest = run_dir / "input_conversion_manifest.tsv"
    if not manifest.is_file():
        return []
    with manifest.open("r", encoding="utf-8-sig", newline="") as handle:
        candidates = [
            row
            for row in csv.DictReader(handle, delimiter="\t")
            if str(row.get("conversion_status", "")).strip() == "converted"
            and str(row.get("format", "")).strip().lower() in {"pdb", "cif", "mmcif"}
            and str(row.get("input_id", "")).strip()
            and str(row.get("record_id", "")).strip()
        ]
    if not candidates:
        return []
    queries: list[StructureQuery] = []
    identities: set[tuple[str, str]] = set()
    for row in candidates:
        input_id = str(row.get("input_id", "")).strip()
        record_id = str(row.get("record_id", "")).strip()
        identity = (input_id, record_id)
        if identity in identities:
            raise ValueError("foldseek_query_identity_missing")
        identities.add(identity)
        declared_digest = str(row.get("content_sha256", "")).strip().lower()
        if not valid_digest(declared_digest):
            raise ValueError("foldseek_query_digest_invalid")
        query_path = structure_query_path(run_dir, row)
        if not query_path.is_file() or query_path.stat().st_size <= 0:
            raise ValueError("foldseek_query_path_missing")
        query_digest = sha256_path(query_path)
        if query_digest != declared_digest:
            raise ValueError("foldseek_query_digest_mismatch")
        queries.append(
            StructureQuery(
                query_path,
                input_id,
                record_id,
                structure_query_id(input_id, record_id, query_digest),
                query_digest,
            )
        )
    return queries


def prepared_structure_queries(run_dir: Path, index_path: Path) -> list[StructureQuery]:
    root = run_dir.resolve(strict=True)
    if is_link_like(index_path):
        raise ValueError("foldseek_query_manifest_link")
    invocation_path = run_dir / "worker_invocation_id.txt"
    if not invocation_path.is_file() or is_link_like(invocation_path):
        raise ValueError("foldseek_query_invocation_missing")
    invocation_id = invocation_path.read_text(encoding="utf-8-sig").strip()
    if not invocation_id:
        raise ValueError("foldseek_query_invocation_missing")
    conversion_path = run_dir / "input_conversion_manifest.tsv"
    conversion_rows = conversion_records(conversion_path)
    conversion_digest = sha256_path(conversion_path)
    with index_path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if reader.fieldnames != QUERY_STRUCTURE_INDEX_HEADER.rstrip("\n").split("\t"):
            raise ValueError("foldseek_query_manifest_schema_invalid")
        index_rows = list(reader)
    indexed: dict[tuple[str, str], dict[str, str]] = {}
    for row in index_rows:
        identity = (
            str(row.get("input_id", "")).strip(),
            str(row.get("record_id", "")).strip(),
        )
        if not all(identity) or identity in indexed:
            raise ValueError("foldseek_query_manifest_identity_invalid")
        indexed[identity] = row
    if set(indexed) != set(conversion_rows):
        raise ValueError("foldseek_query_manifest_identity_mismatch")

    queries: list[StructureQuery] = []
    for identity in sorted(indexed):
        row = indexed[identity]
        source = conversion_rows[identity]
        query = prepared_structure_query(
            root,
            row,
            source,
            invocation_id,
            conversion_digest,
        )
        if query is not None:
            queries.append(query)
    return queries


def conversion_records(path: Path) -> dict[tuple[str, str], dict[str, str]]:
    if not path.is_file() or is_link_like(path):
        raise ValueError("foldseek_query_conversion_manifest_missing")
    required = {
        "input_id",
        "record_id",
        "canonical_kind",
        "conversion_status",
        "format",
        "source_path",
        "content_sha256",
        "artifact",
    }
    records: dict[tuple[str, str], dict[str, str]] = {}
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if reader.fieldnames is None or not required.issubset(reader.fieldnames):
            raise ValueError("foldseek_query_conversion_manifest_schema_invalid")
        for row in reader:
            if str(row.get("conversion_status", "")).strip() != "converted":
                continue
            identity = (
                str(row.get("input_id", "")).strip(),
                str(row.get("record_id", "")).strip(),
            )
            if not all(identity) or identity in records:
                raise ValueError("foldseek_query_conversion_identity_invalid")
            records[identity] = row
    return records


def prepared_structure_query(
    run_dir: Path,
    row: dict[str, str],
    source: dict[str, str],
    invocation_id: str,
    conversion_digest: str,
) -> StructureQuery | None:
    input_id = str(row.get("input_id", "")).strip()
    record_id = str(row.get("record_id", "")).strip()
    input_kind = str(row.get("input_kind", "")).strip()
    input_digest = str(row.get("input_digest", "")).strip().lower()
    status = str(row.get("status", "")).strip()
    source_digest = str(source.get("content_sha256", "")).strip().lower()
    if (
        input_kind != str(source.get("canonical_kind", "")).strip()
        or not valid_digest(input_digest)
        or not valid_digest(source_digest)
        or status not in {"ready", "not_applicable"}
        or str(row.get("method", "")).strip() != QUERY_STRUCTURE_METHOD_VERSION
    ):
        raise ValueError("foldseek_query_manifest_binding_invalid")

    artifact_dir = query_structure_artifact_dir(run_dir, input_id, record_id, invocation_id)
    manifest_path = artifact_dir / "manifest.json"
    manifest = read_bound_json(run_dir, manifest_path, "foldseek_query_artifact_manifest_invalid")
    common = {
        "run_id": run_dir.name,
        "invocation_id": invocation_id,
        "input_id": input_id,
        "record_id": record_id,
        "input_kind": input_kind,
        "input_digest": input_digest,
        "source_content_sha256": source_digest,
        "method_version": QUERY_STRUCTURE_METHOD_VERSION,
    }
    if (
        manifest.get("schema_version") != QUERY_STRUCTURE_ARTIFACT_SCHEMA_VERSION
        or manifest.get("status") != status
        or any(manifest.get(field) != value for field, value in common.items())
        or not str(manifest.get("job_id", "")).strip()
    ):
        raise ValueError("foldseek_query_artifact_binding_invalid")

    features_value = str(row.get("features_json", "")).strip()
    coordinate_value = str(row.get("coordinate_artifact", "")).strip()
    feature_digest = str(row.get("feature_digest", "")).strip().lower()
    if status == "not_applicable":
        if (
            features_value
            or coordinate_value
            or feature_digest
            or manifest.get("features_json")
            or manifest.get("coordinate_artifact")
            or not str(manifest.get("reason", "")).strip()
        ):
            raise ValueError("foldseek_query_terminal_artifact_invalid")
        return None

    features_path = resolve_run_artifact(run_dir, features_value, "foldseek_query_features_invalid")
    if features_path.parent != artifact_dir.resolve(strict=True):
        raise ValueError("foldseek_query_features_parent_mismatch")
    if not valid_digest(feature_digest) or sha256_path(features_path) != feature_digest:
        raise ValueError("foldseek_query_feature_digest_mismatch")
    features = read_bound_json(run_dir, features_path, "foldseek_query_features_invalid")
    feature_common = {**common, "conversion_manifest_sha256": conversion_digest}
    if (
        features.get("schema_version") != QUERY_STRUCTURE_FEATURE_SCHEMA_VERSION
        or features.get("status") != "ready"
        or any(features.get(field) != value for field, value in feature_common.items())
        or features.get("job_id") != manifest.get("job_id")
        or manifest.get("conversion_manifest_sha256") != conversion_digest
        or manifest.get("features_json") != features_value
        or manifest.get("feature_digest") != feature_digest
        or manifest.get("coordinate_artifact") != coordinate_value
    ):
        raise ValueError("foldseek_query_features_binding_invalid")

    feature_coordinate = features.get("coordinate_artifact")
    if not isinstance(feature_coordinate, dict):
        raise ValueError("foldseek_query_coordinate_metadata_invalid")
    coordinate_digest = str(manifest.get("coordinate_sha256", "")).strip().lower()
    coordinate_bytes = manifest.get("coordinate_size_bytes")
    if (
        feature_coordinate.get("path") != coordinate_value
        or feature_coordinate.get("sha256") != coordinate_digest
        or feature_coordinate.get("size_bytes") != coordinate_bytes
    ):
        raise ValueError("foldseek_query_coordinate_metadata_invalid")
    if not coordinate_value:
        if coordinate_digest or coordinate_bytes != 0:
            raise ValueError("foldseek_query_coordinate_metadata_invalid")
        return None
    if (
        not valid_digest(coordinate_digest)
        or isinstance(coordinate_bytes, bool)
        or not isinstance(coordinate_bytes, int)
        or coordinate_bytes <= 0
    ):
        raise ValueError("foldseek_query_coordinate_metadata_invalid")
    coordinate = resolve_query_coordinate(
        run_dir,
        artifact_dir,
        coordinate_value,
        source,
        str(manifest.get("retention", "")),
    )
    if coordinate.stat().st_size != coordinate_bytes or sha256_path(coordinate) != coordinate_digest:
        raise ValueError("foldseek_query_coordinate_digest_mismatch")
    if structure_suffix(coordinate) not in FOLDSEEK_STRUCTURE_SUFFIXES:
        return None
    return StructureQuery(
        coordinate,
        input_id,
        record_id,
        structure_query_id(input_id, record_id, coordinate_digest),
        coordinate_digest,
    )


def query_structure_artifact_dir(
    run_dir: Path,
    input_id: str,
    record_id: str,
    invocation_id: str,
) -> Path:
    input_key = hashlib.sha256(input_id.encode("utf-8")).hexdigest()[:16]
    record_key = hashlib.sha256(record_id.encode("utf-8")).hexdigest()[:16]
    invocation_key = hashlib.sha256(invocation_id.encode("utf-8")).hexdigest()[:16]
    return run_dir / "tool_outputs" / "query_structure" / input_key / record_key / f"inv_{invocation_key}"


def read_bound_json(run_dir: Path, path: Path, error: str) -> dict[str, Any]:
    try:
        resolved = path.resolve(strict=True)
        resolved.relative_to(run_dir.resolve(strict=True))
        ensure_no_link_components(run_dir, path)
        with resolved.open("r", encoding="utf-8-sig") as handle:
            value = json.load(handle)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        raise ValueError(error) from exc
    if not isinstance(value, dict):
        raise ValueError(error)
    return value


def resolve_run_artifact(run_dir: Path, value: str, error: str) -> Path:
    if not value:
        raise ValueError(error)
    candidate = runtime_path(value)
    if candidate.is_absolute():
        raise ValueError(error)
    candidate = run_dir / candidate
    try:
        resolved = candidate.resolve(strict=True)
        resolved.relative_to(run_dir.resolve(strict=True))
        ensure_no_link_components(run_dir, candidate)
    except (OSError, ValueError) as exc:
        raise ValueError(error) from exc
    if not resolved.is_file():
        raise ValueError(error)
    return resolved


def resolve_query_coordinate(
    run_dir: Path,
    artifact_dir: Path,
    value: str,
    source: dict[str, str],
    retention: str,
) -> Path:
    declared = runtime_path(value)
    inside_run = not declared.is_absolute()
    candidate = declared if declared.is_absolute() else run_dir / declared
    if inside_run:
        ensure_no_link_components(run_dir, candidate)
    try:
        coordinate = candidate.resolve(strict=True)
    except OSError as exc:
        raise ValueError("foldseek_query_coordinate_missing") from exc
    if not coordinate.is_file():
        raise ValueError("foldseek_query_coordinate_missing")
    source_format = str(source.get("format", "")).strip().lower()
    original: Path | None = None
    if source_format in QUERY_STRUCTURE_FORMATS:
        original = structure_query_path(run_dir, source)
    if original is not None and os.path.samefile(coordinate, original):
        if retention != "original_input_reference_only":
            raise ValueError("foldseek_query_original_retention_invalid")
        return coordinate
    if not inside_run:
        raise ValueError("foldseek_query_coordinate_outside_run")
    if coordinate.parent != artifact_dir.resolve(strict=True) or retention != "single_representative_only":
        raise ValueError("foldseek_query_generated_coordinate_binding_invalid")
    return coordinate


def ensure_no_link_components(run_dir: Path, path: Path) -> None:
    lexical = path if path.is_absolute() else run_dir / path
    try:
        relative = lexical.relative_to(run_dir)
    except ValueError as exc:
        raise ValueError("foldseek_query_artifact_outside_run") from exc
    current = run_dir
    for part in relative.parts:
        current = current / part
        if is_link_like(current):
            raise ValueError("foldseek_query_artifact_link")
    try:
        lexical.resolve(strict=True).relative_to(run_dir.resolve(strict=True))
    except ValueError as exc:
        raise ValueError("foldseek_query_artifact_outside_run") from exc


def is_link_like(path: Path) -> bool:
    if path.is_symlink():
        return True
    is_junction = getattr(path, "is_junction", None)
    return bool(is_junction and is_junction())


def structure_suffix(path: Path) -> str:
    suffix = path.suffix.lower()
    if suffix == ".gz":
        suffix = Path(path.stem).suffix.lower()
    return suffix


def structure_query_id(input_id: str, record_id: str, query_digest: str) -> str:
    payload = json.dumps(
        [input_id, record_id, query_digest],
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return f"query_{hashlib.sha256(payload).hexdigest()}"


def structure_query_path(run_dir: Path, row: dict[str, str]) -> Path:
    artifact_value = str(row.get("artifact", "")).strip()
    source_value = str(row.get("source_path", "")).strip()
    if artifact_value:
        artifact = runtime_path(artifact_value)
        artifact = artifact if artifact.is_absolute() else run_dir / artifact
        artifact = artifact.resolve()
        try:
            artifact.relative_to(run_dir.resolve())
        except ValueError as exc:
            raise ValueError("foldseek_query_artifact_outside_run") from exc
        if not artifact.is_file():
            raise ValueError("foldseek_query_path_missing")
        if source_value:
            source = runtime_path(source_value)
            if not source.is_absolute():
                raise ValueError("foldseek_query_source_path_not_absolute")
            if source.is_file() and sha256_path(source) != sha256_path(artifact):
                raise ValueError("foldseek_query_path_content_mismatch")
        return artifact
    if not source_value:
        raise ValueError("foldseek_query_path_missing")
    source = runtime_path(source_value)
    if not source.is_absolute():
        raise ValueError("foldseek_query_source_path_not_absolute")
    if not source.is_file():
        raise ValueError("foldseek_query_path_missing")
    return source.resolve()


def valid_digest(value: str) -> bool:
    return len(value) == 64 and all(ch in "0123456789abcdef" for ch in value)


def sha256_path(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_status(path: Path, status: str, reason: str, row_count: int) -> None:
    write_text(
        path,
        "status\treason\trow_count\n"
        f"{clean_cell(status)}\t{clean_cell(reason)}\t{row_count}\n",
    )


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def decode_tsv_cell(value: str) -> str:
    out: list[str] = []
    index = 0
    while index < len(value):
        ch = value[index]
        if ch != "\\" or index + 1 >= len(value):
            out.append(ch)
            index += 1
            continue
        nxt = value[index + 1]
        out.append({"t": "\t", "n": "\n", "r": "\r", "\\": "\\"}.get(nxt, nxt))
        index += 2
    return "".join(out)


def alignment_score(
    pident: str,
    bits: str,
    query_coverage: str | None = None,
    target_coverage: str | None = None,
    alignment_length: str | None = None,
) -> float:
    try:
        identity = max(0.0, min(float(pident) / 100.0, 1.0))
    except ValueError:
        identity = 0.0
    try:
        bit_score = max(0.0, min(float(bits) / 200.0, 1.0))
    except ValueError:
        bit_score = 0.0
    if query_coverage is None or target_coverage is None or alignment_length is None:
        return max(0.01, min(identity * 0.7 + bit_score * 0.3, 1.0))
    coverage = math.sqrt(max(0.0, min(float(query_coverage), 1.0)) * max(0.0, min(float(target_coverage), 1.0)))
    length_confidence = max(0.0, min(float(alignment_length) / 50.0, 1.0))
    return max(
        0.01,
        min(
            identity * 0.55 + bit_score * 0.20 + coverage * 0.20 + length_confidence * 0.05,
            1.0,
        ),
    )


def clean_cell(value: str) -> str:
    return str(value or "").replace("\t", " ").replace("\r", " ").replace("\n", " ").strip()


def safe_note(value: str) -> str:
    return clean_cell(value)[:240]


def first_line(value: str) -> str:
    return next((line.strip() for line in value.splitlines() if line.strip()), "command_failed")


def safe_id(value: str) -> str:
    cleaned = "".join(ch if ch.isalnum() else "_" for ch in str(value or ""))
    return cleaned or "unknown"


if __name__ == "__main__":
    raise SystemExit(main())
