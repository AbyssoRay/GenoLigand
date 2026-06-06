from __future__ import annotations

import itertools
import math
from pathlib import Path

from .io import bool_text, fmt, read_tsv, sha256_text, write_tsv

AA3 = {
    "ALA": "A",
    "ARG": "R",
    "ASN": "N",
    "ASP": "D",
    "CYS": "C",
    "GLN": "Q",
    "GLU": "E",
    "GLY": "G",
    "HIS": "H",
    "ILE": "I",
    "LEU": "L",
    "LYS": "K",
    "MET": "M",
    "PHE": "F",
    "PRO": "P",
    "SER": "S",
    "THR": "T",
    "TRP": "W",
    "TYR": "Y",
    "VAL": "V",
}
HYDRO = set("AILMFWYV")
AROM = set("FWYH")
POLAR = set("STNQCY")
POS = set("KRH")
NEG = set("DE")


def parse_pdb(path: Path) -> list[dict[str, object]]:
    atoms = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.startswith(("ATOM  ", "HETATM")):
            continue
        try:
            atoms.append(
                {
                    "atom": line[12:16].strip(),
                    "resname": line[17:20].strip(),
                    "chain": line[21].strip() or "A",
                    "resid": int(line[22:26]),
                    "x": float(line[30:38]),
                    "y": float(line[38:46]),
                    "z": float(line[46:54]),
                    "bfactor": float(line[60:66]),
                }
            )
        except ValueError:
            continue
    return atoms


def residue_table(atoms: list[dict[str, object]]) -> list[dict[str, object]]:
    residues = []
    for (_, resid), group in itertools.groupby(atoms, key=lambda atom: (atom["chain"], atom["resid"])):
        grouped = list(group)
        ca = next((atom for atom in grouped if atom["atom"] == "CA"), grouped[0])
        aa = AA3.get(str(ca["resname"]), "X")
        residues.append(
            {
                "chain": ca["chain"],
                "resid": resid,
                "resname": ca["resname"],
                "aa": aa,
                "x": float(ca["x"]),
                "y": float(ca["y"]),
                "z": float(ca["z"]),
                "plddt": float(ca["bfactor"]),
            }
        )
    return residues


def distance(left: dict[str, object], right: dict[str, object]) -> float:
    return math.sqrt(
        (float(left["x"]) - float(right["x"])) ** 2
        + (float(left["y"]) - float(right["y"])) ** 2
        + (float(left["z"]) - float(right["z"])) ** 2
    )


def radius_of_gyration(residues: list[dict[str, object]]) -> float:
    if not residues:
        return 0.0
    cx = sum(float(row["x"]) for row in residues) / len(residues)
    cy = sum(float(row["y"]) for row in residues) / len(residues)
    cz = sum(float(row["z"]) for row in residues) / len(residues)
    return math.sqrt(
        sum(
            (float(row["x"]) - cx) ** 2 + (float(row["y"]) - cy) ** 2 + (float(row["z"]) - cz) ** 2
            for row in residues
        )
        / len(residues)
    )


def connected_components(edges: dict[int, set[int]], count: int) -> tuple[int, int]:
    seen = set()
    sizes = []
    for node in range(count):
        if node in seen:
            continue
        stack = [node]
        seen.add(node)
        size = 0
        while stack:
            item = stack.pop()
            size += 1
            for nxt in edges.get(item, set()):
                if nxt not in seen:
                    seen.add(nxt)
                    stack.append(nxt)
        sizes.append(size)
    return len(sizes), max(sizes or [0])


QUALITY_FIELDS = [
    "candidate_id",
    "is_decoy",
    "length_aa",
    "residue_count",
    "atom_count",
    "ca_count",
    "chain_count",
    "mean_plddt",
    "median_plddt",
    "plddt_q25",
    "plddt_q75",
    "low_confidence_fraction_plddt_lt_50",
    "very_low_confidence_fraction_plddt_lt_30",
    "radius_of_gyration_ca_angstrom",
    "max_ca_distance_angstrom",
    "compactness_score",
    "net_charge_approx",
    "hydrophobic_fraction",
    "aromatic_fraction",
    "charged_fraction",
    "cys_count",
    "gly_count",
    "pro_count",
    "low_complexity_flag",
    "disorder_risk_flag",
    "structure_risk_flag",
    "quality_notes",
]

GRAPH_FIELDS = [
    "candidate_id",
    "node_count",
    "edge_count",
    "mean_degree",
    "max_degree",
    "connected_components",
    "largest_component_fraction",
    "ca_radius_cutoff_angstrom",
]

PATCH_FIELDS = [
    "candidate_id",
    "patch_id",
    "center_residue_index",
    "center_residue_name",
    "center_x",
    "center_y",
    "center_z",
    "radius_angstrom",
    "residue_count",
    "net_charge_approx",
    "hydrophobic_count",
    "aromatic_count",
    "polar_count",
    "positive_count",
    "negative_count",
    "gly_pro_count",
    "cys_count",
    "mean_plddt",
    "min_plddt",
    "solvent_proxy_exposed_fraction",
    "patch_sequence_window",
    "patch_residue_indices",
    "patch_descriptor_sha256",
]


def compute_quality_and_patches(
    manifest_tsv: Path,
    out_quality: Path,
    out_graph: Path,
    out_patch: Path,
    out_secondary: Path | None,
    radius: float = 8.0,
    min_patch_residues: int = 3,
) -> dict[str, int]:
    manifest = read_tsv(manifest_tsv)
    quality_rows = []
    graph_rows = []
    patch_rows = []
    secondary_rows = []
    for row in manifest:
        if row.get("prediction_status") != "success":
            continue
        candidate_id = row["candidate_id"]
        atoms = parse_pdb(Path(row["structure_path"]))
        residues = residue_table(atoms)
        plddts = sorted(float(item["plddt"]) for item in residues)
        aas = [str(item["aa"]) for item in residues]
        charge = sum(1 for aa in aas if aa in POS) - sum(1 for aa in aas if aa in NEG)
        rg = radius_of_gyration(residues)
        max_dist = max((distance(a, b) for a, b in itertools.combinations(residues, 2)), default=0.0)
        low_conf = sum(1 for value in plddts if value < 50.0) / max(1, len(plddts))
        very_low = sum(1 for value in plddts if value < 30.0) / max(1, len(plddts))
        mean_plddt = sum(plddts) / max(1, len(plddts))
        risk = []
        if mean_plddt < 50.0:
            risk.append("low_mean_plddt")
        if low_conf > 0.50:
            risk.append("high_low_confidence_fraction")
        if len(set(aas)) <= 4:
            risk.append("low_complexity_structure")
        quality_rows.append(
            {
                "candidate_id": candidate_id,
                "is_decoy": row.get("is_decoy", ""),
                "length_aa": row.get("length_aa", len(residues)),
                "residue_count": len(residues),
                "atom_count": len(atoms),
                "ca_count": len(residues),
                "chain_count": len(set(item["chain"] for item in residues)),
                "mean_plddt": fmt(mean_plddt),
                "median_plddt": fmt(plddts[len(plddts) // 2] if plddts else 0.0),
                "plddt_q25": fmt(plddts[len(plddts) // 4] if plddts else 0.0),
                "plddt_q75": fmt(plddts[(len(plddts) * 3) // 4] if plddts else 0.0),
                "low_confidence_fraction_plddt_lt_50": fmt(low_conf),
                "very_low_confidence_fraction_plddt_lt_30": fmt(very_low),
                "radius_of_gyration_ca_angstrom": fmt(rg),
                "max_ca_distance_angstrom": fmt(max_dist),
                "compactness_score": fmt(len(residues) / max(1.0, rg)),
                "net_charge_approx": charge,
                "hydrophobic_fraction": fmt(sum(1 for aa in aas if aa in HYDRO) / max(1, len(aas))),
                "aromatic_fraction": fmt(sum(1 for aa in aas if aa in AROM) / max(1, len(aas))),
                "charged_fraction": fmt(sum(1 for aa in aas if aa in POS or aa in NEG) / max(1, len(aas))),
                "cys_count": aas.count("C"),
                "gly_count": aas.count("G"),
                "pro_count": aas.count("P"),
                "low_complexity_flag": bool_text("low_complexity_structure" in risk),
                "disorder_risk_flag": bool_text(mean_plddt < 50.0 or low_conf > 0.50),
                "structure_risk_flag": ";".join(risk) if risk else "none",
                "quality_notes": "predicted_structure_prioritization_only",
            }
        )
        edges: dict[int, set[int]] = {index: set() for index in range(len(residues))}
        for i, a in enumerate(residues):
            for j, b in enumerate(residues[i + 1 :], start=i + 1):
                if distance(a, b) <= radius:
                    edges[i].add(j)
                    edges[j].add(i)
        edge_count = sum(len(values) for values in edges.values()) // 2
        comps, largest = connected_components(edges, len(residues))
        degrees = [len(edges[index]) for index in range(len(residues))]
        graph_rows.append(
            {
                "candidate_id": candidate_id,
                "node_count": len(residues),
                "edge_count": edge_count,
                "mean_degree": fmt(sum(degrees) / max(1, len(degrees))),
                "max_degree": max(degrees or [0]),
                "connected_components": comps,
                "largest_component_fraction": fmt(largest / max(1, len(residues))),
                "ca_radius_cutoff_angstrom": fmt(radius),
            }
        )
        for center_index, center in enumerate(residues):
            neighbors = [idx for idx, item in enumerate(residues) if distance(center, item) <= radius]
            if len(neighbors) < min_patch_residues:
                continue
            patch_aas = [aas[idx] for idx in neighbors]
            patch_plddt = [float(residues[idx]["plddt"]) for idx in neighbors]
            descriptor_basis = f"{candidate_id}|{center_index}|{','.join(map(str, neighbors))}|{''.join(patch_aas)}"
            patch_rows.append(
                {
                    "candidate_id": candidate_id,
                    "patch_id": f"{candidate_id}_patch_{center_index + 1:03d}",
                    "center_residue_index": center_index + 1,
                    "center_residue_name": center["resname"],
                    "center_x": fmt(float(center["x"])),
                    "center_y": fmt(float(center["y"])),
                    "center_z": fmt(float(center["z"])),
                    "radius_angstrom": fmt(radius),
                    "residue_count": len(neighbors),
                    "net_charge_approx": sum(1 for aa in patch_aas if aa in POS) - sum(1 for aa in patch_aas if aa in NEG),
                    "hydrophobic_count": sum(1 for aa in patch_aas if aa in HYDRO),
                    "aromatic_count": sum(1 for aa in patch_aas if aa in AROM),
                    "polar_count": sum(1 for aa in patch_aas if aa in POLAR),
                    "positive_count": sum(1 for aa in patch_aas if aa in POS),
                    "negative_count": sum(1 for aa in patch_aas if aa in NEG),
                    "gly_pro_count": sum(1 for aa in patch_aas if aa in {"G", "P"}),
                    "cys_count": patch_aas.count("C"),
                    "mean_plddt": fmt(sum(patch_plddt) / max(1, len(patch_plddt))),
                    "min_plddt": fmt(min(patch_plddt) if patch_plddt else 0.0),
                    "solvent_proxy_exposed_fraction": fmt(1.0 / max(1, len(neighbors))),
                    "patch_sequence_window": "".join(patch_aas),
                    "patch_residue_indices": ",".join(str(idx + 1) for idx in neighbors),
                    "patch_descriptor_sha256": sha256_text(descriptor_basis),
                }
            )
        secondary_rows.append(
            {
                "candidate_id": candidate_id,
                "method": "not_available",
                "helix_fraction": "",
                "strand_fraction": "",
                "coil_fraction": "",
                "turn_fraction": "",
                "unassigned_fraction": "1.000000",
            }
        )
    write_tsv(out_quality, quality_rows, QUALITY_FIELDS)
    write_tsv(out_graph, graph_rows, GRAPH_FIELDS)
    write_tsv(out_patch, patch_rows, PATCH_FIELDS)
    if out_secondary is not None:
        write_tsv(
            out_secondary,
            secondary_rows,
            ["candidate_id", "method", "helix_fraction", "strand_fraction", "coil_fraction", "turn_fraction", "unassigned_fraction"],
        )
    return {"quality_rows": len(quality_rows), "graph_rows": len(graph_rows), "patch_rows": len(patch_rows)}


def semantic_hash_for_pdb(path: Path, candidate_id: str, sequence_sha256: str, backend: str) -> str:
    atoms = parse_pdb(path)
    parts = [candidate_id, sequence_sha256, backend]
    for atom in atoms:
        parts.append(
            f"{atom['chain']}:{atom['resid']}:{atom['atom']}:{float(atom['x']):.3f}:{float(atom['y']):.3f}:{float(atom['z']):.3f}:{float(atom['bfactor']):.2f}"
        )
    return sha256_text("|".join(parts))
