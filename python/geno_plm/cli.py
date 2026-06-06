from __future__ import annotations

import argparse
import json
import math
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

from geno_plm.backends.fixture_mock import embed_sequence as mock_embed_sequence
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
    embed.add_argument("--backend", required=True, choices=["fixture_mock", "esm2_fair"])
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
    if args.backend == "fixture_mock":
        embedded_rows = [
            {
                "candidate_id": record["candidate_id"],
                "vector": mock_embed_sequence(record["candidate_id"], record["sequence"], args.model_id),
                "sequence_length_effective": len(record["sequence"]),
                "truncated": "false",
            }
            for record in records
        ]
        metadata = {
            "backend": "fixture_mock",
            "model_id": args.model_id,
            "model_source": "deterministic_local_fixture",
            "repr_layer": args.repr_layer,
            "device_requested": args.device,
            "device_resolved": "cpu",
            "dtype": "float32",
            "fallback_events": [],
            "software_versions": {"python": sys.version, "platform": platform.platform()},
        }
    else:
        from geno_plm.backends.esm_fair import EsmConfig, embed_records

        embedded_rows, metadata = embed_records(
            records,
            EsmConfig(
                model_id=args.model_id,
                device=args.device,
                repr_layer=args.repr_layer,
                toks_per_batch=args.toks_per_batch,
                sequence_max_length=args.sequence_max_length,
                allow_cpu_fallback=args.allow_cpu_fallback,
                seed=args.seed,
            ),
        )
    record_by_id = {record["candidate_id"]: record for record in records}
    manifest_rows = []
    for embedded in embedded_rows:
        candidate_id = embedded["candidate_id"]
        record = record_by_id[candidate_id]
        vector = embedded["vector"]
        vector_path = vectors_dir / f"{safe_id(candidate_id)}.mean.npy"
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
                "sequence_length_effective": str(embedded["sequence_length_effective"]),
                "truncated": embedded["truncated"],
                "embedding_file": str(vector_path),
                "embedding_sha256": sha256_file(vector_path),
                "embedding_mean_abs": f"{mean_abs:.6f}",
                "embedding_l2_norm": f"{l2_norm:.6f}",
                "created_at_utc": created,
                "software_versions": json.dumps(metadata.get("software_versions", {}), sort_keys=True),
                "plm_status": "success",
                "plm_notes": "real_esm_embedding" if args.backend != "fixture_mock" else "deterministic_fixture_mock",
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
    rows = read_tsv(args.manifest)
    if not rows:
        raise SystemExit("manifest_has_no_rows")
    for row in rows:
        candidate_id = row.get("candidate_id", "")
        for required in ["model_id", "backend", "device_resolved", "embedding_dim", "plm_status"]:
            if not row.get(required):
                raise SystemExit(f"manifest_missing_{required}:{candidate_id}")
        if row["plm_status"] == "success":
            if not row.get("embedding_sha256"):
                raise SystemExit(f"manifest_missing_embedding_sha256:{candidate_id}")
            dim = int(row["embedding_dim"])
            if dim <= 0:
                raise SystemExit(f"manifest_bad_embedding_dim:{candidate_id}")
            vector = read_npy_float32(row["embedding_file"])
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


def now_utc() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


if __name__ == "__main__":
    raise SystemExit(main())
