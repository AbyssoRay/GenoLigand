from __future__ import annotations

import csv
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


MAX_RECEPTOR_ATOMS = 900
SIMULATION_SEED = 20260629


@dataclass
class ReceptorCoordinate:
    path: Path
    atoms: list[dict[str, str]]
    positions_angstrom: Any
    preparation: str
    risk_flags: list[str]


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
        write_md_not_ready(run_dir, "no small_molecule input available for MD summary", [])
        return openmm_version_text(openmm, "not_ready", "missing_small_molecule")

    receptor_path = find_receptor_coordinate(run_dir)
    if receptor_path is None:
        write_md_not_ready(run_dir, "missing receptor coordinate artifact for complex MD", ["missing_receptor_coordinate"])
        return openmm_version_text(openmm, "not_ready", "missing_receptor_coordinate")

    mol = prepare_ligand(smiles, Chem, AllChem)
    try:
        receptor = load_receptor_coordinate(receptor_path, unit)
    except Exception as exc:  # noqa: BLE001 - not-ready artifact is the contract for unusable coordinates
        write_md_not_ready(
            run_dir,
            f"receptor coordinate preparation failed: {safe_note(str(exc))}",
            ["receptor_coordinate_preparation_failed"],
        )
        return openmm_version_text(openmm, "not_ready", "receptor_coordinate_preparation_failed")
    if not receptor.atoms:
        write_md_not_ready(run_dir, "receptor coordinate file did not contain usable heavy atoms", ["receptor_coordinate_parse_failed"])
        return openmm_version_text(openmm, "not_ready", "receptor_coordinate_parse_failed")

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
    ligand_positions = place_ligand_near_receptor(initial_angstrom, receptor.positions_angstrom)
    parameterization, parameter_flags = ligand_parameterization_status(mol)
    platform_name = select_openmm_platform(openmm)
    system, role_ranges = build_complex_system(
        openmm,
        unit,
        mol,
        receptor,
        ligand_positions,
    )
    integrator = openmm.LangevinMiddleIntegrator(
        300 * unit.kelvin,
        1 / unit.picosecond,
        0.001 * unit.picoseconds,
    )
    integrator.setRandomNumberSeed(SIMULATION_SEED)
    platform = openmm.Platform.getPlatformByName(platform_name)
    context = openmm.Context(system, integrator, platform)
    combined_positions = np.vstack([receptor.positions_angstrom, ligand_positions])
    positions_nm = [Vec3(*(coord / 10.0)) for coord in combined_positions]
    context.setPositions(unit.Quantity(positions_nm, unit.nanometer))
    context.setVelocitiesToTemperature(300 * unit.kelvin, SIMULATION_SEED)

    frames = [current_positions(context, unit)]
    for _ in range(20):
        integrator.step(20)
        frames.append(current_positions(context, unit))
    del context
    del integrator

    candidate_id = top_candidate_id(run_dir) or input_id
    risk_flags = [
        *receptor.risk_flags,
        *parameter_flags,
        "ligand_placement_recorded_from_rdkit_conformer_centroid",
        "coarse_restrained_complex_model",
    ]
    metrics = summarize_complex_frames(mol, receptor, frames, role_ranges)
    write_md_artifacts(
        run_dir,
        candidate_id,
        input_id,
        smiles,
        platform_name,
        metrics,
        frames,
        mol,
        ligand_positions,
        receptor,
        role_ranges,
        parameterization,
        risk_flags,
    )
    return (
        f"OpenMM {openmm.version.full_version}; platform={platform_name}; "
        f"method=openmm_receptor_ligand_complex_dynamics; receptor={receptor.path.name}"
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


def prepare_ligand(smiles: str, Chem: Any, AllChem: Any) -> Any:
    mol = Chem.MolFromSmiles(smiles)
    if mol is None:
        raise ValueError("small_molecule input is not a valid SMILES string")
    mol = Chem.AddHs(mol)
    embed = AllChem.ETKDGv3()
    embed.randomSeed = SIMULATION_SEED
    if AllChem.EmbedMolecule(mol, embed) != 0:
        raise ValueError("RDKit conformer embedding failed")
    if AllChem.MMFFHasAllMoleculeParams(mol):
        AllChem.MMFFOptimizeMolecule(mol, maxIters=200)
    else:
        AllChem.UFFOptimizeMolecule(mol, maxIters=200)
    return mol


def find_receptor_coordinate(run_dir: Path) -> Path | None:
    for rel in [
        "structure_viewer.pdb",
        "structure_viewer.cif",
        "structure_viewer.mmcif",
        "tool_outputs/structure_viewer.pdb",
        "tool_outputs/structure_viewer.cif",
        "tool_outputs/structure_viewer.mmcif",
    ]:
        path = run_dir / rel
        if path.exists() and path.stat().st_size > 0:
            return path
    return None


def load_receptor_coordinate(path: Path, unit: Any) -> ReceptorCoordinate:
    try:
        return load_receptor_with_pdbfixer(path, unit)
    except Exception as pdbfixer_error:  # noqa: BLE001 - fallback keeps real coordinate provenance
        receptor = load_receptor_with_openmm_app(path, unit)
        receptor.risk_flags.append(f"pdbfixer_failed:{safe_note(str(pdbfixer_error))}")
        return receptor


def load_receptor_with_pdbfixer(path: Path, unit: Any) -> ReceptorCoordinate:
    import numpy as np
    from openmm import app
    from pdbfixer import PDBFixer

    fixer = PDBFixer(filename=str(path))
    fixer.findMissingResidues()
    fixer.findNonstandardResidues()
    fixer.replaceNonstandardResidues()
    fixer.removeHeterogens(keepWater=False)
    fixer.findMissingAtoms()
    fixer.addMissingAtoms()
    fixer.addMissingHydrogens(7.0)
    atoms, positions = atoms_from_topology(fixer.topology, fixer.positions, unit)
    atoms, positions, flags = receptor_heavy_atom_subset(atoms, positions)
    prepared = path.parent / "tool_outputs" / "receptor_prepared.pdb"
    prepared.parent.mkdir(parents=True, exist_ok=True)
    with prepared.open("w", encoding="utf-8") as handle:
        app.PDBFile.writeFile(fixer.topology, fixer.positions, handle)
    return ReceptorCoordinate(
        path=path,
        atoms=atoms,
        positions_angstrom=np.asarray(positions, dtype=float),
        preparation="pdbfixer_prepared_receptor",
        risk_flags=flags,
    )


def load_receptor_with_openmm_app(path: Path, unit: Any) -> ReceptorCoordinate:
    import numpy as np
    from openmm import app

    lower = path.name.lower()
    if lower.endswith(".pdb") or lower.endswith(".ent"):
        structure = app.PDBFile(str(path))
    else:
        structure = app.PDBxFile(str(path))
    atoms, positions = atoms_from_topology(structure.topology, structure.positions, unit)
    atoms, positions, flags = receptor_heavy_atom_subset(atoms, positions)
    return ReceptorCoordinate(
        path=path,
        atoms=atoms,
        positions_angstrom=np.asarray(positions, dtype=float),
        preparation="openmm_coordinate_parse",
        risk_flags=["pdbfixer_not_applied", *flags],
    )


def atoms_from_topology(topology: Any, positions: Any, unit: Any) -> tuple[list[dict[str, str]], list[list[float]]]:
    coords = positions.value_in_unit(unit.angstrom)
    atoms: list[dict[str, str]] = []
    out_positions: list[list[float]] = []
    for index, atom in enumerate(topology.atoms()):
        element = getattr(getattr(atom, "element", None), "symbol", "") or "C"
        residue = getattr(atom.residue, "name", "")
        if residue.upper() in {"HOH", "WAT", "DOD"} or element.upper() == "H":
            continue
        chain = getattr(getattr(atom, "residue", None), "chain", None)
        atoms.append(
            {
                "element": element,
                "name": getattr(atom, "name", f"atom_{index}"),
                "residue": residue,
                "residue_id": str(getattr(atom.residue, "id", "")),
                "chain": str(getattr(chain, "id", "")),
            }
        )
        pos = coords[index]
        out_positions.append([float(pos[0]), float(pos[1]), float(pos[2])])
    return atoms, out_positions


def receptor_heavy_atom_subset(
    atoms: list[dict[str, str]],
    positions: list[list[float]],
) -> tuple[list[dict[str, str]], list[list[float]], list[str]]:
    if len(atoms) <= MAX_RECEPTOR_ATOMS:
        return atoms, positions, []
    import numpy as np

    array = np.asarray(positions, dtype=float)
    centroid = array.mean(axis=0)
    distances = np.sqrt(((array - centroid) ** 2).sum(axis=1))
    selected = sorted(np.argsort(distances)[:MAX_RECEPTOR_ATOMS])
    return (
        [atoms[index] for index in selected],
        [positions[index] for index in selected],
        [f"receptor_atom_subset:{MAX_RECEPTOR_ATOMS}_of_{len(atoms)}"],
    )


def ligand_parameterization_status(mol: Any) -> tuple[str, list[str]]:
    try:
        from openff.toolkit import Molecule

        off_mol = Molecule.from_rdkit(mol, allow_undefined_stereo=True, hydrogens_are_explicit=True)
        if off_mol.n_atoms:
            return "openff_toolkit_molecule_from_rdkit", []
    except Exception as exc:  # noqa: BLE001 - RDKit fallback remains recorded in manifest
        return "rdkit_mmff_or_uff", [f"openff_parameterization_failed:{safe_note(str(exc))}"]
    return "rdkit_mmff_or_uff", ["openff_parameterization_empty"]


def place_ligand_near_receptor(initial_angstrom: Any, receptor_positions: Any) -> Any:
    import numpy as np

    receptor = np.asarray(receptor_positions, dtype=float)
    ligand = np.asarray(initial_angstrom, dtype=float)
    receptor_center = receptor.mean(axis=0)
    ligand_center = ligand.mean(axis=0)
    receptor_radius = float(np.percentile(np.sqrt(((receptor - receptor_center) ** 2).sum(axis=1)), 35))
    offset = np.array([max(2.5, min(8.0, receptor_radius * 0.2)), 0.0, 0.0])
    return ligand - ligand_center + receptor_center + offset


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


def build_complex_system(
    openmm: Any,
    unit: Any,
    mol: Any,
    receptor: ReceptorCoordinate,
    ligand_positions: Any,
) -> tuple[Any, dict[str, range]]:
    import numpy as np

    system = openmm.System()
    receptor_positions = np.asarray(receptor.positions_angstrom, dtype=float)
    for atom in receptor.atoms:
        system.addParticle(element_mass(atom["element"]) * unit.dalton)
    for atom in mol.GetAtoms():
        mass = atom.GetMass() or 12.0
        system.addParticle(mass * unit.dalton)

    receptor_range = range(0, len(receptor.atoms))
    ligand_range = range(len(receptor.atoms), len(receptor.atoms) + mol.GetNumAtoms())

    receptor_restraint = openmm.CustomExternalForce("0.5*k_receptor*((x-x0)^2+(y-y0)^2+(z-z0)^2)")
    receptor_restraint.addGlobalParameter("k_receptor", 150 * unit.kilojoule_per_mole / unit.nanometer**2)
    for name in ["x0", "y0", "z0"]:
        receptor_restraint.addPerParticleParameter(name)
    for index, coord in enumerate(receptor_positions):
        receptor_restraint.addParticle(index, [coord[0] / 10.0, coord[1] / 10.0, coord[2] / 10.0])
    system.addForce(receptor_restraint)

    ligand_bonds = openmm.HarmonicBondForce()
    for bond in mol.GetBonds():
        i = len(receptor.atoms) + bond.GetBeginAtomIdx()
        j = len(receptor.atoms) + bond.GetEndAtomIdx()
        distance_nm = distance(ligand_positions[bond.GetBeginAtomIdx()], ligand_positions[bond.GetEndAtomIdx()]) / 10.0
        ligand_bonds.addBond(
            i,
            j,
            distance_nm * unit.nanometer,
            250000 * unit.kilojoule_per_mole / unit.nanometer**2,
        )
    system.addForce(ligand_bonds)

    ligand_anchor = openmm.CustomExternalForce("0.5*k_ligand*((x-x0)^2+(y-y0)^2+(z-z0)^2)")
    ligand_anchor.addGlobalParameter("k_ligand", 8 * unit.kilojoule_per_mole / unit.nanometer**2)
    for name in ["x0", "y0", "z0"]:
        ligand_anchor.addPerParticleParameter(name)
    for offset, coord in enumerate(ligand_positions):
        ligand_anchor.addParticle(len(receptor.atoms) + offset, [coord[0] / 10.0, coord[1] / 10.0, coord[2] / 10.0])
    system.addForce(ligand_anchor)

    centroid = openmm.CustomCentroidBondForce(2, "0.5*k_centroid*(distance(g1,g2)-r0)^2")
    centroid.addGlobalParameter("k_centroid", 5 * unit.kilojoule_per_mole / unit.nanometer**2)
    centroid.addPerBondParameter("r0")
    centroid.addGroup(list(receptor_range))
    centroid.addGroup(list(ligand_range))
    receptor_center = receptor_positions.mean(axis=0)
    ligand_center = np.asarray(ligand_positions, dtype=float).mean(axis=0)
    centroid.addBond([0, 1], [distance(receptor_center, ligand_center) / 10.0])
    system.addForce(centroid)

    return system, {"receptor": receptor_range, "ligand": ligand_range}


def current_positions(context: Any, unit: Any) -> Any:
    return context.getState(getPositions=True).getPositions(asNumpy=True).value_in_unit(
        unit.angstrom
    )


def summarize_complex_frames(
    mol: Any,
    receptor: ReceptorCoordinate,
    frames: list[Any],
    role_ranges: dict[str, range],
) -> dict[str, float]:
    import numpy as np

    receptor_indices = list(role_ranges["receptor"])
    ligand_indices = list(role_ranges["ligand"])
    ligand_heavy_local = [atom.GetIdx() for atom in mol.GetAtoms() if atom.GetAtomicNum() > 1]
    if not ligand_heavy_local:
        ligand_heavy_local = list(range(mol.GetNumAtoms()))
    ligand_heavy = [ligand_indices[index] for index in ligand_heavy_local]
    receptor_heavy = receptor_indices

    receptor_frames = [np.asarray(frame, dtype=float)[receptor_heavy, :] for frame in frames]
    ligand_frames = [np.asarray(frame, dtype=float)[ligand_heavy, :] for frame in frames]
    receptor_reference = receptor_frames[0]
    rmsd_values = [kabsch_rmsd(frame, receptor_reference) for frame in receptor_frames[1:]]
    stack = np.stack([np.vstack([r_frame, l_frame]) for r_frame, l_frame in zip(receptor_frames, ligand_frames)], axis=0)
    mean_by_atom = stack.mean(axis=0)
    rmsf_by_atom = np.sqrt(((stack - mean_by_atom) ** 2).sum(axis=2).mean(axis=0))
    centers = np.array([frame.mean(axis=0) for frame in ligand_frames])
    drift_values = np.sqrt(((centers - centers[0]) ** 2).sum(axis=1))[1:]
    contact = receptor_ligand_contact_occupancy(receptor_frames, ligand_frames)
    hbond = receptor_ligand_hbond_occupancy(receptor, mol, receptor_frames, ligand_frames, ligand_heavy_local)
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


def receptor_ligand_contact_occupancy(receptor_frames: list[Any], ligand_frames: list[Any]) -> float:
    import numpy as np

    occupied = 0
    total = 0
    for receptor_frame, ligand_frame in zip(receptor_frames, ligand_frames):
        distances = np.sqrt(((receptor_frame[:, None, :] - ligand_frame[None, :, :]) ** 2).sum(axis=2))
        occupied += int((distances <= 4.5).sum())
        total += int(distances.size)
    return occupied / total if total else 0.0


def receptor_ligand_hbond_occupancy(
    receptor: ReceptorCoordinate,
    mol: Any,
    receptor_frames: list[Any],
    ligand_frames: list[Any],
    ligand_heavy_local: list[int],
) -> float:
    import numpy as np

    receptor_polar = [
        index for index, atom in enumerate(receptor.atoms) if atom["element"].upper() in {"N", "O", "S"}
    ]
    ligand_polar = [
        out_index
        for out_index, atom_index in enumerate(ligand_heavy_local)
        if mol.GetAtomWithIdx(atom_index).GetAtomicNum() in {7, 8, 16}
    ]
    if not receptor_polar or not ligand_polar:
        return 0.0
    occupied = 0
    total = 0
    for receptor_frame, ligand_frame in zip(receptor_frames, ligand_frames):
        distances = np.sqrt(
            ((receptor_frame[receptor_polar, None, :] - ligand_frame[None, ligand_polar, :]) ** 2).sum(axis=2)
        )
        occupied += int((distances <= 3.5).sum())
        total += int(distances.size)
    return occupied / total if total else 0.0


def distance(left: Any, right: Any) -> float:
    return math.sqrt(sum((float(a) - float(b)) ** 2 for a, b in zip(left, right)))


def element_mass(symbol: str) -> float:
    return {
        "H": 1.008,
        "C": 12.011,
        "N": 14.007,
        "O": 15.999,
        "P": 30.974,
        "S": 32.06,
        "FE": 55.845,
        "ZN": 65.38,
        "MG": 24.305,
        "CA": 40.078,
    }.get(symbol.strip().upper(), 12.0)


def write_md_not_ready(run_dir: Path, reason: str, risk_flags: list[str]) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    write_tsv(
        tool_outputs / "md_not_ready.tsv",
        ["reason", "risk_flags"],
        [[reason, ";".join(risk_flags)]],
    )
    with (tool_outputs / "md_run_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "method": "not_ready",
                "reason": reason,
                "risk_flags": risk_flags,
                "metric_artifacts": [],
                "ligand_coordinate": "",
                "ligand_placement_manifest": "",
                "complex_manifest": "",
                "trajectory": "",
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_md_artifacts(
    run_dir: Path,
    candidate_id: str,
    input_id: str,
    smiles: str,
    platform_name: str,
    metrics: dict[str, float],
    frames: list[Any],
    mol: Any,
    ligand_positions: Any,
    receptor: ReceptorCoordinate,
    role_ranges: dict[str, range],
    parameterization: str,
    risk_flags: list[str],
) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    score_note = "score_decomposition.tsv records Rust-owned candidate scores"
    risk_text = ";".join(risk_flags)
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
                risk_text,
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
            [[candidate_id, input_id, metric, fmt(metrics[metric]), "openmm_receptor_ligand_complex_dynamics"]],
        )
    metric_artifacts = [
        "tool_outputs/md_summary.tsv",
        "tool_outputs/md_rmsd.tsv",
        "tool_outputs/md_rmsf.tsv",
        "tool_outputs/md_ligand_drift.tsv",
        "tool_outputs/md_contacts.tsv",
        "tool_outputs/md_hbonds.tsv",
    ]
    write_ligand_placement(
        tool_outputs / "ligand_placement.tsv",
        candidate_id,
        input_id,
        mol,
        ligand_positions,
    )
    write_trajectory(tool_outputs / "md_trajectory.tsv", candidate_id, frames, mol, receptor, role_ranges)
    write_complex_md_manifest(
        tool_outputs / "complex_md_manifest.json",
        candidate_id,
        input_id,
        platform_name,
        receptor,
        parameterization,
        risk_flags,
        metric_artifacts,
    )
    with (tool_outputs / "md_run_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "candidate_id": candidate_id,
                "input_id": input_id,
                "input_smiles": smiles,
                "method": "openmm_receptor_ligand_complex_dynamics",
                "platform": platform_name,
                "cuda_ready": platform_name == "CUDA",
                "receptor_coordinate": str(receptor.path),
                "receptor_preparation": receptor.preparation,
                "receptor_atom_count": len(receptor.atoms),
                "ligand_parameterization": parameterization,
                "ligand_coordinate": "tool_outputs/ligand_placement.tsv",
                "ligand_coordinate_format": "tsv_angstrom",
                "ligand_atom_count": mol.GetNumAtoms(),
                "ligand_placement": "rdkit_conformer_translated_near_receptor_centroid",
                "ligand_placement_manifest": "tool_outputs/ligand_placement.tsv",
                "complex_manifest": "tool_outputs/complex_md_manifest.json",
                "frame_count": len(frames),
                "trajectory": "tool_outputs/md_trajectory.tsv",
                "metric_artifacts": metric_artifacts,
                "risk_flags": risk_flags,
                "score_decomposition": "score_decomposition.tsv",
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_ligand_placement(
    path: Path,
    candidate_id: str,
    input_id: str,
    mol: Any,
    ligand_positions: Any,
) -> None:
    rows = []
    for atom_index, atom in enumerate(mol.GetAtoms()):
        coord = ligand_positions[atom_index]
        rows.append(
            [
                candidate_id,
                input_id,
                str(atom_index),
                atom.GetSymbol(),
                fmt(coord[0]),
                fmt(coord[1]),
                fmt(coord[2]),
                "angstrom",
                "rdkit_conformer_translated_near_receptor_centroid",
            ]
        )
    write_tsv(
        path,
        [
            "candidate_id",
            "input_id",
            "atom_index",
            "element",
            "x",
            "y",
            "z",
            "unit",
            "placement_method",
        ],
        rows,
    )


def write_complex_md_manifest(
    path: Path,
    candidate_id: str,
    input_id: str,
    platform_name: str,
    receptor: ReceptorCoordinate,
    parameterization: str,
    risk_flags: list[str],
    metric_artifacts: list[str],
) -> None:
    with path.open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "candidate_id": candidate_id,
                "input_id": input_id,
                "method": "openmm_receptor_ligand_complex_dynamics",
                "platform": platform_name,
                "cuda_ready": platform_name == "CUDA",
                "receptor_coordinate": str(receptor.path),
                "receptor_preparation": receptor.preparation,
                "ligand_coordinate": "tool_outputs/ligand_placement.tsv",
                "ligand_parameterization": parameterization,
                "ligand_placement": "rdkit_conformer_translated_near_receptor_centroid",
                "trajectory": "tool_outputs/md_trajectory.tsv",
                "metric_artifacts": metric_artifacts,
                "risk_flags": risk_flags,
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")


def write_trajectory(
    path: Path,
    candidate_id: str,
    frames: list[Any],
    mol: Any,
    receptor: ReceptorCoordinate,
    role_ranges: dict[str, range],
) -> None:
    rows = []
    ligand_elements = [atom.GetSymbol() for atom in mol.GetAtoms()]
    receptor_indices = list(role_ranges["receptor"])
    ligand_indices = list(role_ranges["ligand"])
    for frame_index, frame in enumerate(frames):
        for local_index, global_index in enumerate(receptor_indices):
            coord = frame[global_index]
            atom = receptor.atoms[local_index]
            rows.append(
                [
                    candidate_id,
                    str(frame_index),
                    str(global_index),
                    "receptor",
                    atom["element"],
                    atom["residue"],
                    fmt(coord[0]),
                    fmt(coord[1]),
                    fmt(coord[2]),
                ]
            )
        for local_index, global_index in enumerate(ligand_indices):
            coord = frame[global_index]
            rows.append(
                [
                    candidate_id,
                    str(frame_index),
                    str(global_index),
                    "ligand",
                    ligand_elements[local_index],
                    "LIG",
                    fmt(coord[0]),
                    fmt(coord[1]),
                    fmt(coord[2]),
                ]
            )
    write_tsv(path, ["candidate_id", "frame", "atom_index", "role", "element", "residue", "x", "y", "z"], rows)


def write_tsv(path: Path, header: list[str], rows: list[list[str]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(header)
        writer.writerows(rows)


def fmt(value: float) -> str:
    return f"{float(value):.4f}"


def safe_note(value: str) -> str:
    return value.replace("\t", " ").replace("\n", " ")[:160]


def openmm_version_text(openmm: Any, state: str, reason: str) -> str:
    return f"OpenMM {openmm.version.full_version}; md_summary={state}; reason={reason}"


if __name__ == "__main__":
    raise SystemExit(main())
