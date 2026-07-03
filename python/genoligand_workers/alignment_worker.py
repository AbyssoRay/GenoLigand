from __future__ import annotations

import csv
import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import Any

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


MATCH_HEADER = (
    "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\t"
    "matched_features\tevidence\n"
)


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        version = run_alignment_job(job)
        status = "PASS"
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        version = f"alignment worker failed: {safe_note(str(exc))}"
        status = "FAIL"
        try:
            run_dir = runtime_path(str(job["run_dir"]))
            tool_outputs(run_dir).mkdir(parents=True, exist_ok=True)
            if str(job.get("worker", "")).startswith("foldseek"):
                write_status(tool_outputs(run_dir) / "foldseek_alignment_status.tsv", "not_ready", str(exc), 0)
            else:
                write_status(tool_outputs(run_dir) / "mmseqs_alignment_status.tsv", "not_ready", str(exc), 0)
        except Exception:
            pass
    write_result(args.out, job, status, version)
    return 0 if status == "PASS" else 2


def run_alignment_job(job: dict[str, Any]) -> str:
    worker = str(job.get("worker", ""))
    run_dir = runtime_path(str(job["run_dir"]))
    science_store = runtime_path(str(job["science_store"]))
    if worker == "foldseek_alignment":
        return run_foldseek_alignment(run_dir, science_store)
    return run_mmseqs_alignment(run_dir, science_store)


def run_mmseqs_alignment(run_dir: Path, science_store: Path) -> str:
    out_dir = tool_outputs(run_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    matches_path = out_dir / "mmseqs_alignment_matches.tsv"
    status_path = out_dir / "mmseqs_alignment_status.tsv"
    write_text(matches_path, MATCH_HEADER)

    sequences = read_sequence_inputs(run_dir)
    if not sequences:
        write_status(status_path, "not_ready", "no peptide or protein sequence input for MMseqs2", 0)
        return "mmseqs2=not_ready;reason=no_sequence_input"

    targets = mmseqs_target_databases(science_store)
    if not targets:
        write_status(status_path, "not_ready", "no persistent MMseqs2 target databases in science store indexes", 0)
        return "mmseqs2=not_ready;reason=no_persistent_mmseqs_databases"

    mmseqs = shutil.which("mmseqs")
    if not mmseqs:
        write_status(status_path, "not_ready", "mmseqs executable not found in worker PATH", 0)
        return "mmseqs2=not_ready;reason=missing_mmseqs"

    query = out_dir / "mmseqs_query.fasta"
    write_fasta(query, sequences)
    query_db = out_dir / "mmseqs_query_db"
    raw_dir = out_dir / "mmseqs_raw"
    raw_dir.mkdir(parents=True, exist_ok=True)
    tmp_root = out_dir / "mmseqs_tmp"
    tmp_root.mkdir(parents=True, exist_ok=True)
    timeout = int(os.environ.get("GENOLIGAND_ALIGNMENT_TIMEOUT_SECONDS", "900"))
    run_command([mmseqs, "createdb", str(query), str(query_db)], timeout, "mmseqs_createdb")
    rows: list[str] = []
    failures: list[str] = []
    for index, target in enumerate(targets, start=1):
        raw = raw_dir / f"target_{index}.m8"
        tmp = tmp_root / f"target_{index}"
        result_db = raw_dir / f"target_{index}_result"
        command = [
            mmseqs,
            "search",
            str(query_db),
            str(target),
            str(result_db),
            str(tmp),
            "--threads",
            os.environ.get("GENOLIGAND_ALIGNMENT_THREADS", "1"),
        ]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        if completed.returncode != 0:
            failures.append(f"{target.name}:{first_line(completed.stderr or completed.stdout)}")
            continue
        command = [
            mmseqs,
            "convertalis",
            str(query_db),
            str(target),
            str(result_db),
            str(raw),
            "--format-output",
            "query,target,pident,alnlen,evalue,bits",
        ]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        if completed.returncode != 0:
            failures.append(f"{target.name}:{first_line(completed.stderr or completed.stdout)}")
            continue
        rows.extend(mmseqs_rows(raw, target))
    write_text(matches_path, MATCH_HEADER + "".join(rows))
    if rows:
        write_status(status_path, "ready", f"mmseqs2 persistent database search completed against {len(targets)} target databases", len(rows))
    elif failures:
        write_status(status_path, "not_ready", ";".join(failures[:4]), 0)
    else:
        write_status(status_path, "ready", f"mmseqs2 persistent database search completed against {len(targets)} target databases with no hits", 0)
    return f"mmseqs2_search;targets={len(targets)};matches={len(rows)}"


def run_foldseek_alignment(run_dir: Path, science_store: Path) -> str:
    out_dir = tool_outputs(run_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    matches_path = out_dir / "foldseek_structure_matches.tsv"
    status_path = out_dir / "foldseek_alignment_status.tsv"
    write_text(matches_path, MATCH_HEADER)

    query = structure_query(run_dir)
    if query is None:
        write_status(status_path, "not_ready", "no structure viewer coordinate available for Foldseek", 0)
        return "foldseek=not_ready;reason=no_structure_query"

    target = foldseek_target_database(science_store)
    if target is None:
        write_status(status_path, "not_ready", "no persistent Foldseek target database in science store indexes", 0)
        return "foldseek=not_ready;reason=no_persistent_foldseek_database"

    foldseek = shutil.which("foldseek")
    if not foldseek:
        write_status(status_path, "not_ready", "foldseek executable not found in worker PATH", 0)
        return "foldseek=not_ready;reason=missing_foldseek"

    raw = out_dir / "foldseek_structure_matches.m8"
    tmp = out_dir / "foldseek_tmp"
    query_db = out_dir / "foldseek_query_db"
    result_db = out_dir / "foldseek_result_db"
    timeout = int(os.environ.get("GENOLIGAND_ALIGNMENT_TIMEOUT_SECONDS", "900"))
    run_command([foldseek, "createdb", str(query), str(query_db)], timeout, "foldseek_createdb")
    command = [
        foldseek,
        "search",
        str(query_db),
        str(target),
        str(result_db),
        str(tmp),
        "-a",
        "--threads",
        os.environ.get("GENOLIGAND_ALIGNMENT_THREADS", "1"),
    ]
    completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if completed.returncode != 0:
        reason = first_line(completed.stderr or completed.stdout)
        write_status(status_path, "not_ready", reason, 0)
        return f"foldseek=not_ready;reason={safe_note(reason)}"
    command = [
        foldseek,
        "convertalis",
        str(query_db),
        str(target),
        str(result_db),
        str(raw),
        "--format-output",
        "query,target,pident,alntmscore,evalue,bits",
    ]
    completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if completed.returncode != 0:
        reason = first_line(completed.stderr or completed.stdout)
        write_status(status_path, "not_ready", reason, 0)
        return f"foldseek=not_ready;reason={safe_note(reason)}"
    rows = foldseek_rows(raw)
    write_text(matches_path, MATCH_HEADER + "".join(rows))
    if rows:
        write_status(status_path, "ready", "foldseek persistent database search completed against local RCSB targets", len(rows))
    else:
        write_status(status_path, "ready", "foldseek persistent database search completed with no hits", 0)
    return f"foldseek_search;matches={len(rows)}"


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


def tool_outputs(run_dir: Path) -> Path:
    return run_dir / "tool_outputs"


def read_sequence_inputs(run_dir: Path) -> list[tuple[str, str]]:
    path = run_dir / "input_conversion_manifest.tsv"
    if not path.exists():
        return []
    rows: list[tuple[str, str]] = []
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            if str(row.get("conversion_status", "")).strip() != "converted":
                continue
            kind = str(row.get("canonical_kind", "")).strip()
            if kind not in {"peptide", "protein_ligand", "genome_or_metagenome"}:
                continue
            value = decode_tsv_cell(str(row.get("canonical_value", ""))).strip().upper()
            sequence = "".join(ch for ch in value if ch.isalpha())
            if len(sequence) < 2:
                continue
            rows.append((str(row.get("input_id", "input_1")).strip() or "input_1", sequence))
    return rows


def mmseqs_target_databases(science_store: Path) -> list[Path]:
    manifest = science_store / "indexes" / "mmseqs" / "mmseqs_target_databases.tsv"
    targets: list[Path] = []
    if manifest.exists():
        with manifest.open("r", encoding="utf-8-sig", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                if row.get("status") != "ready":
                    continue
                db_path = str(row.get("db_path", "")).strip()
                if not db_path:
                    continue
                path = Path(db_path)
                if not path.is_absolute():
                    path = science_store / path
                if db_ready(path):
                    targets.append(path)
    if targets:
        return sorted(dedupe_paths(targets))
    return sorted(dedupe_paths(dbtype_prefixes(science_store / "indexes" / "mmseqs")))


def foldseek_target_database(science_store: Path) -> Path | None:
    manifest = science_store / "indexes" / "foldseek" / "foldseek_target_databases.tsv"
    if manifest.exists():
        with manifest.open("r", encoding="utf-8-sig", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                if row.get("status") != "ready":
                    continue
                db_path = str(row.get("db_path", "")).strip()
                if not db_path:
                    continue
                path = Path(db_path)
                if not path.is_absolute():
                    path = science_store / path
                if db_ready(path):
                    return path
    prefixes = dbtype_prefixes(science_store / "indexes" / "foldseek")
    return prefixes[0] if prefixes else None


def dbtype_prefixes(root: Path) -> list[Path]:
    if not root.exists():
        return []
    prefixes: list[Path] = []
    for dbtype in root.rglob("*.dbtype"):
        if dbtype.name.endswith("_h.dbtype") or dbtype.name.endswith("_seq.dbtype"):
            continue
        prefixes.append(dbtype.with_suffix(""))
    return dedupe_paths(prefixes)


def db_ready(prefix: Path) -> bool:
    return prefix.with_suffix(".dbtype").exists() or Path(str(prefix) + ".dbtype").exists()


def dedupe_paths(paths: list[Path]) -> list[Path]:
    seen: set[str] = set()
    out: list[Path] = []
    for path in paths:
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        out.append(path)
    return out


def write_fasta(path: Path, rows: list[tuple[str, str]]) -> None:
    text = "".join(f">{safe_id(input_id)}\n{sequence}\n" for input_id, sequence in rows)
    write_text(path, text)


def mmseqs_rows(path: Path, target_path: Path) -> list[str]:
    if not path.exists():
        return []
    rows: list[str] = []
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            cols = line.rstrip("\n").split("\t")
            if len(cols) < 6:
                continue
            query, target, pident, alnlen, evalue, bits = cols[:6]
            score = alignment_score(pident, bits)
            target_id = safe_id(target.split()[0])
            evidence = (
                f"mmseqs2_search;evalue={safe_note(evalue)};bits={safe_note(bits)};"
                f"pident={safe_note(pident)};alnlen={safe_note(alnlen)};"
                f"target_database={safe_note(target_path.name)}"
            )
            rows.append(
                f"{safe_id(query)}\tMMseqs2\t{target_id}\t{clean_cell(target)}\tunknown\t"
                f"onsite_sequence_alignment\t{score:.4f}\t{clean_cell(target)}\t{evidence}\n"
            )
    return rows


def foldseek_rows(path: Path) -> list[str]:
    if not path.exists():
        return []
    rows: list[str] = []
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            cols = line.rstrip("\n").split("\t")
            if len(cols) < 6:
                continue
            query, target, pident, tm_score, evalue, bits = cols[:6]
            score = alignment_score(pident, bits)
            target_id = safe_id(Path(target).stem or target.split()[0])
            evidence = (
                f"foldseek_search;evalue={safe_note(evalue)};bits={safe_note(bits)};"
                f"pident={safe_note(pident)};alntmscore={safe_note(tm_score)}"
            )
            rows.append(
                f"{safe_id(query)}\tFoldseek\t{target_id}\t{clean_cell(target)}\tunknown\t"
                f"onsite_structure_alignment\t{score:.4f}\t{clean_cell(target)}\t{evidence}\n"
            )
    return rows


def structure_query(run_dir: Path) -> Path | None:
    for rel in [
        "structure_viewer.cif",
        "structure_viewer.mmcif",
        "structure_viewer.pdb",
        "tool_outputs/structure_viewer.cif",
        "tool_outputs/structure_viewer.mmcif",
        "tool_outputs/structure_viewer.pdb",
    ]:
        path = run_dir / rel
        if path.exists() and path.stat().st_size > 0:
            return path
    return None


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


def alignment_score(pident: str, bits: str) -> float:
    try:
        identity = max(0.0, min(float(pident) / 100.0, 1.0))
    except ValueError:
        identity = 0.0
    try:
        bit_score = max(0.0, min(float(bits) / 200.0, 1.0))
    except ValueError:
        bit_score = 0.0
    return max(0.01, min(identity * 0.7 + bit_score * 0.3, 1.0))


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
