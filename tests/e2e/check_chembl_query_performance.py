from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import sqlite3
import sys
import tempfile
import time
from pathlib import Path


DEFAULT_MAX_P95_SECONDS = 2.0
DEFAULT_MAX_PEAK_RSS_BYTES = 256 << 20
DEFAULT_MAX_TEMP_BYTES = 1 << 20


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def peak_rss_bytes() -> int:
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes

        class ProcessMemoryCounters(ctypes.Structure):
            _fields_ = [
                ("cb", wintypes.DWORD),
                ("PageFaultCount", wintypes.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t),
            ]

        counters = ProcessMemoryCounters()
        counters.cb = ctypes.sizeof(counters)
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        kernel32.GetCurrentProcess.restype = wintypes.HANDLE
        psapi.GetProcessMemoryInfo.argtypes = [
            wintypes.HANDLE,
            ctypes.POINTER(ProcessMemoryCounters),
            wintypes.DWORD,
        ]
        psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
        process = kernel32.GetCurrentProcess()
        ok = psapi.GetProcessMemoryInfo(process, ctypes.byref(counters), counters.cb)
        require(bool(ok), f"GetProcessMemoryInfo failed: {ctypes.get_last_error()}")
        return int(counters.PeakWorkingSetSize)

    import resource

    value = int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return value if sys.platform == "darwin" else value * 1024


def directory_bytes(root: Path) -> int:
    return sum(path.stat().st_size for path in root.rglob("*") if path.is_file())


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(4 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def percentile_95(values: list[float]) -> float:
    ordered = sorted(values)
    index = max(0, math.ceil(len(ordered) * 0.95) - 1)
    return ordered[index]


def atomic_json(path: Path, payload: dict[str, object]) -> None:
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


def measure(
    science_store: Path,
    iterations: int,
    max_p95_seconds: float,
    max_peak_rss_bytes: int,
    max_temp_bytes: int,
) -> dict[str, object]:
    database = science_store / "downloads" / "chembl" / "chembl_query.sqlite"
    require(database.is_file() and database.stat().st_size > 0, f"missing database: {database}")
    manifest_path = database.with_suffix(".manifest.json")
    require(manifest_path.is_file(), f"missing publication manifest: {manifest_path}")
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"invalid publication manifest: {error}") from error
    require(isinstance(manifest, dict), "publication manifest must be an object")
    require(
        manifest.get("schema_version") == "genoligand_chembl_query_database_v2"
        and manifest.get("status") == "ready"
        and manifest.get("table") == "genoligand_onsite_activity",
        "publication manifest is not ready",
    )
    expected_release = str(manifest.get("source_release", ""))
    expected_digest = str(manifest.get("source_digest", ""))
    expected_database_digest = str(manifest.get("database_sha256", ""))
    require(expected_release, "publication source release is missing")
    require(
        len(expected_digest) == 64
        and all(ch in "0123456789abcdefABCDEF" for ch in expected_digest),
        "publication source digest is invalid",
    )
    require(
        len(expected_database_digest) == 64
        and int(manifest.get("database_bytes", 0) or 0) == database.stat().st_size
        and sha256_file(database).lower() == expected_database_digest.lower(),
        "published database digest does not match its manifest",
    )

    with tempfile.TemporaryDirectory(prefix="genoligand-chembl-query-") as temporary:
        temp_root = Path(temporary)
        old_temp = {name: os.environ.get(name) for name in ("SQLITE_TMPDIR", "TMPDIR", "TMP", "TEMP")}
        for name in old_temp:
            os.environ[name] = str(temp_root)
        try:
            uri = database.resolve().as_uri() + "?mode=ro"
            connection = sqlite3.connect(uri, uri=True)
            try:
                connection.execute("PRAGMA temp_store=FILE")
                connection.execute("PRAGMA query_only=ON")
                columns = {
                    row[1]
                    for row in connection.execute(
                        "PRAGMA table_info(genoligand_onsite_activity)"
                    )
                }
                required = {
                    "canonical_isomeric_smiles",
                    "target_id",
                    "source_release",
                    "source_digest",
                }
                require(required.issubset(columns), "chembl query schema is incomplete")

                indexes = []
                for row in connection.execute(
                    "PRAGMA index_list(genoligand_onsite_activity)"
                ):
                    name = str(row[1])
                    indexed_columns = [
                        item[2]
                        for item in connection.execute(f"PRAGMA index_info('{name}')")
                    ]
                    indexes.append({"name": name, "columns": indexed_columns})
                require(
                    any(item["columns"][:1] == ["canonical_isomeric_smiles"] for item in indexes),
                    "canonical SMILES index is missing",
                )

                predicate = (
                    "standard_type IN ('Ki', 'Kd', 'IC50', 'EC50') "
                    "AND (standard_units = '' OR standard_units = 'nM')"
                )
                sample = connection.execute(
                    "SELECT canonical_isomeric_smiles FROM genoligand_onsite_activity "
                    "WHERE canonical_isomeric_smiles <> '' AND " + predicate + " "
                    "ORDER BY canonical_isomeric_smiles LIMIT 1"
                ).fetchone()
                require(sample is not None and sample[0], "chembl query database has no queryable canonical SMILES")
                query = str(sample[0])
                sql = (
                    "SELECT canonical_isomeric_smiles, target_id, target_name, organism, "
                    "compound_name, standard_type, standard_relation, standard_value, "
                    "standard_units, source_release, source_digest "
                    "FROM genoligand_onsite_activity "
                    "WHERE canonical_isomeric_smiles IN (?) "
                    "AND " + predicate
                )
                plan = [
                    str(row[-1])
                    for row in connection.execute("EXPLAIN QUERY PLAN " + sql, (query,))
                ]
                require(
                    all("SCAN GENOLIGAND_ONSITE_ACTIVITY" not in row.upper() for row in plan),
                    f"full table scan detected: {plan}",
                )
                require(
                    any(
                        "SEARCH GENOLIGAND_ONSITE_ACTIVITY" in row.upper()
                        and "USING INDEX" in row.upper()
                        for row in plan
                    ),
                    f"canonical SMILES index search is missing: {plan}",
                )
                require(
                    all("USE TEMP B-TREE" not in row.upper() for row in plan),
                    f"query plan requires a temporary B-tree: {plan}",
                )

                latencies = []
                row_count = 0
                source_releases: set[str] = set()
                source_digests: set[str] = set()
                temp_before = directory_bytes(temp_root)
                temp_peak = temp_before

                def sample_temp_bytes() -> int:
                    nonlocal temp_peak
                    temp_peak = max(temp_peak, directory_bytes(temp_root))
                    return 0

                connection.set_progress_handler(sample_temp_bytes, 100_000)
                for _ in range(iterations):
                    started = time.perf_counter()
                    rows = connection.execute(sql, (query,)).fetchall()
                    latencies.append(time.perf_counter() - started)
                    row_count = len(rows)
                    for row in rows:
                        source_releases.add(str(row[-2]))
                        source_digests.add(str(row[-1]))
                    sample_temp_bytes()
                connection.set_progress_handler(None, 0)
            finally:
                connection.close()
        finally:
            for name, value in old_temp.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value

    require(row_count > 0, f"indexed ChEMBL query returned no rows for {query}")
    require(source_releases and all(source_releases), "source release is missing")
    require(
        source_digests
        and all(
            len(value) == 64 and all(ch in "0123456789abcdefABCDEF" for ch in value)
            for value in source_digests
        ),
        "source digest is invalid",
    )
    require(source_releases == {expected_release}, "query source release does not match publication manifest")
    require(
        {value.lower() for value in source_digests} == {expected_digest.lower()},
        "query source digest does not match publication manifest",
    )
    p95 = percentile_95(latencies)
    peak = peak_rss_bytes()
    temp_delta = max(0, temp_peak - temp_before)
    require(p95 <= max_p95_seconds, f"p95 latency {p95:.6f}s exceeds {max_p95_seconds:.6f}s")
    require(peak <= max_peak_rss_bytes, f"peak RSS {peak} exceeds {max_peak_rss_bytes}")
    require(temp_delta <= max_temp_bytes, f"temporary bytes {temp_delta} exceeds {max_temp_bytes}")
    return {
        "schema_version": "genoligand_chembl_query_performance_v1",
        "status": "ready",
        "database": str(database),
        "database_bytes": database.stat().st_size,
        "database_sha256": expected_database_digest.lower(),
        "publication_manifest": str(manifest_path),
        "query": query,
        "query_plan": plan,
        "iterations": iterations,
        "rows": row_count,
        "latency_seconds": {
            "min": min(latencies),
            "max": max(latencies),
            "p95": p95,
        },
        "peak_rss_bytes": peak,
        "temporary_bytes": temp_delta,
        "limits": {
            "max_p95_seconds": max_p95_seconds,
            "max_peak_rss_bytes": max_peak_rss_bytes,
            "max_temp_bytes": max_temp_bytes,
        },
        "source_releases": sorted(source_releases),
        "source_digests": sorted(source_digests),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Measure the published ChEMBL query path.")
    parser.add_argument("--science-store", type=Path, required=True)
    parser.add_argument("--evidence-out", type=Path)
    parser.add_argument("--iterations", type=int, default=20)
    parser.add_argument("--max-p95-seconds", type=float, default=DEFAULT_MAX_P95_SECONDS)
    parser.add_argument("--max-peak-rss-bytes", type=int, default=DEFAULT_MAX_PEAK_RSS_BYTES)
    parser.add_argument("--max-temp-bytes", type=int, default=DEFAULT_MAX_TEMP_BYTES)
    args = parser.parse_args()
    require(args.iterations > 0, "iterations must be positive")
    store = args.science_store.resolve()
    evidence = (
        args.evidence_out.resolve()
        if args.evidence_out
        else store / "manifests" / "chembl_query_performance.json"
    )
    payload = measure(
        store,
        args.iterations,
        args.max_p95_seconds,
        args.max_peak_rss_bytes,
        args.max_temp_bytes,
    )
    atomic_json(evidence, payload)
    print(f"chembl_query_performance PASS evidence={evidence}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"chembl_query_performance FAIL {error}", file=sys.stderr)
        raise SystemExit(1)
