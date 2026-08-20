from __future__ import annotations

import json
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

from genoligand_workers import md_summary_worker as worker


class FakePlatform:
    def __init__(self, name: str) -> None:
        self.name = name

    def getName(self) -> str:
        return self.name

    def getPropertyNames(self) -> list[str]:
        return []


class FakePlatformRegistry:
    names = ["Reference", "CPU", "CUDA"]

    @classmethod
    def getNumPlatforms(cls) -> int:
        return len(cls.names)

    @classmethod
    def getPlatform(cls, index: int) -> FakePlatform:
        return FakePlatform(cls.names[index])

    @classmethod
    def getPlatformByName(cls, name: str) -> FakePlatform:
        if name not in cls.names:
            raise ValueError(name)
        return FakePlatform(name)


class FakeOpenMM:
    Platform = FakePlatformRegistry

    def __init__(self, failing: set[str]) -> None:
        self.failing = failing
        self.context_attempts: list[str] = []

    def Context(self, _system: object, _integrator: object, platform: FakePlatform, *_args: object) -> object:
        self.context_attempts.append(platform.name)
        if platform.name in self.failing:
            raise RuntimeError(f"{platform.name} unavailable")
        return {"platform": platform.name}


class MdSemanticValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.run_dir = Path(self.temp_dir.name) / "run-current"
        self.outputs = self.run_dir / "tool_outputs"
        self.outputs.mkdir(parents=True)
        self.binding = worker.MdInvocationBinding(
            run_dir=self.run_dir.resolve(),
            run_id="run-current",
            job_id="md_summary_ligand_dynamics",
            invocation_id="invocation-current",
            candidate_id="candidate-current",
            candidate_digest="a" * 64,
            input_id="input-current",
            record_id="record-current",
            input_digest="b" * 64,
            source_digest="c" * 64,
            input_kind="small_molecule",
            input_value="CCO",
            receptor_coordinate=None,
            docking_ligand_sdf=None,
            docking_ligand_pdbqt=None,
            docking_pose_pdbqt=None,
            docking_pose_sdf=None,
            output_dir=self.outputs.resolve(),
        )
        self.write_valid_complex_artifacts()

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def write(self, name: str, text: str) -> None:
        (self.outputs / name).write_text(text, encoding="utf-8")

    def write_valid_complex_artifacts(self) -> None:
        candidate = self.binding.candidate_id
        input_id = self.binding.input_id
        summary_header = (
            "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\t"
            "contact_occupancy\tinteraction_persistence\tmd_risk_flags\t"
            "md_score_decomposition\tmd_stability_score\n"
        )
        self.write(
            "md_summary.tsv",
            summary_header + f"{candidate}\t1.2\t0.4\t0.6\t0.7\t0.8\t0.75\tbound\tRust\t0.8\n",
        )
        metric_specs = {
            "md_rmsd.tsv": ("rmsd", "1.2", "angstrom"),
            "md_rmsf.tsv": ("rmsf", "0.4", "angstrom"),
            "md_ligand_drift.tsv": ("ligand_drift", "0.6", "angstrom"),
            "md_contacts.tsv": ("contact_occupancy", "0.8", "fraction"),
            "md_hbonds.tsv": ("hydrogen_bond_occupancy", "0.7", "fraction"),
        }
        for name, (metric, value, unit) in metric_specs.items():
            self.write(
                name,
                "candidate_id\tinput_id\tmetric\tvalue\tunit\tsource\n"
                f"{candidate}\t{input_id}\t{metric}\t{value}\t{unit}\t"
                "openmm_receptor_ligand_complex_dynamics\n",
            )
        self.write(
            "md_protocol.tsv",
            "candidate_id\tinput_id\tstage\tparameter\tvalue\tunit\tsource\n"
            f"{candidate}\t{input_id}\tplatform\topenmm_platform\tCPU\t\tOpenMM\n"
            f"{candidate}\t{input_id}\tplatform\tplatform_policy\tdisabled\t\tGenoLigand job\n"
            f"{candidate}\t{input_id}\tplatform\tavailable_openmm_platforms\tReference,CPU,CUDA\t\tOpenMM\n"
            f"{candidate}\t{input_id}\tplatform\tcuda_platform_detected\ttrue\t\tOpenMM\n"
            f"{candidate}\t{input_id}\tplatform\tcontext_creation_failures\t\t\tOpenMM Context\n"
            f"{candidate}\t{input_id}\tplatform\tfallback_reason\tcuda_disabled_by_policy\t\tOpenMM Context\n"
            f"{candidate}\t{input_id}\tsampling_policy\tpolicy_id\t{worker.MD_STABILITY_POLICY_ID}\t\tGenoLigand\n"
            f"{candidate}\t{input_id}\tsampling_policy\tpurpose\tcandidate_local_stability_screen\t\tGenoLigand\n"
            f"{candidate}\t{input_id}\tintegrator\ttime_step\t0.002\tps\tOpenMM\n"
            f"{candidate}\t{input_id}\tproduction\ttotal_steps\t100\tsteps\tOpenMM\n"
            f"{candidate}\t{input_id}\toutput\tframe_count\t10\tframes\tmd_trajectory.tsv\n"
            f"{candidate}\t{input_id}\toutput\tstate_rows\t2\trows\tmd_state.tsv\n",
        )
        self.write(
            "md_state.tsv",
            "candidate_id\tinput_id\tframe\tstep\ttime_ps\tstage\tpotential_energy_kj_mol\t"
            "kinetic_energy_kj_mol\ttotal_energy_kj_mol\ttemperature_estimate_K\tsource\n"
            f"{candidate}\t{input_id}\t0\t0\t0\tpost_equilibration\t-2\t1\t-1\t300\tOpenMM\n"
            f"{candidate}\t{input_id}\t9\t100\t0.2\tproduction_segment_9\t-2\t1\t-1\t300\tOpenMM\n",
        )
        trajectory = "candidate_id\tframe\tatom_index\trole\telement\tresidue\tx\ty\tz\n"
        trajectory += "".join(
            f"{candidate}\t{frame}\t0\tligand\tC\tLIG\t{frame}.0\t0.0\t0.0\n"
            for frame in range(10)
        )
        self.write("md_trajectory.tsv", trajectory)
        self.write("ligand_placement.tsv", "candidate_id\tinput_id\n" f"{candidate}\t{input_id}\n")
        self.write("complex_md_manifest.json", "{}\n")
        self.write("gpu_telemetry.tsv", "sample_label\n")
        self.write("gpu_telemetry_status.tsv", "cuda_platform\nCPU\n")
        manifest = {
            **worker.binding_fields(self.binding),
            "method": "openmm_receptor_ligand_complex_dynamics",
            "platform": "CPU",
            "platform_policy": "disabled",
            "platform_context_failures": [],
            "available_openmm_platforms": ["Reference", "CPU", "CUDA"],
            "cuda_platform_detected": True,
            "platform_fallback_reason": "cuda_disabled_by_policy",
            "sampling_policy": {
                "policy_id": worker.MD_STABILITY_POLICY_ID,
                "purpose": "candidate_local_stability_screen",
                "timestep_ps": 0.002,
                "production_steps_total": 100,
            },
            "frame_count": 10,
            "state_row_count": 2,
            "trajectory": "tool_outputs/md_trajectory.tsv",
            "state_artifact": "tool_outputs/md_state.tsv",
            "protocol_artifact": "tool_outputs/md_protocol.tsv",
            "ligand_coordinate": "tool_outputs/ligand_placement.tsv",
            "complex_manifest": "tool_outputs/complex_md_manifest.json",
            "gpu_telemetry_artifact": "tool_outputs/gpu_telemetry.tsv",
            "gpu_telemetry_status_artifact": "tool_outputs/gpu_telemetry_status.tsv",
            "metric_artifacts": [
                "tool_outputs/md_summary.tsv",
                "tool_outputs/md_rmsd.tsv",
                "tool_outputs/md_rmsf.tsv",
                "tool_outputs/md_ligand_drift.tsv",
                "tool_outputs/md_contacts.tsv",
                "tool_outputs/md_hbonds.tsv",
            ],
            "metric_units": worker.COMPLEX_MD_METRIC_UNITS,
        }
        (self.outputs / "md_run_manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    def write_valid_selected_receptor_artifacts(self) -> dict[str, Path | str]:
        source_receptor = self.run_dir / "source_receptor.pdb"
        docking_pose = self.run_dir / "docking_pose.sdf"
        prepared_receptor = self.outputs / "receptor_prepared.pdb"
        source_receptor.write_text("SOURCE RECEPTOR\n", encoding="utf-8")
        docking_pose.write_text("DOCKING POSE\n", encoding="utf-8")
        prepared_receptor.write_text("PREPARED RECEPTOR\n", encoding="utf-8")
        self.binding = replace(
            self.binding,
            receptor_coordinate=source_receptor.resolve(),
            docking_pose_sdf=docking_pose.resolve(),
        )
        system = {
            "receptor_chain_selection_algorithm": worker.RECEPTOR_CHAIN_SELECTION_VERSION,
            "receptor_chain_atom_policy": "protein_and_ligand_heavy_atoms",
            "receptor_chain_contact_cutoff_angstrom": worker.RECEPTOR_CHAIN_CONTACT_CUTOFF_ANGSTROM,
            "original_protein_chain_count": 2,
            "retained_protein_chain_count": 1,
            "selected_chain_id": "A",
            "selected_chain_ordinal": 1,
            "selected_chain_signature_sha256": "d" * 64,
            "selected_chain_coordinate_sha256": "e" * 64,
            "selected_chain_minimum_distance_angstrom": 2.0,
            "source_receptor_coordinate": "source_receptor.pdb",
            "source_receptor_sha256": worker.sha256_file(source_receptor),
            "docking_pose_sha256": worker.sha256_file(docking_pose),
            "prepared_receptor_sha256": worker.sha256_file(prepared_receptor),
        }
        system["receptor_chain_selection_digest"] = worker.canonical_digest(
            {field_name: system[field_name] for field_name in worker.RECEPTOR_SELECTION_DIGEST_FIELDS}
        )
        run_manifest_path = self.outputs / "md_run_manifest.json"
        run_manifest = json.loads(run_manifest_path.read_text(encoding="utf-8"))
        run_manifest.update(
            {
                "system": system,
                "receptor_coordinate": "tool_outputs/receptor_prepared.pdb",
                "artifact_provenance": "tool_outputs/md_artifact_provenance.json",
            }
        )
        run_manifest_path.write_text(
            json.dumps(run_manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        (self.outputs / "complex_md_manifest.json").write_text(
            json.dumps(
                {
                    "receptor_coordinate": "tool_outputs/receptor_prepared.pdb",
                    "system": system,
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        protocol_path = self.outputs / "md_protocol.tsv"
        protocol_rows = "".join(
            f"{self.binding.candidate_id}\t{self.binding.input_id}\tinput\t{parameter}\t"
            f"{system[field_name]}\t{unit}\tGenoLigand\n"
            for parameter, field_name, unit in worker.RECEPTOR_SELECTION_PROTOCOL_FIELDS
        )
        protocol_path.write_text(
            protocol_path.read_text(encoding="utf-8") + protocol_rows,
            encoding="utf-8",
        )
        worker.write_md_artifact_provenance(
            self.binding,
            [
                path
                for path in sorted(self.outputs.iterdir())
                if path.name != worker.MD_PROVENANCE_ARTIFACT
            ],
            tool_name="OpenMM",
            tool_version="fixture",
            parameters={"system": system},
        )
        return {
            "source_receptor": source_receptor,
            "docking_pose": docking_pose,
            "prepared_receptor": prepared_receptor,
            "selection_digest": system["receptor_chain_selection_digest"],
        }

    def test_complete_bound_complex_metrics_are_accepted(self) -> None:
        facts = worker.validate_completed_md_semantics(self.binding)
        self.assertEqual(facts["method"], "openmm_receptor_ligand_complex_dynamics")
        self.assertEqual(facts["candidate_id"], self.binding.candidate_id)
        self.assertEqual(facts["frame_count"], 10)
        self.assertEqual(facts["production_steps"], 100)

    def test_selected_receptor_schema_rejects_downgrade_and_tampering(self) -> None:
        self.write_valid_selected_receptor_artifacts()
        worker.validate_completed_md_semantics(self.binding)

        cases = {
            "missing_digest": "md_receptor_selection_field_missing:receptor_chain_selection_digest",
            "forged_digest": "md_receptor_selection_digest_mismatch",
            "source_receptor": "md_source_receptor_sha256_mismatch",
            "docking_pose": "md_docking_pose_sha256_mismatch",
            "prepared_receptor": "md_prepared_receptor_sha256_mismatch",
            "complex_manifest": "md_receptor_selection_manifest_mismatch",
            "protocol": "md_receptor_selection_protocol_mismatch:selected_chain_id",
            "provenance": "md_prepared_receptor_provenance_mismatch",
        }
        for name, reason in cases.items():
            with self.subTest(name=name):
                self.write_valid_complex_artifacts()
                artifacts = self.write_valid_selected_receptor_artifacts()
                run_manifest_path = self.outputs / "md_run_manifest.json"
                complex_manifest_path = self.outputs / "complex_md_manifest.json"
                provenance_path = self.outputs / worker.MD_PROVENANCE_ARTIFACT
                if name == "missing_digest":
                    manifest = json.loads(run_manifest_path.read_text(encoding="utf-8"))
                    del manifest["system"]["receptor_chain_selection_digest"]
                    run_manifest_path.write_text(json.dumps(manifest) + "\n", encoding="utf-8")
                elif name == "forged_digest":
                    forged = "f" * 64
                    for path in (run_manifest_path, complex_manifest_path):
                        value = json.loads(path.read_text(encoding="utf-8"))
                        value["system"]["receptor_chain_selection_digest"] = forged
                        path.write_text(json.dumps(value) + "\n", encoding="utf-8")
                    protocol_path = self.outputs / "md_protocol.tsv"
                    protocol_path.write_text(
                        protocol_path.read_text(encoding="utf-8").replace(
                            str(artifacts["selection_digest"]), forged
                        ),
                        encoding="utf-8",
                    )
                elif name in {"source_receptor", "docking_pose", "prepared_receptor"}:
                    Path(artifacts[name]).write_text("TAMPERED\n", encoding="utf-8")
                elif name == "complex_manifest":
                    value = json.loads(complex_manifest_path.read_text(encoding="utf-8"))
                    value["system"]["selected_chain_id"] = "B"
                    complex_manifest_path.write_text(json.dumps(value) + "\n", encoding="utf-8")
                elif name == "protocol":
                    protocol_path = self.outputs / "md_protocol.tsv"
                    protocol_path.write_text(
                        protocol_path.read_text(encoding="utf-8").replace(
                            "input\tselected_chain_id\tA\t",
                            "input\tselected_chain_id\tB\t",
                        ),
                        encoding="utf-8",
                    )
                else:
                    value = json.loads(provenance_path.read_text(encoding="utf-8"))
                    value["artifacts"] = [
                        row
                        for row in value["artifacts"]
                        if row["artifact_path"] != "tool_outputs/receptor_prepared.pdb"
                    ]
                    provenance_path.write_text(json.dumps(value) + "\n", encoding="utf-8")
                with self.assertRaisesRegex(worker.MdProvenanceError, reason):
                    worker.validate_completed_md_semantics(self.binding)

    def test_missing_metric_wrong_candidate_nonfinite_and_short_runs_fail(self) -> None:
        mutations = {
            "missing_metric": lambda: (self.outputs / "md_hbonds.tsv").unlink(),
            "wrong_candidate": lambda: self.write(
                "md_rmsd.tsv",
                "candidate_id\tinput_id\tmetric\tvalue\tunit\tsource\n"
                "foreign\tinput-current\trmsd\t1.2\tangstrom\t"
                "openmm_receptor_ligand_complex_dynamics\n",
            ),
            "wrong_unit": lambda: self.write(
                "md_rmsd.tsv",
                "candidate_id\tinput_id\tmetric\tvalue\tunit\tsource\n"
                "candidate-current\tinput-current\trmsd\t1.2\tfraction\t"
                "openmm_receptor_ligand_complex_dynamics\n",
            ),
            "wrong_unit_schema": lambda: self.write(
                "md_run_manifest.json",
                (self.outputs / "md_run_manifest.json")
                .read_text(encoding="utf-8")
                .replace('"rmsd": "angstrom"', '"rmsd": "fraction"'),
            ),
            "nonfinite": lambda: self.write(
                "md_summary.tsv",
                "candidate_id\trmsd\trmsf\tligand_drift\thydrogen_bond_occupancy\t"
                "contact_occupancy\tinteraction_persistence\tmd_risk_flags\t"
                "md_score_decomposition\tmd_stability_score\n"
                "candidate-current\tNaN\t0.4\t0.6\t0.7\t0.8\t0.75\tbound\tRust\t0.8\n",
            ),
            "zero_steps": lambda: self.write(
                "md_protocol.tsv",
                (self.outputs / "md_protocol.tsv")
                .read_text(encoding="utf-8")
                .replace("production\ttotal_steps\t100", "production\ttotal_steps\t0"),
            ),
            "single_frame": lambda: self.write(
                "md_protocol.tsv",
                (self.outputs / "md_protocol.tsv")
                .read_text(encoding="utf-8")
                .replace("output\tframe_count\t10", "output\tframe_count\t1"),
            ),
        }
        reasons = {
            "missing_metric": "md_metric_artifact_missing",
            "wrong_candidate": "md_metric_candidate_id_mismatch",
            "wrong_unit": "md_metric_unit_invalid",
            "wrong_unit_schema": "md_metric_unit_schema_invalid",
            "nonfinite": "md_metric_value_invalid",
            "zero_steps": "md_production_steps_invalid",
            "single_frame": "md_frame_count_invalid",
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                self.write_valid_complex_artifacts()
                mutate()
                with self.assertRaisesRegex(worker.MdProvenanceError, reasons[name]):
                    worker.validate_completed_md_semantics(self.binding)

    def test_platform_policy_uses_real_context_creation_and_honors_cpu_mode(self) -> None:
        self.assertEqual(worker.md_platform_policy({}), "preferred")

        preferred_cuda = FakeOpenMM(set())
        _context, name, properties, attempts = worker.create_openmm_context(
            preferred_cuda, object(), object(), worker.md_platform_policy({})
        )
        self.assertEqual(name, "CUDA")
        self.assertEqual(preferred_cuda.context_attempts, ["CUDA"])
        evidence = worker.openmm_platform_execution_evidence(
            "preferred",
            name,
            worker.available_openmm_platforms(preferred_cuda),
            properties,
            attempts,
        )
        self.assertTrue(evidence["cuda_platform_detected"])
        self.assertEqual(evidence["fallback_reason"], "")

        disabled = FakeOpenMM(set())
        _context, name, _properties, attempts = worker.create_openmm_context(
            disabled, object(), object(), "disabled"
        )
        self.assertEqual(name, "CPU")
        self.assertEqual(disabled.context_attempts, ["CPU"])
        self.assertEqual(attempts, [])
        evidence = worker.openmm_platform_execution_evidence(
            "disabled",
            name,
            worker.available_openmm_platforms(disabled),
            {},
            attempts,
        )
        self.assertEqual(evidence["fallback_reason"], "cuda_disabled_by_policy")

        preferred = FakeOpenMM({"CUDA"})
        _context, name, _properties, attempts = worker.create_openmm_context(
            preferred, object(), object(), "preferred"
        )
        self.assertEqual(name, "CPU")
        self.assertEqual(preferred.context_attempts, ["CUDA", "CPU"])
        self.assertEqual(attempts, ["CUDA:RuntimeError:CUDA unavailable"])
        evidence = worker.openmm_platform_execution_evidence(
            "preferred",
            name,
            worker.available_openmm_platforms(preferred),
            {},
            attempts,
        )
        self.assertEqual(evidence["fallback_reason"], "CUDA:RuntimeError:CUDA unavailable")

        missing_cuda = worker.openmm_platform_execution_evidence(
            "preferred",
            "CPU",
            ["Reference", "CPU"],
            {},
            [],
        )
        self.assertEqual(missing_cuda["fallback_reason"], "CUDA_platform_missing")

        required = FakeOpenMM({"CUDA"})
        with self.assertRaisesRegex(ValueError, "gpu_required_context_unavailable"):
            worker.create_openmm_context(required, object(), object(), "required")

    def test_md_sampling_policy_records_the_effective_budget(self) -> None:
        policy = worker.build_md_sampling_policy(0.001, 100_000, 100, 10_000)
        self.assertEqual(policy["policy_id"], worker.MD_STABILITY_POLICY_ID)
        self.assertEqual(policy["equilibration_time_ps"], 100.0)
        self.assertEqual(policy["production_steps_total"], 1_000_000)
        self.assertEqual(policy["production_time_ps"], 1000.0)
        self.assertEqual(policy["expected_frame_count"], 101)


if __name__ == "__main__":
    unittest.main()
