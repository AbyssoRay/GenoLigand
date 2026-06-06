from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable

from geno_plm.device import choose_device


@dataclass
class EsmConfig:
    model_id: str
    device: str
    repr_layer: str
    toks_per_batch: int
    sequence_max_length: int
    allow_cpu_fallback: bool
    seed: int


def embed_records(records: list[dict], config: EsmConfig) -> tuple[list[dict], dict]:
    import random
    import time

    import esm
    import torch

    try:
        import numpy as np
    except Exception:  # pragma: no cover
        np = None

    random.seed(config.seed)
    if np is not None:
        np.random.seed(config.seed)
    torch.manual_seed(config.seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(config.seed)

    resolved_device = choose_device(config.device)
    start = time.time()
    model, alphabet = _load_model(esm, config.model_id)
    model.eval()
    model = model.to(resolved_device)
    batch_converter = alphabet.get_batch_converter()
    repr_layer = model.num_layers if config.repr_layer == "final" else int(config.repr_layer)
    fallback_events: list[dict] = []
    current_toks = config.toks_per_batch

    while True:
        try:
            embeddings = _embed_batches(
                records,
                model,
                batch_converter,
                repr_layer,
                resolved_device,
                current_toks,
                config.sequence_max_length,
                torch,
            )
            break
        except RuntimeError as exc:
            message = str(exc)
            if "out of memory" not in message.lower():
                raise
            fallback_events.append({"event": "cuda_oom", "toks_per_batch": current_toks})
            if resolved_device == "cuda" and current_toks > 256:
                current_toks = max(256, current_toks // 2)
                torch.cuda.empty_cache()
                continue
            if resolved_device == "cuda" and config.allow_cpu_fallback:
                fallback_events.append({"event": "cpu_fallback_after_cuda_oom"})
                resolved_device = "cpu"
                model = model.to("cpu")
                current_toks = config.toks_per_batch
                continue
            raise

    metadata = {
        "backend": "esm2_fair",
        "model_id": config.model_id,
        "model_source": "facebookresearch/esm",
        "repr_layer": repr_layer,
        "device_requested": config.device,
        "device_resolved": resolved_device,
        "dtype": "float32",
        "toks_per_batch_final": current_toks,
        "sequence_max_length": config.sequence_max_length,
        "elapsed_seconds": round(time.time() - start, 3),
        "fallback_events": fallback_events,
        "software_versions": {
            "torch": torch.__version__,
            "torch_cuda": getattr(torch.version, "cuda", None),
            "esm": getattr(esm, "__version__", "unknown"),
        },
    }
    return embeddings, metadata


def _load_model(esm_module, model_id: str):
    if model_id == "esm2_t6_8M_UR50D":
        return esm_module.pretrained.esm2_t6_8M_UR50D()
    if model_id == "esm2_t12_35M_UR50D":
        return esm_module.pretrained.esm2_t12_35M_UR50D()
    raise ValueError(f"Unsupported fair-esm model_id: {model_id}")


def _batches(records: list[dict], toks_per_batch: int, max_len: int) -> Iterable[list[dict]]:
    batch: list[dict] = []
    batch_tokens = 0
    for record in records:
        effective_len = min(len(record["sequence"]), max_len)
        if batch and batch_tokens + effective_len > toks_per_batch:
            yield batch
            batch = []
            batch_tokens = 0
        batch.append(record)
        batch_tokens += effective_len
    if batch:
        yield batch


def _embed_batches(
    records,
    model,
    batch_converter,
    repr_layer,
    device,
    toks_per_batch,
    max_len,
    torch,
) -> list[dict]:
    rows: list[dict] = []
    with torch.inference_mode():
        for batch in _batches(records, toks_per_batch, max_len):
            labels = []
            truncated_lengths = {}
            for record in batch:
                seq = record["sequence"][:max_len]
                labels.append((record["candidate_id"], seq))
                truncated_lengths[record["candidate_id"]] = len(seq)
            _, _, tokens = batch_converter(labels)
            tokens = tokens.to(device)
            result = model(tokens, repr_layers=[repr_layer], return_contacts=False)
            reps = result["representations"][repr_layer].detach().cpu()
            for index, (candidate_id, seq) in enumerate(labels):
                effective_len = truncated_lengths[candidate_id]
                mean = reps[index, 1 : effective_len + 1].mean(0).float().tolist()
                rows.append(
                    {
                        "candidate_id": candidate_id,
                        "vector": [float(value) for value in mean],
                        "sequence_length_effective": effective_len,
                        "truncated": str(effective_len < len(batch[index]["sequence"])).lower(),
                    }
                )
    return rows
