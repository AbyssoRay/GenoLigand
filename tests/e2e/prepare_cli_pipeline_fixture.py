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
    "indexes/bindingdb/bindingdb_query.manifest.json",
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


def write_bindingdb_query_fixture(store: Path) -> None:
    database = store / "indexes/bindingdb/bindingdb_query.sqlite"
    database.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(database)
    try:
        connection.execute(
            """
            CREATE TABLE genoligand_bindingdb_activity (
                source_row_id INTEGER PRIMARY KEY,
                canonical_smiles TEXT NOT NULL,
                compact_smiles TEXT NOT NULL,
                ligand_smiles TEXT NOT NULL,
                ligand_name TEXT NOT NULL,
                ligand_name_lower TEXT NOT NULL,
                target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                organism TEXT NOT NULL,
                ligand_code TEXT NOT NULL,
                pdb_ids TEXT NOT NULL,
                target_chain_pdb_ids TEXT NOT NULL,
                activity TEXT NOT NULL,
                activity_type TEXT NOT NULL,
                relation TEXT NOT NULL,
                numeric_rank REAL NOT NULL,
                relation_rank INTEGER NOT NULL,
                activity_score REAL NOT NULL,
                pubchem_cid TEXT NOT NULL
            )
            """
        )
        connection.execute(
            "CREATE TABLE genoligand_bindingdb_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
        )
        connection.executemany(
            "INSERT INTO genoligand_bindingdb_metadata VALUES (?, ?)",
            [
                ("schema_version", "genoligand_bindingdb_query_database_v1"),
                ("source_digest", "a" * 64),
                ("source_release", "unit_test_release"),
            ],
        )
        connection.execute(
            "INSERT INTO genoligand_bindingdb_activity VALUES "
            "(1, 'CCO', 'cco', 'CCO', 'ethanol', 'ethanol', 'P24941', "
            "'Cyclin-dependent kinase 2', 'Homo sapiens', '', '', '', '12', "
            "'Ki (nM)', '=', 12.0, 0, 0.95, '')"
        )
        connection.execute(
            "CREATE INDEX idx_bindingdb_activity_canonical_smiles "
            "ON genoligand_bindingdb_activity(canonical_smiles)"
        )
        connection.execute(
            "CREATE INDEX idx_bindingdb_activity_compact_smiles "
            "ON genoligand_bindingdb_activity(compact_smiles)"
        )
        connection.commit()
    finally:
        connection.close()
    manifest = {
        "schema_version": "genoligand_bindingdb_query_database_v1",
        "status": "ready",
        "table": "genoligand_bindingdb_activity",
        "rows": 1,
        "database_bytes": database.stat().st_size,
        "database_sha256": file_sha256(database),
        "source_path": "downloads/bindingdb/bindingdb_all.tsv.zip",
        "source_bytes": 1,
        "source_digest": "a" * 64,
        "source_release": "unit_test_release",
        "source_archive_sha256": "b" * 64,
        "builder_version": "unit-test-fixture",
        "indexes": [
            "idx_bindingdb_activity_canonical_smiles",
            "idx_bindingdb_activity_compact_smiles",
        ],
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

    write_bindingdb_query_fixture(store)

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
