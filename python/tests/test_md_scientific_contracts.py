from __future__ import annotations

import math
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import numpy as np
from openmm import Vec3, app, unit
from pdbfixer import PDBFixer
from rdkit import Chem

from genoligand_workers import md_summary_worker as worker


MINIMUM_PRODUCTION_SAMPLES = 10


class FixtureAtom:
    def __init__(self, index: int, atomic_number: int) -> None:
        self.index = index
        self.atomic_number = atomic_number

    def GetIdx(self) -> int:
        return self.index

    def GetAtomicNum(self) -> int:
        return self.atomic_number

    def GetSymbol(self) -> str:
        return {6: "C", 8: "O"}[self.atomic_number]


class FixtureBond:
    def __init__(self, begin: int, end: int) -> None:
        self.begin = begin
        self.end = end

    def GetBeginAtomIdx(self) -> int:
        return self.begin

    def GetEndAtomIdx(self) -> int:
        return self.end


class FixtureMolecule:
    def __init__(self) -> None:
        self.atoms = [FixtureAtom(0, 8), FixtureAtom(1, 6)]
        self.bonds = [FixtureBond(0, 1)]

    def GetAtoms(self) -> list[FixtureAtom]:
        return self.atoms

    def GetBonds(self) -> list[FixtureBond]:
        return self.bonds

    def GetNumAtoms(self) -> int:
        return len(self.atoms)

    def GetAtomWithIdx(self, index: int) -> FixtureAtom:
        return self.atoms[index]


def complex_fixture(
    donor_hydrogen_bond: bool = True,
) -> tuple[FixtureMolecule, worker.ReceptorCoordinate, np.ndarray, dict[str, range]]:
    topology = app.Topology()
    chain = topology.addChain("A")
    residue = topology.addResidue("REC", chain)
    nitrogen = topology.addAtom("N", app.Element.getByAtomicNumber(7), residue)
    hydrogen = topology.addAtom("H", app.Element.getByAtomicNumber(1), residue)
    carbon_1 = topology.addAtom("C1", app.Element.getByAtomicNumber(6), residue)
    carbon_2 = topology.addAtom("C2", app.Element.getByAtomicNumber(6), residue)
    if donor_hydrogen_bond:
        topology.addBond(nitrogen, hydrogen)
    topology.addBond(nitrogen, carbon_1)
    topology.addBond(nitrogen, carbon_2)
    receptor = worker.ReceptorCoordinate(
        path=Path("fixture.pdb"),
        topology=topology,
        atoms=[
            {"element": "N", "residue": "REC"},
            {"element": "H", "residue": "REC"},
            {"element": "C", "residue": "REC"},
            {"element": "C", "residue": "REC"},
        ],
        positions=None,
        positions_angstrom=None,
        preparation="fixture",
        risk_flags=[],
    )
    coordinates = np.array(
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
            [2.5, 0.0, 0.0],
            [3.5, 0.0, 0.0],
        ],
        dtype=float,
    )
    return FixtureMolecule(), receptor, coordinates, {"receptor": range(0, 4), "ligand": range(4, 6)}


class MdScientificContractTests(unittest.TestCase):
    def write_receptor(self, root: Path, chains: list[tuple[str, float]], *, zinc: bool = False) -> Path:
        topology = app.Topology()
        positions = []
        for chain_id, x in chains:
            chain = topology.addChain(chain_id)
            residue = topology.addResidue("ALA", chain, id="1")
            topology.addAtom("CA", app.Element.getBySymbol("C"), residue)
            positions.append(Vec3(x, 0.0, 0.0))
        if zinc:
            chain = topology.addChain("Z")
            residue = topology.addResidue("ZN", chain, id="401")
            topology.addAtom("ZN", app.Element.getBySymbol("Zn"), residue)
            positions.append(Vec3(2.0, 0.0, 0.0))
        path = root / "receptor.pdb"
        with path.open("w", encoding="utf-8") as handle:
            app.PDBFile.writeFile(topology, unit.Quantity(positions, unit.angstrom), handle, keepIds=True)
        return path

    def ligand_at(self, x: float) -> object:
        mol = Chem.MolFromSmiles("C")
        conformer = Chem.Conformer(1)
        conformer.SetAtomPosition(0, (x, 0.0, 0.0))
        mol.AddConformer(conformer)
        return mol

    def test_pose_heavy_atom_chain_selection_requires_exactly_one_contact_chain(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            path = self.write_receptor(root, [("B", 20.0), ("A", 0.0)])
            selected = worker.select_receptor_chain_from_pose(path, self.ligand_at(2.0), unit)
            self.assertEqual(selected["selected_chain_id"], "A")
            self.assertEqual(selected["selected_chain_ordinal"], 1)
            self.assertEqual(selected["original_protein_chain_count"], 2)
            self.assertEqual(selected["retained_protein_chain_count"], 1)

            reordered_path = self.write_receptor(root, [("A", 0.0), ("B", 20.0)])
            reordered = worker.select_receptor_chain_from_pose(reordered_path, self.ligand_at(2.0), unit)
            self.assertEqual(reordered["selected_chain_id"], "A")
            self.assertEqual(reordered["selected_chain_ordinal"], 0)
            self.assertEqual(
                reordered["selected_chain_coordinate_sha256"],
                selected["selected_chain_coordinate_sha256"],
            )

            with self.assertRaisesRegex(ValueError, "no_receptor_chain_within_pose_contact_cutoff"):
                worker.select_receptor_chain_from_pose(reordered_path, self.ligand_at(10.0), unit)

            ambiguous = self.write_receptor(root, [("A", 0.0), ("B", 8.0)])
            with self.assertRaisesRegex(ValueError, "ambiguous_receptor_chain_contacts:A,B"):
                worker.select_receptor_chain_from_pose(ambiguous, self.ligand_at(4.0), unit)

            multi_contact = self.write_receptor(root, [("A", 0.0), ("B", 6.0)])
            nearest = worker.select_receptor_chain_from_pose(multi_contact, self.ligand_at(2.0), unit)
            self.assertEqual(nearest["selected_chain_id"], "A")
            self.assertEqual(nearest["selected_chain_ordinal"], 0)

    def test_md_defaults_preserve_duration_with_stable_one_femtosecond_steps(self) -> None:
        self.assertEqual(worker.DEFAULT_TIMESTEP_PS, 0.001)
        self.assertEqual(worker.DEFAULT_EQUILIBRATION_STEPS * worker.DEFAULT_TIMESTEP_PS, 20.0)
        self.assertEqual(
            worker.DEFAULT_PRODUCTION_SEGMENTS
            * worker.DEFAULT_PRODUCTION_STEPS_PER_SEGMENT
            * worker.DEFAULT_TIMESTEP_PS,
            100.0,
        )
        self.assertEqual(worker.DEFAULT_MINIMIZATION_ITERATIONS, 2000)
        self.assertEqual(worker.DEFAULT_SOLVENT_BOX_SHAPE, "dodecahedron")

    def test_receptor_metals_are_structured_before_pdbfixer_can_remove_them(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = self.write_receptor(Path(temp), [("A", 0.0)], zinc=True)
            self.assertEqual(
                worker.receptor_metal_components(path, unit),
                [
                    {
                        "identity": "Z:ZN:401:ZN:1:ZN",
                        "element": "ZN",
                        "atom_name": "ZN",
                        "atom_index": 1,
                        "chain_id": "Z",
                        "chain_ordinal": 1,
                        "residue_name": "ZN",
                        "residue_id": "401",
                    }
                ],
            )

            observed_missing_residues = []
            original_find_missing_atoms = PDBFixer.findMissingAtoms

            def inject_missing_residue(fixer: PDBFixer) -> None:
                fixer.missingResidues = {(0, 0): ["GLY"]}

            def capture_missing_residues(fixer: PDBFixer) -> None:
                observed_missing_residues.append(dict(fixer.missingResidues))
                fixer.missingResidues = {}
                original_find_missing_atoms(fixer)

            with (
                mock.patch.object(PDBFixer, "findMissingResidues", inject_missing_residue),
                mock.patch.object(PDBFixer, "findMissingAtoms", capture_missing_residues),
            ):
                receptor = worker.load_receptor_with_pdbfixer(path, unit, selected_chain_ordinal=0)

            self.assertEqual(observed_missing_residues, [{}])
            self.assertEqual(receptor.preparation, "pdbfixer_observed_residues_prepared_receptor")

    def test_openmm_finite_gate_reports_the_exact_stage(self) -> None:
        class Quantity:
            def __init__(self, value: object) -> None:
                self.value = value

            def value_in_unit(self, _unit: object) -> object:
                return self.value

        class State:
            def getPositions(self, asNumpy: bool = False) -> Quantity:
                return Quantity(np.asarray([[math.nan, 0.0, 0.0]]))

            def getPotentialEnergy(self) -> Quantity:
                return Quantity(0.0)

            def getForces(self, asNumpy: bool = False) -> Quantity:
                return Quantity(np.asarray([[0.0, 0.0, 0.0]]))

        class Context:
            def getState(self, **_kwargs: object) -> State:
                return State()

        with self.assertRaisesRegex(ValueError, "openmm_non_finite_state:post_minimization"):
            worker.assert_openmm_state_finite(Context(), unit, "post_minimization")

        class Integrator:
            def step(self, _steps: int) -> None:
                raise RuntimeError("Particle coordinate is NaN")

        with self.assertRaisesRegex(
            ValueError,
            "openmm_integration_failed:production_segment_7:Particle coordinate is NaN",
        ):
            worker.integrate_openmm(Integrator(), 1000, "production_segment_7")

        class Minimizer:
            @staticmethod
            def minimize(*_args: object, **_kwargs: object) -> None:
                raise RuntimeError("Particle coordinate is NaN")

        class OpenMM:
            LocalEnergyMinimizer = Minimizer

        with self.assertRaisesRegex(
            ValueError,
            "openmm_minimization_failed:post_minimization:Particle coordinate is NaN",
        ):
            worker.minimize_openmm(OpenMM(), object(), unit, 200)

    def test_complex_metrics_share_a_pbc_corrected_receptor_reference_frame(self) -> None:
        mol, receptor, reference, role_ranges = complex_fixture()
        angle = math.radians(73.0)
        rotation = np.array(
            [
                [math.cos(angle), -math.sin(angle), 0.0],
                [math.sin(angle), math.cos(angle), 0.0],
                [0.0, 0.0, 1.0],
            ]
        )
        transformed = reference @ rotation.T + np.array([12.0, 7.0, 3.0])
        transformed[role_ranges["ligand"], :] -= np.array([20.0, 0.0, 0.0])
        frames = [reference] + [transformed.copy() for _ in range(MINIMUM_PRODUCTION_SAMPLES)]
        box = np.diag([20.0, 20.0, 20.0])

        metrics = worker.summarize_complex_frames(
            mol,
            receptor,
            frames,
            role_ranges,
            box_vectors=[box] * len(frames),
            production_steps_total=1000,
        )

        self.assertAlmostEqual(metrics["rmsd"], 0.0, places=4)
        self.assertAlmostEqual(metrics["rmsf"], 0.0, places=4)
        self.assertAlmostEqual(metrics["ligand_drift"], 0.0, places=4)
        self.assertEqual(metrics["contact_occupancy"], 1.0)
        self.assertEqual(metrics["hydrogen_bond_occupancy"], 1.0)

    def test_contact_occupancy_uses_sampled_frames_as_denominator(self) -> None:
        receptor_frames = [
            np.array([[0.0, 0.0, 0.0], [50.0, 0.0, 0.0]]),
            np.array([[0.0, 0.0, 0.0], [50.0, 0.0, 0.0]]),
        ]
        ligand_frames = [np.array([[4.0, 0.0, 0.0]]), np.array([[10.0, 0.0, 0.0]])]

        self.assertEqual(worker.receptor_ligand_contact_occupancy(receptor_frames, ligand_frames), 0.5)

    def test_hbond_requires_donor_hydrogen_acceptor_angle(self) -> None:
        mol, receptor, frame, role_ranges = complex_fixture()
        frame[4] = [1.0, 1.5, 0.0]
        frame[5] = [1.0, 2.5, 0.0]
        frames = [frame.copy() for _ in range(MINIMUM_PRODUCTION_SAMPLES + 1)]

        metrics = worker.summarize_complex_frames(
            mol,
            receptor,
            frames,
            role_ranges,
            production_steps_total=1000,
        )

        self.assertEqual(metrics["hydrogen_bond_occupancy"], 0.0)

        mol, receptor, frame, role_ranges = complex_fixture(donor_hydrogen_bond=False)
        frames = [frame.copy() for _ in range(MINIMUM_PRODUCTION_SAMPLES + 1)]
        metrics = worker.summarize_complex_frames(
            mol,
            receptor,
            frames,
            role_ranges,
            production_steps_total=1000,
        )
        self.assertEqual(metrics["hydrogen_bond_occupancy"], 0.0)

    def test_zero_step_single_frame_and_low_sampling_are_rejected(self) -> None:
        self.assertEqual(
            worker.md_sampling_rejection_reason(0, MINIMUM_PRODUCTION_SAMPLES + 1),
            "zero_production_steps",
        )
        self.assertEqual(worker.md_sampling_rejection_reason(1000, 1), "single_frame_trajectory")
        self.assertEqual(
            worker.md_sampling_rejection_reason(1000, MINIMUM_PRODUCTION_SAMPLES),
            "insufficient_production_samples",
        )
        self.assertEqual(worker.md_sampling_rejection_reason(1000, MINIMUM_PRODUCTION_SAMPLES + 1), "")

        mol, receptor, frame, role_ranges = complex_fixture()
        with self.assertRaisesRegex(ValueError, "insufficient_production_samples"):
            worker.summarize_complex_frames(
                mol,
                receptor,
                [frame.copy() for _ in range(MINIMUM_PRODUCTION_SAMPLES)],
                role_ranges,
                production_steps_total=1000,
            )

    def test_angstrom_scales_are_applied_before_clamping(self) -> None:
        self.assertEqual(worker.md_stability_score(8.0, 6.0, 0.0, 0.0, 0.0), 0.0)
        self.assertAlmostEqual(worker.md_stability_score(2.0, 1.5, 0.0, 0.0, 0.0), 0.275)


if __name__ == "__main__":
    unittest.main()
