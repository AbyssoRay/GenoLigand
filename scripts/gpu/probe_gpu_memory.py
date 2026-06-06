import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.cuda_env import memory_probe_payload, nvidia_memory_mib
from genoligand_gpu.io import write_json


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--process-name", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    before = nvidia_memory_mib()
    payload = memory_probe_payload(args.process_name, before, before, before, 0)
    write_json(Path(args.out), payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
