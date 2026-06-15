import argparse
import gzip
import hashlib
import json
import shutil
import subprocess
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


SWISSPROT_URL = "https://ftp.uniprot.org/pub/databases/uniprot/current_release/knowledgebase/complete/uniprot_sprot.fasta.gz"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db-dir", required=True)
    parser.add_argument("--backend", default="auto", choices=["auto", "diamond", "blastp"])
    parser.add_argument("--manifest", required=True)
    args = parser.parse_args()
    db_dir = Path(args.db_dir)
    db_dir.mkdir(parents=True, exist_ok=True)
    fasta_gz = db_dir / "uniprot_sprot.fasta.gz"
    fasta = db_dir / "uniprot_sprot.fasta"
    if not fasta.exists():
        if not fasta_gz.exists():
            urllib.request.urlretrieve(SWISSPROT_URL, fasta_gz)
        with gzip.open(fasta_gz, "rb") as source, open(fasta, "wb") as target:
            shutil.copyfileobj(source, target)
    backend = choose_backend(args.backend)
    if backend == "diamond":
        db_prefix = db_dir / "uniprot_sprot"
        run(["diamond", "makedb", "--in", str(fasta), "-d", str(db_prefix)])
        db_path = str(db_prefix) + ".dmnd"
    else:
        db_prefix = db_dir / "uniprot_sprot_blast"
        run(["makeblastdb", "-in", str(fasta), "-dbtype", "prot", "-parse_seqids", "-out", str(db_prefix)])
        db_path = str(db_prefix)
    manifest_row = {
        "resource_id": "uniprot_swissprot_reviewed",
        "resource_type": "sequence_database",
        "source_name": "UniProtKB/Swiss-Prot reviewed protein FASTA",
        "source_url": SWISSPROT_URL,
        "download_command": f"urllib.request.urlretrieve({SWISSPROT_URL})",
        "local_path": str(fasta),
        "version_or_release": "current_release",
        "sha256": sha256_file(fasta),
        "license_note": "UniProt data; release/license details recorded from source URL during science-store preparation.",
        "used_in_pipeline": "current_real_scientific_pipeline",
        "created_at_utc": now(),
    }
    append_manifest(args.manifest, [manifest_row])
    print(json.dumps({"backend": backend, "fasta": str(fasta), "db_path": db_path, "sha256": manifest_row["sha256"]}))
    return 0


def choose_backend(requested: str) -> str:
    if requested in {"diamond", "blastp"}:
        return requested
    if shutil.which("diamond"):
        return "diamond"
    if shutil.which("blastp") and shutil.which("makeblastdb"):
        return "blastp"
    raise SystemExit("no_real_similarity_backend_available")


def run(cmd: list[str]) -> None:
    subprocess.check_call(cmd)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def append_manifest(path: str, rows: list[dict]) -> None:
    fields = [
        "resource_id",
        "resource_type",
        "source_name",
        "source_url",
        "download_command",
        "local_path",
        "version_or_release",
        "sha256",
        "license_note",
        "used_in_pipeline",
        "created_at_utc",
    ]
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    exists = target.exists()
    with open(target, "a", encoding="utf-8", newline="") as handle:
        if not exists:
            handle.write("\t".join(fields) + "\n")
        for row in rows:
            handle.write("\t".join(str(row.get(field, "")).replace("\t", " ") for field in fields) + "\n")


def now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


if __name__ == "__main__":
    raise SystemExit(main())
