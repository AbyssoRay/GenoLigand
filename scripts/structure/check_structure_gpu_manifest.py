import argparse
from pathlib import Path

from genoligand_gpu.io import read_json, read_tsv, sha256_file


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--backend-manifest", required=True)
    parser.add_argument("--mandatory-id", action="append", default=[])
    args = parser.parse_args()
    rows = read_tsv(Path(args.manifest))
    backend = read_json(Path(args.backend_manifest))
    if not str(backend.get("device", "")).startswith("cuda"):
        raise SystemExit("structure_backend_device_not_cuda")
    by_id = {row["candidate_id"]: row for row in rows}
    for candidate_id in args.mandatory_id:
        row = by_id.get(candidate_id)
        if not row or row.get("prediction_status") != "success":
            raise SystemExit(f"mandatory_structure_missing:{candidate_id}")
        if not row.get("device", "").startswith("cuda"):
            raise SystemExit(f"mandatory_structure_not_cuda:{candidate_id}")
        path = Path(row["structure_path"])
        if not path.exists() or sha256_file(path) != row.get("structure_sha256"):
            raise SystemExit(f"structure_hash_mismatch:{candidate_id}")
    print("structure_gpu_manifest_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
