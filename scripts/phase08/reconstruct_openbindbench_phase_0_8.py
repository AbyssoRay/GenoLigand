from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase06.common import ensure_parent, read_tsv, sha256_text, write_json, write_tsv
from genoligand_phase08.full_database_strict import PHASE08_DIRNAME, path_arg, utc_now


REQUIRED_RECIPE_SOURCES = ["chembl", "bindingdb", "plinder", "binding_moad", "dude_z"]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE08_DIRNAME)
    parser.add_argument("--out-dir", type=Path, default=ROOT / "data/external_phase_0_8/reconstructed_openbindbench")
    args = parser.parse_args()
    manifest = args.artifact_dir / "external_resource_manifest.tsv"
    source_rows = {row.get("resource_id", ""): row for row in read_tsv(manifest)} if manifest.exists() else {}
    components = []
    for source_id in REQUIRED_RECIPE_SOURCES:
        row = source_rows.get(source_id, {})
        components.append(
            {
                "source_id": source_id,
                "source_status": row.get("status", "absent"),
                "source_sha256": row.get("sha256", ""),
                "file_count": row.get("file_count", "0"),
                "row_or_record_count": row.get("row_or_record_count", "0"),
            }
        )
    recipe = {
        "recipe_id": "phase08_reconstructed_openbindbench_v1",
        "policy": "repo_reconstructed_from_open_sources",
        "generated_at_utc": utc_now(),
        "required_sources": components,
        "positive_label_sources": ["chembl", "bindingdb", "plinder", "binding_moad"],
        "negative_label_sources": ["dude_z", "bindingdb_inactive_rows"],
        "leakage_controls": ["exact_ligand_target", "inchikey", "uniprot", "pdb_structure", "open_benchmark_overlap"],
    }
    recipe_text = json.dumps(recipe, indent=2, sort_keys=True)
    recipe["recipe_sha256"] = sha256_text(recipe_text)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    out_json = args.out_dir / "openbindbench_reconstruction_recipe.json"
    write_json(out_json, recipe)
    status = "PASS" if all(item["source_status"] == "PASS" and item["source_sha256"] for item in components) else "FAIL"
    rows = [
        {
            "recipe_id": recipe["recipe_id"],
            "recipe_sha256": recipe["recipe_sha256"],
            "component_source_count": len(components),
            "component_sources_pass": status == "PASS",
            "status": status,
            "notes": "all component sources available" if status == "PASS" else "component sources absent or unaudited",
        }
    ]
    write_tsv(args.artifact_dir / "outputs/benchmark/openbindbench_reconstruction_summary.tsv", rows, ["recipe_id", "recipe_sha256", "component_source_count", "component_sources_pass", "status", "notes"])
    return 0 if status == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())

