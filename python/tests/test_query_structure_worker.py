from __future__ import annotations

import csv
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from genoligand_workers.query_structure_worker import (
    MAX_COORDINATE_BYTES,
    MAX_FEATURE_BYTES,
    METHOD_VERSION,
    TAUTOMER_MAX_TAUTOMERS,
    TAUTOMER_MAX_TRANSFORMS,
    query_structure_input_digest,
    run_query_structure_job,
    write_worker_result_atomic,
)


CONVERSION_HEADER = (
    "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\t"
    "canonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\t"
    "source_path\tcontent_sha256\treason\tartifact\trisk_flags\tstructure_atom_count\t"
    "structure_chain_count\tstructure_residue_count\n"
)


def encode_tsv(value: str) -> str:
    return (
        value.replace("\\", "\\\\")
        .replace("\t", "\\t")
        .replace("\r", "\\r")
        .replace("\n", "\\n")
    )


class QueryStructureWorkerTests(unittest.TestCase):
    def make_run(
        self,
        root: Path,
        *,
        input_kind: str,
        canonical_value: str,
        input_format: str = "plain_text",
        source_kind: str = "inline",
        source_path: str = "",
        artifact: str = "",
        source_digest: str | None = None,
        counts: tuple[int, int, int] = (0, 0, 0),
        input_id: str = "input_1",
        record_id: str = "record_1",
    ) -> dict[str, object]:
        root.mkdir(parents=True, exist_ok=True)
        raw_manifest = root / "input.json"
        raw_manifest.write_text('{"inputs":[]}\n', encoding="utf-8")
        (root / "science_store").mkdir(exist_ok=True)
        if source_digest is None:
            source_digest = hashlib.sha256(canonical_value.encode("utf-8")).hexdigest()
        row = [
            input_id,
            record_id,
            input_kind,
            input_kind,
            input_kind,
            encode_tsv(canonical_value),
            "converted",
            "test_conversion",
            input_format,
            source_kind,
            encode_tsv(source_path),
            source_digest,
            "recognized",
            encode_tsv(artifact),
            "",
            str(counts[0]),
            str(counts[1]),
            str(counts[2]),
        ]
        (root / "input_conversion_manifest.tsv").write_text(
            CONVERSION_HEADER + "\t".join(row) + "\n",
            encoding="utf-8",
        )
        input_digest = query_structure_input_digest(
            input_id,
            record_id,
            input_kind,
            canonical_value.strip(),
            source_digest,
        )
        return {
            "schema_version": "genoligand_worker_current_v2",
            "pipeline_mode": "genoligand_scientific_pipeline",
            "job_id": f"query_structure_prepare__{input_id}__{record_id}",
            "worker": "query_structure_prepare",
            "stage": "query_structure_prepare",
            "run_id": "run_test",
            "invocation_id": "invocation_test",
            "rust_owned_decision_logic": True,
            "decision_owner": "GenoLigand Rust core",
            "input_manifest": str(raw_manifest.resolve()),
            "run_dir": str(root.resolve()),
            "science_store": str((root / "science_store").resolve()),
            "expected_result_schema": "genoligand_worker_current_v2",
            "input_id": input_id,
            "record_id": record_id,
            "input_kind": input_kind,
            "input_digest": input_digest,
            "source_digest": source_digest,
        }

    @staticmethod
    def index_row(root: Path) -> dict[str, str]:
        with (root / "query_structure_manifest.tsv").open(
            "r", encoding="utf-8", newline=""
        ) as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        if len(rows) != 1:
            raise AssertionError(f"expected one query structure row, found {len(rows)}")
        return rows[0]

    def test_module_entrypoint_writes_worker_result(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_entrypoint_") as tmp:
            root = Path(tmp)
            job = self.make_run(
                root,
                input_kind="genome_or_metagenome",
                canonical_value="ACGT",
            )
            job_path = root / "job.json"
            result_path = root / "result.json"
            job_path.write_text(json.dumps(job, sort_keys=True) + "\n", encoding="utf-8")
            env = os.environ.copy()
            python_root = str(Path(__file__).resolve().parents[1])
            env["PYTHONPATH"] = os.pathsep.join(
                value for value in (python_root, env.get("PYTHONPATH", "")) if value
            )
            completed = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "genoligand_workers.query_structure_worker",
                    "--job",
                    str(job_path),
                    "--out",
                    str(result_path),
                ],
                capture_output=True,
                check=False,
                env=env,
                text=True,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            result = json.loads(result_path.read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "NOT_APPLICABLE")
            self.assertEqual(result["execution_status"], "not_applicable")
            self.assertEqual(
                result["reason"],
                "nucleotide_input_has_no_direct_3d_query_structure",
            )

    def test_small_molecule_is_deterministic_compact_and_retains_one_coordinate(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_molecule_") as tmp:
            root = Path(tmp)
            job = self.make_run(root, input_kind="small_molecule", canonical_value="C[C@H](O)F")

            first = run_query_structure_job(job)
            row = self.index_row(root)
            features_path = root / row["features_json"]
            coordinate_path = root / row["coordinate_artifact"]
            features = json.loads(features_path.read_text(encoding="utf-8"))
            first_digest = hashlib.sha256(features_path.read_bytes()).hexdigest()

            self.assertEqual(first.status, "completed")
            self.assertEqual(row["status"], "ready")
            self.assertEqual(row["method"], METHOD_VERSION)
            self.assertEqual(features["canonical_smiles"], "CC(O)F")
            self.assertEqual(features["isomeric_smiles"], "C[C@H](O)F")
            self.assertEqual(features["fingerprint"]["encoding"], "rdkit_binary_text_hex_v1")
            self.assertEqual(len(features["fingerprint"]["packed_hex"]), 512)
            self.assertNotIn("bit_vector", features["fingerprint"])
            self.assertNotIn("on_bits", features["fingerprint"])
            self.assertEqual(features["shape"]["method"], "rdkit_usr_usrcat_v1")
            self.assertEqual(len(features["shape"]["usr"]), 12)
            self.assertEqual(len(features["shape"]["usrcat"]), 60)
            self.assertEqual(features["conformers"]["requested_count"], 5)
            self.assertEqual(features["conformers"]["retained_coordinate_count"], 1)
            self.assertEqual(coordinate_path.read_text(encoding="utf-8").count("$$$$"), 1)
            self.assertLess(features_path.stat().st_size, MAX_FEATURE_BYTES)
            self.assertLess(coordinate_path.stat().st_size, MAX_COORDINATE_BYTES)

            independent_root = root / "independent_run"
            independent_job = self.make_run(
                independent_root,
                input_kind="small_molecule",
                canonical_value="C[C@H](O)F",
            )
            independent = run_query_structure_job(independent_job)
            independent_row = self.index_row(independent_root)
            independent_features = independent_root / independent_row["features_json"]
            self.assertEqual(independent.status, "completed")
            self.assertEqual(
                hashlib.sha256(independent_features.read_bytes()).hexdigest(),
                first_digest,
            )

            second = run_query_structure_job(job)
            second_row = self.index_row(root)
            second_path = root / second_row["features_json"]
            self.assertEqual(second.status, "completed")
            self.assertEqual(hashlib.sha256(second_path.read_bytes()).hexdigest(), first_digest)
            self.assertEqual(
                len(list((root / "tool_outputs/query_structure").glob("**/manifest.json"))),
                1,
            )
            self.assertFalse(list((root / "tool_outputs/query_structure").glob("**/*.tmp")))
            self.assertFalse(list((root / "tool_outputs/query_structure").glob("**/*.previous")))

            result_path = root / "worker_results/query_structure.json"
            write_worker_result_atomic(result_path, job, second)
            result = json.loads(result_path.read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "PASS")
            self.assertEqual(result["input_id"], "input_1")
            self.assertEqual(result["record_id"], "record_1")
            self.assertEqual(result["artifact_path"], second_row["features_json"])
            self.assertTrue((root / result["manifest"]).is_file())
            self.assertFalse(list(result_path.parent.glob("*.tmp")))

            output_dir = next(
                (root / "tool_outputs/query_structure").glob("**/manifest.json")
            ).parent
            backup = output_dir.with_name(f".{output_dir.name}.previous")
            shutil.copytree(output_dir, backup)
            self.assertEqual(run_query_structure_job(job).status, "completed")
            self.assertFalse(backup.exists())

            feature_payload = json.loads(features_path.read_text(encoding="utf-8"))
            feature_payload["input_id"] = "tampered"
            features_path.write_text(
                json.dumps(feature_payload, ensure_ascii=True, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            manifest_path = output_dir / "manifest.json"
            manifest_payload = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest_payload["feature_digest"] = hashlib.sha256(features_path.read_bytes()).hexdigest()
            manifest_path.write_text(
                json.dumps(manifest_payload, ensure_ascii=True, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            self.assertEqual(run_query_structure_job(job).status, "completed")
            repaired = json.loads(features_path.read_text(encoding="utf-8"))
            self.assertEqual(repaired["input_id"], "input_1")

    def test_small_molecule_standardization_removes_counterion_and_records_policy(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_salt_") as tmp:
            root = Path(tmp)
            job = self.make_run(
                root,
                input_kind="small_molecule",
                canonical_value="CC[NH3+].[Cl-]",
            )

            execution = run_query_structure_job(job)
            row = self.index_row(root)
            features = json.loads((root / row["features_json"]).read_text(encoding="utf-8"))

            self.assertEqual(execution.status, "completed")
            self.assertNotIn("Cl", features["isomeric_smiles"])
            self.assertEqual(features["standardization"]["input_fragment_count"], 2)
            self.assertEqual(features["standardization"]["removed_fragment_count"], 1)
            self.assertIn(
                "ph_specific_microstates_not_enumerated",
                features["standardization"]["risk_flags"],
            )

            parent_root = root / "parent_only"
            parent_job = self.make_run(
                parent_root,
                input_kind="small_molecule",
                canonical_value="CC[NH3+]",
            )
            self.assertEqual(run_query_structure_job(parent_job).status, "completed")
            parent_row = self.index_row(parent_root)
            parent_features = json.loads(
                (parent_root / parent_row["features_json"]).read_text(encoding="utf-8")
            )
            self.assertEqual(features["fingerprint"], parent_features["fingerprint"])
            self.assertEqual(features["conformers"], parent_features["conformers"])
            self.assertEqual(features["pharmacophore"], parent_features["pharmacophore"])

    def test_tautomer_enumerator_limits_are_recorded_and_deterministic(self) -> None:
        feature_digests = []
        for name in ("first", "second"):
            with tempfile.TemporaryDirectory(prefix=f"genoligand_query_tautomer_{name}_") as tmp:
                root = Path(tmp)
                job = self.make_run(
                    root,
                    input_kind="small_molecule",
                    canonical_value="CC(=O)CC(C)=O",
                )

                self.assertEqual(run_query_structure_job(job).status, "completed")
                row = self.index_row(root)
                features_path = root / row["features_json"]
                features = json.loads(features_path.read_text(encoding="utf-8"))
                standardization = features["standardization"]
                self.assertEqual(
                    standardization["tautomer_max_tautomers"],
                    TAUTOMER_MAX_TAUTOMERS,
                )
                self.assertEqual(
                    standardization["tautomer_max_transforms"],
                    TAUTOMER_MAX_TRANSFORMS,
                )
                feature_digests.append(hashlib.sha256(features_path.read_bytes()).hexdigest())

        self.assertEqual(feature_digests[0], feature_digests[1])

    def test_nonconverged_force_field_is_a_structured_failure(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_nonconverged_") as tmp:
            root = Path(tmp)
            job = self.make_run(root, input_kind="small_molecule", canonical_value="CCO")

            with patch(
                "rdkit.Chem.AllChem.MMFFOptimizeMoleculeConfs",
                return_value=[(1, -100.0)] * 5,
            ):
                execution = run_query_structure_job(job)

            self.assertEqual(execution.status, "failed")
            self.assertEqual(
                execution.reason,
                "query_structure_conformer_optimization_not_converged",
            )
            self.assertEqual(self.index_row(root)["status"], "failed")

    def test_peptide_generates_reproducible_ensemble_summary_and_one_representative(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_peptide_") as tmp:
            root = Path(tmp)
            job = self.make_run(root, input_kind="peptide", canonical_value="ACDEFG")

            execution = run_query_structure_job(job)
            row = self.index_row(root)
            features = json.loads((root / row["features_json"]).read_text(encoding="utf-8"))
            coordinate = root / row["coordinate_artifact"]

            self.assertEqual(execution.status, "completed")
            self.assertEqual(features["sequence"], "ACDEFG")
            self.assertEqual(features["sequence_features"]["length"], 6)
            self.assertIn("estimated_net_charge_ph7", features["sequence_features"])
            self.assertIn("mean_kyte_doolittle_hydropathy", features["sequence_features"])
            self.assertEqual(features["conformers"]["requested_count"], 3)
            self.assertEqual(features["conformers"]["retained_coordinate_count"], 1)
            self.assertTrue(coordinate.read_text(encoding="utf-8").rstrip().endswith("END"))
            output_files = {path.name for path in coordinate.parent.iterdir()}
            self.assertEqual(output_files, {"features.json", "manifest.json", "representative.pdb"})

    def test_spooled_protein_uses_compact_sequence_prior_without_coordinate(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_protein_") as tmp:
            root = Path(tmp)
            sequence = "ACDEFGHIKLMNPQRSTVWY" * 5
            prefix = b"prefix"
            spool = root / "input_sequence_spool.bin"
            spool.parent.mkdir(parents=True, exist_ok=True)
            spool.write_bytes(prefix + sequence.encode("ascii"))
            digest = hashlib.sha256(sequence.encode("ascii")).hexdigest()
            job = self.make_run(
                root,
                input_kind="protein_ligand",
                canonical_value="",
                input_format="fasta",
                source_kind="file",
                source_path=str((root / "source.fasta").resolve()),
                artifact=f"input_sequence_spool.bin#offset={len(prefix)}&length={len(sequence)}",
                source_digest=digest,
            )
            job["input_digest"] = query_structure_input_digest(
                "input_1", "record_1", "protein_ligand", sequence, digest
            )

            execution = run_query_structure_job(job)
            row = self.index_row(root)
            features = json.loads((root / row["features_json"]).read_text(encoding="utf-8"))

            self.assertEqual(execution.status, "completed")
            self.assertEqual(row["coordinate_artifact"], "")
            self.assertEqual(features["sequence_features"]["length"], len(sequence))
            self.assertEqual(features["sequence_sha256"], digest)
            self.assertNotIn("sequence", features)
            self.assertFalse(features["structure_prior"]["coordinate_generated"])
            self.assertIn(
                "secondary_structure_propensity",
                features["sequence_features"],
            )

            features_path = root / row["features_json"]
            manifest_path = features_path.parent / "manifest.json"
            old_conversion_digest = features["conversion_manifest_sha256"]
            features["coordinate_artifact"]["sha256"] = "a" * 64
            features["coordinate_artifact"]["size_bytes"] = 1
            features_path.write_text(
                json.dumps(features, ensure_ascii=True, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["coordinate_sha256"] = "a" * 64
            manifest["coordinate_size_bytes"] = 1
            manifest["feature_digest"] = hashlib.sha256(features_path.read_bytes()).hexdigest()
            manifest_path.write_text(
                json.dumps(manifest, ensure_ascii=True, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            self.assertEqual(run_query_structure_job(job).status, "completed")
            repaired = json.loads(features_path.read_text(encoding="utf-8"))
            self.assertEqual(repaired["coordinate_artifact"]["sha256"], "")
            self.assertEqual(repaired["coordinate_artifact"]["size_bytes"], 0)

            with (root / "input_conversion_manifest.tsv").open("a", encoding="utf-8") as handle:
                handle.write("\n")
            self.assertEqual(run_query_structure_job(job).status, "completed")
            rebound = json.loads(features_path.read_text(encoding="utf-8"))
            self.assertNotEqual(rebound["conversion_manifest_sha256"], old_conversion_digest)

            old_output_dir = next(
                (root / "tool_outputs/query_structure").glob("**/manifest.json")
            ).parent
            job["invocation_id"] = "invocation_replacement"
            replaced = run_query_structure_job(job)
            self.assertEqual(replaced.status, "completed")
            self.assertFalse(old_output_dir.exists())
            self.assertEqual(
                len(list((root / "tool_outputs/query_structure").glob("**/manifest.json"))),
                1,
            )
            self.assertFalse(list((root / "tool_outputs/query_structure").glob("**/*.tmp")))

    def test_long_peptide_uses_sequence_prior_instead_of_large_conformer_artifacts(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_long_peptide_") as tmp:
            root = Path(tmp)
            sequence = "ACDEFGHIKLMNPQRSTVWY" * 2
            job = self.make_run(root, input_kind="peptide", canonical_value=sequence)

            execution = run_query_structure_job(job)
            row = self.index_row(root)
            features = json.loads((root / row["features_json"]).read_text(encoding="utf-8"))

            self.assertEqual(execution.status, "completed")
            self.assertEqual(row["coordinate_artifact"], "")
            self.assertFalse(features["structure_prior"]["coordinate_generated"])
            self.assertEqual(
                features["structure_prior"]["reason"],
                "peptide_conformer_limit_exceeded",
            )
            self.assertNotIn("conformers", features)
            self.assertEqual(
                {path.name for path in (root / row["features_json"]).parent.iterdir()},
                {"features.json", "manifest.json"},
            )

    def test_existing_structure_extracts_coordinate_statistics_without_copying_source(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_structure_") as tmp:
            root = Path(tmp)
            pdb = root / "query.pdb"
            pdb_text = (
                "ATOM      1  N   GLY A   1      11.104  13.207   9.318  1.00 20.00           N  \n"
                "ATOM      2  CA  GLY A   1      12.560  13.159   9.120  1.00 20.00           C  \n"
                "ATOM      3  C   GLY A   1      13.190  14.520   9.390  1.00 20.00           C  \n"
                "ATOM      4  O   GLY A   1      12.620  15.570   9.100  1.00 20.00           O  \n"
                "TER\nEND\n"
            )
            pdb.write_text(pdb_text, encoding="utf-8")
            digest = hashlib.sha256(pdb.read_bytes()).hexdigest()
            job = self.make_run(
                root,
                input_kind="complex",
                canonical_value=pdb_text,
                input_format="pdb",
                source_kind="file",
                source_path=str(pdb.resolve()),
                source_digest=digest,
                counts=(4, 1, 1),
            )

            execution = run_query_structure_job(job)
            row = self.index_row(root)
            self.assertEqual(execution.status, "completed", execution.reason)
            features_path = root / row["features_json"]
            features = json.loads(features_path.read_text(encoding="utf-8"))

            self.assertEqual(features["structure"]["atom_count"], 4)
            self.assertEqual(features["structure"]["chain_count"], 1)
            self.assertEqual(features["structure"]["residue_count"], 1)
            self.assertEqual(features["structure"]["coordinate_sha256"], digest)
            self.assertIn("center_of_mass_angstrom", features["structure"])
            self.assertGreater(features["structure"]["radius_of_gyration_angstrom"], 0.0)
            self.assertEqual((root / row["coordinate_artifact"]).resolve(), pdb.resolve())
            self.assertEqual(
                {path.name for path in features_path.parent.iterdir()},
                {"features.json", "manifest.json"},
            )

            limited_root = root / "limited"
            limited_pdb = limited_root / "query.pdb"
            limited_root.mkdir()
            limited_pdb.write_text(pdb_text, encoding="utf-8")
            limited_digest = hashlib.sha256(limited_pdb.read_bytes()).hexdigest()
            limited_job = self.make_run(
                limited_root,
                input_kind="complex",
                canonical_value=pdb_text,
                input_format="pdb",
                source_kind="file",
                source_path=str(limited_pdb.resolve()),
                source_digest=limited_digest,
                counts=(4, 1, 1),
            )
            with patch(
                "genoligand_workers.query_structure_worker.MAX_STRUCTURE_SOURCE_BYTES",
                8,
            ):
                limited = run_query_structure_job(limited_job)
            self.assertEqual(limited.status, "failed")
            self.assertEqual(limited.reason, "query_structure_coordinate_source_too_large")

            atom_limit_root = root / "atom_limit"
            atom_limit_pdb = atom_limit_root / "query.pdb"
            atom_limit_root.mkdir()
            atom_limit_pdb.write_text(pdb_text, encoding="utf-8")
            atom_limit_digest = hashlib.sha256(atom_limit_pdb.read_bytes()).hexdigest()
            atom_limit_job = self.make_run(
                atom_limit_root,
                input_kind="complex",
                canonical_value=pdb_text,
                input_format="pdb",
                source_kind="file",
                source_path=str(atom_limit_pdb.resolve()),
                source_digest=atom_limit_digest,
            )
            with patch(
                "genoligand_workers.query_structure_worker.MAX_STRUCTURE_ATOMS",
                2,
            ):
                atom_limited = run_query_structure_job(atom_limit_job)
            self.assertEqual(atom_limited.status, "failed")
            self.assertEqual(
                atom_limited.reason,
                "query_structure_coordinate_atom_limit_exceeded",
            )

            partial_root = root / "partial_counts"
            partial_pdb = partial_root / "query.pdb"
            partial_root.mkdir()
            partial_pdb.write_text(pdb_text, encoding="utf-8")
            partial_digest = hashlib.sha256(partial_pdb.read_bytes()).hexdigest()
            partial_job = self.make_run(
                partial_root,
                input_kind="complex",
                canonical_value=pdb_text,
                input_format="pdb",
                source_kind="file",
                source_path=str(partial_pdb.resolve()),
                source_digest=partial_digest,
                counts=(3, 0, 1),
            )
            partial = run_query_structure_job(partial_job)
            self.assertEqual(partial.status, "failed")
            self.assertEqual(partial.reason, "query_structure_coordinate_count_mismatch")

    def test_output_parent_symlink_is_rejected_without_external_writes(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_link_") as tmp, tempfile.TemporaryDirectory(
            prefix="genoligand_query_link_outside_"
        ) as outside_tmp:
            root = Path(tmp)
            outside = Path(outside_tmp)
            job = self.make_run(root, input_kind="protein_ligand", canonical_value="ACDEFGHIK")
            try:
                (root / "tool_outputs").symlink_to(outside, target_is_directory=True)
            except OSError as exc:
                self.skipTest(f"directory symlink unavailable: {exc}")

            execution = run_query_structure_job(job)

            self.assertEqual(execution.status, "failed")
            self.assertEqual(execution.reason, "query_structure_output_root_link_or_file")
            self.assertEqual(list(outside.iterdir()), [])

    def test_text_complex_and_genome_are_structured_not_applicable(self) -> None:
        cases = (
            ("complex", "protein and ligand description", "coordinate_required_for_direct_complex_structure"),
            ("genome_or_metagenome", "ATGCGTTAA", "nucleotide_input_has_no_direct_3d_query_structure"),
        )
        for input_kind, value, reason in cases:
            with self.subTest(input_kind=input_kind), tempfile.TemporaryDirectory(
                prefix="genoligand_query_not_applicable_"
            ) as tmp:
                root = Path(tmp)
                job = self.make_run(root, input_kind=input_kind, canonical_value=value)

                execution = run_query_structure_job(job)
                row = self.index_row(root)
                manifests = list((root / "tool_outputs/query_structure").glob("**/manifest.json"))
                manifest = json.loads(manifests[0].read_text(encoding="utf-8"))

                self.assertEqual(execution.status, "not_applicable")
                self.assertEqual(execution.reason, reason)
                self.assertEqual(row["status"], "not_applicable")
                self.assertEqual(row["features_json"], "")
                self.assertEqual(manifest["reason"], reason)

    def test_digest_mismatch_publishes_structured_failure_and_atomic_result(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_query_failure_") as tmp:
            root = Path(tmp)
            job = self.make_run(root, input_kind="protein_ligand", canonical_value="ACDEFGHIK")
            job["input_digest"] = "f" * 64

            execution = run_query_structure_job(job)
            row = self.index_row(root)
            manifest_path = next((root / "tool_outputs/query_structure").glob("**/manifest.json"))
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            result_path = root / "worker_results/query_structure_failure.json"
            write_worker_result_atomic(result_path, job, execution)
            result = json.loads(result_path.read_text(encoding="utf-8"))

            self.assertEqual(execution.status, "failed")
            self.assertEqual(execution.reason, "query_structure_input_digest_mismatch")
            self.assertEqual(row["status"], "failed")
            self.assertEqual(manifest["status"], "failed")
            self.assertEqual(manifest["reason"], "query_structure_input_digest_mismatch")
            self.assertEqual(result["status"], "FAIL")
            self.assertEqual(result["reason"], "query_structure_input_digest_mismatch")
            self.assertTrue((root / result["manifest"]).is_file())
            self.assertFalse(list(root.glob("**/*.tmp")))


if __name__ == "__main__":
    unittest.main()
