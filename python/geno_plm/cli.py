from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

from geno_plm.device import print_cuda_diagnostics
from geno_plm.io import (
    all_finite,
    read_fasta,
    read_npy_float32,
    read_tsv,
    sha256_file,
    vector_stats,
    write_npy_float32,
    write_tsv,
)


MANIFEST_FIELDS = [
    "candidate_id",
    "model_id",
    "backend",
    "model_source",
    "tokenizer_id",
    "device_requested",
    "device_resolved",
    "dtype",
    "repr_layer",
    "pooling",
    "embedding_dim",
    "sequence_length_input",
    "sequence_length_effective",
    "truncated",
    "embedding_file",
    "embedding_sha256",
    "embedding_mean_abs",
    "embedding_l2_norm",
    "created_at_utc",
    "software_versions",
    "plm_status",
    "plm_notes",
]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m geno_plm.cli")
    sub = parser.add_subparsers(dest="command", required=True)

    diagnose = sub.add_parser("diagnose-device")
    diagnose.set_defaults(func=cmd_diagnose_device)

    embed = sub.add_parser("embed")
    embed.add_argument("--input-fasta", required=True)
    embed.add_argument("--output-dir", required=True)
    embed.add_argument("--backend", required=True, choices=["esm2_fair"])
    embed.add_argument("--model-id", required=True)
    embed.add_argument("--device", default="auto", choices=["auto", "cpu", "cuda"])
    embed.add_argument("--include", default="mean")
    embed.add_argument("--repr-layer", default="final")
    embed.add_argument("--toks-per-batch", type=int, default=4096)
    embed.add_argument("--seed", type=int, default=3407)
    embed.add_argument("--sequence-max-length", type=int, default=1022)
    embed.add_argument("--allow-cpu-fallback", action="store_true")
    embed.add_argument("--metadata", required=True)
    embed.set_defaults(func=cmd_embed)

    summarize = sub.add_parser("summarize")
    summarize.add_argument("--manifest", required=True)
    summarize.add_argument("--out")
    summarize.set_defaults(func=cmd_summarize)

    validate = sub.add_parser("validate")
    validate.add_argument("--manifest", required=True)
    validate.set_defaults(func=cmd_validate)

    args = parser.parse_args(argv)
    args.func(args)
    return 0


def cmd_diagnose_device(_args) -> None:
    print_cuda_diagnostics()


def cmd_embed(args) -> None:
    output_dir = Path(args.output_dir)
    vectors_dir = output_dir / "vectors"
    output_dir.mkdir(parents=True, exist_ok=True)
    vectors_dir.mkdir(parents=True, exist_ok=True)
    records = read_fasta(args.input_fasta)
    created = now_utc()
    from geno_plm.backends.esm_fair import EsmConfig, embed_records

    embedding_rows, metadata = embed_records(
        records,
        EsmConfig(
            model_id=args.model_id,
            device=args.device,
            repr_layer=args.repr_layer,
            toks_per_batch=args.toks_per_batch,
            sequence_max_length=args.sequence_max_length,
            allow_cpu_fallback=args.allow_cpu_fallback,
            seed=args.seed,
        )
    )
    record_by_id = {record["candidate_id"]: record for record in records}
    manifest_rows = []
    for row in embedding_rows:
        candidate_id = row["candidate_id"]
        record = record_by_id[candidate_id]
        vector = row["vector"]
        vector_path = (vectors_dir / embedding_filename(candidate_id)).resolve()
        write_npy_float32(vector_path, vector)
        mean_abs, l2_norm = vector_stats(vector)
        manifest_rows.append(
            {
                "candidate_id": candidate_id,
                "model_id": args.model_id,
                "backend": args.backend,
                "model_source": metadata.get("model_source", ""),
                "tokenizer_id": args.model_id,
                "device_requested": args.device,
                "device_resolved": metadata.get("device_resolved", "cpu"),
                "dtype": metadata.get("dtype", "float32"),
                "repr_layer": str(metadata.get("repr_layer", args.repr_layer)),
                "pooling": "mean",
                "embedding_dim": str(len(vector)),
                "sequence_length_input": str(len(record["sequence"])),
                "sequence_length_effective": str(row["sequence_length_effective"]),
                "truncated": row["truncated"],
                "embedding_file": str(vector_path),
                "embedding_sha256": sha256_file(vector_path),
                "embedding_mean_abs": f"{mean_abs:.6f}",
                "embedding_l2_norm": f"{l2_norm:.6f}",
                "created_at_utc": created,
                "software_versions": json.dumps(metadata.get("software_versions", {}), sort_keys=True),
                "plm_status": "success",
                "plm_notes": "esm_embedding",
            }
        )
    write_tsv(output_dir / "embedding_manifest.tsv", manifest_rows, MANIFEST_FIELDS)
    metadata["created_at_utc"] = created
    metadata["input_fasta"] = str(args.input_fasta)
    metadata["manifest"] = str(output_dir / "embedding_manifest.tsv")
    with open(args.metadata, "w", encoding="utf-8") as handle:
        json.dump(metadata, handle, indent=2, sort_keys=True)


def cmd_summarize(args) -> None:
    rows = read_tsv(args.manifest)
    summary = {
        "manifest": args.manifest,
        "rows": len(rows),
        "backends": sorted({row.get("backend", "") for row in rows}),
        "statuses": sorted({row.get("plm_status", "") for row in rows}),
        "devices": sorted({row.get("device_resolved", "") for row in rows}),
    }
    text = json.dumps(summary, indent=2, sort_keys=True)
    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    else:
        print(text)


def cmd_validate(args) -> None:
    validate_manifest(args.manifest)


def validate_manifest(manifest: str | Path) -> None:
    manifest_path = Path(manifest).resolve()
    rows = read_tsv(manifest_path)
    if not rows:
        raise SystemExit("manifest_has_no_rows")
    embedding_references: dict[str, str] = {}
    for row in rows:
        candidate_id = row.get("candidate_id", "")
        for required in ["model_id", "backend", "device_resolved", "embedding_dim", "plm_status"]:
            if not row.get(required):
                raise SystemExit(f"manifest_missing_{required}:{candidate_id}")
        if row["plm_status"] == "success":
            if not row.get("embedding_sha256"):
                raise SystemExit(f"manifest_missing_embedding_sha256:{candidate_id}")
            if not row.get("embedding_file"):
                raise SystemExit(f"manifest_missing_embedding_file:{candidate_id}")
            vector_path = Path(row["embedding_file"])
            if not vector_path.is_absolute():
                vector_path = manifest_path.parent / vector_path
            vector_path = vector_path.resolve()
            reference = os.path.normcase(str(vector_path))
            previous_candidate = embedding_references.get(reference)
            if previous_candidate is not None:
                raise SystemExit(
                    f"manifest_embedding_reference_collision:{candidate_id}:{previous_candidate}"
                )
            embedding_references[reference] = candidate_id
            try:
                actual_sha256 = sha256_file(vector_path)
            except OSError as exc:
                raise SystemExit(f"manifest_embedding_unreadable:{candidate_id}:{exc}") from exc
            if actual_sha256.lower() != row["embedding_sha256"].strip().lower():
                raise SystemExit(f"manifest_embedding_sha256_mismatch:{candidate_id}")
            dim = int(row["embedding_dim"])
            if dim <= 0:
                raise SystemExit(f"manifest_bad_embedding_dim:{candidate_id}")
            vector = read_npy_float32(vector_path)
            if len(vector) != dim:
                raise SystemExit(f"manifest_vector_dim_mismatch:{candidate_id}")
            if not all_finite(vector):
                raise SystemExit(f"manifest_vector_nonfinite:{candidate_id}")
            if not math.isfinite(float(row["embedding_l2_norm"])):
                raise SystemExit(f"manifest_l2_nonfinite:{candidate_id}")
        elif not row.get("plm_notes"):
            raise SystemExit(f"manifest_failed_row_missing_notes:{candidate_id}")
    print(f"plm_manifest_ok\trows={len(rows)}")


def safe_id(value: str) -> str:
    return "".join(ch if ch.isalnum() or ch in "._-" else "_" for ch in value)


def embedding_filename(candidate_id: str) -> str:
    readable_id = safe_id(candidate_id)[:80] or "candidate"
    content_digest = hashlib.sha256(candidate_id.encode("utf-8")).hexdigest()
    return f"{readable_id}.{content_digest}.mean.npy"


def now_utc() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


if __name__ == "__main__":
    raise SystemExit(main())
