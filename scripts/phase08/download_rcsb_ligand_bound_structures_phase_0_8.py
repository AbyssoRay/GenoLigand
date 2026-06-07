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


def download_one(pdb_id: str, structures_dir: Path, timeout: int, overwrite: bool) -> dict[str, object]:
    pdb_id = pdb_id.upper()
    url = RCSB_CIF_URL.format(pdb_id=pdb_id)
    target = structures_dir / f"{pdb_id}.cif"
    if target.exists() and target.stat().st_size > 0 and not overwrite:
        return {
            "pdb_id": pdb_id,
            "url": url,
            "local_path": target.relative_to(ROOT).as_posix(),
            "bytes": target.stat().st_size,
            "sha256": sha256_file(target),
            "status": "PASS",
            "notes": "existing_structure_file_reused",
        }
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
    parser.add_argument("--limit", type=int, default=0, help="0 means all manifest rows.")
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--overwrite", action="store_true")
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
        if args.limit and len(pdb_ids) >= args.limit:
            break
    args.structures_dir.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, object]] = []
    started = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    with ThreadPoolExecutor(max_workers=max(1, args.workers)) as executor:
        futures = [executor.submit(download_one, pdb_id, args.structures_dir, args.timeout, args.overwrite) for pdb_id in pdb_ids]
        for future in as_completed(futures):
            row = future.result()
            row["generated_at_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            row["scope_manifest_row_count"] = len(manifest_rows)
            row["requested_pdb_count"] = len(pdb_ids)
            row["run_started_at_utc"] = started
            rows.append(row)
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
        "run_started_at_utc",
        "generated_at_utc",
    ]
    write_tsv(args.audit, rows, fields)
    return 0 if rows and all(row.get("status") == "PASS" for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
