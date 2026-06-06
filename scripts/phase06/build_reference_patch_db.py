from __future__ import annotations

import argparse

from _bootstrap import DEFAULT_ARTIFACT_DIR, path_arg
from genoligand_phase06.pipeline import build_reference_patch_db


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--biolip-tsv", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_structures/biolip2_interactions.tsv")
    parser.add_argument("--reference-manifest", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_structures/reference_structure_manifest.tsv")
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_patches/reference_binding_patches.tsv")
    parser.add_argument("--manifest-json", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "outputs/reference_patches/reference_patch_manifest.json")
    parser.add_argument("--min-patches", type=int, default=200)
    args = parser.parse_args()
    build_reference_patch_db(args.biolip_tsv, args.reference_manifest, args.out, args.manifest_json, args.min_patches)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
