from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import re
import sqlite3
import sys
import tempfile
import time
import zlib
from collections import deque
from concurrent.futures import ProcessPoolExecutor
from contextlib import contextmanager
from functools import lru_cache
from pathlib import Path
from typing import Any, Iterable, Iterator


SCHEMA_VERSION = "genoligand_ligand_similarity_index_v2"
FINGERPRINT_TYPE = "morgan_ecfp4"
FINGERPRINT_RADIUS = 2
FINGERPRINT_BITS = 2048
FINGERPRINT_INCLUDE_CHIRALITY = True
STANDARDIZATION_METHOD = "rdkit_cleanup_fragment_parent_reionize_canonical_tautomer_v1"
TAUTOMER_MAX_TAUTOMERS = 100
TAUTOMER_MAX_TRANSFORMS = 100
SEARCH_ALGORITHM = "popcount_bitbound_exact_tanimoto"
BITBOUND_FORMULA = "ceil(t*q_popcount) <= db_popcount <= floor(q_popcount/t)"
REFERENCE = {"name": "FPSim2", "license": "MIT", "commit": "e03e669"}
QUERY_FEATURE_SCHEMA_VERSION = "genoligand_query_structure_features_v1"
QUERY_ARTIFACT_SCHEMA_VERSION = "genoligand_query_structure_artifact_v1"
QUERY_METHOD_VERSION = "genoligand_query_structure_prepare_v1"
COMPRESSION_SAMPLE_LIMIT = 50_000
COMPARISON_BENCHMARK_LIMIT = 10_000
MAX_SQL_PARAMETERS = 800
MAX_BUILD_WORKERS = 8
PARALLEL_BUILD_MIN_MOLECULES = 10_000
FINGERPRINT_BATCH_SIZE = 1000
EVIDENCE_BATCH_SIZE = 100_000
BUILD_CACHE_KIB = 512 * 1024
PROGRESS_INTERVAL_SECONDS = 30.0
STRUCTURAL_CANDIDATE_BATCH_SIZE = 200
STRUCTURE_SCORING_METHOD = "rdkit_usr_usrcat_v1"
RUNTIME_PUBLICATION_CACHE_SIZE = 8
CHEMBL_ACTIVITY_PATTERN = re.compile(
    r"^(?:Ki|Kd|IC50|EC50)=([+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?) nM$"
)
KNOWN_DATABASE_HEADER = (
    "input_id\tsource\ttarget_id\ttarget_name\torganism\ttarget_class\tmatch_score\t"
    "matched_features\tevidence\tsource_row_provenance\tactivity_value\tligand_identity\t"
    "current_run_query_id\trecord_id"
)


class LigandSimilarityIndexError(RuntimeError):
    pass


class _Progress:
    def __init__(self, phase: str, total: int) -> None:
        self.phase = phase
        self.total = max(0, int(total))
        self.started = time.monotonic()
        self.last_report = self.started
        self.last_processed = -1

    def update(self, processed: int, *, force: bool = False) -> None:
        now = time.monotonic()
        processed = max(0, int(processed))
        if processed == self.last_processed:
            return
        if not force and processed < self.total and now - self.last_report < PROGRESS_INTERVAL_SECONDS:
            return
        elapsed = max(0.000001, now - self.started)
        print(
            json.dumps(
                {
                    "phase": self.phase,
                    "processed": processed,
                    "total": self.total,
                    "rate": round(processed / elapsed, 3),
                    "elapsed": round(elapsed, 3),
                },
                sort_keys=True,
            ),
            file=sys.stderr,
            flush=True,
        )
        self.last_report = now
        self.last_processed = processed


def build_similarity_index(
    chembl_db: str | Path,
    bindingdb_db: str | Path,
    output_db: str | Path,
    manifest_path: str | Path,
    *,
    chembl_manifest: str | Path,
    bindingdb_manifest: str | Path,
) -> dict[str, Any]:
    """Build and atomically publish the compact replacement query database."""

    chembl_path = Path(chembl_db).resolve()
    bindingdb_path = Path(bindingdb_db).resolve()
    output_path = Path(output_db).resolve()
    manifest_output = Path(manifest_path).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_output.parent.mkdir(parents=True, exist_ok=True)

    chembl_input = _inspect_chembl_input(chembl_path, Path(chembl_manifest).resolve())
    bindingdb_input = _inspect_bindingdb_input(
        bindingdb_path, Path(bindingdb_manifest).resolve()
    )
    input_bytes = chembl_input["bytes"] + bindingdb_input["bytes"]

    staging = _temporary_path(output_path, ".sqlite")
    staging_manifest = _temporary_path(manifest_output, ".json")
    try:
        fingerprint = _build_database(staging, chembl_path, bindingdb_path, chembl_input, bindingdb_input)
        output_bytes = staging.stat().st_size
        if output_bytes >= input_bytes:
            raise LigandSimilarityIndexError(
                f"ligand_similarity_index_not_smaller:{output_bytes}>={input_bytes}"
            )
        database_sha256 = _sha256_file(staging)
        counts = _database_counts(staging)
        manifest = {
            "schema_version": SCHEMA_VERSION,
            "status": "ready",
            "database": {
                "path": str(output_path),
                "bytes": output_bytes,
                "sha256": database_sha256,
            },
            "inputs": {
                "chembl": chembl_input,
                "bindingdb": bindingdb_input,
            },
            "counts": counts,
            "fingerprint": fingerprint,
            "reclaimed_candidate_bytes": input_bytes - output_bytes,
        }
        _write_json_fsync(staging_manifest, manifest)
        _validate_similarity_index(staging, staging_manifest, expected_database=output_path)
        os.replace(staging, output_path)
        os.replace(staging_manifest, manifest_output)
        return manifest
    finally:
        _remove_sqlite_family(staging)
        staging_manifest.unlink(missing_ok=True)


def validate_similarity_index(
    database: str | Path,
    manifest_path: str | Path,
    *,
    verify_hash: bool = True,
) -> dict[str, Any]:
    return _validate_similarity_index(
        Path(database).resolve(),
        Path(manifest_path).resolve(),
        verify_hash=verify_hash,
    )


def query_similarity(
    database: str | Path,
    query_smiles: str,
    *,
    top_k: int = 50,
    threshold: float = 0.5,
    manifest_path: str | Path | None = None,
) -> list[dict[str, Any]]:
    _validate_query_limits(top_k, threshold)
    database_path = Path(database).resolve()
    manifest = _runtime_manifest(database_path, manifest_path)
    query_fp, _canonical = _fingerprint_from_smiles(query_smiles)
    candidates = _exact_candidates(database_path, manifest, query_fp, threshold)
    candidates.sort(key=lambda row: (-row["similarity"], row["canonical_smiles"], row["id"]))
    candidates = candidates[:top_k]
    sources = _candidate_sources(database_path, [row["id"] for row in candidates])
    results: list[dict[str, Any]] = []
    for candidate in candidates:
        for source in sources.get(candidate["id"], []):
            results.append(
                {
                    "source": source,
                    "canonical_smiles": candidate["canonical_smiles"],
                    "similarity": candidate["similarity"],
                }
            )
    results.sort(key=lambda row: (-row["similarity"], row["canonical_smiles"], row["source"]))
    return results[:top_k]


def query_targets(
    database: str | Path,
    query_smiles: str,
    top_k: int = 50,
    threshold: float = 0.5,
    *,
    manifest_path: str | Path | None = None,
    query_feature_digest: str = "",
    query_structure_features: dict[str, Any] | None = None,
    _validated_manifest: dict[str, Any] | None = None,
) -> list[dict[str, Any]]:
    """Return best source/target evidence for every exact BitBound similarity hit."""

    _validate_query_limits(top_k, threshold)
    database_path = Path(database).resolve()
    manifest = _validated_manifest or _runtime_manifest(database_path, manifest_path)
    query_fp, _canonical = _fingerprint_from_smiles(query_smiles)
    candidates = _exact_candidates(database_path, manifest, query_fp, threshold)
    exact_candidate_count = len(candidates)
    candidates.sort(key=lambda row: (-row["similarity"], row["canonical_smiles"], row["id"]))
    if query_structure_features:
        activity_upper_bounds = _candidate_activity_upper_bounds(
            database_path, [candidate["id"] for candidate in candidates]
        )
        candidates = _rank_structural_candidates(candidates, activity_upper_bounds)
        batch_size = STRUCTURAL_CANDIDATE_BATCH_SIZE
    else:
        batch_size = MAX_SQL_PARAMETERS
    by_target: dict[tuple[str, str], dict[str, Any]] = {}
    processed_candidate_count = 0
    with _read_connection(database_path) as connection:
        for batch in _batches(candidates, batch_size):
            if query_structure_features:
                _apply_structure_compatibility(batch, query_structure_features)
            else:
                for candidate in batch:
                    candidate["structural_compatibility"] = candidate["similarity"]
            candidate_by_id = {row["id"]: row for row in batch}
            placeholders = ",".join("?" for _ in batch)
            sql = f"""
                SELECT e.molecule_id, t.source, t.target_id, t.target_name, t.organism,
                       t.target_class, t.source_release, t.source_digest,
                       e.activity_score, e.activity, e.relation_rank, e.numeric_rank,
                       COALESCE(h.ligand_code, ''), COALESCE(h.pdb_ids, ''),
                       COALESCE(h.target_chain_pdb_ids, '')
                FROM evidence AS e
                JOIN target AS t ON t.id = e.target_fk
                LEFT JOIN structure_hint AS h
                  ON h.molecule_id=e.molecule_id AND h.target_fk=e.target_fk
                WHERE e.molecule_id IN ({placeholders})
                ORDER BY e.molecule_id, t.source, t.target_id
            """
            for row in connection.execute(sql, list(candidate_by_id)):
                candidate = candidate_by_id[int(row[0])]
                similarity = float(candidate["similarity"])
                structural_compatibility = float(candidate["structural_compatibility"])
                activity_score = _validated_activity_score(
                    str(row[1]), str(row[9]), row[8]
                )
                if activity_score is None:
                    continue
                result = {
                    "source": str(row[1]),
                    "target_id": str(row[2]),
                    "target_name": str(row[3]),
                    "organism": str(row[4]),
                    "target_class": str(row[5]),
                    "canonical_smiles": candidate["canonical_smiles"],
                    "similarity": similarity,
                    "structural_compatibility": structural_compatibility,
                    "structural_compatibility_method": str(
                        candidate.get("structural_compatibility_method", "fingerprint_only")
                    ),
                    "activity_score": activity_score,
                    "match_score": (
                        0.85 * similarity + 0.15 * structural_compatibility
                    ) * activity_score,
                    "activity": str(row[9]),
                    "source_release": str(row[6]),
                    "source_digest": str(row[7]),
                    "relation_rank": int(row[10]),
                    "numeric_rank": float(row[11]),
                    "ligand_code": str(row[12]),
                    "pdb_ids": str(row[13]),
                    "target_chain_pdb_ids": str(row[14]),
                }
                key = (result["source"], result["target_id"])
                previous = by_target.get(key)
                if previous is None or _target_rank(result) < _target_rank(previous):
                    by_target[key] = result
            processed_candidate_count += len(batch)
            if query_structure_features and _structural_top_k_is_complete(
                by_target,
                top_k,
                (
                    float(candidates[processed_candidate_count]["match_score_upper_bound"])
                    if processed_candidate_count < exact_candidate_count
                    else None
                ),
            ):
                break
    truncated = processed_candidate_count < exact_candidate_count
    publication = _runtime_publication_identity(manifest)
    results = list(by_target.values())
    for result in results:
        result.update(publication)
        result["query_feature_digest"] = query_feature_digest
        result["exact_candidate_count"] = exact_candidate_count
        result["rescored_candidate_count"] = (
            processed_candidate_count if query_structure_features else 0
        )
        result["structural_candidates_truncated"] = truncated
        result["evidence"] = _target_evidence(result)
    results.sort(key=_target_rank)
    for result in results:
        result.pop("relation_rank", None)
        result.pop("numeric_rank", None)
    return results[:top_k]


def explain_candidate_query(
    database: str | Path, query_smiles: str, threshold: float = 0.5
) -> list[str]:
    _validate_query_limits(1, threshold)
    query_fp, _canonical = _fingerprint_from_smiles(query_smiles)
    low, high = _bitbound(query_fp.GetNumOnBits(), threshold)
    with _read_connection(Path(database).resolve()) as connection:
        rows = connection.execute(
            "EXPLAIN QUERY PLAN SELECT id, canonical_smiles, fingerprint "
            "FROM molecule INDEXED BY idx_molecule_popcount "
            "WHERE popcount BETWEEN ? AND ?",
            (low, high),
        ).fetchall()
    return [str(row[-1]) for row in rows]


def _build_database(
    staging: Path,
    chembl_path: Path,
    bindingdb_path: Path,
    chembl_input: dict[str, Any],
    bindingdb_input: dict[str, Any],
) -> dict[str, Any]:
    connection = sqlite3.connect(staging, uri=True)
    try:
        connection.create_function("merge_pdb_hints", 2, _merge_pdb_hints, deterministic=True)
        connection.create_function("merge_ligand_codes", 2, _merge_ligand_codes, deterministic=True)
        connection.create_function("normalize_pdb_hints", 1, lambda value: _merge_pdb_hints("", value), deterministic=True)
        connection.create_function("normalize_ligand_codes", 1, lambda value: _merge_ligand_codes("", value), deterministic=True)
        _configure_build_connection(connection)
        connection.executescript(
            """
            CREATE TABLE metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            ) WITHOUT ROWID;
            CREATE TABLE molecule (
                id INTEGER PRIMARY KEY,
                canonical_smiles TEXT NOT NULL UNIQUE,
                fingerprint BLOB NOT NULL,
                popcount INTEGER NOT NULL CHECK(popcount BETWEEN 1 AND 2048)
            );
            CREATE TABLE target (
                id INTEGER PRIMARY KEY,
                source TEXT NOT NULL,
                target_id TEXT NOT NULL,
                source_target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                organism TEXT NOT NULL,
                target_class TEXT NOT NULL,
                source_release TEXT NOT NULL,
                source_digest TEXT NOT NULL,
                UNIQUE(source, target_id)
            );
            CREATE TABLE evidence (
                molecule_id INTEGER NOT NULL REFERENCES molecule(id),
                target_fk INTEGER NOT NULL REFERENCES target(id),
                activity_score REAL NOT NULL CHECK(activity_score BETWEEN 0.0 AND 1.0),
                activity TEXT NOT NULL,
                relation_rank INTEGER NOT NULL,
                numeric_rank REAL NOT NULL,
                PRIMARY KEY(molecule_id, target_fk)
            ) WITHOUT ROWID;
            CREATE TABLE structure_hint (
                molecule_id INTEGER NOT NULL REFERENCES molecule(id),
                target_fk INTEGER NOT NULL REFERENCES target(id),
                ligand_code TEXT NOT NULL,
                pdb_ids TEXT NOT NULL,
                target_chain_pdb_ids TEXT NOT NULL,
                PRIMARY KEY(molecule_id, target_fk)
            ) WITHOUT ROWID;
            CREATE TABLE _build_raw_smiles (raw_smiles TEXT PRIMARY KEY) WITHOUT ROWID;
            CREATE TABLE _build_smiles_map (
                raw_smiles TEXT PRIMARY KEY,
                canonical_smiles TEXT NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX _build_smiles_map_canonical ON _build_smiles_map(canonical_smiles);
            """
        )
        connection.execute("ATTACH DATABASE ? AS chembl", (_read_uri(chembl_path),))
        connection.execute("ATTACH DATABASE ? AS bindingdb", (_read_uri(bindingdb_path),))
        connection.execute(
            "INSERT OR IGNORE INTO _build_raw_smiles "
            "SELECT canonical_isomeric_smiles FROM chembl.genoligand_onsite_activity "
            "WHERE canonical_isomeric_smiles IS NOT NULL AND TRIM(canonical_isomeric_smiles) <> ''"
        )
        connection.execute(
            "INSERT OR IGNORE INTO _build_raw_smiles "
            "SELECT canonical_smiles FROM bindingdb.genoligand_bindingdb_activity "
            "WHERE canonical_smiles IS NOT NULL AND TRIM(canonical_smiles) <> ''"
        )
        connection.commit()

        fingerprint = _fingerprint_benchmark(connection)
        fingerprint["build_workers"] = _populate_molecules(
            connection, fingerprint["encoding"]
        )
        _populate_targets(connection, chembl_input, bindingdb_input)
        _populate_evidence(connection)

        connection.executescript(
            """
            DROP INDEX _build_smiles_map_canonical;
            DROP TABLE _build_smiles_map;
            DROP TABLE _build_raw_smiles;
            CREATE INDEX idx_molecule_popcount ON molecule(popcount, id);
            ANALYZE main;
            """
        )
        counts = _connection_counts(connection)
        if min(counts[key] for key in ("molecules", "targets", "evidence")) <= 0:
            raise LigandSimilarityIndexError(f"ligand_similarity_index_empty:{counts}")
        identity = {
            "chembl": {
                "source_digest": chembl_input["source_digest"],
                "source_release": chembl_input["source_release"],
            },
            "bindingdb": {
                "source_digest": bindingdb_input["source_digest"],
                "source_release": bindingdb_input["source_release"],
            },
        }
        metadata = {
            "schema_version": SCHEMA_VERSION,
            "status": "ready",
            "counts": json.dumps(counts, sort_keys=True, separators=(",", ":")),
            "fingerprint": json.dumps(
                _stable_fingerprint_metadata(fingerprint), sort_keys=True, separators=(",", ":")
            ),
            "source_identity": json.dumps(identity, sort_keys=True, separators=(",", ":")),
        }
        connection.executemany(
            "INSERT INTO metadata(key, value) VALUES (?, ?)", sorted(metadata.items())
        )
        connection.commit()
        foreign_errors = connection.execute("PRAGMA foreign_key_check").fetchone()
        if foreign_errors is not None:
            raise LigandSimilarityIndexError("ligand_similarity_foreign_key_check_failed")
        if connection.execute("PRAGMA quick_check").fetchone() != ("ok",):
            raise LigandSimilarityIndexError("ligand_similarity_quick_check_failed")
        connection.execute("DETACH DATABASE chembl")
        connection.execute("DETACH DATABASE bindingdb")
        connection.execute("VACUUM")
        connection.commit()
        return fingerprint
    except sqlite3.Error as exc:
        raise LigandSimilarityIndexError(f"ligand_similarity_build_failed:{_safe_error(exc)}") from exc
    finally:
        connection.close()


def _configure_build_connection(connection: sqlite3.Connection) -> None:
    connection.execute("PRAGMA page_size=4096")
    connection.execute("PRAGMA journal_mode=OFF")
    connection.execute("PRAGMA synchronous=OFF")
    connection.execute("PRAGMA temp_store=FILE")
    connection.execute(f"PRAGMA cache_size=-{BUILD_CACHE_KIB}")
    connection.execute("PRAGMA foreign_keys=ON")


def _fingerprint_benchmark(connection: sqlite3.Connection) -> dict[str, Any]:
    Chem, DataStructs, _rdBase, generator = _rdkit()
    count = int(connection.execute("SELECT COUNT(*) FROM _build_raw_smiles").fetchone()[0])
    stride = max(1, count // COMPRESSION_SAMPLE_LIMIT)
    raw_blobs: list[bytes] = []
    compressed_blobs: list[bytes] = []
    for index, (smiles,) in enumerate(
        connection.execute("SELECT raw_smiles FROM _build_raw_smiles ORDER BY raw_smiles")
    ):
        if index % stride:
            continue
        molecule = Chem.MolFromSmiles(str(smiles).strip())
        if molecule is None:
            continue
        raw = bytes(DataStructs.BitVectToBinaryText(generator.GetFingerprint(molecule)))
        raw_blobs.append(raw)
        compressed_blobs.append(zlib.compress(raw, level=1))
        if len(raw_blobs) >= COMPRESSION_SAMPLE_LIMIT:
            break
    if not raw_blobs:
        raise LigandSimilarityIndexError("ligand_similarity_no_valid_smiles")
    benchmark_count = min(len(raw_blobs), COMPARISON_BENCHMARK_LIMIT)
    query_fp = DataStructs.CreateFromBinaryText(raw_blobs[0])
    started = time.perf_counter()
    for raw in raw_blobs[:benchmark_count]:
        DataStructs.TanimotoSimilarity(query_fp, DataStructs.CreateFromBinaryText(raw))
    raw_seconds = time.perf_counter() - started
    started = time.perf_counter()
    for packed in compressed_blobs[:benchmark_count]:
        restored = zlib.decompress(packed)
        DataStructs.TanimotoSimilarity(query_fp, DataStructs.CreateFromBinaryText(restored))
    zlib_seconds = time.perf_counter() - started
    raw_average = sum(map(len, raw_blobs)) / len(raw_blobs)
    zlib_average = sum(map(len, compressed_blobs)) / len(compressed_blobs)
    savings = 1.0 - zlib_average / raw_average
    compression_acceptable = zlib_seconds <= max(2.0, raw_seconds * 3.0)
    encoding = "zlib1_rdkit_binary_text" if savings >= 0.25 and compression_acceptable else "rdkit_binary_text"
    return {
        "type": FINGERPRINT_TYPE,
        "radius": FINGERPRINT_RADIUS,
        "bits": FINGERPRINT_BITS,
        "include_chirality": FINGERPRINT_INCLUDE_CHIRALITY,
        "standardization": STANDARDIZATION_METHOD,
        "tautomer_max_tautomers": TAUTOMER_MAX_TAUTOMERS,
        "tautomer_max_transforms": TAUTOMER_MAX_TRANSFORMS,
        "lsh_bands": 0,
        "search_algorithm": SEARCH_ALGORITHM,
        "bitbound_formula": BITBOUND_FORMULA,
        "reference": REFERENCE,
        "encoding": encoding,
        "encoding_sample_size": len(raw_blobs),
        "raw_average_bytes": round(raw_average, 4),
        "zlib_level_1_average_bytes": round(zlib_average, 4),
        "compression_savings_fraction": round(savings, 6),
        "comparison_benchmark_count": benchmark_count,
        "raw_comparison_seconds": round(raw_seconds, 6),
        "zlib_level_1_comparison_seconds": round(zlib_seconds, 6),
        "selection_reason": (
            "zlib_level_1_saved_at_least_25_percent_and_10k_comparisons_were_acceptable"
            if encoding.startswith("zlib")
            else "raw_selected_because_compression_size_or_comparison_gate_failed"
        ),
    }


def _populate_molecules(connection: sqlite3.Connection, encoding: str) -> int:
    total = int(connection.execute("SELECT COUNT(*) FROM _build_raw_smiles").fetchone()[0])
    progress = _Progress("fingerprint", total)
    workers = min(MAX_BUILD_WORKERS, os.cpu_count() or 1) if total >= PARALLEL_BUILD_MIN_MOLECULES else 1
    if workers == 1:
        _initialize_fingerprint_worker()
        results = map(
            lambda batch: _fingerprint_smiles_batch(batch, encoding),
            _raw_smiles_batches(connection),
        )
        _store_fingerprint_batches(connection, results, progress)
        return 1
    with ProcessPoolExecutor(
        max_workers=workers,
        initializer=_initialize_fingerprint_worker,
    ) as executor:
        batches = iter(_raw_smiles_batches(connection))
        pending = deque()
        for _ in range(workers * 2):
            batch = next(batches, None)
            if batch is None:
                break
            pending.append(
                (batch[0], batch[-1], executor.submit(_fingerprint_smiles_batch, batch, encoding))
            )
        processed = 0
        while pending:
            first, last, future = pending.popleft()
            try:
                result = future.result()
            except Exception as exc:
                raise LigandSimilarityIndexError(
                    f"ligand_similarity_fingerprint_batch_failed:{first}:{last}:{_safe_error(exc)}"
                ) from exc
            _store_fingerprint_batch(connection, result)
            processed += result[2]
            progress.update(processed)
            batch = next(batches, None)
            if batch is not None:
                pending.append(
                    (
                        batch[0],
                        batch[-1],
                        executor.submit(_fingerprint_smiles_batch, batch, encoding),
                    )
                )
    progress.update(total, force=True)
    return workers


_FINGERPRINT_WORKER_STATE: tuple[Any, Any, Any, Any] | None = None


def _initialize_fingerprint_worker() -> None:
    global _FINGERPRINT_WORKER_STATE
    Chem, DataStructs, _rdBase, generator = _rdkit()
    _FINGERPRINT_WORKER_STATE = (Chem, DataStructs, generator, _standardization_tools())


def _fingerprint_smiles_batch(
    raw_smiles_values: list[str], encoding: str
) -> tuple[list[tuple[str, bytes, int]], list[tuple[str, str]], int]:
    if _FINGERPRINT_WORKER_STATE is None:
        _initialize_fingerprint_worker()
    assert _FINGERPRINT_WORKER_STATE is not None
    Chem, DataStructs, generator, standardization = _FINGERPRINT_WORKER_STATE
    molecules: list[tuple[str, bytes, int]] = []
    mappings: list[tuple[str, str]] = []
    for raw_smiles in raw_smiles_values:
        molecule = _standardized_smiles_molecule(raw_smiles, Chem, standardization)
        if molecule is None:
            continue
        canonical = str(Chem.MolToSmiles(molecule, isomericSmiles=True))
        fingerprint = generator.GetFingerprint(molecule)
        raw = bytes(DataStructs.BitVectToBinaryText(fingerprint))
        packed = zlib.compress(raw, level=1) if encoding.startswith("zlib") else raw
        molecules.append((canonical, packed, int(fingerprint.GetNumOnBits())))
        mappings.append((raw_smiles, canonical))
    return molecules, mappings, len(raw_smiles_values)


def _raw_smiles_batches(connection: sqlite3.Connection) -> Iterable[list[str]]:
    last_smiles = ""
    while True:
        rows = connection.execute(
            "SELECT raw_smiles FROM _build_raw_smiles WHERE raw_smiles > ? "
            "ORDER BY raw_smiles LIMIT ?",
            (last_smiles, FINGERPRINT_BATCH_SIZE),
        ).fetchall()
        if not rows:
            break
        values = [str(row[0]) for row in rows]
        last_smiles = values[-1]
        yield values


def _store_fingerprint_batches(
    connection: sqlite3.Connection,
    results: Iterable[tuple[list[tuple[str, bytes, int]], list[tuple[str, str]], int]],
    progress: _Progress,
) -> None:
    processed = 0
    for result in results:
        _store_fingerprint_batch(connection, result)
        processed += result[2]
        progress.update(processed)
    progress.update(processed, force=True)


def _store_fingerprint_batch(
    connection: sqlite3.Connection,
    result: tuple[list[tuple[str, bytes, int]], list[tuple[str, str]], int],
) -> None:
    molecules, mappings, _processed = result
    connection.executemany(
        "INSERT OR IGNORE INTO molecule(canonical_smiles, fingerprint, popcount) VALUES (?, ?, ?)",
        [(canonical, sqlite3.Binary(blob), popcount) for canonical, blob, popcount in molecules],
    )
    connection.executemany(
        "INSERT INTO _build_smiles_map(raw_smiles, canonical_smiles) VALUES (?, ?)",
        mappings,
    )
    connection.commit()


def _populate_targets(
    connection: sqlite3.Connection,
    chembl_input: dict[str, Any],
    bindingdb_input: dict[str, Any],
) -> None:
    connection.execute(
        """
        INSERT INTO target(
            source, target_id, source_target_id, target_name, organism, target_class,
            source_release, source_digest
        )
        SELECT 'ChEMBL', target_id, MIN(source_target_id),
               COALESCE(MIN(NULLIF(TRIM(target_name), '')), target_id),
               COALESCE(MIN(NULLIF(TRIM(organism), '')), 'unknown'),
               CASE WHEN LOWER(COALESCE(MIN(NULLIF(TRIM(organism), '')), '')) LIKE '%homo sapiens%'
                         OR LOWER(COALESCE(MIN(NULLIF(TRIM(organism), '')), '')) LIKE '%human%'
                    THEN 'known_human_protein' ELSE 'known_protein' END,
               ?, ?
        FROM chembl.genoligand_onsite_activity
        WHERE target_id IS NOT NULL AND TRIM(target_id) <> ''
        GROUP BY target_id
        ORDER BY target_id
        """,
        (chembl_input["source_release"], chembl_input["source_digest"]),
    )
    connection.execute(
        """
        INSERT INTO target(
            source, target_id, source_target_id, target_name, organism, target_class,
            source_release, source_digest
        )
        SELECT 'BindingDB', target_id, target_id,
               COALESCE(MIN(NULLIF(TRIM(target_name), '')), target_id),
               COALESCE(MIN(NULLIF(TRIM(organism), '')), 'unknown'),
               CASE WHEN LOWER(COALESCE(MIN(NULLIF(TRIM(organism), '')), '')) LIKE '%homo sapiens%'
                         OR LOWER(COALESCE(MIN(NULLIF(TRIM(organism), '')), '')) LIKE '%human%'
                    THEN 'known_human_protein' ELSE 'known_protein' END,
               ?, ?
        FROM bindingdb.genoligand_bindingdb_activity
        WHERE target_id IS NOT NULL AND TRIM(target_id) <> ''
        GROUP BY target_id
        ORDER BY target_id
        """,
        (bindingdb_input["source_release"], bindingdb_input["source_digest"]),
    )
    connection.commit()


def _populate_evidence(connection: sqlite3.Connection) -> None:
    better = """
        excluded.relation_rank < evidence.relation_rank
        OR (excluded.relation_rank = evidence.relation_rank
            AND excluded.numeric_rank < evidence.numeric_rank)
        OR (excluded.relation_rank = evidence.relation_rank
            AND excluded.numeric_rank = evidence.numeric_rank
            AND excluded.activity_score > evidence.activity_score)
        OR (excluded.relation_rank = evidence.relation_rank
            AND excluded.numeric_rank = evidence.numeric_rank
            AND excluded.activity_score = evidence.activity_score
            AND excluded.activity < evidence.activity)
    """
    upsert_tail = f"""
        ON CONFLICT(molecule_id, target_fk) DO UPDATE SET
            activity_score=excluded.activity_score,
            activity=excluded.activity,
            relation_rank=excluded.relation_rank,
            numeric_rank=excluded.numeric_rank
        WHERE {better}
    """
    chembl_sql = """
        INSERT INTO evidence(
            molecule_id, target_fk, activity_score, activity, relation_rank, numeric_rank
        )
        SELECT m.id, t.id,
               CASE WHEN TRIM(COALESCE(a.standard_relation, '')) NOT IN ('=', '~') THEN 0.0
                    WHEN CAST(a.standard_value AS REAL) <= 0.01 THEN 1.0
                    WHEN CAST(a.standard_value AS REAL) <= 0.1 THEN 0.98
                    WHEN CAST(a.standard_value AS REAL) <= 1 THEN 0.95
                    WHEN CAST(a.standard_value AS REAL) <= 10 THEN 0.90
                    WHEN CAST(a.standard_value AS REAL) <= 100 THEN 0.85
                    WHEN CAST(a.standard_value AS REAL) <= 1000 THEN 0.75
                    WHEN CAST(a.standard_value AS REAL) <= 10000 THEN 0.60
                    WHEN CAST(a.standard_value AS REAL) <= 100000 THEN 0.45
                    ELSE 0.30 END,
               TRIM(a.standard_type || COALESCE(a.standard_relation, '?') ||
                    CAST(a.standard_value AS TEXT) || ' ' || COALESCE(a.standard_units, '')),
               CASE WHEN TRIM(COALESCE(a.standard_relation, '')) IN ('=', '~') THEN 0
                    WHEN TRIM(COALESCE(a.standard_relation, '')) IN ('<', '<=') THEN 1
                    WHEN TRIM(COALESCE(a.standard_relation, '')) IN ('>', '>=') THEN 2
                    ELSE 3 END,
               CASE WHEN TRIM(COALESCE(a.standard_relation, '')) IN ('=', '~')
                    THEN CAST(a.standard_value AS REAL) ELSE 1.0e300 END
        FROM chembl.genoligand_onsite_activity AS a
        JOIN _build_smiles_map AS sm ON sm.raw_smiles = a.canonical_isomeric_smiles
        JOIN molecule AS m ON m.canonical_smiles = sm.canonical_smiles
        JOIN target AS t ON t.source = 'ChEMBL' AND t.target_id = a.target_id
        WHERE a.rowid > ? AND a.rowid <= ?
          AND TRIM(COALESCE(a.standard_relation, '')) IN ('=', '~', '<', '>', '<=', '>=')
          AND LOWER(TRIM(COALESCE(a.standard_units, ''))) = 'nm'
          AND TYPEOF(a.standard_value) IN ('integer', 'real')
          AND CAST(a.standard_value AS REAL) > 0.0
          AND CAST(a.standard_value AS REAL) < 1.0e308
        """ + upsert_tail
    bindingdb_sql = """
        INSERT INTO evidence(
            molecule_id, target_fk, activity_score, activity, relation_rank, numeric_rank
        )
        SELECT m.id, t.id, a.activity_score,
               TRIM(
                   a.activity_type ||
                   CASE WHEN LTRIM(COALESCE(a.activity, '')) GLOB '[<>=~]*'
                        THEN '' ELSE COALESCE(a.relation, '') END ||
                   COALESCE(a.activity, '')
               ),
               a.relation_rank, a.numeric_rank
        FROM bindingdb.genoligand_bindingdb_activity AS a
        JOIN _build_smiles_map AS sm ON sm.raw_smiles = a.canonical_smiles
        JOIN molecule AS m ON m.canonical_smiles = sm.canonical_smiles
        JOIN target AS t ON t.source = 'BindingDB' AND t.target_id = a.target_id
        WHERE a.source_row_id > ? AND a.source_row_id <= ?
        """ + upsert_tail
    hint_sql = """
        INSERT INTO structure_hint(
            molecule_id, target_fk, ligand_code, pdb_ids, target_chain_pdb_ids
        )
        SELECT m.id, t.id, normalize_ligand_codes(a.ligand_code),
               normalize_pdb_hints(a.pdb_ids), normalize_pdb_hints(a.target_chain_pdb_ids)
        FROM bindingdb.genoligand_bindingdb_activity AS a
        JOIN _build_smiles_map AS sm ON sm.raw_smiles = a.canonical_smiles
        JOIN molecule AS m ON m.canonical_smiles = sm.canonical_smiles
        JOIN target AS t ON t.source = 'BindingDB' AND t.target_id = a.target_id
        WHERE (
               TRIM(COALESCE(a.ligand_code, '')) <> ''
            OR TRIM(COALESCE(a.pdb_ids, '')) <> ''
            OR TRIM(COALESCE(a.target_chain_pdb_ids, '')) <> ''
        ) AND (
               normalize_ligand_codes(a.ligand_code) <> ''
            OR normalize_pdb_hints(a.pdb_ids) <> ''
            OR normalize_pdb_hints(a.target_chain_pdb_ids) <> ''
        ) AND a.source_row_id > ? AND a.source_row_id <= ?
        ON CONFLICT(molecule_id, target_fk) DO UPDATE SET
            ligand_code=merge_ligand_codes(structure_hint.ligand_code, excluded.ligand_code),
            pdb_ids=merge_pdb_hints(structure_hint.pdb_ids, excluded.pdb_ids),
            target_chain_pdb_ids=merge_pdb_hints(
                structure_hint.target_chain_pdb_ids, excluded.target_chain_pdb_ids
            )
        """
    chembl_max = int(
        connection.execute(
            "SELECT COALESCE(MAX(rowid), 0) FROM chembl.genoligand_onsite_activity"
        ).fetchone()[0]
    )
    bindingdb_max = int(
        connection.execute(
            "SELECT COALESCE(MAX(source_row_id), 0) "
            "FROM bindingdb.genoligand_bindingdb_activity"
        ).fetchone()[0]
    )
    total = chembl_max + bindingdb_max * 2
    progress = _Progress("evidence", total)
    processed = 0
    for sql, maximum in (
        (chembl_sql, chembl_max),
        (bindingdb_sql, bindingdb_max),
        (hint_sql, bindingdb_max),
    ):
        for low, high in _integer_batches(maximum, EVIDENCE_BATCH_SIZE):
            connection.execute(sql, (low, high))
            connection.commit()
            processed += high - low
            progress.update(processed)
    progress.update(processed, force=True)


def _inspect_chembl_input(database: Path, manifest_path: Path) -> dict[str, Any]:
    payload = _validated_input_manifest(database, manifest_path)
    required = {
        "canonical_isomeric_smiles",
        "target_id",
        "source_target_id",
        "target_name",
        "organism",
        "standard_type",
        "standard_relation",
        "standard_value",
        "standard_units",
        "source_release",
        "source_digest",
    }
    with _read_connection(database) as connection:
        columns = _columns(connection, "genoligand_onsite_activity")
        if not required.issubset(columns):
            raise LigandSimilarityIndexError("chembl_similarity_source_schema_invalid")
        _require_leading_index(connection, "genoligand_onsite_activity", "canonical_isomeric_smiles")
        identities = connection.execute(
            "SELECT source_release, source_digest FROM genoligand_onsite_activity "
            "GROUP BY source_release, source_digest LIMIT 2"
        ).fetchall()
    if len(identities) != 1:
        raise LigandSimilarityIndexError("chembl_similarity_source_identity_ambiguous")
    source_release, source_digest = map(str, identities[0])
    _require_source_identity(payload, source_release, source_digest, "chembl")
    return _input_record(database, manifest_path, payload, source_release, source_digest)


def _inspect_bindingdb_input(database: Path, manifest_path: Path) -> dict[str, Any]:
    payload = _validated_input_manifest(database, manifest_path)
    required = {
        "canonical_smiles",
        "target_id",
        "target_name",
        "organism",
        "activity",
        "activity_type",
        "relation",
        "numeric_rank",
        "relation_rank",
        "activity_score",
        "ligand_code",
        "pdb_ids",
        "target_chain_pdb_ids",
    }
    with _read_connection(database) as connection:
        columns = _columns(connection, "genoligand_bindingdb_activity")
        if not required.issubset(columns):
            raise LigandSimilarityIndexError("bindingdb_similarity_source_schema_invalid")
        _require_leading_index(connection, "genoligand_bindingdb_activity", "canonical_smiles")
        metadata = dict(
            connection.execute("SELECT key, value FROM genoligand_bindingdb_metadata")
        )
    source_release = str(metadata.get("source_release", ""))
    source_digest = str(metadata.get("source_digest", ""))
    _require_source_identity(payload, source_release, source_digest, "bindingdb")
    return _input_record(database, manifest_path, payload, source_release, source_digest)


def _validated_input_manifest(database: Path, manifest_path: Path) -> dict[str, Any]:
    if not database.is_file() or not manifest_path.is_file():
        raise LigandSimilarityIndexError(f"similarity_source_missing:{database}")
    payload = _read_json(manifest_path)
    size = database.stat().st_size
    digest = _sha256_file(database)
    if payload.get("database_bytes") != size or payload.get("database_sha256") != digest:
        raise LigandSimilarityIndexError(f"similarity_source_manifest_mismatch:{database}")
    return payload


def _require_source_identity(
    payload: dict[str, Any], source_release: str, source_digest: str, source: str
) -> None:
    if (
        not source_release
        or len(source_digest) != 64
        or payload.get("source_release") != source_release
        or payload.get("source_digest") != source_digest
    ):
        raise LigandSimilarityIndexError(f"{source}_similarity_source_identity_invalid")


def _input_record(
    database: Path,
    manifest_path: Path,
    payload: dict[str, Any],
    source_release: str,
    source_digest: str,
) -> dict[str, Any]:
    return {
        "path": str(database),
        "bytes": database.stat().st_size,
        "sha256": str(payload["database_sha256"]),
        "source_digest": source_digest,
        "source_release": source_release,
        "manifest_path": str(manifest_path),
    }


def _validate_similarity_index(
    database: Path,
    manifest_path: Path,
    *,
    verify_hash: bool = True,
    expected_database: Path | None = None,
) -> dict[str, Any]:
    if not database.is_file() or not manifest_path.is_file():
        raise LigandSimilarityIndexError("ligand_similarity_publication_missing")
    manifest = _read_json(manifest_path)
    if manifest.get("schema_version") != SCHEMA_VERSION or manifest.get("status") != "ready":
        raise LigandSimilarityIndexError("ligand_similarity_manifest_schema_invalid")
    database_meta = manifest.get("database")
    counts = manifest.get("counts")
    fingerprint = manifest.get("fingerprint")
    inputs = manifest.get("inputs")
    if not all(isinstance(item, dict) for item in (database_meta, counts, fingerprint, inputs)):
        raise LigandSimilarityIndexError("ligand_similarity_manifest_fields_invalid")
    expected_path = expected_database or database
    if Path(str(database_meta.get("path", ""))).resolve() != expected_path.resolve():
        raise LigandSimilarityIndexError("ligand_similarity_manifest_database_path_invalid")
    if database_meta.get("bytes") != database.stat().st_size:
        raise LigandSimilarityIndexError("ligand_similarity_database_size_invalid")
    if not _valid_retained_source_identity(inputs):
        raise LigandSimilarityIndexError("ligand_similarity_retained_source_identity_invalid")
    input_bytes = sum(int(inputs[source].get("bytes", 0)) for source in ("chembl", "bindingdb"))
    reclaimed = input_bytes - int(database_meta.get("bytes", 0))
    if reclaimed <= 0 or manifest.get("reclaimed_candidate_bytes") != reclaimed:
        raise LigandSimilarityIndexError("ligand_similarity_size_reduction_invalid")
    if not _is_sha256(database_meta.get("sha256")):
        raise LigandSimilarityIndexError("ligand_similarity_database_digest_invalid")
    if verify_hash and database_meta.get("sha256") != _sha256_file(database):
        raise LigandSimilarityIndexError("ligand_similarity_database_hash_invalid")
    if not _valid_fingerprint_manifest(fingerprint):
        raise LigandSimilarityIndexError("ligand_similarity_fingerprint_metadata_invalid")
    try:
        with _read_connection(database) as connection:
            objects = {
                (str(row[0]), str(row[1]))
                for row in connection.execute(
                    "SELECT type, name FROM sqlite_master WHERE type IN ('table', 'index')"
                )
            }
            required_objects = {
                ("table", "molecule"),
                ("table", "target"),
                ("table", "evidence"),
                ("table", "structure_hint"),
                ("table", "metadata"),
                ("index", "idx_molecule_popcount"),
            }
            if not required_objects.issubset(objects):
                raise LigandSimilarityIndexError("ligand_similarity_database_objects_missing")
            if not _compact_schema_valid(connection):
                raise LigandSimilarityIndexError("ligand_similarity_database_columns_invalid")
            if any(name.startswith("_build_") for kind, name in objects if kind == "table"):
                raise LigandSimilarityIndexError("ligand_similarity_build_tables_retained")
            metadata = dict(connection.execute("SELECT key, value FROM metadata"))
            if metadata.get("schema_version") != SCHEMA_VERSION or metadata.get("status") != "ready":
                raise LigandSimilarityIndexError("ligand_similarity_database_metadata_invalid")
            actual_counts = _connection_counts(connection)
            if actual_counts != counts or min(
                actual_counts[key] for key in ("molecules", "targets", "evidence")
            ) <= 0:
                raise LigandSimilarityIndexError("ligand_similarity_database_counts_invalid")
            if json.loads(metadata.get("counts", "{}")) != counts:
                raise LigandSimilarityIndexError("ligand_similarity_metadata_counts_invalid")
            if json.loads(metadata.get("fingerprint", "{}")) != _stable_fingerprint_metadata(fingerprint):
                raise LigandSimilarityIndexError("ligand_similarity_metadata_fingerprint_invalid")
            expected_identity = {
                source: {
                    "source_digest": inputs[source]["source_digest"],
                    "source_release": inputs[source]["source_release"],
                }
                for source in ("chembl", "bindingdb")
            }
            if json.loads(metadata.get("source_identity", "{}")) != expected_identity:
                raise LigandSimilarityIndexError("ligand_similarity_metadata_source_identity_invalid")
            plan = connection.execute(
                "EXPLAIN QUERY PLAN SELECT id FROM molecule INDEXED BY idx_molecule_popcount "
                "WHERE popcount BETWEEN ? AND ?",
                (1, FINGERPRINT_BITS),
            ).fetchall()
            if not any("IDX_MOLECULE_POPCOUNT" in str(row[-1]).upper() for row in plan):
                raise LigandSimilarityIndexError("ligand_similarity_popcount_index_unused")
            if connection.execute("PRAGMA foreign_key_check").fetchone() is not None:
                raise LigandSimilarityIndexError("ligand_similarity_foreign_key_check_failed")
            if connection.execute("PRAGMA quick_check").fetchone() != ("ok",):
                raise LigandSimilarityIndexError("ligand_similarity_quick_check_failed")
    except sqlite3.Error as exc:
        raise LigandSimilarityIndexError(
            f"ligand_similarity_database_corrupt:{_safe_error(exc)}"
        ) from exc
    return manifest


def _runtime_manifest(database: Path, manifest_path: str | Path | None) -> dict[str, Any]:
    path = Path(manifest_path).resolve() if manifest_path else database.with_suffix(".manifest.json")
    if not database.is_file() or not path.is_file():
        raise LigandSimilarityIndexError("ligand_similarity_publication_missing")
    database_stat = database.stat()
    manifest_stat = path.stat()
    manifest_bytes, database_sha256, manifest_sha256 = _cached_runtime_publication(
        str(database),
        database_stat.st_size,
        database_stat.st_mtime_ns,
        str(path),
        manifest_stat.st_size,
        manifest_stat.st_mtime_ns,
    )
    manifest = _read_json_bytes(manifest_bytes, path)
    database_meta = manifest.get("database")
    fingerprint = manifest.get("fingerprint")
    inputs = manifest.get("inputs")
    input_bytes = 0
    if isinstance(inputs, dict) and all(
        isinstance(inputs.get(source), dict) for source in ("chembl", "bindingdb")
    ):
        input_bytes = sum(
            int(inputs[source].get("bytes", 0) or 0) for source in ("chembl", "bindingdb")
        )
    if (
        manifest.get("schema_version") != SCHEMA_VERSION
        or manifest.get("status") != "ready"
        or not isinstance(database_meta, dict)
        or not isinstance(fingerprint, dict)
        or not isinstance(inputs, dict)
        or Path(str(database_meta.get("path", ""))).resolve() != database
        or database_meta.get("bytes") != database.stat().st_size
        or not _is_sha256(database_meta.get("sha256"))
        or manifest.get("reclaimed_candidate_bytes")
        != input_bytes - int(database_meta.get("bytes", 0) or 0)
        or input_bytes <= int(database_meta.get("bytes", 0) or 0)
        or not _valid_fingerprint_manifest(fingerprint)
        or not _valid_retained_source_identity(inputs)
    ):
        raise LigandSimilarityIndexError("ligand_similarity_runtime_manifest_invalid")
    if database_meta.get("sha256") != database_sha256:
        raise LigandSimilarityIndexError("ligand_similarity_database_hash_invalid")
    try:
        with _read_connection(database) as connection:
            objects = {
                (str(row[0]), str(row[1]))
                for row in connection.execute(
                    "SELECT type, name FROM sqlite_master WHERE type IN ('table', 'index')"
                )
            }
            required = {
                ("table", "molecule"),
                ("table", "target"),
                ("table", "evidence"),
                ("table", "structure_hint"),
                ("table", "metadata"),
                ("index", "idx_molecule_popcount"),
            }
            if not required.issubset(objects):
                raise LigandSimilarityIndexError("ligand_similarity_runtime_objects_missing")
            if not _compact_schema_valid(connection):
                raise LigandSimilarityIndexError("ligand_similarity_runtime_columns_invalid")
            metadata = dict(
                connection.execute(
                    "SELECT key, value FROM metadata WHERE key IN "
                    "('schema_version','status','fingerprint','source_identity')"
                )
            )
            expected_identity = {
                source: {
                    "source_digest": inputs[source]["source_digest"],
                    "source_release": inputs[source]["source_release"],
                }
                for source in ("chembl", "bindingdb")
            }
            if (
                metadata.get("schema_version") != SCHEMA_VERSION
                or metadata.get("status") != "ready"
                or json.loads(metadata.get("fingerprint", "{}"))
                != _stable_fingerprint_metadata(fingerprint)
                or json.loads(metadata.get("source_identity", "{}")) != expected_identity
            ):
                raise LigandSimilarityIndexError("ligand_similarity_runtime_metadata_invalid")
    except (sqlite3.Error, json.JSONDecodeError) as exc:
        raise LigandSimilarityIndexError(
            f"ligand_similarity_runtime_database_invalid:{_safe_error(exc)}"
        ) from exc
    manifest["_runtime_publication"] = {
        "compact_database_sha256": database_sha256,
        "compact_manifest_sha256": manifest_sha256,
        "compact_schema_version": SCHEMA_VERSION,
        "compact_build_identity": _compact_build_identity(manifest),
    }
    return manifest


@lru_cache(maxsize=RUNTIME_PUBLICATION_CACHE_SIZE)
def _cached_runtime_publication(
    database_path: str,
    database_size: int,
    database_mtime_ns: int,
    manifest_path: str,
    manifest_size: int,
    manifest_mtime_ns: int,
) -> tuple[bytes, str, str]:
    database = Path(database_path)
    manifest = Path(manifest_path)
    try:
        manifest_bytes = manifest.read_bytes()
        manifest_sha256 = hashlib.sha256(manifest_bytes).hexdigest()
        database_sha256 = _sha256_file(database)
        database_after = database.stat()
        manifest_after = manifest.stat()
    except OSError as exc:
        raise LigandSimilarityIndexError(
            f"ligand_similarity_publication_read_failed:{_safe_error(exc)}"
        ) from exc
    if (
        database_after.st_size != database_size
        or database_after.st_mtime_ns != database_mtime_ns
        or manifest_after.st_size != manifest_size
        or manifest_after.st_mtime_ns != manifest_mtime_ns
    ):
        raise LigandSimilarityIndexError("ligand_similarity_publication_changed_during_validation")
    return manifest_bytes, database_sha256, manifest_sha256


def _read_json_bytes(value: bytes, path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(value.decode("utf-8-sig"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise LigandSimilarityIndexError(f"invalid_json:{path}:{_safe_error(exc)}") from exc
    if not isinstance(payload, dict):
        raise LigandSimilarityIndexError(f"json_object_required:{path}")
    return payload


def _compact_build_identity(manifest: dict[str, Any]) -> str:
    inputs = manifest.get("inputs", {})
    identity = {
        "schema_version": manifest.get("schema_version"),
        "fingerprint": _stable_fingerprint_metadata(manifest.get("fingerprint", {})),
        "sources": {
            source: {
                "source_digest": inputs.get(source, {}).get("source_digest"),
                "source_release": inputs.get(source, {}).get("source_release"),
            }
            for source in ("chembl", "bindingdb")
        },
    }
    encoded = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _runtime_publication_identity(manifest: dict[str, Any]) -> dict[str, str]:
    publication = manifest.get("_runtime_publication")
    if not isinstance(publication, dict) or any(
        not publication.get(field)
        for field in (
            "compact_database_sha256",
            "compact_manifest_sha256",
            "compact_schema_version",
            "compact_build_identity",
        )
    ):
        raise LigandSimilarityIndexError("ligand_similarity_runtime_publication_identity_missing")
    return {field: str(value) for field, value in publication.items()}


def _exact_candidates(
    database: Path, manifest: dict[str, Any], query_fp: Any, threshold: float
) -> list[dict[str, Any]]:
    _Chem, DataStructs, _rdBase, _generator = _rdkit()
    query_popcount = int(query_fp.GetNumOnBits())
    low, high = _bitbound(query_popcount, threshold)
    encoding = str(manifest["fingerprint"]["encoding"])
    candidates: list[dict[str, Any]] = []
    with _read_connection(database) as connection:
        cursor = connection.execute(
            "SELECT id, canonical_smiles, fingerprint FROM molecule "
            "INDEXED BY idx_molecule_popcount WHERE popcount BETWEEN ? AND ? "
            "ORDER BY ABS(popcount - ?), popcount, id",
            (low, high, query_popcount),
        )
        scanned = 0
        zero_match_streak = 0
        while rows := cursor.fetchmany(2048):
            fps = [DataStructs.CreateFromBinaryText(_decode_blob(bytes(row[2]), encoding)) for row in rows]
            similarities = DataStructs.BulkTanimotoSimilarity(query_fp, fps)
            batch_matches = 0
            for row, similarity in zip(rows, similarities):
                scanned += 1
                if similarity + 1e-12 >= threshold:
                    candidates.append(
                        {
                            "id": int(row[0]),
                            "canonical_smiles": str(row[1]),
                            "similarity": float(similarity),
                        }
                    )
                    batch_matches += 1
            if batch_matches == 0:
                zero_match_streak += 1
            else:
                zero_match_streak = 0
            if zero_match_streak >= 32 and len(candidates) >= 10:
                break
    return candidates


def _candidate_sources(database: Path, molecule_ids: list[int]) -> dict[int, list[str]]:
    result: dict[int, list[str]] = {}
    with _read_connection(database) as connection:
        for batch in _batches(molecule_ids, MAX_SQL_PARAMETERS):
            placeholders = ",".join("?" for _ in batch)
            rows = connection.execute(
                f"SELECT DISTINCT e.molecule_id, t.source FROM evidence AS e "
                f"JOIN target AS t ON t.id=e.target_fk WHERE e.molecule_id IN ({placeholders}) "
                "ORDER BY e.molecule_id, t.source",
                batch,
            )
            for molecule_id, source in rows:
                result.setdefault(int(molecule_id), []).append(str(source))
    return result


def _candidate_activity_upper_bounds(
    database: Path, molecule_ids: list[int]
) -> dict[int, float]:
    result: dict[int, float] = {}
    with _read_connection(database) as connection:
        connection.create_function(
            "validated_activity_score", 3, _validated_activity_score, deterministic=True
        )
        for batch in _batches(molecule_ids, MAX_SQL_PARAMETERS):
            placeholders = ",".join("?" for _ in batch)
            for molecule_id, score in connection.execute(
                f"SELECT e.molecule_id, MAX(validated_activity_score("
                f"t.source, e.activity, e.activity_score)) "
                f"FROM evidence AS e JOIN target AS t ON t.id=e.target_fk "
                f"WHERE e.molecule_id IN ({placeholders}) GROUP BY e.molecule_id",
                batch,
            ):
                if score is not None:
                    result[int(molecule_id)] = float(score)
    return result


def _validated_activity_score(
    source: str, activity: str, stored_score: object
) -> float | None:
    try:
        score = float(stored_score)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(score) or not 0.0 <= score <= 1.0:
        return None
    if source != "ChEMBL":
        return score
    matched = CHEMBL_ACTIVITY_PATTERN.fullmatch(activity.strip())
    if matched is None:
        return None
    value = float(matched.group(1))
    if not math.isfinite(value) or value <= 0.0:
        return None
    if value <= 10.0:
        return 1.0
    if value <= 100.0:
        return 0.95
    if value <= 1_000.0:
        return 0.85
    if value <= 10_000.0:
        return 0.70
    if value <= 100_000.0:
        return 0.55
    return 0.40


def _rank_structural_candidates(
    candidates: list[dict[str, Any]],
    activity_upper_bounds: dict[int, float],
) -> list[dict[str, Any]]:
    ranked = [
        {
            **candidate,
            "match_score_upper_bound": (
                (0.85 * float(candidate["similarity"]) + 0.15)
                * activity_upper_bounds.get(int(candidate["id"]), 0.0)
            ),
        }
        for candidate in candidates
    ]
    ranked.sort(
        key=lambda row: (
            -float(row["match_score_upper_bound"]),
            -float(row["similarity"]),
            str(row["canonical_smiles"]),
            int(row["id"]),
        ),
    )
    return ranked


def _structural_top_k_is_complete(
    by_target: dict[tuple[str, str], dict[str, Any]],
    top_k: int,
    next_candidate_upper_bound: float | None,
) -> bool:
    if len(by_target) < top_k:
        return False
    if next_candidate_upper_bound is None:
        return True
    kth_score = float(sorted(by_target.values(), key=_target_rank)[top_k - 1]["match_score"])
    return kth_score > float(next_candidate_upper_bound) + 1e-12


def _apply_structure_compatibility(
    candidates: list[dict[str, Any]], query_features: dict[str, Any]
) -> None:
    _shape_descriptors(query_features, required=True)
    for candidate in candidates:
        summary = _three_dimensional_structure_summary(candidate["canonical_smiles"])
        if summary is None:
            summary = _lightweight_structure_summary(candidate["canonical_smiles"])
            candidate["structural_compatibility"] = _two_dimensional_compatibility(
                query_features, summary, float(candidate["similarity"])
            )
            candidate["structural_compatibility_method"] = "2d_candidate_3d_unavailable"
            continue
        candidate["structural_compatibility"] = _structure_compatibility(
            query_features, summary, float(candidate["similarity"])
        )
        candidate["structural_compatibility_method"] = STRUCTURE_SCORING_METHOD


@lru_cache(maxsize=4096)
def _lightweight_structure_summary(smiles: str) -> dict[str, Any]:
    Chem, _DataStructs, _rdBase, _generator = _rdkit()
    molecule = _standardized_smiles_molecule(smiles, Chem, _standardization_tools())
    if molecule is None:
        raise LigandSimilarityIndexError("candidate_structure_summary_failed")
    from rdkit.Chem import Crippen, Descriptors, Lipinski, rdMolDescriptors

    formal_charge = sum(atom.GetFormalCharge() for atom in molecule.GetAtoms())
    counts: dict[str, int] = {}
    for feature in _chemical_feature_factory().GetFeaturesForMol(molecule):
        family = str(feature.GetFamily())
        counts[family] = counts.get(family, 0) + 1
    return {
        "physicochemical": {
            "molecular_weight": float(Descriptors.MolWt(molecule)),
            "exact_molecular_weight": float(Descriptors.ExactMolWt(molecule)),
            "logp": float(Crippen.MolLogP(molecule)),
            "topological_polar_surface_area": float(rdMolDescriptors.CalcTPSA(molecule)),
            "hydrogen_bond_donors": int(Lipinski.NumHDonors(molecule)),
            "hydrogen_bond_acceptors": int(Lipinski.NumHAcceptors(molecule)),
            "rotatable_bonds": int(Lipinski.NumRotatableBonds(molecule)),
            "ring_count": int(rdMolDescriptors.CalcNumRings(molecule)),
            "aromatic_ring_count": int(rdMolDescriptors.CalcNumAromaticRings(molecule)),
            "fraction_csp3": float(rdMolDescriptors.CalcFractionCSP3(molecule)),
            "formal_charge": formal_charge,
            "heavy_atom_count": int(molecule.GetNumHeavyAtoms()),
        },
        "pharmacophore": {"counts": counts},
    }


@lru_cache(maxsize=4096)
def _three_dimensional_structure_summary(smiles: str) -> dict[str, Any] | None:
    Chem, _DataStructs, _rdBase, _generator = _rdkit()
    molecule = _standardized_smiles_molecule(smiles, Chem, _standardization_tools())
    if molecule is None:
        return None
    from rdkit.Chem import AllChem, rdMolDescriptors

    embedded = Chem.AddHs(Chem.Mol(molecule))
    parameters = AllChem.ETKDGv3()
    parameters.randomSeed = _three_dimensional_seed(smiles)
    parameters.numThreads = 1
    if AllChem.EmbedMolecule(embedded, parameters) != 0:
        return None
    if AllChem.MMFFHasAllMoleculeParams(embedded):
        status = AllChem.MMFFOptimizeMolecule(
            embedded,
            confId=0,
            maxIters=300,
            mmffVariant="MMFF94s",
        )
    elif AllChem.UFFHasAllMoleculeParams(embedded):
        status = AllChem.UFFOptimizeMolecule(embedded, confId=0, maxIters=300)
    else:
        return None
    if int(status) != 0:
        return None
    try:
        usr = [float(value) for value in rdMolDescriptors.GetUSR(embedded, confId=0)]
        usrcat = [
            float(value)
            for value in rdMolDescriptors.GetUSRCAT(embedded, confId=0)
        ]
    except (RuntimeError, ValueError):
        return None
    if (
        len(usr) != 12
        or len(usrcat) != 60
        or not all(math.isfinite(value) for value in (*usr, *usrcat))
    ):
        return None
    return {
        **_lightweight_structure_summary(smiles),
        "shape": {
            "method": STRUCTURE_SCORING_METHOD,
            "usr": usr,
            "usrcat": usrcat,
        },
    }


def _three_dimensional_seed(smiles: str) -> int:
    value = int(hashlib.sha256(smiles.encode("utf-8")).hexdigest()[:8], 16) & 0x7FFFFFFF
    return value or 1


@lru_cache(maxsize=1)
def _chemical_feature_factory() -> Any:
    from rdkit import RDConfig
    from rdkit.Chem import ChemicalFeatures

    return ChemicalFeatures.BuildFeatureFactory(
        str(Path(RDConfig.RDDataDir) / "BaseFeatures.fdef")
    )


def _structure_compatibility(
    query: dict[str, Any], candidate: dict[str, Any], fallback: float
) -> float:
    from rdkit.Chem import rdMolDescriptors

    query_usr, query_usrcat = _shape_descriptors(query, required=True)
    candidate_usr, candidate_usrcat = _shape_descriptors(candidate, required=False)
    if candidate_usr is None or candidate_usrcat is None:
        return _two_dimensional_compatibility(query, candidate, fallback)
    shape_score = float(rdMolDescriptors.GetUSRScore(query_usr, candidate_usr))
    pharmacophore_score = float(
        rdMolDescriptors.GetUSRScore(
            query_usrcat,
            candidate_usrcat,
            [0.0, 1.0, 1.0, 1.0, 1.0],
        )
    )
    if not math.isfinite(shape_score) or not math.isfinite(pharmacophore_score):
        raise LigandSimilarityIndexError("candidate_structure_score_invalid")
    two_dimensional = _two_dimensional_compatibility(query, candidate, fallback)
    compatibility = (
        0.45 * shape_score + 0.35 * pharmacophore_score + 0.20 * two_dimensional
    )
    return min(1.0, max(0.0, compatibility))


def _shape_descriptors(
    summary: dict[str, Any], *, required: bool
) -> tuple[list[float] | None, list[float] | None]:
    shape = summary.get("shape")
    if not isinstance(shape, dict) or shape.get("method") != STRUCTURE_SCORING_METHOD:
        if required:
            raise LigandSimilarityIndexError("query_structure_3d_descriptor_missing")
        return None, None
    try:
        usr = [float(value) for value in shape["usr"]]
        usrcat = [float(value) for value in shape["usrcat"]]
    except (KeyError, TypeError, ValueError) as exc:
        if required:
            raise LigandSimilarityIndexError("query_structure_3d_descriptor_invalid") from exc
        return None, None
    if (
        len(usr) != 12
        or len(usrcat) != 60
        or not all(math.isfinite(value) for value in (*usr, *usrcat))
    ):
        if required:
            raise LigandSimilarityIndexError("query_structure_3d_descriptor_invalid")
        return None, None
    return usr, usrcat


def _two_dimensional_compatibility(
    query: dict[str, Any], candidate: dict[str, Any], fallback: float
) -> float:
    query_physical = query.get("physicochemical")
    candidate_physical = candidate.get("physicochemical")
    physical_scores: list[float] = []
    scales = {
        "molecular_weight": 100.0,
        "exact_molecular_weight": 100.0,
        "logp": 2.0,
        "topological_polar_surface_area": 40.0,
        "hydrogen_bond_donors": 2.0,
        "hydrogen_bond_acceptors": 3.0,
        "rotatable_bonds": 3.0,
        "ring_count": 2.0,
        "aromatic_ring_count": 2.0,
        "fraction_csp3": 0.5,
        "formal_charge": 2.0,
        "heavy_atom_count": 10.0,
    }
    if isinstance(query_physical, dict) and isinstance(candidate_physical, dict):
        for key, floor in scales.items():
            try:
                left = float(query_physical[key])
                right = float(candidate_physical[key])
            except (KeyError, TypeError, ValueError):
                continue
            scale = max(floor, abs(left), abs(right))
            physical_scores.append(max(0.0, 1.0 - abs(left - right) / scale))
    query_counts = (
        query.get("pharmacophore", {}).get("counts", {})
        if isinstance(query.get("pharmacophore"), dict)
        else {}
    )
    candidate_counts = (
        candidate.get("pharmacophore", {}).get("counts", {})
        if isinstance(candidate.get("pharmacophore"), dict)
        else {}
    )
    pharmacophore_score: float | None = None
    if isinstance(query_counts, dict) and isinstance(candidate_counts, dict):
        families = set(query_counts) | set(candidate_counts)
        if families:
            intersection = sum(
                min(float(query_counts.get(key, 0)), float(candidate_counts.get(key, 0)))
                for key in families
            )
            union = sum(
                max(float(query_counts.get(key, 0)), float(candidate_counts.get(key, 0)))
                for key in families
            )
            if union > 0:
                pharmacophore_score = intersection / union
    if physical_scores and pharmacophore_score is not None:
        compatibility = 0.7 * (sum(physical_scores) / len(physical_scores)) + 0.3 * pharmacophore_score
    elif physical_scores:
        compatibility = sum(physical_scores) / len(physical_scores)
    elif pharmacophore_score is not None:
        compatibility = pharmacophore_score
    else:
        compatibility = fallback
    return min(1.0, max(0.0, float(compatibility)))


def _target_rank(result: dict[str, Any]) -> tuple[Any, ...]:
    return (
        -float(result["match_score"]),
        -float(result["similarity"]),
        -float(result["activity_score"]),
        int(result.get("relation_rank", 0)),
        float(result.get("numeric_rank", 0.0)),
        str(result["source"]),
        str(result["target_id"]),
        str(result["canonical_smiles"]),
    )


def _target_evidence(result: dict[str, Any]) -> str:
    source_marker = (
        "bindingdb_onsite_activity_row"
        if result["source"] == "BindingDB"
        else "chembl_onsite_activity_row"
    )
    relation = "=" if int(result.get("relation_rank", 0)) == 0 else "other"
    return (
        f"{source_marker};{result['activity']};relation={relation};"
        "ligand_similarity_index;"
        f"activity_score={float(result['activity_score']):.4f};"
        f"structural_compatibility={float(result['structural_compatibility']):.6f};"
        f"structural_compatibility_method={result.get('structural_compatibility_method', '')};"
        f"tanimoto={float(result['similarity']):.6f};"
        f"ligand_code={result.get('ligand_code', '')};"
        f"pdb_ids={result.get('pdb_ids', '')};"
        f"target_chain_pdb_ids={result.get('target_chain_pdb_ids', '')};"
        f"source_digest={result['source_digest']};"
        f"source_release={result['source_release']};"
        f"compact_database_sha256={result['compact_database_sha256']};"
        f"compact_manifest_sha256={result['compact_manifest_sha256']};"
        f"compact_schema_version={result['compact_schema_version']};"
        f"compact_build_identity={result['compact_build_identity']};"
        f"query_feature_digest={result.get('query_feature_digest', '')};"
        f"exact_candidate_count={int(result.get('exact_candidate_count', 0))};"
        f"rescored_candidate_count={int(result.get('rescored_candidate_count', 0))};"
        f"structural_candidates_truncated={str(bool(result.get('structural_candidates_truncated', False))).lower()};"
        f"search_algorithm={SEARCH_ALGORITHM}"
    )


def _write_tsv_row(handle: Any, values: Iterable[Any]) -> None:
    fields = [str(value).replace("\t", " ").replace("\r", " ").replace("\n", " ") for value in values]
    handle.write("\t".join(fields) + "\n")


def _write_known_database_rows(
    records: list[dict[str, Any]],
    database: Path,
    manifest: Path,
    output: Path,
    top_k: int,
    threshold: float,
) -> dict[str, Any]:
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = _temporary_path(output, ".tsv")
    row_count = 0
    exact_candidate_count = 0
    rescored_candidate_count = 0
    truncated = False
    try:
        validated_manifest = _runtime_manifest(database, manifest)
        with staging.open("w", encoding="utf-8", newline="") as handle:
            _write_tsv_row(handle, KNOWN_DATABASE_HEADER.split("\t"))
            for record in records:
                targets = query_targets(
                    database,
                    record["canonical_smiles"],
                    top_k,
                    threshold,
                    manifest_path=manifest,
                    query_feature_digest=record["feature_digest"],
                    query_structure_features=record.get("query_structure"),
                    _validated_manifest=validated_manifest,
                )
                if targets:
                    exact_candidate_count += int(targets[0]["exact_candidate_count"])
                    rescored_candidate_count += int(targets[0]["rescored_candidate_count"])
                    truncated = truncated or bool(
                        targets[0]["structural_candidates_truncated"]
                    )
                for result in targets:
                    query_id = hashlib.sha256(
                        "\0".join(
                            [
                                record["input_id"],
                                record["record_id"],
                                record["feature_digest"],
                                result["source"],
                                result["target_id"],
                                result["canonical_smiles"],
                            ]
                        ).encode("utf-8")
                    ).hexdigest()
                    matched = (
                        f"canonical_smiles={result['canonical_smiles']};"
                        f"tanimoto={result['similarity']:.6f};"
                        f"structural_compatibility={result['structural_compatibility']:.6f};"
                        f"structural_compatibility_method={result.get('structural_compatibility_method', '')};"
                        f"activity_score={result['activity_score']:.4f};"
                        f"ligand_code={result.get('ligand_code', '')};"
                        f"pdb_ids={result.get('pdb_ids', '')};"
                        f"target_chain_pdb_ids={result.get('target_chain_pdb_ids', '')}"
                    )
                    provenance = (
                        "ligand_similarity_index;"
                        f"source_digest={result['source_digest']};"
                        f"source_release={result['source_release']};"
                        f"compact_database_sha256={result['compact_database_sha256']};"
                        f"compact_manifest_sha256={result['compact_manifest_sha256']};"
                        f"compact_schema_version={result['compact_schema_version']};"
                        f"compact_build_identity={result['compact_build_identity']}"
                    )
                    _write_tsv_row(
                        handle,
                        [
                            record["input_id"],
                            result["source"],
                            result["target_id"],
                            result["target_name"],
                            result["organism"],
                            result["target_class"],
                            f"{result['match_score']:.4f}",
                            matched,
                            result["evidence"],
                            provenance,
                            result["activity"],
                            (
                                f"ligand_smiles={result['canonical_smiles']};"
                                f"ligand_code={result.get('ligand_code', '')}"
                            ),
                            query_id,
                            record["record_id"],
                        ],
                    )
                    row_count += 1
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(staging, output)
        return {
            "rows": row_count,
            "exact_candidate_count": exact_candidate_count,
            "rescored_candidate_count": rescored_candidate_count,
            "structural_candidates_truncated": truncated,
        }
    finally:
        staging.unlink(missing_ok=True)


def _read_query_manifest(path: Path) -> list[dict[str, Any]]:
    conversion_digest, expected_inputs, current_invocation = _current_query_inputs(path.parent)
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        required = {
            "input_id",
            "record_id",
            "input_kind",
            "input_digest",
            "method",
            "features_json",
            "feature_digest",
            "status",
        }
        if reader.fieldnames is None or not required.issubset(reader.fieldnames):
            raise LigandSimilarityIndexError("query_structure_manifest_schema_invalid")
        rows = list(reader)

    query_rows: dict[tuple[str, str], dict[str, str]] = {}
    for row in rows:
        if str(row.get("input_kind", "")).strip() != "small_molecule":
            continue
        identity = (
            str(row.get("input_id", "")).strip(),
            str(row.get("record_id", "")).strip(),
        )
        if identity in query_rows:
            raise LigandSimilarityIndexError("query_structure_feature_identity_duplicate")
        query_rows[identity] = row
    if set(query_rows) != set(expected_inputs):
        raise LigandSimilarityIndexError("query_structure_expected_identity_mismatch")

    records: list[dict[str, Any]] = []
    seen_features: set[Path] = set()
    for identity in sorted(expected_inputs):
        row = query_rows[identity]
        status = str(row.get("status", "")).strip()
        if status != "ready":
            raise LigandSimilarityIndexError(
                f"query_structure_expected_record_not_ready:{identity[0]}:{identity[1]}:{status or 'missing'}"
            )
        if str(row.get("method", "")).strip() != QUERY_METHOD_VERSION:
            raise LigandSimilarityIndexError("query_structure_manifest_method_invalid")
        feature_path = Path(str(row.get("features_json", "")).strip())
        if not feature_path.is_absolute():
            feature_path = path.parent / feature_path
        feature_path = feature_path.resolve(strict=True)
        try:
            feature_path.relative_to(path.parent.resolve())
        except ValueError as exc:
            raise LigandSimilarityIndexError(
                "query_structure_features_path_outside_run_dir"
            ) from exc
        if feature_path in seen_features:
            raise LigandSimilarityIndexError("query_structure_feature_identity_duplicate")
        seen_features.add(feature_path)
        feature = _read_json(feature_path)
        input_id, record_id = identity
        feature_digest = str(row.get("feature_digest", "")).strip()
        if not _is_sha256(feature_digest) or _sha256_file(feature_path) != feature_digest:
            raise LigandSimilarityIndexError("query_structure_feature_digest_mismatch")
        source_digest = expected_inputs[identity]
        expected = {
            "schema_version": QUERY_FEATURE_SCHEMA_VERSION,
            "status": "ready",
            "input_id": input_id,
            "record_id": record_id,
            "input_kind": "small_molecule",
            "input_digest": str(row.get("input_digest", "")).strip(),
            "source_content_sha256": source_digest,
            "conversion_manifest_sha256": conversion_digest,
            "invocation_id": current_invocation,
            "method_version": QUERY_METHOD_VERSION,
        }
        if any(feature.get(key) != value for key, value in expected.items()):
            raise LigandSimilarityIndexError("query_structure_feature_identity_mismatch")
        _shape_descriptors(feature, required=True)
        artifact_path = (feature_path.parent / "manifest.json").resolve(strict=True)
        try:
            artifact_path.relative_to(path.parent.resolve())
        except ValueError as exc:
            raise LigandSimilarityIndexError(
                "query_structure_artifact_path_outside_run_dir"
            ) from exc
        artifact = _read_json(artifact_path)
        artifact_expected = {
            **expected,
            "schema_version": QUERY_ARTIFACT_SCHEMA_VERSION,
            "feature_digest": feature_digest,
            "run_id": feature.get("run_id"),
            "job_id": feature.get("job_id"),
        }
        if any(artifact.get(key) != value for key, value in artifact_expected.items()):
            raise LigandSimilarityIndexError("query_structure_artifact_identity_mismatch")
        artifact_feature = Path(str(artifact.get("features_json", "")))
        if not artifact_feature.is_absolute():
            artifact_feature = path.parent / artifact_feature
        if artifact_feature.resolve(strict=True) != feature_path:
            raise LigandSimilarityIndexError("query_structure_artifact_feature_path_mismatch")
        if not all(
            _is_sha256(value)
            for value in (
                expected["input_digest"],
                source_digest,
                conversion_digest,
                feature_digest,
            )
        ):
            raise LigandSimilarityIndexError("query_structure_feature_identity_digest_invalid")
        if not all(
            isinstance(feature.get(key), str) and str(feature.get(key)).strip()
            for key in ("run_id", "job_id", "invocation_id")
        ):
            raise LigandSimilarityIndexError("query_structure_feature_run_binding_invalid")
        canonical = str(feature.get("isomeric_smiles") or feature.get("canonical_smiles") or "")
        if not canonical:
            raise LigandSimilarityIndexError("query_structure_canonical_smiles_missing")
        fingerprint = _validate_query_fingerprint(feature.get("fingerprint"))
        query_fp, standardized = _fingerprint_from_smiles(canonical)
        _Chem, DataStructs, _rdBase, _generator = _rdkit()
        recomputed = bytes(DataStructs.BitVectToBinaryText(query_fp))
        if (
            hashlib.sha256(recomputed).hexdigest() != fingerprint["packed_sha256"]
            or standardized != canonical
        ):
            raise LigandSimilarityIndexError("query_structure_fingerprint_smiles_mismatch")
        records.append(
            {
                "input_id": input_id,
                "record_id": record_id,
                "canonical_smiles": canonical,
                "feature_digest": feature_digest,
                "query_structure": feature,
            }
        )
    return records


def _current_query_inputs(
    run_dir: Path,
) -> tuple[str, dict[tuple[str, str], str], str]:
    conversion = run_dir / "input_conversion_manifest.tsv"
    if not conversion.is_file():
        raise LigandSimilarityIndexError("query_structure_conversion_manifest_missing")
    with conversion.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        required = {
            "input_id",
            "record_id",
            "canonical_kind",
            "conversion_status",
            "content_sha256",
        }
        if reader.fieldnames is None or not required.issubset(reader.fieldnames):
            raise LigandSimilarityIndexError("query_structure_conversion_manifest_schema_invalid")
        expected: dict[tuple[str, str], str] = {}
        for row in reader:
            if (
                str(row.get("conversion_status", "")).strip() != "converted"
                or str(row.get("canonical_kind", "")).strip() != "small_molecule"
            ):
                continue
            identity = (
                str(row.get("input_id", "")).strip(),
                str(row.get("record_id", "")).strip(),
            )
            source_digest = str(row.get("content_sha256", "")).strip().lower()
            if not all(identity) or not _is_sha256(source_digest):
                raise LigandSimilarityIndexError("query_structure_conversion_identity_invalid")
            if identity in expected:
                raise LigandSimilarityIndexError("query_structure_conversion_identity_duplicate")
            expected[identity] = source_digest
    invocation_path = run_dir / "worker_invocation_id.txt"
    if not invocation_path.is_file():
        raise LigandSimilarityIndexError("query_structure_worker_invocation_missing")
    current_invocation = invocation_path.read_text(encoding="utf-8").strip()
    if not current_invocation:
        raise LigandSimilarityIndexError("query_structure_worker_invocation_missing")
    return _sha256_file(conversion), expected, current_invocation


def _validate_query_fingerprint(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise LigandSimilarityIndexError("query_structure_fingerprint_missing")
    if (
        value.get("algorithm") != FINGERPRINT_TYPE
        or value.get("radius") != FINGERPRINT_RADIUS
        or value.get("n_bits") != FINGERPRINT_BITS
        or value.get("include_chirality") is not True
        or value.get("encoding") != "rdkit_binary_text_hex_v1"
    ):
        raise LigandSimilarityIndexError("query_structure_fingerprint_incompatible")
    packed_hex = value.get("packed_hex")
    if not isinstance(packed_hex, str) or len(packed_hex) != FINGERPRINT_BITS // 4:
        raise LigandSimilarityIndexError("query_structure_fingerprint_encoding_invalid")
    try:
        packed = bytes.fromhex(packed_hex)
    except ValueError as exc:
        raise LigandSimilarityIndexError("query_structure_fingerprint_encoding_invalid") from exc
    if value.get("packed_sha256") != hashlib.sha256(packed).hexdigest():
        raise LigandSimilarityIndexError("query_structure_fingerprint_digest_invalid")
    return value


def _valid_fingerprint_manifest(value: dict[str, Any]) -> bool:
    return bool(
        value.get("type") == FINGERPRINT_TYPE
        and value.get("radius") == FINGERPRINT_RADIUS
        and value.get("bits") == FINGERPRINT_BITS
        and value.get("include_chirality") is True
        and value.get("standardization") == STANDARDIZATION_METHOD
        and value.get("tautomer_max_tautomers") == TAUTOMER_MAX_TAUTOMERS
        and value.get("tautomer_max_transforms") == TAUTOMER_MAX_TRANSFORMS
        and isinstance(value.get("build_workers"), int)
        and 1 <= value.get("build_workers", 0) <= MAX_BUILD_WORKERS
        and value.get("lsh_bands") == 0
        and value.get("search_algorithm") == SEARCH_ALGORITHM
        and value.get("bitbound_formula") == BITBOUND_FORMULA
        and value.get("reference") == REFERENCE
        and value.get("encoding") in {"rdkit_binary_text", "zlib1_rdkit_binary_text"}
    )


def _valid_retained_source_identity(inputs: dict[str, Any]) -> bool:
    for source in ("chembl", "bindingdb"):
        value = inputs.get(source)
        if not isinstance(value, dict):
            return False
        if (
            not value.get("source_release")
            or not _is_sha256(value.get("source_digest"))
            or not _is_sha256(value.get("sha256"))
            or not isinstance(value.get("bytes"), int)
            or value.get("bytes", 0) <= 0
        ):
            return False
    return True


def _stable_fingerprint_metadata(value: dict[str, Any]) -> dict[str, Any]:
    keys = (
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
    return {key: value[key] for key in keys}


def _database_counts(database: Path) -> dict[str, int]:
    with _read_connection(database) as connection:
        return _connection_counts(connection)


def _connection_counts(connection: sqlite3.Connection) -> dict[str, int]:
    return {
        "molecules": int(connection.execute("SELECT COUNT(*) FROM molecule").fetchone()[0]),
        "targets": int(connection.execute("SELECT COUNT(*) FROM target").fetchone()[0]),
        "evidence": int(connection.execute("SELECT COUNT(*) FROM evidence").fetchone()[0]),
        "hints": int(connection.execute("SELECT COUNT(*) FROM structure_hint").fetchone()[0]),
    }


def _fingerprint_from_smiles(smiles: str) -> tuple[Any, str]:
    Chem, _DataStructs, _rdBase, generator = _rdkit()
    molecule = _standardized_smiles_molecule(
        str(smiles), Chem, _standardization_tools()
    )
    if molecule is None:
        raise LigandSimilarityIndexError("invalid_query_smiles")
    return generator.GetFingerprint(molecule), str(
        Chem.MolToSmiles(molecule, isomericSmiles=True)
    )


def _standardization_tools() -> tuple[Any, Any, Any]:
    try:
        from rdkit.Chem.MolStandardize import rdMolStandardize
    except ImportError as exc:
        raise LigandSimilarityIndexError("rdkit_molstandardize_required") from exc
    tautomer = rdMolStandardize.TautomerEnumerator()
    tautomer.SetMaxTautomers(TAUTOMER_MAX_TAUTOMERS)
    tautomer.SetMaxTransforms(TAUTOMER_MAX_TRANSFORMS)
    return (rdMolStandardize, rdMolStandardize.Reionizer(), tautomer)


def _standardized_smiles_molecule(
    smiles: str, Chem: Any, tools: tuple[Any, Any, Any]
) -> Any | None:
    molecule = Chem.MolFromSmiles(str(smiles).strip())
    if molecule is None:
        return None
    rdMolStandardize, reionizer, tautomer = tools
    try:
        cleaned = rdMolStandardize.Cleanup(Chem.Mol(molecule))
        parent = rdMolStandardize.FragmentParent(cleaned)
        reionized = reionizer.reionize(parent)
        standardized = tautomer.Canonicalize(reionized)
        Chem.SanitizeMol(standardized)
    except Exception:
        return None
    return standardized if standardized.GetNumHeavyAtoms() > 0 else None


def _rdkit() -> tuple[Any, Any, Any, Any]:
    try:
        from rdkit import Chem, DataStructs, RDLogger, rdBase
        from rdkit.Chem import rdFingerprintGenerator
    except ImportError as exc:
        raise LigandSimilarityIndexError("rdkit_required_for_ligand_similarity_index") from exc
    RDLogger.DisableLog("rdApp.error")
    RDLogger.DisableLog("rdApp.warning")
    generator = rdFingerprintGenerator.GetMorganGenerator(
        radius=FINGERPRINT_RADIUS,
        fpSize=FINGERPRINT_BITS,
        includeChirality=FINGERPRINT_INCLUDE_CHIRALITY,
    )
    return Chem, DataStructs, rdBase, generator


def _decode_blob(value: bytes, encoding: str) -> bytes:
    if encoding == "zlib1_rdkit_binary_text":
        try:
            return zlib.decompress(value)
        except zlib.error as exc:
            raise LigandSimilarityIndexError("ligand_similarity_fingerprint_corrupt") from exc
    if encoding == "rdkit_binary_text":
        return value
    raise LigandSimilarityIndexError("ligand_similarity_fingerprint_encoding_unknown")


def _bitbound(query_popcount: int, threshold: float) -> tuple[int, int]:
    return math.ceil(threshold * query_popcount), math.floor(query_popcount / threshold)


def _validate_query_limits(top_k: int, threshold: float) -> None:
    if not isinstance(top_k, int) or top_k <= 0:
        raise LigandSimilarityIndexError("invalid_similarity_top_k")
    if not isinstance(threshold, (int, float)) or not 0.0 < float(threshold) <= 1.0:
        raise LigandSimilarityIndexError("invalid_similarity_threshold")


@contextmanager
def _read_connection(path: Path) -> Iterator[sqlite3.Connection]:
    connection = sqlite3.connect(_read_uri(path), uri=True)
    try:
        connection.execute("PRAGMA query_only=ON")
        connection.execute("PRAGMA foreign_keys=ON")
        yield connection
    finally:
        connection.close()


def _read_uri(path: Path) -> str:
    return path.resolve().as_uri() + "?mode=ro&immutable=1"


def _columns(connection: sqlite3.Connection, table: str) -> set[str]:
    return {str(row[1]) for row in connection.execute(f"PRAGMA table_info('{table}')")}


def _compact_schema_valid(connection: sqlite3.Connection) -> bool:
    return bool(
        {"id", "canonical_smiles", "fingerprint", "popcount"}.issubset(
            _columns(connection, "molecule")
        )
        and {
            "id",
            "source",
            "target_id",
            "target_name",
            "organism",
            "target_class",
            "source_release",
            "source_digest",
        }.issubset(_columns(connection, "target"))
        and {
            "molecule_id",
            "target_fk",
            "activity_score",
            "activity",
            "relation_rank",
            "numeric_rank",
        }.issubset(_columns(connection, "evidence"))
        and {
            "molecule_id",
            "target_fk",
            "ligand_code",
            "pdb_ids",
            "target_chain_pdb_ids",
        }.issubset(_columns(connection, "structure_hint"))
        and {"key", "value"}.issubset(_columns(connection, "metadata"))
    )


def _require_leading_index(connection: sqlite3.Connection, table: str, column: str) -> None:
    for row in connection.execute(f"PRAGMA index_list('{table}')"):
        index = str(row[1]).replace("'", "''")
        columns = [str(item[2]) for item in connection.execute(f"PRAGMA index_info('{index}')")]
        if columns and columns[0] == column:
            return
    raise LigandSimilarityIndexError(f"similarity_source_index_missing:{table}:{column}")


def _read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        raise LigandSimilarityIndexError(f"invalid_json:{path}:{_safe_error(exc)}") from exc
    if not isinstance(value, dict):
        raise LigandSimilarityIndexError(f"json_object_required:{path}")
    return value


def _write_json_fsync(path: Path, value: dict[str, Any]) -> None:
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(8 * 1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _temporary_path(destination: Path, suffix: str) -> Path:
    handle = tempfile.NamedTemporaryFile(
        prefix=f".{destination.name}.", suffix=f"{suffix}.tmp", dir=destination.parent, delete=False
    )
    handle.close()
    path = Path(handle.name)
    path.unlink()
    return path


def _remove_sqlite_family(path: Path) -> None:
    path.unlink(missing_ok=True)
    Path(f"{path}-wal").unlink(missing_ok=True)
    Path(f"{path}-shm").unlink(missing_ok=True)
    Path(f"{path}-journal").unlink(missing_ok=True)


def _batches(values: Iterable[Any], size: int) -> Iterable[list[Any]]:
    batch: list[Any] = []
    for value in values:
        batch.append(value)
        if len(batch) >= size:
            yield batch
            batch = []
    if batch:
        yield batch


def _integer_batches(maximum: int, size: int) -> Iterable[tuple[int, int]]:
    low = 0
    while low < maximum:
        high = min(maximum, low + size)
        yield low, high
        low = high


def _is_sha256(value: Any) -> bool:
    text = str(value or "")
    return len(text) == 64 and all(ch in "0123456789abcdefABCDEF" for ch in text)


def _safe_error(exc: Exception) -> str:
    return " ".join(str(exc).split())[:500] or exc.__class__.__name__


def _merge_pdb_hints(left: Any, right: Any) -> str:
    identifiers = {
        token.upper()
        for value in (left, right)
        for token in re.findall(r"(?i)(?<![A-Z0-9])[0-9][A-Z0-9]{3}(?![A-Z0-9])", str(value or ""))
    }
    return ",".join(sorted(identifiers)[:16])


def _merge_ligand_codes(left: Any, right: Any) -> str:
    codes = {
        token.upper()
        for value in (left, right)
        for token in re.findall(r"(?i)(?<![A-Z0-9])[A-Z0-9]{1,8}(?![A-Z0-9])", str(value or ""))
    }
    return ",".join(sorted(codes)[:8])[:80]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build and query the compact ligand similarity index")
    subparsers = parser.add_subparsers(dest="command", required=True)

    build = subparsers.add_parser("build")
    build.add_argument("--chembl-db", required=True)
    build.add_argument("--chembl-manifest", required=True)
    build.add_argument("--bindingdb-db", required=True)
    build.add_argument("--bindingdb-manifest", required=True)
    build.add_argument("--output-db", required=True)
    build.add_argument("--manifest", required=True)

    validate = subparsers.add_parser("validate")
    validate.add_argument("--database", required=True)
    validate.add_argument("--manifest", required=True)

    query = subparsers.add_parser("query-targets")
    query.add_argument("--database", required=True)
    query.add_argument("--manifest", required=True)
    query.add_argument("--query-manifest", required=True)
    query.add_argument("--output-tsv", required=True)
    query.add_argument("--top-k", type=int, default=50)
    query.add_argument("--threshold", type=float, default=0.5)

    args = parser.parse_args(argv)
    try:
        if args.command == "build":
            result = build_similarity_index(
                args.chembl_db,
                args.bindingdb_db,
                args.output_db,
                args.manifest,
                chembl_manifest=args.chembl_manifest,
                bindingdb_manifest=args.bindingdb_manifest,
            )
        elif args.command == "validate":
            result = validate_similarity_index(args.database, args.manifest)
        else:
            records = _read_query_manifest(Path(args.query_manifest).resolve())
            query_summary = _write_known_database_rows(
                records,
                Path(args.database).resolve(),
                Path(args.manifest).resolve(),
                Path(args.output_tsv).resolve(),
                args.top_k,
                args.threshold,
            )
            result = {"status": "ready", "queries": len(records), **query_summary}
    except LigandSimilarityIndexError as exc:
        print(str(exc), file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
