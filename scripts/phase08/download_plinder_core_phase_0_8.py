from __future__ import annotations

import argparse
import csv
import json
import time
import urllib.parse
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT_DIR = ROOT / "data/external_phase_0_8/plinder/2024-06/v2"
PREFIXES = [
    "clusters",
    "dbs",
    "entries",
    "fingerprints",
    "index",
    "ligand_scores",
    "ligands",
    "linked_structures",
    "links",
    "mmp",
    "scores",
    "splits",
    "systems",
]
OBJECTS = {
    "bucket_index_listing": "https://storage.googleapis.com/storage/v1/b/plinder/o?prefix=2024-06/v2/index/",
    "annotation_table": "https://storage.googleapis.com/plinder/2024-06/v2/index/annotation_table.parquet",
    "split_table": "https://storage.googleapis.com/plinder/2024-06/v2/splits/split.parquet",
    "split_yaml": "https://storage.googleapis.com/plinder/2024-06/v2/splits/split.yaml",
}


def sha256_file(path: Path) -> str:
    import hashlib

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_tsv(path: Path, rows: list[dict[str, object]], fields: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


def download(url: str, path: Path) -> str:
    if path.exists() and path.stat().st_size > 0:
        return "existing_file_reused"
    request = urllib.request.Request(url, headers={"User-Agent": "GenoLigand-Phase08/0.8"})
    path.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(request, timeout=300) as response, path.open("wb") as handle:
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            handle.write(chunk)
    return "downloaded"


def fetch_json(url: str) -> dict[str, object]:
    request = urllib.request.Request(url, headers={"User-Agent": "GenoLigand-Phase08/0.8"})
    with urllib.request.urlopen(request, timeout=300) as response:
        return json.loads(response.read().decode("utf-8"))


def list_prefix(prefix: str) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    token = ""
    while True:
        url = "https://storage.googleapis.com/storage/v1/b/plinder/o?prefix=" + urllib.parse.quote(f"2024-06/v2/{prefix}/") + "&maxResults=1000"
        if token:
            url += "&pageToken=" + urllib.parse.quote(token)
        payload = fetch_json(url)
        for item in payload.get("items", []):
            if not isinstance(item, dict):
                continue
            rows.append(
                {
                    "prefix": prefix,
                    "object_name": item.get("name", ""),
                    "media_link": item.get("mediaLink", ""),
                    "generation": item.get("generation", ""),
                    "updated": item.get("updated", ""),
                    "size": item.get("size", "0"),
                    "md5_hash": item.get("md5Hash", ""),
                    "crc32c": item.get("crc32c", ""),
                    "etag": item.get("etag", ""),
                    "status": "PASS" if item.get("name") and item.get("size") is not None and item.get("md5Hash") else "FAIL",
                    "notes": "gcs_object_metadata_verified",
                }
            )
        token = str(payload.get("nextPageToken", ""))
        if not token:
            return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT_DIR)
    parser.add_argument("--audit", type=Path, default=ROOT / "outputs/phase_0_8/download_plinder_core.tsv")
    parser.add_argument("--remote-manifest", type=Path, default=ROOT / "data/external_phase_0_8/plinder/2024-06/v2/plinder_gcs_object_manifest.tsv")
    args = parser.parse_args()
    rows: list[dict[str, object]] = []
    for object_id, url in OBJECTS.items():
        if object_id == "bucket_index_listing":
            local_path = args.out_dir / "index/gcs_index_listing.json"
        elif object_id == "annotation_table":
            local_path = args.out_dir / "index/annotation_table.parquet"
        elif object_id == "split_table":
            local_path = args.out_dir / "splits/split.parquet"
        else:
            local_path = args.out_dir / "splits/split.yaml"
        try:
            notes = download(url, local_path)
            if object_id == "bucket_index_listing":
                json.loads(local_path.read_text(encoding="utf-8"))
            status = "PASS" if local_path.exists() and local_path.stat().st_size > 0 else "FAIL"
        except Exception as exc:  # noqa: BLE001
            notes = repr(exc)
            status = "FAIL"
        rows.append(
            {
                "source_id": "plinder",
                "object_id": object_id,
                "source_url": url,
                "local_path": local_path,
                "bytes": local_path.stat().st_size if local_path.exists() else 0,
                "sha256": sha256_file(local_path) if local_path.exists() and local_path.is_file() else "",
                "status": status,
                "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "notes": notes,
            }
        )
    manifest_rows: list[dict[str, object]] = []
    for prefix in PREFIXES:
        manifest_rows.extend(list_prefix(prefix))
    write_tsv(args.remote_manifest, manifest_rows, ["prefix", "object_name", "media_link", "generation", "updated", "size", "md5_hash", "crc32c", "etag", "status", "notes"])
    rows.append(
        {
            "source_id": "plinder",
            "object_id": "full_gcs_object_manifest",
            "source_url": "gs://plinder/2024-06/v2/",
            "local_path": args.remote_manifest,
            "bytes": sum(int(row.get("size", "0")) for row in manifest_rows),
            "sha256": sha256_file(args.remote_manifest) if args.remote_manifest.exists() else "",
            "status": "PASS" if manifest_rows and all(row["status"] == "PASS" for row in manifest_rows) else "FAIL",
            "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "notes": f"remote_prefixes={len(PREFIXES)}; remote_object_count={len(manifest_rows)}; remote_total_bytes={sum(int(row.get('size', '0')) for row in manifest_rows)}",
        }
    )
    write_tsv(args.audit, rows, ["source_id", "object_id", "source_url", "local_path", "bytes", "sha256", "status", "generated_at_utc", "notes"])
    return 0 if all(row["status"] == "PASS" for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
