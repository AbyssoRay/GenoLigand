import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import write_json
from genoligand_gpu.plm import embed_fair_esm_gpu


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-fasta", required=True)
    parser.add_argument("--candidate-tsv", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--model-id", default="esm2_t6_8M_UR50D")
    parser.add_argument("--device", default="cuda:0")
    parser.add_argument("--batch-size", type=int, default=4)
    parser.add_argument("--memory-probe-out", required=True)
    args = parser.parse_args()
    result = embed_fair_esm_gpu(
        Path(args.input_fasta),
        Path(args.candidate_tsv),
        Path(args.output_dir),
        args.model_id,
        args.batch_size,
        args.device,
    )
    write_json(Path(args.memory_probe_out), result["memory_probe"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
