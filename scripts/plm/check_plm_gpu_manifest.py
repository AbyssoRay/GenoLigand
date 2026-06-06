import argparse
from pathlib import Path

from genoligand_gpu.io import read_json, read_tsv


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--embeddings", required=True)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--min-rows", type=int, default=1)
    args = parser.parse_args()
    rows = read_tsv(Path(args.embeddings))
    manifest = read_json(Path(args.manifest))
    if len(rows) < args.min_rows:
        raise SystemExit("plm_gpu_rows_below_minimum")
    if manifest.get("backend") != "fair_esm_gpu":
        raise SystemExit("plm_gpu_backend_not_fair_esm_gpu")
    if not str(manifest.get("device", "")).startswith("cuda"):
        raise SystemExit("plm_manifest_device_not_cuda")
    bad = [row["candidate_id"] for row in rows if not row.get("device", "").startswith("cuda")]
    if bad:
        raise SystemExit("plm_rows_not_cuda:" + ",".join(bad[:10]))
    print("plm_gpu_manifest_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
