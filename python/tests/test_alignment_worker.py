from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from genoligand_workers.alignment_worker import (
    mmseqs_rows,
    read_sequence_inputs,
    run_foldseek_alignment,
    run_mmseqs_alignment,
    sequence_query_id,
    write_fasta,
)
from genoligand_workers.md_summary_worker import run_md_summary_job


class AlignmentRecordIdentityTests(unittest.TestCase):
    @staticmethod
    def write_sequence_manifest(run_dir: Path) -> str:
        run_dir.mkdir(parents=True, exist_ok=True)
        query_id = sequence_query_id("input_1", "record_1")
        (run_dir / "input_conversion_manifest.tsv").write_text(
            "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\t"
            "canonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\t"
            "source_path\tcontent_sha256\treason\tartifact\trisk_flags\n"
            "input_1\trecord_1\tprotein_ligand\tprotein_ligand\tprotein_ligand\tMKTAYI\t"
            "converted\tstandard_amino_acid_sequence\tfasta\tinline\t\t"
            + "a" * 64
            + "\trecognized\t\t\n",
            encoding="utf-8",
        )
        return query_id

    def test_multi_record_queries_keep_input_and_record_identity(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_alignment_identity_") as tmp:
            run_dir = Path(tmp)
            (run_dir / "input_conversion_manifest.tsv").write_text(
                "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\t"
                "canonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\t"
                "source_path\tcontent_sha256\treason\tartifact\trisk_flags\n"
                "input_1\talpha\tprotein_ligand\tprotein_ligand\tprotein_ligand\tMKTAYI\t"
                "converted\tstandard_amino_acid_sequence\tfasta\tfile\tqueries.fasta\t"
                + "a" * 64
                + "\trecognized\t\t\n"
                "input_1\tbeta\tprotein_ligand\tprotein_ligand\tprotein_ligand\tACDEFG\t"
                "converted\tstandard_amino_acid_sequence\tfasta\tfile\tqueries.fasta\t"
                + "a" * 64
                + "\trecognized\t\t\n",
                encoding="utf-8",
            )

            rows = read_sequence_inputs(run_dir)
            query = run_dir / "query.fasta"
            write_fasta(query, rows)
            text = query.read_text(encoding="utf-8")

            self.assertEqual(len(rows), 2)
            self.assertIn(f">{sequence_query_id('input_1', 'alpha')}\nMKTAYI\n", text)
            self.assertIn(f">{sequence_query_id('input_1', 'beta')}\nACDEFG\n", text)
            self.assertEqual(text.count(">"), 2)

    def test_mmseqs_rows_restore_separate_record_identity(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_identity_") as tmp:
            root = Path(tmp)
            raw = root / "matches.m8"
            raw.write_text(
                "input_1__record__alpha\ttarget_a\t80\t6\t1e-8\t60\n"
                "input_1__record__beta\ttarget_b\t75\t6\t1e-7\t55\n",
                encoding="utf-8",
            )
            rows = mmseqs_rows(
                raw,
                root / "known_targets",
                {
                    "input_1__record__alpha": ("input_1", "alpha"),
                    "input_1__record__beta": ("input_1", "beta"),
                },
            )

            self.assertEqual(len(rows), 2)
            self.assertTrue(rows[0].startswith("input_1\tMMseqs2\t"))
            self.assertTrue(rows[0].endswith("\talpha\n"))
            self.assertTrue(rows[1].endswith("\tbeta\n"))
            self.assertIn("query_id=input_1__record__alpha", rows[0])

    def test_coordinate_content_is_not_converted_to_a_sequence_query(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_alignment_coordinate_") as tmp:
            run_dir = Path(tmp)
            (run_dir / "input_conversion_manifest.tsv").write_text(
                "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\t"
                "canonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\t"
                "source_path\tcontent_sha256\treason\tartifact\trisk_flags\n"
                "input_1\trecord_1\tprotein_ligand\tprotein_ligand\tprotein_ligand\t"
                "ATOM N GLY A 1\tconverted\topenmm_structure_file\tpdb\tfile\tquery.pdb\t"
                + "a" * 64
                + "\trecognized\t\t\n",
                encoding="utf-8",
            )

            self.assertEqual(read_sequence_inputs(run_dir), [])

    def test_sanitized_record_ids_cannot_collide(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_alignment_collision_") as tmp:
            run_dir = Path(tmp)
            (run_dir / "input_conversion_manifest.tsv").write_text(
                "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\t"
                "canonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\t"
                "source_path\tcontent_sha256\treason\tartifact\trisk_flags\n"
                "input_1\tA:B\tprotein_ligand\tprotein_ligand\tprotein_ligand\tMKTAYI\t"
                "converted\tstandard_amino_acid_sequence\tfasta\tfile\tqueries.fasta\t"
                + "a" * 64
                + "\trecognized\t\t\n"
                "input_1\tA/B\tprotein_ligand\tprotein_ligand\tprotein_ligand\tACDEFG\t"
                "converted\tstandard_amino_acid_sequence\tfasta\tfile\tqueries.fasta\t"
                + "a" * 64
                + "\trecognized\t\t\n",
                encoding="utf-8",
            )

            rows = read_sequence_inputs(run_dir)
            self.assertEqual(len(rows), 2)
            self.assertNotEqual(rows[0][0], rows[1][0])

    def test_mmseqs_distinguishes_not_applicable_from_missing_database(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_outcome_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            no_input = run_mmseqs_alignment(run_dir, store)
            self.assertEqual(no_input.status, "not_applicable")
            self.assertEqual(no_input.result_status, "NOT_APPLICABLE")
            self.assertFalse(no_input.outcomes[0].required)

            self.write_sequence_manifest(run_dir)
            missing_database = run_mmseqs_alignment(run_dir, store)
            self.assertEqual(missing_database.status, "not_ready")
            self.assertEqual(missing_database.result_status, "NOT_READY")
            self.assertTrue(missing_database.outcomes[0].required)

    def test_foldseek_distinguishes_not_applicable_from_missing_database(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_foldseek_outcome_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            no_structure = run_foldseek_alignment(run_dir, store)
            self.assertEqual(no_structure.status, "not_applicable")
            self.assertFalse(no_structure.outcomes[0].required)

            run_dir.mkdir(parents=True, exist_ok=True)
            (run_dir / "structure_viewer.pdb").write_text("ATOM\n", encoding="utf-8")
            missing_database = run_foldseek_alignment(run_dir, store)
            self.assertEqual(missing_database.status, "not_ready")
            self.assertTrue(missing_database.outcomes[0].required)

    def test_mmseqs_mixed_database_failure_fails_stage_and_keeps_each_outcome(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_mixed_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            query_id = self.write_sequence_manifest(run_dir)
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(
                "source_id\tdb_path\tstatus\n"
                "known_db\tindexes/mmseqs/known/target\tready\n"
                "broken_db\tindexes/mmseqs/broken/target\tready\n",
                encoding="utf-8",
            )
            for database_id in ("known", "broken"):
                prefix = store / f"indexes/mmseqs/{database_id}/target"
                prefix.parent.mkdir(parents=True)
                Path(f"{prefix}.dbtype").write_bytes(b"\x00\x00\x00\x00")

            def fake_run(command, capture_output=True, text=True, timeout=None):
                operation = command[1]
                if operation == "search" and "broken" in command[3]:
                    return subprocess.CompletedProcess(command, 1, "", "fixture search failed")
                if operation == "convertalis":
                    Path(command[5]).write_text(
                        f"{query_id}\ttarget_a\t80\t6\t1e-8\t60\n",
                        encoding="utf-8",
                    )
                return subprocess.CompletedProcess(command, 0, "", "")

            with patch("genoligand_workers.alignment_worker.shutil.which", return_value="mmseqs"), patch(
                "genoligand_workers.alignment_worker.subprocess.run", side_effect=fake_run
            ):
                execution = run_mmseqs_alignment(run_dir, store)

            self.assertEqual(execution.status, "failed")
            self.assertEqual(execution.result_status, "FAIL")
            outcomes = {outcome.database_id: outcome for outcome in execution.outcomes}
            self.assertEqual(outcomes["known_db"].status, "completed")
            self.assertEqual(outcomes["known_db"].row_count, 1)
            self.assertEqual(outcomes["broken_db"].status, "failed")
            self.assertTrue(outcomes["broken_db"].required)
            matches = (run_dir / "tool_outputs/mmseqs_alignment_matches.tsv").read_text(encoding="utf-8")
            self.assertIn("target_a", matches)

    def test_md_outcomes_distinguish_no_input_from_missing_receptor(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_md_outcome_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            run_dir.mkdir()
            input_manifest = root / "input.json"
            input_manifest.write_text(json.dumps({"inputs": [], "files": []}), encoding="utf-8")
            job = {
                "input_manifest": str(input_manifest),
                "run_dir": str(run_dir),
                "science_store": str(root / "store"),
            }
            no_input = run_md_summary_job(job)
            self.assertEqual(no_input.status, "not_applicable")
            self.assertFalse(no_input.outcomes[0].required)

            input_manifest.write_text(
                json.dumps({"inputs": [{"field": "small_molecule", "value": "CCO"}], "files": []}),
                encoding="utf-8",
            )
            missing_receptor = run_md_summary_job(job)
            self.assertEqual(missing_receptor.status, "not_ready")
            self.assertTrue(missing_receptor.outcomes[0].required)
            self.assertEqual(missing_receptor.outcomes[0].reason, "missing_receptor_coordinate")


if __name__ == "__main__":
    unittest.main()
