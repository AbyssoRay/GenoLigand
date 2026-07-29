from __future__ import annotations

import hashlib
import json
import math
from typing import Any, Iterable


STANDARD_PROTEIN_RESIDUES = {
    "ALA", "ARG", "ASN", "ASP", "CYS", "GLN", "GLU", "GLY", "HIS", "ILE",
    "LEU", "LYS", "MET", "PHE", "PRO", "SER", "THR", "TRP", "TYR", "VAL",
}
POLYMER_RESIDUES = STANDARD_PROTEIN_RESIDUES | {
    "A", "C", "G", "U", "I", "DA", "DC", "DG", "DT", "DI", "N", "UNK",
}
WATER_RESIDUES = {"HOH", "WAT", "DOD"}
BIOLOGICAL_METALS = {"CA", "CO", "CU", "FE", "MG", "MN", "MO", "NI", "V", "W", "ZN"}
COFACTOR_RESIDUES = {
    "ADP", "ATP", "COA", "FAD", "FMN", "HEA", "HEB", "HEC", "HEM", "NAD",
    "NAP", "NDP", "PQQ", "PLP", "SAM", "SAH", "SF4", "F3S", "FES", "MOLY",
}
RECEPTOR_CHAIN_CONTACT_CUTOFF_ANGSTROM = 4.5


def chain_signature(chain: Any) -> str:
    residues = [
        [
            str(residue.name).upper(),
            str(residue.id),
            [
                str(atom.name)
                for atom in residue.atoms()
                if getattr(atom, "element", None) is not None
            ],
        ]
        for residue in chain.residues()
        if str(residue.name).upper() in STANDARD_PROTEIN_RESIDUES
    ]
    return hashlib.sha256(
        json.dumps(residues, separators=(",", ":")).encode("utf-8")
    ).hexdigest()


def receptor_contact_chains(
    topology: Any,
    positions_angstrom: Any,
    contact_points: Iterable[tuple[float, float, float]],
    cutoff_angstrom: float,
) -> list[dict[str, Any]]:
    points = [tuple(float(value) for value in point) for point in contact_points]
    if not points or any(len(point) != 3 or not all(math.isfinite(value) for value in point) for point in points):
        raise ValueError("receptor_contact_points_invalid")
    rows: list[dict[str, Any]] = []
    for chain_ordinal, chain in enumerate(topology.chains()):
        heavy_atoms = [
            atom
            for residue in chain.residues()
            if str(residue.name).upper() in STANDARD_PROTEIN_RESIDUES
            for atom in residue.atoms()
            if getattr(atom, "element", None) is not None
            and str(atom.element.symbol).upper() != "H"
        ]
        if not heavy_atoms:
            continue
        minimum = min(
            math.dist(
                tuple(float(positions_angstrom[atom.index][axis]) for axis in range(3)),
                point,
            )
            for atom in heavy_atoms
            for point in points
        )
        if minimum > cutoff_angstrom:
            continue
        rows.append(
            {
                "chain_id": str(chain.id),
                "chain_ordinal": chain_ordinal,
                "chain_signature_sha256": chain_signature(chain),
                "minimum_distance_angstrom": minimum,
            }
        )
    return sorted(rows, key=lambda row: (row["minimum_distance_angstrom"], row["chain_ordinal"]))
