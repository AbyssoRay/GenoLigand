from __future__ import annotations

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from genoligand_workers.alignment_worker import (
    FoldseekTargetDatabase,
    SequenceInput,
    StructureQuery,
    TargetDatabase,
    foldseek_rows,
    foldseek_target_databases,
    mmseqs_rows,
    read_sequence_inputs,
    run_foldseek_alignment,
    run_mmseqs_alignment,
    sequence_query_id,
    write_fasta,
)
from genoligand_workers.md_summary_worker import canonical_digest, run_md_summary_job


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

    @staticmethod
    def append_genome_sequence(run_dir: Path) -> None:
        path = run_dir / "input_conversion_manifest.tsv"
        with path.open("a", encoding="utf-8") as handle:
            handle.write(
                "input_2\tcontig_1\tgenome_or_metagenome\tgenome_or_metagenome\tgenome_or_metagenome\t"
                "ATGCGTGTGTAA\tconverted\tstandard_nucleotide_sequence\tfasta\tinline\t\t"
                + "b" * 64
                + "\trecognized\t\t\n"
            )

    @staticmethod
    def write_structure_query(run_dir: Path) -> StructureQuery:
        import hashlib

        run_dir.mkdir(parents=True, exist_ok=True)
        query = run_dir / "structure_viewer.pdb"
        query.write_text("ATOM      1  CA  GLY A   1       0.0  0.0  0.0\n", encoding="utf-8")
        digest = hashlib.sha256(query.read_bytes()).hexdigest()
        (run_dir / "input_conversion_manifest.tsv").write_text(
            "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\t"
            "format\tcontent_sha256\n"
            f"input-structure\trecord-structure\tcomplex\tstructure_viewer.pdb\tconverted\tpdb\t{digest}\n",
            encoding="utf-8",
        )
        return StructureQuery(
            query,
            "input-structure",
            "record-structure",
            f"query_{digest}",
            digest,
        )

    @staticmethod
    def write_foldseek_database(prefix: Path) -> None:
        prefix.parent.mkdir(parents=True, exist_ok=True)
        prefix.write_bytes(b"database")
        Path(f"{prefix}.dbtype").write_bytes(b"\x00\x00\x00\x00")
        Path(f"{prefix}.index").write_text("0\t0\t8\n", encoding="ascii")
        Path(f"{prefix}.lookup").write_text("0\t1ABC\t0\n", encoding="utf-8")

    @classmethod
    def write_foldseek_manifest(cls, store: Path, database_ids: tuple[str, ...]) -> None:
        manifest = store / "indexes/foldseek/foldseek_target_databases.tsv"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        rows = [
            "manifest_schema_version\tsource_id\tsource_namespace\tsource_class\t"
            "source_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n"
        ]
        for index, database_id in enumerate(database_ids, start=1):
            prefix = store / f"indexes/foldseek/{database_id}/target"
            cls.write_foldseek_database(prefix)
            rows.append(
                "genoligand_alignment_database_manifest_v2\t"
                f"{database_id}\tRCSB\tstructure\t{str(index) * 64}\t1\t8\tfoldseek-test-{index}\t"
                f"indexes/foldseek/{database_id}/target\tready\n"
            )
        manifest.write_text("".join(rows), encoding="utf-8")

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
                "input_1__record__alpha\ttarget_a\t80\t6\t1.0\t1.0\t1e-8\t60\t1\t6\t1\t6\t6M\n"
                "input_1__record__beta\ttarget_b\t75\t6\t1.0\t1.0\t1e-7\t55\t1\t6\t1\t6\t6M\n",
                encoding="utf-8",
            )
            rows = mmseqs_rows(
                raw,
                TargetDatabase(
                    "uniprot_test", "UniProt", "known", "a" * 64, "mmseqs-test", root / "known_targets"
                ),
                {
                    "input_1__record__alpha": SequenceInput(
                        "input_1__record__alpha", "input_1", "alpha", "protein_ligand", "MKTAYI"
                    ),
                    "input_1__record__beta": SequenceInput(
                        "input_1__record__beta", "input_1", "beta", "protein_ligand", "ACDEFG"
                    ),
                },
                "MMseqs2-test",
            )

            self.assertEqual(len(rows), 2)
            self.assertTrue(rows[0].startswith("input_1\tUniProt\t"))
            self.assertIn("\talpha\tprotein_ligand\t6\t1.0\t1.0\t6M\n", rows[0])
            self.assertIn("\tbeta\tprotein_ligand\t6\t1.0\t1.0\t6M\n", rows[1])
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

            self.write_structure_query(run_dir)
            missing_database = run_foldseek_alignment(run_dir, store)
            self.assertEqual(missing_database.status, "not_ready")
            self.assertTrue(missing_database.outcomes[0].required)

    def test_foldseek_manifest_is_authoritative_and_all_ready_databases_execute(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_foldseek_multi_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            query = self.write_structure_query(run_dir)
            self.write_foldseek_manifest(store, ("rcsb_primary", "plinder_structures"))
            commands: list[list[str]] = []

            def fake_run(command, capture_output=True, text=True, timeout=None):
                commands.append(command)
                if command[1] == "version":
                    return subprocess.CompletedProcess(command, 0, "Foldseek-test\n", "")
                if command[1] == "convertalis":
                    target = Path(command[3]).parent.name
                    Path(command[5]).write_text(
                        f"{query.query_id}\t{target}_hit\t40\t0.8\t1e-8\t60\n",
                        encoding="utf-8",
                    )
                return subprocess.CompletedProcess(command, 0, "", "")

            with patch("genoligand_workers.alignment_worker.shutil.which", return_value="foldseek"), patch(
                "genoligand_workers.alignment_worker.subprocess.run", side_effect=fake_run
            ):
                execution = run_foldseek_alignment(run_dir, store)

            self.assertEqual(execution.status, "completed")
            self.assertEqual(len(execution.outcomes), 2)
            self.assertEqual(
                {row.database_id for row in execution.outcomes},
                {"rcsb_primary", "plinder_structures"},
            )
            self.assertEqual(len([command for command in commands if command[1] == "search"]), 2)
            matches = (run_dir / "tool_outputs/foldseek_structure_matches.tsv").read_text(
                encoding="utf-8"
            )
            self.assertIn(
                "source_database_id\tsource_digest\tbuilder_version",
                matches.splitlines()[0],
            )
            self.assertIn("current_run_query_id\tquery_digest\ttm_score", matches.splitlines()[0])
            self.assertIn("input-structure", matches)
            self.assertIn("record-structure", matches)

    def test_foldseek_rejects_orphan_empty_and_unproven_databases(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_foldseek_strict_") as tmp:
            root = Path(tmp)
            store = root / "store"
            orphan = store / "indexes/foldseek/orphan/target"
            orphan.parent.mkdir(parents=True)
            Path(f"{orphan}.dbtype").write_bytes(b"\x00\x00\x00\x00")
            self.assertEqual(foldseek_target_databases(store), [])

            required = (
                "manifest_schema_version",
                "source_namespace",
                "source_class",
                "source_digest",
                "source_files",
                "source_bytes",
                "builder_version",
                "db_path",
            )
            for field in required:
                with self.subTest(field=field):
                    self.write_foldseek_manifest(store, ("rcsb_primary",))
                    manifest = store / "indexes/foldseek/foldseek_target_databases.tsv"
                    lines = manifest.read_text(encoding="utf-8").splitlines()
                    header = lines[0].split("\t")
                    row = lines[1].split("\t")
                    row[header.index(field)] = ""
                    manifest.write_text("\t".join(header) + "\n" + "\t".join(row) + "\n", encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, "foldseek_target_manifest_fields_missing"):
                        foldseek_target_databases(store)

            self.write_foldseek_manifest(store, ("rcsb_primary",))
            manifest = store / "indexes/foldseek/foldseek_target_databases.tsv"
            manifest.write_text(
                manifest.read_text(encoding="utf-8").replace(
                    "genoligand_alignment_database_manifest_v2",
                    "genoligand_alignment_database_manifest_v1",
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "foldseek_target_manifest_schema_invalid"):
                foldseek_target_databases(store)

            self.write_foldseek_manifest(store, ("rcsb_primary",))
            target = store / "indexes/foldseek/rcsb_primary/target"
            target.write_bytes(b"")
            with self.assertRaisesRegex(ValueError, "foldseek_target_database_not_ready"):
                foldseek_target_databases(store)

    def test_foldseek_tm_score_changes_structure_match_score(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_foldseek_tm_") as tmp:
            root = Path(tmp)
            raw = root / "matches.m8"
            raw.write_text(
                "query-current\ttarget-low\t40\t0.1\t1e-8\t60\n"
                "query-current\ttarget-high\t40\t0.9\t1e-8\t60\n",
                encoding="utf-8",
            )
            target = FoldseekTargetDatabase(
                "rcsb", "a" * 64, 1, 8, "Foldseek-test", root / "target"
            )
            query = StructureQuery(
                root / "query.pdb",
                "input-current",
                "record-current",
                "query-current",
                "b" * 64,
            )
            rows = foldseek_rows(raw, target, query, "Foldseek-test")
            low = float(rows[0].split("\t")[6])
            high = float(rows[1].split("\t")[6])
            self.assertGreater(high, low)

    def test_mmseqs_mixed_database_failure_fails_stage_and_keeps_each_outcome(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_mixed_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            query_id = self.write_sequence_manifest(run_dir)
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(
                "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\t"
                "source_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n"
                f"known_db\tgenoligand_alignment_database_manifest_v2\tUniProt\tknown\t{'a' * 64}\t1\t8\tmmseqs-test\tindexes/mmseqs/known/target\tready\n"
                f"broken_db\tgenoligand_alignment_database_manifest_v2\tBioLiP2\tknown\t{'b' * 64}\t1\t8\tmmseqs-test\tindexes/mmseqs/broken/target\tready\n",
                encoding="utf-8",
            )
            for database_id in ("known", "broken"):
                prefix = store / f"indexes/mmseqs/{database_id}/target"
                prefix.parent.mkdir(parents=True)
                prefix.write_bytes(b"database")
                Path(f"{prefix}.dbtype").write_bytes(b"\x00\x00\x00\x00")

            def fake_run(command, capture_output=True, text=True, timeout=None):
                operation = command[1]
                if operation == "version":
                    return subprocess.CompletedProcess(command, 0, "MMseqs2-test\n", "")
                if operation == "search" and "broken" in command[3]:
                    return subprocess.CompletedProcess(command, 1, "", "fixture search failed")
                if operation == "convertalis":
                    Path(command[5]).write_text(
                        f"{query_id}\ttarget_a\t80\t6\t1.0\t1.0\t1e-8\t60\t1\t6\t1\t6\t6M\n",
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

    def test_mmseqs_builds_typed_query_databases_and_requires_alignment_backtrace(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_typed_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            protein_query_id = self.write_sequence_manifest(run_dir)
            self.append_genome_sequence(run_dir)
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(
                "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\t"
                "source_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n"
                f"uniprot_human\tgenoligand_alignment_database_manifest_v2\tUniProt\tknown\t{'a' * 64}\t1\t8\tmmseqs-test\tindexes/mmseqs/uniprot/target\tready\n",
                encoding="utf-8",
            )
            target = store / "indexes/mmseqs/uniprot/target"
            target.parent.mkdir(parents=True)
            target.write_bytes(b"database")
            Path(f"{target}.dbtype").write_bytes(b"\x00\x00\x00\x00")
            commands: list[list[str]] = []

            def fake_run(command, capture_output=True, text=True, timeout=None):
                commands.append(command)
                if command[1] == "version":
                    return subprocess.CompletedProcess(command, 0, "MMseqs2-test\n", "")
                if command[1] == "convertalis":
                    query_id = protein_query_id
                    query_fasta = Path(command[2]).name
                    if "nucleotide" in query_fasta:
                        query_id = next(
                            line[1:]
                            for line in (run_dir / "tool_outputs/mmseqs_query_nucleotide.fasta")
                            .read_text(encoding="utf-8")
                            .splitlines()
                            if line.startswith(">")
                        )
                    if "translated_protein" in query_fasta:
                        query_id = next(
                            line[1:]
                            for line in (run_dir / "tool_outputs/mmseqs_query_translated_protein.fasta")
                            .read_text(encoding="utf-8")
                            .splitlines()
                            if line.startswith(">")
                        )
                    Path(command[5]).write_text(
                        f"{query_id}\ttarget_a\t80\t6\t1.0\t1.0\t1e-8\t60\t1\t6\t1\t6\t6M\n",
                        encoding="utf-8",
                    )
                return subprocess.CompletedProcess(command, 0, "", "")

            with patch("genoligand_workers.alignment_worker.shutil.which", return_value="mmseqs"), patch(
                "genoligand_workers.alignment_worker.subprocess.run", side_effect=fake_run
            ):
                execution = run_mmseqs_alignment(run_dir, store)

            self.assertEqual(execution.status, "completed")
            createdb = [command for command in commands if command[1] == "createdb"]
            self.assertEqual(len(createdb), 3)
            created_types = {Path(command[3]).name: command[command.index("--dbtype") + 1] for command in createdb}
            self.assertEqual(created_types["mmseqs_query_protein_db"], "1")
            self.assertEqual(created_types["mmseqs_query_nucleotide_db"], "2")
            self.assertEqual(created_types["mmseqs_query_translated_protein_db"], "1")
            searches = [command for command in commands if command[1] == "search"]
            self.assertEqual(len(searches), 3)
            self.assertTrue(all("-a" in command for command in searches))
            search_types = {Path(command[2]).name: command[command.index("--search-type") + 1] for command in searches}
            self.assertEqual(search_types["mmseqs_query_protein_db"], "1")
            self.assertEqual(search_types["mmseqs_query_nucleotide_db"], "2")
            self.assertEqual(search_types["mmseqs_query_translated_protein_db"], "1")
            output = (run_dir / "tool_outputs/mmseqs_alignment_matches.tsv").read_text(encoding="utf-8")
            self.assertIn("source_database_id\tsource_namespace\tsource_class", output.splitlines()[0])
            self.assertIn("\tuniprot_human\tUniProt\tknown\t", output)
            self.assertIn(f"\t{'a' * 64}\tmmseqs-test\tMMseqs2-test\t", output)

    def test_mmseqs_rejects_rows_without_real_alignment_backtrace(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_backtrace_") as tmp:
            root = Path(tmp)
            raw = root / "matches.m8"
            raw.write_text("query_1\ttarget_a\t80\t6\t1e-8\t60\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "alignment_backtrace"):
                mmseqs_rows(
                    raw,
                    TargetDatabase(
                        "uniprot_test",
                        "UniProt",
                        "known",
                        "a" * 64,
                        "mmseqs-test",
                        root / "known_targets",
                    ),
                    {
                        "query_1": SequenceInput(
                            "query_1", "input_1", "record_1", "protein_ligand", "MKTAYI"
                        )
                    },
                    "MMseqs2-test",
                )

    def test_mmseqs_rejects_ready_target_without_explicit_provenance(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_provenance_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            self.write_sequence_manifest(run_dir)
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(
                "source_id\tdb_path\tstatus\n"
                "uniprot_human\tindexes/mmseqs/uniprot/target\tready\n",
                encoding="utf-8",
            )
            target = store / "indexes/mmseqs/uniprot/target"
            target.parent.mkdir(parents=True)
            target.write_bytes(b"database")
            Path(f"{target}.dbtype").write_bytes(b"\x00\x00\x00\x00")

            with self.assertRaisesRegex(ValueError, "target_manifest_fields_missing"):
                run_mmseqs_alignment(run_dir, store)

    def test_mmseqs_ready_target_with_missing_database_fails_instead_of_being_skipped(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_missing_ready_db_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            store = root / "store"
            self.write_sequence_manifest(run_dir)
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(
                "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\t"
                "source_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\n"
                f"uniprot_human\tgenoligand_alignment_database_manifest_v2\tUniProt\tknown\t{'a' * 64}\t1\t8\tmmseqs-test\tindexes/mmseqs/missing/target\tready\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "target_database_not_ready:uniprot_human"):
                run_mmseqs_alignment(run_dir, store)

    def test_mmseqs_accepts_reverse_coordinates_and_penalizes_low_coverage(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_mmseqs_reverse_coverage_") as tmp:
            root = Path(tmp)
            raw = root / "matches.m8"
            raw.write_text(
                "query_high\ttarget_a\t80\t60\t1.0\t1.0\t1e-8\t60\t60\t1\t1\t60\t60M\n"
                "query_low\ttarget_b\t80\t6\t0.1\t0.1\t1e-8\t60\t6\t1\t1\t6\t6M\n",
                encoding="utf-8",
            )
            target = TargetDatabase(
                "uniprot_test",
                "UniProt",
                "known",
                "a" * 64,
                "mmseqs-test",
                root / "known_targets",
            )
            identities = {
                "query_high": SequenceInput(
                    "query_high", "input_1", "record_1", "nucleotide", "ATGCGT"
                ),
                "query_low": SequenceInput(
                    "query_low", "input_1", "record_1", "nucleotide", "ATGCGT"
                ),
            }

            rows = mmseqs_rows(raw, target, identities, "MMseqs2-test")

            high_score = float(rows[0].split("\t")[6])
            low_score = float(rows[1].split("\t")[6])
            self.assertGreater(high_score, low_score)

    def test_md_outcomes_distinguish_no_input_from_missing_bound_candidate(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_md_outcome_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            run_dir.mkdir()
            input_manifest = root / "input.json"
            input_manifest.write_text(json.dumps({"inputs": [], "files": []}), encoding="utf-8")
            job = {
                "job_id": "md_summary_ligand_dynamics",
                "invocation_id": "invocation-current",
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
            missing_candidate = run_md_summary_job(job)
            self.assertEqual(missing_candidate.status, "failed")
            self.assertTrue(missing_candidate.outcomes[0].required)
            self.assertEqual(missing_candidate.outcomes[0].reason, "candidate_id_missing")

    def test_md_outcome_reports_missing_receptor_after_candidate_binding(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_md_missing_receptor_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            tool_outputs = run_dir / "tool_outputs"
            tool_outputs.mkdir(parents=True)
            candidate_id = "candidate-current"
            input_id = "input-current"
            source_digest = "a" * 64
            input_digest = canonical_digest(
                {
                    "input_id": input_id,
                    "record_id": "record-current",
                    "canonical_kind": "small_molecule",
                    "canonical_value": "CCO",
                    "source_content_sha256": source_digest,
                }
            )
            input_manifest = root / "input.json"
            input_manifest.write_text(
                json.dumps({"inputs": [{"field": "small_molecule", "value": "CCO"}], "files": []}),
                encoding="utf-8",
            )
            (run_dir / "complex_candidate_selection.tsv").write_text(
                "candidate_id\tinput_id\trecord_id\n"
                f"{candidate_id}\t{input_id}\trecord-current\n",
                encoding="utf-8",
            )
            candidate_digest = canonical_digest(
                {
                    "candidate_id": candidate_id,
                    "input_id": input_id,
                    "record_id": "record-current",
                }
            )
            (run_dir / "input_conversion_manifest.tsv").write_text(
                "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\tcontent_sha256\n"
                f"{input_id}\trecord-current\tsmall_molecule\tCCO\tconverted\t{source_digest}\n",
                encoding="utf-8",
            )
            docking_artifacts = {
                "receptor_pdbqt": "tool_outputs/docking_receptor.pdbqt",
                "ligand_sdf": "tool_outputs/docking_ligand.sdf",
                "ligand_pdbqt": "tool_outputs/docking_ligand.pdbqt",
                "pose": "tool_outputs/docking_pose.pdbqt",
                "pose_sdf": "tool_outputs/docking_pose.sdf",
                "log": "tool_outputs/docking_vina.log",
            }
            for artifact_name in docking_artifacts.values():
                (run_dir / artifact_name).write_text(f"fixture:{artifact_name}\n", encoding="utf-8")
            docking_manifest = tool_outputs / "docking_manifest.json"
            docking_manifest.write_text(
                json.dumps(
                    {
                        "artifact_schema_version": "genoligand_docking_artifacts_v1",
                        "status": "completed",
                        "job_id": "docking_vina",
                        "run_id": run_dir.name,
                        "invocation_id": "invocation-current",
                        "candidate_id": candidate_id,
                        "candidate_digest": candidate_digest,
                        "input_id": input_id,
                        "record_id": "record-current",
                        "input_digest": input_digest,
                        "source_content_sha256": source_digest,
                        "seed_provenance": {"input_digest": input_digest},
                        **docking_artifacts,
                    }
                ),
                encoding="utf-8",
            )
            job = {
                "job_id": "md_summary_ligand_dynamics",
                "run_id": run_dir.name,
                "invocation_id": "invocation-current",
                "input_manifest": str(input_manifest),
                "run_dir": str(run_dir),
                "science_store": str(root / "store"),
                "candidate_id": candidate_id,
                "candidate_digest": candidate_digest,
                "input_id": input_id,
                "record_id": "record-current",
                "input_digest": input_digest,
                "source_digest": source_digest,
                "docking_manifest": "tool_outputs/docking_manifest.json",
                "docking_manifest_digest": hashlib.sha256(
                    docking_manifest.read_bytes()
                ).hexdigest(),
                "docking_job_id": "docking_vina",
                "docking_invocation_id": "invocation-current",
                "platform_policy": "disabled",
            }

            missing_receptor = run_md_summary_job(job)

            self.assertEqual(missing_receptor.status, "failed")
            self.assertTrue(missing_receptor.outcomes[0].required)
            self.assertEqual(
                missing_receptor.outcomes[0].reason,
                "docking_artifact_missing:receptor_coordinate",
            )


if __name__ == "__main__":
    unittest.main()
