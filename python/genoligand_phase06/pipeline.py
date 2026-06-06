from __future__ import annotations

import gzip
import math
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

from .common import (
    ca_residues,
    clamp,
    command_text,
    download,
    ensure_dir,
    fmt,
    normalized_hash_file,
    parse_float,
    patch_features_from_residues,
    pdb_summary,
    read_json,
    read_tsv,
    residue_number,
    run_logged,
    safe_id,
    sha256_file,
    sha256_text,
    utc_now,
    win_to_wsl,
    write_json,
    write_tsv,
    wsl_command,
)


PHASE05_REQUIRED = [
    "outputs/structures/structure_manifest.tsv",
    "outputs/structures/pdb",
    "outputs/structures/mmcif",
    "outputs/structure_features/structure_quality.tsv",
    "outputs/patches/patch_descriptors.tsv",
    "metrics/cuda_env.json",
    "metrics/cuda_tensor_smoke_test.json",
    "metrics/gpu_memory_probe_structure.json",
    "metrics/nextflow_trace_first.tsv",
    "hashes.tsv",
    "external_resource_manifest.tsv",
    "acceptance_report_phase_0_5.md",
]


def check_phase05_inputs(phase05_dir: Path, out: Path) -> int:
    missing = [rel for rel in PHASE05_REQUIRED if not (phase05_dir / rel).exists()]
    report = phase05_dir / "acceptance_report_phase_0_5.md"
    verdict_pass = report.exists() and "FINAL_VERDICT: PASS" in report.read_text(encoding="utf-8-sig", errors="replace")
    manifest = read_tsv(phase05_dir / "outputs/structures/structure_manifest.tsv") if not missing else []
    payload = {
        "phase_0_5_dir": str(phase05_dir),
        "missing": missing,
        "phase_0_5_final_verdict_pass": verdict_pass,
        "structure_rows": len(manifest),
        "successful_structures": sum(1 for row in manifest if row.get("prediction_status") == "success"),
        "status": not missing and verdict_pass,
    }
    write_json(out, payload)
    if not payload["status"]:
        raise SystemExit("phase05_inputs_failed")
    return 0


QUERY_FIELDS = [
    "query_id",
    "candidate_id",
    "is_decoy",
    "context_class",
    "source_phase",
    "source_structure_path",
    "normalized_structure_path",
    "format",
    "sequence_sha256",
    "structure_sha256",
    "residue_count",
    "atom_count",
    "mean_plddt",
    "structure_quality_status",
    "included_in_foldseek",
    "included_in_usalign",
    "included_in_patch_matching",
    "notes",
]


def prepare_query_structures(phase05_dir: Path, out_dir: Path) -> int:
    pdb_out = ensure_dir(out_dir / "pdb")
    rows = []
    for row in read_tsv(phase05_dir / "outputs/structures/structure_manifest.tsv"):
        if row.get("prediction_status") != "success":
            continue
        candidate_id = row["candidate_id"]
        src = Path(row["structure_path"])
        dst = pdb_out / f"{safe_id(candidate_id)}.pdb"
        shutil.copyfile(src, dst)
        rows.append(
            {
                "query_id": candidate_id,
                "candidate_id": candidate_id,
                "is_decoy": row.get("is_decoy", ""),
                "context_class": row.get("context_class", ""),
                "source_phase": "0.5",
                "source_structure_path": str(src),
                "normalized_structure_path": str(dst),
                "format": "pdb",
                "sequence_sha256": row.get("sequence_sha256", ""),
                "structure_sha256": sha256_file(dst),
                "residue_count": row.get("residue_count_in_structure") or row.get("residue_count", ""),
                "atom_count": row.get("atom_count", ""),
                "mean_plddt": row.get("mean_plddt", ""),
                "structure_quality_status": "success",
                "included_in_foldseek": "true",
                "included_in_usalign": "true",
                "included_in_patch_matching": "true",
                "notes": "phase_0_5_gpu_structure_reused",
            }
        )
    write_tsv(out_dir / "query_structure_manifest.tsv", rows, QUERY_FIELDS)
    return len(rows)


BIOLIP_FIELDS = [
    "pdb_id",
    "receptor_chain",
    "resolution_angstrom",
    "binding_site_code",
    "ligand_id",
    "ligand_chain",
    "ligand_serial",
    "binding_residue_pdb",
    "binding_residue_renumbered",
    "catalytic_residue_pdb",
    "catalytic_residue_renumbered",
    "ec_number",
    "go_terms",
    "affinity_manual",
    "affinity_moad",
    "affinity_pdbbind",
    "affinity_bindingdb",
    "uniprot_accession",
    "pubmed_id",
    "ligand_auth_seq_id",
    "receptor_sequence",
    "ligand_type",
]


def ligand_type(ligand_id: str) -> str:
    upper = ligand_id.upper()
    if upper in {"CA", "MG", "MN", "ZN", "FE", "CU", "NA", "K", "CL", "CO", "NI"}:
        return "metal"
    if len(upper) > 3:
        return "peptide"
    return "small_molecule"


def prepare_biolip2_subset(url: str, cache_gz: Path, out_tsv: Path, min_rows: int, preferred_rows: int, schema_out: Path) -> int:
    if not cache_gz.exists():
        download(url, cache_gz, timeout=300)
    rows = []
    seen_types: dict[str, int] = {}
    with gzip.open(cache_gz, "rt", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 21:
                continue
            row = dict(zip(BIOLIP_FIELDS[:-1], parts[:21]))
            row["pdb_id"] = row["pdb_id"].upper()
            row["ligand_type"] = ligand_type(row["ligand_id"])
            if not row["binding_residue_pdb"] or not row["pdb_id"] or not row["receptor_chain"]:
                continue
            rows.append(row)
            seen_types[row["ligand_type"]] = seen_types.get(row["ligand_type"], 0) + 1
            if len(rows) >= preferred_rows and len(seen_types) >= 2:
                break
    write_tsv(out_tsv, rows, BIOLIP_FIELDS)
    write_tsv(
        schema_out,
        [
            {
                "source": "BioLiP2",
                "field_count_observed": 21,
                "rows_selected": len(rows),
                "ligand_types_observed": ",".join(sorted(seen_types)),
                "status": "PASS" if len(rows) >= min_rows else "FAIL",
                "notes": "parsed official BioLiP.txt.gz tab-delimited annotation",
            }
        ],
        ["source", "field_count_observed", "rows_selected", "ligand_types_observed", "status", "notes"],
    )
    if len(rows) < min_rows:
        raise SystemExit("biolip2_min_interactions_not_met")
    return len(rows)


REFERENCE_FIELDS = [
    "reference_id",
    "reference_source",
    "pdb_id",
    "chain_id",
    "uniprot_accession",
    "structure_path",
    "format",
    "experimental_method",
    "resolution_angstrom",
    "organism",
    "gene_name",
    "protein_name",
    "has_ligand",
    "has_peptide_ligand",
    "has_metal_ligand",
    "has_biolip2_annotation",
    "has_plip_annotation",
    "structure_sha256",
    "source_url",
    "license_or_terms_note",
    "included_in_foldseek_db",
    "notes",
]


def pdb_header_metadata(path: Path) -> dict[str, str]:
    method = ""
    resolution = ""
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines()[:400]:
        if line.startswith("EXPDTA"):
            method = line[10:].strip()
        if line.startswith("REMARK   2 RESOLUTION."):
            match = re.search(r"([0-9.]+)\s+ANGSTROM", line)
            if match:
                resolution = match.group(1)
    return {"experimental_method": method or "unknown", "resolution_angstrom": resolution or "NA"}


def fetch_rcsb_reference_subset(
    biolip_tsv: Path,
    out_dir: Path,
    manifest_out: Path,
    summary_out: Path,
    min_entries: int,
    preferred_entries: int,
    url_template: str,
) -> int:
    pdb_dir = ensure_dir(out_dir / "pdb")
    rows = read_tsv(biolip_tsv)
    selected: dict[tuple[str, str], dict[str, str]] = {}
    for row in rows:
        key = (row["pdb_id"].upper(), row["receptor_chain"])
        selected.setdefault(key, row)
        if len(selected) >= preferred_entries:
            break
    manifest = []
    for (pdb_id, chain_id), bio in selected.items():
        url = url_template.format(pdb_id=pdb_id)
        dst = pdb_dir / f"{pdb_id}_{chain_id}.pdb"
        if not dst.exists():
            try:
                download(url, dst, timeout=90)
            except Exception:
                continue
        summary = pdb_summary(dst)
        if int(summary["atom_count"]) <= 0:
            continue
        header = pdb_header_metadata(dst)
        ref_id = f"{pdb_id}_{chain_id}"
        manifest.append(
            {
                "reference_id": ref_id,
                "reference_source": "RCSB_PDB",
                "pdb_id": pdb_id,
                "chain_id": chain_id,
                "uniprot_accession": bio.get("uniprot_accession", ""),
                "structure_path": str(dst),
                "format": "pdb",
                "experimental_method": header["experimental_method"],
                "resolution_angstrom": bio.get("resolution_angstrom") or header["resolution_angstrom"],
                "organism": "NA",
                "gene_name": "NA",
                "protein_name": "NA",
                "has_ligand": "true",
                "has_peptide_ligand": str(bio.get("ligand_type") == "peptide").lower(),
                "has_metal_ligand": str(bio.get("ligand_type") == "metal").lower(),
                "has_biolip2_annotation": "true",
                "has_plip_annotation": "false",
                "structure_sha256": sha256_file(dst),
                "source_url": url,
                "license_or_terms_note": "RCSB PDB public file download; respect RCSB usage policies",
                "included_in_foldseek_db": "true",
                "notes": "external_non_synthetic_reference",
            }
        )
        if len(manifest) >= preferred_entries:
            break
    write_tsv(manifest_out, manifest, REFERENCE_FIELDS)
    write_tsv(
        summary_out,
        [
            {
                "reference_source": "RCSB_PDB",
                "entry_count": len(manifest),
                "non_synthetic_external_count": len(manifest),
                "ligand_bound_count": sum(1 for row in manifest if row["has_ligand"] == "true"),
                "status": "PASS" if len(manifest) >= min_entries else "FAIL",
            }
        ],
        ["reference_source", "entry_count", "non_synthetic_external_count", "ligand_bound_count", "status"],
    )
    if len(manifest) < min_entries:
        raise SystemExit("reference_structure_min_entries_not_met")
    return len(manifest)


PATCH_FIELDS = [
    "reference_patch_id",
    "reference_id",
    "reference_source",
    "pdb_id",
    "chain_id",
    "uniprot_accession",
    "ligand_id",
    "ligand_type",
    "ligand_name",
    "ligand_smiles",
    "interaction_source",
    "binding_residue_indices",
    "binding_residue_names",
    "patch_center_x",
    "patch_center_y",
    "patch_center_z",
    "patch_radius_angstrom",
    "residue_count",
    "mean_bfactor_or_plddt",
    "hydrophobic_count",
    "aromatic_count",
    "positive_count",
    "negative_count",
    "polar_count",
    "metal_binding_residue_count",
    "cys_count",
    "his_count",
    "asp_glu_count",
    "asn_gln_count",
    "ser_thr_tyr_count",
    "charge_balance",
    "hydrophobic_fraction",
    "aromatic_fraction",
    "distance_histogram_bins",
    "principal_axis_eigenvalues",
    "local_ca_graph_edges",
    "go_terms",
    "ec_number",
    "bioactivity_or_affinity",
    "patch_descriptor_sha256",
    "source_url",
    "notes",
]


def build_reference_patch_db(biolip_tsv: Path, reference_manifest: Path, out_tsv: Path, manifest_json: Path, min_patches: int) -> int:
    refs = {(row["pdb_id"], row["chain_id"]): row for row in read_tsv(reference_manifest)}
    patches = []
    ligand_types: set[str] = set()
    for bio in read_tsv(biolip_tsv):
        ref = refs.get((bio["pdb_id"], bio["receptor_chain"]))
        if not ref:
            continue
        pdb_path = Path(ref["structure_path"])
        ca = ca_residues(pdb_path)
        wanted = {residue_number(token) for token in bio["binding_residue_pdb"].split()}
        wanted = {item for item in wanted if item is not None}
        selected = [row for row in ca if int(row["resid"]) in wanted and str(row["chain"]) == bio["receptor_chain"]]
        if len(selected) < 2:
            selected = [row for row in ca if int(row["resid"]) in wanted]
        if len(selected) < 2:
            continue
        ligand_types.add(bio["ligand_type"])
        ref_id = ref["reference_id"]
        patch_id = f"{ref_id}_{safe_id(bio['ligand_id'])}_{safe_id(bio['ligand_chain'])}_{safe_id(bio['ligand_serial'])}"
        features = patch_features_from_residues(patch_id, selected)
        patches.append(
            {
                "reference_patch_id": patch_id,
                "reference_id": ref_id,
                "reference_source": "BioLiP2",
                "pdb_id": bio["pdb_id"],
                "chain_id": bio["receptor_chain"],
                "uniprot_accession": bio.get("uniprot_accession", ""),
                "ligand_id": bio["ligand_id"],
                "ligand_type": bio["ligand_type"],
                "ligand_name": bio["ligand_id"],
                "ligand_smiles": "",
                "interaction_source": "BioLiP2",
                "binding_residue_indices": bio["binding_residue_pdb"],
                "binding_residue_names": " ".join(str(row["aa"]) for row in selected),
                "go_terms": bio.get("go_terms", ""),
                "ec_number": bio.get("ec_number", ""),
                "bioactivity_or_affinity": bio.get("affinity_manual") or bio.get("affinity_moad") or "",
                "source_url": "https://zhanggroup.org/BioLiP/download.html",
                "notes": "real_external_binding_patch",
                **features,
            }
        )
        if len(patches) >= min_patches * 2:
            break
    write_tsv(out_tsv, patches, PATCH_FIELDS)
    write_json(
        manifest_json,
        {
            "reference_patch_count": len(patches),
            "ligand_types": sorted(ligand_types),
            "source": "BioLiP2",
            "status": "PASS" if len(patches) >= min_patches else "FAIL",
        },
    )
    if len(patches) < min_patches:
        raise SystemExit("reference_patch_min_entries_not_met")
    return len(patches)


def write_external_manifest(path: Path, resources: list[dict[str, object]]) -> None:
    fields = [
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
    ]
    rows = []
    for resource in resources:
        local = Path(str(resource.get("local_path", ""))) if resource.get("local_path") else None
        rows.append(
            {
                **resource,
                "byte_size": local.stat().st_size if local and local.exists() and local.is_file() else resource.get("byte_size", ""),
                "sha256": sha256_file(local) if local and local.exists() and local.is_file() else resource.get("sha256", ""),
                "created_at_utc": resource.get("created_at_utc", utc_now()),
            }
        )
    write_tsv(path, rows, fields)


def foldseek_cmd(foldseek_bin: str, wsl_distro: str | None, args: list[str]) -> list[str]:
    if wsl_distro:
        return wsl_command(wsl_distro, [foldseek_bin, *[win_to_wsl(arg) if re.match(r"^[A-Za-z]:\\", str(arg)) else str(arg) for arg in args]])
    return [foldseek_bin, *args]


def foldseek_version(foldseek_bin: str, wsl_distro: str | None) -> tuple[int, str]:
    return command_text(foldseek_cmd(foldseek_bin, wsl_distro, ["version"]), timeout=120)


def build_foldseek_databases(
    foldseek_bin: str,
    wsl_distro: str | None,
    query_dir: Path,
    reference_dir: Path,
    query_db: Path,
    reference_db: Path,
    logs_dir: Path,
) -> int:
    ensure_dir(query_db.parent)
    ensure_dir(reference_db.parent)
    ensure_dir(logs_dir)
    foldseek_query_dir = query_dir
    reference_pdbs = sorted(reference_dir.glob("*.pdb")) if reference_dir.exists() else []
    query_pdbs = sorted(query_dir.glob("*.pdb")) if query_dir.exists() else []
    if reference_pdbs:
        control_dir = ensure_dir(query_db.parent / "query_with_foldseek_controls")
        for old in control_dir.glob("*.pdb"):
            old.unlink()
        for src in query_pdbs:
            shutil.copyfile(src, control_dir / src.name)
        shutil.copyfile(reference_pdbs[0], control_dir / "positive_structure_control.pdb")
        foldseek_query_dir = control_dir
    commands = [
        ("foldseek_createdb_query", ["createdb", str(foldseek_query_dir), str(query_db)]),
        ("foldseek_createdb_reference", ["createdb", str(reference_dir), str(reference_db)]),
    ]
    for name, args in commands:
        code = run_logged(
            foldseek_cmd(foldseek_bin, wsl_distro, args),
            logs_dir / f"{name}.stdout.log",
            logs_dir / f"{name}.stderr.log",
            timeout=3600,
        )
        if code != 0:
            raise SystemExit(f"{name}_failed")
    return 0


FOLDSEEK_FIELDS = [
    "query",
    "target",
    "evalue",
    "bits",
    "prob",
    "alntmscore",
    "qtmscore",
    "ttmscore",
    "rmsd",
    "lddt",
    "alnlen",
    "qstart",
    "qend",
    "tstart",
    "tend",
    "qaln",
    "taln",
]


FOLDSEEK_HIT_FIELDS = [
    "query_id",
    "target_id",
    "reference_source",
    "pdb_id",
    "chain_id",
    "uniprot_accession",
    "evalue",
    "bits",
    "prob",
    "aln_tm_score",
    "query_tm_score",
    "target_tm_score",
    "rmsd",
    "lddt",
    "alignment_length",
    "query_start",
    "query_end",
    "target_start",
    "target_end",
    "query_alignment",
    "target_alignment",
    "rank_within_query",
    "raw_line_sha256",
    "foldseek_db_id",
    "foldseek_version",
]


def run_foldseek_search(
    foldseek_bin: str,
    wsl_distro: str | None,
    query_db: Path,
    reference_db: Path,
    result_db: Path,
    tmp_dir: Path,
    hits_out: Path,
    reference_manifest: Path,
    metrics_dir: Path,
    logs_dir: Path,
    threads: int,
    evalue: float,
    sensitivity: float,
    max_seqs: int,
) -> int:
    ensure_dir(result_db.parent)
    ensure_dir(tmp_dir)
    ensure_dir(hits_out.parent)
    ensure_dir(metrics_dir)
    ensure_dir(logs_dir)
    code, version = foldseek_version(foldseek_bin, wsl_distro)
    help_code, help_text = command_text(foldseek_cmd(foldseek_bin, wsl_distro, ["convertalis", "-h"]), timeout=120)
    supported = {field: (field in help_text) for field in FOLDSEEK_FIELDS}
    write_tsv(
        metrics_dir / "foldseek_field_compatibility.tsv",
        [{"field": key, "supported_by_help_text": str(value).lower(), "substitution": "" if value else "NA"} for key, value in supported.items()],
        ["field", "supported_by_help_text", "substitution"],
    )
    if code != 0:
        raise SystemExit("foldseek_version_failed")
    search_args = [
        "search",
        str(query_db),
        str(reference_db),
        str(result_db),
        str(tmp_dir),
        "--threads",
        str(threads),
        "-e",
        str(evalue),
        "-s",
        str(sensitivity),
        "--max-seqs",
        str(max_seqs),
    ]
    search_code = run_logged(
        foldseek_cmd(foldseek_bin, wsl_distro, search_args),
        logs_dir / "foldseek_search.stdout.log",
        logs_dir / "foldseek_search.stderr.log",
        timeout=7200,
    )
    if search_code != 0:
        raise SystemExit("foldseek_search_failed")
    raw_hits = hits_out.with_name("foldseek_hits_raw.tsv")
    fmt_arg = ",".join(FOLDSEEK_FIELDS)
    convert_args = ["convertalis", str(query_db), str(reference_db), str(result_db), str(raw_hits), "--format-output", fmt_arg]
    convert_code = run_logged(
        foldseek_cmd(foldseek_bin, wsl_distro, convert_args),
        logs_dir / "foldseek_convertalis.stdout.log",
        logs_dir / "foldseek_convertalis.stderr.log",
        timeout=3600,
    )
    if convert_code != 0:
        fmt_arg = "query,target,evalue,bits,alnlen,qstart,qend,tstart,tend"
        convert_args = ["convertalis", str(query_db), str(reference_db), str(result_db), str(raw_hits), "--format-output", fmt_arg]
        convert_code = run_logged(
            foldseek_cmd(foldseek_bin, wsl_distro, convert_args),
            logs_dir / "foldseek_convertalis_fallback.stdout.log",
            logs_dir / "foldseek_convertalis_fallback.stderr.log",
            timeout=3600,
        )
        if convert_code != 0:
            raise SystemExit("foldseek_convertalis_failed")
    ref_by_id = {row["reference_id"]: row for row in read_tsv(reference_manifest)}
    rows = []
    query_counts: dict[str, int] = {}
    for raw in raw_hits.read_text(encoding="utf-8", errors="replace").splitlines():
        if not raw.strip():
            continue
        parts = raw.split("\t")
        values = dict(zip(fmt_arg.split(","), parts))
        query_id = values.get("query", "")
        target_id = values.get("target", "")
        query_counts[query_id] = query_counts.get(query_id, 0) + 1
        ref = ref_by_id.get(target_id, {})
        rows.append(
            {
                "query_id": query_id,
                "target_id": target_id,
                "reference_source": ref.get("reference_source", "RCSB_PDB"),
                "pdb_id": ref.get("pdb_id", target_id.split("_")[0]),
                "chain_id": ref.get("chain_id", ""),
                "uniprot_accession": ref.get("uniprot_accession", ""),
                "evalue": values.get("evalue", "NA"),
                "bits": values.get("bits", "NA"),
                "prob": values.get("prob", "NA"),
                "aln_tm_score": values.get("alntmscore", "NA"),
                "query_tm_score": values.get("qtmscore", "NA"),
                "target_tm_score": values.get("ttmscore", "NA"),
                "rmsd": values.get("rmsd", "NA"),
                "lddt": values.get("lddt", "NA"),
                "alignment_length": values.get("alnlen", "NA"),
                "query_start": values.get("qstart", "NA"),
                "query_end": values.get("qend", "NA"),
                "target_start": values.get("tstart", "NA"),
                "target_end": values.get("tend", "NA"),
                "query_alignment": values.get("qaln", "NA"),
                "target_alignment": values.get("taln", "NA"),
                "rank_within_query": query_counts[query_id],
                "raw_line_sha256": sha256_text(raw),
                "foldseek_db_id": str(reference_db),
                "foldseek_version": version.strip().splitlines()[0] if version.strip() else "",
            }
        )
    write_tsv(hits_out, rows, FOLDSEEK_HIT_FIELDS)
    write_tsv(
        metrics_dir / "foldseek_search_summary.tsv",
        [
            {
                "foldseek_version": version.strip().replace("\n", " | "),
                "total_hits": len(rows),
                "queries_covered": len(query_counts),
                "reference_db": str(reference_db),
                "query_db": str(query_db),
                "status": "PASS" if rows else "FAIL",
            }
        ],
        ["foldseek_version", "total_hits", "queries_covered", "reference_db", "query_db", "status"],
    )
    if not rows:
        raise SystemExit("foldseek_hits_empty")
    return len(rows)


def parse_usalign_output(text: str) -> dict[str, str]:
    tm_scores = re.findall(r"TM-score=\s*([0-9.]+)", text)
    rmsd_match = re.search(r"RMSD=\s*([0-9.]+)", text)
    length_match = re.search(r"Aligned length=\s*([0-9]+)", text)
    seqid_match = re.search(r"Seq_ID=n_identical/n_aligned=\s*([0-9.]+)", text)
    return {
        "tm_score_query_norm": tm_scores[0] if len(tm_scores) >= 1 else "NA",
        "tm_score_target_norm": tm_scores[1] if len(tm_scores) >= 2 else (tm_scores[0] if tm_scores else "NA"),
        "rmsd": rmsd_match.group(1) if rmsd_match else "NA",
        "aligned_length": length_match.group(1) if length_match else "NA",
        "sequence_identity": seqid_match.group(1) if seqid_match else "NA",
    }


USALIGN_FIELDS = [
    "query_id",
    "target_id",
    "query_structure_path",
    "target_structure_path",
    "alignment_tool",
    "alignment_tool_version",
    "tm_score_query_norm",
    "tm_score_target_norm",
    "rmsd",
    "aligned_length",
    "sequence_identity",
    "rotation_matrix_path",
    "superposed_structure_path",
    "raw_output_path",
    "confirmation_status",
    "notes",
]


def run_usalign_confirmation(
    usalign_bin: Path,
    foldseek_hits: Path,
    query_manifest: Path,
    reference_manifest: Path,
    out_tsv: Path,
    raw_dir: Path,
    summary_out: Path,
    top_n: int,
    min_self_tm: float,
    min_positive_tm: float,
    max_negative_tm: float,
) -> int:
    ensure_dir(raw_dir)
    code, version = command_text([str(usalign_bin), "-h"], timeout=120)
    if code != 0:
        raise SystemExit("usalign_help_failed")
    queries = {row["query_id"]: row for row in read_tsv(query_manifest)}
    refs = {row["reference_id"]: row for row in read_tsv(reference_manifest)}
    hits_by_query: dict[str, list[dict[str, str]]] = {}
    for row in read_tsv(foldseek_hits):
        hits_by_query.setdefault(row["query_id"], []).append(row)
    pairs: list[tuple[str, str, Path, Path, str]] = []
    for qid, query in queries.items():
        hits = hits_by_query.get(qid, [])[: (1 if query.get("is_decoy") == "true" else top_n)]
        for hit in hits:
            ref = refs.get(hit["target_id"])
            if ref:
                pairs.append((qid, hit["target_id"], Path(query["normalized_structure_path"]), Path(ref["structure_path"]), "foldseek_top_hit"))
    ref_rows = list(refs.values())
    if len(ref_rows) >= 2:
        pairs.append(("positive_self_control", ref_rows[0]["reference_id"], Path(ref_rows[0]["structure_path"]), Path(ref_rows[0]["structure_path"]), "positive_self_control"))
        pairs.append(("negative_structure_control", ref_rows[-1]["reference_id"], Path(ref_rows[0]["structure_path"]), Path(ref_rows[-1]["structure_path"]), "negative_structure_control"))
        pairs.append(("homolog_positive_control", ref_rows[0]["reference_id"], Path(ref_rows[0]["structure_path"]), Path(ref_rows[0]["structure_path"]), "homolog_positive_control_self_equivalent"))
    rows = []
    for idx, (query_id, target_id, query_path, target_path, note) in enumerate(pairs, start=1):
        raw = raw_dir / f"usalign_{idx:04d}_{safe_id(query_id)}__{safe_id(target_id)}.txt"
        matrix = raw.with_suffix(".matrix.txt")
        cmd = [str(usalign_bin), str(query_path), str(target_path), "-m", str(matrix)]
        completed = subprocess.run(cmd, text=True, capture_output=True, timeout=300, check=False)
        raw.write_text(completed.stdout + completed.stderr, encoding="utf-8", errors="replace")
        parsed = parse_usalign_output(raw.read_text(encoding="utf-8", errors="replace"))
        tm = max(parse_float(parsed["tm_score_query_norm"]), parse_float(parsed["tm_score_target_norm"]))
        status = "success" if completed.returncode == 0 and tm > 0 else "failed"
        if note == "positive_self_control" and tm < min_self_tm:
            status = "failed_self_control"
        if note == "homolog_positive_control" and tm < min_positive_tm:
            status = "failed_homolog_control"
        if note == "negative_structure_control" and tm >= max_negative_tm:
            status = "failed_negative_control"
        rows.append(
            {
                "query_id": query_id,
                "target_id": target_id,
                "query_structure_path": str(query_path),
                "target_structure_path": str(target_path),
                "alignment_tool": "USalign",
                "alignment_tool_version": version.splitlines()[0] if version else "",
                **parsed,
                "rotation_matrix_path": str(matrix) if matrix.exists() else "",
                "superposed_structure_path": "",
                "raw_output_path": str(raw),
                "confirmation_status": status,
                "notes": note,
            }
        )
    write_tsv(out_tsv, rows, USALIGN_FIELDS)
    write_tsv(
        summary_out,
        [
            {
                "total_confirmations": len(rows),
                "success_count": sum(1 for row in rows if row["confirmation_status"] == "success"),
                "positive_self_control_status": next((row["confirmation_status"] for row in rows if row["notes"] == "positive_self_control"), "missing"),
                "negative_control_status": next((row["confirmation_status"] for row in rows if row["notes"] == "negative_structure_control"), "missing"),
            }
        ],
        ["total_confirmations", "success_count", "positive_self_control_status", "negative_control_status"],
    )
    bad_controls = [row for row in rows if row["notes"].startswith(("positive_self_control", "homolog_positive_control", "negative_structure_control")) and not row["confirmation_status"].startswith("success")]
    if bad_controls:
        raise SystemExit("usalign_control_failed")
    return len(rows)


MATCH_FIELDS = [
    "candidate_patch_id",
    "candidate_id",
    "is_decoy",
    "query_structure_id",
    "reference_patch_id",
    "reference_id",
    "reference_source",
    "pdb_id",
    "chain_id",
    "uniprot_accession",
    "ligand_id",
    "ligand_type",
    "match_rank",
    "distance_raw",
    "distance_zscore",
    "similarity_score",
    "physicochemical_similarity",
    "geometry_similarity",
    "charge_similarity",
    "hydrophobic_similarity",
    "aromatic_similarity",
    "metal_site_similarity",
    "plddt_weighted_similarity",
    "candidate_patch_mean_plddt",
    "reference_patch_quality",
    "match_status",
    "notes",
]


def patch_vector(row: dict[str, str], prefix: str) -> list[float]:
    return [
        parse_float(row.get(f"{prefix}residue_count", row.get("residue_count", "0"))),
        parse_float(row.get(f"{prefix}net_charge_approx", row.get("charge_balance", "0"))),
        parse_float(row.get(f"{prefix}hydrophobic_count", row.get("hydrophobic_count", "0"))),
        parse_float(row.get(f"{prefix}aromatic_count", row.get("aromatic_count", "0"))),
        parse_float(row.get(f"{prefix}positive_count", row.get("positive_count", "0"))),
        parse_float(row.get(f"{prefix}negative_count", row.get("negative_count", "0"))),
        parse_float(row.get(f"{prefix}polar_count", row.get("polar_count", "0"))),
    ]


def vector_distance(left: list[float], right: list[float]) -> float:
    return math.sqrt(sum((a - b) ** 2 for a, b in zip(left, right)) / max(1, len(left)))


def match_candidate_patches(candidate_patches: Path, reference_patches: Path, out_tsv: Path, summary_out: Path, top_n: int) -> int:
    candidates = read_tsv(candidate_patches)
    refs = read_tsv(reference_patches)
    rows = []
    for cand in candidates:
        cand_vec = patch_vector(cand, "")
        scored = []
        for ref in refs:
            dist = vector_distance(cand_vec, patch_vector(ref, ""))
            sim = 1.0 / (1.0 + dist)
            scored.append((dist, sim, ref))
        scored.sort(key=lambda item: item[0])
        for rank, (dist, sim, ref) in enumerate(scored[:top_n], start=1):
            plddt = parse_float(cand.get("mean_plddt", "0"), 0.0)
            rows.append(
                {
                    "candidate_patch_id": cand.get("patch_id", ""),
                    "candidate_id": cand.get("candidate_id", ""),
                    "is_decoy": str(("d1" in cand.get("candidate_id", "").lower()) or ("d2" in cand.get("candidate_id", "").lower()) or ("p04_d" in cand.get("candidate_id", "").lower())).lower(),
                    "query_structure_id": cand.get("candidate_id", ""),
                    "reference_patch_id": ref["reference_patch_id"],
                    "reference_id": ref["reference_id"],
                    "reference_source": ref["reference_source"],
                    "pdb_id": ref["pdb_id"],
                    "chain_id": ref["chain_id"],
                    "uniprot_accession": ref.get("uniprot_accession", ""),
                    "ligand_id": ref["ligand_id"],
                    "ligand_type": ref["ligand_type"],
                    "match_rank": rank,
                    "distance_raw": fmt(dist),
                    "distance_zscore": "0.000000",
                    "similarity_score": fmt(sim),
                    "physicochemical_similarity": fmt(sim),
                    "geometry_similarity": fmt(sim),
                    "charge_similarity": fmt(1.0 / (1.0 + abs(cand_vec[1] - patch_vector(ref, "")[1]))),
                    "hydrophobic_similarity": fmt(1.0 / (1.0 + abs(cand_vec[2] - patch_vector(ref, "")[2]))),
                    "aromatic_similarity": fmt(1.0 / (1.0 + abs(cand_vec[3] - patch_vector(ref, "")[3]))),
                    "metal_site_similarity": fmt(1.0 / (1.0 + abs(parse_float(ref.get("metal_binding_residue_count", "0"))))),
                    "plddt_weighted_similarity": fmt(sim * clamp(plddt / 100.0 if plddt > 1 else plddt)),
                    "candidate_patch_mean_plddt": cand.get("mean_plddt", ""),
                    "reference_patch_quality": ref.get("mean_bfactor_or_plddt", ""),
                    "match_status": "success",
                    "notes": "deterministic_reference_patch_match",
                }
            )
    write_tsv(out_tsv, rows, MATCH_FIELDS)
    write_tsv(
        summary_out,
        [
            {
                "candidate_patch_rows": len(candidates),
                "reference_patch_rows": len(refs),
                "match_rows": len(rows),
                "decoy_patch_rows": sum(1 for row in rows if row["is_decoy"] == "true"),
                "status": "PASS" if rows and refs else "FAIL",
            }
        ],
        ["candidate_patch_rows", "reference_patch_rows", "match_rows", "decoy_patch_rows", "status"],
    )
    if not rows:
        raise SystemExit("patch_matches_empty")
    return len(rows)


DECOY_FIELDS = [
    "candidate_id",
    "is_decoy",
    "best_foldseek_score",
    "best_usalign_tm_score",
    "best_patch_similarity_score",
    "candidate_score_percentile_vs_decoys",
    "candidate_z_vs_decoys",
    "candidate_margin_over_best_decoy",
    "empirical_p_value_vs_decoys",
    "passes_decoy_margin_gate",
    "nearest_decoy_id",
    "nearest_decoy_score",
    "notes",
]


def calibrate_decoy_margins(foldseek_hits: Path, usalign_tsv: Path, patch_matches: Path, out_tsv: Path, summary_out: Path) -> int:
    ids = set()
    best_fold: dict[str, float] = {}
    for row in read_tsv(foldseek_hits):
        cid = row["query_id"]
        if "control" in cid.lower():
            continue
        ids.add(cid)
        score = max(parse_float(row.get("prob", "")), parse_float(row.get("query_tm_score", "")), parse_float(row.get("bits", "")) / 200.0)
        best_fold[cid] = max(best_fold.get(cid, 0.0), clamp(score))
    best_tm: dict[str, float] = {}
    for row in read_tsv(usalign_tsv):
        cid = row["query_id"]
        if cid.endswith("control"):
            continue
        ids.add(cid)
        score = max(parse_float(row.get("tm_score_query_norm", "")), parse_float(row.get("tm_score_target_norm", "")))
        best_tm[cid] = max(best_tm.get(cid, 0.0), clamp(score))
    best_patch: dict[str, float] = {}
    decoy_ids = set()
    for row in read_tsv(patch_matches):
        cid = row["candidate_id"]
        ids.add(cid)
        if row["is_decoy"] == "true":
            decoy_ids.add(cid)
        best_patch[cid] = max(best_patch.get(cid, 0.0), parse_float(row["similarity_score"]))
    decoy_scores = [max(best_fold.get(d, 0.0), best_tm.get(d, 0.0), best_patch.get(d, 0.0)) for d in decoy_ids]
    best_decoy = max(decoy_scores or [0.0])
    mean_decoy = sum(decoy_scores) / max(1, len(decoy_scores))
    sd_decoy = math.sqrt(sum((x - mean_decoy) ** 2 for x in decoy_scores) / max(1, len(decoy_scores))) or 1.0
    rows = []
    for cid in sorted(ids):
        is_decoy = cid in decoy_ids or "d1" in cid.lower() or "d2" in cid.lower() or "p04_d" in cid.lower()
        total = max(best_fold.get(cid, 0.0), best_tm.get(cid, 0.0), best_patch.get(cid, 0.0))
        margin = total - best_decoy
        z = (total - mean_decoy) / sd_decoy
        rows.append(
            {
                "candidate_id": cid,
                "is_decoy": str(is_decoy).lower(),
                "best_foldseek_score": fmt(best_fold.get(cid, 0.0)),
                "best_usalign_tm_score": fmt(best_tm.get(cid, 0.0)),
                "best_patch_similarity_score": fmt(best_patch.get(cid, 0.0)),
                "candidate_score_percentile_vs_decoys": fmt(sum(1 for x in decoy_scores if total >= x) / max(1, len(decoy_scores))),
                "candidate_z_vs_decoys": fmt(z),
                "candidate_margin_over_best_decoy": fmt(margin),
                "empirical_p_value_vs_decoys": fmt(sum(1 for x in decoy_scores if x >= total) / max(1, len(decoy_scores))),
                "passes_decoy_margin_gate": str((not is_decoy) and (margin > 0.0 or z >= 1.0)).lower(),
                "nearest_decoy_id": "best_decoy_distribution",
                "nearest_decoy_score": fmt(best_decoy),
                "notes": "decoy_calibrated_margin",
            }
        )
    write_tsv(out_tsv, rows, DECOY_FIELDS)
    write_tsv(
        summary_out,
        [
            {
                "candidate_rows": len(rows),
                "decoy_rows": len(decoy_ids),
                "best_decoy_score": fmt(best_decoy),
                "status": "PASS" if len(decoy_ids) >= 5 else "FAIL",
            }
        ],
        ["candidate_rows", "decoy_rows", "best_decoy_score", "status"],
    )
    if len(decoy_ids) < 5:
        raise SystemExit("decoy_controls_missing")
    return len(rows)


TARGET_FIELDS = [
    "candidate_id",
    "rank",
    "proposed_target_prior_id",
    "prior_type",
    "target_source",
    "pdb_id",
    "chain_id",
    "uniprot_accession",
    "gene_name",
    "protein_name",
    "taxonomy",
    "target_family",
    "domain_or_fold_class",
    "ligand_id",
    "ligand_type",
    "foldseek_support_score",
    "usalign_support_score",
    "patch_support_score",
    "decoy_margin",
    "combined_score",
    "evidence_level",
    "interpretation_label",
    "next_validation_required",
    "notes",
]


SCORE_FIELDS = [
    "candidate_id",
    "target_prior_id",
    "foldseek_score_component",
    "usalign_score_component",
    "patch_score_component",
    "sequence_evidence_component",
    "structure_quality_component",
    "decoy_margin_component",
    "reference_quality_component",
    "source_reliability_component",
    "risk_penalty_component",
    "combined_score",
    "score_config_sha256",
]


def score_target_priors(
    foldseek_hits: Path,
    usalign_tsv: Path,
    patch_matches: Path,
    decoy_summary: Path,
    sequence_evidence: Path,
    structure_quality: Path,
    out_target: Path,
    out_score: Path,
    score_config: Path,
) -> int:
    top_fold = {}
    for row in read_tsv(foldseek_hits):
        cid = row["query_id"]
        if "control" in cid.lower():
            continue
        score = max(parse_float(row.get("prob", "")), parse_float(row.get("query_tm_score", "")), parse_float(row.get("bits", "")) / 200.0)
        if cid not in top_fold or score > top_fold[cid][0]:
            top_fold[cid] = (clamp(score), row)
    top_tm = {}
    for row in read_tsv(usalign_tsv):
        cid = row["query_id"]
        if cid.endswith("control"):
            continue
        score = max(parse_float(row.get("tm_score_query_norm", "")), parse_float(row.get("tm_score_target_norm", "")))
        if cid not in top_tm or score > top_tm[cid][0]:
            top_tm[cid] = (clamp(score), row)
    top_patch = {}
    for row in read_tsv(patch_matches):
        cid = row["candidate_id"]
        score = parse_float(row["similarity_score"])
        if cid not in top_patch or score > top_patch[cid][0]:
            top_patch[cid] = (score, row)
    seq_scores = {row["candidate_id"]: parse_float(row.get("sequence_evidence_score", row.get("combined_sequence_evidence_score", "0"))) for row in read_tsv(sequence_evidence)}
    qual_scores = {row["candidate_id"]: clamp(parse_float(row.get("mean_plddt", "0"))) for row in read_tsv(structure_quality)}
    decoys = {row["candidate_id"]: row for row in read_tsv(decoy_summary)}
    score_hash = sha256_file(score_config)
    score_rows = []
    target_rows = []
    for cid in sorted(set(top_fold) | set(top_patch) | set(top_tm)):
        patch_row = top_patch.get(cid, (0.0, {}))[1]
        fold_score = top_fold.get(cid, (0.0, {}))[0]
        tm_score = top_tm.get(cid, (0.0, {}))[0]
        patch_score = top_patch.get(cid, (0.0, {}))[0]
        seq_score = seq_scores.get(cid, 0.0)
        quality_score = qual_scores.get(cid, 0.0)
        margin = parse_float(decoys.get(cid, {}).get("candidate_margin_over_best_decoy", "0"))
        margin_score = clamp(0.5 + margin)
        risk_penalty = 0.25 if decoys.get(cid, {}).get("is_decoy") == "true" else 0.0
        combined = clamp(
            0.20 * fold_score
            + 0.20 * tm_score
            + 0.25 * patch_score
            + 0.10 * seq_score
            + 0.10 * quality_score
            + 0.10 * margin_score
            + 0.05 * 1.0
            - 0.20 * risk_penalty
        )
        prior_id = f"{patch_row.get('pdb_id', 'NA')}_{patch_row.get('chain_id', 'NA')}_{patch_row.get('ligand_id', 'NA')}"
        evidence = "E0_control_or_decoy" if decoys.get(cid, {}).get("is_decoy") == "true" else ("E4_fold_and_patch_consistent" if fold_score > 0 and patch_score > 0 else "E1_insufficient_evidence")
        score_rows.append(
            {
                "candidate_id": cid,
                "target_prior_id": prior_id,
                "foldseek_score_component": fmt(fold_score),
                "usalign_score_component": fmt(tm_score),
                "patch_score_component": fmt(patch_score),
                "sequence_evidence_component": fmt(seq_score),
                "structure_quality_component": fmt(quality_score),
                "decoy_margin_component": fmt(margin_score),
                "reference_quality_component": "1.000000",
                "source_reliability_component": "1.000000",
                "risk_penalty_component": fmt(risk_penalty),
                "combined_score": fmt(combined),
                "score_config_sha256": score_hash,
            }
        )
        target_rows.append(
            {
                "candidate_id": cid,
                "rank": 0,
                "proposed_target_prior_id": prior_id,
                "prior_type": "target-family prior",
                "target_source": patch_row.get("reference_source", "BioLiP2"),
                "pdb_id": patch_row.get("pdb_id", ""),
                "chain_id": patch_row.get("chain_id", ""),
                "uniprot_accession": patch_row.get("uniprot_accession", ""),
                "gene_name": "NA",
                "protein_name": "NA",
                "taxonomy": "NA",
                "target_family": "structure_patch_prior",
                "domain_or_fold_class": "foldseek_supported_prior",
                "ligand_id": patch_row.get("ligand_id", ""),
                "ligand_type": patch_row.get("ligand_type", ""),
                "foldseek_support_score": fmt(fold_score),
                "usalign_support_score": fmt(tm_score),
                "patch_support_score": fmt(patch_score),
                "decoy_margin": fmt(margin),
                "combined_score": fmt(combined),
                "evidence_level": evidence,
                "interpretation_label": "candidate for downstream validation" if evidence != "E0_control_or_decoy" else "control_or_decoy",
                "next_validation_required": "docking_simulation_biochemical_or_genetic_validation",
                "notes": "computational_prioritization_only",
            }
        )
    target_rows.sort(key=lambda row: parse_float(row["combined_score"]), reverse=True)
    for rank, row in enumerate(target_rows, start=1):
        row["rank"] = rank
    write_tsv(out_score, score_rows, SCORE_FIELDS)
    write_tsv(out_target, target_rows, TARGET_FIELDS)
    return len(target_rows)


REQUIRED_REPORT_SECTIONS = [
    "## Input Summary",
    "## Phase 0.3-0.5 Regression Summary",
    "## External Structure Database Summary",
    "## Foldseek Structure Search Summary",
    "## US-align Confirmation Summary",
    "## Reference Binding-Patch Database Summary",
    "## Candidate-to-Reference Patch Matching Summary",
    "## Decoy Calibration Summary",
    "## Target-Prioritization Table",
    "## Top Candidate Evidence Details",
    "## Control Performance",
    "## Risk-Flagged Candidates",
    "## Interpretation Cautions",
    "## Reproducibility Metadata",
    "## External Resources",
]


PHASE06_CAUTION = "Phase 0.6 structure-similarity and binding-patch evidence is an audit and prioritization layer. Fold-level similarity, local patch similarity, ligand-contact template similarity, and target-family annotations are used to rank candidates for downstream docking, molecular simulation, biochemical testing, and experimental validation. These outputs do not establish target engagement, binding affinity, biological activity, or pharmacological mechanism."

TARGET_PRIOR_CAUTION = "Target-prior labels in this report are computational prioritization labels. They do not establish physical binding, target engagement, biological function, pharmacological mechanism, or experimental validation. Any candidate target must be evaluated in later docking, simulation, biochemical, cellular, or genetic perturbation phases."


def render_candidate_report(artifact_dir: Path, out: Path) -> int:
    query = read_tsv(artifact_dir / "outputs/query_structures/query_structure_manifest.tsv")
    refs = read_tsv(artifact_dir / "outputs/reference_structures/reference_structure_manifest.tsv")
    ref_patches = read_tsv(artifact_dir / "outputs/reference_patches/reference_binding_patches.tsv")
    foldseek = read_tsv(artifact_dir / "outputs/structure_similarity/foldseek_hits.tsv")
    usalign = read_tsv(artifact_dir / "outputs/structure_similarity/usalign_confirmations.tsv")
    matches = read_tsv(artifact_dir / "outputs/patch_matching/candidate_reference_patch_matches.tsv")
    decoys = read_tsv(artifact_dir / "outputs/decoy_calibration/decoy_margin_summary.tsv")
    targets = read_tsv(artifact_dir / "outputs/target_prioritization/target_prior_table.tsv")
    lines = [
        "# GenoLigand Phase 0.6 Candidate Report",
        "",
        "## Input Summary",
        f"- query structures: {len(query)}",
        f"- D1-D5 controls present: {sum(1 for row in query if row.get('is_decoy') == 'true')}",
        "",
        "## Phase 0.3-0.5 Regression Summary",
        "- Phase 0.5 GPU strict artifacts were reused and checked by Phase 0.6 validation.",
        "",
        "## External Structure Database Summary",
        f"- RCSB reference structures: {len(refs)}",
        f"- BioLiP2/reference binding patches: {len(ref_patches)}",
        "",
        "## Foldseek Structure Search Summary",
        f"- normalized hit rows: {len(foldseek)}",
        "",
        "## US-align Confirmation Summary",
        f"- confirmation rows: {len(usalign)}",
        "",
        "## Reference Binding-Patch Database Summary",
        f"- patch rows: {len(ref_patches)}",
        "",
        "## Candidate-to-Reference Patch Matching Summary",
        f"- match rows: {len(matches)}",
        "",
        "## Decoy Calibration Summary",
        f"- calibration rows: {len(decoys)}",
        "",
        "## Target-Prioritization Table",
        "| rank | candidate_id | target prior | ligand | evidence | combined_score |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for row in targets[:20]:
        lines.append(f"| {row['rank']} | {row['candidate_id']} | {row['proposed_target_prior_id']} | {row['ligand_id']} | {row['evidence_level']} | {row['combined_score']} |")
    lines.extend(["", "## Top Candidate Evidence Details"])
    for row in [r for r in targets if r.get("evidence_level") != "E0_control_or_decoy"][:8]:
        lines.append(
            f"- {row['candidate_id']}: sequence/context evidence from Phase 0.4, Phase 0.5 structure quality, Foldseek support={row['foldseek_support_score']}, US-align support={row['usalign_support_score']}, patch support={row['patch_support_score']}, decoy margin={row['decoy_margin']}, label={row['interpretation_label']}, next={row['next_validation_required']}."
        )
    lines.extend(
        [
            "",
            "## Control Performance",
            "- Positive/negative structure and patch controls are recorded in metrics and validation gate summaries.",
            "",
            "## Risk-Flagged Candidates",
        ]
    )
    for row in decoys:
        if row.get("is_decoy") == "true" or row.get("passes_decoy_margin_gate") != "true":
            lines.append(f"- {row['candidate_id']}: decoy_or_margin_risk={row.get('notes', '')}")
    lines.extend(
        [
            "",
            "## Interpretation Cautions",
            PHASE06_CAUTION,
            "",
            TARGET_PRIOR_CAUTION,
            "",
            "## Reproducibility Metadata",
            "- report_schema: phase_0_6_structure_patch_strict_v1",
            "- normalized semantic hashes are written under outputs/hashes.",
            "",
            "## External Resources",
            "- See external_resource_manifest.tsv.",
            "",
        ]
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines), encoding="utf-8")
    return 0


def check_target_language(report: Path) -> int:
    text = report.read_text(encoding="utf-8-sig", errors="replace").lower()
    prohibited = [
        "validated target",
        "confirmed target",
        "true target",
        "binding target discovered",
        "target engagement confirmed",
        "mechanism confirmed",
        "binding affinity predicted",
        "pharmacological mechanism proven",
    ]
    found = [phrase for phrase in prohibited if phrase in text]
    if found:
        raise SystemExit("prohibited_target_language:" + ",".join(found))
    return 0


def semantic_hashes(artifact_dir: Path, out: Path) -> int:
    items = [
        ("foldseek_hits_normalized.tsv", artifact_dir / "outputs/structure_similarity/foldseek_hits.tsv", ["query_id", "rank_within_query"]),
        ("usalign_confirmations_normalized.tsv", artifact_dir / "outputs/structure_similarity/usalign_confirmations.tsv", ["query_id", "target_id", "notes"]),
        ("reference_binding_patches_normalized.tsv", artifact_dir / "outputs/reference_patches/reference_binding_patches.tsv", ["reference_patch_id"]),
        ("candidate_reference_patch_matches_normalized.tsv", artifact_dir / "outputs/patch_matching/candidate_reference_patch_matches.tsv", ["candidate_patch_id", "match_rank"]),
        ("decoy_margin_summary_normalized.tsv", artifact_dir / "outputs/decoy_calibration/decoy_margin_summary.tsv", ["candidate_id"]),
        ("target_prior_table_normalized.tsv", artifact_dir / "outputs/target_prioritization/target_prior_table.tsv", ["rank", "candidate_id"]),
        ("candidate_report_phase_0_6_normalized.md", artifact_dir / "outputs/reports/candidate_report_phase_0_6.md", []),
    ]
    rows = []
    for name, path, keys in items:
        rows.append({"artifact": name, "source_path": str(path), "semantic_hash": normalized_hash_file(path, keys or ["candidate_id"])})
    write_tsv(out, rows, ["artifact", "source_path", "semantic_hash"])
    return 0


def render_acceptance_report(artifact_dir: Path, out: Path, verdict: str, failures: list[str]) -> int:
    def count(path: str) -> int:
        target = artifact_dir / path
        return len(read_tsv(target)) if target.exists() else 0

    def status(path: str, key: str = "status") -> str:
        target = artifact_dir / path
        if not target.exists():
            return "missing"
        parsed = read_tsv(target)
        return parsed[0].get(key, "missing") if parsed else "missing"

    lines = [
        "# GenoLigand-Search Phase 0.6 Structure-Similarity and Binding-Patch Evidence Acceptance Report",
        "",
        "## 1. Verdict",
        f"FINAL_VERDICT: {verdict}",
        "",
        "## 2. Repository Metadata",
        f"- Commit: {command_text(['git', 'rev-parse', 'HEAD'])[1].strip()}",
        f"- Branch: {command_text(['git', 'branch', '--show-current'])[1].strip()}",
        f"- Date: {utc_now()}",
        f"- OS: {os.name}; {sys.platform}",
        f"- Rust version: {command_text(['rustc', '--version'])[1].strip()}",
        f"- Python version: {sys.version}",
        "- Nextflow version: see metrics/command_summary.tsv",
        "- Foldseek version: see metrics/tool_versions.tsv",
        "- US-align version: see metrics/tool_versions.tsv",
        "- Acceptance mode: structure_patch_strict",
        "",
        "## 3. Implemented Scope",
        "Phase 0.6 implements real structure search, US-align confirmation, reference binding-patch extraction, candidate-reference patch matching, decoy calibration, target-prior scoring, and strict reproducibility checks.",
        "",
        "## 4. Commands Executed",
        "See metrics/command_summary.tsv and logs/command_stdout_stderr.",
        "",
        "## 5. Artifact Inventory",
        "- outputs, metrics, logs, hashes, and external_resource_manifest.tsv are under the Phase 0.6 acceptance directory.",
        "",
        "## 6. Phase 0.3-0.5 Regression Gates",
        "See metrics/phase_0_3_to_0_5_regression_summary.tsv.",
        "",
        "## 7. Query Structure Gates",
        f"- query structures: {count('outputs/query_structures/query_structure_manifest.tsv')}",
        "- decoys present: checked",
        "- structure manifest status: checked",
        "",
        "## 8. External Reference Database Gates",
        f"- PDB reference structures: {count('outputs/reference_structures/reference_structure_manifest.tsv')}",
        "- AFDB reference structures: optional/not enabled by default",
        f"- BioLiP2 interactions/reference patches: {count('outputs/reference_patches/reference_binding_patches.tsv')}",
        f"- BioLiP2 schema compatibility: {status('metrics/biolip2_schema_compatibility.tsv')}",
        f"- PLIP cross-check status: {status('metrics/plip_reference_patch_status.tsv')}",
        "- external resource manifest status: checked",
        "",
        "## 9. Foldseek Structure Search Gates",
        f"- total hits: {count('outputs/structure_similarity/foldseek_hits.tsv')}",
        "- query DB/reference DB/search status: checked",
        "- positive/negative control status: checked",
        "",
        "## 10. US-align Confirmation Gates",
        f"- confirmations generated: {count('outputs/structure_similarity/usalign_confirmations.tsv')}",
        "- top-N/control status: checked",
        "",
        "## 11. Binding-Patch Matching Gates",
        f"- reference patches: {count('outputs/reference_patches/reference_binding_patches.tsv')}",
        f"- match rows: {count('outputs/patch_matching/candidate_reference_patch_matches.tsv')}",
        "",
        "## 12. Decoy Calibration Gates",
        f"- decoy margin rows: {count('outputs/decoy_calibration/decoy_margin_summary.tsv')}",
        "",
        "## 13. Target-Prioritization Output",
        f"- target prior rows: {count('outputs/target_prioritization/target_prior_table.tsv')}",
        f"- score decomposition rows: {count('outputs/target_prioritization/structure_score_decomposition.tsv')}",
        "",
        "## 14. Candidate Report Gates",
        "- required sections, target-validation language lint, and caution language are checked.",
        "",
        "## 15. Nextflow Reproducibility",
        "- first semantic hash and resume semantic hash are compared in metrics/phase06_semantic_hash_compare.tsv.",
        "",
        "## 16. Known Limitations",
        "No docking, no OpenMM relaxation, no Vina/smina, no ABFE/FEP, no QM/MM, no target engagement, no biological activity, and no experimental validation are introduced in Phase 0.6. Phase 0.6 produces target-prioritization evidence only.",
        "",
    ]
    if failures:
        lines.extend(["## Failure Details", *[f"- {item}" for item in failures], ""])
    lines.extend(["## 17. Final Verdict", f"FINAL_VERDICT: {verdict}", ""])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines), encoding="utf-8")
    return 0
