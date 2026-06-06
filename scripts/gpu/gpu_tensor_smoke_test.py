import argparse
from pathlib import Path

from genoligand_gpu.cuda_env import tensor_smoke
from genoligand_gpu.io import write_json


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--device", default="cuda:0")
    parser.add_argument("--size", type=int, default=4096)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    payload = tensor_smoke(args.device, args.size)
    write_json(Path(args.out), payload)
    return 0 if payload.get("success") else 1


if __name__ == "__main__":
    raise SystemExit(main())
