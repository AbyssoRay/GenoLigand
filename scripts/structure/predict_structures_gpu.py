import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import write_json
from genoligand_gpu.structure_backends import predict_esmfold_hf_gpu


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-fasta", required=True)
    parser.add_argument("--candidate-tsv", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--backend", default="esmfold_hf_gpu")
    parser.add_argument("--model-id", default="facebook/esmfold_v1")
    parser.add_argument("--device", default="cuda:0")
    parser.add_argument("--max-candidates", type=int, default=11)
    parser.add_argument("--mandatory-id", action="append", default=[])
    parser.add_argument("--memory-probe-out", required=True)
    args = parser.parse_args()
    if args.backend != "esmfold_hf_gpu":
        raise SystemExit(f"unsupported_structure_backend:{args.backend}")
    result = predict_esmfold_hf_gpu(
        Path(args.input_fasta),
        Path(args.candidate_tsv),
        Path(args.output_dir),
        args.model_id,
        args.device,
        args.max_candidates,
        set(args.mandatory_id),
    )
    write_json(Path(args.memory_probe_out), result["memory_probe"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
