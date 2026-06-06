from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, DEFAULT_RCSB_PDB_URL, ROOT, path_arg
from genoligand_phase06.pipeline import fetch_rcsb_reference_subset


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--biolip-tsv", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_structures/biolip2_interactions.tsv")
    parser.add_argument("--out-dir", type=path_arg, default=ROOT / "data/external_phase_0_6/rcsb")
    parser.add_argument("--manifest-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_structures/reference_structure_manifest.tsv")
    parser.add_argument("--summary-out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "metrics/reference_structure_summary.tsv")
    parser.add_argument("--min-entries", type=int, default=200)
    parser.add_argument("--preferred-entries", type=int, default=500)
    parser.add_argument("--url-template", default=DEFAULT_RCSB_PDB_URL)
    args = parser.parse_args()
    fetch_rcsb_reference_subset(
        args.biolip_tsv,
        args.out_dir,
        args.manifest_out,
        args.summary_out,
        args.min_entries,
        args.preferred_entries,
        args.url_template,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
