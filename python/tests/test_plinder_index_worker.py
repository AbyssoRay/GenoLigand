from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

from genoligand_workers.plinder_index_worker import (
    CANONICAL_HEADER,
    MAX_BATCH_SIZE,
    PlinderIndexError,
    build_plinder_canonical,
    inspect_plinder_parquet,
)


class PlinderIndexWorkerTests(unittest.TestCase):
    def test_real_parquet_values_are_published_with_bounded_batches(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plinder_") as tmp:
            root = Path(tmp)
            source = root / "annotation_table.parquet"
            row_count = MAX_BATCH_SIZE + 3
            pq.write_table(
                pa.table(
                    {
                        "entry_pdb_id": ["3grt"] * row_count,
                        "system_id": [f"3grt__1__system_{index}" for index in range(row_count)],
                        "ligand_ccd_code": ["FAD"] * row_count,
                        "ligand_rdkit_canonical_smiles": ["CC(=O)O"] * row_count,
                        "ligand_protein_chains_auth_id": [["A", "B"]] * row_count,
                        "ligand_auth_id": ["C"] * row_count,
                    }
                ),
                source,
                row_group_size=row_count,
            )
            canonical = root / "plinder_canonical.tsv"
            manifest_path = root / "plinder_canonical.manifest.json"

            manifest = build_plinder_canonical(
                source,
                canonical,
                manifest_path,
                batch_size=MAX_BATCH_SIZE * 2,
            )

            lines = canonical.read_text(encoding="utf-8").splitlines()
            self.assertEqual(lines[0], CANONICAL_HEADER.rstrip("\n"))
            self.assertEqual(len(lines) - 1, row_count)
            self.assertIn("\t3grt__1__system_0\tPLINDER 3grt system 3grt__1__system_0\t", lines[1])
            self.assertIn("\tstructure_record\t", lines[1])
            self.assertIn("plinder_2024-06_v2_parquet", lines[1])
            self.assertNotIn("annotation_table", lines[1])
            self.assertEqual(manifest["status"], "ready")
            self.assertEqual(manifest["canonical"]["row_count"], row_count)
            self.assertEqual(manifest["scan"]["batch_size"], MAX_BATCH_SIZE)
            self.assertLessEqual(manifest["scan"]["max_batch_rows"], MAX_BATCH_SIZE)
            self.assertGreaterEqual(manifest["scan"]["batch_count"], 2)
            self.assertEqual(manifest["scan"]["filtered_rows"], 0)
            self.assertEqual(
                manifest["canonical"]["sha256"],
                hashlib.sha256(canonical.read_bytes()).hexdigest(),
            )
            self.assertEqual(json.loads(manifest_path.read_text(encoding="utf-8")), manifest)
            self.assertFalse(list(root.glob("*.tmp")))

    def test_filtered_rows_are_accounted_for_in_manifest(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plinder_filtered_") as tmp:
            root = Path(tmp)
            source = root / "filtered.parquet"
            pq.write_table(
                pa.table(
                    {
                        "entry_pdb_id": ["1abc", ""],
                        "system_id": ["system_1", "system_2"],
                        "ligand_ccd_code": ["ATP", "FAD"],
                    }
                ),
                source,
            )

            manifest = build_plinder_canonical(
                source,
                root / "canonical.tsv",
                root / "manifest.json",
            )

            self.assertEqual(manifest["source"]["parquet_rows"], 2)
            self.assertEqual(manifest["canonical"]["row_count"], 1)
            self.assertEqual(manifest["scan"]["filtered_rows"], 1)

    def test_supported_aliases_are_resolved_into_manifest(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plinder_aliases_") as tmp:
            root = Path(tmp)
            source = root / "aliases.parquet"
            pq.write_table(
                pa.table(
                    {
                        "pdb_id": ["1abc"],
                        "complex_id": ["1abc_complex_1"],
                        "ligand_unique_ccd_code": ["ATP"],
                        "canonical_smiles": ["NC1=NC=NC2=C1N=CN2"],
                        "protein_chain_ids": [["A"]],
                        "ligand_chain_id": ["L"],
                    }
                ),
                source,
            )

            manifest = build_plinder_canonical(
                source,
                root / "canonical.tsv",
                root / "manifest.json",
            )

            self.assertEqual(
                manifest["schema_mapping"],
                {
                    "ligand_chain": "ligand_chain_id",
                    "ligand_id": "ligand_unique_ccd_code",
                    "ligand_smiles": "canonical_smiles",
                    "protein_chains": "protein_chain_ids",
                    "structure_id": "pdb_id",
                    "system_id": "complex_id",
                },
            )

    def test_corrupt_parquet_fails_without_publishing(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plinder_corrupt_") as tmp:
            root = Path(tmp)
            source = root / "broken.parquet"
            source.write_bytes(b"PAR1not-a-parquet-filePAR1")
            canonical = root / "canonical.tsv"
            manifest = root / "manifest.json"

            with self.assertRaisesRegex(PlinderIndexError, "invalid_plinder_parquet"):
                build_plinder_canonical(source, canonical, manifest)

            self.assertFalse(canonical.exists())
            self.assertFalse(manifest.exists())

    def test_missing_semantic_columns_fails_before_scanning(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plinder_schema_") as tmp:
            root = Path(tmp)
            source = root / "missing_columns.parquet"
            pq.write_table(pa.table({"entry_pdb_id": ["1abc"], "unrelated": ["value"]}), source)

            with self.assertRaisesRegex(
                PlinderIndexError,
                "missing_plinder_semantic_columns:ligand_id,system_id",
            ):
                build_plinder_canonical(source, root / "canonical.tsv", root / "manifest.json")

    def test_zero_valid_rows_fails_without_replacing_existing_publication(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plinder_empty_") as tmp:
            root = Path(tmp)
            source = root / "empty_values.parquet"
            pq.write_table(
                pa.table(
                    {
                        "entry_pdb_id": ["", None],
                        "system_id": ["", None],
                        "ligand_ccd_code": ["", None],
                    }
                ),
                source,
            )
            canonical = root / "canonical.tsv"
            manifest = root / "manifest.json"
            canonical.write_text("previous canonical\n", encoding="utf-8")
            manifest.write_text('{"status":"previous"}\n', encoding="utf-8")

            with self.assertRaisesRegex(PlinderIndexError, "zero_valid_plinder_rows"):
                build_plinder_canonical(source, canonical, manifest)

            self.assertEqual(canonical.read_text(encoding="utf-8"), "previous canonical\n")
            self.assertEqual(manifest.read_text(encoding="utf-8"), '{"status":"previous"}\n')
            self.assertFalse(list(root.glob("*.tmp")))

    def test_metadata_inspection_does_not_scan_rows(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plinder_inspect_") as tmp:
            root = Path(tmp)
            source = root / "inspect.parquet"
            pq.write_table(
                pa.table(
                    {
                        "entry_pdb_id": ["1abc"],
                        "system_id": ["1abc_system"],
                        "ligand_ccd_code": ["ATP"],
                    }
                ),
                source,
            )

            summary = inspect_plinder_parquet(source)

            self.assertEqual(summary["parquet_rows"], 1)
            self.assertEqual(summary["row_groups"], 1)
            self.assertEqual(summary["schema_mapping"]["structure_id"], "entry_pdb_id")
            self.assertEqual(summary["schema_mapping"]["system_id"], "system_id")
            self.assertEqual(summary["schema_mapping"]["ligand_id"], "ligand_ccd_code")


if __name__ == "__main__":
    unittest.main()
