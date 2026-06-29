from __future__ import annotations

import csv
import json
import math
from pathlib import Path
from typing import Any

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        version = run_md_summary_job(job)
        status = "PASS"
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        version = f"MD summary worker failed: {exc}"
        status = "FAIL"
    write_result(args.out, job, status, version)
    return 0 if status == "PASS" else 2


def run_md_summary_job(job: dict[str, Any]) -> str:
    import numpy as np
    import openmm
    from openmm import Vec3, unit
    from rdkit import Chem
    from rdkit.Chem import AllChem

    input_manifest = runtime_path(str(job["input_manifest"]))
    run_dir = runtime_path(str(job["run_dir"]))
    smiles, input_id = read_small_molecule_input(input_manifest)
    if not smiles:
        raise ValueError("no small_molecule input available for MD summary")

    mol = Chem.MolFromSmiles(smiles)
    if mol is None:
        raise ValueError("small_molecule input is not a valid SMILES string")
    mol = Chem.AddHs(mol)
    embed = AllChem.ETKDGv3()
    embed.randomSeed = 20260629
    if AllChem.EmbedMolecule(mol, embed) != 0:
        raise ValueError("RDKit conformer embedding failed")
    if AllChem.MMFFHasAllMoleculeParams(mol):
        AllChem.MMFFOptimizeMolecule(mol, maxIters=200)
    else:
        AllChem.UFFOptimizeMolecule(mol, maxIters=200)

    conformer = mol.GetConformer()
    initial_angstrom = np.array(
        [
            [
                conformer.GetAtomPosition(index).x,
                conformer.GetAtomPosition(index).y,
                conformer.GetAtomPosition(index).z,
            ]
            for index in range(mol.GetNumAtoms())
        ],
        dtype=float,
    )

    platform_name = select_openmm_platform(openmm)
    system = build_ligand_system(openmm, unit, mol, initial_angstrom)
    integrator = openmm.LangevinMiddleIntegrator(
        300 * unit.kelvin,
        1 / unit.picosecond,
        0.001 * unit.picoseconds,
    )
    integrator.setRandomNumberSeed(20260629)
    platform = openmm.Platform.getPlatformByName(platform_name)
    context = openmm.Context(system, integrator, platform)
    positions_nm = [Vec3(*(coord / 10.0)) for coord in initial_angstrom]
    context.setPositions(unit.Quantity(positions_nm, unit.nanometer))
    context.setVelocitiesToTemperature(300 * unit.kelvin, 20260629)

    frames = [current_positions(context, unit)]
    for _ in range(20):
        integrator.step(20)
        frames.append(current_positions(context, unit))
    del context
    del integrator

    candidate_id = top_candidate_id(run_dir) or input_id
    metrics = summarize_frames(mol, frames)
    write_md_artifacts(
        run_dir,
        candidate_id,
        input_id,
        smiles,
        platform_name,
        metrics,
        frames,
        mol,
    )
    return (
        f"OpenMM {openmm.version.full_version}; platform={platform_name}; "
        f"RDKit {getattr(Chem, 'rdBase', Chem).rdkitVersion}"
    )


def runtime_path(value: str) -> Path:
    path = Path(value)
    if path.exists():
        return path
    if len(value) >= 3 and value[1] == ":" and value[2] in "\\/":
        drive = value[0].lower()
        rest = value[3:].replace("\\", "/")
        return Path(f"/mnt/{drive}/{rest}")
    return path


def read_small_molecule_input(path: Path) -> tuple[str, str]:
    with path.open("r", encoding="utf-8-sig") as handle:
        payload = json.load(handle)
    for index, row in enumerate(payload.get("inputs", []), start=1):
        field = str(row.get("field", "")).lower()
        label = str(row.get("label", "")).lower()
        value = str(row.get("value", "")).strip()
        if value and any(token in f"{field} {label}" for token in ["small_molecule", "smiles", "ligand"]):
            return value, f"input_{index}"
    return "", "input_1"


def top_candidate_id(run_dir: Path) -> str:
    path = run_dir / "target_candidate_ranking.tsv"
    if not path.exists():
        return ""
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            value = str(row.get("candidate_id", "")).strip()
            if value:
                return value
    return ""


def select_openmm_platform(openmm: Any) -> str:
    names = [
        openmm.Platform.getPlatform(index).getName()
        for index in range(openmm.Platform.getNumPlatforms())
    ]
    for preferred in ["CUDA", "CPU", "Reference"]:
        if preferred in names:
            return preferred
    if not names:
        raise ValueError("OpenMM has no available platform")
    return names[0]


def build_ligand_system(openmm: Any, unit: Any, mol: Any, initial_angstrom: Any) -> Any:
    system = openmm.System()
    for atom in mol.GetAtoms():
        mass = atom.GetMass() or 12.0
        system.addParticle(mass * unit.dalton)

    bond_force = openmm.HarmonicBondForce()
    for bond in mol.GetBonds():
        i = bond.GetBeginAtomIdx()
        j = bond.GetEndAtomIdx()
        distance_nm = distance(initial_angstrom[i], initial_angstrom[j]) / 10.0
        bond_force.addBond(
            i,
            j,
            distance_nm * unit.nanometer,
            250000 * unit.kilojoule_per_mole / unit.nanometer**2,
        )
    system.addForce(bond_force)

    restraint = openmm.CustomExternalForce("0.5*k*((x-x0)^2+(y-y0)^2+(z-z0)^2)")
    restraint.addGlobalParameter("k", 25 * unit.kilojoule_per_mole / unit.nanometer**2)
    restraint.addPerParticleParameter("x0")
    restraint.addPerParticleParameter("y0")
    restraint.addPerParticleParameter("z0")
    for index, coord in enumerate(initial_angstrom):
        restraint.addParticle(index, [coord[0] / 10.0, coord[1] / 10.0, coord[2] / 10.0])
    system.addForce(restraint)
    return system


def current_positions(context: Any, unit: Any) -> Any:
    return context.getState(getPositions=True).getPositions(asNumpy=True).value_in_unit(
        unit.angstrom
    )


def summarize_frames(mol: Any, frames: list[Any]) -> dict[str, float]:
    import numpy as np

    heavy = [atom.GetIdx() for atom in mol.GetAtoms() if atom.GetAtomicNum() > 1]
    if not heavy:
        heavy = list(range(mol.GetNumAtoms()))
    heavy_frames = [np.asarray(frame, dtype=float)[heavy, :] for frame in frames]
    reference = heavy_frames[0]
    rmsd_values = [kabsch_rmsd(frame, reference) for frame in heavy_frames[1:]]
    stack = np.stack(heavy_frames, axis=0)
    mean_by_atom = stack.mean(axis=0)
    rmsf_by_atom = np.sqrt(((stack - mean_by_atom) ** 2).sum(axis=2).mean(axis=0))
    centers = np.array([frame.mean(axis=0) for frame in heavy_frames])
    drift_values = np.sqrt(((centers - centers[0]) ** 2).sum(axis=1))[1:]
    contact = contact_occupancy(mol, heavy, heavy_frames)
    hbond = hbond_occupancy(mol, heavy_frames)
    rmsd = float(np.mean(rmsd_values)) if rmsd_values else 0.0
    rmsf = float(np.mean(rmsf_by_atom)) if len(rmsf_by_atom) else 0.0
    drift = float(np.mean(drift_values)) if len(drift_values) else 0.0
    persistence = max(contact, hbond)
    stability = max(0.0, min(1.0, 1.0 - (rmsd / 5.0 + drift / 5.0) / 2.0))
    return {
        "rmsd": rmsd,
        "rmsf": rmsf,
        "ligand_drift": drift,
        "contact_occupancy": contact,
        "hydrogen_bond_occupancy": hbond,
        "interaction_persistence": persistence,
        "md_stability_score": stability,
    }


def kabsch_rmsd(frame: Any, reference: Any) -> float:
    import numpy as np

    left = np.asarray(frame, dtype=float) - np.asarray(frame, dtype=float).mean(axis=0)
    right = np.asarray(reference, dtype=float) - np.asarray(reference, dtype=float).mean(axis=0)
    covariance = left.T @ right
    v, _, wt = np.linalg.svd(covariance)
    sign = np.sign(np.linalg.det(v @ wt))
    correction = np.diag([1.0, 1.0, sign])
    rotation = v @ correction @ wt
    aligned = left @ rotation
    return float(np.sqrt(((aligned - right) ** 2).sum(axis=1).mean()))


def contact_occupancy(mol: Any, heavy: list[int], frames: list[Any]) -> float:
    from rdkit import Chem

    graph = Chem.GetDistanceMatrix(mol)
    pairs: list[tuple[int, int]] = []
    for left_index, left in enumerate(heavy):
        for right in heavy[left_index + 1 :]:
            if graph[left, right] > 2:
                pairs.append((left_index, heavy.index(right)))
    if not pairs:
        return 0.0
    occupied = 0
    total = 0
    for frame in frames:
        for left, right in pairs:
            total += 1
            if distance(frame[left], frame[right]) <= 4.5:
                occupied += 1
    return occupied / total if total else 0.0


def hbond_occupancy(mol: Any, heavy_frames: list[Any]) -> float:
    atoms = list(mol.GetAtoms())
    heavy_atoms = [atom for atom in atoms if atom.GetAtomicNum() > 1]
    donors = [
        index
        for index, atom in enumerate(heavy_atoms)
        if atom.GetAtomicNum() in {7, 8, 16}
        and any(neighbor.GetAtomicNum() == 1 for neighbor in atom.GetNeighbors())
    ]
    acceptors = [
        index for index, atom in enumerate(heavy_atoms) if atom.GetAtomicNum() in {7, 8, 16}
    ]
    pairs = [(donor, acceptor) for donor in donors for acceptor in acceptors if donor != acceptor]
    if not pairs:
        return 0.0
    occupied = 0
    total = 0
    for frame in heavy_frames:
        for donor, acceptor in pairs:
            total += 1
            if distance(frame[donor], frame[acceptor]) <= 3.5:
                occupied += 1
    return occupied / total if total else 0.0


def distance(left: Any, right: Any) -> float:
    return math.sqrt(sum((float(a) - float(b)) ** 2 for a, b in zip(left, right)))


def write_md_artifacts(
    run_dir: Path,
    candidate_id: str,
    input_id: str,
    smiles: str,
    platform_name: str,
    metrics: dict[str, float],
    frames: list[Any],
    mol: Any,
) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    risk_flags = "ligand_only_no_receptor_coordinate"
    score_note = "score_decomposition.tsv records Rust-owned candidate scores"
    summary_header = [
        "candidate_id",
        "rmsd",
        "rmsf",
        "ligand_drift",
        "hydrogen_bond_occupancy",
        "contact_occupancy",
        "interaction_persistence",
        "md_risk_flags",
        "md_score_decomposition",
        "md_stability_score",
    ]
    write_tsv(
        tool_outputs / "md_summary.tsv",
        summary_header,
        [
            [
                candidate_id,
                fmt(metrics["rmsd"]),
                fmt(metrics["rmsf"]),
                fmt(metrics["ligand_drift"]),
                fmt(metrics["hydrogen_bond_occupancy"]),
                fmt(metrics["contact_occupancy"]),
                fmt(metrics["interaction_persistence"]),
                risk_flags,
                score_note,
                fmt(metrics["md_stability_score"]),
            ]
        ],
    )
    metric_specs = [
        ("md_rmsd.tsv", "rmsd"),
        ("md_rmsf.tsv", "rmsf"),
        ("md_ligand_drift.tsv", "ligand_drift"),
        ("md_contacts.tsv", "contact_occupancy"),
        ("md_hbonds.tsv", "hydrogen_bond_occupancy"),
    ]
    for file_name, metric in metric_specs:
        write_tsv(
            tool_outputs / file_name,
            ["candidate_id", "input_id", "metric", "value", "source"],
            [[candidate_id, input_id, metric, fmt(metrics[metric]), "openmm_ligand_dynamics"]],
        )
    write_trajectory(tool_outputs / "md_trajectory.tsv", candidate_id, frames, mol)
    with (tool_outputs / "md_run_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "candidate_id": candidate_id,
                "input_id": input_id,
                "input_smiles": smiles,
                "method": "openmm_ligand_dynamics",
                "platform": platform_name,
                "frame_count": len(frames),
                "risk_flags": [risk_flags],
                "score_decomposition": "score_decomposition.tsv",
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_trajectory(path: Path, candidate_id: str, frames: list[Any], mol: Any) -> None:
    rows = []
    elements = [atom.GetSymbol() for atom in mol.GetAtoms()]
    for frame_index, frame in enumerate(frames):
        for atom_index, coord in enumerate(frame):
            rows.append(
                [
                    candidate_id,
                    str(frame_index),
                    str(atom_index),
                    elements[atom_index],
                    fmt(coord[0]),
                    fmt(coord[1]),
                    fmt(coord[2]),
                ]
            )
    write_tsv(path, ["candidate_id", "frame", "atom_index", "element", "x", "y", "z"], rows)


def write_tsv(path: Path, header: list[str], rows: list[list[str]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(header)
        writer.writerows(rows)


def fmt(value: float) -> str:
    return f"{float(value):.4f}"


if __name__ == "__main__":
    raise SystemExit(main())
