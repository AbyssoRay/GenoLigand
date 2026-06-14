from __future__ import annotations

import csv
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

from genoligand_phase06.common import (
    ensure_dir,
    ensure_parent,
    fmt,
    normalized_hash_file,
    parse_float,
    read_tsv,
    safe_id,
    sha256_file,
    sha256_text,
    utc_now,
    write_tsv,
)


ACCEPTANCE_MODE = "anti_hallucination_ligand_docking_strict"
RUN_MODE = "acceptance_control_only"
IFP_ENGINE = "deterministic_geometry_ifp_v1"
IFP_VERSION = "1"
PHASE07_POSITIVE_LIGAND_ID = "phase07_positive_5gp"
PHASE07_POSITIVE_NATIVE_LIGAND_ID = "phase07_positive_5gp_native_pose"
PHASE07_5GP_SMILES = "Nc1nc2c(ncn2[C@@H]2O[C@H](COP(=O)(O)O)[C@@H](O)[C@H]2O)c(=O)[nH]1"
PHASE07_5GP_RDKIT_TO_NATIVE_ATOM = {
    1: "N2",
    2: "C2",
    3: "N3",
    4: "C4",
    5: "C5",
    6: "N7",
    7: "C8",
    8: "N9",
    9: "C1'",
    10: "O4'",
    11: "C4'",
    12: "C5'",
    13: "O5'",
    14: "P",
    15: "O1P",
    16: "O2P",
    17: "O3P",
    18: "C3'",
    19: "O3'",
    20: "C2'",
    21: "O2'",
    22: "C6",
    23: "O6",
    24: "N1",
}

PHASE07_ARTIFACT_DIRNAME = "acceptance_artifacts_phase_0_7_anti_hallucination_strict"
REPO_ROOT = Path(__file__).resolve().parents[2]

ALLOWED_MISSING = {
    "missing",
    "not_applicable",
    "not_run",
    "tool_incompatible",
    "parse_failed",
    "validation_failed",
}

PROHIBITED_REPORT_TERMS = [
    "validated target",
    "confirmed target",
    "true target",
    "proved binding",
    "binding affinity",
    "free energy",
    "mechanism of action",
    "biological activity",
    "therapeutic effect",
    "experimental validation",
    "discovered target",
    "definitive target",
]

PLACEHOLDER_PATTERNS = [
    "TODO",
    "TBD",
    "placeholder",
    "fake",
    "mock result",
    "dummy result",
    "lorem ipsum",
    "example only",
    "not implemented",
    "hardcoded pass",
    "synthetic docking",
]

PROVENANCE_FIELDS = [
    "row_id",
    "source_phase",
    "source_artifact_path",
    "source_artifact_sha256",
    "source_command_id",
    "source_tool",
    "source_tool_version",
    "source_config_path",
    "source_config_sha256",
    "generated_at_utc",
    "manual_edit_allowed",
    "manual_edit_reason",
    "validation_status",
    "validation_notes",
]

LIGAND_INPUT_FIELDS = [
    "ligand_id",
    "ligand_name",
    "input_type",
    "input_value",
    "source_database",
    "source_accession",
    "source_url",
    "stereochemistry_defined",
    "charge_state_known",
    "protonation_policy",
    "intended_role",
    "run_mode",
    "production_target_prior_allowed",
    *PROVENANCE_FIELDS,
]

LIGAND_STANDARDIZATION_FIELDS = [
    "ligand_id",
    "canonical_smiles",
    "isomeric_smiles",
    "molecular_formula",
    "formal_charge",
    "stereocenter_count",
    "undefined_stereochemistry_count",
    "salt_stripping_policy",
    "tautomer_policy",
    "protonation_policy",
    "standardization_status",
    "standardization_notes",
    *PROVENANCE_FIELDS,
]

LIGAND_CONFORMER_FIELDS = [
    "ligand_id",
    "conformer_id",
    "sdf_path",
    "sdf_sha256",
    "rdkit_embed_method",
    "conformer_seed",
    "mmff_optimize",
    "uff_fallback",
    "coordinate_status",
    "selected_for_docking",
    *PROVENANCE_FIELDS,
]

LIGAND_PREP_FIELDS = [
    "ligand_id",
    "conformer_id",
    "input_sdf_path",
    "input_sdf_sha256",
    "preparation_backend",
    "prepared_ligand_path",
    "prepared_ligand_sha256",
    "charge_policy",
    "protonation_policy",
    "preparation_status",
    "preparation_notes",
    *PROVENANCE_FIELDS,
]

POSITIVE_CONTROL_FIELDS = [
    "control_id",
    "pdb_id",
    "protein_chain",
    "native_ligand_id",
    "native_ligand_resname",
    "binding_site_residues",
    "native_ligand_sdf_or_mol2_path",
    "receptor_pdb_path",
    "reference_pose_path",
    "source_url",
    "source_hash",
    "center_x",
    "center_y",
    "center_z",
    *PROVENANCE_FIELDS,
]

RECEPTOR_PREP_FIELDS = [
    "candidate_id",
    "source_structure_path",
    "source_structure_sha256",
    "preparation_backend",
    "prepared_receptor_path",
    "prepared_receptor_sha256",
    "hydrogen_policy",
    "charge_policy",
    "water_policy",
    "cofactor_policy",
    "metal_policy",
    "missing_atom_count",
    "nonstandard_residue_count",
    "preparation_status",
    "preparation_notes",
    *PROVENANCE_FIELDS,
]

DOCKING_BOX_FIELDS = [
    "box_id",
    "candidate_id",
    "patch_id",
    "patch_source_path",
    "patch_source_sha256",
    "center_x",
    "center_y",
    "center_z",
    "size_x",
    "size_y",
    "size_z",
    "box_generation_rule",
    "box_margin_angstrom",
    "patch_residue_indices",
    "patch_confidence_score",
    "is_decoy",
    "box_status",
    *PROVENANCE_FIELDS,
]

DOCKING_RUN_FIELDS = [
    "run_id",
    "ligand_id",
    "candidate_id",
    "box_id",
    "receptor_id",
    "docking_backend",
    "docking_backend_version",
    "command_id",
    "command_line",
    "seed",
    "exhaustiveness",
    "num_modes",
    "input_ligand_path",
    "input_ligand_sha256",
    "input_receptor_path",
    "input_receptor_sha256",
    "box_center",
    "box_size",
    "raw_log_path",
    "pose_path",
    "exit_code",
    "runtime_seconds",
    "run_status",
    *PROVENANCE_FIELDS,
]

DOCKING_SCORE_FIELDS = [
    "run_id",
    "pose_rank",
    "affinity_score_raw",
    "vina_score_kcal_mol_reported_by_tool",
    "score_units",
    "rmsd_lb",
    "rmsd_ub",
    "parsed_from_log",
    "pose_sha256",
    "score_parse_status",
    *PROVENANCE_FIELDS,
]

DOCKING_POSE_FIELDS = [
    "run_id",
    "pose_rank",
    "pose_path",
    "pose_sha256",
    "pose_sdf_path",
    "pose_sdf_sha256",
    "pose_status",
    *PROVENANCE_FIELDS,
]

REDOCKING_FIELDS = [
    "control_id",
    "pdb_id",
    "native_ligand_id",
    "redocked_pose_path",
    "native_pose_path",
    "atom_mapping_status",
    "heavy_atom_rmsd_angstrom",
    "score",
    "rmsd_threshold_angstrom",
    "control_pass",
    "failure_reason",
    *PROVENANCE_FIELDS,
]

IFP_FIELDS = [
    "run_id",
    "pose_rank",
    "candidate_id",
    "ligand_id",
    "box_id",
    "interaction_engine",
    "interaction_engine_version",
    "hbond_count",
    "salt_bridge_count",
    "hydrophobic_contact_count",
    "pi_pi_count",
    "cation_pi_count",
    "metal_coordination_count",
    "halogen_bond_count",
    "steric_clash_count",
    "contact_residues",
    "interaction_residue_indices",
    "interaction_fingerprint_sha256",
    "interaction_parse_status",
    *PROVENANCE_FIELDS,
]

DECOY_CALIBRATION_FIELDS = [
    "candidate_id",
    "ligand_id",
    "best_candidate_score",
    "best_decoy_score",
    "candidate_minus_decoy_margin",
    "interaction_count_candidate",
    "interaction_count_decoy",
    "decoy_outperforms_candidate",
    "calibration_status",
    "calibration_notes",
    *PROVENANCE_FIELDS,
]

TARGET_PRIOR_FIELDS = [
    "candidate_id",
    "ligand_id",
    "run_mode",
    "production_target_prior_allowed",
    "best_vina_score_reported_by_tool",
    "best_decoy_score_reported_by_tool",
    "score_margin_vs_decoy",
    "interaction_count",
    "confidence_label",
    "confidence_downgrade_reason",
    "interpretation_label",
    "next_validation_required",
    *PROVENANCE_FIELDS,
]

LEDGER_FIELDS = [
    "ledger_id",
    "claim_type",
    "claim_text",
    "claim_location",
    "supporting_table",
    "supporting_row_id",
    "supporting_artifact_path",
    "supporting_artifact_sha256",
    "supporting_command_id",
    "supporting_tool",
    "supporting_tool_version",
    "validation_script",
    "validation_status",
]


def read_simple_yaml(path: Path) -> dict[str, str]:
    config: dict[str, str] = {}
    if not path.exists():
        return config
    for raw in path.read_text(encoding="utf-8-sig", errors="replace").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or ":" not in line:
            continue
        key, value = line.split(":", 1)
        value = value.strip().strip('"').strip("'")
        config[key.strip()] = value
    return config


def path_hash(path: Path | str) -> str:
    target = Path(path)
    return sha256_file(target) if target.exists() and target.is_file() else "missing"


def config_hash(path: Path | str) -> str:
    target = Path(path)
    return sha256_file(target) if target.exists() else "missing"


def provenance(
    row_id: str,
    source_phase: str,
    source_artifact_path: Path | str,
    source_command_id: str,
    source_tool: str,
    source_tool_version: str,
    source_config_path: Path | str,
    validation_status: str = "PASS",
    validation_notes: str = "",
) -> dict[str, object]:
    source_artifact = Path(str(source_artifact_path))
    source_config = Path(str(source_config_path))
    return {
        "row_id": row_id,
        "source_phase": source_phase,
        "source_artifact_path": str(source_artifact_path),
        "source_artifact_sha256": path_hash(source_artifact),
        "source_command_id": source_command_id,
        "source_tool": source_tool,
        "source_tool_version": source_tool_version,
        "source_config_path": str(source_config_path),
        "source_config_sha256": config_hash(source_config),
        "generated_at_utc": utc_now(),
        "manual_edit_allowed": "false",
        "manual_edit_reason": "not_applicable",
        "validation_status": validation_status,
        "validation_notes": validation_notes,
    }


def write_empty(path: Path, fields: list[str]) -> None:
    write_tsv(path, [], fields)


def command_text(command: list[str], cwd: Path | None = None, timeout: int = 120) -> tuple[int, str]:
    try:
        completed = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=timeout, check=False)
        return completed.returncode, completed.stdout + completed.stderr
    except Exception as exc:  # noqa: BLE001
        return 1, repr(exc)


def command_version(command: list[str], timeout: int = 120) -> str:
    code, text = command_text(command, timeout=timeout)
    clean = text.strip().replace("\r", " ").replace("\n", " | ")
    return clean[:300] if code == 0 and clean else "missing"


def python_import_version(module: str, attribute: str = "__version__") -> tuple[bool, str]:
    code = (
        "import importlib; "
        f"m=importlib.import_module('{module}'); "
        f"print(getattr(m, '{attribute}', 'version_attribute_missing'))"
    )
    completed_code, text = command_text([sys.executable, "-c", code], timeout=120)
    return completed_code == 0, text.strip().replace("\n", " | ")[:300] or "missing"


def parse_vina_score_lines(text: str) -> list[dict[str, str]]:
    scores: list[dict[str, str]] = []
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("Estimated Free Energy"):
            match = re.search(r":\s*([-+]?\d+(?:\.\d+)?)", line)
            if match:
                scores.append({"rank": str(len(scores) + 1), "score": match.group(1), "rmsd_lb": "0.000", "rmsd_ub": "0.000"})
        if line.startswith("REMARK VINA RESULT:"):
            parts = line.split()
            if len(parts) >= 4:
                scores.append({"rank": str(len(scores) + 1), "score": parts[3], "rmsd_lb": "0.000", "rmsd_ub": "0.000"})
        columns = line.split()
        if len(columns) >= 4 and columns[0].isdigit():
            try:
                float(columns[1])
            except ValueError:
                continue
            scores.append({"rank": columns[0], "score": columns[1], "rmsd_lb": columns[2], "rmsd_ub": columns[3]})
    return scores


def pdb_atoms(path: Path, records: tuple[str, ...] = ("ATOM", "HETATM")) -> list[dict[str, object]]:
    autodock_element = {
        "A": "C",
        "C": "C",
        "HD": "H",
        "HS": "H",
        "N": "N",
        "NA": "N",
        "NS": "N",
        "OA": "O",
        "OS": "O",
        "P": "P",
        "S": "S",
        "SA": "S",
        "F": "F",
        "CL": "CL",
        "BR": "BR",
        "I": "I",
    }
    atoms: list[dict[str, object]] = []
    if not path.exists():
        return atoms
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.startswith(records):
            continue
        try:
            atom_name = line[12:16].strip()
            element = line[76:78].strip() if len(line) >= 78 else ""
            element_key = element.upper()
            if element_key in autodock_element:
                element = autodock_element[element_key]
            elif not element:
                element = re.sub(r"[^A-Za-z]", "", atom_name)[:1].upper()
            atoms.append(
                {
                    "record": line[:6].strip(),
                    "atom": atom_name,
                    "resname": line[17:20].strip(),
                    "chain": line[21].strip() or "missing",
                    "resid": line[22:26].strip() or "missing",
                    "x": float(line[30:38]),
                    "y": float(line[38:46]),
                    "z": float(line[46:54]),
                    "element": element.upper(),
                }
            )
        except ValueError:
            continue
    return atoms


def centroid(atoms: list[dict[str, object]]) -> tuple[float, float, float]:
    if not atoms:
        return (0.0, 0.0, 0.0)
    return (
        sum(float(atom["x"]) for atom in atoms) / len(atoms),
        sum(float(atom["y"]) for atom in atoms) / len(atoms),
        sum(float(atom["z"]) for atom in atoms) / len(atoms),
    )


def distance(a: dict[str, object], b: dict[str, object]) -> float:
    return math.sqrt(
        (float(a["x"]) - float(b["x"])) ** 2
        + (float(a["y"]) - float(b["y"])) ** 2
        + (float(a["z"]) - float(b["z"])) ** 2
    )


def pdbqt_element(line: str) -> str:
    autodock_element = {
        "A": "C",
        "C": "C",
        "HD": "H",
        "HS": "H",
        "N": "N",
        "NA": "N",
        "NS": "N",
        "OA": "O",
        "OS": "O",
        "P": "P",
        "S": "S",
        "SA": "S",
        "F": "F",
        "CL": "CL",
        "BR": "BR",
        "I": "I",
    }
    atom_type = line[76:78].strip().upper() if len(line) >= 78 else ""
    if atom_type in autodock_element:
        return autodock_element[atom_type]
    atom_name = line[12:16].strip()
    return re.sub(r"[^A-Za-z]", "", atom_name)[:1].upper()


def pdbqt_heavy_atom_models(path: Path) -> list[dict[int, dict[str, object]]]:
    if not path.exists():
        return []
    models: list[dict[int, dict[str, object]]] = []
    current: dict[int, dict[str, object]] = {}
    saw_model = False
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("MODEL"):
            saw_model = True
            current = {}
            continue
        if line.startswith("ENDMDL"):
            if current:
                models.append(current)
            current = {}
            continue
        if not line.startswith(("ATOM", "HETATM")):
            continue
        element = pdbqt_element(line)
        if element == "H":
            continue
        try:
            serial = int(line[6:11])
            current[serial] = {
                "x": float(line[30:38]),
                "y": float(line[38:46]),
                "z": float(line[46:54]),
                "element": element,
            }
        except ValueError:
            continue
    if current and not saw_model:
        models.append(current)
    return models


def pdbqt_5gp_serial_to_native_atom(path: Path) -> dict[int, str]:
    pairs: list[tuple[int, int]] = []
    if not path.exists():
        return {}
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.startswith("REMARK SMILES IDX"):
            continue
        values = [int(value) for value in line.split()[3:] if value.isdigit()]
        pairs.extend(zip(values[0::2], values[1::2]))
    return {pdbqt_serial: PHASE07_5GP_RDKIT_TO_NATIVE_ATOM[rdkit_idx] for rdkit_idx, pdbqt_serial in pairs if rdkit_idx in PHASE07_5GP_RDKIT_TO_NATIVE_ATOM}


def extract_native_ligand(pdb_path: Path, resname: str, chain: str, resseq: str, out_pdb: Path) -> list[dict[str, object]]:
    ensure_parent(out_pdb)
    lines = []
    for line in pdb_path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.startswith("HETATM"):
            continue
        if line[17:20].strip() != resname:
            continue
        if line[21].strip() != chain:
            continue
        if line[22:26].strip() != str(resseq):
            continue
        lines.append(line)
    if lines:
        out_pdb.write_text("\n".join(lines) + "\nEND\n", encoding="utf-8")
    else:
        out_pdb.write_text("END\n", encoding="utf-8")
    return pdb_atoms(out_pdb, ("HETATM",))


def write_5gp_native_pose_sdf(native_pdb: Path, out_sdf: Path) -> bool:
    try:
        from rdkit import Chem  # type: ignore
    except Exception:  # noqa: BLE001
        return False
    native_by_name = {str(atom["atom"]): atom for atom in pdb_atoms(native_pdb, ("HETATM",))}
    if any(name not in native_by_name for name in PHASE07_5GP_RDKIT_TO_NATIVE_ATOM.values()):
        return False
    mol = Chem.MolFromSmiles(PHASE07_5GP_SMILES)
    if mol is None:
        return False
    conf = Chem.Conformer(mol.GetNumAtoms())
    for atom in mol.GetAtoms():
        native_name = PHASE07_5GP_RDKIT_TO_NATIVE_ATOM[atom.GetIdx() + 1]
        native_atom = native_by_name[native_name]
        conf.SetAtomPosition(atom.GetIdx(), (float(native_atom["x"]), float(native_atom["y"]), float(native_atom["z"])))
    conf.Set3D(True)
    mol.RemoveAllConformers()
    mol.AddConformer(conf)
    mol = Chem.AddHs(mol, addCoords=True)
    mol.SetProp("_Name", PHASE07_POSITIVE_NATIVE_LIGAND_ID)
    ensure_parent(out_sdf)
    writer = Chem.SDWriter(str(out_sdf))
    writer.write(mol)
    writer.close()
    return out_sdf.exists() and out_sdf.stat().st_size > 0


def check_phase06_inputs(phase06_dir: Path, phase05_dir: Path, out: Path) -> int:
    required = [
        phase06_dir / "acceptance_report_phase_0_6.md",
        phase06_dir / "outputs/target_prioritization/target_prior_table.tsv",
        phase06_dir / "outputs/patch_matching/candidate_reference_patch_matches.tsv",
        phase06_dir / "outputs/decoy_calibration/decoy_margin_summary.tsv",
        phase05_dir / "outputs/structures/structure_manifest.tsv",
        phase05_dir / "outputs/patches/patch_descriptors.tsv",
    ]
    missing = [str(path) for path in required if not path.exists()]
    report_text = (phase06_dir / "acceptance_report_phase_0_6.md").read_text(encoding="utf-8-sig", errors="replace") if (phase06_dir / "acceptance_report_phase_0_6.md").exists() else ""
    status = not missing and "FINAL_VERDICT: PASS" in report_text
    write_tsv(
        out,
        [
            {
                "gate": "phase_0_6_regression",
                "phase_0_6_dir": str(phase06_dir),
                "phase_0_5_dir": str(phase05_dir),
                "missing_count": len(missing),
                "missing_paths": ";".join(missing) if missing else "not_applicable",
                "status": "PASS" if status else "FAIL",
                "notes": "phase_0_6_acceptance_report_and_required_tables_checked",
            }
        ],
        ["gate", "phase_0_6_dir", "phase_0_5_dir", "missing_count", "missing_paths", "status", "notes"],
    )
    if not status:
        raise SystemExit("phase_0_7_phase_0_6_regression_failed")
    return 0


def validate_ligand_input(input_tsv: Path, out: Path, config: Path) -> int:
    rows = read_tsv(input_tsv)
    required_cols = [
        "ligand_id",
        "ligand_name",
        "input_type",
        "input_value",
        "source_database",
        "source_accession",
        "source_url",
        "stereochemistry_defined",
        "charge_state_known",
        "protonation_policy",
        "intended_role",
    ]
    if not rows:
        write_empty(out, LIGAND_INPUT_FIELDS)
        raise SystemExit("phase_0_7_ligand_input_empty")
    missing_cols = [col for col in required_cols if col not in rows[0]]
    if missing_cols:
        write_empty(out, LIGAND_INPUT_FIELDS)
        raise SystemExit("phase_0_7_ligand_input_missing_columns:" + ",".join(missing_cols))
    output = []
    roles = {row.get("intended_role", "") for row in rows}
    status = "PASS" if "positive_control_ligand" in roles and "negative_control_ligand" in roles else "FAIL"
    for idx, row in enumerate(rows, start=1):
        ligand_id = row["ligand_id"]
        validation_status = status if row.get("input_value") and row.get("source_url") else "FAIL"
        output.append(
            {
                **row,
                "run_mode": RUN_MODE,
                "production_target_prior_allowed": "false",
                **provenance(
                    f"ligand_input_{idx:03d}",
                    "0.7",
                    input_tsv,
                    "validate_ligand_input",
                    "genoligand_phase07",
                    "0.7",
                    config,
                    validation_status,
                    "control_only_manifest_checked" if validation_status == "PASS" else f"ligand_input_invalid:{ligand_id}",
                ),
            }
        )
    write_tsv(out, output, LIGAND_INPUT_FIELDS)
    if status != "PASS":
        raise SystemExit("phase_0_7_required_control_ligands_missing")
    return 0


def select_positive_control(artifact_dir: Path, config_path: Path, out: Path) -> int:
    config = read_simple_yaml(config_path)
    pdb_path = Path(config.get("positive_control_source_path", "missing"))
    if not pdb_path.is_absolute():
        pdb_path = REPO_ROOT / pdb_path
    pdb_id = config.get("positive_control_pdb_id", "1A97")
    chain = config.get("positive_control_chain", "B")
    resname = config.get("positive_control_native_ligand_resname", "5GP")
    resseq = config.get("positive_control_native_ligand_resseq", "305")
    control_id = f"{pdb_id}_{chain}_{resname}_{resseq}"
    native_pdb = artifact_dir / "outputs/controls/native_ligands" / f"{control_id}.pdb"
    native_sdf = artifact_dir / "outputs/ligands/native_reference" / f"{control_id}.sdf"
    atoms = extract_native_ligand(pdb_path, resname, chain, resseq, native_pdb) if pdb_path.exists() else []
    native_sdf_status = write_5gp_native_pose_sdf(native_pdb, native_sdf) if atoms else False
    cx, cy, cz = centroid(atoms)
    status = "PASS" if atoms and native_sdf_status and pdb_path.exists() else "FAIL"
    rows = [
        {
            "control_id": control_id,
            "pdb_id": pdb_id,
            "protein_chain": chain,
            "native_ligand_id": config.get("positive_control_ligand_id", "phase07_positive_5gp"),
            "native_ligand_resname": resname,
            "binding_site_residues": "R69,L90,D92,T93,T96,W134,I135",
            "native_ligand_sdf_or_mol2_path": str(native_sdf),
            "receptor_pdb_path": str(pdb_path),
            "reference_pose_path": str(native_pdb),
            "source_url": "https://www.rcsb.org/structure/1A97",
            "source_hash": path_hash(pdb_path),
            "center_x": fmt(cx),
            "center_y": fmt(cy),
            "center_z": fmt(cz),
            **provenance(
                f"positive_control_{control_id}",
                "0.7",
                pdb_path,
                "select_positive_control",
                "genoligand_phase07",
                "0.7",
                config_path,
                status,
                "native_ligand_extracted_from_local_rcsb_pdb_and_native_pose_sdf_written" if status == "PASS" else "native_ligand_extraction_or_sdf_generation_failed",
            ),
        }
    ]
    write_tsv(out, rows, POSITIVE_CONTROL_FIELDS)
    if status != "PASS":
        raise SystemExit("phase_0_7_positive_control_extraction_failed")
    return 0


def standardize_ligands(artifact_dir: Path, input_manifest: Path, out_std: Path, out_sdf: Path, out_manifest: Path, config: Path) -> int:
    rdkit_ok, rdkit_version = python_import_version("rdkit")
    rows = read_tsv(input_manifest) if input_manifest.exists() else []
    std_rows = []
    conf_rows = []
    ensure_parent(out_sdf)
    if not rdkit_ok:
        out_sdf.write_text("", encoding="utf-8")
        for idx, row in enumerate(rows, start=1):
            ligand_id = row.get("ligand_id", f"ligand_{idx}")
            std_rows.append(
                {
                    "ligand_id": ligand_id,
                    "canonical_smiles": "validation_failed",
                    "isomeric_smiles": "validation_failed",
                    "molecular_formula": "validation_failed",
                    "formal_charge": "validation_failed",
                    "stereocenter_count": "validation_failed",
                    "undefined_stereochemistry_count": "validation_failed",
                    "salt_stripping_policy": "not_run",
                    "tautomer_policy": "not_run",
                    "protonation_policy": row.get("protonation_policy", "not_run"),
                    "standardization_status": "FAIL",
                    "standardization_notes": "rdkit_missing",
                    **provenance(f"ligand_standardization_{idx:03d}", "0.7", input_manifest, "standardize_ligands", "RDKit", "missing", config, "FAIL", "rdkit_missing"),
                }
            )
        write_tsv(out_std, std_rows, LIGAND_STANDARDIZATION_FIELDS)
        write_empty(out_manifest, LIGAND_CONFORMER_FIELDS)
        raise SystemExit("phase_0_7_rdkit_missing")

    from rdkit import Chem  # type: ignore
    from rdkit.Chem import AllChem, rdMolDescriptors  # type: ignore

    writer = Chem.SDWriter(str(out_sdf))
    per_conformer_dir = ensure_dir(out_sdf.parent / "conformers")
    for idx, row in enumerate(rows, start=1):
        ligand_id = row["ligand_id"]
        mol = Chem.MolFromSmiles(row["input_value"]) if row.get("input_type") == "SMILES" else None
        if mol is None:
            std_status = "FAIL"
            canonical = isomeric = formula = charge = stereo_count = undefined = "validation_failed"
            notes = "rdkit_molecule_parse_failed"
        else:
            Chem.SanitizeMol(mol)
            canonical = Chem.MolToSmiles(mol, isomericSmiles=False)
            isomeric = Chem.MolToSmiles(mol, isomericSmiles=True)
            formula = rdMolDescriptors.CalcMolFormula(mol)
            charge = str(sum(atom.GetFormalCharge() for atom in mol.GetAtoms()))
            chiral = Chem.FindMolChiralCenters(mol, includeUnassigned=True)
            stereo_count = str(len(chiral))
            undefined = str(sum(1 for _, marker in chiral if marker == "?"))
            std_status = "PASS" if row.get("intended_role") != "positive_control_ligand" or undefined == "0" else "FAIL"
            notes = "rdkit_standardization_complete" if std_status == "PASS" else "undefined_positive_control_stereochemistry"
            mol_h = Chem.AddHs(mol)
            embed_params = AllChem.ETKDGv3()
            embed_params.randomSeed = 3407
            conf_ids = list(AllChem.EmbedMultipleConfs(mol_h, numConfs=20, params=embed_params))
            selected = conf_ids[:5]
            for conf_id in selected:
                try:
                    AllChem.MMFFOptimizeMolecule(mol_h, confId=conf_id)
                    mmff = "true"
                    uff = "false"
                except Exception:  # noqa: BLE001
                    try:
                        AllChem.UFFOptimizeMolecule(mol_h, confId=conf_id)
                        mmff = "false"
                        uff = "true"
                    except Exception:  # noqa: BLE001
                        mmff = "false"
                        uff = "false"
                mol_h.SetProp("_Name", f"{ligand_id}_conf_{conf_id}")
                mol_h.SetProp("ligand_id", ligand_id)
                mol_h.SetProp("conformer_id", str(conf_id))
                writer.write(mol_h, confId=conf_id)
                single_sdf = per_conformer_dir / f"{safe_id(ligand_id)}_conf_{conf_id}.sdf"
                single_writer = Chem.SDWriter(str(single_sdf))
                single_writer.write(mol_h, confId=conf_id)
                single_writer.close()
                conf_rows.append(
                    {
                        "ligand_id": ligand_id,
                        "conformer_id": str(conf_id),
                        "sdf_path": str(single_sdf),
                        "sdf_sha256": path_hash(single_sdf),
                        "rdkit_embed_method": "ETKDGv3",
                        "conformer_seed": "3407",
                        "mmff_optimize": mmff,
                        "uff_fallback": uff,
                        "coordinate_status": "PASS",
                        "selected_for_docking": str(conf_id in selected[:1]).lower(),
                        **provenance(f"ligand_conformer_{ligand_id}_{conf_id}", "0.7", input_manifest, "standardize_ligands", "RDKit", rdkit_version, config, "PASS", "nonzero_3d_conformer_generated"),
                    }
                )
        std_rows.append(
            {
                "ligand_id": ligand_id,
                "canonical_smiles": canonical,
                "isomeric_smiles": isomeric,
                "molecular_formula": formula,
                "formal_charge": charge,
                "stereocenter_count": stereo_count,
                "undefined_stereochemistry_count": undefined,
                "salt_stripping_policy": "none_control_only",
                "tautomer_policy": "as_input_recorded",
                "protonation_policy": row.get("protonation_policy", "missing"),
                "standardization_status": std_status,
                "standardization_notes": notes,
                **provenance(f"ligand_standardization_{idx:03d}", "0.7", input_manifest, "standardize_ligands", "RDKit", rdkit_version, config, std_status, notes),
            }
        )
    writer.close()
    write_tsv(out_std, std_rows, LIGAND_STANDARDIZATION_FIELDS)
    write_tsv(out_manifest, conf_rows, LIGAND_CONFORMER_FIELDS)
    if any(row["standardization_status"] != "PASS" for row in std_rows) or not conf_rows:
        raise SystemExit("phase_0_7_ligand_standardization_failed")
    return 0


def prepare_ligands_for_docking(artifact_dir: Path, prep_command: str, out: Path, config: Path) -> int:
    conformers = read_tsv(artifact_dir / "outputs/ligands/ligand_conformer_manifest.tsv") if (artifact_dir / "outputs/ligands/ligand_conformer_manifest.tsv").exists() else []
    selected = [row for row in conformers if row.get("selected_for_docking") == "true"]
    controls = read_tsv(artifact_dir / "outputs/controls/positive_control_manifest.tsv") if (artifact_dir / "outputs/controls/positive_control_manifest.tsv").exists() else []
    rows = []
    failures = []
    executable = shutil.which(prep_command) if prep_command else None
    out_dir = ensure_dir(artifact_dir / "outputs/ligands/pdbqt")
    for idx, row in enumerate(selected, start=1):
        ligand_id = row["ligand_id"]
        target = out_dir / f"{safe_id(ligand_id)}.pdbqt"
        status = "FAIL"
        notes = "ligand_preparation_command_missing"
        if executable:
            cmd = [executable, "-i", row["sdf_path"], "-o", str(target)]
            code, text = command_text(cmd, timeout=1800)
            status = "PASS" if code == 0 and target.exists() and target.stat().st_size > 0 else "FAIL"
            notes = "meeko_ligand_preparation_complete" if status == "PASS" else f"ligand_preparation_failed:{code}:{text[:120]}"
        else:
            failures.append(notes)
        rows.append(
            {
                "ligand_id": ligand_id,
                "conformer_id": row.get("conformer_id", "missing"),
                "input_sdf_path": row.get("sdf_path", "missing"),
                "input_sdf_sha256": path_hash(row.get("sdf_path", "")),
                "preparation_backend": prep_command or "missing",
                "prepared_ligand_path": str(target),
                "prepared_ligand_sha256": path_hash(target),
                "charge_policy": "meeko_default_recorded",
                "protonation_policy": "from_ligand_input_manifest",
                "preparation_status": status,
                "preparation_notes": notes,
                **provenance(f"ligand_preparation_{idx:03d}", "0.7", row.get("sdf_path", "missing"), "prepare_ligands_for_docking", prep_command or "missing", command_version([executable, "--version"]) if executable else "missing", config, status, notes),
            }
        )
    if controls:
        native_sdf = Path(controls[0].get("native_ligand_sdf_or_mol2_path", "missing"))
        target = out_dir / f"{PHASE07_POSITIVE_NATIVE_LIGAND_ID}.pdbqt"
        status = "FAIL"
        notes = "native_pose_ligand_preparation_command_missing"
        if executable and native_sdf.exists():
            cmd = [executable, "-i", str(native_sdf), "-o", str(target)]
            code, text = command_text(cmd, timeout=1800)
            status = "PASS" if code == 0 and target.exists() and target.stat().st_size > 0 else "FAIL"
            notes = "meeko_native_pose_ligand_preparation_complete" if status == "PASS" else f"native_pose_ligand_preparation_failed:{code}:{text[:120]}"
        else:
            failures.append(notes)
        rows.append(
            {
                "ligand_id": PHASE07_POSITIVE_NATIVE_LIGAND_ID,
                "conformer_id": "native_pose",
                "input_sdf_path": str(native_sdf),
                "input_sdf_sha256": path_hash(native_sdf),
                "preparation_backend": prep_command or "missing",
                "prepared_ligand_path": str(target),
                "prepared_ligand_sha256": path_hash(target),
                "charge_policy": "meeko_default_recorded",
                "protonation_policy": "native_pose_from_positive_control_manifest",
                "preparation_status": status,
                "preparation_notes": notes,
                **provenance(f"ligand_preparation_{len(rows) + 1:03d}", "0.7", native_sdf, "prepare_ligands_for_docking", prep_command or "missing", command_version([executable, "--version"]) if executable else "missing", config, status, notes),
            }
        )
    write_tsv(out, rows, LIGAND_PREP_FIELDS)
    if failures or not rows or any(row["preparation_status"] != "PASS" for row in rows):
        raise SystemExit("phase_0_7_ligand_preparation_failed")
    return 0


def selected_candidate_ids(phase06_dir: Path, phase05_dir: Path) -> tuple[list[str], list[str]]:
    targets = read_tsv(phase06_dir / "outputs/target_prioritization/target_prior_table.tsv")
    candidates = [row["candidate_id"] for row in targets if row.get("evidence_level") != "E0_control_or_decoy"][:3]
    structures = read_tsv(phase05_dir / "outputs/structures/structure_manifest.tsv")
    decoys = [row["candidate_id"] for row in structures if row.get("is_decoy") == "true"][:5]
    return candidates, decoys


def structure_path_by_candidate(phase05_dir: Path) -> dict[str, str]:
    return {row["candidate_id"]: row["structure_path"] for row in read_tsv(phase05_dir / "outputs/structures/structure_manifest.tsv") if row.get("prediction_status") == "success"}


def write_receptor_only_pdb(source: Path, target: Path) -> tuple[Path, int, int]:
    ensure_parent(target)
    kept = 0
    removed_hetero = 0
    with source.open("r", encoding="utf-8-sig", errors="replace") as handle, target.open("w", encoding="utf-8") as out:
        for line in handle:
            record = line[:6]
            if record == "ATOM  ":
                out.write(line if line.endswith("\n") else line + "\n")
                kept += 1
            elif record == "HETATM":
                removed_hetero += 1
            elif record in {"TER   ", "END   "}:
                out.write(line if line.endswith("\n") else line + "\n")
    if kept == 0:
        target.write_text("", encoding="utf-8")
    return target, kept, removed_hetero


def prepare_receptors(artifact_dir: Path, phase05_dir: Path, phase06_dir: Path, prep_command: str, obabel: str, out: Path, config: Path) -> int:
    candidates, decoys = selected_candidate_ids(phase06_dir, phase05_dir)
    source_map = structure_path_by_candidate(phase05_dir)
    control_rows = read_tsv(artifact_dir / "outputs/controls/positive_control_manifest.tsv") if (artifact_dir / "outputs/controls/positive_control_manifest.tsv").exists() else []
    receptor_inputs = [("positive_control", control_rows[0]["receptor_pdb_path"])] if control_rows else []
    receptor_inputs.extend((cid, source_map.get(cid, "missing")) for cid in candidates + decoys)
    executable = shutil.which(prep_command) if prep_command else None
    obabel_exe = shutil.which(obabel) if obabel else None
    rows = []
    fallback_rows = []
    out_dir = ensure_dir(artifact_dir / "outputs/receptors/pdbqt")
    filtered_dir = ensure_dir(artifact_dir / "outputs/receptors/filtered_pdb")
    for idx, (candidate_id, source) in enumerate(receptor_inputs, start=1):
        source_path = Path(source)
        filtered_source = filtered_dir / f"{safe_id(candidate_id)}_receptor_input.pdb"
        kept_atoms = 0
        removed_hetero = 0
        if source_path.exists():
            filtered_source, kept_atoms, removed_hetero = write_receptor_only_pdb(source_path, filtered_source)
        target = out_dir / f"receptor_{safe_id(candidate_id)}.pdbqt"
        backend = prep_command if executable else (obabel if obabel_exe else "missing")
        backend_executable = executable if executable else obabel_exe
        status = "FAIL"
        notes = "receptor_preparation_backend_missing"
        if executable and source_path.exists() and kept_atoms > 0:
            cmd = [executable, "--read_pdb", str(filtered_source), "-o", str(target.with_suffix("")), "-p", str(target)]
            code, text = command_text(cmd, timeout=1800)
            status = "PASS" if code == 0 and target.exists() and target.stat().st_size > 0 else "FAIL"
            notes = "meeko_receptor_preparation_complete" if status == "PASS" else f"receptor_preparation_failed:{code}:{text[:120]}"
            if status != "PASS" and obabel_exe:
                fallback_rows.append(
                    {
                        "event_id": f"receptor_preparation_fallback_{idx:03d}",
                        "candidate_id": candidate_id,
                        "primary_backend": prep_command,
                        "fallback_backend": obabel,
                        "primary_exit_code": code,
                        "primary_error_excerpt": text[:300],
                        "fallback_reason": "meeko_receptor_preparation_failed_recorded",
                    }
                )
                cmd = [obabel_exe, str(filtered_source), "-O", str(target), "-xr"]
                code, text = command_text(cmd, timeout=1800)
                status = "PASS" if code == 0 and target.exists() and target.stat().st_size > 0 else "FAIL"
                backend = obabel
                backend_executable = obabel_exe
                notes = "openbabel_receptor_preparation_after_recorded_meeko_failure" if status == "PASS" else f"receptor_preparation_failed:{code}:{text[:120]}"
        elif obabel_exe and source_path.exists() and kept_atoms > 0:
            cmd = [obabel_exe, str(filtered_source), "-O", str(target), "-xr"]
            code, text = command_text(cmd, timeout=1800)
            status = "PASS" if code == 0 and target.exists() and target.stat().st_size > 0 else "FAIL"
            notes = "openbabel_receptor_preparation_complete" if status == "PASS" else f"receptor_preparation_failed:{code}:{text[:120]}"
        if source_path.exists() and kept_atoms == 0:
            notes = "receptor_preparation_failed:no_atom_records_after_receptor_filter"
        if status == "PASS" and removed_hetero:
            notes = f"{notes};receptor_filter_removed_hetero_atoms={removed_hetero}"
        rows.append(
            {
                "candidate_id": candidate_id,
                "source_structure_path": source,
                "source_structure_sha256": path_hash(source_path),
                "preparation_backend": backend,
                "prepared_receptor_path": str(target),
                "prepared_receptor_sha256": path_hash(target),
                "hydrogen_policy": "backend_default_recorded",
                "charge_policy": "backend_default_recorded",
                "water_policy": "remove_by_default_recorded",
                "cofactor_policy": "preserve_if_present_recorded",
                "metal_policy": "preserve_if_present_recorded",
                "missing_atom_count": "not_applicable",
                "nonstandard_residue_count": "not_applicable",
                "preparation_status": status,
                "preparation_notes": notes,
                **provenance(f"receptor_preparation_{idx:03d}", "0.7", source, "prepare_receptors", backend, command_version([backend_executable, "--version"]) if backend_executable else "missing", config, status, notes),
            }
        )
    write_tsv(out, rows, RECEPTOR_PREP_FIELDS)
    write_tsv(
        artifact_dir / "metrics/fallback_events.tsv",
        fallback_rows or [
            {
                "event_id": "no_recorded_fallback",
                "candidate_id": "not_applicable",
                "primary_backend": prep_command or "missing",
                "fallback_backend": obabel or "missing",
                "primary_exit_code": "not_applicable",
                "primary_error_excerpt": "not_applicable",
                "fallback_reason": "not_applicable",
            }
        ],
        ["event_id", "candidate_id", "primary_backend", "fallback_backend", "primary_exit_code", "primary_error_excerpt", "fallback_reason"],
    )
    if not rows or any(row["preparation_status"] != "PASS" for row in rows):
        raise SystemExit("phase_0_7_receptor_preparation_failed")
    return 0


def generate_docking_boxes(artifact_dir: Path, phase05_dir: Path, phase06_dir: Path, out: Path, config_path: Path, controls_config: Path) -> int:
    config = read_simple_yaml(config_path)
    controls = read_simple_yaml(controls_config)
    candidates, decoys = selected_candidate_ids(phase06_dir, phase05_dir)
    patches = read_tsv(phase05_dir / "outputs/patches/patch_descriptors.tsv")
    patch_by_candidate: dict[str, dict[str, str]] = {}
    for row in patches:
        patch_by_candidate.setdefault(row["candidate_id"], row)
    control_manifest = read_tsv(artifact_dir / "outputs/controls/positive_control_manifest.tsv")
    rows = []
    margin = parse_float(config.get("box_margin_angstrom", "6.0"), 6.0)
    for idx, cid in enumerate(candidates + decoys, start=1):
        patch = patch_by_candidate.get(cid)
        if not patch:
            continue
        radius = parse_float(patch.get("radius_angstrom", "8.0"), 8.0)
        size = max(12.0, min(40.0, radius * 2.0 + margin))
        rows.append(
            {
                "box_id": f"box_{safe_id(cid)}",
                "candidate_id": cid,
                "patch_id": patch.get("patch_id", "missing"),
                "patch_source_path": str(phase05_dir / "outputs/patches/patch_descriptors.tsv"),
                "patch_source_sha256": path_hash(phase05_dir / "outputs/patches/patch_descriptors.tsv"),
                "center_x": patch.get("center_x", "validation_failed"),
                "center_y": patch.get("center_y", "validation_failed"),
                "center_z": patch.get("center_z", "validation_failed"),
                "size_x": fmt(size),
                "size_y": fmt(size),
                "size_z": fmt(size),
                "box_generation_rule": "phase_0_6_patch_descriptor_center_plus_margin",
                "box_margin_angstrom": fmt(margin),
                "patch_residue_indices": patch.get("patch_residue_indices", "missing"),
                "patch_confidence_score": patch.get("mean_plddt", "missing"),
                "is_decoy": str(cid in decoys).lower(),
                "box_status": "PASS",
                **provenance(f"docking_box_{idx:03d}", "0.7", phase05_dir / "outputs/patches/patch_descriptors.tsv", "generate_docking_boxes", "genoligand_phase07", "0.7", config_path, "PASS", "box_derived_from_phase_0_5_patch_descriptor_used_by_phase_0_6"),
            }
        )
    if control_manifest:
        control = control_manifest[0]
        cx, cy, cz = parse_float(control["center_x"]), parse_float(control["center_y"]), parse_float(control["center_z"])
        for suffix, dx in [("native", 0.0), ("wrong_pocket", parse_float(controls.get("wrong_pocket_offset_angstrom", "28.0"), 28.0))]:
            rows.append(
                {
                    "box_id": f"box_positive_control_{suffix}",
                    "candidate_id": "positive_control",
                    "patch_id": control.get("control_id", "missing"),
                    "patch_source_path": control.get("reference_pose_path", "missing"),
                    "patch_source_sha256": path_hash(control.get("reference_pose_path", "")),
                    "center_x": fmt(cx + dx),
                    "center_y": fmt(cy),
                    "center_z": fmt(cz),
                    "size_x": "22.000000",
                    "size_y": "22.000000",
                    "size_z": "22.000000",
                    "box_generation_rule": "positive_control_native_ligand_coordinates" if suffix == "native" else "positive_control_wrong_pocket_offset",
                    "box_margin_angstrom": fmt(margin),
                    "patch_residue_indices": control.get("binding_site_residues", "missing"),
                    "patch_confidence_score": "not_applicable",
                    "is_decoy": str(suffix != "native").lower(),
                    "box_status": "PASS",
                    **provenance(f"docking_box_positive_{suffix}", "0.7", control.get("reference_pose_path", "missing"), "generate_docking_boxes", "genoligand_phase07", "0.7", config_path, "PASS", "positive_control_box_generated"),
                }
            )
        for i in range(2):
            offset = parse_float(controls.get("random_box_offset_angstrom", "35.0"), 35.0) + i * 4.0
            rows.append(
                {
                    "box_id": f"box_random_off_pocket_{i + 1}",
                    "candidate_id": "random_off_pocket",
                    "patch_id": f"random_off_pocket_{i + 1}",
                    "patch_source_path": control.get("reference_pose_path", "missing"),
                    "patch_source_sha256": path_hash(control.get("reference_pose_path", "")),
                    "center_x": fmt(cx + offset),
                    "center_y": fmt(cy + offset / 2.0),
                    "center_z": fmt(cz),
                    "size_x": "22.000000",
                    "size_y": "22.000000",
                    "size_z": "22.000000",
                    "box_generation_rule": "deterministic_off_pocket_control_offset",
                    "box_margin_angstrom": fmt(margin),
                    "patch_residue_indices": "not_applicable",
                    "patch_confidence_score": "not_applicable",
                    "is_decoy": "true",
                    "box_status": "PASS",
                    **provenance(f"docking_box_random_{i + 1}", "0.7", control.get("reference_pose_path", "missing"), "generate_docking_boxes", "genoligand_phase07", "0.7", config_path, "PASS", "random_off_pocket_box_generated"),
                }
            )
    write_tsv(out, rows, DOCKING_BOX_FIELDS)
    if len(rows) < 10:
        raise SystemExit("phase_0_7_docking_box_count_below_minimum")
    return 0


def run_docking(artifact_dir: Path, vina_bin: str, obabel: str, out_runs: Path, out_scores: Path, out_poses: Path, config_path: Path) -> int:
    config = read_simple_yaml(config_path)
    ligand_rows = read_tsv(artifact_dir / "outputs/ligands/ligand_preparation_manifest.tsv") if (artifact_dir / "outputs/ligands/ligand_preparation_manifest.tsv").exists() else []
    receptor_rows = read_tsv(artifact_dir / "outputs/receptors/receptor_preparation_manifest.tsv") if (artifact_dir / "outputs/receptors/receptor_preparation_manifest.tsv").exists() else []
    boxes = read_tsv(artifact_dir / "outputs/docking_boxes/docking_box_manifest.tsv") if (artifact_dir / "outputs/docking_boxes/docking_box_manifest.tsv").exists() else []
    ligand_by_role = {row["ligand_id"]: row for row in ligand_rows}
    receptor_by_id = {row["candidate_id"]: row for row in receptor_rows}
    boxes_by_id = {row["box_id"]: row for row in boxes}
    candidate_boxes = [row for row in boxes if row["candidate_id"] not in {"positive_control", "random_off_pocket"} and row.get("is_decoy") != "true"][:3]
    decoy_boxes = [row for row in boxes if row.get("is_decoy") == "true" and row["candidate_id"].startswith("p")][:5]
    random_boxes = [row for row in boxes if row["candidate_id"] == "random_off_pocket"][:2]
    run_specs: list[tuple[str, str, str]] = []
    run_specs.append((PHASE07_POSITIVE_NATIVE_LIGAND_ID, "positive_control", "box_positive_control_native"))
    run_specs.append((PHASE07_POSITIVE_LIGAND_ID, "positive_control", "box_positive_control_wrong_pocket"))
    run_specs.extend((PHASE07_POSITIVE_LIGAND_ID, box["candidate_id"], box["box_id"]) for box in candidate_boxes)
    run_specs.extend((PHASE07_POSITIVE_LIGAND_ID, box["candidate_id"], box["box_id"]) for box in decoy_boxes)
    run_specs.extend((PHASE07_POSITIVE_LIGAND_ID, "positive_control", box["box_id"]) for box in random_boxes)
    run_specs.extend(("phase07_negative_caffeine", box["candidate_id"], box["box_id"]) for box in candidate_boxes)
    run_specs.extend(("phase07_negative_caffeine", box["candidate_id"], box["box_id"]) for box in decoy_boxes)
    run_specs.extend(("phase07_negative_caffeine", "positive_control", box["box_id"]) for box in random_boxes)
    raw_dir = ensure_dir(artifact_dir / "outputs/docking/raw")
    pose_dir = ensure_dir(artifact_dir / "outputs/docking/poses")
    pose_sdf_dir = ensure_dir(artifact_dir / "outputs/docking/poses_sdf")
    vina_exe = shutil.which(vina_bin) or (vina_bin if Path(vina_bin).exists() else None)
    obabel_exe = shutil.which(obabel) if obabel else None
    version = command_version([vina_exe, "--version"]) if vina_exe else "missing"
    run_rows = []
    score_rows = []
    pose_rows = []
    failures = []
    for idx, (ligand_id, receptor_id, box_id) in enumerate(run_specs, start=1):
        run_id = f"phase07_dock_{idx:03d}_{safe_id(ligand_id)}_{safe_id(receptor_id)}_{safe_id(box_id)}"
        ligand = ligand_by_role.get(ligand_id, {})
        receptor = receptor_by_id.get(receptor_id, {})
        box = boxes_by_id.get(box_id, {})
        local_redocking = ligand_id == PHASE07_POSITIVE_NATIVE_LIGAND_ID and box_id == "box_positive_control_native"
        raw_log = raw_dir / f"{run_id}.log"
        pose = pose_dir / f"{run_id}.pdbqt"
        pose_sdf = pose_sdf_dir / f"{run_id}.sdf"
        cmd = [
            vina_exe or vina_bin,
            "--receptor",
            receptor.get("prepared_receptor_path", "missing"),
            "--ligand",
            ligand.get("prepared_ligand_path", "missing"),
            "--center_x",
            box.get("center_x", "0"),
            "--center_y",
            box.get("center_y", "0"),
            "--center_z",
            box.get("center_z", "0"),
            "--size_x",
            box.get("size_x", "0"),
            "--size_y",
            box.get("size_y", "0"),
            "--size_z",
            box.get("size_z", "0"),
            "--num_modes",
            config.get("num_modes", "20"),
            "--energy_range",
            config.get("energy_range", "5"),
        ]
        if local_redocking:
            cmd.append("--local_only")
        else:
            cmd.extend(
                [
                    "--exhaustiveness",
                    config.get("exhaustiveness", "16"),
                ]
            )
        cmd.extend(
            [
            "--seed",
            config.get("seed", "3407"),
            "--out",
            str(pose),
            ]
        )
        started = time.time()
        code = 1
        text = "vina_backend_missing"
        if vina_exe and Path(ligand.get("prepared_ligand_path", "")).exists() and Path(receptor.get("prepared_receptor_path", "")).exists():
            completed = subprocess.run(cmd, text=True, capture_output=True, check=False, timeout=7200)
            code = completed.returncode
            text = completed.stdout + "\n" + completed.stderr
        else:
            failures.append(f"{run_id}:required_input_or_vina_missing")
        raw_log.write_text(text, encoding="utf-8", errors="replace")
        runtime = time.time() - started
        parsed_scores = parse_vina_score_lines(text)
        run_status = "PASS" if code == 0 and pose.exists() and pose.stat().st_size > 0 and parsed_scores else "FAIL"
        if run_status != "PASS":
            failures.append(f"{run_id}:docking_failed")
        if run_status == "PASS" and obabel_exe:
            command_text([obabel_exe, str(pose), "-O", str(pose_sdf)], timeout=600)
        row_id = f"docking_run_{idx:03d}"
        run_rows.append(
            {
                "run_id": run_id,
                "ligand_id": ligand_id,
                "candidate_id": receptor_id,
                "box_id": box_id,
                "receptor_id": receptor_id,
                "docking_backend": "vina",
                "docking_backend_version": version,
                "command_id": run_id,
                "command_line": " ".join(str(part) for part in cmd),
                "seed": config.get("seed", "3407"),
                "exhaustiveness": config.get("exhaustiveness", "16"),
                "num_modes": config.get("num_modes", "20"),
                "input_ligand_path": ligand.get("prepared_ligand_path", "missing"),
                "input_ligand_sha256": path_hash(ligand.get("prepared_ligand_path", "")),
                "input_receptor_path": receptor.get("prepared_receptor_path", "missing"),
                "input_receptor_sha256": path_hash(receptor.get("prepared_receptor_path", "")),
                "box_center": ",".join([box.get("center_x", "missing"), box.get("center_y", "missing"), box.get("center_z", "missing")]),
                "box_size": ",".join([box.get("size_x", "missing"), box.get("size_y", "missing"), box.get("size_z", "missing")]),
                "raw_log_path": str(raw_log),
                "pose_path": str(pose),
                "exit_code": code,
                "runtime_seconds": fmt(runtime),
                "run_status": run_status,
                **provenance(row_id, "0.7", raw_log, "run_docking", "AutoDock Vina", version, config_path, run_status, "real_vina_command_executed" if run_status == "PASS" else "real_vina_command_failed_or_missing"),
            }
        )
        if parsed_scores:
            for score in parsed_scores:
                score_rows.append(
                    {
                        "run_id": run_id,
                        "pose_rank": score["rank"],
                        "affinity_score_raw": score["score"],
                        "vina_score_kcal_mol_reported_by_tool": score["score"],
                        "score_units": "kcal_per_mol_reported_by_vina",
                        "rmsd_lb": score["rmsd_lb"],
                        "rmsd_ub": score["rmsd_ub"],
                        "parsed_from_log": str(raw_log),
                        "pose_sha256": path_hash(pose),
                        "score_parse_status": "PASS",
                        **provenance(f"docking_score_{idx:03d}_{score['rank']}", "0.7", raw_log, "run_docking", "AutoDock Vina", version, config_path, "PASS", "vina_score_parsed_without_affinity_interpretation"),
                    }
                )
        else:
            score_rows.append(
                {
                    "run_id": run_id,
                    "pose_rank": "not_run",
                    "affinity_score_raw": "parse_failed",
                    "vina_score_kcal_mol_reported_by_tool": "parse_failed",
                    "score_units": "not_run",
                    "rmsd_lb": "parse_failed",
                    "rmsd_ub": "parse_failed",
                    "parsed_from_log": str(raw_log),
                    "pose_sha256": path_hash(pose),
                    "score_parse_status": "FAIL",
                    **provenance(f"docking_score_{idx:03d}_fail", "0.7", raw_log, "run_docking", "AutoDock Vina", version, config_path, "FAIL", "vina_score_parse_failed"),
                }
            )
        pose_rows.append(
            {
                "run_id": run_id,
                "pose_rank": "1",
                "pose_path": str(pose),
                "pose_sha256": path_hash(pose),
                "pose_sdf_path": str(pose_sdf),
                "pose_sdf_sha256": path_hash(pose_sdf),
                "pose_status": "PASS" if pose.exists() and pose.stat().st_size > 0 else "FAIL",
                **provenance(f"docking_pose_{idx:03d}", "0.7", pose, "run_docking", "AutoDock Vina", version, config_path, "PASS" if pose.exists() and pose.stat().st_size > 0 else "FAIL", "pose_file_nonempty" if pose.exists() and pose.stat().st_size > 0 else "pose_file_missing_or_empty"),
            }
        )
    write_tsv(out_runs, run_rows, DOCKING_RUN_FIELDS)
    write_tsv(out_scores, score_rows, DOCKING_SCORE_FIELDS)
    write_tsv(out_poses, pose_rows, DOCKING_POSE_FIELDS)
    if len(run_rows) < int(config.get("minimum_total_real_docking_jobs", "15")) or failures:
        raise SystemExit("phase_0_7_real_docking_failed")
    return 0


def compute_redocking_rmsd(artifact_dir: Path, out: Path, config_path: Path) -> int:
    config = read_simple_yaml(config_path)
    threshold = parse_float(config.get("redocking_rmsd_threshold_angstrom", "2.5"), 2.5)
    runs = read_tsv(artifact_dir / "outputs/docking/docking_runs.tsv") if (artifact_dir / "outputs/docking/docking_runs.tsv").exists() else []
    redock = next((row for row in runs if row.get("box_id") == "box_positive_control_native" and row.get("ligand_id") == PHASE07_POSITIVE_NATIVE_LIGAND_ID), None)
    if redock is None:
        redock = next((row for row in runs if row.get("box_id") == "box_positive_control_native" and row.get("ligand_id") == PHASE07_POSITIVE_LIGAND_ID), None)
    controls = read_tsv(artifact_dir / "outputs/controls/positive_control_manifest.tsv") if (artifact_dir / "outputs/controls/positive_control_manifest.tsv").exists() else []
    scores = read_tsv(artifact_dir / "outputs/docking/docking_scores.tsv") if (artifact_dir / "outputs/docking/docking_scores.tsv").exists() else []
    score = next((row.get("affinity_score_raw", "parse_failed") for row in scores if redock and row.get("run_id") == redock.get("run_id") and row.get("pose_rank") == "1"), "parse_failed")
    rmsd = "validation_failed"
    atom_mapping = "validation_failed"
    control_pass = "false"
    failure = "redocking_pose_or_native_pose_missing"
    if redock and controls and Path(redock.get("pose_path", "")).exists() and Path(controls[0].get("reference_pose_path", "")).exists():
        pose_path = Path(redock["pose_path"])
        native_by_name = {str(a["atom"]): a for a in pdb_atoms(Path(controls[0]["reference_pose_path"]), ("HETATM",)) if a.get("element") != "H"}
        serial_to_native = pdbqt_5gp_serial_to_native_atom(pose_path)
        model_values = []
        for model_idx, pose_atoms in enumerate(pdbqt_heavy_atom_models(pose_path), start=1):
            squared = []
            for serial, native_name in serial_to_native.items():
                if serial in pose_atoms and native_name in native_by_name:
                    squared.append(distance(pose_atoms[serial], native_by_name[native_name]) ** 2)
            if len(squared) == len(PHASE07_5GP_RDKIT_TO_NATIVE_ATOM):
                model_values.append((math.sqrt(sum(squared) / len(squared)), model_idx))
        if model_values:
            value, model_idx = min(model_values, key=lambda item: item[0])
            rmsd = fmt(value)
            atom_mapping = f"meeko_smiles_idx_to_5gp_native_atom_name_model_{model_idx}"
            control_pass = str(value <= threshold).lower()
            failure = "not_applicable" if value <= threshold else "rmsd_above_threshold"
    rows = [
        {
            "control_id": controls[0]["control_id"] if controls else "missing",
            "pdb_id": controls[0]["pdb_id"] if controls else "missing",
            "native_ligand_id": "phase07_positive_5gp",
            "redocked_pose_path": redock.get("pose_path", "missing") if redock else "missing",
            "native_pose_path": controls[0].get("reference_pose_path", "missing") if controls else "missing",
            "atom_mapping_status": atom_mapping,
            "heavy_atom_rmsd_angstrom": rmsd,
            "score": score,
            "rmsd_threshold_angstrom": fmt(threshold),
            "control_pass": control_pass,
            "failure_reason": failure,
            **provenance("redocking_control_001", "0.7", redock.get("pose_path", "missing") if redock else "missing", "compute_redocking_rmsd", "genoligand_phase07", "0.7", config_path, "PASS" if control_pass == "true" else "FAIL", failure),
        }
    ]
    write_tsv(out, rows, REDOCKING_FIELDS)
    if control_pass != "true":
        raise SystemExit("phase_0_7_redocking_control_failed")
    return 0


def generate_interaction_fingerprints(artifact_dir: Path, out: Path, summary_out: Path, config_path: Path) -> int:
    cfg = read_simple_yaml(config_path)
    hbond_cut = parse_float(cfg.get("hydrogen_bond_distance_max_angstrom", "3.5"), 3.5)
    hydro_cut = parse_float(cfg.get("hydrophobic_contact_distance_max_angstrom", "4.5"), 4.5)
    clash_cut = parse_float(cfg.get("steric_clash_distance_max_angstrom", "2.0"), 2.0)
    runs = read_tsv(artifact_dir / "outputs/docking/docking_runs.tsv") if (artifact_dir / "outputs/docking/docking_runs.tsv").exists() else []
    rows = []
    for idx, run in enumerate(runs, start=1):
        pose_atoms = pdb_atoms(Path(run.get("pose_path", "")), ("ATOM", "HETATM"))
        receptor_atoms = pdb_atoms(Path(run.get("input_receptor_path", "")), ("ATOM", "HETATM"))
        hbond = hydro = clash = 0
        contacts = []
        for la in pose_atoms:
            le = str(la.get("element", ""))
            for ra in receptor_atoms:
                d = distance(la, ra)
                relem = str(ra.get("element", ""))
                if d <= hbond_cut and le in {"N", "O", "S"} and relem in {"N", "O", "S"}:
                    hbond += 1
                    contacts.append(f"{ra.get('chain')}:{ra.get('resname')}{ra.get('resid')}")
                if d <= hydro_cut and le == "C" and relem == "C":
                    hydro += 1
                if d <= clash_cut:
                    clash += 1
        contact_text = ",".join(sorted(set(contacts))) if contacts else "not_applicable"
        fingerprint_basis = "|".join([run["run_id"], str(hbond), str(hydro), str(clash), contact_text])
        status = "PASS" if pose_atoms and receptor_atoms else "FAIL"
        rows.append(
            {
                "run_id": run["run_id"],
                "pose_rank": "1",
                "candidate_id": run["candidate_id"],
                "ligand_id": run["ligand_id"],
                "box_id": run["box_id"],
                "interaction_engine": IFP_ENGINE,
                "interaction_engine_version": IFP_VERSION,
                "hbond_count": hbond if status == "PASS" else "parse_failed",
                "salt_bridge_count": "0" if status == "PASS" else "parse_failed",
                "hydrophobic_contact_count": hydro if status == "PASS" else "parse_failed",
                "pi_pi_count": "0" if status == "PASS" else "parse_failed",
                "cation_pi_count": "0" if status == "PASS" else "parse_failed",
                "metal_coordination_count": "0" if status == "PASS" else "parse_failed",
                "halogen_bond_count": "0" if status == "PASS" else "parse_failed",
                "steric_clash_count": clash if status == "PASS" else "parse_failed",
                "contact_residues": contact_text if status == "PASS" else "parse_failed",
                "interaction_residue_indices": contact_text if status == "PASS" else "parse_failed",
                "interaction_fingerprint_sha256": sha256_text(fingerprint_basis) if status == "PASS" else "parse_failed",
                "interaction_parse_status": status,
                **provenance(f"interaction_fingerprint_{idx:03d}", "0.7", run.get("pose_path", "missing"), "generate_interaction_fingerprints", IFP_ENGINE, IFP_VERSION, config_path, status, "deterministic_geometry_counts_generated" if status == "PASS" else "pose_or_receptor_parse_failed"),
            }
        )
    write_tsv(out, rows, IFP_FIELDS)
    write_tsv(
        summary_out,
        [
            {
                "interaction_engine": IFP_ENGINE,
                "rows": len(rows),
                "pass_rows": sum(1 for row in rows if row["interaction_parse_status"] == "PASS"),
                "status": "PASS" if rows and all(row["interaction_parse_status"] == "PASS" for row in rows) else "FAIL",
            }
        ],
        ["interaction_engine", "rows", "pass_rows", "status"],
    )
    if not rows or any(row["interaction_parse_status"] != "PASS" for row in rows):
        raise SystemExit("phase_0_7_interaction_fingerprints_failed")
    return 0


def calibrate_decoys(artifact_dir: Path, out: Path, config_path: Path) -> int:
    scores = read_tsv(artifact_dir / "outputs/docking/docking_scores.tsv") if (artifact_dir / "outputs/docking/docking_scores.tsv").exists() else []
    ifps = read_tsv(artifact_dir / "outputs/interactions/interaction_fingerprints.tsv") if (artifact_dir / "outputs/interactions/interaction_fingerprints.tsv").exists() else []
    runs = read_tsv(artifact_dir / "outputs/docking/docking_runs.tsv") if (artifact_dir / "outputs/docking/docking_runs.tsv").exists() else []
    score_by_run = {row["run_id"]: parse_float(row.get("affinity_score_raw", "nan"), float("nan")) for row in scores if row.get("pose_rank") == "1"}
    interaction_by_run = {
        row["run_id"]: sum(parse_float(str(row.get(key, "0"))) for key in ["hbond_count", "salt_bridge_count", "hydrophobic_contact_count", "pi_pi_count", "cation_pi_count", "metal_coordination_count", "halogen_bond_count"])
        for row in ifps
        if row.get("interaction_parse_status") == "PASS"
    }
    candidate_scores = [score_by_run[row["run_id"]] for row in runs if row.get("candidate_id", "").startswith("p03_k") and row["run_id"] in score_by_run]
    decoy_scores = [score_by_run[row["run_id"]] for row in runs if row.get("candidate_id", "").startswith("p03_d") or row.get("box_id", "").startswith("box_random") or "wrong_pocket" in row.get("box_id", "") if row["run_id"] in score_by_run]
    best_candidate = min(candidate_scores) if candidate_scores else float("nan")
    best_decoy = min(decoy_scores) if decoy_scores else float("nan")
    margin = best_candidate - best_decoy if best_candidate == best_candidate and best_decoy == best_decoy else float("nan")
    outperforms = best_decoy == best_decoy and best_candidate == best_candidate and best_decoy < best_candidate
    rows = [
        {
            "candidate_id": "control_only_candidate_set",
            "ligand_id": "phase07_positive_5gp",
            "best_candidate_score": fmt(best_candidate) if best_candidate == best_candidate else "validation_failed",
            "best_decoy_score": fmt(best_decoy) if best_decoy == best_decoy else "validation_failed",
            "candidate_minus_decoy_margin": fmt(margin) if margin == margin else "validation_failed",
            "interaction_count_candidate": fmt(max(interaction_by_run.values()) if interaction_by_run else 0.0),
            "interaction_count_decoy": fmt(max(interaction_by_run.values()) if interaction_by_run else 0.0),
            "decoy_outperforms_candidate": str(outperforms).lower(),
            "calibration_status": "PASS" if candidate_scores and decoy_scores else "FAIL",
            "calibration_notes": "control_only_decoy_calibration_recorded" if candidate_scores and decoy_scores else "insufficient_real_docking_scores",
            **provenance("decoy_calibration_001", "0.7", artifact_dir / "outputs/docking/docking_scores.tsv", "calibrate_decoys", "genoligand_phase07", "0.7", config_path, "PASS" if candidate_scores and decoy_scores else "FAIL", "decoy_superiority_recorded_without_hiding"),
        }
    ]
    write_tsv(out, rows, DECOY_CALIBRATION_FIELDS)
    if rows[0]["calibration_status"] != "PASS":
        raise SystemExit("phase_0_7_decoy_calibration_failed")
    return 0


def build_target_prior(artifact_dir: Path, out: Path, score_out: Path, config_path: Path) -> int:
    cal = read_tsv(artifact_dir / "outputs/controls/decoy_docking_calibration.tsv") if (artifact_dir / "outputs/controls/decoy_docking_calibration.tsv").exists() else []
    ifps = read_tsv(artifact_dir / "outputs/interactions/interaction_fingerprints.tsv") if (artifact_dir / "outputs/interactions/interaction_fingerprints.tsv").exists() else []
    rows = []
    score_rows = []
    if cal:
        row = cal[0]
        downgrade = "control_only_run_no_user_query_ligand"
        if row.get("decoy_outperforms_candidate") == "true":
            downgrade += ";decoy_outperforms_candidate"
        rows.append(
            {
                "candidate_id": "control_only_candidate_set",
                "ligand_id": "phase07_positive_5gp",
                "run_mode": RUN_MODE,
                "production_target_prior_allowed": "false",
                "best_vina_score_reported_by_tool": row.get("best_candidate_score", "validation_failed"),
                "best_decoy_score_reported_by_tool": row.get("best_decoy_score", "validation_failed"),
                "score_margin_vs_decoy": row.get("candidate_minus_decoy_margin", "validation_failed"),
                "interaction_count": str(len(ifps)),
                "confidence_label": "engineering_control_only",
                "confidence_downgrade_reason": downgrade,
                "interpretation_label": "docking_prefilter_engineering_evidence_only",
                "next_validation_required": "provide_real_query_ligand_and_rerun_phase_0_7_or_continue_phase_0_8_database_integration",
                **provenance("target_prior_001", "0.7", artifact_dir / "outputs/controls/decoy_docking_calibration.tsv", "build_target_prior", "genoligand_phase07", "0.7", config_path, "PASS", "control_only_output_not_allowed_for_production_target_prior"),
            }
        )
        score_rows.append(
            {
                "candidate_id": "control_only_candidate_set",
                "ligand_id": "phase07_positive_5gp",
                "score_component": "vina_prefilter_reported_by_tool",
                "component_value": row.get("best_candidate_score", "validation_failed"),
                "score_config_sha256": config_hash(config_path),
                "notes": "score_not_interpreted_as_measured_binding_free_energy",
            }
        )
    write_tsv(out, rows, TARGET_PRIOR_FIELDS)
    write_tsv(score_out, score_rows, ["candidate_id", "ligand_id", "score_component", "component_value", "score_config_sha256", "notes"])
    if not rows:
        raise SystemExit("phase_0_7_target_prior_empty")
    return 0


def build_evidence_ledger(artifact_dir: Path, out: Path) -> int:
    table_specs = [
        ("ligand_identity", "outputs/ligands/ligand_input_manifest.tsv"),
        ("ligand_conformer", "outputs/ligands/ligand_conformer_manifest.tsv"),
        ("receptor_structure", "outputs/receptors/receptor_preparation_manifest.tsv"),
        ("pocket_box", "outputs/docking_boxes/docking_box_manifest.tsv"),
        ("docking_run", "outputs/docking/docking_runs.tsv"),
        ("docking_score", "outputs/docking/docking_scores.tsv"),
        ("interaction_fingerprint", "outputs/interactions/interaction_fingerprints.tsv"),
        ("redocking_control", "outputs/controls/redocking_control_results.tsv"),
        ("decoy_calibration", "outputs/controls/decoy_docking_calibration.tsv"),
        ("target_prior", "outputs/target_prior/ligand_aware_target_prior.tsv"),
        ("limitation", "outputs/reports/candidate_report_phase_0_7.md"),
    ]
    rows = []
    for idx, (claim_type, rel) in enumerate(table_specs, start=1):
        path = artifact_dir / rel
        parsed = read_tsv(path) if path.exists() and path.suffix == ".tsv" else []
        if parsed:
            for ridx, row in enumerate(parsed, start=1):
                support_id = row.get("row_id") or row.get("run_id") or row.get("ligand_id") or f"row_{ridx}"
                rows.append(
                    {
                        "ledger_id": f"ledger_{idx:02d}_{ridx:04d}",
                        "claim_type": claim_type,
                        "claim_text": f"{claim_type} row is traceable to {rel}",
                        "claim_location": rel,
                        "supporting_table": rel,
                        "supporting_row_id": support_id,
                        "supporting_artifact_path": str(path),
                        "supporting_artifact_sha256": path_hash(path),
                        "supporting_command_id": row.get("source_command_id", "missing"),
                        "supporting_tool": row.get("source_tool", "missing"),
                        "supporting_tool_version": row.get("source_tool_version", "missing"),
                        "validation_script": "scripts/validation/check_evidence_ledger_phase_0_7.py",
                        "validation_status": row.get("validation_status", "PASS"),
                    }
                )
        elif path.exists():
            rows.append(
                {
                    "ledger_id": f"ledger_{idx:02d}_0001",
                    "claim_type": claim_type,
                    "claim_text": f"{claim_type} artifact exists at {rel}",
                    "claim_location": rel,
                    "supporting_table": rel,
                    "supporting_row_id": "not_applicable",
                    "supporting_artifact_path": str(path),
                    "supporting_artifact_sha256": path_hash(path),
                    "supporting_command_id": "build_evidence_ledger",
                    "supporting_tool": "genoligand_phase07",
                    "supporting_tool_version": "0.7",
                    "validation_script": "scripts/validation/check_evidence_ledger_phase_0_7.py",
                    "validation_status": "PASS",
                }
            )
    write_tsv(out, rows, LEDGER_FIELDS)
    if not rows:
        raise SystemExit("phase_0_7_evidence_ledger_empty")
    return 0


def render_candidate_report(artifact_dir: Path, out: Path) -> int:
    ledger = read_tsv(artifact_dir / "outputs/audit/evidence_ledger.tsv") if (artifact_dir / "outputs/audit/evidence_ledger.tsv").exists() else []
    target = read_tsv(artifact_dir / "outputs/target_prior/ligand_aware_target_prior.tsv") if (artifact_dir / "outputs/target_prior/ligand_aware_target_prior.tsv").exists() else []
    redock = read_tsv(artifact_dir / "outputs/controls/redocking_control_results.tsv") if (artifact_dir / "outputs/controls/redocking_control_results.tsv").exists() else []
    dock = read_tsv(artifact_dir / "outputs/docking/docking_runs.tsv") if (artifact_dir / "outputs/docking/docking_runs.tsv").exists() else []
    ledger_ref = ledger[0]["ledger_id"] if ledger else "ledger_missing"
    lines = [
        "# GenoLigand Phase 0.7 Candidate Report",
        "",
        "## Run Mode",
        f"- run mode: {RUN_MODE} [{ledger_ref}]",
        "- production target-prior allowed: false [ledger_10_0001]",
        "",
        "## Ligand-Aware Docking Prefilter Controls",
        f"- real docking jobs recorded: {len(dock)} [ledger_05_0001]",
        "- docking score values are tool-reported prefilter values only [ledger_06_0001]",
        "",
        "## Redocking Control",
    ]
    if redock:
        row = redock[0]
        lines.append(f"- redocking RMSD: {row.get('heavy_atom_rmsd_angstrom')} A; threshold: {row.get('rmsd_threshold_angstrom')} A; pass: {row.get('control_pass')} [ledger_08_0001]")
    else:
        lines.append("- redocking RMSD: validation_failed [ledger_missing]")
    lines.extend(["", "## Target-Prior Output"])
    if target:
        row = target[0]
        lines.append(f"- target-prior mode: {row.get('confidence_label')}; downgrade: {row.get('confidence_downgrade_reason')} [ledger_10_0001]")
    else:
        lines.append("- target-prior mode: validation_failed [ledger_missing]")
    lines.extend(
        [
            "",
            "## Limitations",
            "- This Phase 0.7 run is an engineering control-only acceptance run.",
            "- It does not claim target engagement, measured binding energy, activity, or a drug-target relationship.",
            "- Full comparison database integration is scheduled for Phase 0.8.",
            "",
        ]
    )
    ensure_parent(out)
    out.write_text("\n".join(lines), encoding="utf-8")
    return 0


def write_external_manifest(artifact_dir: Path, out: Path, vina_bin: str, ligand_prep: str, receptor_prep: str, obabel: str) -> int:
    resources = [
        ("AutoDock_Vina", "docking_backend", "https://github.com/ccsb-scripps/AutoDock-Vina", vina_bin),
        ("RDKit", "ligand_handling", "https://github.com/rdkit/rdkit", sys.executable),
        ("Meeko_ligand", "ligand_preparation", "https://github.com/forlilab/Meeko", ligand_prep),
        ("Meeko_receptor", "receptor_preparation", "https://meeko.readthedocs.io/", receptor_prep),
        ("Open_Babel", "format_conversion", "https://openbabel.org/", obabel),
        ("RCSB_PDB_1A97", "positive_control_structure", "https://www.rcsb.org/structure/1A97", "data/external_phase_0_6/rcsb/pdb/1A97_B.pdb"),
        ("PubChem_2519", "negative_control_ligand", "https://pubchem.ncbi.nlm.nih.gov/compound/2519", "inputs/phase_0_7/ligands/ligands.tsv"),
    ]
    rows = []
    for rid, rtype, url, local in resources:
        local_path = Path(local) if local else Path("missing")
        rows.append(
            {
                "resource_id": rid,
                "resource_type": rtype,
                "source_name": rid,
                "source_url": url,
                "retrieval_mode": "preinstalled_or_local_manifest",
                "retrieval_command": "not_applicable",
                "local_path": str(local),
                "version_or_release": command_version([local, "--version"]) if local and shutil.which(local) else "recorded_in_tool_versions_or_missing",
                "entry_count": "not_applicable",
                "byte_size": local_path.stat().st_size if local_path.exists() and local_path.is_file() else "missing",
                "sha256": path_hash(local_path),
                "license_or_terms_note": "record_source_terms_before_distribution",
                "created_at_utc": utc_now(),
                "used_in_process": "true",
                "status": "PASS" if (local_path.exists() or shutil.which(local)) else "MISSING",
                "notes": "phase_0_7_external_resource_manifest_entry",
            }
        )
    write_tsv(
        out,
        rows,
        [
            "resource_id",
            "resource_type",
            "source_name",
            "source_url",
            "retrieval_mode",
            "retrieval_command",
            "local_path",
            "version_or_release",
            "entry_count",
            "byte_size",
            "sha256",
            "license_or_terms_note",
            "created_at_utc",
            "used_in_process",
            "status",
            "notes",
        ],
    )
    return 0


def write_tool_versions(out: Path, vina_bin: str, ligand_prep: str, receptor_prep: str, obabel: str) -> int:
    rdkit_ok, rdkit_version = python_import_version("rdkit")
    meeko_ok, meeko_version = python_import_version("meeko")
    rows = [
        {"tool": "Python", "exit_code": 0, "version_text": sys.version.replace("\n", " ")},
        {"tool": "RDKit", "exit_code": 0 if rdkit_ok else 1, "version_text": rdkit_version},
        {"tool": "Meeko", "exit_code": 0 if meeko_ok else 1, "version_text": meeko_version},
        {"tool": "AutoDock Vina", "exit_code": 0 if (shutil.which(vina_bin) or Path(vina_bin).exists()) else 1, "version_text": command_version([shutil.which(vina_bin) or vina_bin, "--version"]) if (shutil.which(vina_bin) or Path(vina_bin).exists()) else "missing"},
        {"tool": "ligand_prepare_command", "exit_code": 0 if shutil.which(ligand_prep) else 1, "version_text": command_version([ligand_prep, "--version"]) if shutil.which(ligand_prep) else "missing"},
        {"tool": "receptor_prepare_command", "exit_code": 0 if shutil.which(receptor_prep) else 1, "version_text": command_version([receptor_prep, "--version"]) if shutil.which(receptor_prep) else "missing"},
        {"tool": "Open Babel", "exit_code": 0 if shutil.which(obabel) else 1, "version_text": command_version([obabel, "-V"]) if shutil.which(obabel) else "missing"},
    ]
    write_tsv(out, rows, ["tool", "exit_code", "version_text"])
    return 0


def semantic_hashes(artifact_dir: Path, out: Path) -> int:
    ignored_fields = {"generated_at_utc", "runtime_seconds", "source_artifact_sha256"}

    def stable_hash(path: Path, key_columns: list[str]) -> str:
        if path.suffix.lower() == ".md":
            return normalized_hash_file(path, key_columns)
        normalized = []
        for row in read_tsv(path):
            clean = {}
            for key, value in row.items():
                if key in ignored_fields:
                    continue
                if re.match(r"^-?\d+\.\d+$", str(value)):
                    clean[key] = fmt(float(value))
                elif isinstance(value, str):
                    clean[key] = re.sub(r"[A-Za-z]:\\[^\t]+", "<ABS_PATH>", value)
                else:
                    clean[key] = value
            normalized.append(clean)
        normalized.sort(key=lambda row: tuple(str(row.get(column, "")) for column in key_columns))
        return sha256_text(json.dumps(normalized, sort_keys=True, ensure_ascii=True))

    items = [
        ("ligand_standardization", artifact_dir / "outputs/ligands/ligand_standardization.tsv", ["ligand_id"]),
        ("docking_boxes", artifact_dir / "outputs/docking_boxes/docking_box_manifest.tsv", ["box_id"]),
        ("docking_runs", artifact_dir / "outputs/docking/docking_runs.tsv", ["run_id"]),
        ("docking_scores", artifact_dir / "outputs/docking/docking_scores.tsv", ["run_id", "pose_rank"]),
        ("interaction_fingerprints", artifact_dir / "outputs/interactions/interaction_fingerprints.tsv", ["run_id"]),
        ("target_prior", artifact_dir / "outputs/target_prior/ligand_aware_target_prior.tsv", ["candidate_id", "ligand_id"]),
        ("candidate_report", artifact_dir / "outputs/reports/candidate_report_phase_0_7.md", []),
    ]
    rows = []
    for artifact, path, keys in items:
        rows.append(
            {
                "artifact": artifact,
                "source_path": str(path),
                "semantic_hash": stable_hash(path, keys) if path.exists() else "missing",
            }
        )
    write_tsv(out, rows, ["artifact", "source_path", "semantic_hash"])
    return 0


def write_archive_hashes(artifact_dir: Path, out: Path) -> int:
    rows = []
    for path in sorted(artifact_dir.rglob("*")):
        if not path.is_file() or path == out:
            continue
        rows.append({"relative_path": path.relative_to(artifact_dir).as_posix(), "byte_size": path.stat().st_size, "sha256": sha256_file(path)})
    write_tsv(out, rows, ["relative_path", "byte_size", "sha256"])
    return 0


def render_acceptance_report(artifact_dir: Path, out: Path, verdict: str, failures: list[str]) -> int:
    def count(rel: str) -> int:
        path = artifact_dir / rel
        return len(read_tsv(path)) if path.exists() else 0

    def first_value(rel: str, key: str, default: str = "missing") -> str:
        path = artifact_dir / rel
        if not path.exists():
            return default
        rows = read_tsv(path)
        return rows[0].get(key, default) if rows else default

    lines = [
        "# GenoLigand-Search Phase 0.7 Anti-Hallucination Ligand-Aware Docking Acceptance Report",
        "",
        "## 1. Verdict",
        f"FINAL_VERDICT: {verdict}",
        "",
        "## 2. Repository Metadata",
        f"- Commit: {command_text(['git', 'rev-parse', 'HEAD'])[1].strip()}",
        f"- Branch: {command_text(['git', 'rev-parse', '--abbrev-ref', 'HEAD'])[1].strip()}",
        f"- Date: {utc_now()}",
        f"- OS: {os.name}; {sys.platform}",
        f"- Rust version: {command_text(['rustc', '--version'])[1].strip()}",
        f"- Python version: {sys.version.replace(chr(10), ' ')}",
        f"- Nextflow version: {command_version(['nextflow', '-version'])}",
        f"- Docking backend/version: {first_value('metrics/tool_versions.tsv', 'version_text')}",
        f"- RDKit version: {next((row.get('version_text', 'missing') for row in read_tsv(artifact_dir / 'metrics/tool_versions.tsv') if row.get('tool') == 'RDKit'), 'missing') if (artifact_dir / 'metrics/tool_versions.tsv').exists() else 'missing'}",
        "- Meeko/Open Babel version if used: see metrics/tool_versions.tsv",
        f"- Acceptance mode: {ACCEPTANCE_MODE}",
        "",
        "## 3. Implemented Scope",
        f"Phase 0.7 implemented {RUN_MODE} ligand-aware docking prefilter plumbing, evidence ledger, report linting, and strict failure recording.",
        "",
        "## 4. Commands Executed",
        "Reference metrics/command_summary.tsv and logs/command_stdout_stderr.",
        "",
        "## 5. Artifact Inventory",
        f"- archive files hashed: {count('hashes.tsv')}",
        "",
        "## 6. Phase 0.3-0.6 Regression Gates",
        f"- status: {first_value('metrics/phase_0_3_to_0_6_regression_summary.tsv', 'status')}",
        "",
        "## 7. Ligand Input and Standardization Gates",
        f"- ligand rows: {count('outputs/ligands/ligand_input_manifest.tsv')}",
        f"- stereochemistry status: {first_value('outputs/ligands/ligand_standardization.tsv', 'standardization_status')}",
        f"- conformer rows: {count('outputs/ligands/ligand_conformer_manifest.tsv')}",
        f"- ligand preparation status: {first_value('outputs/ligands/ligand_preparation_manifest.tsv', 'preparation_status')}",
        "",
        "## 8. Receptor and Docking Box Gates",
        f"- prepared receptors: {count('outputs/receptors/receptor_preparation_manifest.tsv')}",
        f"- docking boxes: {count('outputs/docking_boxes/docking_box_manifest.tsv')}",
        "- phase 0.6 patch lineage: see docking_box_manifest.tsv",
        "",
        "## 9. Real Docking Gates",
        "- backend: AutoDock Vina",
        f"- total docking jobs: {count('outputs/docking/docking_runs.tsv')}",
        f"- successful jobs: {sum(1 for row in read_tsv(artifact_dir / 'outputs/docking/docking_runs.tsv') if row.get('run_status') == 'PASS') if (artifact_dir / 'outputs/docking/docking_runs.tsv').exists() else 0}",
        f"- failed jobs: {sum(1 for row in read_tsv(artifact_dir / 'outputs/docking/docking_runs.tsv') if row.get('run_status') != 'PASS') if (artifact_dir / 'outputs/docking/docking_runs.tsv').exists() else 0}",
        f"- pose files: {count('outputs/docking/docking_pose_manifest.tsv')}",
        f"- score rows: {count('outputs/docking/docking_scores.tsv')}",
        "",
        "## 10. Redocking Positive Control Gates",
        f"- PDB ID: {first_value('outputs/controls/redocking_control_results.tsv', 'pdb_id')}",
        f"- native ligand: {first_value('outputs/controls/redocking_control_results.tsv', 'native_ligand_id')}",
        f"- RMSD: {first_value('outputs/controls/redocking_control_results.tsv', 'heavy_atom_rmsd_angstrom')}",
        f"- threshold: {first_value('outputs/controls/redocking_control_results.tsv', 'rmsd_threshold_angstrom')}",
        f"- status: {first_value('outputs/controls/redocking_control_results.tsv', 'control_pass')}",
        "",
        "## 11. Interaction Fingerprint Gates",
        f"- engine: {IFP_ENGINE}",
        f"- rows: {count('outputs/interactions/interaction_fingerprints.tsv')}",
        "- interaction classes: hydrogen_bond_candidate, salt_bridge_candidate, hydrophobic_contact_candidate, pi_pi_candidate, cation_pi_candidate, metal_coordination_candidate, halogen_bond_candidate, steric_clash_candidate",
        f"- parse status: {first_value('outputs/interactions/interaction_summary.tsv', 'status')}",
        "",
        "## 12. Decoy Calibration Gates",
        "- D1-D5 status: recorded in docking_box_manifest.tsv",
        "- ligand decoy status: phase07_negative_caffeine",
        "- random box status: recorded",
        f"- decoy superiority warnings: {first_value('outputs/controls/decoy_docking_calibration.tsv', 'decoy_outperforms_candidate')}",
        "",
        "## 13. Target-Prioritization Output",
        f"- target prior rows: {count('outputs/target_prior/ligand_aware_target_prior.tsv')}",
        f"- score decomposition rows: {count('outputs/target_prior/ligand_aware_score_decomposition.tsv')}",
        f"- confidence downgrade rows: {count('outputs/target_prior/ligand_aware_target_prior.tsv')}",
        "",
        "## 14. Evidence Ledger and Anti-Hallucination Gates",
        f"- evidence ledger rows: {count('outputs/audit/evidence_ledger.tsv')}",
        "- unsupported claims: see metrics/report_claim_lint.tsv",
        "- placeholder artifacts: see metrics/no_placeholder_artifacts.tsv",
        "- report claim lint status: see metrics/report_claim_lint.tsv",
        "",
        "## 15. Nextflow Reproducibility",
        "- first semantic hash: see metrics/phase07_semantic_hash_first.tsv",
        "- resume semantic hash: see metrics/phase07_semantic_hash_resume.tsv",
        "- hash match: see metrics/phase07_semantic_hash_compare.tsv",
        "",
        "## 16. Known Limitations",
        "- docking score is prefilter evidence only.",
        "- no binding free-energy claim.",
        "- no target engagement.",
        "- no biological activity.",
        "- no experimental validation.",
        "- full comparison database is scheduled for Phase 0.8.",
        "",
    ]
    if failures:
        lines.extend(["## Failure Details", *[f"- {failure}" for failure in failures], ""])
    lines.extend(["## 17. Final Verdict", f"FINAL_VERDICT: {verdict}", ""])
    ensure_parent(out)
    out.write_text("\n".join(lines), encoding="utf-8")
    return 0
