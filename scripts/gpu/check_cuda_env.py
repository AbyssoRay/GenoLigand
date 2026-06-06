import argparse
from pathlib import Path

from genoligand_gpu.cuda_env import collect_cuda_env, write_or_fail


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--nvcc", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    payload = collect_cuda_env(args.nvcc)
    return write_or_fail(payload, Path(args.out), args.strict)


if __name__ == "__main__":
    raise SystemExit(main())
