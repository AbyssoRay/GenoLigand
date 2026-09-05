from __future__ import annotations

import argparse
import hashlib
import json
import sqlite3
from pathlib import Path


EVIDENCE_PATHS = (
    "source_requirements.tsv",
    "manifests/source_file_manifest.tsv",
    "install_status.json",
    "index_build_manifest.tsv",
    "index_readiness_manifest.tsv",
    "science_index_readiness.tsv",
    "indexes/known_targets.tsv",
    "indexes/metagenome_targets.tsv",
    "indexes/structure_targets.tsv",
    "indexes/tool_versions.tsv",
    "indexes/mmseqs/mmseqs_target_databases.tsv",
    "indexes/foldseek/foldseek_target_databases.tsv",
    "indexes/ligand_similarity/ligand_similarity.manifest.json",
    "downloads/rcsb/ligand_bound_pdb_scope.json",
    "downloads/rcsb/ligand_bound_pdb_scope.tsv",
)

STORE_HEADER = (
    "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
    "sequence_tokens\tstructure_tokens\tevidence\n"
)

REQUIRED_SOURCE_IDS = (
    "chembl",
    "bindingdb",
    "uniprot_human_reviewed",
    "rcsb_ligand_bound",
    "biolip2",
    "plinder",
    "binding_moad",
    "metagenome_potential",
)

REQUIRED_TOOL_IDS = (
    "rdkit",
    "openbabel",
    "mmseqs2",
    "foldseek",
    "autodock_vina",
    "openmm",
    "pdbfixer",
    "openff_toolkit",
    "mdtraj",
    "mdanalysis",
)

QUERY_STRUCTURE_WORKER_SOURCE = r'''from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path


def runtime_path(raw: str) -> Path:
    if os.name != "nt" and len(raw) >= 3 and raw[1] == ":":
        return Path("/mnt") / raw[0].lower() / raw[3:].replace("\\", "/")
    return Path(raw)


def write_json(path: Path, value: dict) -> str:
    payload = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
    path.write_bytes(payload)
    return hashlib.sha256(payload).hexdigest()


parser = argparse.ArgumentParser()
parser.add_argument("--job", required=True)
parser.add_argument("--out", required=True)
args = parser.parse_args()
job = json.loads(runtime_path(args.job).read_text(encoding="utf-8-sig"))
run_dir = runtime_path(str(job["run_dir"]))
artifact_dir = run_dir / "tool_outputs" / "query_structure" / job["job_id"]
artifact_dir.mkdir(parents=True, exist_ok=True)
relative_dir = artifact_dir.relative_to(run_dir).as_posix()
method = "genoligand_query_structure_prepare_v1"
conversion_digest = hashlib.sha256(
    (run_dir / "input_conversion_manifest.tsv").read_bytes()
).hexdigest()

coordinate = artifact_dir / "query.sdf"
coordinate.write_text(
    "unit-test-query\n  GenoLigand\n\n  0  0  0  0  0  0            999 V2000\n"
    "M  END\n$$$$\n",
    encoding="utf-8",
)
coordinate_relative = f"{relative_dir}/query.sdf"
coordinate_bytes = coordinate.read_bytes()
coordinate_digest = hashlib.sha256(coordinate_bytes).hexdigest()
packed = bytes(256)
common = {
    "status": "ready",
    "run_id": job["run_id"],
    "job_id": job["job_id"],
    "invocation_id": job["invocation_id"],
    "input_id": job["input_id"],
    "record_id": job["record_id"],
    "input_kind": job["input_kind"],
    "input_digest": job["input_digest"],
    "source_content_sha256": job["source_digest"],
    "method_version": method,
}
features = {
    **common,
    "schema_version": "genoligand_query_structure_features_v1",
    "conversion_manifest_sha256": conversion_digest,
    "canonical_smiles": "CCO",
    "physicochemical": {
        "molecular_weight": 46.07,
        "logp": -0.3,
        "topological_polar_surface_area": 20.23,
        "hydrogen_bond_donors": 1,
        "hydrogen_bond_acceptors": 1,
        "rotatable_bonds": 0,
        "ring_count": 0,
        "aromatic_ring_count": 0,
        "fraction_csp3": 1.0,
        "formal_charge": 0,
        "heavy_atom_count": 3,
        "qed": 0.4,
    },
    "fingerprint": {
        "algorithm": "morgan_ecfp4",
        "radius": 2,
        "n_bits": 2048,
        "include_chirality": True,
        "encoding": "rdkit_binary_text_hex_v1",
        "packed_hex": packed.hex(),
        "packed_sha256": hashlib.sha256(packed).hexdigest(),
        "population_count": 0,
    },
    "pharmacophore": {"counts": {}, "point_count": 0},
    "coordinate_artifact": {
        "path": coordinate_relative,
        "sha256": coordinate_digest,
        "size_bytes": len(coordinate_bytes),
        "retention": "current_run_query_coordinate",
    },
}
features_path = artifact_dir / "features.json"
feature_digest = write_json(features_path, features)
features_relative = f"{relative_dir}/features.json"
manifest = {
    **common,
    "schema_version": "genoligand_query_structure_artifact_v1",
    "conversion_manifest_sha256": conversion_digest,
    "features_json": features_relative,
    "feature_digest": feature_digest,
    "coordinate_artifact": coordinate_relative,
    "coordinate_sha256": coordinate_digest,
    "coordinate_size_bytes": len(coordinate_bytes),
    "retention": "current_run_query_artifacts",
}
manifest_path = artifact_dir / "manifest.json"
write_json(manifest_path, manifest)
manifest_relative = f"{relative_dir}/manifest.json"

(run_dir / "query_structure_manifest.tsv").write_text(
    "input_id\trecord_id\tinput_kind\tinput_digest\tmethod\tfeatures_json\t"
    "coordinate_artifact\tfeature_digest\tstatus\n"
    f"{job['input_id']}\t{job['record_id']}\t{job['input_kind']}\t"
    f"{job['input_digest']}\t{method}\t{features_relative}\t"
    f"{coordinate_relative}\t{feature_digest}\tready\n",
    encoding="utf-8",
)
reason = "query_structure_fixture_completed"
result = {
    "schema_version": "genoligand_worker_current_v2",
    "job_id": job["job_id"],
    "worker": "query_structure_prepare",
    "run_id": job["run_id"],
    "invocation_id": job["invocation_id"],
    "status": "PASS",
    "engine_version": "unit-test-query-structure-fixture",
    "execution_status": "completed",
    "reason": reason,
    "outcomes": [{
        "status": "completed",
        "reason": reason,
        "required": True,
        "database_id": "query_structure_prepare",
        "row_count": 1,
    }],
    "python_owns_final_score": False,
    "python_owns_report_semantics": False,
    "primitive_execution_only": True,
    "input_id": job["input_id"],
    "record_id": job["record_id"],
    "input_digest": job["input_digest"],
    "source_digest": job["source_digest"],
    "artifact_path": features_relative,
    "manifest": manifest_relative,
}
runtime_path(args.out).write_text(json.dumps(result), encoding="utf-8")
'''


LIGAND_SIMILARITY_WORKER_SOURCE = r'''from __future__ import annotations

import argparse
import csv
import hashlib
import json
from pathlib import Path


parser = argparse.ArgumentParser()
subparsers = parser.add_subparsers(dest="command", required=True)
query = subparsers.add_parser("query-targets")
query.add_argument("--database", required=True)
query.add_argument("--manifest", required=True)
query.add_argument("--query-manifest", required=True)
query.add_argument("--output-tsv", required=True)
query.add_argument("--top-k", required=True)
query.add_argument("--threshold", required=True)
args = parser.parse_args()

with Path(args.query_manifest).open("r", encoding="utf-8-sig", newline="") as handle:
    row = next(csv.DictReader(handle, delimiter="\t"))
feature_digest = row["feature_digest"]
database_sha256 = hashlib.sha256(Path(args.database).read_bytes()).hexdigest()
manifest_bytes = Path(args.manifest).read_bytes()
manifest_sha256 = hashlib.sha256(manifest_bytes).hexdigest()
manifest = json.loads(manifest_bytes.decode("utf-8-sig"))
stable_fingerprint = {
    name: manifest["fingerprint"][name]
    for name in (
        "type",
        "radius",
        "bits",
        "include_chirality",
        "standardization",
        "tautomer_max_tautomers",
        "tautomer_max_transforms",
        "lsh_bands",
        "search_algorithm",
        "bitbound_formula",
        "reference",
        "encoding",
    )
}
build_identity_payload = {
    "schema_version": manifest["schema_version"],
    "fingerprint": stable_fingerprint,
    "sources": {
        source: {
            "source_digest": manifest["inputs"][source]["source_digest"],
            "source_release": manifest["inputs"][source]["source_release"],
        }
        for source in ("chembl", "bindingdb")
    },
}
build_identity = hashlib.sha256(
    json.dumps(
        build_identity_payload, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
).hexdigest()
publication = (
    "ligand_similarity_index;"
    f"compact_database_sha256={database_sha256};"
    f"compact_manifest_sha256={manifest_sha256};"
    f"compact_schema_version={manifest['schema_version']};"
    f"compact_build_identity={build_identity}"
)
query_id = hashlib.sha256(
    "\0".join(
        [row["input_id"], row["record_id"], feature_digest, "ChEMBL", "CHEMBL_T1", "CCO"]
    ).encode()
).hexdigest()
header = (
    "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\t"
    "matched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\t"
    "current_run_query_id\trecord_id\n"
)
result = (
    f"{row['input_id']}\tChEMBL\tCHEMBL_T1\tCyclin-dependent kinase 2\t"
    "Homo sapiens\tenzyme\t0.9500\tcanonical_smiles=CCO;tanimoto=1.0000;"
    f"structural_compatibility=1.0000\t{publication};"
    f"query_feature_digest={feature_digest}\t{publication}\tKi=12 nM\t"
    f"ligand_smiles=CCO\t{query_id}\t{row['record_id']}\n"
)
Path(args.output_tsv).write_text(header + result, encoding="utf-8")
'''


WORKER_SOURCE = r'''from __future__ import annotations

import argparse
import json
import os
from pathlib import Path


parser = argparse.ArgumentParser()
parser.add_argument("--job", required=True)
parser.add_argument("--out", required=True)
args = parser.parse_args()
with open(args.job, "r", encoding="utf-8-sig") as handle:
    job = json.load(handle)

headers = {
    "mmseqs_alignment": "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\tsource_database_id\tsource_namespace\tsource_class\tsource_digest\tbuilder_version\tmmseqs_version\tcurrent_run_query_id\trecord_id\tquery_kind\talignment_length\tquery_coverage\ttarget_coverage\talignment_cigar\n",
    "foldseek_alignment": "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\tmatched_features\tevidence\trecord_id\tsource_database_id\tsource_digest\tbuilder_version\tfoldseek_version\tcurrent_run_query_id\tquery_digest\ttm_score\n",
}
if job["worker"] in headers:
    raw_run_dir = str(job["run_dir"])
    if os.name != "nt" and len(raw_run_dir) >= 3 and raw_run_dir[1] == ":":
        run_dir = Path("/mnt") / raw_run_dir[0].lower() / raw_run_dir[3:].replace("\\", "/")
    else:
        run_dir = Path(raw_run_dir)
    output_dir = run_dir / "tool_outputs"
    output_dir.mkdir(parents=True, exist_ok=True)
    name = (
        "mmseqs_alignment_matches.tsv"
        if job["worker"] == "mmseqs_alignment"
        else "foldseek_structure_matches.tsv"
    )
    (output_dir / name).write_text(headers[job["worker"]], encoding="utf-8")

reason = "test_fixture_not_applicable"
result = {
    "schema_version": "genoligand_worker_current_v2",
    "job_id": job["job_id"],
    "worker": job["worker"],
    "invocation_id": job["invocation_id"],
    "status": "NOT_APPLICABLE",
    "engine_version": "test worker fixture",
    "execution_status": "not_applicable",
    "reason": reason,
    "outcomes": [{
        "status": "not_applicable",
        "reason": reason,
        "required": False,
        "database_id": job["worker"],
        "row_count": 0,
    }],
    "python_owns_final_score": False,
    "python_owns_report_semantics": False,
    "primitive_execution_only": True,
}
for field in (
    "run_id",
    "candidate_id",
    "candidate_digest",
    "input_id",
    "record_id",
    "input_digest",
    "source_digest",
    "receptor_digest",
):
    if field in job:
        result[field] = job[field]
with open(args.out, "w", encoding="utf-8") as handle:
    json.dump(result, handle)
'''


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_ligand_similarity_fixture(store: Path) -> None:
    database = store / "indexes/ligand_similarity/ligand_similarity.sqlite"
    database.parent.mkdir(parents=True, exist_ok=True)
    counts = {"molecules": 1, "targets": 1, "evidence": 1, "hints": 0}
    fingerprint = {
        "type": "morgan_ecfp4",
        "radius": 2,
        "bits": 2048,
        "include_chirality": True,
        "standardization": "rdkit_cleanup_fragment_parent_reionize_canonical_tautomer_v1",
        "tautomer_max_tautomers": 100,
        "tautomer_max_transforms": 100,
        "lsh_bands": 0,
        "search_algorithm": "popcount_bitbound_exact_tanimoto",
        "bitbound_formula": (
            "ceil(t*q_popcount) <= db_popcount <= floor(q_popcount/t)"
        ),
        "reference": {"name": "FPSim2", "license": "MIT", "commit": "e03e669"},
        "encoding": "rdkit_binary_text",
    }
    source_identity = {
        source_id: {
            "source_digest": "a" * 64,
            "source_release": "unit_test_release",
        }
        for source_id in ("chembl", "bindingdb")
    }
    connection = sqlite3.connect(database)
    try:
        connection.execute(
            "CREATE TABLE molecule ("
            "id INTEGER PRIMARY KEY, canonical_smiles TEXT NOT NULL UNIQUE, "
            "fingerprint BLOB NOT NULL, popcount INTEGER NOT NULL)"
        )
        connection.execute(
            "CREATE TABLE target ("
            "id INTEGER PRIMARY KEY, source TEXT NOT NULL, target_id TEXT NOT NULL, "
            "source_target_id TEXT NOT NULL, target_name TEXT NOT NULL, "
            "organism TEXT NOT NULL, target_class TEXT NOT NULL, "
            "source_release TEXT NOT NULL, source_digest TEXT NOT NULL)"
        )
        connection.execute(
            "CREATE TABLE evidence ("
            "molecule_id INTEGER NOT NULL, target_fk INTEGER NOT NULL, "
            "activity_score REAL NOT NULL, activity TEXT NOT NULL, "
            "relation_rank INTEGER NOT NULL, numeric_rank REAL NOT NULL, "
            "PRIMARY KEY (molecule_id, target_fk)) WITHOUT ROWID"
        )
        connection.execute(
            "CREATE TABLE structure_hint ("
            "molecule_id INTEGER NOT NULL, target_fk INTEGER NOT NULL, "
            "ligand_code TEXT NOT NULL, pdb_ids TEXT NOT NULL, "
            "target_chain_pdb_ids TEXT NOT NULL, "
            "PRIMARY KEY (molecule_id, target_fk, ligand_code)) WITHOUT ROWID"
        )
        connection.execute(
            "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID"
        )
        connection.execute(
            "CREATE INDEX idx_molecule_popcount ON molecule(popcount, id)"
        )
        connection.execute("INSERT INTO molecule VALUES (1, 'CCO', ?, 3)", (b"x",))
        connection.execute(
            "INSERT INTO target VALUES "
            "(1, 'ChEMBL', 'CHEMBL_T1', 'CHEMBL_T1', "
            "'Cyclin-dependent kinase 2', 'Homo sapiens', 'enzyme', "
            "'unit_test_release', ?)",
            ("a" * 64,),
        )
        connection.execute(
            "INSERT INTO evidence VALUES (1, 1, 0.95, 'Ki=12 nM', 0, 12.0)"
        )
        connection.executemany(
            "INSERT INTO metadata VALUES (?, ?)",
            [
                ("schema_version", "genoligand_ligand_similarity_index_v3"),
                ("status", "ready"),
                ("counts", json.dumps(counts, sort_keys=True)),
                ("fingerprint", json.dumps(fingerprint, sort_keys=True)),
                ("source_identity", json.dumps(source_identity, sort_keys=True)),
            ],
        )
        connection.commit()
    finally:
        connection.close()
    input_bytes = 1_000_000
    database_bytes = database.stat().st_size
    manifest = {
        "schema_version": "genoligand_ligand_similarity_index_v3",
        "status": "ready",
        "database": {
            "path": str(database.resolve()),
            "bytes": database_bytes,
            "sha256": file_sha256(database),
        },
        "counts": counts,
        "fingerprint": fingerprint,
        "inputs": {
            source_id: {
                "path": f"superseded/{source_id}_query.sqlite",
                "manifest_path": f"superseded/{source_id}_query.manifest.json",
                "bytes": input_bytes,
                "sha256": "b" * 64,
                "source_digest": "a" * 64,
                "source_release": "unit_test_release",
            }
            for source_id in ("chembl", "bindingdb")
        },
        "reclaimed_candidate_bytes": input_bytes * 2 - database_bytes,
    }
    database.with_suffix(".manifest.json").write_text(
        json.dumps(manifest, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def write_ready_store(store: Path) -> None:
    indexes = store / "indexes"
    indexes.mkdir(parents=True, exist_ok=True)
    for name in ("known_targets.tsv", "metagenome_targets.tsv", "structure_targets.tsv"):
        (indexes / name).write_text(STORE_HEADER, encoding="utf-8")

    (store / "science_store_manifest.tsv").write_text(
        "item\tstatus\tpath\n"
        "warehouse\tready\tindexes/known_targets.tsv\n"
        "metagenome_index\tready\tindexes/metagenome_targets.tsv\n"
        "structure_index\tready\tindexes/structure_targets.tsv\n"
        "tool_versions\tready\tindexes/tool_versions.tsv\n",
        encoding="utf-8",
    )
    (store / "install_status.json").write_text(
        '{"status":"ready","test_context":"unit_only","missing_components":[]}\n',
        encoding="utf-8",
    )
    (store / "index_build_manifest.tsv").write_text(
        "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
        "known_targets\tunit_test/known_targets.tsv\tused\t0\tindexes/known_targets.tsv\tfull_test_index\n"
        "metagenome_targets\tunit_test/metagenome_targets.tsv\tused\t0\tindexes/metagenome_targets.tsv\tfull_test_index\n"
        "structure_targets\tunit_test/structure_targets.tsv\tused\t0\tindexes/structure_targets.tsv\tfull_test_index\n",
        encoding="utf-8",
    )

    requirements = (
        "source_id\tlabel\tcategory\tstatus\tlanding_url\taccess_method\tlicense_note\tlocal_path\tindex_target\trepair_action\n"
    )
    source_manifest = (
        "source_id\tsource_label\tsource_class\tlocal_path\tstatus\tfile_count\tbytes\tsha256\tprovenance\trelease_identity\ttrust_source\n"
    )
    for source_id in REQUIRED_SOURCE_IDS:
        requirements += (
            f"{source_id}\t{source_id}\tunit_test\tready\tunit_test\tunit_test\t"
            f"unit_test\tdownloads/{source_id}\tindexes/known_targets.tsv\tunit_test\n"
        )
        source_manifest += (
            f"{source_id}\t{source_id}\tunit_test\tdownloads/{source_id}\tready\t"
            f"1\t1\t{'a' * 64}\tunit_test\tunit_test_release\tunit_test\n"
        )
    (store / "source_requirements.tsv").write_text(requirements, encoding="utf-8")
    manifests = store / "manifests"
    manifests.mkdir(exist_ok=True)
    (manifests / "source_file_manifest.tsv").write_text(
        source_manifest, encoding="utf-8"
    )

    tool_versions = "tool\tstatus\tversion\tlicense\tlabel\tdetail\n"
    for tool_id in REQUIRED_TOOL_IDS:
        tool_versions += (
            f"{tool_id}\tready\tunit-test-fixture\tunit_test\t{tool_id}\t"
            "unit_test_store_fixture\n"
        )
    (indexes / "tool_versions.tsv").write_text(tool_versions, encoding="utf-8")

    for tool in ("mmseqs", "foldseek"):
        relative = f"indexes/{tool}/unit_test/target"
        prefix = store / relative
        prefix.parent.mkdir(parents=True, exist_ok=True)
        prefix.write_text("unit-test-database\n", encoding="utf-8")
        for suffix, contents in (
            (".dbtype", "1234"),
            (".index", "index\n"),
            (".lookup", "lookup\n"),
        ):
            Path(f"{prefix}{suffix}").write_text(contents, encoding="utf-8")
        (indexes / tool / f"{tool}_target_databases.tsv").write_text(
            "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\t"
            "source_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n"
            f"unit_test\tgenoligand_alignment_database_manifest_v2\tunit_test\tunit_test\t"
            f"{'a' * 64}\t1\t1\tunit_test_builder\t{relative}\tready\n",
            encoding="utf-8",
        )

    write_ligand_similarity_fixture(store)

    evidence = []
    for relative in EVIDENCE_PATHS:
        path = store / relative
        if path.is_file():
            evidence.append({"path": relative, "sha256": file_sha256(path)})
    digest_input = "".join(
        f"{row['path']}\t{row['sha256']}\n" for row in evidence
    ).encode("utf-8")
    readiness = {
        "schema_version": "genoligand_science_store_readiness_v1",
        "status": "ready",
        "ready": True,
        "generated_at": 1,
        "evidence_manifest": evidence,
        "evidence_digest": hashlib.sha256(digest_input).hexdigest(),
        "sources": [],
        "indexes": [],
        "blockers": [],
        "repair_actions": [],
    }
    (store / "science_store_readiness.json").write_text(
        json.dumps(readiness, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def prepare(root: Path, mode: str) -> None:
    root.mkdir(parents=True, exist_ok=True)
    input_manifest = root / "input_manifest.json"
    if mode == "failed":
        input_manifest.write_text("{\n", encoding="utf-8")
    else:
        input_manifest.write_text(
            '{"inputs":[{"field":"small_molecule","value":"CCO"}],"files":[]}\n',
            encoding="utf-8",
        )

    store = root / "science_store"
    store.mkdir(exist_ok=True)
    if mode == "ready":
        write_ready_store(store)

    package = root / "python" / "genoligand_workers"
    package.mkdir(parents=True, exist_ok=True)
    (package / "__init__.py").write_text("", encoding="utf-8")
    (package / "query_structure_worker.py").write_text(
        QUERY_STRUCTURE_WORKER_SOURCE, encoding="utf-8"
    )
    (package / "ligand_similarity_index.py").write_text(
        LIGAND_SIMILARITY_WORKER_SOURCE, encoding="utf-8"
    )
    for module in ("alignment_worker.py", "docking_worker.py", "md_summary_worker.py"):
        (package / module).write_text(WORKER_SOURCE, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--mode", choices=("ready", "blocked", "failed"), required=True)
    args = parser.parse_args()
    prepare(args.root.resolve(), args.mode)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
