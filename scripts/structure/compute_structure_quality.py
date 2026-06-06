import argparse
from pathlib import Path

from genoligand_gpu.pdb_quality import compute_quality_and_patches


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--out-quality", required=True)
    parser.add_argument("--out-graph", required=True)
    parser.add_argument("--out-patches", required=True)
    parser.add_argument("--out-secondary", required=True)
    parser.add_argument("--radius", type=float, default=8.0)
    parser.add_argument("--min-patch-residues", type=int, default=3)
    args = parser.parse_args()
    stats = compute_quality_and_patches(
        Path(args.manifest),
        Path(args.out_quality),
        Path(args.out_graph),
        Path(args.out_patches),
        Path(args.out_secondary),
        args.radius,
        args.min_patch_residues,
    )
    print(stats)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
