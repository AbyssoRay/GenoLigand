from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase06.common import ensure_parent, sha256_file, write_tsv
from genoligand_phase08.full_database_strict import SOURCE_SPECS, utc_now


def http_download(url: str, out: Path) -> int:
    ensure_parent(out)
    request = urllib.request.Request(url, headers={"User-Agent": "GenoLigand-Phase08/0.8"})
    with urllib.request.urlopen(request, timeout=120) as response, out.open("wb") as handle:
        shutil.copyfileobj(response, handle)
    return 0


def download_source(source_id: str, allow_large: bool) -> dict[str, object]:
    specs = {str(spec["resource_id"]): spec for spec in SOURCE_SPECS}
    spec = specs[source_id]
    url = str(spec["source_url"])
    out_dir = ROOT / "data/external_phase_0_8" / source_id
    out_dir.mkdir(parents=True, exist_ok=True)
    status = "FAIL"
    notes = "unsupported_source_download_requires_manual_or_specialized_client"
    local_path = ""
    exit_code = 1
    if source_id == "plinder":
        if not allow_large:
            notes = "plinder_requires_gsutil_and_allow_large"
        else:
            command = ["gsutil", "-m", "cp", "-r", "gs://plinder/2024-06/v2/*", str(out_dir / "2024-06/v2")]
            completed = subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=False)
            exit_code = completed.returncode
            local_path = str(out_dir)
            status = "PASS" if exit_code == 0 else "FAIL"
            notes = "gsutil_plinder_download_attempted"
    elif source_id == "rcsb_ligand_bound":
        notes = "rcsb_ligand_bound_requires_query_scope_file_before_download"
    elif source_id == "reconstructed_openbindbench":
        notes = "use scripts/phase08/reconstruct_openbindbench_phase_0_8.py"
    elif url.startswith("http"):
        if not allow_large and source_id in {"chembl", "bindingdb", "afdb_human", "dude_z"}:
            notes = "large_source_requires_allow_large"
        else:
            filename = source_id + "_" + Path(url.split("?")[0]).name
            if filename.endswith("_"):
                filename += "download.dat"
            local_path = str(out_dir / filename)
            try:
                exit_code = http_download(url, Path(local_path))
                status = "PASS"
                notes = "http_download_complete"
            except Exception as exc:  # noqa: BLE001
                notes = f"http_download_failed:{exc.__class__.__name__}"
    else:
        notes = "manual_source_required"
    digest = sha256_file(Path(local_path)) if local_path and Path(local_path).exists() and Path(local_path).is_file() else ""
    return {
        "source_id": source_id,
        "source_url": url,
        "local_path": local_path,
        "sha256": digest,
        "exit_code": exit_code,
        "status": status,
        "generated_at_utc": utc_now(),
        "notes": notes,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True)
    parser.add_argument("--allow-large", action="store_true")
    parser.add_argument("--out", type=Path, default=ROOT / "outputs/phase_0_8/download_attempts.tsv")
    args = parser.parse_args()
    ids = [str(spec["resource_id"]) for spec in SOURCE_SPECS if args.source == "all" or args.source == spec["resource_id"]]
    if not ids:
        raise SystemExit(f"unknown_phase08_source:{args.source}")
    rows = [download_source(source_id, args.allow_large) for source_id in ids]
    write_tsv(args.out, rows, ["source_id", "source_url", "local_path", "sha256", "exit_code", "status", "generated_at_utc", "notes"])
    return 0 if all(row["status"] == "PASS" for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())

