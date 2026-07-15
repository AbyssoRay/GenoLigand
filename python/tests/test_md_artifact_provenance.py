from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

from genoligand_workers import md_summary_worker as worker


def canonical_digest(value: object) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


class FixtureAtom:
    def GetSymbol(self) -> str:
        return "C"

    def GetAtomicNum(self) -> int:
        return 6

    def GetPDBResidueInfo(self) -> None:
        return None


class FixtureMolecule:
    def __init__(self) -> None:
        self.atoms = [FixtureAtom()]

    def GetAtoms(self) -> list[FixtureAtom]:
        return self.atoms

    def GetNumAtoms(self) -> int:
        return len(self.atoms)


class MdArtifactProvenanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        self.run_dir = self.root / "run-current"
        self.tool_outputs = self.run_dir / "tool_outputs"
        self.tool_outputs.mkdir(parents=True)
        self.candidate_id = "candidate-current"
        self.input_id = "input-current"
        self.md_outputs = worker.md_output_dir(
            self.run_dir,
            self.candidate_id,
            "invocation-current",
        )
        self.source_digest = hashlib.sha256(b"CCO").hexdigest()
        self.input_digest = canonical_digest(
            {
                "input_id": self.input_id,
                "record_id": "record-current",
                "canonical_kind": "small_molecule",
                "canonical_value": "CCO",
                "source_content_sha256": self.source_digest,
            }
        )

        (self.run_dir / "input.json").write_text(
            '{"inputs":[{"field":"small_molecule","value":"CCO"}]}\n',
            encoding="utf-8",
        )
        (self.run_dir / "complex_candidate_selection.tsv").write_text(
            "candidate_id\tinput_id\trecord_id\ttarget_id\ttarget_name\torganism\ttarget_class\t"
            "source\tpdb_id\tevidence\n"
            f"{self.candidate_id}\t{self.input_id}\trecord-current\tT1\tTarget 1\tHomo sapiens\tprotein\t"
            "fixture\t1ABC\tselected\n",
            encoding="utf-8",
        )
        (self.run_dir / "input_conversion_manifest.tsv").write_text(
            "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\t"
            "content_sha256\n"
            f"{self.input_id}\trecord-current\tsmall_molecule\tCCO\tconverted\t"
            f"{self.source_digest}\n",
            encoding="utf-8",
        )

        self.receptor = self.run_dir / "structure_viewer.pdb"
        self.receptor.write_text("ATOM\n", encoding="utf-8")
        for name in [
            "docking_receptor.pdb",
            "docking_receptor.pdbqt",
            "docking_ligand.sdf",
            "docking_ligand.pdbqt",
            "docking_pose.pdbqt",
            "docking_pose.sdf",
            "docking_vina.log",
        ]:
            (self.tool_outputs / name).write_text(name + "\n", encoding="utf-8")
        docking_manifest_digest = self.write_docking_manifest()
        candidate_digest = worker.selected_md_candidate(self.run_dir)[1]
        self.job = {
            "schema_version": "genoligand_worker_current_v2",
            "pipeline_mode": "genoligand_scientific_pipeline",
            "job_id": "md_summary_ligand_dynamics",
            "invocation_id": "invocation-current",
            "run_id": "run-current",
            "worker": "md_summary",
            "stage": "ligand_dynamics",
            "run_dir": str(self.run_dir),
            "input_manifest": str(self.run_dir / "input.json"),
            "candidate_id": self.candidate_id,
            "candidate_digest": candidate_digest,
            "input_id": self.input_id,
            "record_id": "record-current",
            "input_digest": self.input_digest,
            "source_digest": self.source_digest,
            "docking_manifest": "tool_outputs/docking_manifest.json",
            "docking_manifest_digest": docking_manifest_digest,
            "docking_job_id": "autodock_vina_complex_docking",
            "docking_invocation_id": "invocation-current",
            "platform_policy": "disabled",
        }

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def md_relative(self, name: str) -> str:
        return (self.md_outputs / name).relative_to(self.run_dir).as_posix()

    def write_docking_manifest(self, **updates: object) -> str:
        candidate_digest = worker.selected_md_candidate(self.run_dir)[1]
        manifest = {
            "artifact_schema_version": "genoligand_docking_artifacts_v1",
            "status": "completed",
            "job_id": "autodock_vina_complex_docking",
            "run_id": "run-current",
            "invocation_id": "invocation-current",
            "candidate_id": self.candidate_id,
            "candidate_digest": candidate_digest,
            "input_id": self.input_id,
            "record_id": "record-current",
            "input_digest": self.input_digest,
            "source_content_sha256": self.source_digest,
            "receptor_coordinate": str(self.receptor),
            "receptor_sha256": hashlib.sha256(self.receptor.read_bytes()).hexdigest(),
            "receptor_digest": hashlib.sha256(self.receptor.read_bytes()).hexdigest(),
            "receptor_pdb": "tool_outputs/docking_receptor.pdb",
            "receptor_pdbqt": "tool_outputs/docking_receptor.pdbqt",
            "ligand_sdf": "tool_outputs/docking_ligand.sdf",
            "ligand_pdbqt": "tool_outputs/docking_ligand.pdbqt",
            "pose": "tool_outputs/docking_pose.pdbqt",
            "pose_sdf": "tool_outputs/docking_pose.sdf",
            "log": "tool_outputs/docking_vina.log",
            "seed_provenance": {"input_digest": self.input_digest},
        }
        manifest.update(updates)
        artifact_fields = {
            "receptor_coordinate": manifest.get("receptor_coordinate"),
            "receptor_pdb": manifest.get("receptor_pdb"),
            "receptor_pdbqt": manifest.get("receptor_pdbqt"),
            "ligand_sdf": manifest.get("ligand_sdf"),
            "ligand_pdbqt": manifest.get("ligand_pdbqt"),
            "pose": manifest.get("pose"),
            "pose_sdf": manifest.get("pose_sdf"),
            "log": manifest.get("log"),
        }
        artifacts = []
        for artifact_name, value in artifact_fields.items():
            path = Path(str(value))
            if not path.is_absolute():
                path = self.run_dir / path
            if path.is_file():
                try:
                    relative = path.resolve().relative_to(self.run_dir.resolve()).as_posix()
                except ValueError:
                    continue
                artifacts.append(
                    {
                        "artifact_name": artifact_name,
                        "artifact_path": relative,
                        "artifact_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                        "job_id": manifest["job_id"],
                        "run_id": manifest["run_id"],
                        "invocation_id": manifest["invocation_id"],
                        "candidate_id": manifest["candidate_id"],
                        "candidate_digest": manifest["candidate_digest"],
                        "input_id": manifest["input_id"],
                        "record_id": manifest["record_id"],
                        "input_digest": manifest["input_digest"],
                        "source_digest": manifest["source_content_sha256"],
                        **(
                            {"receptor_digest": manifest["receptor_digest"]}
                            if artifact_name == "receptor_coordinate"
                            else {}
                        ),
                    }
                )
        manifest["artifacts"] = artifacts
        manifest_path = self.tool_outputs / "docking_manifest.json"
        manifest_path.write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        digest = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
        if hasattr(self, "job"):
            self.job["docking_manifest_digest"] = digest
        return digest

    def refresh_docking_manifest_digest(self) -> None:
        manifest_path = self.tool_outputs / "docking_manifest.json"
        self.job["docking_manifest_digest"] = hashlib.sha256(
            manifest_path.read_bytes()
        ).hexdigest()

    def test_current_job_candidate_input_and_docking_artifacts_are_bound(self) -> None:
        binding = worker.resolve_md_invocation(self.job)

        self.assertEqual(binding.job_id, self.job["job_id"])
        self.assertEqual(binding.invocation_id, self.job["invocation_id"])
        self.assertEqual(binding.candidate_id, self.candidate_id)
        self.assertEqual(binding.input_id, self.input_id)
        self.assertEqual(binding.input_digest, self.input_digest)
        self.assertEqual(binding.source_digest, self.source_digest)
        self.assertEqual(binding.receptor_coordinate, self.receptor.resolve())
        self.assertEqual(
            binding.docking_pose_sdf,
            (self.tool_outputs / "docking_pose.sdf").resolve(),
        )

    def test_windows_run_relative_docking_artifacts_resolve_inside_current_run(self) -> None:
        receptor = self.run_dir / "structure_candidates" / "target_1" / "receptor.cif"
        receptor.parent.mkdir(parents=True)
        self.receptor.replace(receptor)
        self.receptor = receptor
        self.write_docking_manifest()
        manifest_path = self.tool_outputs / "docking_manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        for row in manifest["artifacts"]:
            relative = str(row["artifact_path"])
            manifest[str(row["artifact_name"])] = relative.replace("/", "\\")
            row["artifact_path"] = relative.replace("/", "\\")
        manifest_path.write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        self.job["docking_manifest"] = r"tool_outputs\docking_manifest.json"
        self.refresh_docking_manifest_digest()

        binding = worker.resolve_md_invocation(self.job)

        self.assertEqual(binding.receptor_coordinate, receptor.resolve())
        self.assertEqual(
            binding.docking_ligand_sdf,
            (self.tool_outputs / "docking_ligand.sdf").resolve(),
        )
        self.assertEqual(
            binding.docking_pose_sdf,
            (self.tool_outputs / "docking_pose.sdf").resolve(),
        )

    def test_missing_or_wrong_job_docking_bindings_fail_closed(self) -> None:
        for field in [
            "docking_manifest_digest",
            "docking_job_id",
            "docking_invocation_id",
        ]:
            with self.subTest(field=field):
                missing = {**self.job}
                missing.pop(field)
                with self.assertRaisesRegex(
                    worker.MdProvenanceError,
                    "docking_manifest_binding_missing",
                ):
                    worker.resolve_md_invocation(missing)

        wrong_bindings = [
            (
                {**self.job, "docking_manifest_digest": "0" * 64},
                "docking_manifest_sha256_mismatch",
            ),
            (
                {**self.job, "docking_job_id": "docking-old"},
                "docking_job_id_mismatch",
            ),
            (
                {**self.job, "docking_invocation_id": "invocation-old"},
                "docking_invocation_id_mismatch",
            ),
        ]
        for job, reason in wrong_bindings:
            with self.subTest(reason=reason):
                with self.assertRaisesRegex(worker.MdProvenanceError, reason):
                    worker.resolve_md_invocation(job)

    def test_stale_or_mismatched_docking_binding_fails_closed(self) -> None:
        cases = [
            ({"invocation_id": "invocation-old"}, "docking_invocation_id_mismatch"),
            ({"candidate_id": "candidate-old"}, "docking_candidate_id_mismatch"),
            ({"input_id": "input-old"}, "docking_input_id_mismatch"),
            ({"source_content_sha256": "f" * 64}, "docking_source_digest_mismatch"),
            (
                {"seed_provenance": {"input_digest": "e" * 64}},
                "docking_input_digest_mismatch",
            ),
        ]
        for updates, reason in cases:
            with self.subTest(reason=reason):
                self.write_docking_manifest(**updates)
                with self.assertRaisesRegex(worker.MdProvenanceError, reason):
                    worker.resolve_md_invocation(self.job)
        self.write_docking_manifest()

        mismatched_job = {**self.job, "input_digest": "d" * 64}
        with self.assertRaisesRegex(worker.MdProvenanceError, "job_input_digest_mismatch"):
            worker.resolve_md_invocation(mismatched_job)

    def test_docking_artifact_set_is_exact_bound_and_tamper_evident(self) -> None:
        manifest_path = self.tool_outputs / "docking_manifest.json"
        original = json.loads(manifest_path.read_text(encoding="utf-8"))

        cases = []
        missing = json.loads(json.dumps(original))
        missing["artifacts"] = missing["artifacts"][1:]
        cases.append((missing, "docking_artifact_set_mismatch"))
        duplicate = json.loads(json.dumps(original))
        duplicate["artifacts"].append(dict(duplicate["artifacts"][0]))
        cases.append((duplicate, "docking_artifact_duplicate"))
        extra_path = self.tool_outputs / "unexpected.txt"
        extra_path.write_text("unexpected\n", encoding="utf-8")
        extra = json.loads(json.dumps(original))
        extra["artifacts"].append(
            {
                **extra["artifacts"][0],
                "artifact_name": "unexpected",
                "artifact_path": "tool_outputs/unexpected.txt",
                "artifact_sha256": hashlib.sha256(extra_path.read_bytes()).hexdigest(),
            }
        )
        cases.append((extra, "docking_artifact_set_mismatch"))
        foreign = json.loads(json.dumps(original))
        foreign["artifacts"][0]["candidate_id"] = "candidate-old"
        cases.append((foreign, "docking_artifact_candidate_id_mismatch"))

        for manifest, reason in cases:
            with self.subTest(reason=reason):
                manifest_path.write_text(
                    json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
                self.refresh_docking_manifest_digest()
                with self.assertRaisesRegex(worker.MdProvenanceError, reason):
                    worker.resolve_md_invocation(self.job)

        manifest_path.write_text(
            json.dumps(original, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        self.refresh_docking_manifest_digest()
        self.tool_outputs.joinpath("docking_pose.sdf").write_text("tampered\n", encoding="utf-8")
        with self.assertRaisesRegex(worker.MdProvenanceError, "docking_artifact_sha256_mismatch"):
            worker.resolve_md_invocation(self.job)

    def test_absolute_relative_and_symlink_artifact_escapes_fail_closed(self) -> None:
        outside = self.root / "outside.sdf"
        outside.write_text("outside\n", encoding="utf-8")
        for escaped in [
            str(outside),
            "../outside.sdf",
            r"..\outside.sdf",
            r"C:\outside\pose.sdf",
            r"\\server\share\pose.sdf",
        ]:
            with self.subTest(escaped=escaped):
                self.write_docking_manifest(pose_sdf=escaped)
                with self.assertRaisesRegex(worker.MdProvenanceError, "artifact_outside_current_run"):
                    worker.resolve_md_invocation(self.job)

        link = self.tool_outputs / "linked_pose.sdf"
        try:
            link.symlink_to(outside)
        except OSError as exc:
            self.skipTest(f"symlink creation unavailable: {exc}")
        self.write_docking_manifest(pose_sdf="tool_outputs/linked_pose.sdf")
        with self.assertRaisesRegex(worker.MdProvenanceError, "artifact_outside_current_run"):
            worker.resolve_md_invocation(self.job)

    def test_rerun_clears_md_outputs_without_removing_docking_inputs(self) -> None:
        self.md_outputs.mkdir(parents=True)
        old_summary = self.md_outputs / "md_summary.tsv"
        old_manifest = self.md_outputs / "md_run_manifest.json"
        old_provenance = self.md_outputs / "md_artifact_provenance.json"
        for path in [old_summary, old_manifest, old_provenance]:
            path.write_text("old invocation\n", encoding="utf-8")
        docking_pose = self.tool_outputs / "docking_pose.sdf"

        worker.clear_previous_md_outputs(self.run_dir, self.candidate_id)

        self.assertFalse(old_summary.exists())
        self.assertFalse(old_manifest.exists())
        self.assertFalse(old_provenance.exists())
        self.assertTrue(docking_pose.exists())

    def test_worker_rejects_stale_docking_before_scientific_execution(self) -> None:
        self.md_outputs.mkdir(parents=True)
        old_summary = self.md_outputs / "md_summary.tsv"
        old_summary.write_text("old invocation\n", encoding="utf-8")
        self.write_docking_manifest(invocation_id="invocation-old")

        execution = worker.run_md_summary_job(self.job)

        self.assertEqual(execution.status, "failed")
        self.assertEqual(execution.reason, "docking_invocation_id_mismatch")
        self.assertFalse(old_summary.exists())
        self.assertFalse((self.md_outputs / "md_run_manifest.json").exists())

    def test_worker_preserves_not_applicable_for_a_current_job_without_md_input(self) -> None:
        (self.run_dir / "input_conversion_manifest.tsv").write_text(
            "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\t"
            "content_sha256\n",
            encoding="utf-8",
        )
        (self.run_dir / "input.json").write_text(
            '{"inputs":[],"files":[]}\n',
            encoding="utf-8",
        )
        self.md_outputs.mkdir(parents=True)
        old_summary = self.md_outputs / "md_summary.tsv"
        old_summary.write_text("old invocation\n", encoding="utf-8")

        execution = worker.run_md_summary_job(self.job)

        self.assertEqual(execution.status, "not_applicable")
        self.assertEqual(execution.reason, "no_applicable_md_input")
        self.assertFalse(old_summary.exists())

    def test_not_ready_outputs_are_bound_to_the_current_invocation(self) -> None:
        binding = worker.resolve_md_invocation(self.job)
        worker.clear_previous_md_outputs(self.run_dir)

        worker.write_md_not_ready(
            self.run_dir,
            "fixture_not_ready",
            ["fixture_not_ready"],
            binding=binding,
        )

        run_manifest = json.loads(
            (self.md_outputs / "md_run_manifest.json").read_text(encoding="utf-8")
        )
        self.assertEqual(run_manifest["job_id"], self.job["job_id"])
        self.assertEqual(run_manifest["invocation_id"], self.job["invocation_id"])
        self.assertEqual(run_manifest["candidate_id"], self.candidate_id)
        self.assertEqual(run_manifest["input_digest"], self.input_digest)
        provenance_path = self.md_outputs / "md_artifact_provenance.json"
        worker.validate_md_artifact_provenance(provenance_path, binding)
        provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
        self.assertEqual(
            {row["artifact_path"] for row in provenance["artifacts"]},
            {
                self.md_relative("md_not_ready.tsv"),
                self.md_relative("md_run_manifest.json"),
            },
        )

    def test_completed_peptide_outputs_are_all_bound_and_hashed(self) -> None:
        binding = worker.resolve_md_invocation(self.job)
        sequence = "AC"
        source_digest = hashlib.sha256(sequence.encode("utf-8")).hexdigest()
        binding = replace(
            binding,
            input_kind="peptide",
            input_value=sequence,
            source_digest=source_digest,
            input_digest=canonical_digest(
                {
                    "input_id": self.input_id,
                    "record_id": "record-current",
                    "canonical_kind": "peptide",
                    "canonical_value": sequence,
                    "source_content_sha256": source_digest,
                }
            ),
            receptor_coordinate=None,
            docking_ligand_sdf=None,
            docking_ligand_pdbqt=None,
            docking_pose_pdbqt=None,
            docking_pose_sdf=None,
        )
        worker.clear_previous_md_outputs(self.run_dir)
        metrics = {
            "mean_heavy_atom_rmsd_angstrom": 1.0,
            "mean_heavy_atom_rmsf_angstrom": 1.1,
            "mean_centroid_drift_angstrom": 1.2,
        }

        worker.write_peptide_conformer_artifacts(
            self.run_dir,
            self.candidate_id,
            self.input_id,
            sequence,
            metrics,
            [[[0.0, 0.0, 0.0]]],
            FixtureMolecule(),
            "fixture_parameterization",
            ["fixture"],
            "fixture-rdkit",
            binding,
        )

        provenance_path = self.md_outputs / "md_artifact_provenance.json"
        worker.validate_md_artifact_provenance(provenance_path, binding)
        provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
        self.assertEqual(len(provenance["artifacts"]), 5)
        self.assertEqual(
            {row["artifact_path"] for row in provenance["artifacts"]},
            {
                self.md_relative("md_run_manifest.json"),
                self.md_relative("peptide_conformer_coordinates.tsv"),
                self.md_relative("peptide_conformer_ensemble.tsv"),
                self.md_relative("peptide_conformer_metrics.tsv"),
                self.md_relative("peptide_conformer_summary.tsv"),
            },
        )
        run_manifest = json.loads(
            (self.md_outputs / "md_run_manifest.json").read_text(encoding="utf-8")
        )
        self.assertEqual(run_manifest["artifact_schema_version"], "genoligand_peptide_conformer_v1")
        self.assertEqual(run_manifest["method_class"], "conformer_ensemble")
        self.assertFalse(run_manifest["has_receptor"])
        self.assertFalse(run_manifest["uses_openmm"])
        self.assertEqual(run_manifest["md_score"], 0.0)
        self.assertFalse((self.md_outputs / "md_summary.tsv").exists())

    def test_provenance_hashes_every_output_and_detects_stale_or_tampered_files(self) -> None:
        binding = worker.resolve_md_invocation(self.job)
        self.md_outputs.mkdir(parents=True, exist_ok=True)
        summary = self.md_outputs / "md_summary.tsv"
        state = self.md_outputs / "md_state.tsv"
        summary.write_text("candidate_id\ncurrent\n", encoding="utf-8")
        state.write_text("candidate_id\ncurrent\n", encoding="utf-8")

        provenance_path = worker.write_md_artifact_provenance(
            binding,
            [summary, state],
            tool_name="OpenMM",
            tool_version="8.2",
            parameters={"production_steps": 1000},
        )
        manifest = json.loads(provenance_path.read_text(encoding="utf-8"))

        self.assertEqual(manifest["job_id"], self.job["job_id"])
        self.assertEqual(manifest["invocation_id"], self.job["invocation_id"])
        self.assertEqual(manifest["candidate_id"], self.candidate_id)
        self.assertEqual(manifest["input_digest"], self.input_digest)
        self.assertEqual(
            {row["artifact_path"] for row in manifest["artifacts"]},
            {self.md_relative("md_summary.tsv"), self.md_relative("md_state.tsv")},
        )
        for row in manifest["artifacts"]:
            self.assertEqual(row["job_id"], self.job["job_id"])
            self.assertEqual(row["invocation_id"], self.job["invocation_id"])
            self.assertEqual(row["candidate_id"], self.candidate_id)
            self.assertEqual(row["input_digest"], self.input_digest)
            artifact = self.run_dir / row["artifact_path"]
            self.assertEqual(row["artifact_sha256"], hashlib.sha256(artifact.read_bytes()).hexdigest())

        worker.validate_md_artifact_provenance(provenance_path, binding)
        with self.assertRaisesRegex(worker.MdProvenanceError, "provenance_invocation_id_mismatch"):
            worker.validate_md_artifact_provenance(
                provenance_path,
                replace(binding, invocation_id="invocation-next"),
            )

        summary.write_text("tampered\n", encoding="utf-8")
        with self.assertRaisesRegex(worker.MdProvenanceError, "artifact_sha256_mismatch"):
            worker.validate_md_artifact_provenance(provenance_path, binding)

    def test_worker_result_path_must_resolve_inside_current_run(self) -> None:
        inside = self.run_dir / "worker_results" / "md_summary_ligand_dynamics.json"
        self.assertEqual(worker.resolve_worker_output_path(self.job, inside), inside.resolve())

        outside = self.root / "outside-result.json"
        with self.assertRaisesRegex(worker.MdProvenanceError, "artifact_outside_current_run"):
            worker.resolve_worker_output_path(self.job, outside)

    def test_worker_result_is_bound_and_rejects_stale_provenance(self) -> None:
        binding = worker.resolve_md_invocation(self.job)
        worker.clear_previous_md_outputs(self.run_dir)
        worker.write_md_not_ready(
            self.run_dir,
            "fixture_not_ready",
            ["fixture_not_ready"],
            binding=binding,
        )
        result_path = self.run_dir / "worker_results" / "md_summary_ligand_dynamics.json"
        result_path.parent.mkdir()
        result_path.write_text(
            json.dumps({"job_id": self.job["job_id"], "status": "NOT_READY"}) + "\n",
            encoding="utf-8",
        )

        worker.bind_worker_result(result_path, self.job)

        result = json.loads(result_path.read_text(encoding="utf-8"))
        self.assertEqual(result["invocation_id"], self.job["invocation_id"])
        self.assertEqual(result["candidate_id"], self.candidate_id)
        self.assertEqual(result["input_digest"], self.input_digest)
        self.assertEqual(
            result["artifact_provenance"],
            self.md_relative("md_artifact_provenance.json"),
        )

        provenance_path = self.md_outputs / "md_artifact_provenance.json"
        provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
        provenance["invocation_id"] = "invocation-old"
        provenance_path.write_text(
            json.dumps(provenance, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(worker.MdProvenanceError, "provenance_invocation_id_mismatch"):
            worker.bind_worker_result(result_path, self.job)


if __name__ == "__main__":
    unittest.main()
