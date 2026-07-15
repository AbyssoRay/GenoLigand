from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import sqlite3
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


TABLE = "genoligand_onsite_activity"
RELATIONS = {
    "=": "exact",
    ">": "lower_exclusive",
    "<": "upper_exclusive",
    "~": "approximate",
    ">=": "lower_inclusive",
    "<=": "upper_inclusive",
    "NULL": "unknown",
}
ELIGIBLE = (
    "standard_type IN ('Ki', 'Kd', 'IC50', 'EC50') "
    "AND standard_value IS NOT NULL "
    "AND (standard_units = '' OR standard_units = 'nM')"
)
COLUMNS = (
    "canonical_isomeric_smiles, target_id, source_target_id, target_name, organism, "
    "compound_name, standard_type, standard_relation, standard_value, standard_units, "
    "source_release, source_digest"
)
DISCOVERY_LIMIT = 100_000


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(4 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def valid_sha256(value: object) -> bool:
    text = str(value or "")
    return len(text) == 64 and all(ch in "0123456789abcdefABCDEF" for ch in text)


def atomic_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = path.with_name(f".{path.name}.{os.getpid()}.{time.time_ns()}.tmp")
    try:
        with pending.open("x", encoding="utf-8", newline="\n") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(pending, path)
    finally:
        pending.unlink(missing_ok=True)


def relation_key(value: object) -> str:
    if value is None or not str(value).strip():
        return "NULL"
    return str(value).strip()


def file_snapshot(root: Path) -> dict[str, tuple[int, int]]:
    if not root.is_dir():
        return {}
    return {
        path.relative_to(root).as_posix(): (path.stat().st_size, path.stat().st_mtime_ns)
        for path in root.rglob("*")
        if path.is_file()
    }


def quote_identifier(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def load_publication(database: Path) -> tuple[Path, dict[str, Any], str]:
    require(database.is_file() and database.stat().st_size > 0, f"missing database: {database}")
    manifest_path = database.with_suffix(".manifest.json")
    require(manifest_path.is_file(), f"missing publication manifest: {manifest_path}")
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"invalid publication manifest: {error}") from error
    require(isinstance(manifest, dict), "publication manifest must be an object")
    require(
        manifest.get("schema_version") == "genoligand_chembl_query_database_v1"
        and manifest.get("status") == "ready"
        and manifest.get("table") == TABLE,
        "publication manifest is not ready",
    )
    require(
        int(manifest.get("database_bytes", 0) or 0) == database.stat().st_size,
        "publication database size does not match its manifest",
    )
    require(valid_sha256(manifest.get("database_sha256")), "publication database digest is invalid")
    actual_digest = sha256_file(database)
    require(
        actual_digest == str(manifest["database_sha256"]).lower(),
        "publication database digest does not match its manifest",
    )
    require(str(manifest.get("source_release", "")), "publication source release is missing")
    require(valid_sha256(manifest.get("source_digest")), "publication source digest is invalid")
    return manifest_path, manifest, actual_digest


def canonical_index(connection: sqlite3.Connection) -> tuple[str, list[dict[str, Any]]]:
    indexes: list[dict[str, Any]] = []
    selected = ""
    for row in connection.execute(f"PRAGMA index_list({quote_identifier(TABLE)})"):
        name = str(row[1])
        columns = [
            str(item[2])
            for item in connection.execute(f"PRAGMA index_info({quote_identifier(name)})")
        ]
        indexes.append({"name": name, "columns": columns})
        if columns[:1] == ["canonical_isomeric_smiles"] and not selected:
            selected = name
    require(selected, "canonical SMILES query index is missing")
    return selected, indexes


def lookup_sql() -> str:
    return (
        f"SELECT {COLUMNS} FROM {TABLE} "
        "WHERE canonical_isomeric_smiles IN (?) AND "
        + ELIGIBLE
    )


def lookup_rows(
    connection: sqlite3.Connection, canonical: str
) -> tuple[list[sqlite3.Row], list[str], float]:
    sql = lookup_sql()
    plan = [
        str(row[-1])
        for row in connection.execute("EXPLAIN QUERY PLAN " + sql, (canonical,))
    ]
    require(
        not any("SCAN GENOLIGAND_ONSITE_ACTIVITY" in row.upper() for row in plan),
        f"runtime lookup performs a full table scan: {plan}",
    )
    require(
        any(
            "SEARCH GENOLIGAND_ONSITE_ACTIVITY" in row.upper()
            and "USING INDEX" in row.upper()
            for row in plan
        ),
        f"runtime lookup does not use the canonical index: {plan}",
    )
    started = time.perf_counter()
    rows = list(connection.execute(sql, (canonical,)))
    elapsed = time.perf_counter() - started
    return rows, plan, elapsed


def sample_payload(row: sqlite3.Row, bound: str) -> dict[str, Any]:
    relation = relation_key(row[7])
    quantitative = relation == "="
    value = float(row[8])
    require(math.isfinite(value), "activity value is not finite")
    units = str(row[9] or "")
    require(units in {"", "nM"}, f"ineligible activity unit reached runtime lookup: {units}")
    target_id = str(row[1] or "").strip()
    source_target_id = str(row[2] or "").strip()
    source_release = str(row[10] or "").strip()
    source_digest = str(row[11] or "").strip()
    require(target_id, "canonical target_id is missing")
    require(source_target_id, "source_target_id is missing")
    require(source_release, "row source release is missing")
    require(valid_sha256(source_digest), "row source digest is invalid")
    return {
        "canonical_isomeric_smiles": str(row[0]),
        "canonical_target_id": target_id,
        "source_target_id": source_target_id,
        "canonical_target_strategy": (
            "component_accession" if target_id != source_target_id else "source_target_fallback"
        ),
        "target_name": str(row[3] or ""),
        "organism": str(row[4] or ""),
        "compound_name": str(row[5] or ""),
        "standard_type": str(row[6]),
        "relation": None if row[7] is None else str(row[7]),
        "bound": bound,
        "standard_value": value,
        "source_units": units,
        "normalized_units": "nM",
        "quantitative": quantitative,
        "score_channel": "exact_potency" if quantitative else "evidence_only",
        "exact_potency_nm": value if quantitative else None,
        "source_release": source_release,
        "source_digest": source_digest,
    }


def audit(science_store: Path, discovery_limit: int = DISCOVERY_LIMIT) -> dict[str, Any]:
    database = science_store / "downloads" / "chembl" / "chembl_query.sqlite"
    chembl_dir = database.parent
    before = file_snapshot(chembl_dir)
    manifest_path, manifest, database_digest = load_publication(database)
    uri = database.resolve().as_uri() + "?mode=ro&immutable=1"
    connection = sqlite3.connect(uri, uri=True)
    connection.row_factory = sqlite3.Row
    try:
        connection.execute("PRAGMA query_only=ON")
        columns = {str(row[1]) for row in connection.execute(f"PRAGMA table_info({TABLE})")}
        required = {
            "canonical_isomeric_smiles",
            "target_id",
            "source_target_id",
            "target_name",
            "organism",
            "compound_name",
            "standard_type",
            "standard_relation",
            "standard_value",
            "standard_units",
            "source_release",
            "source_digest",
        }
        require(required.issubset(columns), "chembl query schema is incomplete")
        index_name, indexes = canonical_index(connection)
        discovery_sql = (
            f"SELECT {COLUMNS} FROM {TABLE} INDEXED BY {quote_identifier(index_name)} "
            "WHERE canonical_isomeric_smiles <> '' AND "
            + ELIGIBLE
            + " ORDER BY canonical_isomeric_smiles LIMIT ?"
        )
        discovery_plan = [
            str(row[-1])
            for row in connection.execute(
                "EXPLAIN QUERY PLAN " + discovery_sql, (discovery_limit,)
            )
        ]
        discovered: dict[str, sqlite3.Row] = {}
        rows_examined = 0
        discovery_started = time.perf_counter()
        for row in connection.execute(discovery_sql, (discovery_limit,)):
            rows_examined += 1
            key = relation_key(row[7])
            if key in RELATIONS and key not in discovered:
                discovered[key] = row
            if len(discovered) == len(RELATIONS):
                break
        discovery_elapsed = time.perf_counter() - discovery_started
        require(rows_examined > 0, "published chembl query database has no eligible activity rows")

        matrix: list[dict[str, Any]] = []
        for relation, bound in RELATIONS.items():
            discovered_row = discovered.get(relation)
            if discovered_row is None:
                matrix.append(
                    {
                        "relation": None if relation == "NULL" else relation,
                        "relation_key": relation,
                        "bound": bound,
                        "status": "unavailable",
                        "reason": f"not_observed_in_first_{discovery_limit}_indexed_rows",
                        "hit_count": 0,
                        "quantitative": relation == "=",
                        "score_channel": "exact_potency" if relation == "=" else "evidence_only",
                        "exact_potency_nm": None,
                        "sample": None,
                        "query_plan": [],
                        "elapsed_seconds": None,
                    }
                )
                continue
            canonical = str(discovered_row[0])
            rows, plan, elapsed = lookup_rows(connection, canonical)
            hits = [row for row in rows if relation_key(row[7]) == relation]
            require(hits, f"indexed lookup lost discovered relation {relation}")
            sample = sample_payload(hits[0], bound)
            require(sample["source_release"] == manifest["source_release"], "row release differs from manifest")
            require(
                str(sample["source_digest"]).lower() == str(manifest["source_digest"]).lower(),
                "row source digest differs from manifest",
            )
            matrix.append(
                {
                    "relation": None if relation == "NULL" else relation,
                    "relation_key": relation,
                    "bound": bound,
                    "status": "available",
                    "hit_count": len(hits),
                    "lookup_row_count": len(rows),
                    "quantitative": relation == "=",
                    "score_channel": "exact_potency" if relation == "=" else "evidence_only",
                    "exact_potency_nm": sample["exact_potency_nm"],
                    "sample": sample,
                    "query_plan": plan,
                    "elapsed_seconds": elapsed,
                }
            )
    finally:
        connection.close()

    after = file_snapshot(chembl_dir)
    unchanged = before == after
    require(unchanged, "chembl directory changed during read-only relation queries")
    unavailable = [row["relation_key"] for row in matrix if row["status"] == "unavailable"]
    archives = sorted(
        name for name in before if name.lower().endswith((".tar.gz", ".tgz", ".zip"))
    )
    return {
        "schema_version": "genoligand_chembl_relation_matrix_v1",
        "status": "ready" if not unavailable else "failed",
        "mode": "live",
        "science_store": str(science_store),
        "database": str(database),
        "database_bytes": database.stat().st_size,
        "database_sha256_declared": str(manifest["database_sha256"]).lower(),
        "database_sha256_actual": database_digest,
        "publication_manifest": str(manifest_path),
        "source_release": str(manifest["source_release"]),
        "source_digest": str(manifest["source_digest"]).lower(),
        "canonical_index": index_name,
        "indexes": indexes,
        "runtime_query_uses_canonical_index": True,
        "runtime_query_uses_archive": False,
        "archive_access": "metadata_snapshot_only",
        "archives_observed": archives,
        "chembl_directory_unchanged": unchanged,
        "archive_extraction_observed": False,
        "discovery": {
            "method": "bounded_canonical_index_walk",
            "limit": discovery_limit,
            "rows_examined": rows_examined,
            "elapsed_seconds": discovery_elapsed,
            "query_plan": discovery_plan,
        },
        "relation_coverage": "complete" if not unavailable else "partial",
        "unavailable_relations": unavailable,
        "matrix": matrix,
    }


def write_fixture(science_store: Path) -> dict[str, str]:
    chembl_dir = science_store / "downloads" / "chembl"
    chembl_dir.mkdir(parents=True, exist_ok=True)
    database = chembl_dir / "chembl_query.sqlite"
    source_digest = hashlib.sha256(b"chembl-relation-matrix-source").hexdigest()
    source_release = "chembl-self-test"
    rows = [
        ("C", "P00918", "CHEMBL_T_EQ", "CA2", "Homo sapiens", "exact", "IC50", "=", 10.0, "nM"),
        ("CC", "P00918", "CHEMBL_T_GT", "CA2", "Homo sapiens", "lower", "Ki", ">", 11.0, ""),
        ("CCC", "P00918", "CHEMBL_T_LT", "CA2", "Homo sapiens", "upper", "Kd", "<", 12.0, "nM"),
        ("CN", "P00918", "CHEMBL_T_APPROX", "CA2", "Homo sapiens", "approx", "EC50", "~", 13.0, ""),
        ("CO", "P00918", "CHEMBL_T_GE", "CA2", "Homo sapiens", "lower inclusive", "Ki", ">=", 14.0, "nM"),
        ("C=C", "P00918", "CHEMBL_T_LE", "CA2", "Homo sapiens", "upper inclusive", "Kd", "<=", 15.0, ""),
        ("C#N", "P00918", "CHEMBL_T_NULL", "CA2", "Homo sapiens", "unknown", "IC50", None, 16.0, "nM"),
        ("CCl", "P00918", "CHEMBL_T_EMPTY_UNIT", "CA2", "Homo sapiens", "empty unit exact", "Ki", "=", 17.0, ""),
        ("N", "P00918", "CHEMBL_T_NON_NM", "CA2", "Homo sapiens", "wrong unit", "Ki", "=", 18.0, "uM"),
        ("O", "P00918", "CHEMBL_T_NULL_UNIT", "CA2", "Homo sapiens", "null unit", "Ki", "=", 19.0, None),
        ("S", "P00918", "CHEMBL_T_NULL_VALUE", "CA2", "Homo sapiens", "null value", "Ki", "=", None, "nM"),
    ]
    connection = sqlite3.connect(database)
    try:
        connection.execute(
            f"""
            CREATE TABLE {TABLE} (
                canonical_isomeric_smiles TEXT NOT NULL,
                target_id TEXT NOT NULL,
                source_target_id TEXT NOT NULL,
                target_name TEXT,
                organism TEXT,
                compound_name TEXT,
                standard_type TEXT,
                standard_relation TEXT,
                standard_value REAL,
                standard_units TEXT,
                source_release TEXT NOT NULL,
                source_digest TEXT NOT NULL
            )
            """
        )
        connection.executemany(
            f"INSERT INTO {TABLE} VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            [row + (source_release, source_digest) for row in rows],
        )
        connection.execute(
            f"CREATE INDEX idx_genoligand_onsite_activity_smiles "
            f"ON {TABLE}(canonical_isomeric_smiles)"
        )
        connection.commit()
    finally:
        connection.close()
    manifest = {
        "schema_version": "genoligand_chembl_query_database_v1",
        "status": "ready",
        "database": str(database),
        "database_bytes": database.stat().st_size,
        "database_sha256": sha256_file(database),
        "rows": len(rows),
        "table": TABLE,
        "canonical_query_column": "canonical_isomeric_smiles",
        "source_target_column": "source_target_id",
        "indexes": ["idx_genoligand_onsite_activity_smiles"],
        "source_release": source_release,
        "source_digest": source_digest,
        "activity_types": ["Ki", "Kd", "IC50", "EC50"],
        "activity_units": ["", "nM"],
    }
    atomic_json(database.with_suffix(".manifest.json"), manifest)
    archive = chembl_dir / "chembl_self_test_sqlite.tar.gz"
    archive.write_bytes(b"archive sentinel: relation audit must not read or extract this file\n")
    return {
        "empty_unit": "CCl",
        "non_nm": "N",
        "null_unit": "O",
        "null_value": "S",
    }


def run_self_test(work_root: Path) -> dict[str, Any]:
    work_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="chembl-relation-matrix-", dir=work_root) as temporary:
        science_store = Path(temporary) / "science_store"
        probes = write_fixture(science_store)
        payload = audit(science_store, discovery_limit=1_000)
        matrix = {row["relation_key"]: row for row in payload["matrix"]}
        require(set(matrix) == set(RELATIONS), "self-test relation matrix is incomplete")
        require(all(row["status"] == "available" for row in matrix.values()), "self-test relation is unavailable")
        require(matrix["="]["quantitative"] is True, "exact relation is not quantitative")
        require(matrix["="]["exact_potency_nm"] is not None, "exact relation lost exact potency")
        for relation in RELATIONS.keys() - {"="}:
            require(matrix[relation]["quantitative"] is False, f"{relation} became quantitative")
            require(matrix[relation]["exact_potency_nm"] is None, f"{relation} became exact potency")
            require(matrix[relation]["score_channel"] == "evidence_only", f"{relation} left evidence channel")
        require(matrix["NULL"]["bound"] == "unknown", "NULL relation semantics changed")
        require(matrix[">"]["bound"] == "lower_exclusive", "> bound direction changed")
        require(matrix["<"]["bound"] == "upper_exclusive", "< bound direction changed")
        require(matrix[">="]["bound"] == "lower_inclusive", ">= bound direction changed")
        require(matrix["<="]["bound"] == "upper_inclusive", "<= bound direction changed")
        require(matrix["~"]["bound"] == "approximate", "approximate semantics changed")
        database = science_store / "downloads" / "chembl" / "chembl_query.sqlite"
        uri = database.resolve().as_uri() + "?mode=ro&immutable=1"
        connection = sqlite3.connect(uri, uri=True)
        connection.row_factory = sqlite3.Row
        try:
            empty_rows, empty_plan, _ = lookup_rows(connection, probes["empty_unit"])
            rejected = {}
            for name in ("non_nm", "null_unit", "null_value"):
                rows, plan, _ = lookup_rows(connection, probes[name])
                rejected[name] = {"rows": len(rows), "query_plan": plan}
                require(not rows, f"{name} row bypassed runtime eligibility filter")
        finally:
            connection.close()
        require(len(empty_rows) == 1, "empty-unit nM activity was rejected")
        empty_sample = sample_payload(empty_rows[0], "exact")
        require(
            empty_sample["quantitative"] is True
            and empty_sample["normalized_units"] == "nM"
            and empty_sample["exact_potency_nm"] == 17.0,
            "empty-unit exact activity was not normalized to nM",
        )
        require(
            all(
                row["sample"]["canonical_target_id"] == "P00918"
                and row["sample"]["source_target_id"].startswith("CHEMBL_T_")
                and row["sample"]["canonical_target_strategy"] == "component_accession"
                for row in matrix.values()
            ),
            "canonical/source target identity semantics changed",
        )
        incomplete_store = Path(temporary) / "incomplete_store"
        write_fixture(incomplete_store)
        incomplete_database = incomplete_store / "downloads" / "chembl" / "chembl_query.sqlite"
        connection = sqlite3.connect(incomplete_database)
        try:
            connection.execute(f"DELETE FROM {TABLE} WHERE standard_relation IS NULL")
            connection.commit()
        finally:
            connection.close()
        incomplete_manifest_path = incomplete_database.with_suffix(".manifest.json")
        incomplete_manifest = json.loads(incomplete_manifest_path.read_text(encoding="utf-8"))
        incomplete_manifest["database_bytes"] = incomplete_database.stat().st_size
        incomplete_manifest["database_sha256"] = sha256_file(incomplete_database)
        atomic_json(incomplete_manifest_path, incomplete_manifest)
        incomplete_evidence = Path(temporary) / "incomplete-evidence.json"
        incomplete_code, incomplete_payload = run_live_audit(
            incomplete_store,
            incomplete_evidence,
        )
        require(incomplete_code != 0, "incomplete live relation coverage returned success")
        require(incomplete_evidence.is_file(), "incomplete live relation coverage wrote no evidence")
        require(
            incomplete_payload["status"] == "failed"
            and incomplete_payload["relation_coverage"] == "partial"
            and incomplete_payload["unavailable_relations"] == ["NULL"],
            "incomplete live relation coverage did not produce strict failure evidence",
        )

        tampered_store = Path(temporary) / "tampered_store"
        write_fixture(tampered_store)
        tampered_database = tampered_store / "downloads" / "chembl" / "chembl_query.sqlite"
        original = tampered_database.read_bytes()
        changed = bytearray(original)
        changed[-1] ^= 1
        tampered_database.write_bytes(changed)
        require(tampered_database.stat().st_size == len(original), "tamper changed database size")
        tampered_evidence = Path(temporary) / "tampered-evidence.json"
        tampered_code, tampered_payload = run_live_audit(tampered_store, tampered_evidence)
        require(tampered_code != 0, "same-size database tamper returned success")
        require(tampered_evidence.is_file(), "same-size database tamper wrote no evidence")
        require(
            tampered_payload["status"] == "failed"
            and "digest does not match" in str(tampered_payload.get("error", "")),
            "same-size database tamper was not rejected by digest",
        )
        payload["mode"] = "self_test"
        payload["self_test"] = {
            "status": "passed",
            "relations_covered": list(RELATIONS),
            "exact_is_quantitative": True,
            "censored_and_unknown_are_evidence_only": True,
            "empty_unit_normalized_to_nm": True,
            "nm_unit_accepted": True,
            "non_nm_unit_rejected": True,
            "null_unit_rejected": True,
            "null_value_rejected": True,
            "canonical_and_source_target_ids_preserved": True,
            "runtime_lookup_uses_canonical_index": True,
            "archive_not_extracted": True,
            "incomplete_live_coverage_is_nonzero_with_failure_evidence": True,
            "same_size_database_tamper_rejected": True,
            "empty_unit_probe": {
                "rows": len(empty_rows),
                "sample": empty_sample,
                "query_plan": empty_plan,
            },
            "rejection_probes": rejected,
        }
        return payload


def run_live_audit(science_store: Path, evidence: Path) -> tuple[int, dict[str, Any]]:
    try:
        payload = audit(science_store)
    except Exception as error:
        payload = {
            "schema_version": "genoligand_chembl_relation_matrix_v1",
            "status": "failed",
            "mode": "live",
            "science_store": str(science_store),
            "relation_coverage": "unknown",
            "unavailable_relations": [],
            "error": str(error),
        }
    complete = (
        payload.get("status") == "ready"
        and payload.get("relation_coverage") == "complete"
        and payload.get("unavailable_relations") == []
    )
    if not complete:
        payload["status"] = "failed"
        payload.setdefault("failure_reason", "live_relation_matrix_incomplete")
    atomic_json(evidence, payload)
    return (0 if complete else 1), payload


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate ChEMBL relation, bound, unit, target identity, and indexed-query semantics."
    )
    parser.add_argument("--science-store", type=Path)
    parser.add_argument("--evidence-out", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require(args.self_test or args.science_store is not None, "--science-store is required in live mode")
    if args.self_test:
        work_root = (Path.cwd() / "work" / "chembl-relation-matrix").resolve()
        payload = run_self_test(work_root)
        evidence = (
            args.evidence_out.resolve()
            if args.evidence_out
            else work_root / "self-test.json"
        )
    else:
        science_store = args.science_store.resolve()
        evidence = (
            args.evidence_out.resolve()
            if args.evidence_out
            else science_store / "manifests" / "chembl_relation_matrix.json"
        )
        return_code, payload = run_live_audit(science_store, evidence)
        message = "PASS" if return_code == 0 else "FAIL"
        print(
            f"chembl_relation_matrix {message} status={payload['status']} "
            f"coverage={payload['relation_coverage']} evidence={evidence}",
            file=sys.stdout if return_code == 0 else sys.stderr,
        )
        return return_code
    atomic_json(evidence, payload)
    print(
        f"chembl_relation_matrix PASS status={payload['status']} "
        f"coverage={payload['relation_coverage']} evidence={evidence}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"chembl_relation_matrix FAIL {error}", file=sys.stderr)
        raise SystemExit(1)
