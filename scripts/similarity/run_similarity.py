import argparse
import csv
import shutil
import subprocess
from pathlib import Path


HIT_FIELDS = [
    "candidate_id",
    "backend",
    "database_id",
    "database_release",
    "target_accession",
    "target_name",
    "target_taxid",
    "target_reviewed",
    "evalue",
    "bitscore",
    "pident",
    "query_cover",
    "target_cover",
    "alignment_length",
    "query_start",
    "query_end",
    "target_start",
    "target_end",
    "hit_rank",
    "hit_class",
    "raw_output_file",
]

SUMMARY_FIELDS = [
    "candidate_id",
    "best_hit_accession",
    "best_hit_name",
    "best_hit_evalue",
    "best_hit_bitscore",
    "best_hit_pident",
    "best_hit_query_cover",
    "reviewed_hit_present",
    "known_similarity_flag",
    "novelty_flag",
    "short_sequence_alignment_warning",
    "similarity_backend",
    "similarity_database",
    "similarity_config_sha256",
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--query-fasta", required=True)
    parser.add_argument("--candidate-tsv", required=True)
    parser.add_argument("--db", required=True)
    parser.add_argument("--backend", required=True, choices=["diamond", "blastp"])
    parser.add_argument("--out-hits", required=True)
    parser.add_argument("--out-summary", required=True)
    parser.add_argument("--threads", default="12")
    args = parser.parse_args()
    Path(args.out_hits).parent.mkdir(parents=True, exist_ok=True)
    raw = Path(args.out_hits).with_suffix(".raw.tsv")
    if args.backend == "diamond":
        if not shutil.which("diamond"):
            raise SystemExit("diamond_not_found")
        subprocess.check_call(
            [
                "diamond",
                "blastp",
                "--query",
                args.query_fasta,
                "--db",
                args.db,
                "--out",
                str(raw),
                "--outfmt",
                "6",
                "qseqid",
                "sseqid",
                "pident",
                "length",
                "mismatch",
                "gapopen",
                "qstart",
                "qend",
                "sstart",
                "send",
                "evalue",
                "bitscore",
                "stitle",
                "--max-target-seqs",
                "10",
                "--threads",
                args.threads,
            ]
        )
    else:
        if not shutil.which("blastp"):
            raise SystemExit("blastp_not_found")
        subprocess.check_call(
            [
                "blastp",
                "-query",
                args.query_fasta,
                "-db",
                args.db,
                "-outfmt",
                "6 qseqid sseqid pident length mismatch gapopen qstart qend sstart send evalue bitscore stitle",
                "-max_target_seqs",
                "10",
                "-num_threads",
                args.threads,
                "-out",
                str(raw),
            ]
        )
    candidate_rows = read_tsv(args.candidate_tsv)
    lengths = {row["candidate_id"]: int(row["aa_length"]) for row in candidate_rows}
    parsed = parse_raw(raw, args.backend, lengths)
    write_tsv(args.out_hits, parsed, HIT_FIELDS)
    write_tsv(args.out_summary, summarize(candidate_rows, parsed, args.backend), SUMMARY_FIELDS)
    return 0


def parse_raw(raw: Path, backend: str, lengths: dict[str, int]) -> list[dict]:
    rows = []
    ranks: dict[str, int] = {}
    if not raw.exists():
        return rows
    with open(raw, "r", encoding="utf-8", newline="") as handle:
        for line in handle:
            cells = line.rstrip("\n").split("\t")
            if len(cells) < 12:
                continue
            qseqid, sseqid, pident, length, _mismatch, _gapopen, qstart, qend, sstart, send, evalue, bitscore = cells[:12]
            stitle = cells[12] if len(cells) > 12 else sseqid
            candidate_id = qseqid.split("|", 1)[0]
            ranks[candidate_id] = ranks.get(candidate_id, 0) + 1
            qlen = max(1, lengths.get(candidate_id, int(length)))
            query_cover = int(length) / qlen
            rows.append(
                {
                    "candidate_id": candidate_id,
                    "backend": backend,
                    "database_id": "uniprot_swissprot_reviewed",
                    "database_release": "current_release",
                    "target_accession": sseqid,
                    "target_name": stitle,
                    "target_taxid": "",
                    "target_reviewed": "true" if sseqid.startswith("sp|") else "unknown",
                    "evalue": evalue,
                    "bitscore": bitscore,
                    "pident": pident,
                    "query_cover": f"{query_cover:.6f}",
                    "target_cover": "",
                    "alignment_length": length,
                    "query_start": qstart,
                    "query_end": qend,
                    "target_start": sstart,
                    "target_end": send,
                    "hit_rank": str(ranks[candidate_id]),
                    "hit_class": "reviewed_known_similarity" if sseqid.startswith("sp|") else "known_similarity",
                    "raw_output_file": str(raw),
                }
            )
    return rows


def summarize(candidates: list[dict], hits: list[dict], backend: str) -> list[dict]:
    hits_by_candidate: dict[str, list[dict]] = {}
    for hit in hits:
        hits_by_candidate.setdefault(hit["candidate_id"], []).append(hit)
    rows = []
    for candidate in candidates:
        candidate_id = candidate["candidate_id"]
        length = int(candidate["aa_length"])
        candidate_hits = sorted(
            hits_by_candidate.get(candidate_id, []),
            key=lambda row: (float(row["evalue"]), -float(row["bitscore"])),
        )
        best = candidate_hits[0] if candidate_hits else {}
        reviewed = any(hit.get("target_reviewed") == "true" for hit in candidate_hits)
        known = bool(candidate_hits)
        rows.append(
            {
                "candidate_id": candidate_id,
                "best_hit_accession": best.get("target_accession", ""),
                "best_hit_name": best.get("target_name", ""),
                "best_hit_evalue": best.get("evalue", ""),
                "best_hit_bitscore": best.get("bitscore", ""),
                "best_hit_pident": best.get("pident", ""),
                "best_hit_query_cover": best.get("query_cover", ""),
                "reviewed_hit_present": str(reviewed).lower(),
                "known_similarity_flag": str(known).lower(),
                "novelty_flag": str(not known).lower(),
                "short_sequence_alignment_warning": str(length < 30).lower(),
                "similarity_backend": backend,
                "similarity_database": "uniprot_swissprot_reviewed",
                "similarity_config_sha256": "",
            }
        )
    return rows


def read_tsv(path: str) -> list[dict]:
    with open(path, "r", encoding="utf-8-sig", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def write_tsv(path: str, rows: list[dict], fields: list[str]) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


if __name__ == "__main__":
    raise SystemExit(main())
