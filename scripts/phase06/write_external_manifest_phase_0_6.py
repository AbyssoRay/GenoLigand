from __future__ import annotations

import argparse
from pathlib import Path

from _bootstrap import DEFAULT_ARTIFACT_DIR, ROOT, env_or_default, path_arg
from genoligand_phase06.pipeline import write_external_manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=path_arg, default=DEFAULT_ARTIFACT_DIR / "external_resource_manifest.tsv")
    parser.add_argument("--foldseek-bin", default=env_or_default("FOLDSEEK_BIN", ""))
    parser.add_argument("--usalign-bin", default=env_or_default("USALIGN_BIN", ""))
    parser.add_argument("--biolip-cache", type=path_arg, default=ROOT / "data/external_phase_0_6/biolip2/BioLiP.txt.gz")
    parser.add_argument("--rcsb-dir", type=path_arg, default=ROOT / "data/external_phase_0_6/rcsb/pdb")
    args = parser.parse_args()
    rcsb_count = len(list(args.rcsb_dir.glob("*.pdb"))) if args.rcsb_dir.exists() else 0
    resources = [
        {
            "resource_id": "foldseek",
            "resource_type": "tool",
            "source_name": "Foldseek",
            "source_url": "https://mmseqs.com/foldseek/",
            "retrieval_mode": "local_or_wsl_binary",
            "retrieval_command": "foldseek version",
            "local_path": args.foldseek_bin,
            "version_or_release": "",
            "entry_count": "",
            "license_or_terms_note": "Foldseek executable used for real structure-search acceptance.",
            "used_in_process": "foldseek_createdb_and_search",
            "status": "PASS" if args.foldseek_bin else "FAIL",
            "notes": "",
        },
        {
            "resource_id": "usalign",
            "resource_type": "tool",
            "source_name": "US-align",
            "source_url": "https://zhanggroup.org/US-align/",
            "retrieval_mode": "local_binary",
            "retrieval_command": "USalign -h",
            "local_path": args.usalign_bin,
            "version_or_release": "",
            "entry_count": "",
            "license_or_terms_note": "US-align executable used for independent structure-alignment confirmation.",
            "used_in_process": "usalign_confirmation",
            "status": "PASS" if args.usalign_bin else "FAIL",
            "notes": "",
        },
        {
            "resource_id": "biolip2",
            "resource_type": "external_annotation",
            "source_name": "BioLiP2",
            "source_url": "https://zhanggroup.org/BioLiP/download/BioLiP.txt.gz",
            "retrieval_mode": "download",
            "retrieval_command": "prepare_biolip2_subset.py",
            "local_path": str(args.biolip_cache),
            "version_or_release": "current_download",
            "entry_count": "",
            "license_or_terms_note": "BioLiP/BioLiP2 annotations are external resources; respect provider terms.",
            "used_in_process": "reference_patch_database",
            "status": "PASS" if args.biolip_cache.exists() else "FAIL",
            "notes": "",
        },
        {
            "resource_id": "rcsb_pdb_subset",
            "resource_type": "external_structure_database",
            "source_name": "RCSB PDB",
            "source_url": "https://files.rcsb.org/download/",
            "retrieval_mode": "download",
            "retrieval_command": "fetch_rcsb_reference_subset.py",
            "local_path": str(args.rcsb_dir),
            "version_or_release": "current_download",
            "entry_count": rcsb_count,
            "license_or_terms_note": "RCSB PDB files; respect RCSB usage policies.",
            "used_in_process": "foldseek_reference_database",
            "status": "PASS" if rcsb_count > 0 else "FAIL",
            "notes": "",
        },
    ]
    # Avoid calling Path.stat on WSL-only paths such as /opt/foldseek/bin/foldseek.
    for resource in resources:
        local = str(resource.get("local_path", ""))
        if local.startswith("/"):
            resource["local_path"] = local
        elif local:
            resource["local_path"] = str(Path(local))
    write_external_manifest(args.out, resources)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
