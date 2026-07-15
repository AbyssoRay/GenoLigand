from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any, Callable


SCHEMA_VERSION = "genoligand_plinder_published_index_acceptance_v1"
PUBLISHER_SCHEMA_VERSION = "genoligand_publisher_inventory_cache_v1"
CANONICAL_SCHEMA_VERSION = "genoligand_plinder_canonical_v1"
CANONICAL_HEADER = [
    "source",
    "target_id",
    "target_name",
    "organism",
    "target_class",
    "ligand_tokens",
    "sequence_tokens",
    "structure_tokens",
    "evidence",
]
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
REQUIRED_SEMANTICS = {"structure_id", "system_id", "ligand_id"}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def integer(value: object, label: str) -> int:
    try:
        result = int(str(value))
    except (TypeError, ValueError) as error:
        raise RuntimeError(f"{label} is not an integer: {value}") from error
    require(result >= 0, f"{label} is negative: {result}")
    return result


def is_sha256(value: object) -> bool:
    text = str(value)
    return len(text) == 64 and all(character in "0123456789abcdefABCDEF" for character in text)


def load_json(path: Path, label: str) -> dict[str, Any]:
    require(path.is_file(), f"missing {label}: {path}")
    try:
        payload = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"invalid {label}: {path}: {error}") from error
    require(isinstance(payload, dict), f"{label} must be a JSON object: {path}")
    return payload


def read_source_row(path: Path, source_id: str, label: str) -> dict[str, str]:
    require(path.is_file(), f"missing {label}: {path}")
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            rows = [dict(row) for row in csv.DictReader(handle, delimiter="\t")]
    except (OSError, UnicodeError, csv.Error) as error:
        raise RuntimeError(f"invalid {label}: {path}: {error}") from error
    matches = [row for row in rows if row.get("source_id") == source_id]
    require(len(matches) == 1, f"{label} must contain exactly one {source_id} row")
    return matches[0]


def sha256_file(path: Path, cache: dict[Path, str]) -> str:
    resolved = path.resolve()
    cached = cache.get(resolved)
    if cached is not None:
        return cached
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 << 20), b""):
            digest.update(chunk)
    result = digest.hexdigest()
    cache[resolved] = result
    return result


def publisher_digest(path: Path, algorithm: str) -> str:
    if algorithm == "md5":
        digest = hashlib.md5(usedforsecurity=False)
    elif algorithm == "sha256":
        digest = hashlib.sha256()
    else:
        raise RuntimeError(f"unsupported publisher digest algorithm: {algorithm}")
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resource_manifest_file(path: Path) -> bool:
    name = path.name.lower()
    normalized = path.as_posix()
    return not (
        name.endswith(".part")
        or ".part.quarantine-" in name
        or ".rebuild-" in normalized
        or ".previous-" in normalized
        or name.endswith(".attempt.json")
        or name.endswith(".identity.json")
        or name.endswith(".tmp")
    )


def resource_summary(path: Path, cache: dict[Path, str]) -> dict[str, Any]:
    if path.is_file():
        files = [path] if resource_manifest_file(path) and path.stat().st_size > 0 else []
        root = path.parent
    elif path.is_dir():
        files = sorted(
            child
            for child in path.rglob("*")
            if child.is_file() and resource_manifest_file(child) and child.stat().st_size > 0
        )
        root = path
    else:
        files = []
        root = path
    digest = hashlib.sha256()
    total_bytes = 0
    names: list[str] = []
    for child in files:
        relative = child.name if path.is_file() else child.relative_to(root).as_posix()
        size = child.stat().st_size
        names.append(relative)
        total_bytes += size
        digest.update(relative.encode("utf-8"))
        digest.update(str(size).encode("ascii"))
        digest.update(sha256_file(child, cache).encode("ascii"))
    return {
        "file_count": len(files),
        "bytes": total_bytes,
        "sha256": digest.hexdigest() if files and total_bytes > 0 else "",
        "files": names,
    }


def safe_relative_path(value: object) -> Path:
    text = str(value).strip().replace("\\", "/")
    candidate = Path(text)
    require(
        bool(text)
        and not candidate.is_absolute()
        and not candidate.drive
        and all(part not in {"", ".", ".."} for part in candidate.parts),
        f"unsafe publisher inventory path: {value}",
    )
    return candidate


def resolve_store_path(store: Path, value: object, label: str) -> Path:
    text = str(value).strip()
    require(bool(text), f"{label} path is empty")
    candidate = Path(text)
    resolved = candidate.resolve() if candidate.is_absolute() else (store / candidate).resolve()
    try:
        resolved.relative_to(store.resolve())
    except ValueError as error:
        raise RuntimeError(f"{label} leaves science store: {text}") from error
    return resolved


def portable_path_variants(value: object) -> set[str]:
    text = str(value).strip().replace("\\", "/").rstrip("/").casefold()
    variants = {text} if text else set()
    parts = text.split("/")
    if len(parts) >= 4 and parts[:2] == ["", "mnt"] and len(parts[2]) == 1:
        variants.add(f"{parts[2]}:/" + "/".join(parts[3:]))
    elif len(text) >= 3 and text[1:3] == ":/" and text[0].isalpha():
        variants.add(f"/mnt/{text[0]}/{text[3:]}")
    return variants


def path_matches(value: object, actual: Path, store: Path) -> bool:
    declared = portable_path_variants(value)
    if not declared:
        return False
    actual_paths = portable_path_variants(actual.resolve().as_posix())
    try:
        relative = actual.resolve().relative_to(store).as_posix().rstrip("/").casefold()
    except ValueError:
        relative = ""
    return bool(declared & actual_paths) or bool(relative and relative in declared)


def inspect_parquet(path: Path) -> dict[str, Any]:
    try:
        import pyarrow.parquet as pq
    except ImportError as error:
        raise RuntimeError("pyarrow is required for PLINDER onsite acceptance") from error
    try:
        parquet = pq.ParquetFile(path)
    except Exception as error:
        raise RuntimeError(f"invalid PLINDER parquet: {path}: {error}") from error
    fields = [{"name": field.name, "type": str(field.type)} for field in parquet.schema_arrow]
    available = {field["name"] for field in fields}
    mapping = {
        semantic: resolved
        for semantic, aliases in SEMANTIC_ALIASES.items()
        if (resolved := next((alias for alias in aliases if alias in available), None)) is not None
    }
    missing = sorted(REQUIRED_SEMANTICS - mapping.keys())
    require(not missing, f"PLINDER parquet semantic columns are missing: {missing}")
    schema_json = json.dumps(fields, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return {
        "parquet_rows": int(parquet.metadata.num_rows),
        "row_groups": int(parquet.metadata.num_row_groups),
        "schema_field_count": len(fields),
        "schema_sha256": hashlib.sha256(schema_json).hexdigest(),
        "schema_mapping": mapping,
    }


def validate_publisher_inventory(
    store: Path,
    source_root: Path,
    release_identity: str,
    trust_source: str,
    plan_digest: str,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    path = store / "manifests" / "publisher_inventory_cache.json"
    cache = load_json(path, "publisher inventory")
    require(
        cache.get("schema_version") == PUBLISHER_SCHEMA_VERSION,
        "PLINDER publisher inventory schema version differs",
    )
    entries = cache.get("entries")
    require(isinstance(entries, dict), "publisher inventory entries must be an object")
    entry = entries.get("plinder")
    require(isinstance(entry, dict), "publisher inventory has no PLINDER entry")
    require(entry.get("plan_digest") == plan_digest, "publisher inventory plan digest differs")
    require(entry.get("release_identity") == release_identity, "publisher inventory release differs")
    require(entry.get("trust_source") == trust_source, "publisher inventory trust source differs")
    require(str(entry.get("source_url", "")).strip(), "publisher inventory source URL is missing")
    items = entry.get("items")
    require(isinstance(items, list) and items, "publisher inventory PLINDER items are empty")

    observed: list[dict[str, Any]] = []
    seen: set[str] = set()
    for raw in items:
        require(isinstance(raw, dict), "publisher inventory item must be an object")
        relative = safe_relative_path(raw.get("relative_path", ""))
        relative_text = relative.as_posix()
        require(relative_text not in seen, f"duplicate publisher inventory item: {relative_text}")
        seen.add(relative_text)
        actual = (source_root / relative).resolve()
        try:
            actual.relative_to(source_root)
        except ValueError as error:
            raise RuntimeError(f"publisher item leaves PLINDER source root: {relative_text}") from error
        require(actual.is_file() and actual.stat().st_size > 0, f"publisher item is missing: {actual}")
        size = integer(raw.get("size"), f"publisher item {relative_text} size")
        require(size == actual.stat().st_size, f"publisher item size differs: {relative_text}")
        algorithm = str(raw.get("digest_algorithm", "")).lower()
        expected_digest = str(raw.get("digest", "")).lower()
        expected_length = 32 if algorithm == "md5" else 64 if algorithm == "sha256" else 0
        require(
            expected_length > 0
            and len(expected_digest) == expected_length
            and all(character in "0123456789abcdef" for character in expected_digest),
            f"publisher item digest is invalid: {relative_text}",
        )
        require(
            publisher_digest(actual, algorithm) == expected_digest,
            f"publisher item digest differs: {relative_text}",
        )
        require(str(raw.get("media_url", "")).startswith("https://"), f"publisher item URL is invalid: {relative_text}")
        observed.append(
            {
                "relative_path": relative_text,
                "path": actual,
                "size": size,
                "digest_algorithm": algorithm,
                "digest": expected_digest,
            }
        )
    return {"path": path, "entry": entry}, observed


def query_canonical(path: Path) -> dict[str, str]:
    count = 0
    sample: dict[str, str] | None = None
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            require(reader.fieldnames == CANONICAL_HEADER, f"canonical PLINDER header differs: {reader.fieldnames}")
            for row in reader:
                count += 1
                if sample is None:
                    sample = dict(row)
    except (OSError, UnicodeError, csv.Error) as error:
        raise RuntimeError(f"cannot read canonical PLINDER artifact: {error}") from error
    require(count > 0 and sample is not None, "canonical PLINDER artifact has no rows")
    require(
        sample.get("source") == "PLINDER"
        and bool(sample.get("target_id"))
        and bool(sample.get("target_name"))
        and bool(sample.get("evidence")),
        "canonical PLINDER sample row is incomplete",
    )
    target_id = str(sample["target_id"])
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        match = next(
            (dict(row) for row in csv.DictReader(handle, delimiter="\t") if row.get("target_id") == target_id),
            None,
        )
    require(match is not None, f"canonical PLINDER query returned no row: {target_id}")
    return {
        "row_count": str(count),
        "target_id": target_id,
        "target_name": str(match["target_name"]),
        "evidence": str(match["evidence"]),
    }


def validate(
    science_store: Path,
    parquet_inspector: Callable[[Path], dict[str, Any]] = inspect_parquet,
) -> dict[str, Any]:
    store = science_store.resolve()
    require(store.is_dir(), f"science store does not exist: {store}")
    execution_path = store / "external_sync_execution_manifest.tsv"
    source_manifest_path = store / "manifests" / "source_file_manifest.tsv"
    execution = read_source_row(execution_path, "plinder", "external sync execution manifest")
    source_manifest = read_source_row(source_manifest_path, "plinder", "source file manifest")
    require(execution.get("status") == "ready", "PLINDER external sync is not ready")
    require(source_manifest.get("status") == "ready", "PLINDER source manifest is not ready")

    release_identity = str(execution.get("release_identity", "")).strip()
    trust_source = str(execution.get("trust_source", "")).strip()
    plan_digest = str(execution.get("plan_digest", "")).lower()
    require(release_identity, "PLINDER execution release identity is missing")
    require(trust_source, "PLINDER execution trust source is missing")
    require(is_sha256(plan_digest), "PLINDER execution plan digest is invalid")
    require(source_manifest.get("release_identity") == release_identity, "PLINDER source release differs")
    require(source_manifest.get("trust_source") == trust_source, "PLINDER source trust source differs")

    source_root = resolve_store_path(store, execution.get("target_rel_path"), "PLINDER execution target")
    require(source_root.is_dir(), f"PLINDER source root is missing: {source_root}")
    publisher, publisher_items = validate_publisher_inventory(
        store, source_root, release_identity, trust_source, plan_digest
    )

    canonical_path = store / "indexes" / "plinder_targets.tsv"
    canonical_manifest_path = store / "indexes" / "plinder_targets.manifest.json"
    canonical_manifest = load_json(canonical_manifest_path, "PLINDER canonical manifest")
    require(
        canonical_manifest.get("schema_version") == CANONICAL_SCHEMA_VERSION
        and canonical_manifest.get("status") == "ready",
        "PLINDER canonical manifest is not ready",
    )
    source_meta = canonical_manifest.get("source")
    canonical_meta = canonical_manifest.get("canonical")
    require(isinstance(source_meta, dict), "PLINDER canonical source metadata is missing")
    require(isinstance(canonical_meta, dict), "PLINDER canonical artifact metadata is missing")
    source_matches = [
        item for item in publisher_items if path_matches(source_meta.get("path"), item["path"], store)
    ]
    require(len(source_matches) == 1, "PLINDER canonical source is not bound to one publisher item")
    source_path = source_matches[0]["path"]
    require(path_matches(canonical_meta.get("path"), canonical_path, store), "PLINDER canonical path differs")
    require(canonical_path.is_file() and canonical_path.stat().st_size > 0, f"missing canonical PLINDER artifact: {canonical_path}")

    hash_cache: dict[Path, str] = {}
    source_sha256 = sha256_file(source_path, hash_cache)
    canonical_sha256 = sha256_file(canonical_path, hash_cache)
    require(source_meta.get("sha256") == source_sha256, "PLINDER parquet digest differs from canonical manifest")
    require(integer(source_meta.get("size_bytes"), "canonical source bytes") == source_path.stat().st_size, "PLINDER parquet bytes differ")
    require(canonical_meta.get("sha256") == canonical_sha256, "canonical PLINDER digest differs")
    require(integer(canonical_meta.get("size_bytes"), "canonical artifact bytes") == canonical_path.stat().st_size, "canonical PLINDER bytes differ")

    source_local_path = resolve_store_path(store, source_manifest.get("local_path"), "PLINDER source manifest")
    require(
        source_local_path == source_root or source_local_path == source_path,
        "PLINDER source manifest path is not bound to the publisher source",
    )
    source_summary = resource_summary(source_local_path, hash_cache)
    require(integer(source_manifest.get("file_count"), "source manifest file count") == source_summary["file_count"], "PLINDER source manifest file count differs")
    require(integer(source_manifest.get("bytes"), "source manifest bytes") == source_summary["bytes"], "PLINDER source manifest bytes differ")
    require(source_manifest.get("sha256", "").lower() == source_summary["sha256"], "PLINDER source manifest digest differs")

    root_summary = resource_summary(source_root, hash_cache)
    require(integer(execution.get("bytes"), "external sync bytes") == root_summary["bytes"], "PLINDER external sync bytes differ")
    require(execution.get("sha256", "").lower() == root_summary["sha256"], "PLINDER external sync digest differs")
    require(
        set(root_summary["files"]) == {item["relative_path"] for item in publisher_items},
        "PLINDER publisher inventory and local source files differ",
    )

    parquet = parquet_inspector(source_path)
    for key in ("parquet_rows", "row_groups", "schema_field_count"):
        require(integer(source_meta.get(key), f"canonical source {key}") == integer(parquet.get(key), f"actual parquet {key}"), f"PLINDER parquet {key} differs")
    require(source_meta.get("schema_sha256") == parquet.get("schema_sha256"), "PLINDER parquet schema digest differs")
    require(source_meta.get("schema_mapping") == parquet.get("schema_mapping"), "PLINDER parquet schema mapping differs")
    require(canonical_manifest.get("schema_mapping") == parquet.get("schema_mapping"), "PLINDER canonical schema mapping differs")

    query = query_canonical(canonical_path)
    canonical_rows = integer(query["row_count"], "actual canonical rows")
    parquet_rows = integer(parquet.get("parquet_rows"), "actual parquet rows")
    require(integer(canonical_meta.get("row_count"), "manifest canonical rows") == canonical_rows, "canonical PLINDER row count differs")
    require(0 < canonical_rows <= parquet_rows, "PLINDER parquet-to-canonical row counts are invalid")
    provenance = str(source_manifest.get("provenance", "")).rstrip("/")
    publisher_url = str(publisher["entry"].get("source_url", "")).rstrip("/")
    require(not provenance or provenance == publisher_url, "PLINDER publisher and source provenance differ")

    return {
        "schema_version": SCHEMA_VERSION,
        "status": "ready",
        "science_store": str(store),
        "release_binding": {
            "release_identity": release_identity,
            "trust_source": trust_source,
            "plan_digest": plan_digest,
        },
        "publisher_inventory": {
            "path": str(publisher["path"]),
            "object_count": len(publisher_items),
            "objects": [
                {key: value for key, value in item.items() if key != "path"}
                for item in publisher_items
            ],
        },
        "source_manifest": {
            "path": str(source_manifest_path),
            "local_path": str(source_local_path),
            "file_count": source_summary["file_count"],
            "bytes": source_summary["bytes"],
            "sha256": source_summary["sha256"],
        },
        "conversion": {
            "parquet": {
                "path": str(source_path),
                "rows": parquet_rows,
                "row_groups": integer(parquet.get("row_groups"), "actual parquet row groups"),
                "sha256": source_sha256,
            },
            "canonical": {
                "path": str(canonical_path),
                "rows": canonical_rows,
                "sha256": canonical_sha256,
            },
            "filtered_rows": parquet_rows - canonical_rows,
        },
        "query_smoke": {
            "target_id": query["target_id"],
            "target_name": query["target_name"],
            "evidence": query["evidence"],
        },
    }


def atomic_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        with pending.open("x", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(pending, path)
    finally:
        pending.unlink(missing_ok=True)


def write_tsv(path: Path, fieldnames: list[str], row: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        writer.writerow(row)


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="genoligand-plinder-acceptance-") as temporary:
        store = Path(temporary) / "science_store"
        source_root = store / "downloads" / "plinder" / "index"
        indexes = store / "indexes"
        manifests = store / "manifests"
        source_root.mkdir(parents=True)
        indexes.mkdir(parents=True)
        manifests.mkdir(parents=True)
        source = source_root / "annotation_table.parquet"
        source.write_bytes(b"PAR1genoligand-plinder-self-testPAR1")
        canonical = indexes / "plinder_targets.tsv"
        canonical.write_text(
            "\t".join(CANONICAL_HEADER)
            + "\nPLINDER\tSYS-1\tPLINDER 1ABC system SYS-1\tunknown\tstructure_record\tlig\tchain\t1abc sys 1\tplinder_2024-06_v2_parquet;structure_id=1ABC;system_id=SYS-1;ligand_id=LIG\n"
            + "PLINDER\tSYS-2\tPLINDER 2ABC system SYS-2\tunknown\tstructure_record\tlig\tchain\t2abc sys 2\tplinder_2024-06_v2_parquet;structure_id=2ABC;system_id=SYS-2;ligand_id=LIG\n",
            encoding="utf-8",
        )
        hash_cache: dict[Path, str] = {}
        source_sha = sha256_file(source, hash_cache)
        canonical_sha = sha256_file(canonical, hash_cache)
        mapping = {"structure_id": "entry_pdb_id", "system_id": "system_id", "ligand_id": "ligand_ccd_code"}
        parquet_summary = {
            "parquet_rows": 2,
            "row_groups": 1,
            "schema_field_count": 3,
            "schema_sha256": "b" * 64,
            "schema_mapping": mapping,
        }
        (indexes / "plinder_targets.manifest.json").write_text(
            json.dumps(
                {
                    "schema_version": CANONICAL_SCHEMA_VERSION,
                    "status": "ready",
                    "source": {
                        "path": str(source.resolve()),
                        **parquet_summary,
                        "sha256": source_sha,
                        "size_bytes": source.stat().st_size,
                    },
                    "canonical": {
                        "path": str(canonical.resolve()),
                        "row_count": 2,
                        "sha256": canonical_sha,
                        "size_bytes": canonical.stat().st_size,
                    },
                    "schema_mapping": mapping,
                    "scan": {"batch_size": 4096, "batch_count": 1, "max_batch_rows": 2},
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        plan_digest = "c" * 64
        release_identity = "PLINDER 2024-06 v2 index"
        trust_source = "publisher_versioned_gcs_parquet_schema"
        publisher_md5 = hashlib.md5(source.read_bytes(), usedforsecurity=False).hexdigest()
        (manifests / "publisher_inventory_cache.json").write_text(
            json.dumps(
                {
                    "schema_version": PUBLISHER_SCHEMA_VERSION,
                    "entries": {
                        "plinder": {
                            "plan_digest": plan_digest,
                            "release_identity": release_identity,
                            "trust_source": trust_source,
                            "source_url": "gs://plinder/2024-06/v2/index/",
                            "items": [
                                {
                                    "relative_path": source.name,
                                    "size": source.stat().st_size,
                                    "digest_algorithm": "md5",
                                    "digest": publisher_md5,
                                    "media_url": "https://storage.googleapis.com/plinder/annotation_table.parquet",
                                }
                            ],
                            "updated_at": 1,
                        }
                    },
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        root_summary = resource_summary(source_root, hash_cache)
        write_tsv(
            store / "external_sync_execution_manifest.tsv",
            ["source_id", "source_label", "target_rel_path", "status", "bytes", "sha256", "plan_digest", "release_identity", "trust_source", "notes"],
            {
                "source_id": "plinder",
                "source_label": "PLINDER",
                "target_rel_path": "downloads/plinder/index",
                "status": "ready",
                "bytes": root_summary["bytes"],
                "sha256": root_summary["sha256"],
                "plan_digest": plan_digest,
                "release_identity": release_identity,
                "trust_source": trust_source,
                "notes": "self_test",
            },
        )
        write_tsv(
            manifests / "source_file_manifest.tsv",
            ["source_id", "source_label", "source_class", "local_path", "status", "file_count", "bytes", "sha256", "provenance", "release_identity", "trust_source"],
            {
                "source_id": "plinder",
                "source_label": "PLINDER",
                "source_class": "structure_interaction",
                "local_path": "downloads/plinder/index",
                "status": "ready",
                "file_count": root_summary["file_count"],
                "bytes": root_summary["bytes"],
                "sha256": root_summary["sha256"],
                "provenance": "gs://plinder/2024-06/v2/index/",
                "release_identity": release_identity,
                "trust_source": trust_source,
            },
        )
        result = validate(store, lambda _: dict(parquet_summary))
        require(result["conversion"]["canonical"]["rows"] == 2, "PLINDER self-test row count changed")
        require(result["query_smoke"]["target_id"] == "SYS-1", "PLINDER self-test query changed")
        require(
            path_matches(source.resolve(), source, store),
            "PLINDER self-test absolute path binding changed",
        )
        source_absolute = source.resolve().as_posix()
        if len(source_absolute) >= 3 and source_absolute[1:3] == ":/":
            wsl_path = f"/mnt/{source_absolute[0].lower()}/{source_absolute[3:]}"
            require(
                path_matches(wsl_path, source, store),
                "PLINDER self-test WSL path binding changed",
            )
        require(
            not path_matches(
                f"Z:/forged/{source.relative_to(store).as_posix()}", source, store
            ),
            "PLINDER self-test accepted a suffix-only forged path",
        )
    print("plinder_published_index_self_test PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate the published PLINDER canonical index.")
    parser.add_argument("--science-store", type=Path)
    parser.add_argument("--evidence-out", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    require(args.science_store is not None, "--science-store is required")
    store = args.science_store.resolve()
    evidence = (
        args.evidence_out.resolve()
        if args.evidence_out
        else store / "manifests" / "plinder_published_index_acceptance.json"
    )
    payload = validate(store)
    atomic_json(evidence, payload)
    print(f"plinder_published_index PASS evidence={evidence}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"plinder_published_index FAIL {error}", file=sys.stderr)
        raise SystemExit(1)
