import argparse
import subprocess


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--gls-cli", default="target/release/gls-cli.exe")
    parser.add_argument("--candidates", required=True)
    parser.add_argument("--sequence-evidence", required=True)
    parser.add_argument("--plm-gpu", required=True)
    parser.add_argument("--structure-manifest", required=True)
    parser.add_argument("--structure-quality", required=True)
    parser.add_argument("--patches", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--score-out", required=True)
    args = parser.parse_args()
    command = [
        args.gls_cli,
        "report",
        "phase-0-5",
        "--candidates",
        args.candidates,
        "--sequence-evidence",
        args.sequence_evidence,
        "--plm-gpu",
        args.plm_gpu,
        "--structure-manifest",
        args.structure_manifest,
        "--structure-quality",
        args.structure_quality,
        "--patches",
        args.patches,
        "--out",
        args.out,
        "--score-out",
        args.score_out,
    ]
    return subprocess.call(command)


if __name__ == "__main__":
    raise SystemExit(main())
