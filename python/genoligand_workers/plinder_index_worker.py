from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import sys
import tempfile
from pathlib import Path
from typing import Any


MAX_BATCH_SIZE = 4096
CANONICAL_HEADER = (
    "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
    "sequence_tokens\tstructure_tokens\tevidence\n"
)
SCHEMA_VERSION = "genoligand_plinder_canonical_v1"
SEMANTIC_ALIASES = {
    "structure_id": ("entry_pdb_id", "pdb_id", "structure_id"),
    "system_id": ("system_id", "system_id_no_biounit", "complex_id"),
    "ligand_id": ("ligand_ccd_code", "ligand_unique_ccd_code", "ligand_id"),
    "ligand_smiles": (
        "ligand_rdkit_canonical_smiles",
        "ligand_smiles",
        "ligand_resolved_smiles",
        "canonical_smiles",
    ),
    "protein_chains": (
        "ligand_protein_chains_auth_id",
        "ligand_protein_chains_asym_id",
        "system_protein_chains_auth_id",
        "system_protein_chains_asym_id",
        "protein_chain_ids",
    ),
    "ligand_chain": (
        "ligand_instance_chain",
        "ligand_auth_id",
        "ligand_asym_id",
        "ligand_chain_id",
    ),
}
REQUIRED_SEMANTICS = ("structure_id", "system_id", "ligand_id")


class PlinderIndexError(RuntimeError):
    pass


def inspect_plinder_parquet(source_path: str | Path) -> dict[str, Any]:
    source = Path(source_path)
    parquet = _open_parquet(source)
    mapping = _resolve_schema(parquet.schema_arrow)
    return _source_summary(source, parquet, mapping)


def build_plinder_canonical(
    source_path: str | Path,
    canonical_path: str | Path,
    manifest_path: str | Path,
    *,
    batch_size: int = MAX_BATCH_SIZE,
) -> dict[str, Any]:
    source = Path(source_path)
    canonical = Path(canonical_path)
    manifest_output = Path(manifest_path)
    parquet = _open_parquet(source)
    mapping = _resolve_schema(parquet.schema_arrow)
    source_summary = _source_summary(source, parquet, mapping)
    scan_batch_size = _bounded_batch_size(batch_size)
    selected_columns = list(dict.fromkeys(mapping.values()))
    source_stat = source.stat()

    canonical.parent.mkdir(parents=True, exist_ok=True)
    manifest_output.parent.mkdir(parents=True, exist_ok=True)
    canonical_tmp = _temporary_path(canonical)
    manifest_tmp: Path | None = None
    row_count = 0
    batch_count = 0
    max_batch_rows = 0
    try:
        with canonical_tmp.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
            writer.writerow(CANONICAL_HEADER.rstrip("\n").split("\t"))
            try:
                batches = parquet.iter_batches(
                    batch_size=scan_batch_size,
                    columns=selected_columns,
                    use_threads=True,
                )
                for batch in batches:
                    batch_count += 1
                    max_batch_rows = max(max_batch_rows, batch.num_rows)
                    values = batch.to_pydict()
                    for index in range(batch.num_rows):
                        row = _canonical_row(values, mapping, index)
                        if row is None:
                            continue
                        writer.writerow(row)
                        row_count += 1
            except PlinderIndexError:
                raise
            except Exception as exc:
                raise PlinderIndexError(f"plinder_parquet_scan_failed:{_safe_error(exc)}") from exc
            handle.flush()
            os.fsync(handle.fileno())

        if row_count == 0:
            raise PlinderIndexError("zero_valid_plinder_rows")

        source_sha256, source_bytes = _hash_file(source)
        final_stat = source.stat()
        if (source_stat.st_size, source_stat.st_mtime_ns) != (final_stat.st_size, final_stat.st_mtime_ns):
            raise PlinderIndexError("plinder_source_changed_during_index")
        canonical_sha256, canonical_bytes = _hash_file(canonical_tmp)
        source_summary.update({"sha256": source_sha256, "size_bytes": source_bytes})
        manifest = {
            "schema_version": SCHEMA_VERSION,
            "status": "ready",
            "source": source_summary,
            "canonical": {
                "path": str(canonical.resolve()),
                "row_count": row_count,
                "sha256": canonical_sha256,
                "size_bytes": canonical_bytes,
            },
            "schema_mapping": mapping,
            "scan": {
                "batch_size": scan_batch_size,
                "batch_count": batch_count,
                "max_batch_rows": max_batch_rows,
                "filtered_rows": parquet.metadata.num_rows - row_count,
            },
        }
        manifest_tmp = _temporary_path(manifest_output)
        with manifest_tmp.open("w", encoding="utf-8", newline="\n") as handle:
            json.dump(manifest, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())

        os.replace(canonical_tmp, canonical)
        os.replace(manifest_tmp, manifest_output)
        return manifest
    finally:
        canonical_tmp.unlink(missing_ok=True)
        if manifest_tmp is not None:
            manifest_tmp.unlink(missing_ok=True)


def _open_parquet(source: Path) -> Any:
    if not source.is_file():
        raise PlinderIndexError(f"plinder_parquet_missing:{source}")
    try:
        import pyarrow.parquet as pq
    except ImportError as exc:
        raise PlinderIndexError("pyarrow_required_for_plinder_index") from exc
    try:
        return pq.ParquetFile(source)
    except Exception as exc:
        raise PlinderIndexError(f"invalid_plinder_parquet:{_safe_error(exc)}") from exc


def _resolve_schema(schema: Any) -> dict[str, str]:
    try:
        import pyarrow as pa
    except ImportError as exc:
        raise PlinderIndexError("pyarrow_required_for_plinder_index") from exc

    available = set(schema.names)
    mapping: dict[str, str] = {}
    for semantic, aliases in SEMANTIC_ALIASES.items():
        resolved = next((alias for alias in aliases if alias in available), None)
        if resolved is not None:
            mapping[semantic] = resolved
    missing = sorted(set(REQUIRED_SEMANTICS) - mapping.keys())
    if missing:
        raise PlinderIndexError(f"missing_plinder_semantic_columns:{','.join(missing)}")

    for semantic, column in mapping.items():
        data_type = schema.field(column).type
        if semantic == "protein_chains":
            valid = _is_text_type(pa, data_type) or (
                (pa.types.is_list(data_type) or pa.types.is_large_list(data_type))
                and _is_text_type(pa, data_type.value_type)
            )
        else:
            valid = _is_text_type(pa, data_type)
        if not valid:
            raise PlinderIndexError(
                f"unsupported_plinder_column_type:{semantic}:{column}:{data_type}"
            )
    return mapping


def _is_text_type(pa: Any, data_type: Any) -> bool:
    if pa.types.is_string(data_type) or pa.types.is_large_string(data_type):
        return True
    return pa.types.is_dictionary(data_type) and (
        pa.types.is_string(data_type.value_type) or pa.types.is_large_string(data_type.value_type)
    )


def _source_summary(source: Path, parquet: Any, mapping: dict[str, str]) -> dict[str, Any]:
    fields = [{"name": field.name, "type": str(field.type)} for field in parquet.schema_arrow]
    schema_json = json.dumps(fields, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return {
        "path": str(source.resolve()),
        "parquet_rows": parquet.metadata.num_rows,
        "row_groups": parquet.metadata.num_row_groups,
        "schema_field_count": len(fields),
        "schema_sha256": hashlib.sha256(schema_json).hexdigest(),
        "schema_mapping": mapping,
    }


def _bounded_batch_size(batch_size: int) -> int:
    try:
        requested = int(batch_size)
    except (TypeError, ValueError) as exc:
        raise PlinderIndexError("invalid_plinder_batch_size") from exc
    if requested <= 0:
        raise PlinderIndexError("invalid_plinder_batch_size")
    return min(requested, MAX_BATCH_SIZE)


def _canonical_row(
    values: dict[str, list[Any]],
    mapping: dict[str, str],
    index: int,
) -> list[str] | None:
    structure_id = _cell_value(values[mapping["structure_id"]][index])
    system_id = _cell_value(values[mapping["system_id"]][index])
    ligand_id = _cell_value(values[mapping["ligand_id"]][index])
    if not structure_id or not system_id or not ligand_id:
        return None

    ligand_smiles = _mapped_value(values, mapping, "ligand_smiles", index)
    protein_chains = _mapped_values(values, mapping, "protein_chains", index)
    ligand_chain = _mapped_value(values, mapping, "ligand_chain", index)
    ligand_tokens = _tokens([ligand_id, ligand_smiles])
    sequence_tokens = _tokens(protein_chains)
    structure_tokens = _tokens([structure_id, system_id, ligand_chain, *protein_chains])
    evidence = (
        "plinder_2024-06_v2_parquet;"
        f"structure_id={_clean_cell(structure_id)};"
        f"system_id={_clean_cell(system_id)};"
        f"ligand_id={_clean_cell(ligand_id)}"
    )
    return [
        "PLINDER",
        _clean_cell(system_id),
        _clean_cell(f"PLINDER {structure_id} system {system_id}"),
        "unknown",
        "structure_record",
        " ".join(ligand_tokens),
        " ".join(sequence_tokens),
        " ".join(structure_tokens),
        evidence,
    ]


def _mapped_value(
    values: dict[str, list[Any]], mapping: dict[str, str], semantic: str, index: int
) -> str:
    column = mapping.get(semantic)
    return _cell_value(values[column][index]) if column is not None else ""


def _mapped_values(
    values: dict[str, list[Any]], mapping: dict[str, str], semantic: str, index: int
) -> list[str]:
    column = mapping.get(semantic)
    if column is None:
        return []
    value = values[column][index]
    if isinstance(value, (list, tuple)):
        return [text for item in value if (text := _cell_value(item))]
    text = _cell_value(value)
    return [text] if text else []


def _cell_value(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace").strip()
    return str(value).strip()


def _tokens(values: list[str]) -> list[str]:
    tokens: set[str] = set()
    for value in values:
        for part in re.split(r"[^A-Za-z0-9]+", value):
            if not part:
                continue
            lower = part.lower()
            if len(lower) <= 3:
                tokens.add(lower)
            else:
                tokens.update(lower[index : index + 3] for index in range(len(lower) - 2))
    return sorted(tokens)


def _clean_cell(value: str) -> str:
    return " ".join(value.replace("\x00", " ").split())


def _hash_file(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as handle:
        while chunk := handle.read(8 * 1024 * 1024):
            digest.update(chunk)
            total += len(chunk)
    return digest.hexdigest(), total


def _temporary_path(destination: Path) -> Path:
    handle = tempfile.NamedTemporaryFile(
        prefix=f".{destination.name}.",
        suffix=".tmp",
        dir=destination.parent,
        delete=False,
    )
    handle.close()
    return Path(handle.name)


def _safe_error(exc: Exception) -> str:
    return " ".join(str(exc).split())[:500] or exc.__class__.__name__


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build the canonical PLINDER structure index")
    parser.add_argument("--source", required=True)
    parser.add_argument("--canonical", required=True)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--batch-size", type=int, default=MAX_BATCH_SIZE)
    args = parser.parse_args(argv)
    try:
        manifest = build_plinder_canonical(
            args.source,
            args.canonical,
            args.manifest,
            batch_size=args.batch_size,
        )
    except PlinderIndexError as exc:
        print(str(exc), file=sys.stderr)
        return 2
    print(json.dumps(manifest, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
