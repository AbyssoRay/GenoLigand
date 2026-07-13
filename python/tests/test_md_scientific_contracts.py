from __future__ import annotations

import math
import unittest
from pathlib import Path

import numpy as np
from openmm import app

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
