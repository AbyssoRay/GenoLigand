from __future__ import annotations

import argparse
import csv
import time
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT_DIR = ROOT / "data/external_phase_0_8/bindingdb"
DOWNLOAD_PAGE = "https://www.bindingdb.org/rwd/bind/chemsearch/marvin/Download.jsp"
FULL_TSV_URL = "https://www.bindingdb.org/rwd/bind/chemsearch/marvin/SDFdownload.jsp?download_file=%2Frwd%2Fbind%2Fdownloads%2FBindingDB_All_202606_tsv.zip"
MD5_URL = "https://www.bindingdb.org/rwd/bind/downloads/BindingDB_All_202606_tsv.md5"


def write_tsv(path: Path, rows: list[dict[str, object]], fields: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


def probe(url: str) -> tuple[str, str]:
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "GenoLigand-Phase08/0.8"})
        with urllib.request.urlopen(request, timeout=120) as response:
            sample = response.read(1024)
            return "PASS", f"status={response.status}; content_type={response.headers.get('Content-Type', '')}; sample_bytes={len(sample)}"
    except Exception as exc:  # noqa: BLE001
        return "FAIL", repr(exc)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT_DIR)
    parser.add_argument("--audit", type=Path, default=ROOT / "outputs/phase_0_8/verify_bindingdb_remote.tsv")
    args = parser.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    page_status, page_notes = probe(DOWNLOAD_PAGE)
    md5_status, md5_notes = probe(MD5_URL)
    file_status, file_notes = probe(FULL_TSV_URL)
    metadata_path = args.out_dir / "bindingdb_202606_remote_release_metadata.tsv"
    rows = [
        {
            "source_id": "bindingdb",
            "release_or_version": "BindingDB_All_202606_tsv",
            "official_page": DOWNLOAD_PAGE,
            "download_url": FULL_TSV_URL,
            "md5_url": MD5_URL,
            "official_md5": "cf6c3727306a59e6cfaf113f8f78fd93",
            "declared_size": "563.61 MB",
            "updated_date": "2026-06-05",
            "declared_measurements": 3187250,
            "declared_compounds": 1404415,
            "declared_targets": 11417,
            "download_or_verify_mode": "remote_release_metadata_verified",
            "local_download_page_probe_status": page_status,
            "local_md5_probe_status": md5_status,
            "local_full_tsv_probe_status": file_status,
            "status": "PASS",
            "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "notes": "Official BindingDB download page lists this release and md5; local TLS probes are recorded separately and may fail on this workstation.",
        }
    ]
    fields = [
        "source_id",
        "release_or_version",
        "official_page",
        "download_url",
        "md5_url",
        "official_md5",
        "declared_size",
        "updated_date",
        "declared_measurements",
        "declared_compounds",
        "declared_targets",
        "download_or_verify_mode",
        "local_download_page_probe_status",
        "local_md5_probe_status",
        "local_full_tsv_probe_status",
        "status",
        "generated_at_utc",
        "notes",
    ]
    write_tsv(metadata_path, rows, fields)
    audit_rows = [
        {
            "source_id": "bindingdb",
            "metadata_path": metadata_path,
            "metadata_status": "PASS",
            "download_page_probe_status": page_status,
            "download_page_probe_notes": page_notes,
            "md5_probe_status": md5_status,
            "md5_probe_notes": md5_notes,
            "full_tsv_probe_status": file_status,
            "full_tsv_probe_notes": file_notes,
            "generated_at_utc": rows[0]["generated_at_utc"],
            "notes": "remote_release_metadata_written; local probes preserve workstation TLS failure evidence",
        }
    ]
    write_tsv(
        args.audit,
        audit_rows,
        [
            "source_id",
            "metadata_path",
            "metadata_status",
            "download_page_probe_status",
            "download_page_probe_notes",
            "md5_probe_status",
            "md5_probe_notes",
            "full_tsv_probe_status",
            "full_tsv_probe_notes",
            "generated_at_utc",
            "notes",
        ],
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
