from __future__ import annotations

import argparse
import csv
import hashlib
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SOURCE_DIR = ROOT / "data/external_phase_0_8/rcsb_ligand_bound"
DEFAULT_MANIFEST = DEFAULT_SOURCE_DIR / "rcsb_ligand_bound_entry_manifest.tsv"
DEFAULT_STRUCTURES_DIR = DEFAULT_SOURCE_DIR / "structures"
DEFAULT_AUDIT = ROOT / "outputs/phase_0_8/download_rcsb_ligand_bound_structures.tsv"
RCSB_CIF_URL = "https://files.rcsb.org/download/{pdb_id}.cif"
RCSB_CIF_GZ_URL = "https://files.rcsb.org/download/{pdb_id}.cif.gz"


def read_tsv(path: Path) -> list[dict[str, str]]:
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def write_tsv(path: Path, rows: list[dict[str, object]], fields: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def existing_structure_path(pdb_id: str, structures_dir: Path) -> Path | None:
    for suffix in [".cif.gz", ".cif"]:
        path = structures_dir / f"{pdb_id}{suffix}"
        if path.exists() and path.stat().st_size > 0:
            return path
    return None


def download_one(pdb_id: str, structures_dir: Path, timeout: int, overwrite: bool, compressed: bool) -> dict[str, object]:
    pdb_id = pdb_id.upper()
    existing = existing_structure_path(pdb_id, structures_dir)
    if existing is not None and not overwrite:
        url = RCSB_CIF_GZ_URL.format(pdb_id=pdb_id) if existing.name.endswith(".gz") else RCSB_CIF_URL.format(pdb_id=pdb_id)
        return {
            "pdb_id": pdb_id,
            "url": url,
            "local_path": existing.relative_to(ROOT).as_posix(),
            "bytes": existing.stat().st_size,
            "sha256": sha256_file(existing),
            "status": "PASS",
            "notes": "existing_structure_file_reused",
        }
    url = RCSB_CIF_GZ_URL.format(pdb_id=pdb_id) if compressed else RCSB_CIF_URL.format(pdb_id=pdb_id)
    target = structures_dir / f"{pdb_id}.cif.gz" if compressed else structures_dir / f"{pdb_id}.cif"
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "GenoLigand-Phase08/0.8"})
        with urllib.request.urlopen(request, timeout=timeout) as response:
            payload = response.read()
        target.write_bytes(payload)
        ok = target.exists() and target.stat().st_size > 0
        return {
            "pdb_id": pdb_id,
            "url": url,
            "local_path": target.relative_to(ROOT).as_posix(),
            "bytes": target.stat().st_size if ok else 0,
            "sha256": sha256_file(target) if ok else "",
            "status": "PASS" if ok else "FAIL",
            "notes": "downloaded_from_rcsb_file_service" if ok else "zero_byte_download",
        }
    except Exception as exc:  # noqa: BLE001
        return {
            "pdb_id": pdb_id,
            "url": url,
            "local_path": target.relative_to(ROOT).as_posix(),
            "bytes": 0,
            "sha256": "",
            "status": "FAIL",
            "notes": repr(exc),
        }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--structures-dir", type=Path, default=DEFAULT_STRUCTURES_DIR)
    parser.add_argument("--audit", type=Path, default=DEFAULT_AUDIT)
    parser.add_argument("--start", type=int, default=0, help="0-based start offset in the manifest-derived unique PDB ID list.")
    parser.add_argument("--limit", type=int, default=0, help="0 means all manifest rows.")
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument("--uncompressed", action="store_true", help="Download .cif instead of the default .cif.gz.")
    args = parser.parse_args()

    manifest_rows = read_tsv(args.manifest)
    pdb_ids = []
    seen = set()
    for row in manifest_rows:
        pdb_id = row.get("pdb_id", "").strip().upper()
        if not pdb_id or pdb_id in seen:
            continue
        seen.add(pdb_id)
        pdb_ids.append(pdb_id)
    selected_pdb_ids = pdb_ids[max(0, args.start) :]
    if args.limit:
        selected_pdb_ids = selected_pdb_ids[: args.limit]
    args.structures_dir.mkdir(parents=True, exist_ok=True)
    existing_audit_rows = {row.get("pdb_id", "").upper(): row for row in read_tsv(args.audit) if row.get("pdb_id")}
    rows: list[dict[str, object]] = []
    started = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    with ThreadPoolExecutor(max_workers=max(1, args.workers)) as executor:
        futures = [executor.submit(download_one, pdb_id, args.structures_dir, args.timeout, args.overwrite, not args.uncompressed) for pdb_id in selected_pdb_ids]
        for future in as_completed(futures):
            row = future.result()
            row["generated_at_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            row["scope_manifest_row_count"] = len(manifest_rows)
            row["requested_pdb_count"] = len(selected_pdb_ids)
            row["chunk_start"] = max(0, args.start)
            row["chunk_limit"] = args.limit
            row["run_started_at_utc"] = started
            rows.append(row)
    for row in rows:
        existing_audit_rows[str(row.get("pdb_id", "")).upper()] = {key: str(value) for key, value in row.items()}
    rows = list(existing_audit_rows.values())
    rows.sort(key=lambda row: str(row.get("pdb_id", "")))
    fields = [
        "pdb_id",
        "url",
        "local_path",
        "bytes",
        "sha256",
        "status",
        "notes",
        "scope_manifest_row_count",
        "requested_pdb_count",
        "chunk_start",
        "chunk_limit",
        "run_started_at_utc",
        "generated_at_utc",
    ]
    write_tsv(args.audit, rows, fields)
    requested = {pdb_id.upper() for pdb_id in selected_pdb_ids}
    requested_rows = [row for row in rows if str(row.get("pdb_id", "")).upper() in requested]
    return 0 if requested_rows and all(row.get("status") == "PASS" for row in requested_rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
