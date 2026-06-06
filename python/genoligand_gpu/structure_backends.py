from __future__ import annotations

import time
from pathlib import Path

from .cuda_env import memory_probe_payload, nvidia_memory_mib
from .io import bool_text, fmt, read_fasta, read_tsv, safe_id, sha256_file, sha256_text, write_json, write_tsv
from .pdb_quality import parse_pdb, residue_table, semantic_hash_for_pdb


STRUCTURE_FIELDS = [
    "candidate_id",
    "sequence_sha256",
    "length_aa",
    "is_decoy",
    "context_class",
    "structure_backend",
    "structure_model_id",
    "device",
    "output_format",
    "dtype",
    "structure_path",
    "structure_sha256",
    "mmcif_path",
    "mmcif_sha256",
    "prediction_status",
    "mean_plddt",
    "median_plddt",
    "min_plddt",
    "low_confidence_fraction_plddt_lt_50",
    "very_low_confidence_fraction_plddt_lt_30",
    "residue_count_in_structure",
    "residue_count",
    "atom_count",
    "chain_count",
    "gpu_memory_peak_mib",
    "runtime_seconds",
    "semantic_structure_hash",
    "gpu_validated",
    "warnings",
    "notes",
]


def _select_records(fasta: Path, candidate_tsv: Path, max_candidates: int, mandatory_ids: set[str]) -> list[dict[str, str]]:
    candidate_rows = {row["candidate_id"]: row for row in read_tsv(candidate_tsv)}
    records = []
    for record in read_fasta(fasta):
        candidate = candidate_rows.get(record["id"], {})
        record = dict(record)
        record.update(candidate)
        records.append(record)
    mandatory = [record for record in records if record["id"] in mandatory_ids]
    others = [record for record in records if record["id"] not in mandatory_ids]
    selected = mandatory + others
    return selected[: max(max_candidates, len(mandatory))]


def _hf_outputs_to_pdb(outputs: dict[str, object]) -> list[str]:
    import torch
    from transformers.models.esm.openfold_utils.feats import atom14_to_atom37
    from transformers.models.esm.openfold_utils.protein import Protein as OFProtein
    from transformers.models.esm.openfold_utils.protein import to_pdb

    final_atom_positions = atom14_to_atom37(outputs["positions"][-1], outputs)
    final_atom_positions = final_atom_positions.detach().cpu().numpy()
    cpu_outputs = {
        key: value.detach().cpu().numpy() if torch.is_tensor(value) else value
        for key, value in outputs.items()
    }
    final_atom_mask = cpu_outputs["atom37_atom_exists"]
    pdbs = []
    for index in range(cpu_outputs["aatype"].shape[0]):
        protein = OFProtein(
            aatype=cpu_outputs["aatype"][index],
            atom_positions=final_atom_positions[index],
            atom_mask=final_atom_mask[index],
            residue_index=cpu_outputs["residue_index"][index] + 1,
            b_factors=cpu_outputs["plddt"][index],
            chain_index=cpu_outputs.get("chain_index", None)[index] if "chain_index" in cpu_outputs else None,
        )
        pdbs.append(to_pdb(protein))
    return pdbs


def _pdb_atoms_to_mmcif(candidate_id: str, pdb_path: Path) -> str:
    atoms = parse_pdb(pdb_path)
    lines = [
        f"data_{safe_id(candidate_id)}",
        "#",
        "loop_",
        "_atom_site.group_PDB",
        "_atom_site.id",
        "_atom_site.type_symbol",
        "_atom_site.label_atom_id",
        "_atom_site.label_alt_id",
        "_atom_site.label_comp_id",
        "_atom_site.label_asym_id",
        "_atom_site.label_entity_id",
        "_atom_site.label_seq_id",
        "_atom_site.pdbx_PDB_ins_code",
        "_atom_site.Cartn_x",
        "_atom_site.Cartn_y",
        "_atom_site.Cartn_z",
        "_atom_site.occupancy",
        "_atom_site.B_iso_or_equiv",
        "_atom_site.auth_seq_id",
        "_atom_site.auth_comp_id",
        "_atom_site.auth_asym_id",
        "_atom_site.auth_atom_id",
        "_atom_site.pdbx_PDB_model_num",
    ]
    for index, atom in enumerate(atoms, start=1):
        atom_name = str(atom["atom"])
        element = "".join(ch for ch in atom_name if ch.isalpha())[:1].upper() or "C"
        chain = str(atom["chain"]) or "A"
        resid = int(atom["resid"])
        lines.append(
            " ".join(
                [
                    "ATOM",
                    str(index),
                    element,
                    atom_name,
                    ".",
                    str(atom["resname"]),
                    chain,
                    "1",
                    str(resid),
                    "?",
                    fmt(float(atom["x"]), 3),
                    fmt(float(atom["y"]), 3),
                    fmt(float(atom["z"]), 3),
                    "1.00",
                    fmt(float(atom["bfactor"]), 2),
                    str(resid),
                    str(atom["resname"]),
                    chain,
                    atom_name,
                    "1",
                ]
            )
        )
    lines.append("#")
    return "\n".join(lines) + "\n"


def _plddt_stats(pdb_path: Path) -> dict[str, object]:
    atoms = parse_pdb(pdb_path)
    residues = residue_table(atoms)
    plddts = sorted(float(row["plddt"]) for row in residues)
    mean = sum(plddts) / max(1, len(plddts))
    return {
        "atoms": atoms,
        "residues": residues,
        "mean_plddt": fmt(mean),
        "median_plddt": fmt(plddts[len(plddts) // 2] if plddts else 0.0),
        "min_plddt": fmt(min(plddts) if plddts else 0.0),
        "low_confidence_fraction_plddt_lt_50": fmt(sum(1 for value in plddts if value < 50.0) / max(1, len(plddts))),
        "very_low_confidence_fraction_plddt_lt_30": fmt(sum(1 for value in plddts if value < 30.0) / max(1, len(plddts))),
        "residue_count_in_structure": len(residues),
        "atom_count": len(atoms),
        "chain_count": len({row["chain"] for row in residues}),
    }


def predict_esmfold_hf_gpu(
    fasta: Path,
    candidate_tsv: Path,
    out_dir: Path,
    model_id: str,
    device: str,
    max_candidates: int,
    mandatory_ids: set[str],
) -> dict[str, object]:
    import torch
    from transformers import AutoTokenizer, EsmForProteinFolding

    if not torch.cuda.is_available():
        raise SystemExit("cuda_unavailable_for_structure")
    if not device.startswith("cuda"):
        raise SystemExit("required_structure_device_not_cuda")
    records = _select_records(fasta, candidate_tsv, max_candidates, mandatory_ids)
    pdb_dir = out_dir / "pdb"
    mmcif_dir = out_dir / "mmcif"
    pdb_dir.mkdir(parents=True, exist_ok=True)
    mmcif_dir.mkdir(parents=True, exist_ok=True)
    before = nvidia_memory_mib()
    started = time.time()
    tokenizer = AutoTokenizer.from_pretrained(model_id)
    model = EsmForProteinFolding.from_pretrained(model_id, low_cpu_mem_usage=True)
    model = model.eval().to(device)
    if hasattr(model, "trunk"):
        model.trunk.set_chunk_size(64)
    rows = []
    torch.cuda.reset_peak_memory_stats()
    peak_mib = before
    with torch.no_grad():
        for record in records:
            record_started = time.time()
            candidate_id = record["id"]
            seq = record["sequence"]
            encoded = tokenizer([seq], return_tensors="pt", add_special_tokens=False)
            encoded = {key: value.to(device) for key, value in encoded.items()}
            outputs = model(**encoded)
            torch.cuda.synchronize()
            pdb_text = _hf_outputs_to_pdb(outputs)[0]
            pdb_path = pdb_dir / f"{safe_id(candidate_id)}.pdb"
            pdb_path.write_text(pdb_text, encoding="utf-8")
            mmcif_path = mmcif_dir / f"{safe_id(candidate_id)}.cif"
            mmcif_path.write_text(_pdb_atoms_to_mmcif(candidate_id, pdb_path), encoding="utf-8")
            stats = _plddt_stats(pdb_path)
            atoms = stats["atoms"]
            residues = stats["residues"]
            seq_sha = sha256_text(seq)
            current_peak_mib = max(peak_mib, nvidia_memory_mib())
            rows.append(
                {
                    "candidate_id": candidate_id,
                    "sequence_sha256": seq_sha,
                    "length_aa": len(seq),
                    "is_decoy": bool_text("control" in record.get("export_status", "")),
                    "context_class": record.get("context_class") or record.get("context") or record.get("export_status", ""),
                    "structure_backend": "esmfold_hf_gpu",
                    "structure_model_id": model_id,
                    "device": device,
                    "output_format": "pdb+mmcif",
                    "dtype": "float32",
                    "structure_path": str(pdb_path),
                    "structure_sha256": sha256_file(pdb_path),
                    "mmcif_path": str(mmcif_path),
                    "mmcif_sha256": sha256_file(mmcif_path),
                    "prediction_status": "success",
                    "mean_plddt": stats["mean_plddt"],
                    "median_plddt": stats["median_plddt"],
                    "min_plddt": stats["min_plddt"],
                    "low_confidence_fraction_plddt_lt_50": stats["low_confidence_fraction_plddt_lt_50"],
                    "very_low_confidence_fraction_plddt_lt_30": stats["very_low_confidence_fraction_plddt_lt_30"],
                    "residue_count_in_structure": stats["residue_count_in_structure"],
                    "residue_count": stats["residue_count_in_structure"],
                    "atom_count": stats["atom_count"],
                    "chain_count": stats["chain_count"],
                    "gpu_memory_peak_mib": current_peak_mib,
                    "runtime_seconds": fmt(time.time() - record_started),
                    "semantic_structure_hash": semantic_hash_for_pdb(pdb_path, candidate_id, seq_sha, "esmfold_hf_gpu"),
                    "gpu_validated": "true",
                    "warnings": "short_sequence_structure_prioritization_only" if len(seq) < 30 else "",
                    "notes": "real_esmfold_hf_cuda_prediction",
                }
            )
            peak_mib = current_peak_mib
    after = nvidia_memory_mib()
    write_tsv(out_dir / "structure_manifest.tsv", rows, STRUCTURE_FIELDS)
    write_tsv(
        out_dir / "structure_hashes.tsv",
        [
            {
                "candidate_id": row["candidate_id"],
                "structure_sha256": row["structure_sha256"],
                "semantic_structure_hash": row["semantic_structure_hash"],
                "structure_path": row["structure_path"],
            }
            for row in rows
        ],
        ["candidate_id", "structure_sha256", "semantic_structure_hash", "structure_path"],
    )
    backend_manifest = {
        "backend": "esmfold_hf_gpu",
        "model_id": model_id,
        "device": device,
        "row_count": len(rows),
        "elapsed_seconds": time.time() - started,
        "torch_version": torch.__version__,
        "torch_cuda_version": torch.version.cuda,
        "torch_max_memory_allocated_bytes": torch.cuda.max_memory_allocated(),
        "all_required_steps_cuda": True,
    }
    write_json(out_dir / "structure_backend_manifest.json", backend_manifest)
    return {
        "backend_manifest": backend_manifest,
        "memory_probe": memory_probe_payload(
            "structure_prediction", before, peak_mib, after, int(torch.cuda.max_memory_allocated())
        ),
    }
