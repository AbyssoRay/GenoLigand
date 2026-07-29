from __future__ import annotations

import csv
import hashlib
import json
import os
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from rdkit import DataStructs

from genoligand_workers import ligand_similarity_index as similarity


class LigandSimilarityIndexTests(unittest.TestCase):
    def test_build_connection_reserves_bounded_page_cache(self) -> None:
        with sqlite3.connect(":memory:") as connection:
            similarity._configure_build_connection(connection)
            self.assertEqual(
                connection.execute("PRAGMA cache_size").fetchone(),
                (-similarity.BUILD_CACHE_KIB,),
            )

    def test_build_is_deterministic_compact_and_survives_source_pruning(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_build_") as tmp:
            root = Path(tmp)
            sources = _write_source_databases(root)
            first = _build(root, sources, "first")
            second = _build(root, sources, "second")

            self.assertEqual(first["manifest"]["schema_version"], similarity.SCHEMA_VERSION)
            self.assertEqual(first["manifest"]["status"], "ready")
            self.assertEqual(first["manifest"]["database"]["sha256"], second["manifest"]["database"]["sha256"])
            self.assertLess(
                first["manifest"]["database"]["bytes"],
                first["manifest"]["inputs"]["chembl"]["bytes"]
                + first["manifest"]["inputs"]["bindingdb"]["bytes"],
            )
            self.assertGreater(first["manifest"]["reclaimed_candidate_bytes"], 0)
            self.assertGreater(first["manifest"]["counts"]["hints"], 0)
            self.assertEqual(
                first["manifest"]["fingerprint"]["search_algorithm"],
                similarity.SEARCH_ALGORITHM,
            )
            self.assertEqual(first["manifest"]["fingerprint"]["lsh_bands"], 0)
            self.assertEqual(
                first["manifest"]["fingerprint"]["standardization"],
                similarity.STANDARDIZATION_METHOD,
            )
            self.assertEqual(
                first["manifest"]["fingerprint"]["tautomer_max_tautomers"], 100
            )
            self.assertIn(first["manifest"]["fingerprint"]["encoding"], {
                "rdkit_binary_text",
                "zlib1_rdkit_binary_text",
            })
            self.assertGreater(first["manifest"]["fingerprint"]["encoding_sample_size"], 0)

            with sqlite3.connect(first["database"]) as connection:
                tables = {
                    row[0]
                    for row in connection.execute(
                        "SELECT name FROM sqlite_master WHERE type='table'"
                    )
                }
                self.assertTrue({"molecule", "target", "evidence", "metadata"}.issubset(tables))
                self.assertFalse(any(name.startswith("_build_") for name in tables))
                columns = {
                    row[1] for row in connection.execute("PRAGMA table_info(molecule)")
                }
                self.assertEqual(columns, {"id", "canonical_smiles", "fingerprint", "popcount"})
                evidence_columns = {
                    row[1] for row in connection.execute("PRAGMA table_info(evidence)")
                }
                hint_columns = {
                    row[1]
                    for row in connection.execute("PRAGMA table_info(structure_hint)")
                }
                self.assertFalse(
                    {"ligand_code", "pdb_ids", "target_chain_pdb_ids"}
                    & evidence_columns
                )
                self.assertTrue(
                    {"ligand_code", "pdb_ids", "target_chain_pdb_ids"}.issubset(
                        hint_columns
                    )
                )
                self.assertEqual(
                    connection.execute(
                        "SELECT COUNT(*) FROM molecule WHERE canonical_smiles='CCO'"
                    ).fetchone()[0],
                    1,
                )
                raw_distinct = len(set(sources["raw_smiles"]))
                indexed = connection.execute("SELECT COUNT(*) FROM molecule").fetchone()[0]
                self.assertLessEqual(indexed, raw_distinct - 2)

            original_hashes = {
                key: _sha256(sources[key]) for key in ("chembl_db", "bindingdb_db")
            }
            self.assertFalse(list(root.glob("*.sqlite-wal")))
            self.assertFalse(list(root.glob("*.sqlite-journal")))
            for key in ("chembl_db", "bindingdb_db", "chembl_manifest", "bindingdb_manifest"):
                sources[key].unlink()

            validated = similarity.validate_similarity_index(
                first["database"], first["manifest_path"]
            )
            self.assertEqual(validated["inputs"]["chembl"]["sha256"], original_hashes["chembl_db"])
            self.assertEqual(validated["inputs"]["bindingdb"]["sha256"], original_hashes["bindingdb_db"])

    def test_salt_and_tautomer_standardization_match_query_fingerprints(self) -> None:
        ethanol_fp, ethanol = similarity._fingerprint_from_smiles("CCO")
        salt_fp, salt = similarity._fingerprint_from_smiles("CCO.[Na+]")
        amide_fp, amide = similarity._fingerprint_from_smiles("CC(=O)N")
        imidic_fp, imidic = similarity._fingerprint_from_smiles("CC(O)=N")

        self.assertEqual(salt, ethanol)
        self.assertEqual(DataStructs.TanimotoSimilarity(ethanol_fp, salt_fp), 1.0)
        self.assertEqual(imidic, amide)
        self.assertEqual(DataStructs.TanimotoSimilarity(amide_fp, imidic_fp), 1.0)

    def test_query_structure_features_change_order_inside_same_similarity_neighborhood(self) -> None:
        candidates = [
            {"id": 1, "canonical_smiles": "CCO", "similarity": 0.60},
            {"id": 2, "canonical_smiles": "c1ccccc1", "similarity": 0.60},
        ]
        ethanol_query = similarity._three_dimensional_structure_summary("CCO")
        ring_query = similarity._three_dimensional_structure_summary("c1ccccc1")
        self.assertIsNotNone(ethanol_query)
        self.assertIsNotNone(ring_query)

        ethanol_ranked = [dict(row) for row in candidates]
        similarity._apply_structure_compatibility(ethanol_ranked, ethanol_query)
        ethanol_ranked.sort(
            key=lambda row: (-row["structural_compatibility"], row["canonical_smiles"])
        )
        ring_ranked = [dict(row) for row in candidates]
        similarity._apply_structure_compatibility(ring_ranked, ring_query)
        ring_ranked.sort(
            key=lambda row: (-row["structural_compatibility"], row["canonical_smiles"])
        )

        self.assertEqual(ethanol_ranked[0]["canonical_smiles"], "CCO")
        self.assertEqual(ring_ranked[0]["canonical_smiles"], "c1ccccc1")

    def test_structural_batches_cross_2000_duplicate_ligands_to_cover_distinct_targets(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_target_coverage_") as tmp:
            root = Path(tmp)
            built = _build(root, _write_source_databases(root), "index")
            validated_manifest = similarity._runtime_manifest(
                built["database"], built["manifest_path"]
            )
            first_molecule_id = 10_000
            hot_count = 2_101
            outside_id = first_molecule_id + hot_count
            tail_count = 400
            with sqlite3.connect(built["database"]) as connection:
                target_start = int(
                    connection.execute("SELECT MAX(id) FROM target").fetchone()[0]
                ) + 1
                hot_target = target_start
                outside_target = target_start + 1
                connection.executemany(
                    "INSERT INTO target VALUES (?, 'ChEMBL', ?, ?, ?, 'Homo sapiens', "
                    "'enzyme', 'fixture-release', ?)",
                    [
                        (hot_target, "HOT", "HOT", "Popular target", "c" * 64),
                        (
                            outside_target,
                            "OUTSIDE",
                            "OUTSIDE",
                            "Pool-exterior target",
                            "c" * 64,
                        ),
                    ],
                )
                molecule_rows = [
                    (molecule_id, f"fixture_hot_{molecule_id}", b"x", 1)
                    for molecule_id in range(first_molecule_id, outside_id)
                ]
                molecule_rows.append((outside_id, "fixture_outside", b"x", 1))
                molecule_rows.extend(
                    (molecule_id, f"fixture_tail_{molecule_id}", b"x", 1)
                    for molecule_id in range(
                        outside_id + 1, outside_id + tail_count + 1
                    )
                )
                connection.executemany("INSERT INTO molecule VALUES (?, ?, ?, ?)", molecule_rows)
                connection.executemany(
                    "INSERT INTO evidence VALUES (?, ?, 1.0, 'Ki=1 nM', 0, 1.0)",
                    (
                        (molecule_id, hot_target)
                        for molecule_id in range(first_molecule_id, outside_id)
                    ),
                )
                connection.execute(
                    "INSERT INTO evidence VALUES (?, ?, 1.0, 'Ki=1 nM', 0, 1.0)",
                    (outside_id, outside_target),
                )

            candidates = [
                {
                    "id": molecule_id,
                    "canonical_smiles": f"fixture_hot_{molecule_id}",
                    "similarity": 0.95,
                }
                for molecule_id in range(first_molecule_id, outside_id)
            ]
            candidates.append(
                {
                    "id": outside_id,
                    "canonical_smiles": "fixture_outside",
                    "similarity": 0.80,
                }
            )
            candidates.extend(
                {
                    "id": molecule_id,
                    "canonical_smiles": f"fixture_tail_{molecule_id}",
                    "similarity": 0.79,
                }
                for molecule_id in range(outside_id + 1, outside_id + tail_count + 1)
            )

            def apply_exact_compatibility(batch, _features):
                for candidate in batch:
                    candidate["structural_compatibility"] = candidate["similarity"]

            with patch.object(similarity, "_exact_candidates", return_value=candidates), patch.object(
                similarity,
                "_apply_structure_compatibility",
                side_effect=apply_exact_compatibility,
            ):
                rows = similarity.query_targets(
                    built["database"],
                    "CCO",
                    top_k=2,
                    threshold=0.5,
                    query_structure_features={"physicochemical": {}},
                    _validated_manifest=validated_manifest,
                )

            self.assertEqual(
                {(row["source"], row["target_id"]) for row in rows},
                {("ChEMBL", "HOT"), ("ChEMBL", "OUTSIDE")},
            )
            self.assertGreater(rows[0]["rescored_candidate_count"], 2_000)
            self.assertLess(rows[0]["rescored_candidate_count"], len(candidates))
            self.assertTrue(rows[0]["structural_candidates_truncated"])

    def test_bitbound_results_equal_bruteforce_truth_and_use_popcount_index(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_recall_") as tmp:
            root = Path(tmp)
            built = _build(root, _write_source_databases(root), "index")
            threshold = 0.30
            query_fp, _canonical = similarity._fingerprint_from_smiles("CCO")
            truth: set[str] = set()
            with sqlite3.connect(built["database"]) as connection:
                smiles_values = [row[0] for row in connection.execute(
                    "SELECT canonical_smiles FROM molecule ORDER BY canonical_smiles"
                )]
            for smiles in smiles_values:
                fingerprint, canonical = similarity._fingerprint_from_smiles(smiles)
                if DataStructs.TanimotoSimilarity(query_fp, fingerprint) + 1e-12 >= threshold:
                    truth.add(canonical)

            indexed = similarity.query_similarity(
                built["database"],
                "CCO",
                top_k=100,
                threshold=threshold,
                manifest_path=built["manifest_path"],
            )
            self.assertEqual({row["canonical_smiles"] for row in indexed}, truth)
            plan = similarity.explain_candidate_query(built["database"], "CCO", threshold)
            self.assertTrue(any("idx_molecule_popcount" in row for row in plan), plan)
            self.assertFalse(any("SCAN molecule" in row for row in plan), plan)

    def test_query_targets_uses_similarity_times_best_activity_and_stable_dedup(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_targets_") as tmp:
            root = Path(tmp)
            built = _build(root, _write_source_databases(root), "index")
            rows = similarity.query_targets(
                built["database"],
                "CCO",
                top_k=20,
                threshold=0.30,
                manifest_path=built["manifest_path"],
                query_feature_digest="f" * 64,
            )

            keys = [(row["source"], row["target_id"]) for row in rows]
            self.assertEqual(len(keys), len(set(keys)))
            self.assertEqual(rows, sorted(rows, key=similarity._target_rank))
            for row in rows:
                self.assertAlmostEqual(
                    row["match_score"], row["similarity"] * row["activity_score"], places=12
                )
                self.assertIn("search_algorithm=popcount_bitbound_exact_tanimoto", row["evidence"])
            near = next(row for row in rows if row["target_id"] == "CHEMBL_NEAR")
            exact = next(row for row in rows if row["target_id"] == "CHEMBL_EXACT")
            self.assertGreater(near["match_score"], exact["match_score"])
            self.assertIn("1.0", near["activity"])

    def test_query_rejects_invalid_legacy_chembl_activity_without_rebuild(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_activity_") as tmp:
            root = Path(tmp)
            built = _build(root, _write_source_databases(root), "index")
            validated_manifest = similarity._runtime_manifest(
                built["database"], built["manifest_path"]
            )
            with sqlite3.connect(built["database"]) as connection:
                molecule_id = int(
                    connection.execute(
                        "SELECT id FROM molecule WHERE canonical_smiles='CCO'"
                    ).fetchone()[0]
                )
                target_id = int(connection.execute("SELECT MAX(id) FROM target").fetchone()[0])
                activities = [
                    ("ZERO", "IC50=0 nM"),
                    ("NEGATIVE", "IC50=-1 nM"),
                    ("MISSING_UNIT", "IC50=5"),
                    ("CENSORED", "IC50<5 nM"),
                    ("NON_FINITE", "IC50=inf nM"),
                    ("VALID", "IC50=5 nM"),
                ]
                for offset, (identifier, activity) in enumerate(activities, start=1):
                    connection.execute(
                        "INSERT INTO target VALUES (?, 'ChEMBL', ?, ?, ?, 'Homo sapiens', "
                        "'known_human_protein', 'fixture-release', ?)",
                        (
                            target_id + offset,
                            identifier,
                            identifier,
                            identifier,
                            "c" * 64,
                        ),
                    )
                    connection.execute(
                        "INSERT INTO evidence VALUES (?, ?, 1.0, ?, 0, 1.0)",
                        (molecule_id, target_id + offset, activity),
                    )

            rows = similarity.query_targets(
                built["database"],
                "CCO",
                top_k=50,
                threshold=0.3,
                _validated_manifest=validated_manifest,
            )
            by_target = {row["target_id"]: row for row in rows}
            self.assertEqual(
                {"ZERO", "NEGATIVE", "MISSING_UNIT", "CENSORED", "NON_FINITE"}
                & by_target.keys(),
                set(),
            )
            self.assertEqual(by_target["VALID"]["activity_score"], 1.0)

    def test_query_manifest_is_validated_once_and_publishes_known_database_contract(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_cli_") as tmp:
            root = Path(tmp)
            built = _build(root, _write_source_databases(root), "index")
            query_manifest = _write_query_manifest(root, ["CCO", "CCCO"])
            output = root / "known_database_matches.tsv"
            original_runtime_validation = similarity._runtime_manifest
            calls = 0

            def counted_runtime_validation(*args, **kwargs):
                nonlocal calls
                calls += 1
                return original_runtime_validation(*args, **kwargs)

            with patch.object(similarity, "_runtime_manifest", side_effect=counted_runtime_validation):
                exit_code = similarity.main(
                    [
                        "query-targets",
                        "--database",
                        str(built["database"]),
                        "--manifest",
                        str(built["manifest_path"]),
                        "--query-manifest",
                        str(query_manifest),
                        "--output-tsv",
                        str(output),
                        "--top-k",
                        "5",
                        "--threshold",
                        "0.3",
                    ]
                )
            self.assertEqual(exit_code, 0)
            self.assertEqual(calls, 1)
            with output.open("r", encoding="utf-8", newline="") as handle:
                reader = csv.DictReader(handle, delimiter="\t")
                self.assertEqual("\t".join(reader.fieldnames or []), similarity.KNOWN_DATABASE_HEADER)
                rows = list(reader)
            self.assertTrue(rows)
            database_sha256 = built["manifest"]["database"]["sha256"]
            manifest_sha256 = _sha256(built["manifest_path"])
            build_identity = similarity._compact_build_identity(built["manifest"])
            for row in rows:
                self.assertIn("canonical_smiles=", row["matched_features"])
                self.assertIn("tanimoto=", row["matched_features"])
                self.assertIn(
                    f"compact_database_sha256={database_sha256}", row["evidence"]
                )
                self.assertIn(
                    f"compact_manifest_sha256={manifest_sha256}", row["evidence"]
                )
                self.assertIn(
                    f"compact_schema_version={similarity.SCHEMA_VERSION}", row["evidence"]
                )
                self.assertIn(
                    f"compact_build_identity={build_identity}", row["evidence"]
                )
                self.assertIn(
                    f"compact_database_sha256={database_sha256}",
                    row["source_row_provenance"],
                )
                self.assertIn(
                    f"compact_manifest_sha256={manifest_sha256}",
                    row["source_row_provenance"],
                )
                similarity_value = float(
                    row["matched_features"].split("tanimoto=", 1)[1].split(";", 1)[0]
                )
                structural = float(
                    row["matched_features"]
                    .split("structural_compatibility=", 1)[1]
                    .split(";", 1)[0]
                )
                activity_score = float(
                    row["matched_features"]
                    .split("activity_score=", 1)[1]
                    .split(";", 1)[0]
                )
                self.assertAlmostEqual(
                    float(row["match_score"]),
                    (0.85 * similarity_value + 0.15 * structural) * activity_score,
                    places=3,
                )
            binding = next(row for row in rows if row["source"] == "BindingDB")
            self.assertIn("bindingdb_onsite_activity_row", binding["evidence"])
            self.assertIn("pdb_ids=1ABC,2DEF,4JKL", binding["evidence"])
            raw_lines = output.read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(raw_lines), len(rows) + 1)
            self.assertTrue(all(len(line.split("\t")) == 14 for line in raw_lines))
            self.assertNotIn("\r", output.read_text(encoding="utf-8"))

    def test_features_json_cannot_escape_query_manifest_directory(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_escape_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            run_dir.mkdir()
            _write_query_context(
                run_dir,
                [("input_1", "record_1", "a" * 64)],
            )
            outside = root / "outside.json"
            outside.write_text("{}\n", encoding="utf-8")
            query_manifest = run_dir / "query_structure_manifest.tsv"
            query_manifest.write_text(
                "input_id\trecord_id\tinput_kind\tinput_digest\tmethod\tfeatures_json\t"
                "coordinate_artifact\tfeature_digest\tstatus\n"
                f"input_1\trecord_1\tsmall_molecule\t{'a' * 64}\t"
                f"{similarity.QUERY_METHOD_VERSION}\t../outside.json\t\t"
                f"{'b' * 64}\tready\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "query_structure_features_path_outside_run_dir",
            ):
                similarity._read_query_manifest(query_manifest)

    def test_failed_small_molecule_query_structure_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_failed_query_") as tmp:
            root = Path(tmp)
            query_manifest = _write_query_manifest(root, ["CCO"])
            with query_manifest.open("r", encoding="utf-8", newline="") as handle:
                reader = csv.DictReader(handle, delimiter="\t")
                fieldnames = list(reader.fieldnames or [])
                rows = list(reader)
            rows[0]["status"] = "failed"
            rows[0]["features_json"] = ""
            rows[0]["feature_digest"] = ""
            with query_manifest.open("w", encoding="utf-8", newline="") as handle:
                writer = csv.DictWriter(
                    handle, fieldnames=fieldnames, delimiter="\t", lineterminator="\n"
                )
                writer.writeheader()
                writer.writerows(rows)

            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "query_structure_expected_record_not_ready",
            ):
                similarity._read_query_manifest(query_manifest)

    def test_query_structure_is_bound_to_current_invocation_and_conversion_manifest(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_current_query_") as tmp:
            root = Path(tmp)
            query_manifest = _write_query_manifest(root, ["CCO"])
            invocation = root / "worker_invocation_id.txt"
            invocation.write_text("new_invocation\n", encoding="utf-8")
            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "query_structure_feature_identity_mismatch",
            ):
                similarity._read_query_manifest(query_manifest)

        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_conversion_query_") as tmp:
            root = Path(tmp)
            query_manifest = _write_query_manifest(root, ["CCO"])
            conversion = root / "input_conversion_manifest.tsv"
            conversion.write_text(
                conversion.read_text(encoding="utf-8") + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "query_structure_feature_identity_mismatch",
            ):
                similarity._read_query_manifest(query_manifest)

    def test_query_feature_swap_and_content_tamper_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_binding_") as tmp:
            root = Path(tmp)
            query_manifest = _write_query_manifest(root, ["CCO", "CCCO"])
            with query_manifest.open("r", encoding="utf-8", newline="") as handle:
                reader = csv.DictReader(handle, delimiter="\t")
                fieldnames = list(reader.fieldnames or [])
                rows = list(reader)
            rows[0]["features_json"], rows[1]["features_json"] = (
                rows[1]["features_json"],
                rows[0]["features_json"],
            )
            rows[0]["feature_digest"], rows[1]["feature_digest"] = (
                rows[1]["feature_digest"],
                rows[0]["feature_digest"],
            )
            with query_manifest.open("w", encoding="utf-8", newline="") as handle:
                writer = csv.DictWriter(
                    handle, fieldnames=fieldnames, delimiter="\t", lineterminator="\n"
                )
                writer.writeheader()
                writer.writerows(rows)
            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "query_structure_feature_identity_mismatch",
            ):
                similarity._read_query_manifest(query_manifest)

        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_tamper_") as tmp:
            root = Path(tmp)
            query_manifest = _write_query_manifest(root, ["CCO"])
            feature = root / "query_1" / "features.json"
            feature.write_text(
                feature.read_text(encoding="utf-8") + " ", encoding="utf-8"
            )
            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "query_structure_feature_digest_mismatch",
            ):
                similarity._read_query_manifest(query_manifest)

    def test_corrupt_database_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_corrupt_") as tmp:
            root = Path(tmp)
            built = _build(root, _write_source_databases(root), "index")
            with built["database"].open("r+b") as handle:
                handle.seek(200)
                byte = handle.read(1)
                handle.seek(200)
                handle.write(bytes([byte[0] ^ 0xFF]))
            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "database_hash_invalid|database_corrupt|quick_check_failed",
            ):
                similarity.validate_similarity_index(
                    built["database"], built["manifest_path"]
                )

    def test_same_size_database_tamper_invalidates_runtime_hash_cache(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_similarity_runtime_tamper_") as tmp:
            root = Path(tmp)
            built = _build(root, _write_source_databases(root), "index")
            similarity._cached_runtime_publication.cache_clear()
            first = similarity.query_targets(
                built["database"],
                "CCO",
                top_k=5,
                threshold=0.3,
                manifest_path=built["manifest_path"],
            )
            self.assertTrue(first)
            before = built["database"].stat()
            with built["database"].open("r+b") as handle:
                content = handle.read()
                marker = b"Exact target"
                offset = content.find(marker)
                self.assertGreaterEqual(offset, 0)
                handle.seek(offset)
                handle.write(b"Tampr target")
                handle.flush()
                os.fsync(handle.fileno())
            after = built["database"].stat()
            self.assertEqual(after.st_size, before.st_size)
            if after.st_mtime_ns == before.st_mtime_ns:
                os.utime(
                    built["database"],
                    ns=(after.st_atime_ns, after.st_mtime_ns + 1),
                )

            with self.assertRaisesRegex(
                similarity.LigandSimilarityIndexError,
                "ligand_similarity_database_hash_invalid",
            ):
                similarity.query_targets(
                    built["database"],
                    "CCO",
                    top_k=5,
                    threshold=0.3,
                    manifest_path=built["manifest_path"],
                )


def _write_source_databases(root: Path) -> dict[str, object]:
    source_digest = {"chembl": "a" * 64, "bindingdb": "b" * 64}
    source_release = {"chembl": "chembl-fixture-1", "bindingdb": "bindingdb-fixture-1"}
    chembl = root / "chembl_query.sqlite"
    bindingdb = root / "bindingdb_query.sqlite"

    chembl_rows = [
        ("CCO", "CHEMBL_EXACT", "CHEMBL_EXACT", "Exact target", "Homo sapiens", "IC50", "=", 200000.0, "nM"),
        ("CCCO", "CHEMBL_NEAR", "CHEMBL_NEAR", "Near target", "Homo sapiens", "Ki", "=", 1.0, "nM"),
        ("CCCO", "CHEMBL_NEAR", "CHEMBL_NEAR", "Near target", "Homo sapiens", "Ki", "=", 100.0, "nM"),
        ("CCO.[Na+]", "CHEMBL_SALT", "CHEMBL_SALT", "Salt target", "Mus musculus", "Kd", "=", 5.0, "nM"),
        ("CC(=O)N", "CHEMBL_TAUTOMER", "CHEMBL_TAUTOMER", "Tautomer target", "Homo sapiens", "Ki", "=", 20.0, "nM"),
        ("CC(O)=N", "CHEMBL_TAUTOMER", "CHEMBL_TAUTOMER", "Tautomer target", "Homo sapiens", "Ki", "=", 30.0, "nM"),
        ("c1ccccc1", "CHEMBL_RING", "CHEMBL_RING", "Ring target", "unknown", "Kd", ">", 500.0, "nM"),
    ]
    with sqlite3.connect(chembl) as connection:
        connection.executescript(
            """
            CREATE TABLE genoligand_onsite_activity (
                canonical_isomeric_smiles TEXT NOT NULL,
                target_id TEXT NOT NULL,
                source_target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                organism TEXT NOT NULL,
                compound_name TEXT NOT NULL,
                standard_type TEXT NOT NULL,
                standard_relation TEXT NOT NULL,
                standard_value REAL NOT NULL,
                standard_units TEXT NOT NULL,
                source_release TEXT NOT NULL,
                source_digest TEXT NOT NULL
            );
            CREATE INDEX idx_genoligand_onsite_activity_smiles
            ON genoligand_onsite_activity(canonical_isomeric_smiles);
            CREATE TABLE legacy_padding(payload BLOB NOT NULL);
            INSERT INTO legacy_padding VALUES (zeroblob(600000));
            """
        )
        connection.executemany(
            "INSERT INTO genoligand_onsite_activity VALUES (?, ?, ?, ?, ?, '', ?, ?, ?, ?, ?, ?)",
            [
                (*row, source_release["chembl"], source_digest["chembl"])
                for row in chembl_rows
            ],
        )

    binding_rows = [
        ("CCO", "BIND_EXACT", "Binding\t exact\r\nname", "Homo sapiens", "5", "Ki (nM)", "=", 5.0, 0, 1.0, "ETH", "1ABC,2DEF", "1ABC"),
        ("CCO", "BIND_EXACT", "Binding exact", "Homo sapiens", "100", "Ki (nM)", "=", 100.0, 0, 0.95, "ETH", "2DEF,4JKL", "4JKL"),
        ("CCN", "BIND_NEAR", "Binding near", "Homo sapiens", "25", "Kd (nM)", "=", 25.0, 0, 0.95, "", "", ""),
        ("CC(=O)N", "BIND_TAUTOMER", "Binding tautomer", "unknown", "50", "IC50 (nM)", "=", 50.0, 0, 0.95, "AMD", "3GHI", "3GHI"),
    ]
    with sqlite3.connect(bindingdb) as connection:
        connection.executescript(
            """
            CREATE TABLE genoligand_bindingdb_activity (
                source_row_id INTEGER PRIMARY KEY,
                canonical_smiles TEXT NOT NULL,
                target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                organism TEXT NOT NULL,
                activity TEXT NOT NULL,
                activity_type TEXT NOT NULL,
                relation TEXT NOT NULL,
                numeric_rank REAL NOT NULL,
                relation_rank INTEGER NOT NULL,
                activity_score REAL NOT NULL,
                ligand_code TEXT NOT NULL,
                pdb_ids TEXT NOT NULL,
                target_chain_pdb_ids TEXT NOT NULL
            );
            CREATE INDEX idx_bindingdb_activity_canonical_smiles
            ON genoligand_bindingdb_activity(canonical_smiles);
            CREATE TABLE genoligand_bindingdb_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE legacy_padding(payload BLOB NOT NULL);
            INSERT INTO legacy_padding VALUES (zeroblob(600000));
            """
        )
        connection.executemany(
            """INSERT INTO genoligand_bindingdb_activity(
                canonical_smiles,target_id,target_name,organism,activity,activity_type,
                relation,numeric_rank,relation_rank,activity_score,ligand_code,pdb_ids,
                target_chain_pdb_ids
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            binding_rows,
        )
        connection.executemany(
            "INSERT INTO genoligand_bindingdb_metadata VALUES (?, ?)",
            [
                ("schema_version", "genoligand_bindingdb_query_database_v1"),
                ("source_release", source_release["bindingdb"]),
                ("source_digest", source_digest["bindingdb"]),
            ],
        )

    chembl_manifest = root / "chembl_query.manifest.json"
    bindingdb_manifest = root / "bindingdb_query.manifest.json"
    _write_source_manifest(
        chembl,
        chembl_manifest,
        source_release["chembl"],
        source_digest["chembl"],
    )
    _write_source_manifest(
        bindingdb,
        bindingdb_manifest,
        source_release["bindingdb"],
        source_digest["bindingdb"],
    )
    return {
        "chembl_db": chembl,
        "chembl_manifest": chembl_manifest,
        "bindingdb_db": bindingdb,
        "bindingdb_manifest": bindingdb_manifest,
        "raw_smiles": [row[0] for row in chembl_rows] + [row[0] for row in binding_rows],
    }


def _write_source_manifest(
    database: Path, manifest: Path, release: str, digest: str
) -> None:
    manifest.write_text(
        json.dumps(
            {
                "status": "ready",
                "database_bytes": database.stat().st_size,
                "database_sha256": _sha256(database),
                "source_release": release,
                "source_digest": digest,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )


def _build(root: Path, sources: dict[str, object], name: str) -> dict[str, object]:
    database = root / f"{name}.sqlite"
    manifest_path = root / f"{name}.manifest.json"
    manifest = similarity.build_similarity_index(
        sources["chembl_db"],
        sources["bindingdb_db"],
        database,
        manifest_path,
        chembl_manifest=sources["chembl_manifest"],
        bindingdb_manifest=sources["bindingdb_manifest"],
    )
    return {"database": database, "manifest_path": manifest_path, "manifest": manifest}


def _write_query_manifest(root: Path, smiles_values: list[str]) -> Path:
    query_manifest = root / "query_structure_manifest.tsv"
    prepared = []
    for index, smiles in enumerate(smiles_values, start=1):
        fingerprint, canonical = similarity._fingerprint_from_smiles(smiles)
        prepared.append(
            (
                index,
                fingerprint,
                canonical,
                f"input_{index}",
                f"record_{index}",
                hashlib.sha256(f"record_{index}".encode()).hexdigest(),
            )
        )
    invocation_id = "invocation_fixture"
    _write_query_context(
        root,
        [(input_id, record_id, source_digest) for _, _, _, input_id, record_id, source_digest in prepared],
        invocation_id=invocation_id,
    )
    conversion_digest = _sha256(root / "input_conversion_manifest.tsv")
    lines = [
        "input_id\trecord_id\tinput_kind\tinput_digest\tmethod\tfeatures_json\t"
        "coordinate_artifact\tfeature_digest\tstatus"
    ]
    for index, fingerprint, canonical, input_id, record_id, source_digest in prepared:
        packed = bytes(DataStructs.BitVectToBinaryText(fingerprint))
        feature_dir = root / f"query_{index}"
        feature_dir.mkdir()
        feature_name = f"query_{index}/features.json"
        input_digest = hashlib.sha256(input_id.encode()).hexdigest()
        run_id = "run_fixture"
        structure = similarity._lightweight_structure_summary(canonical)
        three_dimensional = similarity._three_dimensional_structure_summary(canonical)
        if three_dimensional is None:
            raise AssertionError(f"fixture conformer generation failed: {canonical}")
        structure["shape"] = three_dimensional["shape"]
        feature = {
            "schema_version": similarity.QUERY_FEATURE_SCHEMA_VERSION,
            "status": "ready",
            "run_id": run_id,
            "job_id": f"job_{index}",
            "invocation_id": invocation_id,
            "input_id": input_id,
            "record_id": record_id,
            "input_kind": "small_molecule",
            "input_digest": input_digest,
            "source_content_sha256": source_digest,
            "conversion_manifest_sha256": conversion_digest,
            "method_version": similarity.QUERY_METHOD_VERSION,
            "canonical_smiles": canonical,
            "isomeric_smiles": canonical,
            "fingerprint": {
                "algorithm": similarity.FINGERPRINT_TYPE,
                "radius": similarity.FINGERPRINT_RADIUS,
                "n_bits": similarity.FINGERPRINT_BITS,
                "include_chirality": True,
                "encoding": "rdkit_binary_text_hex_v1",
                "packed_hex": packed.hex(),
                "population_count": int(fingerprint.GetNumOnBits()),
                "packed_sha256": hashlib.sha256(packed).hexdigest(),
            },
            **structure,
        }
        feature_path = root / feature_name
        feature_path.write_text(
            json.dumps(feature, sort_keys=True) + "\n", encoding="utf-8"
        )
        feature_digest = _sha256(feature_path)
        (feature_dir / "manifest.json").write_text(
            json.dumps(
                {
                    "schema_version": similarity.QUERY_ARTIFACT_SCHEMA_VERSION,
                    "status": "ready",
                    "run_id": run_id,
                    "job_id": f"job_{index}",
                    "invocation_id": invocation_id,
                    "input_id": input_id,
                    "record_id": record_id,
                    "input_kind": "small_molecule",
                    "input_digest": input_digest,
                    "source_content_sha256": source_digest,
                    "conversion_manifest_sha256": conversion_digest,
                    "method_version": similarity.QUERY_METHOD_VERSION,
                    "features_json": feature_name,
                    "feature_digest": feature_digest,
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        lines.append(
            f"{input_id}\t{record_id}\tsmall_molecule\t{input_digest}\t"
            f"{similarity.QUERY_METHOD_VERSION}\t{feature_name}\t\t{feature_digest}\tready"
        )
    query_manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return query_manifest


def _write_query_context(
    root: Path,
    identities: list[tuple[str, str, str]],
    *,
    invocation_id: str = "invocation_fixture",
) -> None:
    lines = [
        "input_id\trecord_id\tcanonical_kind\tconversion_status\tcontent_sha256"
    ]
    lines.extend(
        f"{input_id}\t{record_id}\tsmall_molecule\tconverted\t{source_digest}"
        for input_id, record_id, source_digest in identities
    )
    (root / "input_conversion_manifest.tsv").write_text(
        "\n".join(lines) + "\n",
        encoding="utf-8",
    )
    (root / "worker_invocation_id.txt").write_text(
        invocation_id + "\n",
        encoding="utf-8",
    )


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


if __name__ == "__main__":
    unittest.main()
