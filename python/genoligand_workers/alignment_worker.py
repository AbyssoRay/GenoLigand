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

    targets = discover_sequence_targets(science_store)
    if not targets:
        write_status(status_path, "not_ready", "no FASTA target files in science store", 0)
        return "mmseqs2=not_ready;reason=no_fasta_targets"

    mmseqs = shutil.which("mmseqs")
    if not mmseqs:
        write_status(status_path, "not_ready", "mmseqs executable not found in worker PATH", 0)
        return "mmseqs2=not_ready;reason=missing_mmseqs"

    query = out_dir / "mmseqs_query.fasta"
    write_fasta(query, sequences)
    raw_dir = out_dir / "mmseqs_raw"
    raw_dir.mkdir(parents=True, exist_ok=True)
    tmp_root = out_dir / "mmseqs_tmp"
    tmp_root.mkdir(parents=True, exist_ok=True)
    timeout = int(os.environ.get("GENOLIGAND_ALIGNMENT_TIMEOUT_SECONDS", "900"))
    rows: list[str] = []
    failures: list[str] = []
    for index, target in enumerate(targets, start=1):
        raw = raw_dir / f"target_{index}.m8"
        tmp = tmp_root / f"target_{index}"
        command = [
            mmseqs,
            "easy-search",
            str(query),
            str(target),
            str(raw),
            str(tmp),
            "--format-output",
            "query,target,pident,alnlen,evalue,bits",
            "--threads",
            os.environ.get("GENOLIGAND_ALIGNMENT_THREADS", "1"),
        ]
        completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        if completed.returncode != 0:
            failures.append(f"{target.name}:{first_line(completed.stderr or completed.stdout)}")
            continue
        rows.extend(mmseqs_rows(raw, target))
    write_text(matches_path, MATCH_HEADER + "".join(rows))
    if rows:
        write_status(status_path, "ready", f"mmseqs2_easy_search completed against {len(targets)} FASTA target files", len(rows))
    elif failures:
        write_status(status_path, "not_ready", ";".join(failures[:4]), 0)
    else:
        write_status(status_path, "ready", f"mmseqs2_easy_search completed against {len(targets)} FASTA target files with no hits", 0)
    return f"mmseqs2_easy_search;targets={len(targets)};matches={len(rows)}"


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

    target = science_store / "downloads" / "rcsb" / "mmcif"
    if not target.exists():
        write_status(status_path, "not_ready", "local RCSB mmCIF target directory is missing", 0)
        return "foldseek=not_ready;reason=no_structure_targets"

    foldseek = shutil.which("foldseek")
    if not foldseek:
        write_status(status_path, "not_ready", "foldseek executable not found in worker PATH", 0)
        return "foldseek=not_ready;reason=missing_foldseek"

    raw = out_dir / "foldseek_structure_matches.m8"
    tmp = out_dir / "foldseek_tmp"
    timeout = int(os.environ.get("GENOLIGAND_ALIGNMENT_TIMEOUT_SECONDS", "900"))
    command = [
        foldseek,
        "easy-search",
        str(query),
        str(target),
        str(raw),
        str(tmp),
        "--format-output",
        "query,target,pident,alntmscore,evalue,bits",
        "--threads",
        os.environ.get("GENOLIGAND_ALIGNMENT_THREADS", "1"),
    ]
    completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if completed.returncode != 0:
        reason = first_line(completed.stderr or completed.stdout)
        write_status(status_path, "not_ready", reason, 0)
        return f"foldseek=not_ready;reason={safe_note(reason)}"
    rows = foldseek_rows(raw)
    write_text(matches_path, MATCH_HEADER + "".join(rows))
    if rows:
        write_status(status_path, "ready", "foldseek_easy_search completed against local RCSB mmCIF targets", len(rows))
    else:
        write_status(status_path, "ready", "foldseek_easy_search completed with no hits", 0)
    return f"foldseek_easy_search;matches={len(rows)}"


def runtime_path(value: str) -> Path:
    path = Path(value)
    if path.exists():
        return path
    if len(value) >= 3 and value[1] == ":" and value[2] in "\\/":
        drive = value[0].lower()
        rest = value[3:].replace("\\", "/")
        return Path(f"/mnt/{drive}/{rest}")
    return path


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


def discover_sequence_targets(science_store: Path) -> list[Path]:
    roots = [
        science_store / "downloads" / "uniprot",
        science_store / "downloads" / "metagenome",
        science_store / "downloads" / "biolip2",
    ]
    suffixes = (".fa", ".faa", ".fasta", ".fa.gz", ".faa.gz", ".fasta.gz")
    targets: list[Path] = []
    for root in roots:
        if not root.exists():
            continue
        for path in root.rglob("*"):
            lower = path.name.lower()
            if path.is_file() and lower.endswith(suffixes) and not lower.endswith(".part"):
                targets.append(path)
    return sorted(targets)


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
                f"mmseqs2_easy_search;evalue={safe_note(evalue)};bits={safe_note(bits)};"
                f"pident={safe_note(pident)};alnlen={safe_note(alnlen)};"
                f"target_source={safe_note(target_path.name)}"
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
                f"foldseek_easy_search;evalue={safe_note(evalue)};bits={safe_note(bits)};"
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
