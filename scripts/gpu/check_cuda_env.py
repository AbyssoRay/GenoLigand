import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.cuda_env import collect_cuda_env, write_or_fail


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--nvcc", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--torch-probe-out")
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    payload = collect_cuda_env(args.nvcc)
    if args.torch_probe_out:
        from genoligand_gpu.io import write_json

        write_json(Path(args.torch_probe_out), payload)
    return write_or_fail(payload, Path(args.out), args.strict)


if __name__ == "__main__":
    raise SystemExit(main())
