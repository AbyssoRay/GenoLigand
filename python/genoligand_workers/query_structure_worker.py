from __future__ import annotations

import csv
import hashlib
import json
import math
import os
import re
import shutil
import tempfile
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from genoligand_workers.common import WorkerExecution, WorkerOutcome
from genoligand_workers.worker_io import (
    parse_worker_args,
    read_job,
    runtime_artifact_path,
    write_result,
)


WORKER_NAME = "query_structure_prepare"
METHOD_VERSION = "genoligand_query_structure_prepare_v1"
FEATURE_SCHEMA_VERSION = "genoligand_query_structure_features_v1"
ARTIFACT_SCHEMA_VERSION = "genoligand_query_structure_artifact_v1"
INDEX_HEADER = (
    "input_id\trecord_id\tinput_kind\tinput_digest\tmethod\tfeatures_json\t"
    "coordinate_artifact\tfeature_digest\tstatus\n"
)
SMALL_MOLECULE_CONFORMERS = 5
PEPTIDE_CONFORMERS = 3
PEPTIDE_CONFORMER_MAX_RESIDUES = 32
MORGAN_RADIUS = 2
MORGAN_BITS = 2048
TAUTOMER_MAX_TAUTOMERS = 100
TAUTOMER_MAX_TRANSFORMS = 100
MAX_FEATURE_BYTES = 256 * 1024
MAX_COORDINATE_BYTES = 2 * 1024 * 1024
MAX_STRUCTURE_SOURCE_BYTES = 128 * 1024 * 1024
MAX_MMCIF_SOURCE_BYTES = 64 * 1024 * 1024
MAX_STRUCTURE_ATOMS = 500_000
MAX_STRUCTURE_RESIDUES = 200_000
MAX_PHARMACOPHORE_POINTS = 64
STANDARD_AMINO_ACIDS = set("ACDEFGHIKLMNPQRSTVWY")
STRUCTURE_FORMATS = {"pdb", "ent", "cif", "mmcif"}
SPOOL_PATTERN = re.compile(r"^([^#]+)#offset=(\d+)&length=(\d+)$")
HYDROPATHY = {
    "A": 1.8,
    "C": 2.5,
    "D": -3.5,
    "E": -3.5,
    "F": 2.8,
    "G": -0.4,
    "H": -3.2,
    "I": 4.5,
    "K": -3.9,
    "L": 3.8,
    "M": 1.9,
    "N": -3.5,
    "P": -1.6,
    "Q": -3.5,
    "R": -4.5,
    "S": -0.8,
    "T": -0.7,
    "V": 4.2,
    "W": -0.9,
    "Y": -1.3,
}
CHOU_FASMAN_ALPHA = dict(
    zip(
        "ACDEFGHIKLMNPQRSTVWY",
        (1.42, 0.70, 1.01, 1.51, 1.13, 0.57, 1.00, 1.08, 1.16, 1.21,
         1.45, 0.67, 0.57, 1.11, 0.98, 0.77, 0.83, 1.06, 1.08, 0.69),
        strict=True,
    )
)
CHOU_FASMAN_BETA = dict(
    zip(
        "ACDEFGHIKLMNPQRSTVWY",
        (0.83, 1.19, 0.54, 0.37, 1.38, 0.75, 0.87, 1.60, 0.74, 1.30,
         1.05, 0.89, 0.55, 1.10, 0.93, 0.75, 1.19, 1.70, 1.37, 1.47),
        strict=True,
    )
)
CHOU_FASMAN_TURN = dict(
    zip(
        "ACDEFGHIKLMNPQRSTVWY",
        (0.66, 1.19, 1.46, 0.74, 0.60, 1.56, 0.95, 0.47, 1.01, 0.59,
         0.60, 1.56, 1.52, 0.98, 0.95, 1.43, 0.96, 0.50, 0.96, 1.14),
        strict=True,
    )
)


class QueryStructureError(ValueError):
    def __init__(self, code: str, detail: str = "") -> None:
        super().__init__(code)
        self.code = code
        self.detail = _safe_text(detail)


@dataclass(frozen=True)
class InputRecord:
    input_id: str
    record_id: str
    input_kind: str
    canonical_value: str
    source_digest: str
    input_digest: str
    source_kind: str
    source_path: str
    artifact: str
    input_format: str
    declared_atom_count: int
    declared_chain_count: int
    declared_residue_count: int


@dataclass(frozen=True)
class PublishedArtifacts:
    manifest: Path
    features: Path | None
    coordinate: str
    feature_digest: str
    status: str


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    execution = run_query_structure_job(job)
    write_worker_result_atomic(args.out, job, execution)
    return 0 if execution.status in {"completed", "not_applicable"} else 2


def run_query_structure_job(job: dict[str, Any]) -> WorkerExecution:
    record: InputRecord | None = None
    try:
        _validate_job(job)
        run_dir = _resolve_run_dir(job)
        record = _selected_input_record(run_dir, job)
        _validate_record_binding(record, job)
        if record.input_kind == "genome_or_metagenome":
            _publish_terminal_artifact(
                run_dir,
                job,
                record,
                "not_applicable",
                "nucleotide_input_has_no_direct_3d_query_structure",
            )
            return _execution(
                "not_applicable",
                "nucleotide_input_has_no_direct_3d_query_structure",
                f"{METHOD_VERSION}; status=not_applicable",
                False,
            )
        if record.input_kind == "complex" and record.input_format not in STRUCTURE_FORMATS:
            _publish_terminal_artifact(
                run_dir,
                job,
                record,
                "not_applicable",
                "coordinate_required_for_direct_complex_structure",
            )
            return _execution(
                "not_applicable",
                "coordinate_required_for_direct_complex_structure",
                f"{METHOD_VERSION}; status=not_applicable",
                False,
            )
        _prepare_and_publish(run_dir, job, record)
        return _execution(
            "completed",
            "query_structure_prepared",
            _engine_version(record),
            True,
            1,
        )
    except QueryStructureError as exc:
        _try_publish_failure(job, record, exc)
        return _execution(
            "failed",
            exc.code,
            f"{METHOD_VERSION}; status=failed; reason={exc.code}",
            True,
        )
    except Exception as exc:  # pragma: no cover - defensive worker boundary
        error = QueryStructureError("query_structure_worker_failed", str(exc))
        _try_publish_failure(job, record, error)
        return _execution(
            "failed",
            error.code,
            f"{METHOD_VERSION}; status=failed; reason={error.code}",
            True,
        )


def write_worker_result_atomic(
    path: str | Path,
    job: dict[str, Any],
    execution: WorkerExecution,
) -> None:
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.{os.getpid()}.tmp")
    try:
        write_result(
            temporary,
            job,
            execution.result_status,
            execution.engine_version,
            execution_status=execution.status,
            reason=execution.reason,
            outcomes=execution.outcome_dicts(),
        )
        result = json.loads(temporary.read_text(encoding="utf-8"))
        published = _find_published_artifacts(job)
        if published is not None:
            run_dir = _resolve_run_dir(job)
            result["manifest"] = _artifact_reference(run_dir, published.manifest)
            if published.features is not None:
                result["artifact_path"] = _artifact_reference(run_dir, published.features)
            manifest_value = _read_json(published.manifest)
            source_digest = str(manifest_value.get("source_content_sha256", ""))
            if valid_sha256(source_digest):
                result["source_digest"] = source_digest
            input_manifest = runtime_artifact_path(str(job.get("input_manifest", "")))
            if input_manifest.is_file():
                result["input_manifest_sha256"] = sha256_file(input_manifest)
        _write_json_atomic(output, result)
    finally:
        temporary.unlink(missing_ok=True)


def query_structure_input_digest(
    input_id: str,
    record_id: str,
    input_kind: str,
    canonical_value: str,
    source_digest: str,
) -> str:
    return canonical_digest(
        {
            "input_id": input_id,
            "record_id": record_id,
            "canonical_kind": input_kind,
            "canonical_value": canonical_value,
            "source_content_sha256": source_digest,
        }
    )


def canonical_digest(value: Any) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def valid_sha256(value: str) -> bool:
    return len(value) == 64 and all(ch in "0123456789abcdef" for ch in value)


def _execution(
    status: str,
    reason: str,
    version: str,
    required: bool,
    row_count: int = 0,
) -> WorkerExecution:
    return WorkerExecution(
        status,
        reason,
        version,
        (WorkerOutcome(status, reason, required, WORKER_NAME, row_count),),
    )


def _validate_job(job: dict[str, Any]) -> None:
    if str(job.get("worker", "")) != WORKER_NAME:
        raise QueryStructureError("query_structure_worker_name_mismatch")
    for field in ("job_id", "run_id", "invocation_id", "input_id", "record_id", "input_kind"):
        if not str(job.get(field, "")).strip():
            raise QueryStructureError(f"query_structure_{field}_missing")
    input_digest = str(job.get("input_digest", "")).strip().lower()
    if not valid_sha256(input_digest):
        raise QueryStructureError("query_structure_input_digest_invalid")


def _resolve_run_dir(job: dict[str, Any]) -> Path:
    value = str(job.get("run_dir", "")).strip()
    if not value:
        raise QueryStructureError("query_structure_run_dir_missing")
    try:
        run_dir = runtime_artifact_path(value).resolve(strict=True)
    except OSError as exc:
        raise QueryStructureError("query_structure_run_dir_unavailable", str(exc)) from exc
    if not run_dir.is_dir():
        raise QueryStructureError("query_structure_run_dir_not_directory")
    return run_dir


def _selected_input_record(run_dir: Path, job: dict[str, Any]) -> InputRecord:
    path = run_dir / "input_conversion_manifest.tsv"
    if not path.is_file():
        raise QueryStructureError("query_structure_conversion_manifest_missing")
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        required = {
            "input_id",
            "record_id",
            "canonical_kind",
            "canonical_value",
            "conversion_status",
            "format",
            "source_kind",
            "source_path",
            "content_sha256",
            "artifact",
        }
        if reader.fieldnames is None or not required.issubset(reader.fieldnames):
            raise QueryStructureError("query_structure_conversion_manifest_schema_invalid")
        rows = [
            row
            for row in reader
            if row.get("input_id") == str(job["input_id"])
            and row.get("record_id") == str(job["record_id"])
            and row.get("conversion_status") == "converted"
        ]
    if len(rows) != 1:
        raise QueryStructureError("query_structure_input_record_not_unique")
    row = rows[0]
    input_kind = str(row.get("canonical_kind", "")).strip()
    source_digest = str(row.get("content_sha256", "")).strip().lower()
    if not valid_sha256(source_digest):
        raise QueryStructureError("query_structure_source_digest_invalid")
    canonical_value = _record_value(run_dir, row, source_digest)
    input_digest = query_structure_input_digest(
        str(job["input_id"]),
        str(job["record_id"]),
        input_kind,
        canonical_value,
        source_digest,
    )
    return InputRecord(
        input_id=str(job["input_id"]),
        record_id=str(job["record_id"]),
        input_kind=input_kind,
        canonical_value=canonical_value,
        source_digest=source_digest,
        input_digest=input_digest,
        source_kind=str(row.get("source_kind", "")).strip(),
        source_path=decode_tsv_cell(str(row.get("source_path", ""))).strip(),
        artifact=decode_tsv_cell(str(row.get("artifact", ""))).strip(),
        input_format=str(row.get("format", "")).strip().lower(),
        declared_atom_count=_nonnegative_int(row.get("structure_atom_count", "0")),
        declared_chain_count=_nonnegative_int(row.get("structure_chain_count", "0")),
        declared_residue_count=_nonnegative_int(row.get("structure_residue_count", "0")),
    )


def _validate_record_binding(record: InputRecord, job: dict[str, Any]) -> None:
    if record.input_kind != str(job["input_kind"]).strip():
        raise QueryStructureError("query_structure_input_kind_mismatch")
    for field in ("source_digest", "content_sha256"):
        expected = str(job.get(field, "")).strip().lower()
        if expected and expected != record.source_digest:
            raise QueryStructureError("query_structure_source_digest_mismatch")
    if record.input_digest != str(job["input_digest"]).strip().lower():
        raise QueryStructureError("query_structure_input_digest_mismatch")


def _record_value(run_dir: Path, row: dict[str, str], source_digest: str) -> str:
    value = decode_tsv_cell(str(row.get("canonical_value", ""))).strip()
    artifact = decode_tsv_cell(str(row.get("artifact", ""))).strip()
    if value:
        return value
    match = SPOOL_PATTERN.fullmatch(artifact)
    if match is None:
        raise QueryStructureError("query_structure_input_value_missing")
    spool = _resolve_run_artifact(run_dir, match.group(1))
    offset = int(match.group(2))
    length = int(match.group(3))
    if length <= 0:
        raise QueryStructureError("query_structure_spool_length_invalid")
    with spool.open("rb") as handle:
        handle.seek(offset)
        data = handle.read(length)
    if len(data) != length:
        raise QueryStructureError("query_structure_spool_truncated")
    if hashlib.sha256(data).hexdigest() != source_digest:
        raise QueryStructureError("query_structure_spool_digest_mismatch")
    try:
        return data.decode("utf-8").strip()
    except UnicodeDecodeError as exc:
        raise QueryStructureError("query_structure_spool_utf8_invalid") from exc


def _prepare_and_publish(run_dir: Path, job: dict[str, Any], record: InputRecord) -> PublishedArtifacts:
    output_dir = _entry_output_dir(run_dir, job, record)
    _recover_missing_output_dir(output_dir)
    existing = _valid_existing_artifact(run_dir, job, record, output_dir)
    if existing is not None:
        _remove_backup_directory(output_dir)
        _upsert_index(run_dir, record, existing)
        return existing
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=f".{output_dir.name}.", dir=output_dir.parent))
    conversion_manifest = run_dir / "input_conversion_manifest.tsv"
    conversion_digest = sha256_file(conversion_manifest)
    try:
        if record.input_format in STRUCTURE_FORMATS:
            features, coordinate = _prepare_existing_structure(run_dir, record)
            coordinate_path = coordinate
        elif record.input_kind == "small_molecule":
            features, coordinate_name = _prepare_small_molecule(record, temporary)
            coordinate_path = _artifact_reference(run_dir, output_dir / coordinate_name)
        elif record.input_kind == "peptide":
            peptide_length = len("".join(record.canonical_value.split()))
            if peptide_length <= PEPTIDE_CONFORMER_MAX_RESIDUES:
                features, coordinate_name = _prepare_peptide(record, temporary)
                coordinate_path = _artifact_reference(run_dir, output_dir / coordinate_name)
            else:
                features = _prepare_protein_sequence_prior(record)
                coordinate_path = ""
        elif record.input_kind == "protein_ligand":
            features = _prepare_protein_sequence_prior(record)
            coordinate_path = ""
        else:
            raise QueryStructureError("query_structure_input_kind_not_supported")

        if sha256_file(conversion_manifest) != conversion_digest:
            raise QueryStructureError("query_structure_conversion_manifest_changed")

        coordinate_digest = ""
        coordinate_bytes = 0
        if coordinate_path:
            coordinate_candidate = temporary / Path(coordinate_path).name
            if not coordinate_candidate.is_file():
                coordinate_candidate = runtime_artifact_path(coordinate_path)
                if not coordinate_candidate.is_absolute():
                    coordinate_candidate = run_dir / coordinate_candidate
            coordinate_candidate = coordinate_candidate.resolve(strict=True)
            coordinate_digest = sha256_file(coordinate_candidate)
            coordinate_bytes = coordinate_candidate.stat().st_size

        features_path = temporary / "features.json"
        features_payload = {
            "schema_version": FEATURE_SCHEMA_VERSION,
            "status": "ready",
            "run_id": str(job["run_id"]),
            "job_id": str(job["job_id"]),
            "invocation_id": str(job["invocation_id"]),
            "input_id": record.input_id,
            "record_id": record.record_id,
            "input_kind": record.input_kind,
            "input_digest": record.input_digest,
            "source_content_sha256": record.source_digest,
            "conversion_manifest_sha256": conversion_digest,
            "method_version": METHOD_VERSION,
            "coordinate_artifact": {
                "path": coordinate_path,
                "sha256": coordinate_digest,
                "size_bytes": coordinate_bytes,
                "retention": "single_representative_or_original_input",
            },
            **features,
        }
        _write_json_file(features_path, features_payload)
        if features_path.stat().st_size > MAX_FEATURE_BYTES:
            raise QueryStructureError("query_structure_features_too_large")
        feature_digest = sha256_file(features_path)
        if coordinate_path:
            coordinate_local = temporary / Path(coordinate_path).name
            if coordinate_local.is_file() and coordinate_local.stat().st_size > MAX_COORDINATE_BYTES:
                raise QueryStructureError("query_structure_coordinate_too_large")
        manifest_payload = {
            "schema_version": ARTIFACT_SCHEMA_VERSION,
            "status": "ready",
            "run_id": str(job["run_id"]),
            "job_id": str(job["job_id"]),
            "invocation_id": str(job["invocation_id"]),
            "input_id": record.input_id,
            "record_id": record.record_id,
            "input_kind": record.input_kind,
            "input_digest": record.input_digest,
            "source_content_sha256": record.source_digest,
            "conversion_manifest_sha256": conversion_digest,
            "method_version": METHOD_VERSION,
            "features_json": _artifact_reference(run_dir, output_dir / "features.json"),
            "feature_digest": feature_digest,
            "coordinate_artifact": coordinate_path,
            "coordinate_sha256": coordinate_digest,
            "coordinate_size_bytes": coordinate_bytes,
            "retention": (
                "original_input_reference_only"
                if record.input_format in STRUCTURE_FORMATS
                else "single_representative_only"
                if coordinate_path
                else "no_coordinate_artifact"
            ),
        }
        _write_json_file(temporary / "manifest.json", manifest_payload)
        if sha256_file(conversion_manifest) != conversion_digest:
            raise QueryStructureError("query_structure_conversion_manifest_changed")
        _replace_directory(temporary, output_dir)
        published = PublishedArtifacts(
            output_dir / "manifest.json",
            output_dir / "features.json",
            coordinate_path,
            feature_digest,
            "ready",
        )
        _upsert_index(run_dir, record, published)
        return published
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def _prepare_small_molecule(record: InputRecord, output_dir: Path) -> tuple[dict[str, Any], str]:
    from rdkit import Chem, DataStructs, RDConfig, rdBase
    from rdkit.Chem import AllChem, ChemicalFeatures, Crippen, Descriptors, Lipinski, QED, rdMolDescriptors

    value = record.canonical_value
    source_molecule = (
        Chem.MolFromInchi(value) if value.startswith("InChI=") else Chem.MolFromSmiles(value)
    )
    if source_molecule is None:
        raise QueryStructureError("query_structure_small_molecule_parse_failed")
    molecule, standardization = _standardize_small_molecule(source_molecule, Chem)
    if molecule.GetNumHeavyAtoms() == 0 or molecule.GetNumHeavyAtoms() > 256:
        raise QueryStructureError("query_structure_small_molecule_size_invalid")
    canonical_smiles = Chem.MolToSmiles(molecule, canonical=True, isomericSmiles=False)
    isomeric_smiles = Chem.MolToSmiles(molecule, canonical=True, isomericSmiles=True)
    seed_digest = canonical_digest(
        {
            "selected_isomeric_smiles": isomeric_smiles,
            "standardization_method": standardization["method"],
            "conformer_method": "ETKDGv3",
            "method_version": METHOD_VERSION,
        }
    )
    standardization["conformer_seed_digest"] = seed_digest
    fingerprint = _morgan_fingerprint(molecule, DataStructs)
    embedded, ensemble = _conformer_ensemble(
        molecule,
        SMALL_MOLECULE_CONFORMERS,
        _seed(seed_digest),
        Chem,
        AllChem,
    )
    representative = int(ensemble["representative_conformer_id"])
    coordinate_name = "representative.sdf"
    _write_representative_sdf(output_dir / coordinate_name, embedded, representative, record, Chem)
    pharmacophore = _pharmacophore_summary(
        embedded,
        representative,
        ChemicalFeatures,
        Path(RDConfig.RDDataDir) / "BaseFeatures.fdef",
    )
    formal_charge = sum(atom.GetFormalCharge() for atom in molecule.GetAtoms())
    return {
        "canonical_smiles": canonical_smiles,
        "isomeric_smiles": isomeric_smiles,
        "standardization": standardization,
        "fingerprint": fingerprint,
        "physicochemical": {
            "molecular_weight": _round(Descriptors.MolWt(molecule)),
            "exact_molecular_weight": _round(Descriptors.ExactMolWt(molecule)),
            "logp": _round(Crippen.MolLogP(molecule)),
            "topological_polar_surface_area": _round(rdMolDescriptors.CalcTPSA(molecule)),
            "hydrogen_bond_donors": int(Lipinski.NumHDonors(molecule)),
            "hydrogen_bond_acceptors": int(Lipinski.NumHAcceptors(molecule)),
            "rotatable_bonds": int(Lipinski.NumRotatableBonds(molecule)),
            "ring_count": int(rdMolDescriptors.CalcNumRings(molecule)),
            "aromatic_ring_count": int(rdMolDescriptors.CalcNumAromaticRings(molecule)),
            "fraction_csp3": _round(rdMolDescriptors.CalcFractionCSP3(molecule)),
            "formal_charge": formal_charge,
            "heavy_atom_count": molecule.GetNumHeavyAtoms(),
            "qed": _round(QED.qed(molecule)),
        },
        "pharmacophore": pharmacophore,
        "conformers": ensemble,
        "engine_versions": {"rdkit": rdBase.rdkitVersion},
    }, coordinate_name


def _prepare_peptide(record: InputRecord, output_dir: Path) -> tuple[dict[str, Any], str]:
    from rdkit import Chem, RDConfig, rdBase
    from rdkit.Chem import AllChem, ChemicalFeatures, Descriptors

    sequence = "".join(record.canonical_value.split()).upper()
    if len(sequence) < 2 or any(residue not in STANDARD_AMINO_ACIDS for residue in sequence):
        raise QueryStructureError("query_structure_peptide_sequence_invalid")
    molecule = Chem.MolFromSequence(sequence)
    if molecule is None:
        raise QueryStructureError("query_structure_peptide_build_failed")
    embedded, ensemble = _conformer_ensemble(
        molecule,
        PEPTIDE_CONFORMERS,
        _seed(record.input_digest),
        Chem,
        AllChem,
    )
    representative = int(ensemble["representative_conformer_id"])
    coordinate_name = "representative.pdb"
    _write_representative_pdb(output_dir / coordinate_name, embedded, representative, Chem)
    composition = Counter(sequence)
    hydropathy = sum(HYDROPATHY[residue] for residue in sequence) / len(sequence)
    hydrophobic = sum(composition[residue] for residue in "AILMFWVY") / len(sequence)
    pharmacophore = _pharmacophore_summary(
        embedded,
        representative,
        ChemicalFeatures,
        Path(RDConfig.RDDataDir) / "BaseFeatures.fdef",
    )
    return {
        "sequence": sequence,
        "sequence_features": {
            "length": len(sequence),
            "composition": dict(sorted(composition.items())),
            "estimated_net_charge_ph7": _round(_peptide_charge(sequence, 7.0)),
            "mean_kyte_doolittle_hydropathy": _round(hydropathy),
            "hydrophobic_fraction": _round(hydrophobic),
            "molecular_weight": _round(Descriptors.MolWt(molecule)),
        },
        "pharmacophore": pharmacophore,
        "conformers": ensemble,
        "engine_versions": {"rdkit": rdBase.rdkitVersion},
    }, coordinate_name


def _prepare_protein_sequence_prior(record: InputRecord) -> dict[str, Any]:
    sequence = "".join(record.canonical_value.split()).upper()
    if len(sequence) < 2 or any(residue not in STANDARD_AMINO_ACIDS for residue in sequence):
        raise QueryStructureError("query_structure_protein_sequence_invalid")
    composition = Counter(sequence)
    length = len(sequence)
    hydropathy = sum(HYDROPATHY[residue] for residue in sequence) / length
    hydrophobic = sum(composition[residue] for residue in "AILMFWVY") / length
    propensity = {
        "alpha_helix": sum(CHOU_FASMAN_ALPHA[residue] for residue in sequence) / length,
        "beta_sheet": sum(CHOU_FASMAN_BETA[residue] for residue in sequence) / length,
        "turn": sum(CHOU_FASMAN_TURN[residue] for residue in sequence) / length,
    }
    return {
        "sequence_sha256": hashlib.sha256(sequence.encode("ascii")).hexdigest(),
        "sequence_features": {
            "length": length,
            "composition": dict(sorted(composition.items())),
            "estimated_net_charge_ph7": _round(_peptide_charge(sequence, 7.0)),
            "mean_kyte_doolittle_hydropathy": _round(hydropathy),
            "hydrophobic_fraction": _round(hydrophobic),
            "secondary_structure_propensity": {
                "method": "chou_fasman_mean_v1",
                **{name: _round(value) for name, value in propensity.items()},
            },
        },
        "structure_prior": {
            "method": "sequence_descriptors_only",
            "coordinate_generated": False,
            "reason": (
                "peptide_conformer_limit_exceeded"
                if record.input_kind == "peptide"
                else "large_protein_3d_embedding_deferred_until_candidate_structure_stage"
            ),
        },
        "engine_versions": {"descriptor_method": METHOD_VERSION},
    }


def _prepare_existing_structure(run_dir: Path, record: InputRecord) -> tuple[dict[str, Any], str]:
    from openmm import unit
    from openmm.app import PDBFile, PDBxFile
    import openmm

    coordinate = _structure_path(run_dir, record)
    if coordinate.stat().st_size > MAX_STRUCTURE_SOURCE_BYTES:
        raise QueryStructureError("query_structure_coordinate_source_too_large")
    if (
        record.input_format in {"cif", "mmcif"}
        and coordinate.stat().st_size > MAX_MMCIF_SOURCE_BYTES
    ):
        raise QueryStructureError("query_structure_mmcif_source_too_large")
    if record.declared_atom_count > MAX_STRUCTURE_ATOMS:
        raise QueryStructureError("query_structure_coordinate_atom_limit_exceeded")
    if record.declared_residue_count > MAX_STRUCTURE_RESIDUES:
        raise QueryStructureError("query_structure_coordinate_residue_limit_exceeded")
    if record.input_format in {"pdb", "ent"}:
        _preflight_pdb_limits(coordinate)
    if sha256_file(coordinate) != record.source_digest:
        raise QueryStructureError("query_structure_coordinate_digest_mismatch")
    try:
        structure = (
            PDBFile(str(coordinate))
            if record.input_format in {"pdb", "ent"}
            else PDBxFile(str(coordinate))
        )
        atoms = list(structure.topology.atoms())
        chains = list(structure.topology.chains())
        residues = list(structure.topology.residues())
        vectors = structure.positions.value_in_unit(unit.angstrom)
        coordinates = [(float(row.x), float(row.y), float(row.z)) for row in vectors]
    except Exception as exc:
        raise QueryStructureError("query_structure_coordinate_parse_failed", str(exc)) from exc
    if not atoms or len(atoms) != len(coordinates):
        raise QueryStructureError("query_structure_coordinate_atoms_invalid")
    if len(atoms) > MAX_STRUCTURE_ATOMS:
        raise QueryStructureError("query_structure_coordinate_atom_limit_exceeded")
    if len(residues) > MAX_STRUCTURE_RESIDUES:
        raise QueryStructureError("query_structure_coordinate_residue_limit_exceeded")
    observed = (len(atoms), len(chains), len(residues))
    declared = (
        record.declared_atom_count,
        record.declared_chain_count,
        record.declared_residue_count,
    )
    for declared_value, observed_value in zip(declared, observed, strict=True):
        if declared_value and declared_value != observed_value:
            raise QueryStructureError("query_structure_coordinate_count_mismatch")
    centroid = tuple(sum(row[index] for row in coordinates) / len(coordinates) for index in range(3))
    masses = [
        float(atom.element.mass.value_in_unit(unit.dalton))
        if atom.element is not None
        else 1.0
        for atom in atoms
    ]
    total_mass = sum(masses)
    center_of_mass = tuple(
        sum(row[index] * mass for row, mass in zip(coordinates, masses, strict=True))
        / total_mass
        for index in range(3)
    )
    radius = math.sqrt(
        sum(
            mass * sum((row[index] - center_of_mass[index]) ** 2 for index in range(3))
            for row, mass in zip(coordinates, masses, strict=True)
        )
        / total_mass
    )
    minimum = tuple(min(row[index] for row in coordinates) for index in range(3))
    maximum = tuple(max(row[index] for row in coordinates) for index in range(3))
    elements = Counter(
        atom.element.symbol if atom.element is not None else "unknown" for atom in atoms
    )
    residue_names = Counter(residue.name for residue in residues)
    return {
        "structure": {
            "format": record.input_format,
            "atom_count": len(atoms),
            "heavy_atom_count": sum(
                1 for atom in atoms if atom.element is None or atom.element.symbol != "H"
            ),
            "chain_count": len(chains),
            "residue_count": len(residues),
            "element_counts": dict(sorted(elements.items())),
            "residue_counts": dict(sorted(residue_names.items())),
            "centroid_angstrom": [_round(value) for value in centroid],
            "center_of_mass_angstrom": [_round(value) for value in center_of_mass],
            "bounding_box_min_angstrom": [_round(value) for value in minimum],
            "bounding_box_max_angstrom": [_round(value) for value in maximum],
            "radius_of_gyration_angstrom": _round(radius),
            "coordinate_sha256": record.source_digest,
        },
        "engine_versions": {"openmm": str(openmm.__version__)},
    }, _artifact_reference(run_dir, coordinate)


def _preflight_pdb_limits(path: Path) -> None:
    atom_count = 0
    residue_count = 0
    previous_residue: tuple[str, str, str, str] | None = None
    try:
        with path.open("r", encoding="ascii", errors="replace") as handle:
            for line in handle:
                if not line.startswith(("ATOM  ", "HETATM")):
                    continue
                atom_count += 1
                if atom_count > MAX_STRUCTURE_ATOMS:
                    raise QueryStructureError("query_structure_coordinate_atom_limit_exceeded")
                residue = (line[21:22], line[22:26], line[26:27], line[17:20])
                if residue != previous_residue:
                    residue_count += 1
                    previous_residue = residue
                    if residue_count > MAX_STRUCTURE_RESIDUES:
                        raise QueryStructureError(
                            "query_structure_coordinate_residue_limit_exceeded"
                        )
    except OSError as exc:
        raise QueryStructureError("query_structure_coordinate_preflight_failed", str(exc)) from exc


def _standardize_small_molecule(molecule: Any, chem: Any) -> tuple[Any, dict[str, Any]]:
    from rdkit.Chem.MolStandardize import rdMolStandardize

    input_smiles = chem.MolToSmiles(molecule, canonical=True, isomericSmiles=True)
    input_fragments = len(chem.GetMolFrags(molecule))
    try:
        cleaned = rdMolStandardize.Cleanup(chem.Mol(molecule))
        parent = rdMolStandardize.FragmentParent(cleaned)
        reionized = rdMolStandardize.Reionizer().reionize(parent)
        tautomer_enumerator = rdMolStandardize.TautomerEnumerator()
        tautomer_enumerator.SetMaxTautomers(TAUTOMER_MAX_TAUTOMERS)
        tautomer_enumerator.SetMaxTransforms(TAUTOMER_MAX_TRANSFORMS)
        standardized = tautomer_enumerator.Canonicalize(reionized)
        chem.SanitizeMol(standardized)
    except Exception as exc:
        raise QueryStructureError("query_structure_standardization_failed", str(exc)) from exc
    if standardized.GetNumHeavyAtoms() == 0:
        raise QueryStructureError("query_structure_standardization_empty")
    output_smiles = chem.MolToSmiles(standardized, canonical=True, isomericSmiles=True)
    return standardized, {
        "method": "rdkit_cleanup_fragment_parent_reionize_canonical_tautomer_v1",
        "input_isomeric_smiles": input_smiles,
        "selected_isomeric_smiles": output_smiles,
        "input_fragment_count": input_fragments,
        "removed_fragment_count": max(0, input_fragments - 1),
        "tautomer_max_tautomers": TAUTOMER_MAX_TAUTOMERS,
        "tautomer_max_transforms": TAUTOMER_MAX_TRANSFORMS,
        "protonation_policy": "input_microstate_normalized_without_ph_specific_enumeration",
        "risk_flags": ["ph_specific_microstates_not_enumerated"],
    }


def _morgan_fingerprint(molecule: Any, data_structs: Any) -> dict[str, Any]:
    try:
        from rdkit.Chem import rdFingerprintGenerator

        generator = rdFingerprintGenerator.GetMorganGenerator(
            radius=MORGAN_RADIUS,
            fpSize=MORGAN_BITS,
            includeChirality=True,
        )
        fingerprint = generator.GetFingerprint(molecule)
    except (ImportError, AttributeError):  # pragma: no cover - older supported RDKit
        from rdkit.Chem import AllChem

        fingerprint = AllChem.GetMorganFingerprintAsBitVect(
            molecule,
            MORGAN_RADIUS,
            nBits=MORGAN_BITS,
            useChirality=True,
        )
    packed = bytes(data_structs.BitVectToBinaryText(fingerprint))
    return {
        "algorithm": "morgan_ecfp4",
        "radius": MORGAN_RADIUS,
        "n_bits": MORGAN_BITS,
        "include_chirality": True,
        "encoding": "rdkit_binary_text_hex_v1",
        "packed_hex": packed.hex(),
        "population_count": int(fingerprint.GetNumOnBits()),
        "packed_sha256": hashlib.sha256(packed).hexdigest(),
    }


def _conformer_ensemble(
    molecule: Any,
    requested: int,
    seed: int,
    chem: Any,
    all_chem: Any,
) -> tuple[Any, dict[str, Any]]:
    embedded = chem.AddHs(chem.Mol(molecule))
    parameters = all_chem.ETKDGv3()
    parameters.randomSeed = seed
    parameters.numThreads = 1
    parameters.pruneRmsThresh = -1.0
    conformer_ids = list(all_chem.EmbedMultipleConfs(embedded, numConfs=requested, params=parameters))
    if not conformer_ids:
        raise QueryStructureError("query_structure_conformer_embedding_failed")
    if all_chem.MMFFHasAllMoleculeParams(embedded):
        results = all_chem.MMFFOptimizeMoleculeConfs(
            embedded,
            numThreads=1,
            maxIters=500,
            mmffVariant="MMFF94s",
        )
        energy_method = "MMFF94s"
    elif all_chem.UFFHasAllMoleculeParams(embedded):
        results = all_chem.UFFOptimizeMoleculeConfs(embedded, numThreads=1, maxIters=500)
        energy_method = "UFF"
    else:
        raise QueryStructureError("query_structure_force_field_parameters_missing")
    entries = []
    for conformer_id, result in zip(conformer_ids, results, strict=True):
        status, energy = int(result[0]), float(result[1])
        if not math.isfinite(energy):
            raise QueryStructureError("query_structure_conformer_energy_invalid")
        entries.append(
            {
                "conformer_id": int(conformer_id),
                "optimization_status": status,
                "energy_kcal_mol": _round(energy),
            }
        )
    converged = [entry for entry in entries if entry["optimization_status"] == 0]
    if not converged:
        raise QueryStructureError("query_structure_conformer_optimization_not_converged")
    representative = min(
        converged,
        key=lambda row: (row["energy_kcal_mol"], row["conformer_id"]),
    )
    minimum_energy = float(representative["energy_kcal_mol"])
    for entry in entries:
        entry["relative_energy_kcal_mol"] = _round(
            float(entry["energy_kcal_mol"]) - minimum_energy
        )
    return embedded, {
        "method": "ETKDGv3",
        "random_seed": seed,
        "requested_count": requested,
        "generated_count": len(entries),
        "energy_method": energy_method,
        "representative_conformer_id": representative["conformer_id"],
        "retained_coordinate_count": 1,
        "entries": entries,
    }


def _pharmacophore_summary(
    molecule: Any,
    conformer_id: int,
    chemical_features: Any,
    definition: Path,
) -> dict[str, Any]:
    factory = chemical_features.BuildFeatureFactory(str(definition))
    features = factory.GetFeaturesForMol(molecule, confId=conformer_id)
    counts = Counter(feature.GetFamily() for feature in features)
    points = []
    conformer = molecule.GetConformer(conformer_id)
    for feature in features[:MAX_PHARMACOPHORE_POINTS]:
        atom_ids = tuple(int(value) for value in feature.GetAtomIds())
        positions = [conformer.GetAtomPosition(atom_id) for atom_id in atom_ids]
        points.append(
            {
                "family": feature.GetFamily(),
                "atom_ids": list(atom_ids),
                "center_angstrom": [
                    _round(sum(getattr(position, axis) for position in positions) / len(positions))
                    for axis in ("x", "y", "z")
                ],
            }
        )
    return {
        "counts": dict(sorted(counts.items())),
        "representative_points": points,
        "point_count": len(features),
        "points_truncated": len(features) > MAX_PHARMACOPHORE_POINTS,
    }


def _write_representative_sdf(
    path: Path,
    molecule: Any,
    conformer_id: int,
    record: InputRecord,
    chem: Any,
) -> None:
    molecule.SetProp("_Name", f"{record.input_id}:{record.record_id}")
    molecule.SetProp("GENOLIGAND_INPUT_DIGEST", record.input_digest)
    writer = chem.SDWriter(str(path))
    try:
        writer.write(molecule, confId=conformer_id)
    finally:
        writer.close()
    _fsync_file(path)


def _write_representative_pdb(path: Path, molecule: Any, conformer_id: int, chem: Any) -> None:
    path.write_text(chem.MolToPDBBlock(molecule, confId=conformer_id), encoding="utf-8")
    _fsync_file(path)


def _peptide_charge(sequence: str, ph: float) -> float:
    counts = Counter(sequence)
    positive = 1.0 / (1.0 + 10.0 ** (ph - 9.69))
    positive += counts["K"] / (1.0 + 10.0 ** (ph - 10.5))
    positive += counts["R"] / (1.0 + 10.0 ** (ph - 12.5))
    positive += counts["H"] / (1.0 + 10.0 ** (ph - 6.0))
    negative = 1.0 / (1.0 + 10.0 ** (2.34 - ph))
    negative += counts["D"] / (1.0 + 10.0 ** (3.86 - ph))
    negative += counts["E"] / (1.0 + 10.0 ** (4.25 - ph))
    negative += counts["C"] / (1.0 + 10.0 ** (8.33 - ph))
    negative += counts["Y"] / (1.0 + 10.0 ** (10.07 - ph))
    return positive - negative


def _structure_path(run_dir: Path, record: InputRecord) -> Path:
    if record.artifact and not SPOOL_PATTERN.fullmatch(record.artifact):
        candidate = runtime_artifact_path(record.artifact)
        if not candidate.is_absolute():
            candidate = run_dir / candidate
    elif record.source_path:
        candidate = runtime_artifact_path(record.source_path)
    else:
        raise QueryStructureError("query_structure_coordinate_path_missing")
    try:
        path = candidate.resolve(strict=True)
    except OSError as exc:
        raise QueryStructureError("query_structure_coordinate_path_unavailable", str(exc)) from exc
    if not path.is_file():
        raise QueryStructureError("query_structure_coordinate_path_not_file")
    return path


def _entry_output_dir(run_dir: Path, job: dict[str, Any], record: InputRecord) -> Path:
    input_key = hashlib.sha256(record.input_id.encode("utf-8")).hexdigest()[:16]
    record_key = hashlib.sha256(record.record_id.encode("utf-8")).hexdigest()[:16]
    invocation_key = hashlib.sha256(str(job["invocation_id"]).encode("utf-8")).hexdigest()[:16]
    parent = _query_identity_root(run_dir, input_key, record_key, create=True)
    output = parent / f"inv_{invocation_key}"
    if (output.exists() or output.is_symlink()) and _is_link_like(output):
        raise QueryStructureError("query_structure_output_directory_link")
    return output


def _query_output_root(run_dir: Path, *, create: bool) -> Path:
    run_root = run_dir.resolve(strict=True)
    current = run_root
    for name in ("tool_outputs", "query_structure"):
        candidate = current / name
        if candidate.exists() or candidate.is_symlink():
            if _is_link_like(candidate) or not candidate.is_dir():
                raise QueryStructureError("query_structure_output_root_link_or_file")
            resolved = candidate.resolve(strict=True)
            try:
                resolved.relative_to(run_root)
            except ValueError as exc:
                raise QueryStructureError("query_structure_output_root_outside_run") from exc
            current = resolved
        elif create:
            candidate.mkdir()
            current = candidate.resolve(strict=True)
        else:
            return candidate
    return current


def _is_link_like(path: Path) -> bool:
    if path.is_symlink():
        return True
    is_junction = getattr(path, "is_junction", None)
    return bool(is_junction and is_junction())


def _query_identity_root(
    run_dir: Path,
    input_key: str,
    record_key: str,
    *,
    create: bool,
) -> Path:
    run_root = run_dir.resolve(strict=True)
    current = _query_output_root(run_dir, create=create)
    for name in (input_key, record_key):
        candidate = current / name
        if candidate.exists() or candidate.is_symlink():
            if _is_link_like(candidate) or not candidate.is_dir():
                raise QueryStructureError("query_structure_output_directory_link_or_file")
            resolved = candidate.resolve(strict=True)
            try:
                resolved.relative_to(run_root)
            except ValueError as exc:
                raise QueryStructureError("query_structure_output_directory_outside_run") from exc
            current = resolved
        elif create:
            candidate.mkdir()
            current = candidate.resolve(strict=True)
        else:
            return candidate
    return current


def _recover_missing_output_dir(output_dir: Path) -> None:
    backup = output_dir.with_name(f".{output_dir.name}.previous")
    if (backup.exists() or backup.is_symlink()) and _is_link_like(backup):
        raise QueryStructureError("query_structure_backup_directory_link")
    if not output_dir.exists() and backup.is_dir() and not _is_link_like(backup):
        os.replace(backup, output_dir)


def _remove_backup_directory(output_dir: Path) -> None:
    backup = output_dir.with_name(f".{output_dir.name}.previous")
    if (backup.exists() or backup.is_symlink()) and _is_link_like(backup):
        raise QueryStructureError("query_structure_backup_directory_link")
    if backup.is_dir() and not _is_link_like(backup):
        shutil.rmtree(backup)


def _valid_existing_artifact(
    run_dir: Path,
    job: dict[str, Any],
    record: InputRecord,
    output_dir: Path,
) -> PublishedArtifacts | None:
    manifest_path = output_dir / "manifest.json"
    if not manifest_path.is_file():
        return None
    try:
        value = _read_json(manifest_path)
        expected = {
            "schema_version": ARTIFACT_SCHEMA_VERSION,
            "status": "ready",
            "run_id": str(job["run_id"]),
            "job_id": str(job["job_id"]),
            "invocation_id": str(job["invocation_id"]),
            "input_id": record.input_id,
            "record_id": record.record_id,
            "input_kind": record.input_kind,
            "input_digest": record.input_digest,
            "source_content_sha256": record.source_digest,
            "conversion_manifest_sha256": sha256_file(
                run_dir / "input_conversion_manifest.tsv"
            ),
            "method_version": METHOD_VERSION,
        }
        if any(value.get(key) != expected_value for key, expected_value in expected.items()):
            return None
        features = _resolve_run_artifact(run_dir, str(value.get("features_json", "")))
        if features.parent.resolve() != output_dir.resolve():
            return None
        feature_digest = str(value.get("feature_digest", ""))
        if not valid_sha256(feature_digest) or sha256_file(features) != feature_digest:
            return None
        feature_value = _read_json(features)
        feature_expected = {
            **expected,
            "schema_version": FEATURE_SCHEMA_VERSION,
            "status": "ready",
        }
        if any(
            feature_value.get(key) != expected_value
            for key, expected_value in feature_expected.items()
        ):
            return None
        coordinate = str(value.get("coordinate_artifact", ""))
        coordinate_digest = str(value.get("coordinate_sha256", ""))
        coordinate_bytes = int(value.get("coordinate_size_bytes", -1))
        feature_coordinate = feature_value.get("coordinate_artifact")
        if not isinstance(feature_coordinate, dict) or (
            feature_coordinate.get("path") != coordinate
            or feature_coordinate.get("sha256") != coordinate_digest
            or feature_coordinate.get("size_bytes") != coordinate_bytes
        ):
            return None
        if coordinate:
            coordinate_path = runtime_artifact_path(coordinate)
            if not coordinate_path.is_absolute():
                coordinate_path = run_dir / coordinate_path
            if (
                not coordinate_path.is_file()
                or not valid_sha256(coordinate_digest)
                or sha256_file(coordinate_path) != coordinate_digest
                or coordinate_path.stat().st_size != coordinate_bytes
            ):
                return None
            if (
                record.input_format not in STRUCTURE_FORMATS
                and coordinate_path.resolve().parent != output_dir.resolve()
            ):
                return None
        elif coordinate_digest or coordinate_bytes != 0:
            return None
        return PublishedArtifacts(
            manifest_path,
            features,
            coordinate,
            feature_digest,
            "ready",
        )
    except (OSError, ValueError, json.JSONDecodeError, QueryStructureError):
        return None


def _publish_terminal_artifact(
    run_dir: Path,
    job: dict[str, Any],
    record: InputRecord,
    status: str,
    reason: str,
    detail: str = "",
) -> PublishedArtifacts:
    output_dir = _entry_output_dir(run_dir, job, record)
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=f".{output_dir.name}.", dir=output_dir.parent))
    payload = {
        "schema_version": ARTIFACT_SCHEMA_VERSION,
        "status": status,
        "run_id": str(job["run_id"]),
        "job_id": str(job["job_id"]),
        "invocation_id": str(job["invocation_id"]),
        "input_id": record.input_id,
        "record_id": record.record_id,
        "input_kind": record.input_kind,
        "input_digest": record.input_digest,
        "source_content_sha256": record.source_digest,
        "method_version": METHOD_VERSION,
        "reason": reason,
    }
    if detail:
        payload["detail"] = _safe_text(detail)
    try:
        _write_json_file(temporary / "manifest.json", payload)
        _replace_directory(temporary, output_dir)
        published = PublishedArtifacts(output_dir / "manifest.json", None, "", "", status)
        _upsert_index(run_dir, record, published)
        return published
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def _try_publish_failure(
    job: dict[str, Any],
    record: InputRecord | None,
    error: QueryStructureError,
) -> None:
    if record is None:
        return
    try:
        _publish_terminal_artifact(
            _resolve_run_dir(job),
            job,
            record,
            "failed",
            error.code,
            error.detail,
        )
    except Exception:
        pass


def _upsert_index(run_dir: Path, record: InputRecord, published: PublishedArtifacts) -> None:
    path = run_dir / "query_structure_manifest.tsv"
    rows: dict[tuple[str, str], dict[str, str]] = {}
    previous_paths: list[Path] = []
    if path.is_file():
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            if reader.fieldnames != INDEX_HEADER.rstrip("\n").split("\t"):
                raise QueryStructureError("query_structure_index_schema_invalid")
            for row in reader:
                key = (str(row.get("input_id", "")), str(row.get("record_id", "")))
                rows[key] = row
    key = (record.input_id, record.record_id)
    previous = rows.get(key)
    if previous:
        for field in ("features_json", "coordinate_artifact"):
            value = str(previous.get(field, ""))
            if value and not runtime_artifact_path(value).is_absolute():
                previous_paths.append(run_dir / runtime_artifact_path(value))
    rows[key] = {
        "input_id": record.input_id,
        "record_id": record.record_id,
        "input_kind": record.input_kind,
        "input_digest": record.input_digest,
        "method": METHOD_VERSION,
        "features_json": (
            _artifact_reference(run_dir, published.features) if published.features is not None else ""
        ),
        "coordinate_artifact": published.coordinate,
        "feature_digest": published.feature_digest,
        "status": published.status,
    }
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with temporary.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.DictWriter(
                handle,
                fieldnames=INDEX_HEADER.rstrip("\n").split("\t"),
                delimiter="\t",
                lineterminator="\n",
            )
            writer.writeheader()
            for row_key in sorted(rows):
                writer.writerow(rows[row_key])
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)
    current_dir = published.manifest.parent.resolve()
    root = _query_output_root(run_dir, create=True).resolve()
    for previous_path in previous_paths:
        try:
            previous_dir = previous_path.resolve().parent
            previous_dir.relative_to(root)
            if previous_dir != current_dir and previous_dir.is_dir():
                shutil.rmtree(previous_dir)
        except (OSError, ValueError):
            continue


def _find_published_artifacts(job: dict[str, Any]) -> PublishedArtifacts | None:
    try:
        run_dir = _resolve_run_dir(job)
        input_key = hashlib.sha256(str(job["input_id"]).encode("utf-8")).hexdigest()[:16]
        record_key = hashlib.sha256(str(job["record_id"]).encode("utf-8")).hexdigest()[:16]
        root = _query_identity_root(run_dir, input_key, record_key, create=False)
        matches = []
        for path in root.glob("inv_*/manifest.json"):
            value = _read_json(path)
            if value.get("job_id") != str(job["job_id"]):
                continue
            if value.get("invocation_id") != str(job["invocation_id"]):
                continue
            features_value = str(value.get("features_json", ""))
            features = _resolve_run_artifact(run_dir, features_value) if features_value else None
            matches.append(
                PublishedArtifacts(
                    path,
                    features,
                    str(value.get("coordinate_artifact", "")),
                    str(value.get("feature_digest", "")),
                    str(value.get("status", "")),
                )
            )
        return matches[0] if len(matches) == 1 else None
    except (OSError, ValueError, KeyError, json.JSONDecodeError, QueryStructureError):
        return None


def _replace_directory(source: Path, target: Path) -> None:
    backup = target.with_name(f".{target.name}.previous")
    if (target.exists() or target.is_symlink()) and _is_link_like(target):
        raise QueryStructureError("query_structure_output_directory_link")
    if (backup.exists() or backup.is_symlink()) and _is_link_like(backup):
        raise QueryStructureError("query_structure_backup_directory_link")
    if backup.exists():
        shutil.rmtree(backup)
    if target.exists():
        os.replace(target, backup)
    try:
        os.replace(source, target)
    except Exception:
        if backup.exists() and not target.exists():
            os.replace(backup, target)
        raise
    if backup.exists():
        shutil.rmtree(backup)


def _resolve_run_artifact(run_dir: Path, value: str | Path) -> Path:
    raw = str(value).strip()
    if not raw:
        raise QueryStructureError("query_structure_artifact_path_missing")
    path = runtime_artifact_path(raw)
    path = path if path.is_absolute() else run_dir / path
    try:
        resolved = path.resolve(strict=True)
        resolved.relative_to(run_dir.resolve())
    except (OSError, ValueError) as exc:
        raise QueryStructureError("query_structure_artifact_outside_run", raw) from exc
    if not resolved.is_file():
        raise QueryStructureError("query_structure_artifact_not_file", raw)
    return resolved


def _artifact_reference(run_dir: Path, path: str | Path) -> str:
    candidate = runtime_artifact_path(path)
    try:
        return candidate.resolve().relative_to(run_dir.resolve()).as_posix()
    except ValueError:
        return str(candidate.resolve())


def _write_json_file(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(value, handle, ensure_ascii=True, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def _write_json_atomic(path: Path, value: dict[str, Any]) -> None:
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        _write_json_file(temporary, value)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def _read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8-sig") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise QueryStructureError("query_structure_artifact_json_invalid")
    return value


def _fsync_file(path: Path) -> None:
    with path.open("rb") as handle:
        os.fsync(handle.fileno())


def _seed(input_digest: str) -> int:
    return int(input_digest[:8], 16) & 0x7FFFFFFF or 1


def _nonnegative_int(value: Any) -> int:
    try:
        parsed = int(str(value or "0"))
    except ValueError as exc:
        raise QueryStructureError("query_structure_declared_count_invalid") from exc
    if parsed < 0:
        raise QueryStructureError("query_structure_declared_count_invalid")
    return parsed


def _round(value: float) -> float:
    return round(float(value), 6)


def _safe_text(value: str) -> str:
    return " ".join(str(value).replace("\x00", "").split())[:512]


def decode_tsv_cell(value: str) -> str:
    output = []
    index = 0
    while index < len(value):
        if value[index] != "\\" or index + 1 >= len(value):
            output.append(value[index])
            index += 1
            continue
        marker = value[index + 1]
        decoded = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\"}.get(marker)
        if decoded is None:
            output.extend(("\\", marker))
        else:
            output.append(decoded)
        index += 2
    return "".join(output)


def _engine_version(record: InputRecord) -> str:
    if record.input_format in STRUCTURE_FORMATS:
        try:
            import openmm

            engine = f"OpenMM {openmm.__version__}"
        except ImportError:  # pragma: no cover - preparation would already have failed
            engine = "OpenMM unavailable"
    elif record.input_kind == "small_molecule" or (
        record.input_kind == "peptide"
        and len("".join(record.canonical_value.split())) <= PEPTIDE_CONFORMER_MAX_RESIDUES
    ):
        try:
            from rdkit import rdBase

            engine = f"RDKit {rdBase.rdkitVersion}"
        except ImportError:  # pragma: no cover - preparation would already have failed
            engine = "RDKit unavailable"
    else:
        engine = "built-in sequence descriptors"
    if record.input_format in STRUCTURE_FORMATS:
        retention = "original_input_reference_only"
    elif record.input_kind in {"protein_ligand", "genome_or_metagenome"} or (
        record.input_kind == "peptide"
        and len("".join(record.canonical_value.split())) > PEPTIDE_CONFORMER_MAX_RESIDUES
    ):
        retention = "no_coordinate_artifact"
    else:
        retention = "single_representative_only"
    return f"{METHOD_VERSION}; {engine}; retention={retention}"
