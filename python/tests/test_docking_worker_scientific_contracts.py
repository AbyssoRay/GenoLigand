from __future__ import annotations

import json
import hashlib
import os
import subprocess
import sys
import tempfile
import types
import unittest
from dataclasses import replace
from pathlib import Path
from unittest.mock import Mock, patch

from genoligand_workers.docking_worker import (
    canonical_digest,
    copy_query_ligand_sdf,
    DockingNotReady,
    ReceptorComponent,
    ReceptorPreparation,
    build_vina_command,
    choose_search_box,
    clear_previous_docking_outputs,
    derive_docking_seed,
    docking_output_dir,
    fmt,
    main,
    parse_vina_affinity,
    prepare_receptor_for_docking,
    resolve_docking_binding,
    retained_component_provenance,
    run_checked,
    run_docking_job,
    worker_timeout_seconds,
    validate_docking_pose_artifacts,
    write_pose_sdf_from_vina,
)


def write_query_structure_fixture(
    run_dir: Path,
    input_id: str,
    record_id: str,
    source_digest: str,
    source_smiles: str,
    standardized_smiles: str | None = None,
) -> tuple[Path, str, str]:
    docking_smiles = standardized_smiles or source_smiles
    input_digest = canonical_digest(
        {
            "input_id": input_id,
            "record_id": record_id,
            "canonical_kind": "small_molecule",
            "canonical_value": source_smiles,
            "source_content_sha256": source_digest,
        }
    )
    output_dir = run_dir / "tool_outputs" / "query_structure" / input_id
    output_dir.mkdir(parents=True, exist_ok=True)
    coordinate = output_dir / "representative.sdf"
    coordinate.write_text(f"{docking_smiles}\n", encoding="utf-8")
    coordinate_digest = hashlib.sha256(coordinate.read_bytes()).hexdigest()
    coordinate_relative = coordinate.relative_to(run_dir).as_posix()
    features = {
        "status": "ready",
        "input_id": input_id,
        "record_id": record_id,
        "input_kind": "small_molecule",
        "input_digest": input_digest,
        "source_content_sha256": source_digest,
        "isomeric_smiles": docking_smiles,
        "standardization": {"selected_isomeric_smiles": docking_smiles},
        "coordinate_artifact": {
            "path": coordinate_relative,
            "sha256": coordinate_digest,
        },
    }
    features_path = output_dir / "features.json"
    features_path.write_text(
        json.dumps(features, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    feature_digest = hashlib.sha256(features_path.read_bytes()).hexdigest()
    manifest = run_dir / "query_structure_manifest.tsv"
    if not manifest.exists():
        manifest.write_text(
            "input_id\trecord_id\tinput_kind\tinput_digest\tmethod\tfeatures_json\t"
            "coordinate_artifact\tfeature_digest\tstatus\n",
            encoding="utf-8",
        )
    with manifest.open("a", encoding="utf-8") as handle:
        handle.write(
            f"{input_id}\t{record_id}\tsmall_molecule\t{input_digest}\trdkit_etkdg_v3\t"
            f"{features_path.relative_to(run_dir).as_posix()}\t{coordinate_relative}\t"
            f"{feature_digest}\tready\n"
        )
    return coordinate, coordinate_digest, feature_digest


class DockingBindingContractTests(unittest.TestCase):
    def test_selected_candidate_binds_ligand_input_and_receptor(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_binding_") as tmp:
            run_dir = Path(tmp)
            receptor_digest = hashlib.sha256(b"ATOM\n").hexdigest()
            (run_dir / "complex_candidate_selection.tsv").write_text(
                "candidate_id\tinput_id\ttarget_id\ttarget_name\torganism\ttarget_class\t"
                "source\tpdb_id\tevidence\trecord_id\treceptor_artifact\tcoordinate_sha256\n"
                "candidate_2\tinput_2\tTARGET_2\tTarget 2\tHomo sapiens\tprotein\t"
                f"BindingDB\t2XYZ\tligand_code=LIG\trecord_2\tstructure_viewer.pdb\t{receptor_digest}\n",
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
            (run_dir / "structure_viewer.pdb").write_bytes(b"ATOM\n")
            (run_dir / "structure_viewer_status.tsv").write_text(
                "status\tcandidate_id\tinput_id\trecord_id\ttarget_id\tpdb_id\tartifact\t"
                "coordinate_sha256\texpected_path\treason\n"
                f"ready\tcandidate_2\tinput_2\trecord_2\tTARGET_2\t2XYZ\tstructure_viewer.pdb\t"
                f"{receptor_digest}\tstore/2xyz.cif\tselected\n",
                encoding="utf-8",
            )
            query_coordinate, query_digest, feature_digest = write_query_structure_fixture(
                run_dir,
                "input_2",
                "record_2",
                "b" * 64,
                "C[C@@H](O)N",
            )

            binding = resolve_docking_binding(run_dir)

            self.assertEqual(binding.candidate_id, "candidate_2")
            self.assertEqual(binding.input_id, "input_2")
            self.assertEqual(binding.target_id, "TARGET_2")
            self.assertEqual(binding.pdb_id, "2XYZ")
            self.assertEqual(binding.ligand_smiles, "C[C@@H](O)N")
            self.assertEqual(binding.source_content_sha256, "b" * 64)
            self.assertEqual(binding.receptor_coordinate, run_dir / "structure_viewer.pdb")
            self.assertEqual(binding.query_structure_coordinate, query_coordinate)
            self.assertEqual(binding.query_structure_coordinate_digest, query_digest)
            self.assertEqual(binding.query_feature_digest, feature_digest)
            self.assertEqual(binding.docking_ligand_smiles, "C[C@@H](O)N")
            expected_candidate_digest = hashlib.sha256(
                json.dumps(
                    {
                        "schema_version": "genoligand_candidate_selection_digest_v1",
                        "candidate_id": "candidate_2",
                        "identity_namespace": "",
                        "input_id": "input_2",
                        "record_id": "record_2",
                        "target_id": "TARGET_2",
                        "pdb_id": "2XYZ",
                        "selection_rank": "",
                        "receptor_artifact": "structure_viewer.pdb",
                        "coordinate_sha256": receptor_digest,
                    },
                    ensure_ascii=True,
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode("utf-8")
            ).hexdigest()
            self.assertEqual(binding.candidate_digest, expected_candidate_digest)

    def test_explicit_candidate_job_selects_one_row_from_top_n_without_cross_binding(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_topn_") as tmp:
            run_dir = Path(tmp)
            receptor_digest = hashlib.sha256(b"ATOM\n").hexdigest()
            (run_dir / "complex_candidate_selection.tsv").write_text(
                "candidate_id\tinput_id\trecord_id\ttarget_id\tpdb_id\treceptor_artifact\t"
                "coordinate_sha256\n"
                f"candidate_1\tinput_1\trecord_1\tTARGET_2\t2XYZ\t1ABC.pdb\t{receptor_digest}\n"
                f"candidate_2\tinput_2\trecord_2\tTARGET_2\t2XYZ\t2XYZ.pdb\t{receptor_digest}\n",
                encoding="utf-8",
            )
            (run_dir / "input_conversion_manifest.tsv").write_text(
                "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\tcontent_sha256\n"
                f"input_1\trecord_1\tsmall_molecule\tCCO\tconverted\t{'a' * 64}\n"
                f"input_2\trecord_2\tsmall_molecule\tCCC\tconverted\t{'b' * 64}\n",
                encoding="utf-8",
            )
            for pdb_id in ("1ABC", "2XYZ"):
                (run_dir / f"{pdb_id}.pdb").write_bytes(b"ATOM\n")
            (run_dir / "structure_viewer_status.tsv").write_text(
                "status\tcandidate_id\tinput_id\trecord_id\ttarget_id\tpdb_id\tartifact\t"
                "coordinate_sha256\n"
                f"ready\tcandidate_1\tinput_1\trecord_1\tTARGET_2\t2XYZ\t1ABC.pdb\t{receptor_digest}\n"
                f"ready\tcandidate_2\tinput_2\trecord_2\tTARGET_2\t2XYZ\t2XYZ.pdb\t{receptor_digest}\n",
                encoding="utf-8",
            )
            write_query_structure_fixture(
                run_dir,
                "input_2",
                "record_2",
                "b" * 64,
                "CCC",
            )

            binding = resolve_docking_binding(
                run_dir, "candidate_2", "input_2", "record_2"
            )

            self.assertEqual(binding.candidate_id, "candidate_2")
            self.assertEqual(binding.input_id, "input_2")
            self.assertEqual(binding.ligand_record_id, "record_2")
            self.assertEqual(binding.target_id, "TARGET_2")
            self.assertEqual(binding.receptor_coordinate, run_dir / "2XYZ.pdb")
            self.assertEqual(len(binding.candidate_digest), 64)
            self.assertEqual(len(binding.input_digest), 64)


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

    def test_known_ligand_code_requires_unique_coordinate_residue(self) -> None:
        components = [
            ReceptorComponent(
                key=f"{chain}:AZM:402",
                residue_name="AZM",
                chain_id=chain,
                residue_id="402",
                center=(float(index), float(index + 1), float(index + 2)),
                heavy_atom_count=13,
                carbon_atom_count=4,
                kind="ligand_candidate",
            )
            for index, chain in enumerate(
                ("H", "C", "A", "F", "B", "G", "D", "E"), start=1
            )
        ]

        with self.assertRaisesRegex(DockingNotReady, "no_trusted_binding_pocket"):
            choose_search_box(
                {"evidence": "bindingdb_onsite_activity_row;ligand_code=AZM;pdb_ids=4XIW"},
                components,
            )

    def test_ligand_code_without_matching_coordinate_residue_is_not_ready(self) -> None:
        components = [
            ReceptorComponent(
                key="A:LIG:401",
                residue_name="LIG",
                chain_id="A",
                residue_id="401",
                center=(1.0, 2.0, 3.0),
                heavy_atom_count=12,
                carbon_atom_count=8,
                kind="ligand_candidate",
            )
        ]

        with self.assertRaisesRegex(DockingNotReady, "no_trusted_binding_pocket"):
            choose_search_box({"evidence": "ligand_code=AZM"}, components)

    def test_multiple_ligand_codes_are_not_a_trusted_binding_pocket(self) -> None:
        components = [
            ReceptorComponent(
                key=f"A:{name}:{residue_id}",
                residue_name=name,
                chain_id="A",
                residue_id=residue_id,
                center=center,
                heavy_atom_count=12,
                carbon_atom_count=6,
                kind="ligand_candidate",
            )
            for name, residue_id, center in (
                ("AZM", "402", (1.0, 2.0, 3.0)),
                ("LIG", "403", (4.0, 5.0, 6.0)),
            )
        ]

        with self.assertRaisesRegex(DockingNotReady, "no_trusted_binding_pocket"):
            choose_search_box(
                {"evidence": "ligand_code=AZM;ligand_code=LIG"}, components
            )

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

            def chains(self):
                seen = set()
                for residue in self._residues:
                    if id(residue.chain) in seen:
                        continue
                    seen.add(id(residue.chain))
                    yield residue.chain

        coordinates = Positions()
        atom_index = 0
        chains = {}

        def residue(chain_id, name, residue_id, elements, center):
            nonlocal atom_index
            chain = chains.setdefault(
                chain_id,
                types.SimpleNamespace(id=chain_id, _residues=[]),
            )
            chain.residues = lambda chain=chain: iter(chain._residues)
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
            chain._residues.append(row)
            return row

        residues = [
            residue("A", "ALA", "1", ("N", "C", "O"), (20.0, 21.0, 18.0)),
            residue("A", "ZN", "301", ("Zn",), (5.0, 5.0, 5.0)),
            residue("A", "HEM", "302", ("C",) * 8 + ("Fe",), (7.0, 7.0, 7.0)),
            residue("B", "LIG", "401", ("C",) * 6 + ("N", "O"), (20.0, 21.0, 22.0)),
            residue("B", "SO4", "402", ("S", "O", "O", "O", "O"), (30.0, 30.0, 30.0)),
        ]
        topology = Topology(residues)

        class FakeFixer:
            last = None

            def __init__(self, filename):
                self.topology = topology
                self.positions = coordinates
                self.missingResidues = {(0, 1): ["GLY"]}
                FakeFixer.last = self

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
            self.assertEqual(FakeFixer.last.missingResidues, {})
            self.assertEqual(
                [(row["chain_id"], row["chain_ordinal"]) for row in prepared.contact_chains],
                [("A", 0)],
            )


class DockingPoseValidationTests(unittest.TestCase):
    VALID_PDBQT = (
        "MODEL 1\n"
        "REMARK VINA RESULT: -7.500 0.000 0.000\n"
        "ATOM      1  C1  LIG A   1       1.000   2.000   3.000  1.00  0.00     0.000 C\n"
        "ENDMDL\n"
    )

    def test_vina_affinity_requires_a_finite_score(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_affinity_") as tmp:
            root = Path(tmp)
            log_path = root / "vina.log"
            pose_path = root / "pose.pdbqt"
            for line in (
                "    1       nan      0.000      0.000\n",
                "    1       inf      0.000      0.000\n",
                "REMARK VINA RESULT: -inf 0.000 0.000\n",
            ):
                with self.subTest(line=line.strip()):
                    log_path.write_text(line, encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, "vina_affinity_non_finite"):
                        parse_vina_affinity(log_path, pose_path)

            log_path.write_text("    1       -7.500      0.000      0.000\n", encoding="utf-8")
            self.assertEqual(parse_vina_affinity(log_path, pose_path), -7.5)

    def test_pose_artifacts_require_parseable_atoms_and_finite_coordinates(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_pose_validation_") as tmp:
            root = Path(tmp)
            pose_pdbqt = root / "pose.pdbqt"
            pose_sdf = root / "pose.sdf"
            pose_sdf.write_text("pose\n", encoding="utf-8")
            valid_chem = self.chem_with_pose([(1.0, 2.0, 3.0)])

            pose_pdbqt.write_text("REMARK VINA RESULT: -7.500 0.000 0.000\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "docking_pose_pdbqt_has_no_atoms"):
                validate_docking_pose_artifacts(pose_pdbqt, pose_sdf, valid_chem)

            pose_pdbqt.write_text(
                self.VALID_PDBQT.replace("   1.000", "     nan"), encoding="utf-8"
            )
            with self.assertRaisesRegex(ValueError, "docking_pose_pdbqt_coordinates_invalid"):
                validate_docking_pose_artifacts(pose_pdbqt, pose_sdf, valid_chem)

            pose_pdbqt.write_text(self.VALID_PDBQT, encoding="utf-8")
            invalid_chem = types.SimpleNamespace(SDMolSupplier=lambda *_args, **_kwargs: [None])
            with self.assertRaisesRegex(ValueError, "docking_pose_sdf_has_no_molecule"):
                validate_docking_pose_artifacts(pose_pdbqt, pose_sdf, invalid_chem)

            non_finite_chem = self.chem_with_pose([(float("nan"), 2.0, 3.0)])
            with self.assertRaisesRegex(ValueError, "docking_pose_sdf_coordinates_invalid"):
                validate_docking_pose_artifacts(pose_pdbqt, pose_sdf, non_finite_chem)

    def test_pose_artifacts_accept_parseable_finite_coordinates(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_pose_valid_") as tmp:
            root = Path(tmp)
            pose_pdbqt = root / "pose.pdbqt"
            pose_sdf = root / "pose.sdf"
            pose_pdbqt.write_text(self.VALID_PDBQT, encoding="utf-8")
            pose_sdf.write_text("pose\n", encoding="utf-8")

            validate_docking_pose_artifacts(
                pose_pdbqt,
                pose_sdf,
                self.chem_with_pose([(1.0, 2.0, 3.0)]),
            )

    def test_numeric_formatting_rejects_non_finite_values(self) -> None:
        self.assertEqual(fmt(-7.5), "-7.5000")
        for value in (float("nan"), float("inf"), float("-inf")):
            with self.subTest(value=value):
                with self.assertRaisesRegex(ValueError, "non_finite_numeric_output"):
                    fmt(value)

    def test_pose_reconstruction_preserves_template_chemistry_and_halogen_identity(self) -> None:
        try:
            from rdkit import Chem
            from rdkit.Chem import AllChem
        except ImportError:
            self.skipTest("RDKit runtime unavailable")

        with tempfile.TemporaryDirectory(prefix="genoligand_pose_atom_map_") as tmp:
            root = Path(tmp)
            query_sdf = root / "query.sdf"
            ligand_sdf = root / "ligand.sdf"
            ligand_pdbqt = root / "ligand.pdbqt"
            pose_pdbqt = root / "pose.pdbqt"
            pose_sdf = root / "pose.sdf"
            molecule = Chem.AddHs(Chem.MolFromSmiles("C[NH+](C)C(Cl)=CBr"))
            self.assertEqual(AllChem.EmbedMolecule(molecule, randomSeed=20260729), 0)
            writer = Chem.SDWriter(str(query_sdf))
            writer.write(molecule)
            writer.close()
            expected_smiles = Chem.MolToSmiles(
                Chem.RemoveHs(Chem.Mol(molecule)), canonical=True, isomericSmiles=True
            )

            copy_query_ligand_sdf(query_sdf, ligand_sdf, Chem, expected_smiles)
            input_lines = []
            pose_lines = ["MODEL 1\n", "REMARK VINA RESULT: -7.500 0.000 0.000\n"]
            conformer = molecule.GetConformer()
            expected_pose = {}
            for atom in molecule.GetAtoms():
                if atom.GetAtomicNum() <= 1:
                    continue
                serial = atom.GetIdx() + 1
                point = conformer.GetAtomPosition(atom.GetIdx())
                element = atom.GetSymbol()
                atom_type = {"Cl": "Cl", "Br": "Br"}.get(element, element)
                input_lines.append(
                    self.pdbqt_atom_line(serial, element, point.x, point.y, point.z, atom_type)
                )
                shifted = (point.x + 1.0, point.y + 2.0, point.z + 3.0)
                expected_pose[serial] = shifted
                pose_lines.append(
                    self.pdbqt_atom_line(serial, element, *shifted, atom_type)
                )
            pose_lines.append("ENDMDL\n")
            ligand_pdbqt.write_text("".join(input_lines), encoding="utf-8")
            pose_pdbqt.write_text("".join(pose_lines), encoding="utf-8")

            metadata = write_pose_sdf_from_vina(
                ligand_sdf, ligand_pdbqt, pose_pdbqt, pose_sdf, Chem
            )
            actual = next(
                molecule
                for molecule in Chem.SDMolSupplier(str(pose_sdf), removeHs=False)
                if molecule is not None
            )
            expected_heavy = Chem.RemoveHs(Chem.Mol(molecule))
            actual_heavy = Chem.RemoveHs(Chem.Mol(actual))
            self.assertEqual(
                Chem.MolToSmiles(actual_heavy, canonical=True, isomericSmiles=True),
                expected_smiles,
            )
            self.assertEqual(
                [atom.GetFormalCharge() for atom in actual_heavy.GetAtoms()],
                [atom.GetFormalCharge() for atom in expected_heavy.GetAtoms()],
            )
            self.assertEqual(
                self.bond_contract(actual_heavy),
                self.bond_contract(expected_heavy),
            )
            self.assertEqual(
                {atom.GetSymbol() for atom in actual_heavy.GetAtoms()} & {"Cl", "Br"},
                {"Cl", "Br"},
            )
            actual_conformer = actual_heavy.GetConformer()
            heavy_original_indices = [
                atom.GetIdx() for atom in molecule.GetAtoms() if atom.GetAtomicNum() > 1
            ]
            for heavy_index, original_index in enumerate(heavy_original_indices):
                point = actual_conformer.GetAtomPosition(heavy_index)
                for actual_value, expected_value in zip(
                    (point.x, point.y, point.z),
                    expected_pose[original_index + 1],
                ):
                    self.assertAlmostEqual(actual_value, expected_value, places=3)
            self.assertEqual(metadata["heavy_atom_count"], expected_heavy.GetNumAtoms())
            self.assertEqual(
                {row["element"] for row in metadata["atoms"]} & {"Cl", "Br"},
                {"Cl", "Br"},
            )

    def test_worker_timeout_configuration_matches_the_shared_contract(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(worker_timeout_seconds(), 21_600)
        with patch.dict(
            os.environ, {"GENOLIGAND_WORKER_TIMEOUT_SECONDS": " 45 "}, clear=True
        ):
            self.assertEqual(worker_timeout_seconds(), 45)
        for value in ("0", "-1", "invalid", "1.5"):
            with self.subTest(value=value), patch.dict(
                os.environ, {"GENOLIGAND_WORKER_TIMEOUT_SECONDS": value}, clear=True
            ):
                with self.assertRaisesRegex(ValueError, "worker_timeout_invalid"):
                    worker_timeout_seconds()

    def test_run_checked_terminates_a_hanging_process_tree(self) -> None:
        process = Mock(pid=4242)
        process.communicate.side_effect = [
            subprocess.TimeoutExpired(["vina"], 1.0),
            ("partial output", ""),
        ]
        with patch(
            "genoligand_workers.docking_worker.subprocess.Popen", return_value=process
        ), patch(
            "genoligand_workers.docking_worker.time.monotonic", return_value=10.0
        ), patch(
            "genoligand_workers.docking_worker.terminate_process_tree"
        ) as terminate:
            with self.assertRaisesRegex(ValueError, "vina_timed_out"):
                run_checked(["vina"], "vina", deadline=11.0)

        terminate.assert_called_once_with(process)
        self.assertEqual(process.communicate.call_count, 2)

    @staticmethod
    def chem_with_pose(coordinates: list[tuple[float, float, float]]) -> object:
        points = [types.SimpleNamespace(x=x, y=y, z=z) for x, y, z in coordinates]
        conformer = types.SimpleNamespace(GetAtomPosition=lambda index: points[index])
        molecule = types.SimpleNamespace(
            GetNumAtoms=lambda: len(points),
            GetNumConformers=lambda: 1,
            GetConformer=lambda: conformer,
        )
        return types.SimpleNamespace(SDMolSupplier=lambda *_args, **_kwargs: [molecule])

    @staticmethod
    def pdbqt_atom_line(
        serial: int,
        element: str,
        x: float,
        y: float,
        z: float,
        atom_type: str,
    ) -> str:
        atom_name = f"{element}{serial}"[:3]
        return (
            f"ATOM  {serial:5d}  {atom_name:<3} LIG A   1    "
            f"{x:8.3f}{y:8.3f}{z:8.3f}  1.00  0.00     0.000 {atom_type}\n"
        )

    @staticmethod
    def bond_contract(molecule: object) -> list[tuple[int, int, str]]:
        return sorted(
            (
                min(bond.GetBeginAtomIdx(), bond.GetEndAtomIdx()),
                max(bond.GetBeginAtomIdx(), bond.GetEndAtomIdx()),
                str(bond.GetBondType()),
            )
            for bond in molecule.GetBonds()
        )


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
            query_structure_coordinate=receptor.with_name("query_structure.sdf"),
            query_structure_coordinate_digest="e" * 64,
            query_feature_digest="f" * 64,
            docking_ligand_smiles="C[C@@H](O)N",
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
                (
                    docking_output_dir(run_dir, "candidate_2", "invocation-current")
                    / "docking_manifest.json"
                ).read_text(
                    encoding="utf-8"
                )
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
            contact_chains = [
                {
                    "chain_id": "A",
                    "chain_ordinal": 0,
                    "chain_signature_sha256": "d" * 64,
                    "minimum_distance_angstrom": 2.5,
                }
            ]
            preparation = ReceptorPreparation(
                box=box,
                receptor_sha256="c" * 64,
                retained_components=retained,
                contact_chains=contact_chains,
            )
            commands: list[tuple[list[str], str]] = []
            pose_map = {
                "algorithm": "pdbqt_preserved_serial_to_rdkit_heavy_atom_v1",
                "heavy_atom_count": 1,
                "atom_map_sha256": "a" * 64,
                "atoms": [
                    {"pdbqt_serial": 1, "rdkit_heavy_atom_index": 0, "element": "C"}
                ],
            }

            def fake_prepare(_source, output, _selection):
                output.write_text("ATOM\n", encoding="utf-8")
                return preparation

            def fake_ligand(source, output, _chem, expected_smiles):
                self.assertEqual(source.name, "representative.sdf")
                self.assertEqual(expected_smiles, "C[C@@H](O)N")
                output.write_text("query ligand\n", encoding="utf-8")

            def fake_pose(_ligand_sdf, _ligand_pdbqt, _pose_pdbqt, output, _chem):
                output.write_text("mapped pose\n", encoding="utf-8")
                return pose_map

            def fake_run(command, label, log_path=None, deadline=None):
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
                "genoligand_workers.docking_worker.copy_query_ligand_sdf",
                side_effect=fake_ligand,
            ) as ligand_writer, patch(
                "genoligand_workers.docking_worker.write_pose_sdf_from_vina",
                side_effect=fake_pose,
            ), patch(
                "genoligand_workers.docking_worker.validate_docking_pose_artifacts"
            ), patch(
                "genoligand_workers.docking_worker.run_checked", side_effect=fake_run
            ), patch(
                "genoligand_workers.docking_worker.shutil.which",
                side_effect=lambda name: f"/tools/{name}",
            ):
                result = run_docking_job(job)

            manifest = json.loads(
                (
                    docking_output_dir(run_dir, "candidate_2", "invocation-current")
                    / "docking_manifest.json"
                ).read_text(
                    encoding="utf-8"
                )
            )
            vina_command = next(command for command, label in commands if label == "vina")
            seed = manifest["seed"]
            self.assertIn("AutoDock Vina docking completed", result)
            self.assertEqual(manifest["status"], "completed")
            self.assertEqual(
                manifest["artifact_schema_version"], "genoligand_docking_artifacts_v1"
            )
            self.assertEqual(manifest["job_id"], "docking_vina")
            self.assertEqual(manifest["run_id"], run_dir.name)
            self.assertEqual(manifest["invocation_id"], "invocation-current")
            self.assertEqual(manifest["candidate_id"], "candidate_2")
            self.assertEqual(manifest["input_id"], "input_2")
            self.assertEqual(manifest["target_id"], "TARGET_2")
            self.assertEqual(manifest["pdb_id"], "2XYZ")
            self.assertEqual(manifest["ligand_smiles"], "C[C@@H](O)N")
            self.assertEqual(manifest["receptor_sha256"], "c" * 64)
            self.assertEqual(manifest["search_box"]["source"], "selected_pocket")
            self.assertEqual(manifest["retained_receptor_components"], retained)
            self.assertEqual(manifest["receptor_contact_chains"], contact_chains)
            self.assertEqual(manifest["pose_atom_map"], pose_map)
            self.assertEqual(manifest["docking_ligand_smiles"], "C[C@@H](O)N")
            self.assertEqual(
                manifest["query_structure_coordinate"],
                "tool_outputs/query_structure/input_2/representative.sdf",
            )
            self.assertEqual(manifest["vina_command"], vina_command)
            self.assertEqual(vina_command[vina_command.index("--seed") + 1], str(seed))
            self.assertEqual(manifest["seed_provenance"]["algorithm"], "sha256_canonical_v1")
            self.assertEqual(len(manifest["seed_provenance"]["invocation_digest"]), 64)
            self.assertEqual(len(manifest["seed_provenance"]["input_digest"]), 64)
            self.assertEqual(
                manifest["input_digest"], manifest["seed_provenance"]["input_digest"]
            )
            self.assertEqual(len(manifest["seed_provenance"]["seed_digest"]), 64)
            self.assertEqual(ligand_writer.call_args.args[3], "C[C@@H](O)N")
            self.assertEqual(
                {row["artifact_name"] for row in manifest["artifacts"]},
                {
                    "receptor_coordinate",
                    "receptor_pdb",
                    "receptor_pdbqt",
                    "ligand_sdf",
                    "ligand_pdbqt",
                    "pose",
                    "pose_sdf",
                    "log",
                },
            )
            self.assertEqual(len(manifest["artifacts"]), 8)
            for row in manifest["artifacts"]:
                self.assertEqual(row["run_id"], run_dir.name)
                self.assertEqual(row["candidate_id"], "candidate_2")
                self.assertEqual(row["input_id"], "input_2")
                self.assertEqual(row["record_id"], "record_2")
                self.assertEqual(len(row["artifact_sha256"]), 64)

    @staticmethod
    def job(run_dir: Path) -> dict[str, object]:
        binding = resolve_docking_binding(run_dir)
        return {
            "schema_version": "genoligand_worker_current_v2",
            "pipeline_mode": "genoligand_scientific_pipeline",
            "job_id": "docking_vina",
            "worker": "docking",
            "stage": "structure_evaluate",
            "run_id": run_dir.name,
            "invocation_id": "invocation-current",
            "run_dir": str(run_dir),
            "input_manifest": str(run_dir / "input.json"),
            "candidate_id": binding.candidate_id,
            "candidate_digest": binding.candidate_digest,
            "input_id": binding.input_id,
            "record_id": binding.ligand_record_id,
            "input_digest": binding.input_digest,
            "source_digest": binding.source_content_sha256,
            "receptor_digest": binding.receptor_digest,
        }

    @staticmethod
    def write_binding_fixture(run_dir: Path) -> None:
        receptor_digest = hashlib.sha256(b"ATOM\n").hexdigest()
        (run_dir / "complex_candidate_selection.tsv").write_text(
            "candidate_id\tinput_id\ttarget_id\ttarget_name\torganism\ttarget_class\t"
            "source\tpdb_id\tevidence\trecord_id\treceptor_artifact\tcoordinate_sha256\n"
            "candidate_2\tinput_2\tTARGET_2\tTarget 2\tHomo sapiens\tprotein\t"
            f"BindingDB\t2XYZ\tligand_code=LIG\trecord_2\tstructure_viewer.pdb\t{receptor_digest}\n",
            encoding="utf-8",
        )
        (run_dir / "input_conversion_manifest.tsv").write_text(
            "input_id\trecord_id\tcanonical_kind\tcanonical_value\tconversion_status\t"
            "content_sha256\n"
            f"input_2\trecord_2\tsmall_molecule\tC[C@@H](O)N\tconverted\t{'b' * 64}\n",
            encoding="utf-8",
        )
        write_query_structure_fixture(
            run_dir,
            "input_2",
            "record_2",
            "b" * 64,
            "C[C@@H](O)N",
        )
        (run_dir / "structure_viewer.pdb").write_bytes(b"ATOM\n")
        (run_dir / "structure_viewer_status.tsv").write_text(
            "status\tcandidate_id\tinput_id\trecord_id\ttarget_id\tpdb_id\tartifact\t"
            "coordinate_sha256\texpected_path\treason\n"
            f"ready\tcandidate_2\tinput_2\trecord_2\tTARGET_2\t2XYZ\tstructure_viewer.pdb\t"
            f"{receptor_digest}\tstore/2xyz.cif\tselected\n",
            encoding="utf-8",
        )

    def test_windows_run_relative_receptor_artifact_resolves_inside_current_run(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_windows_relative_") as tmp:
            run_dir = Path(tmp)
            self.write_binding_fixture(run_dir)
            receptor = run_dir / "structure_candidates" / "target_2" / "receptor.cif"
            receptor.parent.mkdir(parents=True)
            (run_dir / "structure_viewer.pdb").replace(receptor)
            windows_relative = r"structure_candidates\target_2\receptor.cif"
            for name in ("complex_candidate_selection.tsv", "structure_viewer_status.tsv"):
                path = run_dir / name
                path.write_text(
                    path.read_text(encoding="utf-8").replace(
                        "structure_viewer.pdb", windows_relative
                    ),
                    encoding="utf-8",
                )

            binding = resolve_docking_binding(run_dir)

            self.assertEqual(binding.receptor_coordinate, receptor.resolve())
            self.assertEqual(self.job(run_dir)["receptor_digest"], binding.receptor_digest)

    def test_receptor_artifact_must_stay_inside_current_run(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_docking_outside_") as tmp:
            root = Path(tmp)
            outside = root / "outside.pdb"
            outside.write_bytes(b"ATOM\n")
            receptor_digest = hashlib.sha256(outside.read_bytes()).hexdigest()
            escaped_paths = (
                str(outside),
                r"..\outside.pdb",
                r"C:\outside\receptor.cif",
                r"\\server\share\receptor.cif",
            )
            for index, escaped in enumerate(escaped_paths):
                with self.subTest(escaped=escaped):
                    run_dir = root / f"run-{index}"
                    run_dir.mkdir()
                    self.write_binding_fixture(run_dir)
                    selection = run_dir / "complex_candidate_selection.tsv"
                    selection.write_text(
                        selection.read_text(encoding="utf-8").replace(
                            "structure_viewer.pdb", escaped
                        ),
                        encoding="utf-8",
                    )
                    (run_dir / "structure_viewer_status.tsv").write_text(
                        "status\tcandidate_id\tinput_id\trecord_id\ttarget_id\tpdb_id\tartifact\t"
                        "coordinate_sha256\texpected_path\treason\n"
                        f"ready\tcandidate_2\tinput_2\trecord_2\tTARGET_2\t2XYZ\t{escaped}\t"
                        f"{receptor_digest}\tstore/2xyz.cif\tselected\n",
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
