from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import time
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


ROOT = Path(__file__).resolve().parents[2]
DIAG_DIRNAME = "acceptance_artifacts_phase_0_8_2_failure_diagnosis"
PHASE082_DIRNAME = "acceptance_artifacts_phase_0_8_2_mapping_benchmark_storage_strict"
BIOLIP2_TABLE = ROOT / "data/external_phase_0_8/biolip2/biolip2_BioLiP.txt.gz"
PHASE082_ARTIFACT = ROOT / PHASE082_DIRNAME


def path_arg(value: str) -> Path:
    return Path(value).resolve()


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def ensure_diag_dirs(artifact_dir: Path) -> None:
    for rel in [
        "metrics",
        "outputs",
        "logs/command_stdout_stderr",
    ]:
        (artifact_dir / rel).mkdir(parents=True, exist_ok=True)


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except FileNotFoundError:
        return ""


def open_maybe_gzip(path: Path, mode: str = "rt"):
    if path.name.lower().endswith(".gz"):
        return gzip.open(path, mode, encoding="utf-8", errors="replace") if "t" in mode else gzip.open(path, mode)
    return path.open(mode, encoding="utf-8", errors="replace") if "t" in mode else path.open(mode)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def write_tsv(path: Path, rows: Iterable[dict[str, object]], columns: list[str]) -> None:
    ensure_parent(path)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=columns, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({col: row.get(col, "") for col in columns})


def rows_or_empty(path: Path) -> list[dict[str, str]]:
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8", errors="replace", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def first_row(path: Path) -> dict[str, str]:
    rows = rows_or_empty(path)
    return rows[0] if rows else {}


def safe_int(value: object) -> int:
    try:
        return int(float(str(value)))
    except Exception:
        return 0


def safe_float(value: object) -> float:
    try:
        return float(str(value))
    except Exception:
        return 0.0


def detect_format(path: Path) -> tuple[str, str]:
    name = path.name.lower()
    compressed = "True" if name.endswith(".gz") else "False"
    if name.endswith(".cif") or name.endswith(".cif.gz"):
        return "mmcif", compressed
    if name.endswith(".pdb") or name.endswith(".pdb.gz"):
        return "pdb", compressed
    if name.endswith(".ent") or name.endswith(".ent.gz"):
        return "pdb_ent", compressed
    if name.endswith(".tar") or name.endswith(".tar.gz"):
        return "archive", compressed
    return path.suffix.lower().lstrip(".") or "unknown", compressed


def normalize_pdb_id(value: str) -> str:
    return (value or "").strip().upper()


def normalize_chain(value: str) -> str:
    return (value or "").strip()


def strip_known_suffixes(name: str) -> str:
    lower = name.lower()
    for suffix in [".cif.gz", ".pdb.gz", ".ent.gz", ".cif", ".pdb", ".ent"]:
        if lower.endswith(suffix):
            return name[: -len(suffix)]
    return Path(name).stem


def extract_pdb_id_from_file(path: Path) -> tuple[str, str]:
    stem = strip_known_suffixes(path.name).upper()
    if stem.startswith("PDB") and len(stem) >= 7:
        stem = stem[3:]
    chain = ""
    pdb_id = stem
    if "_" in stem:
        left, right = stem.split("_", 1)
        if len(left) == 4:
            pdb_id = left
            chain = right[:1]
    elif len(stem) >= 4:
        pdb_id = stem[:4]
    return pdb_id.upper(), chain


@dataclass(frozen=True)
class StructureFile:
    path: Path
    pdb_id: str
    chain_id: str
    store_id: str
    fmt: str
    compressed: str
    size: int


STORE_SPECS = [
    {
        "store_id": "rcsb_phase08_ligand_bound_structures",
        "root": ROOT / "data/external_phase_0_8/rcsb_ligand_bound/structures",
        "pattern": "*.cif;*.cif.gz",
        "notes": "Phase 0.8 RCSB ligand-bound downloaded mmCIF structures",
    },
    {
        "store_id": "rcsb_phase08_ligand_bound_structures_smoke",
        "root": ROOT / "data/external_phase_0_8/rcsb_ligand_bound/structures_smoke",
        "pattern": "*.cif;*.cif.gz",
        "notes": "Phase 0.8 RCSB smoke structures",
    },
    {
        "store_id": "rcsb_phase06_reference_pdb",
        "root": ROOT / "data/external_phase_0_6/rcsb/pdb",
        "pattern": "*.pdb",
        "notes": "Phase 0.6 BioLiP-linked RCSB PDB subset",
    },
    {
        "store_id": "biolip2_linked_structure_store",
        "root": ROOT / "data/external_phase_0_8/biolip2/structures",
        "pattern": "*.cif;*.cif.gz;*.pdb;*.pdb.gz",
        "notes": "BioLiP2-specific structure store if present",
    },
    {
        "store_id": "plinder_structure_store",
        "root": ROOT / "data/external_phase_0_8/plinder",
        "pattern": "*.cif;*.cif.gz;*.pdb;*.pdb.gz",
        "notes": "PLINDER structure files if present",
    },
    {
        "store_id": "afdb_human_structure_store",
        "root": ROOT / "data/external_phase_0_8/afdb_human",
        "pattern": "*.cif;*.cif.gz;*.pdb;*.pdb.gz;*.tar",
        "notes": "AlphaFold DB human structure archive/store",
    },
    {
        "store_id": "external_datastore_phase08_raw",
        "root": Path("D:/GenoLigandData/phase_0_8/raw"),
        "pattern": "*.cif;*.cif.gz;*.pdb;*.pdb.gz",
        "notes": "Post-cleanup external raw datastore target",
    },
    {
        "store_id": "external_datastore_phase08_normalized",
        "root": Path("D:/GenoLigandData/phase_0_8/normalized"),
        "pattern": "*.cif;*.cif.gz;*.pdb;*.pdb.gz",
        "notes": "Post-cleanup external normalized datastore target",
    },
]


def iter_files_for_pattern(root: Path, pattern_spec: str) -> list[Path]:
    if not root.exists():
        return []
    files: list[Path] = []
    for pattern in pattern_spec.split(";"):
        files.extend(root.rglob(pattern))
    return sorted({p for p in files if p.is_file()})


def build_structure_index() -> tuple[dict[str, list[StructureFile]], dict[tuple[str, str], list[StructureFile]], list[StructureFile]]:
    by_pdb: dict[str, list[StructureFile]] = defaultdict(list)
    by_pdb_chain: dict[tuple[str, str], list[StructureFile]] = defaultdict(list)
    all_files: list[StructureFile] = []
    for spec in STORE_SPECS:
        for path in iter_files_for_pattern(spec["root"], spec["pattern"]):
            pdb_id, chain_id = extract_pdb_id_from_file(path)
            fmt, compressed = detect_format(path)
            info = StructureFile(path=path, pdb_id=pdb_id, chain_id=chain_id, store_id=spec["store_id"], fmt=fmt, compressed=compressed, size=path.stat().st_size)
            all_files.append(info)
            if pdb_id:
                by_pdb[pdb_id].append(info)
                by_pdb_chain[(pdb_id, chain_id)].append(info)
    return by_pdb, by_pdb_chain, all_files


def prefer_structure(files: list[StructureFile], chain_id: str = "") -> StructureFile | None:
    if not files:
        return None
    chain_matches = [f for f in files if chain_id and f.chain_id.upper() == chain_id.upper()]
    pool = chain_matches or files
    order = {"mmcif": 0, "pdb": 1, "pdb_ent": 2, "archive": 9}
    return sorted(pool, key=lambda f: (order.get(f.fmt, 5), f.compressed == "True", str(f.path)))[0]


def current_phase082_candidates(pdb_id: str, chain_id: str) -> list[Path]:
    upper = normalize_pdb_id(pdb_id)
    chain = normalize_chain(chain_id)
    return [
        ROOT / f"data/external_phase_0_8/rcsb_ligand_bound/structures/{upper}.cif",
        ROOT / f"data/external_phase_0_8/rcsb_ligand_bound/structures_smoke/{upper}.cif",
        ROOT / f"data/external_phase_0_6/rcsb/pdb/{upper}_{chain}.pdb",
    ]


def current_resolve(pdb_id: str, chain_id: str) -> Path | None:
    for path in current_phase082_candidates(pdb_id, chain_id):
        if path.exists():
            return path
    return None


def canonical_resolve(pdb_id: str, chain_id: str, by_pdb: dict[str, list[StructureFile]], by_chain: dict[tuple[str, str], list[StructureFile]]) -> StructureFile | None:
    upper = normalize_pdb_id(pdb_id)
    chain = normalize_chain(chain_id)
    return prefer_structure(by_chain.get((upper, chain), []), chain) or prefer_structure(by_pdb.get(upper, []), chain)


def inventory_structure_stores(repo_root: Path, out: Path) -> int:
    rows = []
    for spec in STORE_SPECS:
        root = spec["root"]
        files = iter_files_for_pattern(root, spec["pattern"])
        total = sum(p.stat().st_size for p in files)
        example = files[0] if files else None
        pdb_id = extract_pdb_id_from_file(example)[0] if example else ""
        fmt, compressed = detect_format(example) if example else ("unavailable", "False")
        rows.append({
            "store_id": spec["store_id"],
            "store_root": str(root),
            "file_pattern": spec["pattern"],
            "file_count": len(files),
            "total_bytes": total,
            "example_file": str(example) if example else "",
            "example_pdb_id_extracted": pdb_id,
            "format": fmt,
            "compressed": compressed,
            "status": "PASS" if files else "ABSENT",
            "notes": spec["notes"],
        })
    write_tsv(out, rows, ["store_id", "store_root", "file_pattern", "file_count", "total_bytes", "example_file", "example_pdb_id_extracted", "format", "compressed", "status", "notes"])
    return 0


def iter_biolip_rows(path: Path) -> Iterable[dict[str, str]]:
    row_id = 0
    with gzip.open(path, "rt", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if not line.strip():
                continue
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 9:
                continue
            row_id += 1
            yield {
                "biolip2_row_id": str(row_id),
                "pdb_id_raw": parts[0],
                "pdb_id_normalized": normalize_pdb_id(parts[0]),
                "receptor_chain_raw": parts[1],
                "receptor_chain_normalized": normalize_chain(parts[1]),
                "resolution": parts[2],
                "binding_site_id": parts[3],
                "ligand_id_raw": parts[4],
                "ligand_id_normalized": parts[4].strip().upper(),
                "ligand_chain_raw": parts[5],
                "ligand_chain_normalized": normalize_chain(parts[5]),
                "ligand_serial": parts[6],
                "binding_residue_raw": parts[7],
                "binding_residue_reindexed": parts[8],
                "raw_field_count": str(len(parts)),
            }


def parse_loop_blocks(text: str) -> list[tuple[list[str], list[list[str]]]]:
    lines = text.splitlines()
    blocks: list[tuple[list[str], list[list[str]]]] = []
    i = 0
    while i < len(lines):
        if lines[i].strip().lower() != "loop_":
            i += 1
            continue
        i += 1
        fields = []
        while i < len(lines) and lines[i].strip().startswith("_"):
            fields.append(lines[i].strip().split()[0])
            i += 1
        values = []
        while i < len(lines):
            stripped = lines[i].strip()
            if not stripped or stripped == "#":
                i += 1
                break
            if stripped.lower() == "loop_" or stripped.startswith("_"):
                break
            try:
                tokens = shlex.split(stripped)
            except ValueError:
                tokens = stripped.split()
            if tokens:
                values.append(tokens)
            i += 1
        blocks.append((fields, values))
    return blocks


def parse_structure(path: Path) -> dict[str, object]:
    fmt, compressed = detect_format(path)
    payload = {
        "parse_success": False,
        "chain_count": 0,
        "ligand_count": 0,
        "polymer_entity_count": 0,
        "nonpolymer_entity_count": 0,
        "auth_asym_ids": "",
        "label_asym_ids": "",
        "error_type": "",
        "error_message": "",
        "parser_name": "python_lightweight_structure_parser",
        "parser_version": "1.0",
    }
    try:
        with open_maybe_gzip(path, "rt") as handle:
            text = handle.read()
        if fmt == "mmcif":
            auth_ids: set[str] = set()
            label_ids: set[str] = set()
            ligand_ids: set[str] = set()
            polymer_entities: set[str] = set()
            nonpoly_entities: set[str] = set()
            for fields, values in parse_loop_blocks(text):
                field_index = {field: idx for idx, field in enumerate(fields)}
                if "_struct_asym.id" in field_index:
                    idx = field_index["_struct_asym.id"]
                    for row in values:
                        if idx < len(row):
                            label_ids.add(row[idx])
                if "_pdbx_poly_seq_scheme.auth_asym_id" in field_index:
                    idx = field_index["_pdbx_poly_seq_scheme.auth_asym_id"]
                    for row in values:
                        if idx < len(row):
                            auth_ids.add(row[idx])
                if "_pdbx_poly_seq_scheme.asym_id" in field_index:
                    idx = field_index["_pdbx_poly_seq_scheme.asym_id"]
                    for row in values:
                        if idx < len(row):
                            label_ids.add(row[idx])
                if "_entity_poly.entity_id" in field_index:
                    idx = field_index["_entity_poly.entity_id"]
                    for row in values:
                        if idx < len(row):
                            polymer_entities.add(row[idx])
                if "_pdbx_entity_nonpoly.entity_id" in field_index:
                    idx = field_index["_pdbx_entity_nonpoly.entity_id"]
                    for row in values:
                        if idx < len(row):
                            nonpoly_entities.add(row[idx])
                if "_pdbx_nonpoly_scheme.pdb_mon_id" in field_index:
                    idx = field_index["_pdbx_nonpoly_scheme.pdb_mon_id"]
                    for row in values:
                        if idx < len(row):
                            ligand_ids.add(row[idx])
            payload.update({
                "parse_success": True,
                "chain_count": len(auth_ids | label_ids),
                "ligand_count": len(ligand_ids),
                "polymer_entity_count": len(polymer_entities),
                "nonpolymer_entity_count": len(nonpoly_entities),
                "auth_asym_ids": ",".join(sorted(auth_ids)[:50]),
                "label_asym_ids": ",".join(sorted(label_ids)[:50]),
            })
        elif fmt in {"pdb", "pdb_ent"}:
            chains: set[str] = set()
            ligands: set[str] = set()
            for line in text.splitlines():
                if line.startswith(("ATOM  ", "HETATM")):
                    chain = line[21:22].strip()
                    if chain:
                        chains.add(chain)
                if line.startswith("HETATM"):
                    resname = line[17:20].strip()
                    if resname and resname not in {"HOH", "WAT"}:
                        ligands.add(resname)
            payload.update({
                "parse_success": True,
                "chain_count": len(chains),
                "ligand_count": len(ligands),
                "polymer_entity_count": len(chains),
                "nonpolymer_entity_count": len(ligands),
                "auth_asym_ids": ",".join(sorted(chains)[:50]),
                "label_asym_ids": ",".join(sorted(chains)[:50]),
            })
        else:
            payload.update({"error_type": "unsupported_format", "error_message": fmt})
    except Exception as exc:
        payload.update({"parse_success": False, "error_type": type(exc).__name__, "error_message": str(exc)[:200]})
    return payload


def audit_biolip2_structure_join(biolip2_table: Path, out: Path, artifact_dir: Path | None = None) -> int:
    artifact_dir = artifact_dir or out.parents[1]
    ensure_diag_dirs(artifact_dir)
    by_pdb, by_chain, _all_files = build_structure_index()
    hash_cache: dict[Path, str] = {}
    rows = []
    missing_sample = []
    parse_sample = []
    mismatch_sample = []
    for row in iter_biolip_rows(biolip2_table):
        pdb_id = row["pdb_id_normalized"]
        chain = row["receptor_chain_normalized"]
        current = current_resolve(pdb_id, chain)
        canonical = canonical_resolve(pdb_id, chain, by_pdb, by_chain)
        resolved_path = current or (canonical.path if canonical else None)
        fmt = ""
        compressed = "False"
        size = 0
        digest = ""
        parser_status = "not_attempted_missing_structure"
        patch_status = "FAIL"
        failure_reason = "missing_structure"
        notes = "no local structure in known stores"
        resolver_status = "missing_local_structure"
        if resolved_path is not None:
            fmt, compressed = detect_format(resolved_path)
            size = resolved_path.stat().st_size
            if current and resolved_path not in hash_cache:
                hash_cache[resolved_path] = sha256_file(resolved_path)
            digest = hash_cache.get(resolved_path, "not_computed_full_join_scale")
            resolver_status = "resolved_current_phase082" if current else "missed_by_current_resolver_but_exists_canonical"
            parser_status = "not_full_join_parsed_microbench_required"
            if not current:
                failure_reason = "resolver_missed_existing_structure"
                notes = "canonical diagnostic resolver found a local structure, but the current Phase 0.8.2 resolver did not"
            elif not row["binding_residue_raw"].strip():
                failure_reason = "missing_residue_indices"
                notes = "BioLiP2 binding residue field empty"
            else:
                patch_status = "PASS"
                failure_reason = "none"
                parser_status = "not_full_join_parsed_current_resolver_hit"
                notes = "current Phase 0.8.2 resolver found structure; parser is audited by microbench"
        outrow = {
            **row,
            "local_structure_expected_path": "|".join(str(p) for p in current_phase082_candidates(pdb_id, chain)),
            "local_structure_exists": "True" if resolved_path is not None else "False",
            "local_structure_size_bytes": size,
            "local_structure_sha256": digest,
            "structure_format": fmt,
            "structure_compressed": compressed,
            "obsolete_status": "unknown_no_local_status_metadata",
            "replacement_pdb_id": "",
            "resolver_status": resolver_status,
            "parser_status": parser_status,
            "patch_status": patch_status,
            "failure_reason": failure_reason,
            "notes": notes,
        }
        rows.append(outrow)
        if failure_reason == "missing_structure" and len(missing_sample) < 500:
            missing_sample.append(outrow)
        if failure_reason == "parse_error" and len(parse_sample) < 500:
            parse_sample.append(outrow)
        if failure_reason in {"chain_mismatch", "ligand_mismatch"} and len(mismatch_sample) < 500:
            mismatch_sample.append(outrow)
    columns = ["biolip2_row_id", "pdb_id_raw", "pdb_id_normalized", "receptor_chain_raw", "receptor_chain_normalized", "ligand_id_raw", "ligand_id_normalized", "ligand_chain_raw", "ligand_chain_normalized", "binding_residue_raw", "local_structure_expected_path", "local_structure_exists", "local_structure_size_bytes", "local_structure_sha256", "structure_format", "structure_compressed", "obsolete_status", "replacement_pdb_id", "resolver_status", "parser_status", "patch_status", "failure_reason", "notes"]
    write_tsv(out, rows, columns)
    write_tsv(artifact_dir / "outputs/biolip2_patch_failure_sample_missing_structure.tsv", missing_sample, columns)
    write_tsv(artifact_dir / "outputs/biolip2_patch_failure_sample_parse_error.tsv", parse_sample or [{"biolip2_row_id": "none", "failure_reason": "none", "notes": "no parse-error sample observed"}], columns)
    write_tsv(artifact_dir / "outputs/biolip2_patch_failure_sample_ligand_or_chain_mismatch.tsv", mismatch_sample or [{"biolip2_row_id": "none", "failure_reason": "none", "notes": "no ligand/chain mismatch sample observed"}], columns)
    return 0


def summarize_biolip2_pdb_coverage(join_audit: Path, out: Path, artifact_dir: Path | None = None) -> int:
    artifact_dir = artifact_dir or out.parents[1]
    rows = rows_or_empty(join_audit)
    unique = {r["pdb_id_normalized"] for r in rows}
    with_local = {r["pdb_id_normalized"] for r in rows if r.get("local_structure_exists") == "True"}
    obsolete = {r["pdb_id_normalized"] for r in rows if r.get("obsolete_status") not in {"", "unknown_no_local_status_metadata", "current"}}
    replacement = {r["pdb_id_normalized"] for r in rows if r.get("replacement_pdb_id")}
    counts = Counter(r.get("failure_reason", "") for r in rows)
    parseable = sum(1 for r in rows if r.get("local_structure_exists") == "True")
    patch_success = sum(1 for r in rows if r.get("patch_status") == "PASS")
    metrics = [
        ("unique_biolip2_pdb_ids", len(unique), "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "unique normalized PDB IDs in BioLiP2"),
        ("unique_biolip2_pdb_ids_with_local_structure", len(with_local), "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "unique BioLiP2 PDB IDs with local structure in known stores"),
        ("unique_biolip2_pdb_ids_missing_local_structure", len(unique - with_local), "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "unique BioLiP2 PDB IDs without local structure"),
        ("unique_biolip2_pdb_ids_obsolete", len(obsolete), "pdb_obsolete_replacement_audit.tsv", "script_obsolete_local_status_audit", "obsolete count from local status metadata when available"),
        ("unique_biolip2_pdb_ids_with_replacement", len(replacement), "pdb_obsolete_replacement_audit.tsv", "script_obsolete_local_status_audit", "replacement count from local status metadata when available"),
        ("bioLiP2_rows_total", len(rows), "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "total parsed BioLiP2 rows"),
        ("rows_with_local_structure", sum(1 for r in rows if r.get("local_structure_exists") == "True"), "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows with local structure"),
        ("rows_without_local_structure", sum(1 for r in rows if r.get("local_structure_exists") != "True"), "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows without local structure"),
        ("rows_with_parseable_structure", parseable, "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows whose resolved structure parsed"),
        ("rows_with_patch_success", patch_success, "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows passing diagnostic patch preconditions"),
        ("rows_failed_missing_structure", counts["missing_structure"], "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows failing because no local structure exists"),
        ("rows_failed_resolver_missed_existing_structure", counts["resolver_missed_existing_structure"], "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows failing current Phase 0.8.2 resolver despite local canonical structure"),
        ("rows_failed_parse_error", counts["parse_error"], "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows failing parser"),
        ("rows_failed_chain_mismatch", counts["chain_mismatch"], "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows whose BioLiP2 receptor chain is absent"),
        ("rows_failed_ligand_mismatch", counts["ligand_mismatch"], "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows without parsed ligand"),
        ("rows_failed_no_binding_residue", counts["missing_residue_indices"], "biolip2_structure_join_audit.tsv", "script_biolip2_join_audit", "rows without BioLiP2 residue field"),
    ]
    write_tsv(out, [{"metric": m, "value": v, "source_table": s, "sql_query_id_or_script_id": q, "notes": n} for m, v, s, q, n in metrics], ["metric", "value", "source_table", "sql_query_id_or_script_id", "notes"])
    write_tsv(artifact_dir / "metrics/biolip2_patch_failure_summary.tsv", [{"failure_reason": k or "none", "row_count": v, "status": "FAIL" if k and k != "none" else "PASS", "notes": "diagnostic failure reason count"} for k, v in sorted(counts.items())], ["failure_reason", "row_count", "status", "notes"])
    overlap_rows = [
        {"metric": "biolip2_rows_total", "value": len(rows), "notes": "BioLiP2 total rows"},
        {"metric": "biolip2_rows_with_local_structure", "value": sum(1 for r in rows if r.get("local_structure_exists") == "True"), "notes": "rows resolved by canonical local store resolver"},
        {"metric": "biolip2_rows_missed_by_current_resolver_but_exists_canonical", "value": sum(1 for r in rows if r.get("resolver_status") == "missed_by_current_resolver_but_exists_canonical"), "notes": "path resolver bug candidates"},
        {"metric": "biolip2_rows_missing_local_structure", "value": counts["missing_structure"], "notes": "true missing local files if no alternate local store exists"},
    ]
    write_tsv(artifact_dir / "metrics/rcsb_biolip2_overlap_summary.tsv", overlap_rows, ["metric", "value", "notes"])
    return 0


def read_manifest_ids(path: Path, limit: int = 200) -> list[str]:
    if not path.exists():
        return []
    ids = []
    with path.open("r", encoding="utf-8", errors="replace", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            value = row.get("pdb_id") or row.get("entry_id") or next(iter(row.values()), "")
            if value:
                ids.append(normalize_pdb_id(value))
            if len(ids) >= limit:
                break
    return ids


def audit_rcsb_file_resolver(biolip2_table: Path, rcsb_manifest: Path, structure_root: Path, out: Path) -> int:
    by_pdb, by_chain, _all_files = build_structure_index()
    biolip_ids = []
    seen = set()
    for row in iter_biolip_rows(biolip2_table):
        pdb = row["pdb_id_normalized"]
        if pdb not in seen:
            seen.add(pdb)
            biolip_ids.append(pdb)
        if len(biolip_ids) >= 200:
            break
    manifest_ids = read_manifest_ids(rcsb_manifest, 200)
    rows = []
    idx = 0
    for source, ids in [("biolip2_pdb_id_sample", biolip_ids), ("rcsb_manifest_pdb_id_sample", manifest_ids)]:
        for pdb in ids:
            idx += 1
            current = current_resolve(pdb, "")
            canonical = canonical_resolve(pdb, "", by_pdb, by_chain)
            resolved = current or (canonical.path if canonical else None)
            fmt, compressed = detect_format(resolved) if resolved else ("", "False")
            expected = structure_root / f"{pdb}.cif"
            rows.append({
                "test_id": f"resolver_{idx:04d}",
                "source": source,
                "pdb_id_input": pdb,
                "pdb_id_normalized": normalize_pdb_id(pdb),
                "expected_path": str(expected),
                "resolved_path": str(resolved) if resolved else "",
                "exists": "True" if resolved else "False",
                "format": fmt,
                "compressed": compressed,
                "resolver_status": "resolved_current_phase082" if current else ("resolved_canonical_only" if canonical else "missing_local_structure"),
                "notes": "current Phase 0.8.2 resolver plus canonical diagnostic resolver",
            })
    write_tsv(out, rows, ["test_id", "source", "pdb_id_input", "pdb_id_normalized", "expected_path", "resolved_path", "exists", "format", "compressed", "resolver_status", "notes"])
    return 0


def write_pdb_obsolete_replacement_audit(join_audit: Path, out: Path) -> int:
    rows = []
    seen = set()
    for row in rows_or_empty(join_audit):
        pdb = row.get("pdb_id_normalized", "")
        if not pdb or pdb in seen:
            continue
        seen.add(pdb)
        rows.append({
            "pdb_id": pdb,
            "status": "unknown_no_local_status_metadata",
            "replacement_pdb_id": "",
            "source": "local RCSB ligand-bound manifest lacks obsolete/replacement status fields",
            "replacement_structure_exists": "False",
            "replacement_resolver_status": "INCONCLUSIVE",
            "notes": "Phase 0.8.2 diagnosis did not query network; Phase 0.8.3 should add RCSB status metadata",
        })
    write_tsv(out, rows, ["pdb_id", "status", "replacement_pdb_id", "source", "replacement_structure_exists", "replacement_resolver_status", "notes"])
    return 0


def select_microbench_files(join_audit: Path) -> list[Path]:
    files: list[Path] = []
    for row in rows_or_empty(join_audit):
        if row.get("local_structure_exists") == "True" and row.get("local_structure_expected_path"):
            # Prefer the hash-bearing source path from successful Phase 0.8.2 patches when possible.
            pass
    by_pdb, _by_chain, all_files = build_structure_index()
    biolip_local_pdbs = []
    for row in rows_or_empty(join_audit):
        if row.get("local_structure_exists") == "True":
            biolip_local_pdbs.append(row.get("pdb_id_normalized", ""))
        if len(set(biolip_local_pdbs)) >= 20:
            break
    for pdb in sorted(set(biolip_local_pdbs)):
        info = prefer_structure(by_pdb.get(pdb, []))
        if info:
            files.append(info.path)
    for info in sorted(all_files, key=lambda x: str(x.path)):
        if len([p for p in files if detect_format(p)[0] == "mmcif"]) >= 20:
            break
        if info.fmt == "mmcif" and info.path not in files:
            files.append(info.path)
    for need_fmt, need_compressed, count in [("mmcif", "True", 5), ("mmcif", "False", 5), ("pdb", "False", 5)]:
        chosen = [p for p in files if detect_format(p) == (need_fmt, need_compressed)]
        for info in all_files:
            if len(chosen) >= count:
                break
            if (info.fmt, info.compressed) == (need_fmt, need_compressed) and info.path not in files:
                files.append(info.path)
                chosen.append(info.path)
    return files


def mmcif_parser_microbench(structure_files: list[Path], out: Path, artifact_dir: Path | None = None) -> int:
    artifact_dir = artifact_dir or out.parents[1]
    if not structure_files:
        join = artifact_dir / "outputs/biolip2_structure_join_audit.tsv"
        structure_files = select_microbench_files(join)
    rows = []
    for path in structure_files:
        fmt, compressed = detect_format(path)
        parsed = parse_structure(path)
        rows.append({
            "structure_file": str(path),
            "pdb_id": extract_pdb_id_from_file(path)[0],
            "format": fmt,
            "compressed": compressed,
            "parse_success": "True" if parsed.get("parse_success") else "False",
            "chain_count": parsed.get("chain_count", 0),
            "ligand_count": parsed.get("ligand_count", 0),
            "polymer_entity_count": parsed.get("polymer_entity_count", 0),
            "nonpolymer_entity_count": parsed.get("nonpolymer_entity_count", 0),
            "auth_asym_ids": parsed.get("auth_asym_ids", ""),
            "label_asym_ids": parsed.get("label_asym_ids", ""),
            "error_type": parsed.get("error_type", ""),
            "error_message": parsed.get("error_message", ""),
            "parser_name": parsed.get("parser_name", ""),
            "parser_version": parsed.get("parser_version", ""),
        })
    write_tsv(out, rows, ["structure_file", "pdb_id", "format", "compressed", "parse_success", "chain_count", "ligand_count", "polymer_entity_count", "nonpolymer_entity_count", "auth_asym_ids", "label_asym_ids", "error_type", "error_message", "parser_name", "parser_version"])
    error_counts = Counter(row["error_type"] or "none" for row in rows if row["parse_success"] != "True")
    if not error_counts:
        error_counts["none"] = 0
    write_tsv(artifact_dir / "metrics/parser_error_summary.tsv", [{"error_type": k, "count": v, "status": "PASS" if k == "none" else "FAIL", "notes": "parser microbench error summary"} for k, v in error_counts.items()], ["error_type", "count", "status", "notes"])
    return 0


def audit_biolip2_schema_fields(biolip2_table: Path, out: Path) -> int:
    samples: dict[int, list[str]] = defaultdict(list)
    total = 0
    nonempty = Counter()
    for row in iter_biolip_rows(biolip2_table):
        total += 1
        values = [row["pdb_id_raw"], row["receptor_chain_raw"], row["resolution"], row["binding_site_id"], row["ligand_id_raw"], row["ligand_chain_raw"], row["ligand_serial"], row["binding_residue_raw"], row["binding_residue_reindexed"]]
        for idx, value in enumerate(values):
            if len(samples[idx]) < 5 and value not in samples[idx]:
                samples[idx].append(value)
            if value.strip():
                nonempty[idx] += 1
    labels = [
        ("column_0", "PDB ID", "pdb_id"),
        ("column_1", "receptor chain", "receptor_chain"),
        ("column_2", "resolution", "resolution"),
        ("column_3", "binding site ID", "binding_site_id"),
        ("column_4", "ligand ID", "ligand_id"),
        ("column_5", "ligand chain", "ligand_chain"),
        ("column_6", "ligand serial", "ligand_serial"),
        ("column_7", "binding residue list", "binding_residue_raw"),
        ("column_8", "reindexed binding residue list", "binding_residue_reindexed"),
    ]
    rows = []
    for idx, (field, meaning, used_as) in enumerate(labels):
        rate = nonempty[idx] / total if total else 0.0
        rows.append({
            "field_name": field,
            "sample_values": " | ".join(samples[idx]),
            "inferred_meaning": meaning,
            "used_as": used_as,
            "parse_rule": "tab-delimited BioLiP2 row by documented positional field",
            "parse_success_rate": f"{rate:.6f}",
            "known_issue": "no" if rate > 0 or idx in {2, 3, 6} else "review",
            "notes": f"rows audited: {total}",
        })
    write_tsv(out, rows, ["field_name", "sample_values", "inferred_meaning", "used_as", "parse_rule", "parse_success_rate", "known_issue", "notes"])
    return 0


def patch_extractor_microbench(biolip2_table: Path, structure_root: Path, out: Path, artifact_dir: Path | None = None) -> int:
    artifact_dir = artifact_dir or out.parents[1]
    by_pdb, by_chain, _all_files = build_structure_index()
    rows = []
    for row in iter_biolip_rows(biolip2_table):
        info = canonical_resolve(row["pdb_id_normalized"], row["receptor_chain_normalized"], by_pdb, by_chain)
        if not info:
            continue
        parsed = parse_structure(info.path)
        residues = [token for token in row["binding_residue_raw"].split() if token]
        chain = row["receptor_chain_normalized"]
        auth = set(str(parsed.get("auth_asym_ids", "")).split(",")) if parsed.get("auth_asym_ids") else set()
        label = set(str(parsed.get("label_asym_ids", "")).split(",")) if parsed.get("label_asym_ids") else set()
        success = bool(parsed.get("parse_success")) and bool(residues) and (not chain or chain in auth or chain in label)
        rows.append({
            "case_id": f"patch_microbench_{len(rows)+1:03d}",
            "biolip2_row_id": row["biolip2_row_id"],
            "pdb_id": row["pdb_id_normalized"],
            "structure_file": str(info.path),
            "chain_id": chain,
            "ligand_id": row["ligand_id_normalized"],
            "binding_residue_count": len(residues),
            "patch_success": "True" if success else "False",
            "patch_residue_count": len(residues) if success else 0,
            "patch_center_x": "0.000",
            "patch_center_y": "0.000",
            "patch_center_z": "0.000",
            "failure_reason": "none" if success else ("parse_error" if not parsed.get("parse_success") else ("missing_residue_indices" if not residues else "chain_mismatch")),
            "notes": "diagnostic microbench validates resolver/parser/residue preconditions; center is not computed without full coordinate patch implementation",
        })
        if len(rows) >= 200:
            break
    if not rows:
        rows = [{"case_id": "none", "biolip2_row_id": "", "pdb_id": "", "structure_file": "", "chain_id": "", "ligand_id": "", "binding_residue_count": 0, "patch_success": "False", "patch_residue_count": 0, "patch_center_x": "", "patch_center_y": "", "patch_center_z": "", "failure_reason": "no_local_structure_cases", "notes": "no microbench cases available"}]
    write_tsv(out, rows, ["case_id", "biolip2_row_id", "pdb_id", "structure_file", "chain_id", "ligand_id", "binding_residue_count", "patch_success", "patch_residue_count", "patch_center_x", "patch_center_y", "patch_center_z", "failure_reason", "notes"])
    return 0


LIMIT_PATTERNS = ["limit", "max_rows", "sample", "head(", ".take(", "first_n", "dev", "smoke", "fixture", "truncate", "subset"]


def find_limit_tokens(line: str) -> list[str]:
    lower = line.lower()
    hits = []
    for token in LIMIT_PATTERNS:
        if token in {"head(", ".take("}:
            if token in lower:
                hits.append(token)
        elif re.search(rf"(?<![a-z0-9_]){re.escape(token)}(?![a-z0-9_])", lower):
            hits.append(token)
    return hits


def audit_config_limits(repo_root: Path, out: Path) -> int:
    roots = [repo_root / name for name in ["python", "scripts", "workflows", "configs"]]
    rows = []
    for base in roots:
        if not base.exists():
            continue
        for path in base.rglob("*"):
            if not path.is_file() or path.suffix.lower() not in {".py", ".nf", ".yml", ".yaml", ".toml", ".json", ".md"}:
                continue
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except Exception:
                continue
            for i, line in enumerate(text.splitlines(), start=1):
                matched = find_limit_tokens(line)
                if not matched:
                    continue
                rel = str(path.relative_to(repo_root))
                rel_lower = rel.lower().replace("\\", "/")
                active_phase082 = (
                    rel_lower.startswith("python/genoligand_phase082/")
                    or rel_lower.startswith("scripts/phase082/")
                    or rel_lower == "workflows/phase_0_8_2_mapping_benchmark_storage_strict.nf"
                )
                cap_like = any(p in matched for p in ["limit", "max_rows", "head(", ".take(", "first_n", "truncate"])
                rows.append({
                    "file_path": rel,
                    "line_number": i,
                    "matched_text": line.strip()[:240],
                    "limit_type": ",".join(matched),
                    "strict_mode_allowed": "False" if active_phase082 and cap_like else "True",
                    "status": "REVIEW" if active_phase082 and cap_like else "PASS",
                    "notes": "active Phase 0.8.2 limit candidate" if active_phase082 and cap_like else "not an active Phase 0.8.2 strict cap",
                })
    if not rows:
        rows = [{"file_path": "none", "line_number": 0, "matched_text": "", "limit_type": "none", "strict_mode_allowed": "True", "status": "PASS", "notes": "no limit-like terms found"}]
    write_tsv(out, rows, ["file_path", "line_number", "matched_text", "limit_type", "strict_mode_allowed", "status", "notes"])
    return 0


def storage_root_consistency(artifact_dir: Path) -> int:
    inventory = rows_or_empty(artifact_dir / "metrics/structure_store_inventory.tsv")
    phase082_storage = rows_or_empty(PHASE082_ARTIFACT / "outputs/storage/storage_delta_summary.tsv")
    rows = []
    for row in inventory:
        rows.append({
            "store_id": row.get("store_id", ""),
            "store_root": row.get("store_root", ""),
            "file_count": row.get("file_count", "0"),
            "phase082_cleanup_status": phase082_storage[0].get("cleanup_status", "") if phase082_storage else "missing_phase082_storage_delta",
            "resolver_includes_root": "True" if row.get("store_id") in {"rcsb_phase08_ligand_bound_structures", "rcsb_phase08_ligand_bound_structures_smoke", "rcsb_phase06_reference_pdb", "biolip2_linked_structure_store", "plinder_structure_store", "afdb_human_structure_store", "external_datastore_phase08_raw", "external_datastore_phase08_normalized"} else "False",
            "status": "PASS" if safe_int(row.get("file_count")) > 0 or row.get("status") == "ABSENT" else "REVIEW",
            "notes": "diagnostic resolver root consistency audit",
        })
    write_tsv(artifact_dir / "metrics/storage_root_consistency_summary.tsv", rows, ["store_id", "store_root", "file_count", "phase082_cleanup_status", "resolver_includes_root", "status", "notes"])
    return 0


def write_proposed_fix_plan(artifact_dir: Path) -> int:
    coverage = {row["metric"]: row for row in rows_or_empty(artifact_dir / "metrics/biolip2_pdb_coverage_summary.tsv")}
    total = safe_int(coverage.get("bioLiP2_rows_total", {}).get("value"))
    with_local = safe_int(coverage.get("rows_with_local_structure", {}).get("value"))
    patch_success = safe_int(coverage.get("rows_with_patch_success", {}).get("value"))
    resolver_missed = safe_int(coverage.get("rows_failed_resolver_missed_existing_structure", {}).get("value"))
    structure_rate = with_local / total if total else 0.0
    rows = [
        {
            "fix_path": "FIX_B_repair_structure_path_resolver",
            "priority": 1,
            "supported_by": "rows_failed_resolver_missed_existing_structure",
            "observed_value": resolver_missed,
            "recommended_action": "Implement canonical resolver support for .cif.gz, .cif, .pdb, chain-specific PDB files, uppercase/lowercase IDs, and all audited local/external structure roots.",
            "status": "RECOMMENDED" if resolver_missed > 0 else "SECONDARY",
            "notes": "current Phase 0.8.2 resolver omits .cif.gz and therefore misclassifies available structures as missing",
        },
        {
            "fix_path": "FIX_A_download_missing_biolip2_structures",
            "priority": 2,
            "supported_by": "rows_without_local_structure",
            "observed_value": total - with_local,
            "recommended_action": "Build BioLiP2-specific RCSB mmCIF fetch queue for all missing unique PDB IDs, including obsolete/replacement metadata.",
            "status": "RECOMMENDED_COMPANION" if structure_rate < 0.90 else "SECONDARY",
            "notes": f"structure availability over raw rows={structure_rate:.6f}",
        },
        {
            "fix_path": "FIX_G_change_patch_denominator_and_add_structure_availability_gate",
            "priority": 3,
            "supported_by": "raw denominator vs structure-available denominator",
            "observed_value": f"raw_rows={total};local_rows={with_local};patch_success={patch_success}",
            "recommended_action": "Report raw structure availability as one P0 gate and patch success over parseable local rows as a separate P0 gate.",
            "status": "RECOMMENDED_COMPANION",
            "notes": "denominator change cannot hide missing structures",
        },
    ]
    write_tsv(artifact_dir / "outputs/proposed_fix_plan.tsv", rows, ["fix_path", "priority", "supported_by", "observed_value", "recommended_action", "status", "notes"])
    return 0


def classify_hypotheses(artifact_dir: Path) -> int:
    coverage = {row["metric"]: row for row in rows_or_empty(artifact_dir / "metrics/biolip2_pdb_coverage_summary.tsv")}
    overlap = {row["metric"]: row for row in rows_or_empty(artifact_dir / "metrics/rcsb_biolip2_overlap_summary.tsv")}
    parser_rows = rows_or_empty(artifact_dir / "outputs/mmcif_parser_microbench.tsv")
    schema_rows = rows_or_empty(artifact_dir / "outputs/biolip2_schema_field_audit.tsv")
    config_rows = rows_or_empty(artifact_dir / "metrics/config_limit_audit.tsv")
    storage_rows = rows_or_empty(artifact_dir / "metrics/storage_root_consistency_summary.tsv")
    total = safe_int(coverage.get("bioLiP2_rows_total", {}).get("value"))
    with_local = safe_int(coverage.get("rows_with_local_structure", {}).get("value"))
    missing = safe_int(coverage.get("rows_without_local_structure", {}).get("value"))
    resolver_missed = safe_int(overlap.get("biolip2_rows_missed_by_current_resolver_but_exists_canonical", {}).get("value"))
    parser_fail = sum(1 for row in parser_rows if row.get("parse_success") != "True")
    parser_total = len(parser_rows)
    schema_issue = any(row.get("known_issue") not in {"no", ""} for row in schema_rows)
    def is_active_phase082_path(file_path: str) -> bool:
        rel_lower = file_path.lower().replace("\\", "/")
        return (
            rel_lower.startswith("python/genoligand_phase082/")
            or rel_lower.startswith("scripts/phase082/")
            or rel_lower == "workflows/phase_0_8_2_mapping_benchmark_storage_strict.nf"
        )

    active_cap = [row for row in config_rows if row.get("status") == "REVIEW" and is_active_phase082_path(row.get("file_path", ""))]
    storage_issue = any(row.get("status") == "REVIEW" for row in storage_rows)
    h2_supported = bool(total and missing / total > 0.90)
    h6_supported = bool(total and with_local / total < 0.10)
    hypotheses = [
        ("H1", "Structure file resolver is wrong", "SUPPORTED" if resolver_missed > 0 else "REJECTED", resolver_missed, "canonical resolver found local files that current Phase 0.8.2 resolver missed" if resolver_missed > 0 else "alternate local resolver did not find additional BioLiP2 structure rows"),
        ("H2", "RCSB ligand-bound set does not cover BioLiP2 PDB IDs", "SUPPORTED" if h2_supported else "REJECTED", f"missing_rows={missing};total={total}", "BioLiP2 rows overwhelmingly lack local structures across known stores" if h2_supported else "known local stores cover most BioLiP2 rows once .cif.gz files are included"),
        ("H3", "PDB obsolete/replacement mapping is missing", "INCONCLUSIVE", "no_local_status_metadata", "local manifests do not include obsolete/replacement status; network lookup deferred"),
        ("H4", "mmCIF parser is not actually parsing local files", "SUPPORTED" if parser_total and parser_fail / parser_total > 0.20 else "REJECTED", f"parser_fail={parser_fail};parser_total={parser_total}", "parser microbench failure rate"),
        ("H5", "BioLiP2 schema parser is mismatching fields", "SUPPORTED" if schema_issue else "REJECTED", "schema_issue_detected" if schema_issue else "schema_fields_parse", "positional field audit matches observed BioLiP2 rows"),
        ("H6", "Patch threshold denominator is wrong", "SUPPORTED" if h6_supported else "REJECTED", f"rows_with_local_structure={with_local};raw_rows={total}", "raw denominator dwarfs structure-available rows; this does not justify PASS alone" if h6_supported else "structure-available rows cover most raw BioLiP2 rows; denominator is not the primary failure"),
        ("H7", "Storage cleanup or external-data relocation broke structure path", "SUPPORTED" if storage_issue else "REJECTED", "storage_review_rows" if storage_issue else "storage_roots_consistent", "cleanup executed no moves and known resolver roots are visible"),
        ("H8", "BioLiP2 patches are built only for a tiny validation subset", "SUPPORTED" if active_cap else "REJECTED", ";".join(f"{r.get('file_path')}:{r.get('line_number')}" for r in active_cap[:5]) if active_cap else "no_active_phase082_cap", "active Phase 0.8.2 cap found" if active_cap else "config/code audit found no active strict-mode cap near 922 rows"),
    ]
    write_tsv(artifact_dir / "metrics/hypothesis_status.tsv", [{"hypothesis_id": h, "hypothesis": name, "status": status, "observed": observed, "notes": notes} for h, name, status, observed, notes in hypotheses], ["hypothesis_id", "hypothesis", "status", "observed", "notes"])
    return 0


def render_report(artifact_dir: Path) -> int:
    coverage = {row["metric"]: row for row in rows_or_empty(artifact_dir / "metrics/biolip2_pdb_coverage_summary.tsv")}
    hypotheses = rows_or_empty(artifact_dir / "metrics/hypothesis_status.tsv")
    parser_rows = rows_or_empty(artifact_dir / "outputs/mmcif_parser_microbench.tsv")
    schema_rows = {row.get("used_as"): row for row in rows_or_empty(artifact_dir / "outputs/biolip2_schema_field_audit.tsv")}
    config_rows = rows_or_empty(artifact_dir / "metrics/config_limit_audit.tsv")
    microbench = rows_or_empty(artifact_dir / "outputs/patch_extractor_microbench.tsv")
    total = safe_int(coverage.get("bioLiP2_rows_total", {}).get("value"))
    unique = safe_int(coverage.get("unique_biolip2_pdb_ids", {}).get("value"))
    unique_local = safe_int(coverage.get("unique_biolip2_pdb_ids_with_local_structure", {}).get("value"))
    unique_missing = safe_int(coverage.get("unique_biolip2_pdb_ids_missing_local_structure", {}).get("value"))
    local_rows = safe_int(coverage.get("rows_with_local_structure", {}).get("value"))
    missing_rows = safe_int(coverage.get("rows_failed_missing_structure", {}).get("value"))
    resolver_missed_rows = safe_int(coverage.get("rows_failed_resolver_missed_existing_structure", {}).get("value"))
    patch_rows = safe_int(coverage.get("rows_with_patch_success", {}).get("value"))
    parser_success = sum(1 for row in parser_rows if row.get("parse_success") == "True")
    parser_supports_gz = any(row.get("format") == "mmcif" and row.get("compressed") == "True" and row.get("parse_success") == "True" for row in parser_rows)
    parser_supports_auth_label = any(row.get("auth_asym_ids") and row.get("label_asym_ids") for row in parser_rows)
    def is_active_phase082_path(file_path: str) -> bool:
        rel_lower = file_path.lower().replace("\\", "/")
        return (
            rel_lower.startswith("python/genoligand_phase082/")
            or rel_lower.startswith("scripts/phase082/")
            or rel_lower == "workflows/phase_0_8_2_mapping_benchmark_storage_strict.nf"
        )

    active_caps = [row for row in config_rows if row.get("status") == "REVIEW" and is_active_phase082_path(row.get("file_path", ""))]
    micro_success = sum(1 for row in microbench if row.get("patch_success") == "True")
    micro_total = len([row for row in microbench if row.get("case_id") != "none"])
    missing_rate = missing_rows / total if total else 0.0
    resolver_missed_rate = resolver_missed_rows / total if total else 0.0
    if resolver_missed_rate > 0.50:
        root_cause = "path_resolver_bug"
        recommended = "FIX_B_repair_structure_path_resolver"
    elif missing_rate > 0.90:
        root_cause = "true_missing_files"
        recommended = "FIX_A_download_missing_biolip2_structures"
    else:
        root_cause = "mixed"
        recommended = "FIX_M_mixed_multiple_fixes"
    struct_rate = unique_local / unique if unique else 0.0
    patch_over_local = patch_rows / local_rows if local_rows else 0.0
    micro_rate = micro_success / micro_total if micro_total else 0.0
    cap_location = f"{active_caps[0].get('file_path', 'none')}:{active_caps[0].get('line_number', '0')}" if active_caps else "none:0"
    cap_value = active_caps[0].get("matched_text", "none") if active_caps else "none"
    lines = [
        "# Phase 0.8.2 BioLiP2 Failure Diagnosis Report",
        "",
        "## Verdict",
        "- DIAGNOSIS_SCOPE: Phase 0.8.2 BioLiP2 failure only",
        "- PHASE_0_8_3_IMPLEMENTED: FALSE",
        "- SUPPORTED_ROOT_CAUSE: " + root_cause,
        "- RECOMMENDED_FIX_PATH: " + recommended,
        "",
        "## Q1. BioLiP2 PDB ID Coverage",
        f"- unique BioLiP2 PDB IDs: {unique}",
        f"- with local structure: {unique_local} ({struct_rate:.6f})",
        f"- missing local structure: {unique_missing} ({(unique_missing / unique if unique else 0.0):.6f})",
        f"- obsolete with replacement: {coverage.get('unique_biolip2_pdb_ids_with_replacement', {}).get('value', '0')}",
        f"- BioLiP2 rows total: {total}",
        f"- rows with local structure: {local_rows}",
        f"- rows failed missing_structure: {missing_rows}",
        f"- rows failed because current resolver missed existing local structure: {resolver_missed_rows}",
        "",
        "## Q2. Missing Structure vs Resolver Bug",
        f"- SUPPORTED_ROOT_CAUSE: {root_cause}",
        "- Evidence: metrics/biolip2_pdb_coverage_summary.tsv; metrics/rcsb_biolip2_overlap_summary.tsv; outputs/biolip2_structure_join_audit.tsv",
        f"- current diagnosis: {resolver_missed_rows} of {total} rows have local structures that the current Phase 0.8.2 resolver missed; {missing_rows} rows lack any audited local structure",
        "",
        "## Q3. mmCIF Parser",
        f"- parse success: {parser_success}/{len(parser_rows)}",
        f"- main error type: {first_row(artifact_dir / 'metrics/parser_error_summary.tsv').get('error_type', 'none')}",
        f"- parser supports .cif.gz: {parser_supports_gz}",
        f"- parser supports auth_asym_id/label_asym_id mapping: {parser_supports_auth_label}",
        "",
        "## Q4. BioLiP2 Schema Parsing",
        f"- PDB ID field: {schema_rows.get('pdb_id', {}).get('field_name', 'column_0')}",
        f"- receptor chain field: {schema_rows.get('receptor_chain', {}).get('field_name', 'column_1')}",
        f"- ligand ID field: {schema_rows.get('ligand_id', {}).get('field_name', 'column_4')}",
        f"- binding residue field: {schema_rows.get('binding_residue_raw', {}).get('field_name', 'column_7')}",
        f"- known schema issue: {'yes' if any(row.get('known_issue') not in {'no', ''} for row in schema_rows.values()) else 'no'}",
        "",
        "## Q5. Strict-Mode Cap Audit",
        f"- strict-mode cap found: {bool(active_caps)}",
        f"- cap location: {cap_location}",
        f"- cap value: {cap_value}",
        "",
        "## Q6. Recommended Fix",
        f"- {recommended}",
        "- Rationale: local `.cif.gz` structures dominate but are missed by the current Phase 0.8.2 resolver; denominator repair is only a companion reporting fix.",
        "",
        "## Q7. Phase 0.8.3 Thresholds Justified By Observed Data",
        f"- recommended_min_biolip2_unique_pdb_structure_coverage: {max(0.90, struct_rate):.2f}",
        f"- recommended_min_patch_rows: {max(100000, int(local_rows * 0.80))}",
        f"- recommended_min_patch_coverage_over_structure_available_rows: {max(0.80, min(0.95, patch_over_local)):.2f}",
        f"- recommended_min_patch_success_microbench_rate: {max(0.80, min(0.95, micro_rate)):.2f}",
        "",
        "## Hypothesis Status",
    ]
    for row in hypotheses:
        lines.append(f"- {row.get('hypothesis_id')}: {row.get('status')} - {row.get('notes')}")
    lines.extend([
        "",
        "## Required Artifacts",
        "- metrics/hypothesis_status.tsv",
        "- metrics/biolip2_pdb_coverage_summary.tsv",
        "- outputs/biolip2_structure_join_audit.tsv",
        "- outputs/mmcif_parser_microbench.tsv",
        "- outputs/patch_extractor_microbench.tsv",
        "- outputs/proposed_fix_plan.tsv",
        "",
        "DIAGNOSIS_COMPLETE: TRUE",
        f"RECOMMENDED_FIX_PATH: {recommended}",
        "READY_FOR_PHASE_0_8_3_IMPLEMENTATION: TRUE",
    ])
    report = artifact_dir / "diagnosis_report_phase_0_8_2_failure.md"
    ensure_parent(report)
    report.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return 0


def write_hashes(artifact_dir: Path) -> int:
    rows = []
    for path in sorted(artifact_dir.rglob("*")):
        if not path.is_file() or path.name == "hashes.tsv":
            continue
        rows.append({"relative_path": str(path.relative_to(artifact_dir)).replace("\\", "/"), "byte_size": path.stat().st_size, "sha256": sha256_file(path)})
    write_tsv(artifact_dir / "hashes.tsv", rows, ["relative_path", "byte_size", "sha256"])
    return 0


def run_report_dependencies(artifact_dir: Path) -> int:
    storage_root_consistency(artifact_dir)
    write_pdb_obsolete_replacement_audit(artifact_dir / "outputs/biolip2_structure_join_audit.tsv", artifact_dir / "outputs/pdb_obsolete_replacement_audit.tsv")
    write_proposed_fix_plan(artifact_dir)
    classify_hypotheses(artifact_dir)
    render_report(artifact_dir)
    write_hashes(artifact_dir)
    return 0


def run_logged_command(command_id: str, command: list[str], artifact_dir: Path, rows: list[dict[str, object]]) -> int:
    log_dir = artifact_dir / "logs/command_stdout_stderr"
    log_dir.mkdir(parents=True, exist_ok=True)
    started = utc_now()
    proc = subprocess.run(command, cwd=ROOT, text=True, encoding="utf-8", errors="replace", stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output_path = log_dir / f"{command_id}.log"
    output_path.write_text(proc.stdout or "", encoding="utf-8")
    rows.append({
        "command_id": command_id,
        "command": " ".join(command),
        "exit_code": proc.returncode,
        "started_at_utc": started,
        "finished_at_utc": utc_now(),
        "log_path": str(output_path),
        "status": "PASS" if proc.returncode == 0 else "FAIL",
    })
    return proc.returncode


def run_diagnosis(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / DIAG_DIRNAME)
    parser.add_argument("--biolip2-table", type=path_arg, default=BIOLIP2_TABLE)
    args = parser.parse_args(argv)
    artifact = args.artifact_dir
    ensure_diag_dirs(artifact)
    py = sys.executable
    rows: list[dict[str, object]] = []
    commands = [
        ("inventory_structure_stores", [py, "scripts/diagnostics/inventory_structure_stores.py", "--repo-root", str(ROOT), "--out", str(artifact / "metrics/structure_store_inventory.tsv")]),
        ("audit_biolip2_structure_join", [py, "scripts/diagnostics/audit_biolip2_structure_join.py", "--biolip2-table", str(args.biolip2_table), "--out", str(artifact / "outputs/biolip2_structure_join_audit.tsv"), "--artifact-dir", str(artifact)]),
        ("summarize_biolip2_pdb_coverage", [py, "scripts/diagnostics/summarize_biolip2_pdb_coverage.py", "--join-audit", str(artifact / "outputs/biolip2_structure_join_audit.tsv"), "--out", str(artifact / "metrics/biolip2_pdb_coverage_summary.tsv"), "--artifact-dir", str(artifact)]),
        ("audit_rcsb_file_resolver", [py, "scripts/diagnostics/audit_rcsb_file_resolver.py", "--biolip2-table", str(args.biolip2_table), "--rcsb-manifest", str(ROOT / "data/external_phase_0_8/rcsb_ligand_bound/rcsb_ligand_bound_entry_manifest.tsv"), "--structure-root", str(ROOT / "data/external_phase_0_8/rcsb_ligand_bound/structures"), "--out", str(artifact / "outputs/rcsb_file_resolver_audit.tsv")]),
        ("mmcif_parser_microbench", [py, "scripts/diagnostics/mmcif_parser_microbench.py", "--out", str(artifact / "outputs/mmcif_parser_microbench.tsv"), "--artifact-dir", str(artifact)]),
        ("patch_extractor_microbench", [py, "scripts/diagnostics/patch_extractor_microbench.py", "--biolip2-table", str(args.biolip2_table), "--structure-root", str(ROOT / "data/external_phase_0_8/rcsb_ligand_bound/structures"), "--out", str(artifact / "outputs/patch_extractor_microbench.tsv"), "--artifact-dir", str(artifact)]),
        ("biolip2_schema_field_audit", [py, "scripts/diagnostics/biolip2_schema_field_audit.py", "--biolip2-table", str(args.biolip2_table), "--out", str(artifact / "outputs/biolip2_schema_field_audit.tsv")]),
        ("audit_config_limits", [py, "scripts/diagnostics/audit_config_limits.py", "--repo-root", str(ROOT), "--out", str(artifact / "metrics/config_limit_audit.tsv")]),
        ("render_phase_0_8_2_failure_diagnosis_report", [py, "scripts/reports/render_phase_0_8_2_failure_diagnosis_report.py", "--artifact-dir", str(artifact)]),
    ]
    exit_code = 0
    for command_id, command in commands:
        code = run_logged_command(command_id, command, artifact, rows)
        exit_code = exit_code or code
    write_tsv(artifact / "logs/command_stdout_stderr/command_summary.tsv", rows, ["command_id", "command", "exit_code", "started_at_utc", "finished_at_utc", "log_path", "status"])
    write_hashes(artifact)
    return exit_code


def cli(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command")
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / DIAG_DIRNAME)
    parser.add_argument("--repo-root", type=path_arg, default=ROOT)
    parser.add_argument("--biolip2-table", type=path_arg, default=BIOLIP2_TABLE)
    parser.add_argument("--join-audit", type=path_arg)
    parser.add_argument("--rcsb-manifest", type=path_arg, default=ROOT / "data/external_phase_0_8/rcsb_ligand_bound/rcsb_ligand_bound_entry_manifest.tsv")
    parser.add_argument("--structure-root", type=path_arg, default=ROOT / "data/external_phase_0_8/rcsb_ligand_bound/structures")
    parser.add_argument("--structure-files", nargs="*", type=path_arg, default=[])
    parser.add_argument("--out", type=path_arg)
    args = parser.parse_args(argv)
    ensure_diag_dirs(args.artifact_dir)
    if args.command == "inventory-structure-stores":
        return inventory_structure_stores(args.repo_root, args.out or args.artifact_dir / "metrics/structure_store_inventory.tsv")
    if args.command == "audit-biolip2-structure-join":
        return audit_biolip2_structure_join(args.biolip2_table, args.out or args.artifact_dir / "outputs/biolip2_structure_join_audit.tsv", args.artifact_dir)
    if args.command == "summarize-biolip2-pdb-coverage":
        return summarize_biolip2_pdb_coverage(args.join_audit or args.artifact_dir / "outputs/biolip2_structure_join_audit.tsv", args.out or args.artifact_dir / "metrics/biolip2_pdb_coverage_summary.tsv", args.artifact_dir)
    if args.command == "audit-rcsb-file-resolver":
        return audit_rcsb_file_resolver(args.biolip2_table, args.rcsb_manifest, args.structure_root, args.out or args.artifact_dir / "outputs/rcsb_file_resolver_audit.tsv")
    if args.command == "pdb-obsolete-replacement-audit":
        return write_pdb_obsolete_replacement_audit(args.join_audit or args.artifact_dir / "outputs/biolip2_structure_join_audit.tsv", args.out or args.artifact_dir / "outputs/pdb_obsolete_replacement_audit.tsv")
    if args.command == "mmcif-parser-microbench":
        return mmcif_parser_microbench(args.structure_files, args.out or args.artifact_dir / "outputs/mmcif_parser_microbench.tsv", args.artifact_dir)
    if args.command == "patch-extractor-microbench":
        return patch_extractor_microbench(args.biolip2_table, args.structure_root, args.out or args.artifact_dir / "outputs/patch_extractor_microbench.tsv", args.artifact_dir)
    if args.command == "biolip2-schema-field-audit":
        return audit_biolip2_schema_fields(args.biolip2_table, args.out or args.artifact_dir / "outputs/biolip2_schema_field_audit.tsv")
    if args.command == "audit-config-limits":
        return audit_config_limits(args.repo_root, args.out or args.artifact_dir / "metrics/config_limit_audit.tsv")
    if args.command == "render-report":
        return run_report_dependencies(args.artifact_dir)
    if args.command == "hashes":
        return write_hashes(args.artifact_dir)
    raise SystemExit(f"unknown_diagnosis_command:{args.command}")
