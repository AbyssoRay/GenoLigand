from __future__ import annotations

from dataclasses import dataclass


SCHEMA_VERSION = "genoligand_md_phase_0_8_4_v1"


@dataclass(frozen=True)
class AdapterEvidence:
    adapter_name: str
    adapter_version: str
    tool_name: str
    tool_version: str
    tool_license: str
    adapter_type: str
    input_schema_version: str
    output_schema_version: str
    validation_status: str
    notes: str = ""


COMPLEX_MANIFEST_COLUMNS = [
    "md_complex_id",
    "complex_class",
    "ligand_id",
    "receptor_id",
    "source_phase",
    "source_database",
    "source_structure_path",
    "source_pose_path",
    "source_box_id",
    "source_score",
    "source_patch_id",
    "source_uniprot_accession",
    "source_pdb_id",
    "source_chain_id",
    "selected_for_md",
    "selection_reason",
    "required_for_pass",
    "notes",
]


LIGAND_PARAMETERIZATION_COLUMNS = [
    "md_complex_id",
    "ligand_id",
    "canonical_smiles",
    "standard_inchikey",
    "formal_charge",
    "stereo_status",
    "parameterization_route",
    "force_field_name",
    "force_field_version",
    "partial_charge_method",
    "parameterization_status",
    "failure_reason",
    "warnings",
]
