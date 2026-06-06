import argparse
import csv
from pathlib import Path


REQUIRED = {
    "GPU_ENV_CHECK",
    "GPU_TENSOR_SMOKE_TEST",
    "EXPORT_CANDIDATE_FASTA",
    "PLM_EMBED_GPU",
    "STRUCTURE_PREDICT_GPU",
    "STRUCTURE_QUALITY",
    "PATCH_DESCRIPTOR",
    "PHASE05_REPORT",
    "PHASE05_ACCEPTANCE",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--trace", required=True)
    args = parser.parse_args()
    path = Path(args.trace)
    text = path.read_text(encoding="utf-8-sig", errors="replace")
    delimiter = "\t" if "\t" in text.splitlines()[0] else ","
    rows = list(csv.DictReader(text.splitlines(), delimiter=delimiter))
    names = {
        row.get("process") or row.get("name") or row.get("task_name") or ""
        for row in rows
    }
    missing = sorted(name for name in REQUIRED if name not in names)
    if missing:
        raise SystemExit("nextflow_gpu_trace_missing_processes:" + ",".join(missing))
    failed = [
        row.get("process") or row.get("name") or row.get("task_name") or ""
        for row in rows
        if row.get("status", "").upper() not in {"COMPLETED", "CACHED"}
    ]
    if failed:
        raise SystemExit("nextflow_gpu_trace_failed_processes:" + ",".join(failed))
    print("nextflow_gpu_trace_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
