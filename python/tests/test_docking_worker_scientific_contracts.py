from __future__ import annotations

import json
import sys
import tempfile
import types
import unittest
from dataclasses import replace
from pathlib import Path
from unittest.mock import patch

from genoligand_workers.docking_worker import (
    DockingNotReady,
    ReceptorComponent,
    ReceptorPreparation,
    build_vina_command,
    choose_search_box,
    clear_previous_docking_outputs,
    derive_docking_seed,
    main,
    prepare_receptor_for_docking,
    resolve_docking_binding,
    retained_component_provenance,
    run_docking_job,
)


class DockingBindingContractTests(unittest.TestCase):
    def test_selected_candidate_binds_ligand_input_and_receptor(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_binding_") as tmp:
            run_dir = Path(tmp)
            (run_dir / "complex_candidate_selection.tsv").write_text(
                "candidate_id\tinput_id\ttarget_id\ttarget_name\torganism\ttarget_class\t"
                "source\tpdb_id\tevidence\n"
                "candidate_2\tinput_2\tTARGET_2\tTarget 2\tHomo sapiens\tprotein\t"
                "BindingDB\t2XYZ\tligand_code=LIG\n",
                encoding="utf-8",
            )
            (run_dir / "input_conversion_manifest.tsv").write_text(
                "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\t"
                "content_sha256\n"
                f"input_1\trecord_1\tsmall_molecule\tC[C@H](O)N\tconverted\t{'a' * 64}\n"
                f"input_2\trecord_2\tsmall_molecule\tC[C@@H](O)N\tconverted\t{'b' * 64}\n",
                encoding="utf-8",
            )
            (run_dir / "known_database_matches.tsv").write_text(
                "input_id\tmatched_features\tevidence\n"
                "input_1\tligand_smiles=WRONG\tligand_smiles=WRONG\n",
                encoding="utf-8",
            )
            (run_dir / "structure_viewer.pdb").write_text("ATOM\n", encoding="utf-8")
            (run_dir / "structure_viewer_status.tsv").write_text(
                "status\ttarget_id\tpdb_id\tartifact\texpected_path\treason\n"
                "ready\tTARGET_2\t2XYZ\tstructure_viewer.pdb\tstore/2xyz.cif\tselected\n",
                encoding="utf-8",
            )

            binding = resolve_docking_binding(run_dir)

            self.assertEqual(binding.candidate_id, "candidate_2")
            self.assertEqual(binding.input_id, "input_2")
            self.assertEqual(binding.target_id, "TARGET_2")
            self.assertEqual(binding.pdb_id, "2XYZ")
            self.assertEqual(binding.ligand_smiles, "C[C@@H](O)N")
            self.assertEqual(binding.source_content_sha256, "b" * 64)
            self.assertEqual(binding.receptor_coordinate, run_dir / "structure_viewer.pdb")


class DockingPocketContractTests(unittest.TestCase):
    def test_selected_pocket_with_provenance_takes_precedence(self) -> None:
        selection = {
            "pocket_center_x": "3.25",
            "pocket_center_y": "-4.5",
            "pocket_center_z": "8",
            "pocket_id": "pocket_7",
            "pocket_source": "selected_structure_patch",
            "evidence": "ligand_code=LIG",
        }
        ligand = ReceptorComponent(
            key="B:LIG:401",
            residue_name="LIG",
            chain_id="B",
            residue_id="401",
            center=(91.0, 92.0, 93.0),
            heavy_atom_count=12,
            carbon_atom_count=8,
            kind="ligand_candidate",
        )

        box = choose_search_box(selection, [ligand])

        self.assertEqual(box.center, (3.25, -4.5, 8.0))
        self.assertEqual(box.source, "selected_pocket")
        self.assertEqual(box.source_id, "pocket_7")
        self.assertIsNone(box.excluded_component_key)

    def test_cocrystal_ligand_centers_box_and_retains_metal_cofactor_provenance(self) -> None:
        components = [
            ReceptorComponent(
                key="A:ZN:301",
                residue_name="ZN",
                chain_id="A",
                residue_id="301",
                center=(10.0, 11.0, 12.0),
                heavy_atom_count=1,
                carbon_atom_count=0,
                kind="metal",
            ),
            ReceptorComponent(
                key="A:HEM:302",
                residue_name="HEM",
                chain_id="A",
                residue_id="302",
                center=(13.0, 14.0, 15.0),
                heavy_atom_count=43,
                carbon_atom_count=34,
                kind="cofactor",
            ),
            ReceptorComponent(
                key="B:LIG:401",
                residue_name="LIG",
                chain_id="B",
                residue_id="401",
                center=(21.5, 22.5, 23.5),
                heavy_atom_count=12,
                carbon_atom_count=8,
                kind="ligand_candidate",
            ),
        ]

        box = choose_search_box({"evidence": "ligand_code=LIG"}, components)
        retained = retained_component_provenance(components, box)

        self.assertEqual(box.center, (21.5, 22.5, 23.5))
        self.assertEqual(box.source, "co_crystal_ligand")
        self.assertEqual(box.source_id, "B:LIG:401")
        self.assertEqual(box.excluded_component_key, "B:LIG:401")
        self.assertEqual([row["residue_name"] for row in retained], ["ZN", "HEM"])
        self.assertTrue(all(row["retained"] for row in retained))

    def test_missing_or_ambiguous_pocket_is_not_ready(self) -> None:
        ambiguous = [
            ReceptorComponent(
                key=f"A:{name}:{index}",
                residue_name=name,
                chain_id="A",
                residue_id=str(index),
                center=(float(index), 0.0, 0.0),
                heavy_atom_count=10,
                carbon_atom_count=6,
                kind="ligand_candidate",
            )
            for index, name in enumerate(("L01", "L02"), start=1)
        ]

        with self.assertRaisesRegex(DockingNotReady, "no_trusted_binding_pocket"):
            choose_search_box({}, ambiguous)

    def test_receptor_preparation_keeps_metal_and_cofactor_only(self) -> None:
        class Positions(list):
            def value_in_unit(self, _unit):
                return self

        class Topology:
            def __init__(self, residues):
                self._residues = list(residues)

            def residues(self):
                return iter(self._residues)

            def atoms(self):
                return (atom for residue in self._residues for atom in residue._atoms)

        coordinates = Positions()
        atom_index = 0

        def residue(chain_id, name, residue_id, elements, center):
            nonlocal atom_index
            chain = types.SimpleNamespace(id=chain_id)
            row = types.SimpleNamespace(
                chain=chain,
                name=name,
                id=residue_id,
                index=int(residue_id),
            )
            atoms = []
            for offset, element in enumerate(elements):
                atom = types.SimpleNamespace(
                    index=atom_index,
                    name=f"{element}{offset + 1}",
                    element=types.SimpleNamespace(symbol=element),
                )
                atoms.append(atom)
                coordinates.append((center[0] + offset * 0.1, center[1], center[2]))
                atom_index += 1
            row._atoms = atoms
            row.atoms = lambda: iter(row._atoms)
            return row

        residues = [
            residue("A", "ALA", "1", ("N", "C", "O"), (0.0, 0.0, 0.0)),
            residue("A", "ZN", "301", ("Zn",), (5.0, 5.0, 5.0)),
            residue("A", "HEM", "302", ("C",) * 8 + ("Fe",), (7.0, 7.0, 7.0)),
            residue("B", "LIG", "401", ("C",) * 6 + ("N", "O"), (20.0, 21.0, 22.0)),
            residue("B", "SO4", "402", ("S", "O", "O", "O", "O"), (30.0, 30.0, 30.0)),
        ]
        topology = Topology(residues)

        class FakeFixer:
            def __init__(self, filename):
                self.topology = topology
                self.positions = coordinates

            def findMissingResidues(self):
                pass

            def findNonstandardResidues(self):
                pass

            def replaceNonstandardResidues(self):
                pass

            def findMissingAtoms(self):
                pass

            def addMissingAtoms(self):
                pass

            def addMissingHydrogens(self, _ph):
                pass

        class FakeModeller:
            def __init__(self, source_topology, source_positions):
                self.topology = Topology(list(source_topology.residues()))
                self.positions = source_positions

            def delete(self, removed):
                removed_ids = {id(item) for item in removed}
                self.topology = Topology(
                    [item for item in self.topology.residues() if id(item) not in removed_ids]
                )

        write_options = {}

        class FakePDBFile:
            @staticmethod
            def writeFile(output_topology, _positions, handle, keepIds=False):
                write_options["keep_ids"] = keepIds
                handle.write(",".join(item.name for item in output_topology.residues()))

        openmm = types.ModuleType("openmm")
        openmm.app = types.SimpleNamespace(Modeller=FakeModeller, PDBFile=FakePDBFile)
        openmm.unit = types.SimpleNamespace(angstrom=object())
        pdbfixer = types.ModuleType("pdbfixer")
        pdbfixer.PDBFixer = FakeFixer

        with tempfile.TemporaryDirectory(prefix="genoligand_receptor_prep_") as tmp, patch.dict(
            sys.modules, {"openmm": openmm, "pdbfixer": pdbfixer}
        ):
            root = Path(tmp)
            source = root / "source.pdb"
            output = root / "prepared.pdb"
            source.write_text("fixture coordinate\n", encoding="utf-8")

            prepared = prepare_receptor_for_docking(
                source,
                output,
                {"evidence": "ligand_code=LIG"},
            )

            self.assertEqual(prepared.box.source, "co_crystal_ligand")
            self.assertAlmostEqual(prepared.box.center[0], 20.35)
            self.assertEqual(
                [item["residue_name"] for item in prepared.retained_components],
                ["ZN", "HEM"],
            )
            self.assertTrue(write_options["keep_ids"])
            self.assertEqual(output.read_text(encoding="utf-8"), "ALA,ZN,HEM")


class DockingReplayContractTests(unittest.TestCase):
    def test_seed_is_path_independent_and_input_sensitive(self) -> None:
        binding = self.binding(Path("D:/runs/first/structure_viewer.pdb"))
        box = choose_search_box(
            {
                "pocket_center_x": "1",
                "pocket_center_y": "2",
                "pocket_center_z": "3",
                "pocket_source": "selected_structure_patch",
            },
            [],
        )
        job = {
            "schema_version": "genoligand_worker_current_v2",
            "pipeline_mode": "genoligand_scientific_pipeline",
            "job_id": "docking_vina",
            "worker": "docking_vina",
            "stage": "structure_evaluate",
            "run_dir": "D:/runs/first",
            "input_manifest": "D:/runs/first/input.json",
        }

        first = derive_docking_seed(job, binding, "c" * 64, box)
        replay = derive_docking_seed(
            {**job, "run_dir": "E:/replay", "input_manifest": "E:/replay/input.json"},
            replace(binding, receptor_coordinate=Path("E:/replay/structure_viewer.pdb")),
            "c" * 64,
            box,
        )
        changed_input = derive_docking_seed(
            job,
            replace(binding, source_content_sha256="d" * 64),
            "c" * 64,
            box,
        )

        self.assertEqual(first.seed, replay.seed)
        self.assertEqual(first.seed_digest, replay.seed_digest)
        self.assertNotEqual(first.seed, changed_input.seed)
        self.assertGreater(first.seed, 0)
        self.assertLessEqual(first.seed, 2_147_483_647)

    def test_vina_command_contains_derived_seed(self) -> None:
        command = build_vina_command(
            "vina",
            Path("receptor.pdbqt"),
            Path("ligand.pdbqt"),
            Path("pose.pdbqt"),
            (1.0, 2.0, 3.0),
            912345,
        )

        seed_index = command.index("--seed")
        self.assertEqual(command[seed_index + 1], "912345")

    @staticmethod
    def binding(receptor: Path):
        from genoligand_workers.docking_worker import DockingBinding

        return DockingBinding(
            candidate_id="candidate_2",
            input_id="input_2",
            target_id="TARGET_2",
            pdb_id="2XYZ",
            ligand_smiles="C[C@@H](O)N",
            ligand_record_id="record_2",
            source_content_sha256="b" * 64,
            receptor_coordinate=receptor,
            selection={"candidate_id": "candidate_2", "input_id": "input_2"},
        )


class DockingWorkerOutputContractTests(unittest.TestCase):
    def test_main_reports_not_ready_worker_result_for_blocked_docking(self) -> None:
        job = {
            "job_id": "docking_vina",
            "worker": "docking_vina",
            "run_dir": "unused",
        }
        with patch(
            "genoligand_workers.docking_worker.parse_worker_args",
            return_value=types.SimpleNamespace(job="job.json", out="result.json"),
        ), patch(
            "genoligand_workers.docking_worker.read_job", return_value=job
        ), patch(
            "genoligand_workers.docking_worker.run_docking_job",
            return_value="docking=not_ready;reason=no_trusted_binding_pocket",
        ), patch(
            "genoligand_workers.docking_worker.write_result"
        ) as result_writer:
            exit_code = main()

        self.assertEqual(exit_code, 2)
        self.assertEqual(result_writer.call_args.args[2], "NOT_READY")
        self.assertEqual(result_writer.call_args.kwargs["execution_status"], "not_ready")
        self.assertEqual(result_writer.call_args.kwargs["reason"], "no_trusted_binding_pocket")
        self.assertEqual(
            result_writer.call_args.kwargs["outcomes"],
            [
                {
                    "status": "not_ready",
                    "reason": "no_trusted_binding_pocket",
                    "required": True,
                    "database_id": "docking_vina",
                    "row_count": 0,
                }
            ],
        )

    def test_no_trusted_pocket_writes_blocked_not_ready_output(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_not_ready_") as tmp:
            run_dir = Path(tmp)
            self.write_binding_fixture(run_dir)
            job = self.job(run_dir)

            with patch(
                "genoligand_workers.docking_worker.prepare_receptor_for_docking",
                side_effect=DockingNotReady("no_trusted_binding_pocket"),
            ), patch("genoligand_workers.docking_worker.shutil.which") as which:
                result = run_docking_job(job)

            self.assertEqual(result, "docking=not_ready;reason=no_trusted_binding_pocket")
            which.assert_not_called()
            manifest = json.loads(
                (run_dir / "tool_outputs/docking_manifest.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["status"], "not_ready")
            self.assertEqual(manifest["execution_state"], "blocked")
            self.assertEqual(manifest["reason"], "no_trusted_binding_pocket")
            self.assertEqual(manifest["candidate_id"], "candidate_2")
            self.assertEqual(manifest["input_id"], "input_2")
            self.assertEqual(manifest["target_id"], "TARGET_2")
            self.assertEqual(manifest["pdb_id"], "2XYZ")

    def test_completed_output_records_binding_provenance_seed_and_command(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_complete_") as tmp:
            run_dir = Path(tmp)
            self.write_binding_fixture(run_dir)
            job = self.job(run_dir)
            box = choose_search_box(
                {
                    "pocket_center_x": "1",
                    "pocket_center_y": "2",
                    "pocket_center_z": "3",
                    "pocket_source": "selected_structure_patch",
                },
                [],
            )
            retained = [
                {
                    "component_id": "A:ZN:301",
                    "residue_name": "ZN",
                    "classification": "metal",
                    "retained": True,
                }
            ]
            preparation = ReceptorPreparation(
                box=box,
                receptor_sha256="c" * 64,
                retained_components=retained,
            )
            commands: list[tuple[list[str], str]] = []

            def fake_prepare(_source, output, _selection):
                output.write_text("ATOM\n", encoding="utf-8")
                return preparation

            def fake_ligand(_smiles, output, _chem, _all_chem, *, seed):
                output.write_text(f"seed={seed}\n", encoding="utf-8")

            def fake_run(command, label, log_path=None):
                commands.append((list(command), label))
                if label == "vina":
                    log_path.write_text("    1       -7.500      0.000      0.000\n", encoding="utf-8")
                    Path(command[command.index("--out") + 1]).write_text(
                        "REMARK VINA RESULT: -7.500 0.000 0.000\n", encoding="utf-8"
                    )
                elif "-O" in command:
                    Path(command[command.index("-O") + 1]).write_text(label, encoding="utf-8")

            rdkit = types.ModuleType("rdkit")
            chem = types.ModuleType("rdkit.Chem")
            chem.AllChem = object()
            rdkit.Chem = chem
            rdkit.rdBase = types.SimpleNamespace(rdkitVersion="test-rdkit")
            with patch.dict(sys.modules, {"rdkit": rdkit, "rdkit.Chem": chem}), patch(
                "genoligand_workers.docking_worker.prepare_receptor_for_docking",
                side_effect=fake_prepare,
            ), patch(
                "genoligand_workers.docking_worker.write_ligand_sdf",
                side_effect=fake_ligand,
            ) as ligand_writer, patch(
                "genoligand_workers.docking_worker.run_checked", side_effect=fake_run
            ), patch(
                "genoligand_workers.docking_worker.shutil.which",
                side_effect=lambda name: f"/tools/{name}",
            ):
                result = run_docking_job(job)

            manifest = json.loads(
                (run_dir / "tool_outputs/docking_manifest.json").read_text(encoding="utf-8")
            )
            vina_command = next(command for command, label in commands if label == "vina")
            seed = manifest["seed"]
            self.assertIn("AutoDock Vina docking completed", result)
            self.assertEqual(manifest["status"], "completed")
            self.assertEqual(manifest["job_id"], "docking_vina")
            self.assertEqual(manifest["invocation_id"], "invocation-current")
            self.assertEqual(manifest["candidate_id"], "candidate_2")
            self.assertEqual(manifest["input_id"], "input_2")
            self.assertEqual(manifest["target_id"], "TARGET_2")
            self.assertEqual(manifest["pdb_id"], "2XYZ")
            self.assertEqual(manifest["ligand_smiles"], "C[C@@H](O)N")
            self.assertEqual(manifest["receptor_sha256"], "c" * 64)
            self.assertEqual(manifest["search_box"]["source"], "selected_pocket")
            self.assertEqual(manifest["retained_receptor_components"], retained)
            self.assertEqual(manifest["vina_command"], vina_command)
            self.assertEqual(vina_command[vina_command.index("--seed") + 1], str(seed))
            self.assertEqual(manifest["seed_provenance"]["algorithm"], "sha256_canonical_v1")
            self.assertEqual(len(manifest["seed_provenance"]["invocation_digest"]), 64)
            self.assertEqual(len(manifest["seed_provenance"]["input_digest"]), 64)
            self.assertEqual(
                manifest["input_digest"], manifest["seed_provenance"]["input_digest"]
            )
            self.assertEqual(len(manifest["seed_provenance"]["seed_digest"]), 64)
            self.assertEqual(ligand_writer.call_args.kwargs["seed"], seed)

    @staticmethod
    def job(run_dir: Path) -> dict[str, object]:
        return {
            "schema_version": "genoligand_worker_current_v2",
            "pipeline_mode": "genoligand_scientific_pipeline",
            "job_id": "docking_vina",
            "worker": "docking_vina",
            "stage": "structure_evaluate",
            "invocation_id": "invocation-current",
            "run_dir": str(run_dir),
            "input_manifest": str(run_dir / "input.json"),
        }

    @staticmethod
    def write_binding_fixture(run_dir: Path) -> None:
        (run_dir / "complex_candidate_selection.tsv").write_text(
            "candidate_id\tinput_id\ttarget_id\ttarget_name\torganism\ttarget_class\t"
            "source\tpdb_id\tevidence\n"
            "candidate_2\tinput_2\tTARGET_2\tTarget 2\tHomo sapiens\tprotein\t"
            "BindingDB\t2XYZ\tligand_code=LIG\n",
            encoding="utf-8",
        )
        (run_dir / "input_conversion_manifest.tsv").write_text(
            "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\t"
            "content_sha256\n"
            f"input_2\trecord_2\tsmall_molecule\tC[C@@H](O)N\tconverted\t{'b' * 64}\n",
            encoding="utf-8",
        )
        (run_dir / "structure_viewer.pdb").write_text("ATOM\n", encoding="utf-8")
        (run_dir / "structure_viewer_status.tsv").write_text(
            "status\ttarget_id\tpdb_id\tartifact\texpected_path\treason\n"
            "ready\tTARGET_2\t2XYZ\tstructure_viewer.pdb\tstore/2xyz.cif\tselected\n",
            encoding="utf-8",
        )

    def test_receptor_artifact_must_stay_inside_current_run(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_outside_") as tmp:
            root = Path(tmp)
            run_dir = root / "run"
            run_dir.mkdir()
            self.write_binding_fixture(run_dir)
            outside = root / "outside.pdb"
            outside.write_text("ATOM\n", encoding="utf-8")
            (run_dir / "structure_viewer_status.tsv").write_text(
                "status\ttarget_id\tpdb_id\tartifact\texpected_path\treason\n"
                f"ready\tTARGET_2\t2XYZ\t{outside}\tstore/2xyz.cif\tselected\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(
                DockingNotReady, "artifact_outside_current_run"
            ):
                resolve_docking_binding(run_dir)

    def test_rerun_removes_previous_docking_outputs(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_cleanup_") as tmp:
            run_dir = Path(tmp)
            tool_outputs = run_dir / "tool_outputs"
            tool_outputs.mkdir()
            stale = [
                run_dir / "docking_summary.tsv",
                tool_outputs / "docking_summary.tsv",
                tool_outputs / "docking_manifest.json",
                tool_outputs / "docking_pose.sdf",
                tool_outputs / "docking_vina.log",
            ]
            for path in stale:
                path.write_text("stale\n", encoding="utf-8")

            clear_previous_docking_outputs(run_dir)

            self.assertTrue(all(not path.exists() for path in stale))


if __name__ == "__main__":
    unittest.main()
