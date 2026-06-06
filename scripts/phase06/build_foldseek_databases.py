from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, ROOT, env_or_default, path_arg
from genoligand_phase06.pipeline import build_foldseek_databases


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--foldseek-bin", default=env_or_default("FOLDSEEK_BIN", "foldseek"))
    parser.add_argument("--wsl-distro", default=env_or_default("FOLDSEEK_WSL_DISTRO", ""))
    parser.add_argument("--query-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/query_structures/pdb")
    parser.add_argument("--reference-dir", type=path_arg, default=ROOT / "data/external_phase_0_6/rcsb/pdb")
    parser.add_argument("--query-db", type=path_arg, default=ROOT / "databases/phase_0_6/foldseek/query_db")
    parser.add_argument("--reference-db", type=path_arg, default=ROOT / "databases/phase_0_6/foldseek/reference_db")
    parser.add_argument("--logs-dir", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "logs/foldseek")
    args = parser.parse_args()
    distro = args.wsl_distro or None
    return build_foldseek_databases(args.foldseek_bin, distro, args.query_dir, args.reference_dir, args.query_db, args.reference_db, args.logs_dir)


if __name__ == "__main__":
    raise SystemExit(main())
