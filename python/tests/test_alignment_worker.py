from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from genoligand_workers.alignment_worker import (
    mmseqs_rows,
    read_sequence_inputs,
    sequence_query_id,
    write_fasta,
)


class AlignmentRecordIdentityTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
