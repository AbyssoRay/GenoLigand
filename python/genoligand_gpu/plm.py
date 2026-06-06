from __future__ import annotations

import json
import math
import struct
import time
from pathlib import Path

from .cuda_env import memory_probe_payload, nvidia_memory_mib
from .io import bool_text, fmt, read_fasta, read_tsv, sha256_bytes, sha256_text, write_json, write_tsv


PLM_FIELDS = [
    "candidate_id",
    "sequence",
    "length_aa",
    "context_class",
    "is_decoy",
    "plm_backend",
    "plm_model_id",
    "plm_layer",
    "device",
    "dtype",
    "embedding_dim",
    "embedding_sha256",
    "embedding_l2_norm",
    "embedding_mean",
    "embedding_std",
    "token_count",
    "sequence_sha256",
    "status",
]


def _stats(values: list[float]) -> tuple[float, float, float]:
    if not values:
        return 0.0, 0.0, 0.0
    mean = sum(values) / len(values)
    var = sum((value - mean) ** 2 for value in values) / len(values)
    l2 = math.sqrt(sum(value * value for value in values))
    return mean, math.sqrt(var), l2


def embed_fair_esm_gpu(
    fasta: Path,
    candidate_tsv: Path,
    out_dir: Path,
    model_id: str,
    batch_size: int,
    device: str,
) -> dict[str, object]:
    import torch
    import esm

    if not torch.cuda.is_available():
        raise SystemExit("cuda_unavailable_for_plm")
    if not device.startswith("cuda"):
        raise SystemExit("required_plm_device_not_cuda")
    candidates = {row["candidate_id"]: row for row in read_tsv(candidate_tsv)}
    records = read_fasta(fasta)
    out_dir.mkdir(parents=True, exist_ok=True)
    vector_dir = out_dir / "vectors"
    vector_dir.mkdir(parents=True, exist_ok=True)
    before = nvidia_memory_mib()
    started = time.time()
    if model_id != "esm2_t6_8M_UR50D":
        raise SystemExit(f"unsupported_plm_model:{model_id}")
    model, alphabet = esm.pretrained.esm2_t6_8M_UR50D()
    batch_converter = alphabet.get_batch_converter()
    model.eval().to(device)
    rows: list[dict[str, object]] = []
    layer = 6
    torch.cuda.reset_peak_memory_stats()
    peak_mib = before
    with torch.no_grad():
        for offset in range(0, len(records), batch_size):
            batch = records[offset : offset + batch_size]
            labels = [record["id"] for record in batch]
            data = [(record["id"], record["sequence"]) for record in batch]
            _, _, tokens = batch_converter(data)
            tokens = tokens.to(device)
            output = model(tokens, repr_layers=[layer], return_contacts=False)
            reps = output["representations"][layer]
            for index, label in enumerate(labels):
                seq = batch[index]["sequence"]
                length = len(seq)
                mean_tensor = reps[index, 1 : length + 1].mean(0).detach().float().cpu()
                values = [float(value) for value in mean_tensor.tolist()]
                packed = struct.pack(f"<{len(values)}f", *values)
                embedding_sha = sha256_bytes(packed)
                vector_path = vector_dir / f"{label}.mean.f32.bin"
                vector_path.write_bytes(packed)
                mean, std, l2 = _stats(values)
                candidate = candidates.get(label, {})
                rows.append(
                    {
                        "candidate_id": label,
                        "sequence": seq,
                        "length_aa": length,
                        "context_class": candidate.get("context_class", ""),
                        "is_decoy": bool_text("control" in candidate.get("export_status", "")),
                        "plm_backend": "fair_esm_gpu",
                        "plm_model_id": model_id,
                        "plm_layer": layer,
                        "device": device,
                        "dtype": "float32",
                        "embedding_dim": len(values),
                        "embedding_sha256": embedding_sha,
                        "embedding_l2_norm": fmt(l2),
                        "embedding_mean": fmt(mean),
                        "embedding_std": fmt(std),
                        "token_count": int(tokens[index].ne(alphabet.padding_idx).sum().item()),
                        "sequence_sha256": sha256_text(seq),
                        "status": "success",
                    }
                )
            torch.cuda.synchronize()
            peak_mib = max(peak_mib, nvidia_memory_mib())
    after = nvidia_memory_mib()
    write_tsv(out_dir / "plm_embeddings.tsv", rows, PLM_FIELDS)
    hashes = [
        {
            "candidate_id": row["candidate_id"],
            "embedding_sha256": row["embedding_sha256"],
            "vector_file": f"vectors/{row['candidate_id']}.mean.f32.bin",
        }
        for row in rows
    ]
    write_tsv(out_dir / "plm_embedding_hashes.tsv", hashes, ["candidate_id", "embedding_sha256", "vector_file"])
    manifest = {
        "backend": "fair_esm_gpu",
        "model_id": model_id,
        "device": device,
        "row_count": len(rows),
        "started_at_unix": started,
        "elapsed_seconds": time.time() - started,
        "torch_version": torch.__version__,
        "torch_cuda_version": torch.version.cuda,
        "torch_max_memory_allocated_bytes": torch.cuda.max_memory_allocated(),
        "all_required_steps_cuda": True,
    }
    write_json(out_dir / "plm_manifest.json", manifest)
    return {
        "manifest": manifest,
        "memory_probe": memory_probe_payload(
            "plm_embedding", before, peak_mib, after, int(torch.cuda.max_memory_allocated())
        ),
    }
