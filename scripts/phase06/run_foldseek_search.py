from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, ROOT, env_or_default, path_arg
from genoligand_phase06.pipeline import run_foldseek_search


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--foldseek-bin", default=env_or_default("FOLDSEEK_BIN", "foldseek"))
    parser.add_argument("--wsl-distro", default=env_or_default("FOLDSEEK_WSL_DISTRO", ""))
    parser.add_argument("--query-db", type=path_arg, default=ROOT / "databases/phase_0_6/foldseek/query_db")
    parser.add_argument("--reference-db", type=path_arg, default=ROOT / "databases/phase_0_6/foldseek/reference_db")
    parser.add_argument("--result-db", type=path_arg, default=ROOT / "databases/phase_0_6/foldseek/result_db")
    parser.add_argument("--tmp-dir", type=path_arg, default=ROOT / "databases/phase_0_6/foldseek/tmp")
    parser.add_argument("--hits-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/structure_similarity/foldseek_hits.tsv")
    parser.add_argument("--reference-manifest", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_structures/reference_structure_manifest.tsv")
    parser.add_argument("--metrics-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics")
    parser.add_argument("--logs-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "logs/foldseek")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--evalue", type=float, default=10.0)
    parser.add_argument("--sensitivity", type=float, default=9.5)
    parser.add_argument("--max-seqs", type=int, default=20)
    args = parser.parse_args()
    distro = args.wsl_distro or None
    run_foldseek_search(
        args.foldseek_bin,
        distro,
        args.query_db,
        args.reference_db,
        args.result_db,
        args.tmp_dir,
        args.hits_out,
        args.reference_manifest,
        args.metrics_dir,
        args.logs_dir,
        args.threads,
        args.evalue,
        args.sensitivity,
        args.max_seqs,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
