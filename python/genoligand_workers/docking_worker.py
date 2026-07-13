from __future__ import annotations

import csv
import hashlib
import json
import math
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


BOX_SIZE_ANGSTROM = 22.0
VINA_EXHAUSTIVENESS = 8
VINA_NUM_MODES = 5
WATER_RESIDUES = {"HOH", "WAT", "DOD"}
POLYMER_RESIDUES = {
    "ALA", "ARG", "ASN", "ASP", "CYS", "GLN", "GLU", "GLY", "HIS", "ILE",
    "LEU", "LYS", "MET", "PHE", "PRO", "SER", "THR", "TRP", "TYR", "VAL",
    "A", "C", "G", "U", "I", "DA", "DC", "DG", "DT", "DI", "N", "UNK",
}
BIOLOGICAL_METALS = {
    "CA", "CO", "CU", "FE", "MG", "MN", "MO", "NI", "V", "W", "ZN",
}
COFACTOR_RESIDUES = {
    "ADP", "ATP", "COA", "FAD", "FMN", "HEA", "HEB", "HEC", "HEM", "NAD",
    "NAP", "NDP", "PQQ", "PLP", "SAM", "SAH", "SF4", "F3S", "FES", "MOLY",
}
COMMON_ADDITIVES = {
    "ACT", "BME", "BOG", "CIT", "DMS", "EDO", "EOH", "FMT", "GOL", "MPD",
    "PEG", "PG4", "PO4", "SO4", "TRS",
}


class DockingNotReady(ValueError):
    pass


@dataclass(frozen=True)
class DockingBinding:
    candidate_id: str
    input_id: str
    target_id: str
    pdb_id: str
    ligand_smiles: str
    ligand_record_id: str
    source_content_sha256: str
    receptor_coordinate: Path
    selection: dict[str, str]


@dataclass(frozen=True)
class ReceptorComponent:
    key: str
    residue_name: str
    chain_id: str
    residue_id: str
    center: tuple[float, float, float]
    heavy_atom_count: int
    carbon_atom_count: int
    kind: str
    atom_names: tuple[str, ...] = ()
    elements: tuple[str, ...] = ()


@dataclass(frozen=True)
class SearchBox:
    center: tuple[float, float, float]
    source: str
    source_id: str
    provenance: str
    excluded_component_key: str | None = None


@dataclass(frozen=True)
class ReceptorPreparation:
    box: SearchBox
    receptor_sha256: str
    retained_components: list[dict[str, Any]]


@dataclass(frozen=True)
class SeedProvenance:
    seed: int
    invocation_digest: str
    input_digest: str
    seed_digest: str
    algorithm: str = "sha256_canonical_v1"


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        version = run_docking_job(job)
        prefix = "docking=not_ready;reason="
        if version.startswith(prefix):
            status = "NOT_READY"
            execution_status = "not_ready"
            reason = version[len(prefix):] or "docking_not_ready"
        else:
            status = "PASS"
            execution_status = "completed"
            reason = "completed"
    except Exception as exc:  # pragma: no cover - depends on local scientific runtime
        version = f"docking worker failed: {exc}"
        status = "FAIL"
        execution_status = "failed"
        reason = str(exc) or "docking_worker_failed"
        try:
            write_docking_not_ready(runtime_path(str(job["run_dir"])), str(exc))
        except Exception:
            pass
    write_result(
        args.out,
        job,
        status,
        version,
        execution_status=execution_status,
        reason=reason,
        outcomes=[
            {
                "status": execution_status,
                "reason": reason,
                "required": True,
                "database_id": str(job["worker"]),
                "row_count": 0,
            }
        ],
    )
    return 0 if status == "PASS" else 2


def run_docking_job(job: dict[str, Any]) -> str:
    run_dir = runtime_path(str(job["run_dir"])).resolve(strict=True)
    if not run_dir.is_dir():
        raise DockingNotReady("run_dir_not_directory")
    invocation_id = str(job.get("invocation_id", "")).strip()
    if not invocation_id:
        raise DockingNotReady("docking_invocation_id_missing")
    clear_previous_docking_outputs(run_dir)
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    receptor_pdb = tool_outputs / "docking_receptor.pdb"
    receptor_pdbqt = tool_outputs / "docking_receptor.pdbqt"
    ligand_sdf = tool_outputs / "docking_ligand.sdf"
    ligand_pdbqt = tool_outputs / "docking_ligand.pdbqt"
    pose_pdbqt = tool_outputs / "docking_pose.pdbqt"
    pose_sdf = tool_outputs / "docking_pose.sdf"
    log_path = tool_outputs / "docking_vina.log"

    try:
        binding = resolve_docking_binding(run_dir)
    except DockingNotReady as exc:
        reason = str(exc)
        write_docking_not_ready(run_dir, reason)
        return f"docking=not_ready;reason={reason}"
    try:
        preparation = prepare_receptor_for_docking(
            binding.receptor_coordinate,
            receptor_pdb,
            binding.selection,
        )
    except DockingNotReady as exc:
        reason = str(exc)
        write_docking_not_ready(run_dir, reason, binding=binding)
        return f"docking=not_ready;reason={reason}"

    vina = shutil.which("vina")
    obabel = shutil.which("obabel")
    if not vina:
        write_docking_not_ready(
            run_dir, "missing_vina", binding=binding, preparation=preparation
        )
        return "docking=not_ready;reason=missing_vina"
    if not obabel:
        write_docking_not_ready(
            run_dir, "missing_obabel", binding=binding, preparation=preparation
        )
        return "docking=not_ready;reason=missing_obabel"

    from rdkit import Chem, rdBase
    from rdkit.Chem import AllChem

    seed = derive_docking_seed(
        job,
        binding,
        preparation.receptor_sha256,
        preparation.box,
    )
    write_ligand_sdf(
        binding.ligand_smiles,
        ligand_sdf,
        Chem,
        AllChem,
        seed=seed.seed,
    )
    run_checked([obabel, str(receptor_pdb), "-O", str(receptor_pdbqt), "-xr"], "obabel_receptor")
    run_checked([obabel, str(ligand_sdf), "-O", str(ligand_pdbqt)], "obabel_ligand")
    vina_command = build_vina_command(
        vina,
        receptor_pdbqt,
        ligand_pdbqt,
        pose_pdbqt,
        preparation.box.center,
        seed.seed,
    )
    run_checked(vina_command, "vina", log_path)
    run_checked([obabel, str(pose_pdbqt), "-O", str(pose_sdf)], "obabel_pose")

    affinity = parse_vina_affinity(log_path, pose_pdbqt)
    center_text = ",".join(fmt(value) for value in preparation.box.center)
    retained_ids = ",".join(
        str(item.get("component_id", ""))
        for item in preparation.retained_components
        if item.get("component_id")
    )
    write_tsv(
        tool_outputs / "docking_summary.tsv",
        ["candidate_id", "docking_status", "docking_score", "docking_engine", "notes"],
        [
            [
                binding.candidate_id,
                "completed",
                fmt(affinity),
                "vina",
                f"input_id={binding.input_id};target_id={binding.target_id};pdb_id={binding.pdb_id};"
                f"pose=tool_outputs/docking_pose.pdbqt;pose_sdf=tool_outputs/docking_pose.sdf;"
                f"receptor={binding.receptor_coordinate.name};box_center={center_text};"
                f"box_source={preparation.box.source};box_size={BOX_SIZE_ANGSTROM};"
                f"seed={seed.seed};retained_components={retained_ids}",
            ]
        ],
    )
    with (tool_outputs / "docking_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "status": "completed",
                "job_id": str(job.get("job_id", "")),
                "invocation_id": invocation_id,
                "candidate_id": binding.candidate_id,
                "input_id": binding.input_id,
                "target_id": binding.target_id,
                "pdb_id": binding.pdb_id,
                "method": "autodock_vina",
                "engine": "vina",
                "engine_path": vina,
                "openbabel_path": obabel,
                "receptor_coordinate": str(binding.receptor_coordinate),
                "receptor_sha256": preparation.receptor_sha256,
                "receptor_pdbqt": "tool_outputs/docking_receptor.pdbqt",
                "ligand_smiles": binding.ligand_smiles,
                "ligand_record_id": binding.ligand_record_id,
                "source_content_sha256": binding.source_content_sha256,
                "input_digest": seed.input_digest,
                "ligand_pdbqt": "tool_outputs/docking_ligand.pdbqt",
                "pose": "tool_outputs/docking_pose.pdbqt",
                "pose_sdf": "tool_outputs/docking_pose.sdf",
                "log": "tool_outputs/docking_vina.log",
                "affinity_kcal_mol": affinity,
                "box_center": list(preparation.box.center),
                "box_size_angstrom": BOX_SIZE_ANGSTROM,
                "search_box": {
                    "center": list(preparation.box.center),
                    "size_angstrom": [BOX_SIZE_ANGSTROM] * 3,
                    "source": preparation.box.source,
                    "source_id": preparation.box.source_id,
                    "provenance": preparation.box.provenance,
                },
                "retained_receptor_components": preparation.retained_components,
                "seed": seed.seed,
                "seed_provenance": {
                    "algorithm": seed.algorithm,
                    "invocation_digest": seed.invocation_digest,
                    "input_digest": seed.input_digest,
                    "seed_digest": seed.seed_digest,
                },
                "vina_command": vina_command,
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")
    return (
        f"AutoDock Vina docking completed; vina={vina}; rdkit={rdBase.rdkitVersion}; "
        f"affinity={fmt(affinity)}; seed={seed.seed}"
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


def resolve_run_artifact(run_dir: Path, artifact: str | Path) -> Path:
    root = run_dir.resolve(strict=True)
    value = str(artifact).strip()
    if not value:
        raise DockingNotReady("artifact_path_missing")
    candidate = runtime_path(value)
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        resolved = candidate.resolve(strict=True)
        resolved.relative_to(root)
    except (OSError, ValueError) as exc:
        raise DockingNotReady(f"artifact_outside_current_run:{value}") from exc
    if not resolved.is_file() or resolved.stat().st_size == 0:
        raise DockingNotReady("selected_candidate_receptor_artifact_unavailable")
    return resolved


def clear_previous_docking_outputs(run_dir: Path) -> None:
    root = run_dir.resolve(strict=True)
    for relative in (
        "docking_summary.tsv",
        "tool_outputs/docking_summary.tsv",
        "tool_outputs/docking_manifest.json",
        "tool_outputs/docking_not_ready.tsv",
        "tool_outputs/docking_receptor.pdb",
        "tool_outputs/docking_receptor.pdbqt",
        "tool_outputs/docking_ligand.sdf",
        "tool_outputs/docking_ligand.pdbqt",
        "tool_outputs/docking_pose.pdbqt",
        "tool_outputs/docking_pose.sdf",
        "tool_outputs/docking_vina.log",
    ):
        path = root / relative
        if path.exists():
            if not path.is_file():
                raise DockingNotReady(f"stale_docking_output_not_file:{relative}")
            path.unlink()


def resolve_docking_binding(run_dir: Path) -> DockingBinding:
    selection_path = run_dir / "complex_candidate_selection.tsv"
    if not selection_path.exists():
        raise DockingNotReady("missing_selected_complex_candidate")
    with selection_path.open("r", encoding="utf-8-sig", newline="") as handle:
        selections = [
            {str(key): str(value or "").strip() for key, value in row.items()}
            for row in csv.DictReader(handle, delimiter="\t")
        ]
    if len(selections) != 1:
        raise DockingNotReady("selected_complex_candidate_must_be_unique")
    selection = selections[0]
    candidate_id = selection.get("candidate_id", "")
    input_id = selection.get("input_id", "")
    target_id = selection.get("target_id", "")
    pdb_id = selection.get("pdb_id", "")
    if not candidate_id or not input_id or not target_id:
        raise DockingNotReady("selected_complex_candidate_missing_binding_identity")

    ligand_path = run_dir / "input_conversion_manifest.tsv"
    if not ligand_path.exists():
        raise DockingNotReady("missing_input_conversion_manifest")
    with ligand_path.open("r", encoding="utf-8-sig", newline="") as handle:
        ligand_rows = [
            row
            for row in csv.DictReader(handle, delimiter="\t")
            if str(row.get("input_id", "")).strip() == input_id
            and str(row.get("conversion_status", "")).strip() == "converted"
            and str(row.get("canonical_kind", "")).strip() == "small_molecule"
            and decode_tsv_cell(str(row.get("canonical_value", ""))).strip()
        ]
    if len(ligand_rows) != 1:
        raise DockingNotReady("selected_input_must_bind_one_converted_small_molecule")
    ligand_row = ligand_rows[0]
    ligand_smiles = decode_tsv_cell(str(ligand_row.get("canonical_value", ""))).strip()
    source_digest = str(ligand_row.get("content_sha256", "")).strip().lower()
    if len(source_digest) != 64 or any(ch not in "0123456789abcdef" for ch in source_digest):
        source_digest = hashlib.sha256(ligand_smiles.encode("utf-8")).hexdigest()

    status_path = run_dir / "structure_viewer_status.tsv"
    if not status_path.exists():
        raise DockingNotReady("missing_structure_viewer_status")
    with status_path.open("r", encoding="utf-8-sig", newline="") as handle:
        receptor_rows = []
        for row in csv.DictReader(handle, delimiter="\t"):
            if str(row.get("status", "")).strip() != "ready":
                continue
            if str(row.get("target_id", "")).strip() != target_id:
                continue
            row_pdb_id = str(row.get("pdb_id", "")).strip()
            if pdb_id and row_pdb_id.upper() != pdb_id.upper():
                continue
            receptor_rows.append(row)
    if len(receptor_rows) != 1:
        raise DockingNotReady("selected_candidate_receptor_binding_not_ready")
    artifact = str(receptor_rows[0].get("artifact", "")).strip()
    if not artifact:
        raise DockingNotReady("selected_candidate_receptor_artifact_missing")
    receptor_coordinate = resolve_run_artifact(run_dir, artifact)

    return DockingBinding(
        candidate_id=candidate_id,
        input_id=input_id,
        target_id=target_id,
        pdb_id=pdb_id,
        ligand_smiles=ligand_smiles,
        ligand_record_id=str(ligand_row.get("record_id", "")).strip(),
        source_content_sha256=source_digest,
        receptor_coordinate=receptor_coordinate,
        selection=selection,
    )


def choose_search_box(
    selection: dict[str, str], components: list[ReceptorComponent]
) -> SearchBox:
    center_fields = (
        selection.get("pocket_center_x", "").strip(),
        selection.get("pocket_center_y", "").strip(),
        selection.get("pocket_center_z", "").strip(),
    )
    has_center_value = any(center_fields)
    if has_center_value:
        if not all(center_fields):
            raise DockingNotReady("invalid_selected_pocket_center")
        try:
            center = tuple(float(value) for value in center_fields)
        except ValueError as exc:
            raise DockingNotReady("invalid_selected_pocket_center") from exc
        if not all(math.isfinite(value) for value in center):
            raise DockingNotReady("invalid_selected_pocket_center")
        provenance = next(
            (
                selection.get(field, "").strip()
                for field in ("pocket_source", "pocket_evidence", "pocket_id")
                if selection.get(field, "").strip()
            ),
            "",
        )
        if not provenance:
            raise DockingNotReady("selected_pocket_missing_provenance")
        source_id = selection.get("pocket_id", "").strip() or provenance
        return SearchBox(
            center=(center[0], center[1], center[2]),
            source="selected_pocket",
            source_id=source_id,
            provenance=provenance,
        )

    ligand_codes = selected_ligand_codes(selection)
    if ligand_codes:
        matches = [
            component
            for component in components
            if component.residue_name.upper() in ligand_codes
            and component.kind not in {"water", "polymer", "additive"}
        ]
    else:
        matches = [component for component in components if component.kind == "ligand_candidate"]
    if len(matches) != 1:
        raise DockingNotReady("no_trusted_binding_pocket")
    ligand = matches[0]
    return SearchBox(
        center=ligand.center,
        source="co_crystal_ligand",
        source_id=ligand.key,
        provenance=f"coordinate_residue={ligand.key}",
        excluded_component_key=ligand.key,
    )


def selected_ligand_codes(selection: dict[str, str]) -> set[str]:
    fields = (
        "ligand_code",
        "ligand_id",
        "ligand_ccd_code",
        "ccd_id",
        "co_crystal_ligand",
        "cocrystal_ligand",
    )
    values = [selection.get(field, "") for field in fields]
    for part in selection.get("evidence", "").split(";"):
        key, separator, value = part.partition("=")
        if separator and key.strip().lower() in fields:
            values.append(value)
    codes = set()
    for value in values:
        for code in str(value).replace(",", " ").split():
            if code.strip():
                codes.add(code.strip().upper())
    return codes


def retained_component_provenance(
    components: list[ReceptorComponent], box: SearchBox
) -> list[dict[str, Any]]:
    retained = []
    for component in components:
        if component.key == box.excluded_component_key:
            continue
        if component.kind not in {"metal", "cofactor"}:
            continue
        retained.append(
            {
                "component_id": component.key,
                "residue_name": component.residue_name,
                "chain_id": component.chain_id,
                "residue_id": component.residue_id,
                "classification": component.kind,
                "atom_names": list(component.atom_names),
                "elements": list(component.elements),
                "heavy_atom_count": component.heavy_atom_count,
                "center": list(component.center),
                "retained": True,
                "retention_reason": f"binding_receptor_{component.kind}",
            }
        )
    return retained


def derive_docking_seed(
    job: dict[str, Any],
    binding: DockingBinding,
    receptor_sha256: str,
    box: SearchBox,
) -> SeedProvenance:
    input_digest = canonical_digest(
        {
            "input_id": binding.input_id,
            "record_id": binding.ligand_record_id,
            "canonical_kind": "small_molecule",
            "canonical_value": binding.ligand_smiles,
            "source_content_sha256": binding.source_content_sha256,
        }
    )
    invocation_digest = canonical_digest(
        {
            "schema_version": str(job.get("schema_version", "")),
            "pipeline_mode": str(job.get("pipeline_mode", "")),
            "job_id": str(job.get("job_id", "")),
            "worker": str(job.get("worker", "")),
            "stage": str(job.get("stage", "")),
            "candidate_id": binding.candidate_id,
            "input_id": binding.input_id,
            "target_id": binding.target_id,
            "pdb_id": binding.pdb_id,
            "receptor_sha256": receptor_sha256,
            "box_center": list(box.center),
            "box_source": box.source,
            "box_source_id": box.source_id,
            "box_provenance": box.provenance,
        }
    )
    seed_digest = hashlib.sha256(
        f"{invocation_digest}:{input_digest}".encode("ascii")
    ).hexdigest()
    seed = int(seed_digest[:16], 16) % 2_147_483_647
    return SeedProvenance(
        seed=seed or 1,
        invocation_digest=invocation_digest,
        input_digest=input_digest,
        seed_digest=seed_digest,
    )


def canonical_digest(value: dict[str, Any]) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def build_vina_command(
    vina: str,
    receptor_pdbqt: Path,
    ligand_pdbqt: Path,
    pose_pdbqt: Path,
    center: tuple[float, float, float],
    seed: int,
) -> list[str]:
    return [
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
        "--seed",
        str(seed),
        "--out",
        str(pose_pdbqt),
    ]


def prepare_receptor_for_docking(
    path: Path,
    out_path: Path,
    selection: dict[str, str],
) -> ReceptorPreparation:
    from openmm import app, unit
    from pdbfixer import PDBFixer

    receptor_sha256 = sha256_file(path)
    fixer = PDBFixer(filename=str(path))
    fixer.findMissingResidues()
    fixer.findNonstandardResidues()
    fixer.replaceNonstandardResidues()

    components = receptor_components(fixer.topology, fixer.positions, unit)
    box = choose_search_box(selection, components)
    retained = retained_component_provenance(components, box)
    component_by_key = {component.key: component for component in components}
    to_delete = []
    for residue in fixer.topology.residues():
        key = receptor_component_key(residue)
        component = component_by_key.get(key)
        if component is None:
            continue
        if component.key == box.excluded_component_key:
            to_delete.append(residue)
        elif component.kind not in {"metal", "cofactor"}:
            to_delete.append(residue)
    if to_delete:
        modeller = app.Modeller(fixer.topology, fixer.positions)
        modeller.delete(to_delete)
        fixer.topology = modeller.topology
        fixer.positions = modeller.positions

    fixer.findMissingAtoms()
    fixer.addMissingAtoms()
    fixer.addMissingHydrogens(7.0)
    heavy_atom_count = sum(
        1
        for atom in fixer.topology.atoms()
        if atom_element(atom).upper() not in {"", "H"}
    )
    if heavy_atom_count == 0:
        raise ValueError("receptor_has_no_heavy_atoms_after_preparation")
    final_component_keys = {
        receptor_component_key(residue) for residue in fixer.topology.residues()
    }
    missing_retained = [
        str(item["component_id"])
        for item in retained
        if str(item["component_id"]) not in final_component_keys
    ]
    if missing_retained:
        raise ValueError(
            "retained_receptor_component_lost:" + ",".join(missing_retained)
        )
    with out_path.open("w", encoding="utf-8") as handle:
        app.PDBFile.writeFile(fixer.topology, fixer.positions, handle, keepIds=True)
    return ReceptorPreparation(
        box=box,
        receptor_sha256=receptor_sha256,
        retained_components=retained,
    )


def receptor_components(topology: Any, positions: Any, unit: Any) -> list[ReceptorComponent]:
    coordinates = positions.value_in_unit(unit.angstrom)
    components = []
    for residue in topology.residues():
        residue_name = str(residue.name).strip().upper()
        if residue_name in POLYMER_RESIDUES:
            continue
        atoms = list(residue.atoms())
        heavy_atoms = [
            atom for atom in atoms if atom_element(atom).upper() not in {"", "H"}
        ]
        if not heavy_atoms:
            continue
        elements = tuple(atom_element(atom) for atom in heavy_atoms)
        center = tuple(
            sum(float(coordinates[atom.index][axis]) for atom in heavy_atoms)
            / len(heavy_atoms)
            for axis in range(3)
        )
        carbon_atom_count = sum(element.upper() == "C" for element in elements)
        components.append(
            ReceptorComponent(
                key=receptor_component_key(residue),
                residue_name=residue_name,
                chain_id=str(getattr(getattr(residue, "chain", None), "id", "") or "_"),
                residue_id=str(getattr(residue, "id", "") or getattr(residue, "index", "")),
                center=(center[0], center[1], center[2]),
                heavy_atom_count=len(heavy_atoms),
                carbon_atom_count=carbon_atom_count,
                kind=classify_receptor_component(
                    residue_name,
                    elements,
                    len(heavy_atoms),
                    carbon_atom_count,
                ),
                atom_names=tuple(str(atom.name) for atom in heavy_atoms),
                elements=elements,
            )
        )
    return components


def classify_receptor_component(
    residue_name: str,
    elements: tuple[str, ...],
    heavy_atom_count: int,
    carbon_atom_count: int,
) -> str:
    name = residue_name.upper()
    normalized_elements = {element.upper() for element in elements}
    if name in WATER_RESIDUES:
        return "water"
    if heavy_atom_count == 1 and normalized_elements <= BIOLOGICAL_METALS:
        return "metal"
    if name in COFACTOR_RESIDUES:
        return "cofactor"
    if name in COMMON_ADDITIVES or heavy_atom_count < 6 or carbon_atom_count < 2:
        return "additive"
    return "ligand_candidate"


def receptor_component_key(residue: Any) -> str:
    chain_id = str(getattr(getattr(residue, "chain", None), "id", "") or "_")
    residue_name = str(getattr(residue, "name", "") or "UNK").strip().upper()
    residue_id = str(getattr(residue, "id", "") or getattr(residue, "index", ""))
    return f"{chain_id}:{residue_name}:{residue_id}"


def atom_element(atom: Any) -> str:
    return str(getattr(getattr(atom, "element", None), "symbol", "") or "")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


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


def write_ligand_sdf(
    smiles: str,
    path: Path,
    Chem: Any,
    AllChem: Any,
    *,
    seed: int,
) -> None:
    mol = Chem.MolFromSmiles(smiles)
    if mol is None:
        raise ValueError("RDKit could not parse ligand smiles")
    mol = Chem.AddHs(mol)
    status = AllChem.EmbedMolecule(mol, randomSeed=seed)
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


def write_docking_not_ready(
    run_dir: Path,
    reason: str,
    *,
    binding: DockingBinding | None = None,
    preparation: ReceptorPreparation | None = None,
) -> None:
    tool_outputs = run_dir / "tool_outputs"
    tool_outputs.mkdir(parents=True, exist_ok=True)
    candidate_id = binding.candidate_id if binding else ""
    write_tsv(
        tool_outputs / "docking_summary.tsv",
        ["candidate_id", "docking_status", "docking_score", "docking_engine", "notes"],
        [[candidate_id, "not_ready", "", "vina", reason]],
    )
    manifest: dict[str, Any] = {
        "method": "autodock_vina",
        "status": "not_ready",
        "execution_state": "blocked",
        "reason": reason,
    }
    if binding is not None:
        manifest.update(
            {
                "candidate_id": binding.candidate_id,
                "input_id": binding.input_id,
                "target_id": binding.target_id,
                "pdb_id": binding.pdb_id,
                "ligand_smiles": binding.ligand_smiles,
                "ligand_record_id": binding.ligand_record_id,
                "source_content_sha256": binding.source_content_sha256,
                "receptor_coordinate": str(binding.receptor_coordinate),
            }
        )
    if preparation is not None:
        manifest.update(
            {
                "receptor_sha256": preparation.receptor_sha256,
                "search_box": {
                    "center": list(preparation.box.center),
                    "size_angstrom": [BOX_SIZE_ANGSTROM] * 3,
                    "source": preparation.box.source,
                    "source_id": preparation.box.source_id,
                    "provenance": preparation.box.provenance,
                },
                "retained_receptor_components": preparation.retained_components,
            }
        )
    with (tool_outputs / "docking_manifest.json").open("w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=True)
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
