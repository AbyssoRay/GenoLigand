from __future__ import annotations

import csv
import json
import math
import shutil
import subprocess
from pathlib import Path
from typing import Any

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


BOX_SIZE_ANGSTROM = 22.0
VINA_EXHAUSTIVENESS = 8
VINA_NUM_MODES = 5


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        version = run_docking_job(job)
        status = "PASS"
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        version = f"docking worker failed: {exc}"
        status = "FAIL"
        try:
            write_docking_not_ready(runtime_path(str(job["run_dir"])), str(exc))
        except Exception:
            pass
    write_result(args.out, job, status, version)
    return 0 if status == "PASS" else 2


def run_docking_job(job: dict[str, Any]) -> str:
    from rdkit import Chem, rdBase
    from rdkit.Chem import AllChem

    run_dir = runtime_path(str(job["run_dir"]))
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)

    receptor_coordinate = find_receptor_coordinate(run_dir)
    if receptor_coordinate is None:
        write_docking_not_ready(run_dir, "missing receptor coordinate artifact")
        return "docking=not_ready;reason=missing_receptor_coordinate"

    smiles = read_ligand_smiles(run_dir)
    if not smiles:
        write_docking_not_ready(run_dir, "missing ligand smiles from input or known search")
        return "docking=not_ready;reason=missing_ligand_smiles"

    vina = shutil.which("vina")
    obabel = shutil.which("obabel")
    if not vina:
        write_docking_not_ready(run_dir, "AutoDock Vina executable not found in worker PATH")
        return "docking=not_ready;reason=missing_vina"
    if not obabel:
        write_docking_not_ready(run_dir, "Open Babel executable not found in worker PATH")
        return "docking=not_ready;reason=missing_obabel"

    receptor_pdb = tool_outputs / "docking_receptor.pdb"
    receptor_pdbqt = tool_outputs / "docking_receptor.pdbqt"
    ligand_sdf = tool_outputs / "docking_ligand.sdf"
    ligand_pdbqt = tool_outputs / "docking_ligand.pdbqt"
    pose_pdbqt = tool_outputs / "docking_pose.pdbqt"
    pose_sdf = tool_outputs / "docking_pose.sdf"
    log_path = tool_outputs / "docking_vina.log"

    center = write_receptor_pdb(receptor_coordinate, receptor_pdb)
    write_ligand_sdf(smiles, ligand_sdf, Chem, AllChem)
    run_checked([obabel, str(receptor_pdb), "-O", str(receptor_pdbqt), "-xr"], "obabel_receptor")
    run_checked([obabel, str(ligand_sdf), "-O", str(ligand_pdbqt)], "obabel_ligand")
    run_checked(
        [
            vina,
            "--receptor",
            str(receptor_pdbqt),
            "--ligand",
            str(ligand_pdbqt),
            "--center_x",
            fmt(center[0]),
            "--center_y",
            fmt(center[1]),
            "--center_z",
            fmt(center[2]),
            "--size_x",
            fmt(BOX_SIZE_ANGSTROM),
            "--size_y",
            fmt(BOX_SIZE_ANGSTROM),
            "--size_z",
            fmt(BOX_SIZE_ANGSTROM),
            "--exhaustiveness",
            str(VINA_EXHAUSTIVENESS),
            "--num_modes",
            str(VINA_NUM_MODES),
            "--out",
            str(pose_pdbqt),
        ],
        "vina",
        log_path,
    )
    run_checked([obabel, str(pose_pdbqt), "-O", str(pose_sdf)], "obabel_pose")

    affinity = parse_vina_affinity(log_path, pose_pdbqt)
    candidate_id = top_candidate_id(run_dir) or "target_docking"
    write_tsv(
        tool_outputs / "docking_summary.tsv",
        ["candidate_id", "docking_status", "docking_score", "docking_engine", "notes"],
        [
            [
                candidate_id,
                "completed",
                fmt(affinity),
                "vina",
                f"pose=tool_outputs/docking_pose.pdbqt;pose_sdf=tool_outputs/docking_pose.sdf;receptor={receptor_coordinate.name};box_center={fmt(center[0])},{fmt(center[1])},{fmt(center[2])};box_size={BOX_SIZE_ANGSTROM}",
            ]
        ],
    )
    with (tool_outputs / "docking_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "candidate_id": candidate_id,
                "method": "autodock_vina",
                "engine": "vina",
                "engine_path": vina,
                "openbabel_path": obabel,
                "receptor_coordinate": str(receptor_coordinate),
                "receptor_pdbqt": "tool_outputs/docking_receptor.pdbqt",
                "ligand_smiles": smiles,
                "ligand_pdbqt": "tool_outputs/docking_ligand.pdbqt",
                "pose": "tool_outputs/docking_pose.pdbqt",
                "pose_sdf": "tool_outputs/docking_pose.sdf",
                "log": "tool_outputs/docking_vina.log",
                "affinity_kcal_mol": affinity,
                "box_center": center,
                "box_size_angstrom": BOX_SIZE_ANGSTROM,
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")
    return f"AutoDock Vina docking completed; vina={vina}; rdkit={rdBase.rdkitVersion}; affinity={fmt(affinity)}"


def runtime_path(value: str) -> Path:
    path = Path(value)
    if path.exists():
        return path
    if len(value) >= 3 and value[1] == ":" and value[2] in "\\/":
        drive = value[0].lower()
        rest = value[3:].replace("\\", "/")
        return Path(f"/mnt/{drive}/{rest}")
    return path


def find_receptor_coordinate(run_dir: Path) -> Path | None:
    for rel in ["structure_viewer.pdb", "structure_viewer.cif", "structure_viewer.mmcif"]:
        path = run_dir / rel
        if path.exists() and path.stat().st_size > 0:
            return path
    return None


def read_ligand_smiles(run_dir: Path) -> str:
    for value in ligand_smiles_from_known_matches(run_dir):
        if value:
            return value
    path = run_dir / "input_conversion_manifest.tsv"
    if not path.exists():
        return ""
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            if row.get("conversion_status") != "converted":
                continue
            if row.get("canonical_kind") != "small_molecule":
                continue
            value = decode_tsv_cell(row.get("canonical_value", "")).strip()
            if value:
                return value
    return ""


def ligand_smiles_from_known_matches(run_dir: Path) -> list[str]:
    path = run_dir / "known_database_matches.tsv"
    if not path.exists():
        return []
    values: list[str] = []
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            text = f"{row.get('matched_features', '')};{row.get('evidence', '')}"
            for part in text.split(";"):
                key, _, value = part.partition("=")
                if key.strip() == "ligand_smiles" and value.strip():
                    values.append(value.strip())
    return values


def decode_tsv_cell(value: str) -> str:
    out = []
    index = 0
    while index < len(value):
        ch = value[index]
        if ch != "\\" or index + 1 >= len(value):
            out.append(ch)
            index += 1
            continue
        nxt = value[index + 1]
        if nxt == "t":
            out.append("\t")
        elif nxt == "n":
            out.append("\n")
        elif nxt == "r":
            out.append("\r")
        elif nxt == "\\":
            out.append("\\")
        else:
            out.append(nxt)
        index += 2
    return "".join(out)


def write_receptor_pdb(path: Path, out_path: Path) -> list[float]:
    from openmm import app, unit
    from pdbfixer import PDBFixer

    fixer = PDBFixer(filename=str(path))
    fixer.findMissingResidues()
    fixer.findNonstandardResidues()
    fixer.replaceNonstandardResidues()
    fixer.removeHeterogens(keepWater=False)
    fixer.findMissingAtoms()
    fixer.addMissingAtoms()
    fixer.addMissingHydrogens(7.0)
    coords = fixer.positions.value_in_unit(unit.angstrom)
    heavy = [
        coord
        for atom, coord in zip(fixer.topology.atoms(), coords)
        if getattr(getattr(atom, "element", None), "symbol", "").upper() != "H"
    ]
    if not heavy:
        raise ValueError("receptor has no heavy atoms after preparation")
    center = [
        sum(float(coord[index]) for coord in heavy) / len(heavy)
        for index in range(3)
    ]
    with out_path.open("w", encoding="utf-8") as handle:
        app.PDBFile.writeFile(fixer.topology, fixer.positions, handle)
    return center


def write_ligand_sdf(smiles: str, path: Path, Chem: Any, AllChem: Any) -> None:
    mol = Chem.MolFromSmiles(smiles)
    if mol is None:
        raise ValueError("RDKit could not parse ligand smiles")
    mol = Chem.AddHs(mol)
    status = AllChem.EmbedMolecule(mol, randomSeed=20260702)
    if status != 0:
        raise ValueError("RDKit ligand embedding failed")
    try:
        AllChem.MMFFOptimizeMolecule(mol, maxIters=250)
    except Exception:
        AllChem.UFFOptimizeMolecule(mol, maxIters=250)
    writer = Chem.SDWriter(str(path))
    writer.write(mol)
    writer.close()


def run_checked(command: list[str], label: str, log_path: Path | None = None) -> None:
    completed = subprocess.run(command, text=True, capture_output=True)
    if log_path is not None:
        log_path.write_text((completed.stdout or "") + (completed.stderr or ""), encoding="utf-8")
    if completed.returncode != 0:
        detail = first_nonempty_line(completed.stderr) or first_nonempty_line(completed.stdout) or "command failed"
        raise ValueError(f"{label}_failed:{detail}")


def parse_vina_affinity(log_path: Path, pose_path: Path) -> float:
    texts = []
    if log_path.exists():
        texts.append(log_path.read_text(encoding="utf-8", errors="replace"))
    if pose_path.exists():
        texts.append(pose_path.read_text(encoding="utf-8", errors="replace"))
    for text in texts:
        for line in text.splitlines():
            fields = line.split()
            if len(fields) >= 2 and fields[0].isdigit():
                try:
                    return float(fields[1])
                except ValueError:
                    continue
            if line.startswith("REMARK VINA RESULT:"):
                parts = line.split()
                for value in parts:
                    try:
                        return float(value)
                    except ValueError:
                        continue
    raise ValueError("vina affinity not found in output")


def top_candidate_id(run_dir: Path) -> str:
    selected = run_dir / "complex_candidate_selection.tsv"
    if selected.exists():
        with selected.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            first = next(reader, None)
            value = str((first or {}).get("candidate_id", "")).strip()
            if value:
                return value
    path = run_dir / "target_candidate_ranking.tsv"
    if not path.exists():
        return ""
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        first = next(reader, None)
        return str(first.get("candidate_id", "")).strip() if first else ""


def write_docking_not_ready(run_dir: Path, reason: str) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    write_tsv(
        tool_outputs / "docking_summary.tsv",
        ["candidate_id", "docking_status", "docking_score", "docking_engine", "notes"],
        [["", "not_ready", "", "vina", reason]],
    )
    with (tool_outputs / "docking_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump({"method": "autodock_vina", "status": "not_ready", "reason": reason}, handle, indent=2, sort_keys=True)
        handle.write("\n")


def write_tsv(path: Path, header: list[str], rows: list[list[Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(header)
        writer.writerows(rows)


def first_nonempty_line(text: str) -> str:
    for line in str(text or "").splitlines():
        if line.strip():
            return line.strip()
    return ""


def fmt(value: float) -> str:
    if not math.isfinite(float(value)):
        return "nan"
    return f"{float(value):.4f}"


if __name__ == "__main__":
    raise SystemExit(main())
