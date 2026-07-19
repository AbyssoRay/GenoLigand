from __future__ import annotations

import gzip
import hashlib
import http.server
import importlib.util
import json
import os
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import threading
import unittest
import urllib.error
import zipfile
from contextlib import closing, contextmanager
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "scripts/setup/install_complete_scientific.py"


def load_installer():
    spec = importlib.util.spec_from_file_location("installer_regressions", INSTALLER)
    if spec is None or spec.loader is None:
        raise RuntimeError("installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def rcsb_mmcif_text(pdb_id: str) -> str:
    return (
        f"data_{pdb_id.lower()}\n"
        f"_entry.id {pdb_id.upper()}\n"
        "loop_\n"
        "_atom_site.group_PDB\n"
        "_atom_site.Cartn_x\n"
        "_atom_site.Cartn_y\n"
        "_atom_site.Cartn_z\n"
        "ATOM 1.0 2.0 3.0\n#\n"
    )


def create_database_directory_fixture(prefix: Path, payload: bytes, entries: int = 1) -> None:
    prefix.parent.mkdir(parents=True, exist_ok=True)
    prefix.write_bytes(payload)
    Path(f"{prefix}.dbtype").write_bytes(b"\x00\x00\x00\x00")
    Path(f"{prefix}.index").write_text(
        "".join(f"{key}\t{key}\t1\n" for key in range(entries)),
        encoding="ascii",
    )
    Path(f"{prefix}.lookup").write_text(
        "".join(f"{key}\tentry_{key}\t0\n" for key in range(entries)),
        encoding="ascii",
    )


def write_structure_publication_fixture(
    installer,
    store: Path,
    contributions: list[dict[str, object]],
) -> Path:
    table = store / "indexes/structure_targets.tsv"
    rows = sum(1 for line in table.read_text(encoding="utf-8").splitlines()[1:] if line.strip())
    manifest = store / installer.STRUCTURE_TARGETS_MANIFEST_REL_PATH
    manifest.write_text(
        json.dumps(
            {
                "schema_version": installer.STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION,
                "status": "ready",
                "path": "indexes/structure_targets.tsv",
                "bytes": table.stat().st_size,
                "sha256": hashlib.sha256(table.read_bytes()).hexdigest(),
                "rows": rows,
                "contributions": contributions,
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return manifest


def directory_bytes(root: Path) -> dict[str, bytes]:
    return {
        path.relative_to(root).as_posix(): path.read_bytes()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


def write_ligand_similarity_fixture(
    installer,
    store: Path,
    *,
    source_digests: dict[str, str] | None = None,
) -> tuple[Path, Path, dict[str, object]]:
    digests = source_digests or {"chembl": "a" * 64, "bindingdb": "b" * 64}
    database = store / installer.LIGAND_SIMILARITY_DATABASE_REL_PATH
    manifest = store / installer.LIGAND_SIMILARITY_MANIFEST_REL_PATH
    database.parent.mkdir(parents=True, exist_ok=True)
    database.unlink(missing_ok=True)
    fingerprint = {
        "type": "morgan_ecfp4",
        "radius": 2,
        "bits": 2048,
        "include_chirality": True,
        "standardization": "rdkit_cleanup_fragment_parent_reionize_canonical_tautomer_v1",
        "tautomer_max_tautomers": 100,
        "tautomer_max_transforms": 100,
        "build_workers": 1,
        "lsh_bands": 0,
        "search_algorithm": "popcount_bitbound_exact_tanimoto",
        "bitbound_formula": "ceil(t*q_popcount) <= db_popcount <= floor(q_popcount/t)",
        "reference": {"name": "FPSim2", "license": "MIT", "commit": "e03e669"},
        "encoding": "rdkit_binary_text",
    }
    stable_fingerprint = {
        key: value for key, value in fingerprint.items() if key != "build_workers"
    }
    counts = {"molecules": 1, "targets": 1, "evidence": 1, "hints": 1}
    source_identity = {
        source_id: {
            "source_digest": digest,
            "source_release": "fixture-release",
        }
        for source_id, digest in digests.items()
    }
    with closing(sqlite3.connect(database)) as connection:
        connection.executescript(
            """
            CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE molecule (
                id INTEGER PRIMARY KEY,
                canonical_smiles TEXT NOT NULL UNIQUE,
                fingerprint BLOB NOT NULL,
                popcount INTEGER NOT NULL CHECK(popcount BETWEEN 1 AND 2048)
            );
            CREATE TABLE target (
                id INTEGER PRIMARY KEY,
                source TEXT NOT NULL,
                target_id TEXT NOT NULL,
                source_target_id TEXT NOT NULL,
                target_name TEXT NOT NULL,
                organism TEXT NOT NULL,
                target_class TEXT NOT NULL,
                source_release TEXT NOT NULL,
                source_digest TEXT NOT NULL,
                UNIQUE(source, target_id)
            );
            CREATE TABLE evidence (
                molecule_id INTEGER NOT NULL REFERENCES molecule(id),
                target_fk INTEGER NOT NULL REFERENCES target(id),
                activity_score REAL NOT NULL CHECK(activity_score BETWEEN 0.0 AND 1.0),
                activity TEXT NOT NULL,
                relation_rank INTEGER NOT NULL,
                numeric_rank REAL NOT NULL,
                PRIMARY KEY(molecule_id, target_fk)
            ) WITHOUT ROWID;
            CREATE TABLE structure_hint (
                molecule_id INTEGER NOT NULL REFERENCES molecule(id),
                target_fk INTEGER NOT NULL REFERENCES target(id),
                ligand_code TEXT NOT NULL,
                pdb_ids TEXT NOT NULL,
                target_chain_pdb_ids TEXT NOT NULL,
                PRIMARY KEY(molecule_id, target_fk)
            ) WITHOUT ROWID;
            CREATE INDEX idx_molecule_popcount ON molecule(popcount, id);
            """
        )
        connection.execute(
            "INSERT INTO molecule VALUES (?, ?, ?, ?)",
            (1, "CCO", sqlite3.Binary(b"\x01" + b"\x00" * 255), 1),
        )
        connection.execute(
            "INSERT INTO target VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                1,
                "chembl",
                "P12345",
                "CHEMBL_T1",
                "Target",
                "Homo sapiens",
                "protein",
                "fixture-release",
                digests["chembl"],
            ),
        )
        connection.execute(
            "INSERT INTO evidence VALUES (?, ?, ?, ?, ?, ?)",
            (1, 1, 0.9, "IC50=1nM", 0, 1.0),
        )
        connection.execute(
            "INSERT INTO structure_hint VALUES (?, ?, ?, ?, ?)",
            (1, 1, "LIG", "1ABC", "1ABC:A"),
        )
        metadata = {
            "schema_version": installer.LIGAND_SIMILARITY_SCHEMA_VERSION,
            "status": "ready",
            "counts": json.dumps(counts, sort_keys=True, separators=(",", ":")),
            "fingerprint": json.dumps(stable_fingerprint, sort_keys=True, separators=(",", ":")),
            "source_identity": json.dumps(source_identity, sort_keys=True, separators=(",", ":")),
        }
        connection.executemany(
            "INSERT INTO metadata(key, value) VALUES (?, ?)",
            sorted(metadata.items()),
        )
        connection.commit()
    output_bytes = database.stat().st_size
    input_bytes = output_bytes + 4096
    payload: dict[str, object] = {
        "schema_version": installer.LIGAND_SIMILARITY_SCHEMA_VERSION,
        "status": "ready",
        "database": {
            "path": str(database.resolve()),
            "bytes": output_bytes,
            "sha256": installer.sha256_file(database),
        },
        "inputs": {
            source_id: {
                "path": str(store / ("downloads/chembl/chembl_query.sqlite" if source_id == "chembl" else "indexes/bindingdb/bindingdb_query.sqlite")),
                "bytes": input_bytes,
                "sha256": ("c" if source_id == "chembl" else "d") * 64,
                "source_digest": digest,
                "source_release": "fixture-release",
                "manifest_path": str(store / ("downloads/chembl/chembl_query.manifest.json" if source_id == "chembl" else "indexes/bindingdb/bindingdb_query.manifest.json")),
            }
            for source_id, digest in digests.items()
        },
        "counts": counts,
        "fingerprint": fingerprint,
        "reclaimed_candidate_bytes": input_bytes * 2 - output_bytes,
    }
    installer.write_json_atomic(manifest, payload)
    return database, manifest, payload


def write_ligand_source_manifests(
    installer,
    store: Path,
    source_digests: dict[str, str] | None = None,
) -> dict[str, str]:
    digests = source_digests or {"chembl": "a" * 64, "bindingdb": "b" * 64}
    manifest = store / "manifests/source_file_manifest.tsv"
    manifest.parent.mkdir(parents=True, exist_ok=True)
    installer.write_simple_tsv(
        manifest,
        ["source_id", "status", "storage_state", "sha256", "release_identity", "trust_source"],
        [
            {
                "source_id": source_id,
                "status": "ready",
                "storage_state": "indexed_source_pruned",
                "sha256": digest,
                "release_identity": "fixture-release",
                "trust_source": "fixture_domain_validation",
            }
            for source_id, digest in digests.items()
        ],
    )
    return digests


def run_database_publish_crash_child(root: Path, stage: str) -> int:
    installer = load_installer()
    os.environ["GENOLIGAND_TEST_DATABASE_PUBLISH_HARD_EXIT_STAGE"] = stage
    installer.publish_database_directory(
        root / ".database.rebuild-fixture/target",
        root / "database/target",
    )
    return 4


def bindingdb_catalog_row(**overrides: str) -> dict[str, str]:
    row = {
        "source_id": "bindingdb",
        "url": "https://www.bindingdb.org/rwd/bind/downloads/BindingDB_All_202607_tsv.zip",
        "local_path": "downloads/bindingdb/bindingdb_all.tsv.zip",
        "sha256": "f89c851bae81657c5509ccc13586676861b6ef9ae2e8424d57c393faaf9a9ed9",
        "release_or_date": "2026-07",
        "release_identity": "BindingDB_All_202607_tsv",
        "trust_source": "publisher_md5_verified_sha256_immutable_release_domain_validation",
        "media_type": "bindingdb_tsv",
        "compression": "zip",
        "min_records": "2",
        "license_note": "BindingDB license",
    }
    row.update(overrides)
    return row


def create_chembl_fixture(root: Path) -> tuple[Path, Path, Path, Path, Path]:
    store = root / "science_store"
    chembl_dir = store / "downloads/chembl"
    chembl_dir.mkdir(parents=True)
    source_db = root / "chembl_37.db"
    with closing(sqlite3.connect(source_db)) as connection:
        connection.executescript(
            """
            CREATE TABLE activities (assay_id INTEGER, molregno INTEGER, standard_type TEXT,
                standard_relation TEXT, standard_value REAL, standard_units TEXT);
            CREATE TABLE assays (assay_id INTEGER, tid INTEGER);
            CREATE TABLE target_dictionary (tid INTEGER, chembl_id TEXT, pref_name TEXT, organism TEXT);
            CREATE TABLE molecule_dictionary (molregno INTEGER, pref_name TEXT);
            CREATE TABLE compound_structures (molregno INTEGER, canonical_smiles TEXT);
            CREATE TABLE target_components (tid INTEGER, component_id INTEGER);
            CREATE TABLE component_sequences (component_id INTEGER, accession TEXT);
            INSERT INTO activities VALUES
                (1, 1, 'IC50', '=', 7.0, 'nM'),
                (2, 2, 'Ki', '<', 8.0, NULL),
                (1, 3, 'Kd', '=', 9.0, ''),
                (1, 4, 'AC50', '=', 10.0, 'nM'),
                (1, 5, 'IC50', '=', 11.0, 'uM'),
                (1, 6, 'IC50', '=', NULL, 'nM');
            INSERT INTO assays VALUES (1, 1), (2, 2);
            INSERT INTO target_dictionary VALUES
                (1, 'CHEMBL_T1', 'Target 1', 'Homo sapiens'),
                (2, 'CHEMBL_T2', 'Target 2', 'Homo sapiens');
            INSERT INTO molecule_dictionary VALUES
                (1, 'Compound 1'), (2, 'Compound 2'), (3, 'Compound 3'),
                (4, 'Compound 4'), (5, 'Compound 5'), (6, 'Compound 6');
            INSERT INTO compound_structures VALUES
                (1, 'CCO'), (2, 'CCN'), (3, 'CCC'), (4, 'CCCl'), (5, 'CCBr'),
                (6, 'CCF');
            INSERT INTO target_components VALUES (1, 10), (1, 11), (2, 20), (2, 21);
            INSERT INTO component_sequences VALUES
                (10, 'P12345'), (11, 'P12345'), (20, 'P22222'), (21, 'P33333');
            """
        )
    archive = chembl_dir / "chembl_sqlite.tar.gz"
    with tarfile.open(archive, "w:gz") as handle:
        handle.add(source_db, arcname="chembl_37/chembl_37.db")
    catalog = root / "source_catalog.tsv"
    catalog.write_text(
        "source_id\trelease_identity\trelease_or_date\nchembl\tCHEMBL37\tCHEMBL37\n",
        encoding="utf-8",
    )
    return store, chembl_dir, source_db, archive, catalog


def create_previous_chembl_query(database: Path) -> Path:
    with closing(sqlite3.connect(database)) as connection:
        connection.executescript(
            """
            CREATE TABLE genoligand_onsite_activity (
                canonical_isomeric_smiles TEXT, target_id TEXT, source_target_id TEXT,
                target_name TEXT, organism TEXT, compound_name TEXT, standard_type TEXT,
                standard_relation TEXT, standard_value REAL, standard_units TEXT,
                source_release TEXT, source_digest TEXT
            );
            INSERT INTO genoligand_onsite_activity VALUES
                ('OLD', 'OLD_TARGET', 'OLD_SOURCE_TARGET', 'Old target', '', 'Old compound',
                 'IC50', '=', 1.0, 'nM', 'CHEMBL36', 'old-digest');
            CREATE INDEX idx_genoligand_onsite_activity_smiles
                ON genoligand_onsite_activity(canonical_isomeric_smiles);
            CREATE INDEX idx_genoligand_onsite_activity_target
                ON genoligand_onsite_activity(target_id);
            """
        )
    manifest = database.with_suffix(".manifest.json")
    manifest.write_text(
        json.dumps(
            {
                "status": "ready",
                "table": "genoligand_onsite_activity",
                "source_release": "CHEMBL36",
                "source_digest": "old-digest",
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )
    return manifest


def run_chembl_crash_child(root: Path, failpoint: int) -> int:
    installer = load_installer()
    store = root / "science_store"
    database = store / "downloads/chembl/chembl_query.sqlite"
    manifest = database.with_suffix(".manifest.json")
    transaction = installer.chembl_query_transaction_path(database)
    scratch = root / "scratch"
    real_replace = installer.os.replace
    replaced = 0

    def crash_after_publish_replace(source: str | Path, target: str | Path) -> None:
        nonlocal replaced
        source_path = Path(source).resolve()
        target_path = Path(target).resolve()
        real_replace(source, target)
        payload = installer.read_json_object(transaction)
        paths = installer.chembl_transaction_paths(database, manifest, payload, scratch)
        if paths is None:
            return
        publish_replaces = {
            (paths["live_database"].resolve(), paths["backup_database"].resolve()),
            (paths["live_manifest"].resolve(), paths["backup_manifest"].resolve()),
            (paths["staging_database"].resolve(), paths["live_database"].resolve()),
            (paths["staging_manifest"].resolve(), paths["live_manifest"].resolve()),
        }
        if (source_path, target_path) in publish_replaces:
            replaced += 1
            if replaced == failpoint:
                os._exit(80 + failpoint)

    installer.os.replace = crash_after_publish_replace
    installer.publish_chembl_query_database(
        store,
        root / "source_catalog.tsv",
        {},
        scratch_dir=scratch,
    )
    return 4


def create_sparse_file(path: Path, size: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w+b") as handle:
        if os.name == "nt":
            import ctypes
            import msvcrt
            from ctypes import wintypes

            device_io_control = ctypes.windll.kernel32.DeviceIoControl
            device_io_control.argtypes = [
                wintypes.HANDLE,
                wintypes.DWORD,
                ctypes.c_void_p,
                wintypes.DWORD,
                ctypes.c_void_p,
                wintypes.DWORD,
                ctypes.POINTER(wintypes.DWORD),
                ctypes.c_void_p,
            ]
            returned = wintypes.DWORD()
            require_sparse = device_io_control(
                wintypes.HANDLE(msvcrt.get_osfhandle(handle.fileno())),
                0x000900C4,  # FSCTL_SET_SPARSE
                None,
                0,
                None,
                0,
                ctypes.byref(returned),
                None,
            )
            if not require_sparse:
                raise OSError(ctypes.get_last_error(), "FSCTL_SET_SPARSE failed")
        handle.seek(size - 1)
        handle.write(b"\0")


@contextmanager
def resumable_server(content: bytes):
    requests: list[tuple[str, str]] = []
    etag = '"fixture-v1"'

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_HEAD(self) -> None:  # noqa: N802
            self.send_response(200)
            self.send_header("Content-Length", str(len(content)))
            self.send_header("ETag", etag)
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()

        def do_GET(self) -> None:  # noqa: N802
            range_header = self.headers.get("Range", "")
            if_range = self.headers.get("If-Range", "")
            requests.append((range_header, if_range))
            start = int(range_header.removeprefix("bytes=").removesuffix("-")) if range_header else 0
            body = content[start:]
            self.send_response(206 if range_header else 200)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("ETag", etag)
            self.send_header("Accept-Ranges", "bytes")
            if range_header:
                self.send_header("Content-Range", f"bytes {start}-{len(content) - 1}/{len(content)}")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *args: object) -> None:
            return

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/resource.bin", etag, requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@contextmanager
def interrupting_resumable_server(content: bytes, cutoff: int):
    requests: list[tuple[str, str]] = []
    state = {"interrupted": False}
    etag = '"large-fixture-v1"'

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_HEAD(self) -> None:  # noqa: N802
            self.send_response(200)
            self.send_header("Content-Length", str(len(content)))
            self.send_header("ETag", etag)
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()

        def do_GET(self) -> None:  # noqa: N802
            range_header = self.headers.get("Range", "")
            if_range = self.headers.get("If-Range", "")
            requests.append((range_header, if_range))
            start = int(range_header.removeprefix("bytes=").removesuffix("-")) if range_header else 0
            body = content[start:]
            self.send_response(206 if range_header else 200)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("ETag", etag)
            self.send_header("Accept-Ranges", "bytes")
            if range_header:
                self.send_header("Content-Range", f"bytes {start}-{len(content) - 1}/{len(content)}")
            self.end_headers()
            if not range_header and not state["interrupted"]:
                state["interrupted"] = True
                self.wfile.write(body[:cutoff])
                self.wfile.flush()
                self.close_connection = True
                return
            self.wfile.write(body)

        def log_message(self, _format: str, *args: object) -> None:
            return

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/large-resource.bin", etag, requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


class InstallerRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.installer = load_installer()

    @staticmethod
    def windows_share_error(code: int = 5) -> PermissionError:
        err = PermissionError(13, f"injected Windows sharing error {code}")
        err.winerror = code
        return err

    def test_json_atomic_retries_windows_sharing_error_with_same_temporary(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "install_progress.json"
            real_replace = installer.os.replace
            sources: list[Path] = []

            def replace_after_three_conflicts(source: Path, destination: Path) -> None:
                sources.append(Path(source))
                if len(sources) <= 3:
                    raise self.windows_share_error(5)
                real_replace(source, destination)

            with mock.patch.object(
                installer.os,
                "replace",
                side_effect=replace_after_three_conflicts,
            ), mock.patch.object(installer.time, "sleep") as sleeper, mock.patch.object(
                installer,
                "fsync_directory_if_supported",
            ) as directory_sync, mock.patch.object(
                installer,
                "windows_atomic_replace_retryable",
                side_effect=lambda err: getattr(err, "winerror", None)
                in installer.WINDOWS_ATOMIC_REPLACE_RETRY_CODES,
            ):
                installer.write_json_atomic(target, {"status": "ready", "sequence": 1})

            self.assertEqual(len(sources), 4)
            self.assertEqual(len(set(sources)), 1, "replace retries must retain one written temporary")
            self.assertEqual(
                [call.args[0] for call in sleeper.call_args_list],
                list(installer.ATOMIC_REPLACE_RETRY_DELAYS[:3]),
            )
            directory_sync.assert_called_once_with(target.parent)
            self.assertEqual(json.loads(target.read_text(encoding="utf-8"))["sequence"], 1)
            self.assertFalse(list(target.parent.glob(f".{target.name}.*.tmp")))

    def test_json_atomic_persistent_sharing_error_keeps_last_good_and_attempt(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "install_progress.json"
            target.write_text('{"status":"last-good"}\n', encoding="utf-8")
            last_good = target.read_bytes()
            error = self.windows_share_error(32)
            with mock.patch.object(installer.os, "replace", side_effect=error) as replace, mock.patch.object(
                installer.time,
                "sleep",
            ) as sleeper, mock.patch.object(
                installer,
                "windows_atomic_replace_retryable",
                side_effect=lambda err: getattr(err, "winerror", None)
                in installer.WINDOWS_ATOMIC_REPLACE_RETRY_CODES,
            ):
                with self.assertRaises(PermissionError) as raised:
                    installer.write_json_atomic(target, {"status": "new"})

            self.assertIs(raised.exception, error)
            self.assertEqual(replace.call_count, len(installer.ATOMIC_REPLACE_RETRY_DELAYS) + 1)
            self.assertEqual(sleeper.call_count, len(installer.ATOMIC_REPLACE_RETRY_DELAYS))
            self.assertEqual(target.read_bytes(), last_good)
            attempt = json.loads(Path(f"{target}.attempt.json").read_text(encoding="utf-8"))
            self.assertEqual(attempt["status"], "failed")
            self.assertEqual(attempt["winerror"], 32)
            self.assertEqual(attempt["replace_attempts"], replace.call_count)
            self.assertFalse(list(target.parent.glob(f".{target.name}.*.tmp")))

    def test_json_atomic_does_not_retry_non_sharing_error(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "install_progress.json"
            target.write_text('{"status":"last-good"}\n', encoding="utf-8")
            error = OSError(22, "injected invalid argument")
            error.winerror = 87
            with mock.patch.object(installer.os, "replace", side_effect=error) as replace, mock.patch.object(
                installer.time,
                "sleep",
            ) as sleeper:
                with self.assertRaises(OSError) as raised:
                    installer.write_json_atomic(target, {"status": "new"})

            self.assertIs(raised.exception, error)
            self.assertEqual(replace.call_count, 1)
            sleeper.assert_not_called()
            self.assertEqual(json.loads(target.read_text(encoding="utf-8"))["status"], "last-good")

    def test_json_atomic_serializes_same_target_and_publishes_complete_json(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "install_progress.json"
            real_replace = installer.os.replace
            workers = 8
            barrier = threading.Barrier(workers)
            guard = threading.Lock()
            active = 0
            maximum_active = 0
            sources: list[Path] = []
            failures: list[BaseException] = []

            def observed_replace(source: Path, destination: Path) -> None:
                nonlocal active, maximum_active
                with guard:
                    active += 1
                    maximum_active = max(maximum_active, active)
                    sources.append(Path(source))
                threading.Event().wait(0.005)
                try:
                    real_replace(source, destination)
                finally:
                    with guard:
                        active -= 1

            def writer(sequence: int) -> None:
                try:
                    barrier.wait()
                    installer.write_json_atomic(
                        target,
                        {"status": "ready", "sequence": sequence, "payload": "x" * 1024},
                    )
                except BaseException as err:  # preserve worker assertion evidence
                    failures.append(err)

            with mock.patch.object(installer.os, "replace", side_effect=observed_replace):
                threads = [threading.Thread(target=writer, args=(sequence,)) for sequence in range(workers)]
                for thread in threads:
                    thread.start()
                for thread in threads:
                    thread.join(timeout=5)

            self.assertFalse(failures)
            self.assertTrue(all(not thread.is_alive() for thread in threads))
            self.assertEqual(maximum_active, 1, "same target replace calls must be serialized")
            self.assertEqual(len(sources), workers)
            self.assertEqual(len(set(sources)), workers, "each writer must use its own temporary")
            final = json.loads(target.read_text(encoding="utf-8"))
            self.assertEqual(final["status"], "ready")
            self.assertIn(final["sequence"], range(workers))
            self.assertEqual(final["payload"], "x" * 1024)
            self.assertFalse(list(target.parent.glob(f".{target.name}.*.tmp")))

    def test_readiness_is_serialized_and_empty_manifests_block(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            (store / "source_requirements.tsv").write_text("source_id\tstatus\n", encoding="utf-8")
            result = installer.evaluate_installer_science_store_readiness(store)
            codes = {item["code"] for item in result["blockers"]}
            self.assertIn("source_requirements_empty", codes)
            self.assertEqual(result["schema_version"], installer.SCIENCE_READINESS_SCHEMA_VERSION)
            persisted = json.loads((store / "science_store_readiness.json").read_text(encoding="utf-8"))
            self.assertEqual(persisted["status"], "needs_repair")

            (store / "source_requirements.tsv").write_text(
                "source_id\tlogical_source\tstatus\trepair_action\n"
                "candidate_a\tknown_targets\tmissing\tselect_a\n"
                "candidate_b\tknown_targets\tready\tselect_b\n",
                encoding="utf-8",
            )
            result = installer.evaluate_installer_science_store_readiness(store)
            known = next(item for item in result["sources"] if item["logical_source"] == "known_targets")
            self.assertEqual(known["status"], "ready")
            self.assertEqual(known["selected_candidate"], "candidate_b")
            self.assertEqual(known["candidates"][0]["status"], "not_selected")

    def test_readiness_evidence_digest_matches_rust_contract(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            evidence, digest = installer.science_readiness_evidence(store)
            self.assertEqual(evidence, [])
            self.assertEqual(
                digest,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            )

            (store / "source_requirements.tsv").write_bytes(b"source_id\tstatus\n")
            evidence, digest = installer.science_readiness_evidence(store)
            self.assertEqual(
                evidence,
                [
                    {
                        "path": "source_requirements.tsv",
                        "sha256": "c98fc19b87dc2a95f7717e75e77f39f30d426017f11522d54592e2920f750268",
                    }
                ],
            )
            self.assertEqual(
                digest,
                "03b37f2669405833e8745f0fb5dcc7c2b0a780da126c07ad56eb43d5b65aefd5",
            )

            retention = store / "manifests/source_retention_manifest.tsv"
            scope = store / "downloads/rcsb/ligand_bound_pdb_scope.json"
            retention.parent.mkdir(parents=True)
            scope.parent.mkdir(parents=True)
            retention.write_bytes(b"schema_version\n")
            scope.write_bytes(b"{}\n")
            evidence, digest = installer.science_readiness_evidence(store)
            self.assertEqual(
                [row["path"] for row in evidence],
                [
                    "source_requirements.tsv",
                    "manifests/source_retention_manifest.tsv",
                    "downloads/rcsb/ligand_bound_pdb_scope.json",
                ],
            )
            self.assertEqual(
                digest,
                "2e9a87483b26f9dca2144590d2b2dd9d551d8ae2658c54a4d6d80d88054f62a6",
            )

    def test_readiness_requires_trusted_source_manifest_evidence(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            rows = "\n".join(
                f"{source_id}\t{source_id}\tready\t{repair}"
                for source_id, repair in installer.REQUIRED_LOGICAL_SOURCES.items()
            )
            (store / "source_requirements.tsv").write_text(
                f"source_id\tlogical_source\tstatus\trepair_action\n{rows}\n",
                encoding="utf-8",
            )
            manifest_dir = store / "manifests"
            manifest_dir.mkdir(parents=True)
            manifest_rows = "\n".join(
                f"{source_id}\tready\t{'a' * 64}\trelease-1\tfixture_domain_validation"
                for source_id in installer.REQUIRED_LOGICAL_SOURCES
                if source_id != "bindingdb"
            )
            (manifest_dir / "source_file_manifest.tsv").write_text(
                "source_id\tstatus\tsha256\trelease_identity\ttrust_source\n" + manifest_rows + "\n",
                encoding="utf-8",
            )
            result = installer.evaluate_installer_science_store_readiness(store, persist=False)
            codes = {item["code"] for item in result["blockers"]}
            self.assertIn("source_required:bindingdb", codes)

            with (manifest_dir / "source_file_manifest.tsv").open("a", encoding="utf-8", newline="") as handle:
                handle.write(f"bindingdb\tready\t{'b' * 64}\tBindingDB_All_202607_tsv\tfixture_domain_validation\n")
            result = installer.evaluate_installer_science_store_readiness(store, persist=False)
            bindingdb = next(item for item in result["sources"] if item["logical_source"] == "bindingdb")
            self.assertEqual(bindingdb["source_manifest_status"], "ready")
            self.assertEqual(bindingdb["source_digest"], "b" * 64)
            self.assertEqual(bindingdb["release_identity"], "BindingDB_All_202607_tsv")

    def test_pre_cleanup_readiness_skips_only_terminal_state_gates(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            manifest_dir = store / "manifests"
            manifest_dir.mkdir(parents=True)
            (manifest_dir / "source_file_manifest.tsv").write_text(
                "source_id\tstatus\tstorage_state\tsha256\trelease_identity\ttrust_source\n"
                f"fixture\tready\tindexed_source_pruned\t{'a' * 64}\tfixture-v1\tpublisher_digest\n",
                encoding="utf-8",
            )
            default = installer.evaluate_installer_science_store_readiness(store, persist=False)
            default_codes = {item["code"] for item in default["blockers"]}
            self.assertIn("source_retention_missing", default_codes)
            self.assertIn("install_status_not_ready", default_codes)

            pre_cleanup = installer.evaluate_installer_science_store_readiness(
                store,
                persist=False,
                check_install_status=False,
                check_source_retention=False,
            )
            pre_cleanup_codes = {item["code"] for item in pre_cleanup["blockers"]}
            self.assertNotIn("source_retention_missing", pre_cleanup_codes)
            self.assertNotIn("install_status_not_ready", pre_cleanup_codes)

    def test_current_component_results_close_a_no_download_install(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            installer.write_capacity_attempt_state(store, status="running")
            ready = {"status": "ready"}
            result = installer.installation_status(
                store,
                subprocess.CompletedProcess(["gls-cli"], 0, "", ""),
                {"status": "skipped"},
                ready,
                ready,
                ready,
                ready,
                ready,
                ready,
                ready,
            )
            self.assertEqual(result["status"], "ready")
            self.assertEqual(result["missing_components_count"], 0)

    def test_alignment_manifest_schema_and_legacy_probe_migration(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            source = store / "downloads/source.fa"
            db = store / "indexes/mmseqs/source/target"
            source.parent.mkdir(parents=True)
            db.parent.mkdir(parents=True)
            source.write_text(">id\nAAAA\n", encoding="utf-8")
            header = db.with_name("target_h")
            for path, payload in (
                (db, b"sequence-db"),
                (Path(f"{db}.dbtype"), b"\x00\x00\x00\x01"),
                (Path(f"{db}.index"), b"0\t0\t11\n"),
                (Path(f"{db}.lookup"), b"0\tid\t0\n"),
                (Path(f"{db}.source"), b"0\tsource.fa\n"),
                (header, b"\x00id\n"),
                (Path(f"{header}.dbtype"), b"\x00\x00\x00\x01"),
                (Path(f"{header}.index"), b"0\t0\t4\n"),
            ):
                path.write_bytes(payload)
            legacy = {
                "source_id": "source",
                "source_path": "downloads/source.fa",
                "db_path": "indexes/mmseqs/source/target",
                "status": "ready",
            }
            summary = installer.resource_payload_summary(source)
            with mock.patch.object(installer, "database_prefix_ready", return_value=True):
                self.assertTrue(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs", legacy, store, source, db, "mmseqs", {}, None, summary, "builder-1"
                    )
                )
                partial = {**legacy, "source_digest": summary["sha256"]}
                self.assertFalse(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs", partial, store, source, db, "mmseqs", {}, None, summary, "builder-1"
                    )
                )
                legacy.update(
                    {
                        "source_digest": summary["sha256"],
                        "source_files": str(summary["file_count"]),
                        "source_bytes": str(summary["bytes"]),
                        "builder_version": "builder-1",
                    }
                )
                self.assertTrue(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs", legacy, store, source, db, "mmseqs", {}, None, summary, "builder-1"
                    )
                )
                Path(f"{db}.source").write_text("0\tsource.fa\n1\tother.fa\n", encoding="utf-8")
                self.assertFalse(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs", legacy, store, source, db, "mmseqs", {}, None, summary, "builder-1"
                    )
                )
                Path(f"{db}.source").write_text("0\tsource.fa\n", encoding="utf-8")
                Path(f"{header}.index").unlink()
                self.assertFalse(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs", legacy, store, source, db, "mmseqs", {}, None, summary, "builder-1"
                    )
                )
                Path(f"{header}.index").write_bytes(b"0\t0\t4\n")
                self.assertFalse(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs",
                        {**legacy, "source_path": "downloads/other.fa"},
                        store,
                        source,
                        db,
                        "mmseqs",
                        {},
                        None,
                        summary,
                        "builder-1",
                    )
                )
            with mock.patch.object(installer, "database_prefix_ready", return_value=False):
                self.assertFalse(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs", legacy, store, source, db, "mmseqs", {}, None, summary, "builder-1"
                    )
                )
            fields = installer.alignment_source_manifest_fields(
                summary, "builder-1"
            )
            self.assertEqual(fields["manifest_schema_version"], installer.ALIGNMENT_MANIFEST_SCHEMA_VERSION)
            for name in ("source_digest", "source_files", "source_bytes", "builder_version"):
                self.assertTrue(fields[name])

    def test_capacity_reuses_strict_legacy_migration_and_ambiguous_rebuild_preserves_database(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            source = store / "downloads/uniprot/uniprot_human_reviewed.fasta.gz"
            source.parent.mkdir(parents=True)
            source.write_bytes(b">id\nAAAA\n")
            source_id = "uniprot_uniprot_human_reviewed.fasta"
            db = store / f"indexes/mmseqs/{source_id}/target"
            db.parent.mkdir(parents=True)
            db.write_bytes(b"last-good-createdb")
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            installer.write_simple_tsv(
                manifest,
                ["source_id", "source_path", "db_path", "status", "notes"],
                [
                    {
                        "source_id": source_id,
                        "source_path": "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
                        "db_path": f"indexes/mmseqs/{source_id}/target",
                        "status": "ready",
                        "notes": "already_available",
                    }
                ],
            )

            with mock.patch.object(
                installer, "legacy_alignment_manifest_can_migrate", return_value=True
            ) as predicate, mock.patch.object(
                installer, "database_builder_version", return_value="builder-1"
            ):
                plan = installer.installation_capacity_plan(
                    store,
                    store / "missing-catalog.tsv",
                    store / "missing-sync.tsv",
                    available_bytes=installer.DISK_SAFETY_MARGIN_BYTES * 4,
                )
            item = next(row for row in plan["items"] if row["component"] == f"alignment:{source_id}")
            self.assertEqual(item["action"], "migrate_after_probe")
            self.assertEqual(item["staging_bytes"], 0)
            self.assertEqual(predicate.call_args.args[:5], ("mmseqs", mock.ANY, store, source, db))

            last_good = db.read_bytes()
            with mock.patch.object(
                installer, "legacy_alignment_manifest_can_migrate", return_value=False
            ), mock.patch.object(
                installer, "database_builder_version", return_value="builder-1"
            ), mock.patch.object(
                installer, "build_mmseqs_database", return_value=("failed", "ambiguous_legacy_binding")
            ) as rebuild:
                rows = installer.build_mmseqs_target_databases(store, {}, None)
            self.assertTrue(rebuild.call_args.kwargs["force_rebuild"])
            self.assertEqual(rows[0]["status"], "failed")
            self.assertEqual(db.read_bytes(), last_good)

    def test_foldseek_on_demand_migrates_valid_legacy_subset_and_rejects_bad_bindings(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            coordinate_root = store / "downloads/rcsb/mmcif"
            scope = {"1ABC", "2DEF", "3GHI"}
            explicit_scope = store / "downloads/rcsb/ligand_bound_pdb_ids.txt"
            explicit_scope.parent.mkdir(parents=True, exist_ok=True)
            explicit_scope.write_text("\n".join(sorted(scope)) + "\n", encoding="utf-8")
            for pdb_id in sorted(scope):
                coordinate = coordinate_root / pdb_id[1:3].lower() / f"{pdb_id.lower()}.cif.gz"
                coordinate.parent.mkdir(parents=True, exist_ok=True)
                with gzip.open(coordinate, "wt", encoding="utf-8") as handle:
                    handle.write(rcsb_mmcif_text(pdb_id))
            snapshot = installer.write_rcsb_scope_manifest(
                store,
                coordinate_root,
                scope,
                scope_mode="on-demand",
            )
            installer.write_rcsb_on_demand_policy(store, snapshot)
            db = store / "indexes/foldseek/rcsb_mmcif/target"
            create_database_directory_fixture(db, b"last-good-foldseek", entries=4)
            listing = Path(f"{db}.source")
            lookup = Path(f"{db}.lookup")
            index = Path(f"{db}.index")
            source_text = "0\t1abc_MODEL_1\n1\t2def_MODEL_1\n"
            lookup_text = (
                "0\t1abc_A\t0\n"
                "1\t1abc_B\t0\n"
                "2\t2def_A\t1\n"
                "3\t2def_B\t1\n"
            )
            index_text = "".join(f"{key}\t{key}\t1\n" for key in range(4))
            listing.write_text(source_text, encoding="utf-8")
            lookup.write_text(lookup_text, encoding="utf-8")
            index.write_text(index_text, encoding="ascii")
            with mock.patch.object(
                installer, "database_prefix_ready", return_value=True
            ), mock.patch.object(
                installer,
                "build_foldseek_database",
                side_effect=AssertionError("valid legacy subset must not rebuild"),
            ):
                rows = installer.build_foldseek_target_database(
                    store,
                    {},
                    None,
                    rcsb_mode="on-demand",
                )
            row = rows[0]
            self.assertEqual(row["status"], "ready")
            self.assertEqual(row["manifest_schema_version"], installer.ALIGNMENT_MANIFEST_SCHEMA_VERSION)
            self.assertEqual(row["scope_status"], "ready_partial_on_demand")
            self.assertEqual(row["scope_mode"], "on_demand")
            self.assertEqual(row["indexed_pdb_count"], "2")
            self.assertEqual(row["indexed_entries"], "4")
            self.assertEqual(row["refresh_needed"], "true")
            self.assertRegex(row["source_digest"], r"^[0-9a-f]{64}$")
            self.assertRegex(row["indexed_source_listing_sha256"], r"^[0-9a-f]{64}$")
            self.assertRegex(row["indexed_mapping_sha256"], r"^[0-9a-f]{64}$")
            validated = store / row["indexed_validated_ids_manifest"]
            validated_rows = installer.read_tsv(validated)
            self.assertEqual([item["pdb_id"] for item in validated_rows], ["1ABC", "2DEF"])
            self.assertTrue(all(item["sha256"] == installer.sha256_file(store / item["coordinate_rel_path"])
                                for item in validated_rows))

            expanded_scope = {*scope, "4JKL"}
            explicit_scope.write_text("\n".join(sorted(expanded_scope)) + "\n", encoding="utf-8")
            self.assertTrue(installer.rcsb_scope_snapshot_matches_current_sources(store, snapshot))
            expanded_validation = installer.foldseek_partial_manifest_validation(
                store,
                row,
                db,
                probe_database=False,
            )
            self.assertEqual(expanded_validation["status"], "ready")

            with mock.patch.object(
                installer, "database_prefix_ready", return_value=True
            ), mock.patch.object(
                installer,
                "resource_payload_summary",
                side_effect=AssertionError("validated partial manifest must bypass the full coordinate tree digest"),
            ), mock.patch.object(
                installer,
                "build_foldseek_database",
                side_effect=AssertionError("validated partial database must be reused"),
            ):
                reused = installer.build_foldseek_target_database(
                    store,
                    {},
                    None,
                    rcsb_mode="on-demand",
                )[0]
            self.assertEqual(reused["status"], "ready")
            self.assertIn("manifest_database_probe_reused", reused["notes"])

            def validate_partial(candidate: dict[str, str] | None = None) -> dict[str, object]:
                return installer.foldseek_partial_manifest_validation(
                    store,
                    candidate or row,
                    db,
                    probe_database=False,
                )

            coordinate = coordinate_root / "ab/1abc.cif.gz"
            original_coordinate = coordinate.read_bytes()
            same_size_replacement = bytearray(original_coordinate)
            same_size_replacement[-1] ^= 1
            coordinate.write_bytes(same_size_replacement)
            self.assertEqual(coordinate.stat().st_size, len(original_coordinate))
            self.assertEqual(validate_partial()["status"], "ready")
            coordinate.write_bytes(original_coordinate)

            lookup.write_text(lookup_text.replace("1abc_A\t0", "1abc_A\t9"), encoding="utf-8")
            self.assertIn("foldseek_lookup_source_key_missing", validate_partial()["reason"])
            lookup.write_text(lookup_text.replace("2def_A\t1", "2def_A\t0"), encoding="utf-8")
            self.assertIn("foldseek_lookup_source_identity_mismatch", validate_partial()["reason"])
            lookup.write_text(
                "0\t1abc_A\t0\n1\t1abc_B\t0\n2\t1abc_C\t0\n3\t1abc_D\t0\n",
                encoding="utf-8",
            )
            self.assertIn("foldseek_source_unreferenced", validate_partial()["reason"])
            lookup.write_text(lookup_text.replace("1abc_A", "1abc_Z"), encoding="utf-8")
            self.assertEqual(validate_partial()["reason"], "foldseek_partial_mapping_digest_mismatch")
            lookup.write_text(lookup_text, encoding="utf-8")
            index.write_text(index_text.replace("3\t3\t1", "4\t3\t1"), encoding="ascii")
            self.assertIn("foldseek_index_lookup_key_mismatch", validate_partial()["reason"])
            index.write_text(index_text, encoding="ascii")

            listing.write_text(source_text.replace("2def", "3ghi"), encoding="utf-8")
            lookup.write_text(lookup_text.replace("2def", "3ghi"), encoding="utf-8")
            changed_mapping = installer.foldseek_database_mapping_summary(db)
            forged_mapping_row = {
                **row,
                "indexed_source_listing_sha256": installer.sha256_file(listing),
                "indexed_mapping_sha256": str(changed_mapping["mapping_sha256"]),
            }
            self.assertEqual(
                validate_partial(forged_mapping_row)["reason"],
                "foldseek_partial_source_validated_ids_mismatch",
            )
            listing.write_text(source_text, encoding="utf-8")
            lookup.write_text(lookup_text, encoding="utf-8")

            scope_manifest = store / "downloads/rcsb/ligand_bound_pdb_scope.tsv"
            scope_text = scope_manifest.read_text(encoding="utf-8")
            scope_manifest.write_text(
                scope_text.replace(validated_rows[0]["sha256"], "0" * 64, 1),
                encoding="utf-8",
            )
            self.assertEqual(
                validate_partial()["reason"],
                "foldseek_partial_scope_snapshot_invalid:rcsb_scope_coordinate_digest_invalid",
            )
            scope_manifest.write_text(scope_text, encoding="utf-8")

            (coordinate_root / "de/2def.cif.gz").unlink()
            with mock.patch.object(installer, "database_prefix_ready", return_value=True):
                missing = installer.foldseek_legacy_partial_migration(
                    store,
                    db,
                    None,
                    {},
                    None,
                    snapshot,
                    db.parent / "missing-evidence.tsv",
                )
            self.assertEqual(missing["reason"], "legacy_foldseek_indexed_coordinate_invalid:2DEF")
            listing.write_text("0\tnot-a-pdb-model\n", encoding="utf-8")
            malformed = installer.foldseek_legacy_partial_migration(
                store,
                db,
                None,
                {},
                None,
                snapshot,
                db.parent / "bad-evidence.tsv",
            )
            self.assertIn("foldseek_source_pdb_id_invalid", malformed["reason"])

    def test_foldseek_cross_volume_publish_trims_index_and_preserves_previous_on_failure(self) -> None:
        installer = self.installer

        def create_artifacts(
            _command: str | None,
            _source: Path,
            prefix: Path,
            _env: dict[str, str] | None,
            _wsl_distro: str | None,
        ) -> tuple[str, str]:
            prefix.parent.mkdir(parents=True, exist_ok=True)
            prefix.write_bytes(b"new-foldseek-core")
            Path(f"{prefix}.dbtype").write_bytes(b"\x00\x00\x00\x00")
            Path(f"{prefix}.index").write_bytes(b"0\t0\t4\n")
            Path(f"{prefix}.lookup").write_bytes(b"0\tfixture\n")
            Path(f"{prefix}.source").write_bytes(b"0\t1abc_MODEL_1\n")
            Path(f"{prefix}.idx").write_bytes(b"x" * 400)
            Path(f"{prefix}.idx.dbtype").write_bytes(b"\x00\x00\x00\x00")
            Path(f"{prefix}.idx.index").write_bytes(b"x" * 400)
            return "ready", "fixture_createdb_createindex_completed"

        cases = [
            ("cross_volume_success", 1 << 50, [True, True], False, "ready", True),
            (
                "compact_after_index_trim",
                installer.DISK_SAFETY_MARGIN_BYTES + 512,
                [True, True, True],
                False,
                "ready",
                False,
            ),
            ("copy_failure", 1 << 50, [True], True, "failed", None),
            ("target_probe_failure", 1 << 50, [True, False], False, "failed", None),
        ]
        for name, target_free, probes, copy_failure, expected_status, expect_index in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                source = root / "source/mmcif"
                source.mkdir(parents=True)
                (source / "1abc.cif.gz").write_bytes(b"coordinate")
                db = root / "science_store/indexes/foldseek/rcsb_mmcif/target"
                db.parent.mkdir(parents=True)
                db.write_bytes(b"last-good-foldseek")
                scratch = root / "scratch-volume"
                scratch.mkdir()
                real_copy = installer.copy_foldseek_database_with_capacity_guard

                def disk_usage(path: Path):
                    resolved = Path(path).resolve()
                    free = (1 << 50) if scratch.resolve() == resolved or scratch.resolve() in resolved.parents else target_free
                    return mock.Mock(free=free)

                copy_patch = (
                    mock.patch.object(
                        installer,
                        "copy_foldseek_database_with_capacity_guard",
                        side_effect=OSError("fixture copy failure"),
                    )
                    if copy_failure
                    else mock.patch.object(
                        installer,
                        "copy_foldseek_database_with_capacity_guard",
                        wraps=real_copy,
                    )
                )
                with mock.patch.object(
                    installer,
                    "foldseek_staging_estimate",
                    return_value={
                        "source_files": 1,
                        "source_bytes": 10,
                        "scratch_staging_bytes": 1,
                        "target_staging_bytes": 1,
                    },
                ), mock.patch.object(
                    installer.shutil, "disk_usage", side_effect=disk_usage
                ), mock.patch.object(
                    installer,
                    "create_foldseek_database_artifacts",
                    side_effect=create_artifacts,
                ), mock.patch.object(
                    installer, "database_prefix_ready", side_effect=probes
                ), mock.patch.object(
                    installer, "same_storage_volume", return_value=False
                ), copy_patch:
                    result = installer.rebuild_foldseek_database(
                        None,
                        source,
                        db,
                        {},
                        None,
                        scratch_dir=scratch,
                    )
                self.assertEqual(result[0], expected_status)
                if expected_status == "ready":
                    self.assertEqual(db.read_bytes(), b"new-foldseek-core")
                    self.assertEqual(Path(f"{db}.idx").exists(), expect_index)
                    self.assertIn("cross_volume_scratch", result[1])
                else:
                    self.assertEqual(db.read_bytes(), b"last-good-foldseek")
                self.assertEqual(list(scratch.glob("foldseek-rcsb-rebuild-*")), [])
                self.assertEqual(list(db.parent.parent.glob(".rcsb_mmcif.rebuild-*")), [])
                self.assertEqual(list(db.parent.parent.glob(".rcsb_mmcif.previous-*")), [])

    def test_database_directory_hard_interruptions_reconcile_without_losing_last_good(self) -> None:
        installer = self.installer
        for stage, expected in (
            ("after_descriptor", b"old-database"),
            ("after_backup", b"old-database"),
            ("after_live", b"new-database"),
        ):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                live = root / "database/target"
                staged = root / ".database.rebuild-fixture/target"
                create_database_directory_fixture(live, b"old-database")
                create_database_directory_fixture(staged, b"new-database")
                completed = subprocess.run(
                    [
                        sys.executable,
                        str(Path(__file__).resolve()),
                        "--database-publish-crash-child",
                        str(root),
                        stage,
                    ],
                    cwd=ROOT,
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                    errors="replace",
                    timeout=30,
                    check=False,
                )
                self.assertEqual(completed.returncode, 97, completed.stdout + completed.stderr)
                recovery = installer.reconcile_database_directory_publish(live)
                self.assertEqual(recovery["status"], "ready", recovery)
                self.assertEqual(live.read_bytes(), expected)
                self.assertFalse(installer.database_directory_transaction_path(live).exists())
                self.assertEqual(list(root.glob(".database.rebuild-*")), [])
                self.assertEqual(list(root.glob(".database.previous-*")), [])

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            live = root / "database/target"
            backups = [root / f".database.previous-{suffix}/target" for suffix in ("a", "b")]
            for index, candidate in enumerate(backups):
                create_database_directory_fixture(candidate, f"old-{index}".encode("ascii"))
            preserved = {candidate.parent: directory_bytes(candidate.parent) for candidate in backups}
            recovery = installer.reconcile_database_directory_publish(live)
            self.assertEqual(recovery["status"], "needs_repair")
            self.assertIn("ambiguous", recovery["reason"])
            self.assertFalse(live.exists())
            self.assertEqual(
                {candidate.parent: directory_bytes(candidate.parent) for candidate in backups},
                preserved,
            )

    def test_foldseek_capacity_and_main_order_use_shared_scratch_before_chembl_and_cli(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = root / "science_store"
            source = store / "downloads/rcsb/mmcif/ab/1abc.cif.gz"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"coordinate")
            db = store / "indexes/foldseek/rcsb_mmcif/target"
            db.parent.mkdir(parents=True)
            db.write_bytes(b"legacy-database")
            Path(f"{db}.source").write_text("0\t1abc_MODEL_1\n", encoding="utf-8")
            scratch = root / "scratch"
            with mock.patch.object(installer, "same_storage_volume", return_value=False):
                plan = installer.installation_capacity_plan(
                    store,
                    root / "missing-catalog.tsv",
                    root / "missing-sync.tsv",
                    available_bytes=installer.DISK_SAFETY_MARGIN_BYTES * 3,
                    scratch_dir=scratch,
                    scratch_available_bytes=installer.DISK_SAFETY_MARGIN_BYTES * 3,
                    rcsb_mode="on-demand",
                )
            item = next(row for row in plan["items"] if row["component"] == "alignment:rcsb_mmcif")
            self.assertEqual(item["action"], "migrate_partial_on_demand_after_probe")
            self.assertEqual(item["target_staging_bytes"], 0)
            self.assertEqual(item["scratch_staging_bytes"], 0)
            self.assertFalse(item["scratch_target_same_volume"])

        source_text = INSTALLER.read_text(encoding="utf-8")
        main_text = source_text[source_text.index("def main() -> int:") :]
        alignment_position = main_text.index("alignment_database_result = build_persistent_alignment_databases")
        chembl_position = main_text.index("chembl_query_result = (", alignment_position)
        retention_position = main_text.index("pre_prepare_ligand_retention = (", alignment_position)
        cli_position = main_text.index("completed = run_command(command, env=command_env)", chembl_position)
        authority_position = main_text.index("index_authority_refresh =", cli_position)
        pre_retention_readiness_position = main_text.index(
            "pre_retention_readiness = evaluate_installer_science_store_readiness(",
            authority_position,
        )
        self.assertLess(alignment_position, chembl_position)
        self.assertLess(retention_position, cli_position)
        self.assertLess(chembl_position, cli_position)
        self.assertLess(authority_position, pre_retention_readiness_position)
        self.assertNotIn("source_requirements_refresh =", main_text)
        self.assertNotIn("tarfile.open", main_text[chembl_position:cli_position])
        self.assertNotIn("chembl_sqlite.tar.gz", main_text[chembl_position:cli_position])
        self.assertIn('parser.add_argument("--rcsb-mode", choices=["on-demand", "complete"], default="on-demand")', main_text)
        self.assertIn("scratch_dir=scratch_dir", main_text[alignment_position:chembl_position])

        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(
            os.environ,
            {"LOCALAPPDATA": str(Path(tmp) / "local"), "GENOLIGAND_SCRATCH_DIR": ""},
        ):
            self.assertIn(
                'return Path(local) / "GenoLigand" / "installer-scratch"',
                source_text,
            )
            explicit = Path(tmp) / "explicit-scratch"
            with mock.patch.dict(os.environ, {"GENOLIGAND_SCRATCH_DIR": str(explicit)}):
                self.assertEqual(installer.default_installer_scratch_dir(), explicit)
        self.assertIn("else default_installer_scratch_dir().resolve()", main_text)

    def test_rcsb_scope_excludes_partial_and_binds_coordinate_snapshot(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            root = store / "downloads/rcsb/mmcif"
            coordinate = root / "ab/1abc.cif.gz"
            coordinate.parent.mkdir(parents=True)
            with gzip.open(coordinate, "wt", encoding="utf-8") as handle:
                handle.write(rcsb_mmcif_text("1ABC"))
            (store / "downloads/rcsb/ligand_bound_pdb_ids.txt").write_text(
                "1ABC\n2DEF\n",
                encoding="utf-8",
            )
            (root / "ab/1abc.cif.gz.part").write_bytes(b"partial")
            snapshot = installer.write_rcsb_scope_manifest(store, root, {"1ABC", "2DEF"})
            installer.write_rcsb_on_demand_policy(store, snapshot)
            installer.write_simple_tsv(
                store / "external_sync_execution_manifest.tsv",
                [
                    "source_id",
                    "source_label",
                    "target_rel_path",
                    "status",
                    "bytes",
                    "sha256",
                    "plan_digest",
                    "release_identity",
                    "trust_source",
                    "scope_mode",
                    "scope_digest",
                    "scope_ready_count",
                    "scope_count",
                    "scope_missing_count",
                    "notes",
                ],
                [
                    {
                        "source_id": "rcsb_ligand_bound",
                        "source_label": "RCSB ligand-bound structures",
                        "target_rel_path": "downloads/rcsb/mmcif",
                        "status": "ready",
                        "bytes": str(snapshot["coordinate_bytes"]),
                        "sha256": str(snapshot["coordinate_digest"]),
                        "plan_digest": "fixture",
                        "release_identity": "current",
                        "trust_source": "publisher_https_mmcif_parse",
                        "scope_mode": "on_demand",
                        "scope_digest": str(snapshot["scope_digest"]),
                        "scope_ready_count": str(snapshot["ready_count"]),
                        "scope_count": str(snapshot["scope_count"]),
                        "scope_missing_count": str(snapshot["missing_count"]),
                        "notes": "partial_on_demand",
                    }
                ],
            )
            self.assertEqual(snapshot["scope_count"], 2)
            self.assertEqual(snapshot["ready_count"], 1)
            self.assertEqual(snapshot["missing_count"], 1)
            self.assertEqual(snapshot["coordinate_files"], 1)
            self.assertEqual(
                installer.resource_payload_summary(root)["file_count"],
                1,
                "partial files must not enter source identity",
            )
            rows = installer.read_tsv(store / "downloads/rcsb/ligand_bound_pdb_scope.tsv")
            self.assertEqual(len(rows), 2)
            self.assertFalse(any(row["coordinate_rel_path"].endswith(".part") for row in rows))
            added = root / "gh/3ghi.cif.gz"
            added.parent.mkdir(parents=True)
            with gzip.open(added, "wt", encoding="utf-8") as handle:
                handle.write(rcsb_mmcif_text("3GHI"))
            readiness = installer.evaluate_installer_science_store_readiness(store, persist=False)
            self.assertNotIn("rcsb_partial_present", {item["code"] for item in readiness["blockers"]})
            self.assertIn("rcsb_partial_download_obsolete", {item["code"] for item in readiness["advisories"]})
            self.assertNotIn("rcsb_partial_download_preserved", {item["code"] for item in readiness["advisories"]})
            self.assertNotIn("rcsb_scope_incomplete", {item["code"] for item in readiness["blockers"]})
            self.assertTrue(readiness["rcsb_refresh_needed"])
            self.assertIn("rcsb_foldseek_refresh_needed", {item["code"] for item in readiness["advisories"]})

    def test_metagenome_readiness_requires_bound_and_probed_full_mmseqs_database(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            source = store / "downloads/metagenome/mgnify/mgy_clusters.fa.gz"
            source.parent.mkdir(parents=True)
            source.write_bytes(b">MGYP003125700799\nMSEQNNTEMT\n")
            database = store / "indexes/mmseqs/metagenome_mgy_clusters.fa/target"
            create_database_directory_fixture(database, b"MSEQNNTEMT\n")
            summary = installer.resource_payload_summary(source)
            database_manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            database_manifest.parent.mkdir(parents=True, exist_ok=True)
            database_manifest.write_text(
                "source_id\tmanifest_schema_version\tsource_namespace\tsource_class\tsource_path\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\tnotes\n"
                f"metagenome\t{installer.ALIGNMENT_MANIFEST_SCHEMA_VERSION}\tMGnify\tmetagenome\tdownloads/metagenome/mgnify/mgy_clusters.fa.gz\t{summary['sha256']}\t{summary['file_count']}\t{summary['bytes']}\ttest-builder\tindexes/mmseqs/metagenome_mgy_clusters.fa/target\tready\tvalidated\n",
                encoding="utf-8",
            )
            build_rows = [
                {
                    "index": "metagenome_targets",
                    "input_path": "downloads/metagenome/mgnify/mgy_clusters.fa.gz",
                    "status": "requires_alignment_database",
                    "rows": "0",
                    "output_path": "indexes/metagenome_targets.tsv",
                    "notes": "large_metagenome_source_uses_mmseqs_target_database",
                }
            ]
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                + "\t".join(build_rows[0][field] for field in (
                    "index", "input_path", "status", "rows", "output_path", "notes"
                ))
                + "\n",
                encoding="utf-8",
            )
            refresh = installer.refresh_science_index_authority_manifest(store)
            self.assertEqual(refresh["status"], "ready")
            self.assertEqual(refresh["updated_rows"], 1)
            build_rows = installer.read_tsv(store / "index_build_manifest.tsv")
            self.assertEqual(build_rows[0]["status"], "requires_alignment_database")
            authority_row = next(
                row for row in build_rows if row["status"] == "alignment_database_used"
            )
            self.assertEqual(
                authority_row["output_path"],
                "indexes/mmseqs/metagenome_mgy_clusters.fa/target",
            )
            self.assertEqual(
                installer.refresh_science_index_authority_manifest(store)["updated_rows"],
                0,
            )
            compatibility = store / "indexes/metagenome_targets.tsv"
            compatibility.write_text(
                "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence\n",
                encoding="utf-8",
            )
            with (
                mock.patch.object(installer, "database_builder_version", return_value="test-builder"),
                mock.patch.object(installer, "database_prefix_ready", return_value=True),
            ):
                authorities = installer.authoritative_metagenome_mmseqs_databases(store, build_rows)
                self.assertTrue(authorities)
            rows = installer.science_index_readiness_rows(
                store,
                metagenome_alignment_authorities=authorities,
            )
            authority_readiness = next(
                row for row in rows if row["index_kind"] == "alignment_database"
            )
            self.assertEqual(authority_readiness["readiness_status"], "full_index")
            self.assertEqual(
                authority_readiness["reason"],
                "full_searchable_alignment_database_ready",
            )
            self.assertTrue(installer.metagenome_compatibility_table_ready(compatibility))

            with (
                mock.patch.object(installer, "database_builder_version", return_value="test-builder"),
                mock.patch.object(installer, "database_prefix_ready", return_value=False),
            ):
                self.assertFalse(
                    installer.authoritative_metagenome_mmseqs_databases(store, build_rows)
                )
            database_manifest.write_text(
                database_manifest.read_text(encoding="utf-8").replace(
                    summary["sha256"], "f" * 64
                ),
                encoding="utf-8",
            )
            with (
                mock.patch.object(installer, "database_builder_version", return_value="test-builder"),
                mock.patch.object(installer, "database_prefix_ready", return_value=True),
            ):
                self.assertFalse(
                    installer.authoritative_metagenome_mmseqs_databases(store, build_rows)
                )
            compatibility.write_bytes(b"")
            self.assertFalse(installer.metagenome_compatibility_table_ready(compatibility))
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                "metagenome_targets\tdownloads/metagenome/mgnify/mgy_clusters.fa.gz\tused\t5000\tindexes/metagenome_targets.tsv\tfasta_sequence_rows\n",
                encoding="utf-8",
            )
            with mock.patch.object(installer, "large_index_source", return_value=True):
                rows = installer.science_index_readiness_rows(store)
            self.assertEqual(rows[0]["readiness_status"], "projection_only")

    def test_refresh_recovers_all_ready_mmseqs_authorities_without_build_rows(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            manifest_rows = []
            sources = [
                ("uniprot", "UniProt", "known", "downloads/uniprot/human.fa.gz"),
            ]
            sources.extend(
                (f"biolip_{Path(source_path).stem}", "BioLiP2", "known", source_path)
                for source_path in sorted(installer.BIOLIP2_MMSEQS_SOURCE_PATHS)
            )
            sources.append(("mgnify", "MGnify", "metagenome", "downloads/metagenome/mgnify.fa.gz"))
            for source_id, namespace, source_class, source_path in sources:
                database_path = f"indexes/mmseqs/{source_id}/target"
                create_database_directory_fixture(store / database_path, source_id.encode("ascii"))
                manifest_rows.append(
                    {
                        "source_id": source_id,
                        "manifest_schema_version": installer.ALIGNMENT_MANIFEST_SCHEMA_VERSION,
                        "source_namespace": namespace,
                        "source_class": source_class,
                        "source_path": source_path,
                        "source_digest": hashlib.sha256(source_id.encode("ascii")).hexdigest(),
                        "source_files": "1",
                        "source_bytes": "1",
                        "builder_version": "test-builder",
                        "db_path": database_path,
                        "status": "ready",
                        "notes": "validated",
                    }
                )
            installer.write_simple_tsv(
                store / "indexes/mmseqs/mmseqs_target_databases.tsv",
                list(manifest_rows[0]),
                manifest_rows,
            )
            refresh = installer.refresh_science_index_authority_manifest(store)
            self.assertEqual(refresh["status"], "ready")
            self.assertEqual(refresh["updated_rows"], 6)
            build_rows = installer.read_tsv(store / "index_build_manifest.tsv")
            self.assertEqual(len(build_rows), 6)
            self.assertEqual(
                {row["index"] for row in build_rows},
                {"known_targets", "metagenome_targets"},
            )
            self.assertEqual(
                sum("source_namespace=BioLiP2" in row["notes"] for row in build_rows),
                4,
            )
            biolip_rows = [
                row for row in manifest_rows if row["source_namespace"] == "BioLiP2"
            ]
            self.assertTrue(
                installer.mmseqs_namespace_rows_complete(biolip_rows, "BioLiP2", 4)
            )
            self.assertFalse(
                installer.mmseqs_namespace_rows_complete(
                    [dict(biolip_rows[0]) for _ in range(4)], "BioLiP2", 4
                )
            )
            self.assertEqual(
                installer.refresh_science_index_authority_manifest(store)["updated_rows"],
                0,
            )

    def test_standard_header_only_indexes_require_complete_authorities_and_no_custom_rows(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            compatibility = Path(tmp) / "known_targets.tsv"
            compatibility.write_text(
                "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
                "sequence_tokens\tstructure_tokens\tevidence\n",
                encoding="utf-8",
            )
            authority_rows = [
                {
                    "index": "known_targets",
                    "status": "alignment_database_used",
                }
            ]
            self.assertTrue(
                installer.standard_header_only_projection_ready(
                    compatibility,
                    "known_targets",
                    authority_rows,
                    True,
                    compact_ready=True,
                )
            )
            self.assertFalse(
                installer.standard_header_only_projection_ready(
                    compatibility,
                    "known_targets",
                    authority_rows,
                    True,
                    compact_ready=False,
                )
            )
            authority_rows.append(
                {
                    "index": "known_targets",
                    "status": "used",
                    "input_path": "imports/known_targets.tsv",
                }
            )
            self.assertFalse(
                installer.standard_header_only_projection_ready(
                    compatibility,
                    "known_targets",
                    authority_rows,
                    True,
                    compact_ready=True,
                )
            )

    def test_metagenome_authority_is_bound_to_each_source_and_database(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            manifest_rows = []
            build_rows = []
            databases = []
            for name, sequence in (("good", "MSEQNNTEMT"), ("bad", "MPEPTIDE")):
                input_path = f"downloads/metagenome/mgnify/{name}.fa.gz"
                output_path = f"indexes/mmseqs/{name}/target"
                source = store / input_path
                source.parent.mkdir(parents=True, exist_ok=True)
                source.write_text(f">{name}\n{sequence}\n", encoding="utf-8")
                database = store / output_path
                create_database_directory_fixture(database, f"{sequence}\n".encode())
                databases.append(database)
                summary = installer.resource_payload_summary(source)
                manifest_rows.append(
                    {
                        "source_id": name,
                        "manifest_schema_version": installer.ALIGNMENT_MANIFEST_SCHEMA_VERSION,
                        "source_namespace": "MGnify",
                        "source_class": "metagenome",
                        "source_path": input_path,
                        "source_digest": summary["sha256"],
                        "source_files": str(summary["file_count"]),
                        "source_bytes": str(summary["bytes"]),
                        "builder_version": "test-builder",
                        "db_path": output_path,
                        "status": "ready",
                        "notes": "validated",
                    }
                )
                build_rows.append(
                    {
                        "index": "metagenome_targets",
                        "input_path": input_path,
                        "status": "alignment_database_used",
                        "rows": "0",
                        "output_path": output_path,
                        "notes": (
                            "full_searchable_alignment_database;consumer=mmseqs_alignment_worker;"
                            f"source_namespace=MGnify;source_digest={summary['sha256']};"
                            "database_manifest=indexes/mmseqs/mmseqs_target_databases.tsv"
                        ),
                    }
                )
            installer.write_simple_tsv(
                store / "indexes/mmseqs/mmseqs_target_databases.tsv",
                [
                    "source_id",
                    "manifest_schema_version",
                    "source_namespace",
                    "source_class",
                    "source_path",
                    "source_digest",
                    "source_files",
                    "source_bytes",
                    "builder_version",
                    "db_path",
                    "status",
                    "notes",
                ],
                manifest_rows,
            )
            installer.write_simple_tsv(
                store / "index_build_manifest.tsv",
                ["index", "input_path", "status", "rows", "output_path", "notes"],
                build_rows,
            )

            def probe(_tool: str, database: Path, *_args: object) -> bool:
                return database == databases[0]

            with (
                mock.patch.object(installer, "database_builder_version", return_value="test-builder"),
                mock.patch.object(installer, "database_prefix_ready", side_effect=probe),
            ):
                authorities = installer.authoritative_metagenome_mmseqs_databases(
                    store,
                    build_rows,
                )
            self.assertEqual(len(authorities), 1)
            rows = installer.science_index_readiness_rows(
                store,
                metagenome_alignment_authorities=authorities,
            )
            statuses = {row["input_path"]: row["readiness_status"] for row in rows}
            self.assertEqual(statuses[build_rows[0]["input_path"]], "full_index")
            self.assertEqual(statuses[build_rows[1]["input_path"]], "incomplete")

    def test_known_index_readiness_aggregates_real_consumers_and_skips_absent_candidates(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            (store / "downloads/bindingdb").mkdir(parents=True)
            bindingdb = store / "downloads/bindingdb/bindingdb_all.tsv.zip"
            bindingdb.write_bytes(b"bindingdb")
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                "known_targets\timports/known_targets.tsv\tmissing\t0\tindexes/known_targets.tsv\tnot_present\n"
                "known_targets\timports/empty_candidate.tsv\tempty\t0\tindexes/known_targets.tsv\t\n"
                "known_targets\tdownloads/bindingdb/bindingdb_all.tsv.zip\tused\t3228554\tindexes/known_targets.tsv\tcanonical_or_source_table_rows;complete_source_scan\n"
                "known_targets\tdownloads/chembl/chembl_query.sqlite\tready\t0\tdownloads/chembl/chembl_query.sqlite\tpublished_chembl_query_used_by_onsite_matcher\n",
                encoding="utf-8",
            )
            rows = installer.science_index_readiness_rows(store)
            statuses = {(row["input_path"], row["readiness_status"]) for row in rows}
            self.assertIn(("imports/known_targets.tsv", "not_selected"), statuses)
            self.assertIn(("imports/empty_candidate.tsv", "not_selected"), statuses)
            self.assertIn(("downloads/bindingdb/bindingdb_all.tsv.zip", "full_index"), statuses)
            self.assertIn(("downloads/chembl/chembl_query.sqlite", "full_index"), statuses)

    def test_bindingdb_superseded_rows_require_exact_compact_authority(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            manifest = store / "index_build_manifest.tsv"
            fields = ["index", "input_path", "status", "rows", "output_path", "notes"]
            compact = {
                "index": "ligand_similarity",
                "input_path": "manifests/source_file_manifest.tsv",
                "status": "used",
                "rows": "100",
                "output_path": installer.LIGAND_SIMILARITY_DATABASE_REL_PATH,
                "notes": "compact_exact_tanimoto_authority;consumer=ligand_similarity_index",
            }
            superseded = {
                "index": "known_targets",
                "input_path": "downloads/bindingdb/bindingdb_all.tsv.zip",
                "status": "superseded",
                "rows": "0",
                "output_path": installer.LIGAND_SIMILARITY_DATABASE_REL_PATH,
                "notes": "compact_ligand_similarity_is_authoritative",
            }
            installer.write_simple_tsv(manifest, fields, [compact, superseded])
            rows = installer.science_index_readiness_rows(store)
            statuses = {row["input_path"]: row["readiness_status"] for row in rows}
            self.assertEqual(statuses[superseded["input_path"]], "not_selected")

            for changed in (
                {**compact, "status": "missing"},
                {**compact, "notes": "unbound"},
                {**compact, "output_path": "indexes/other.sqlite"},
            ):
                installer.write_simple_tsv(manifest, fields, [changed, superseded])
                rows = installer.science_index_readiness_rows(store)
                status = next(row for row in rows if row["input_path"] == superseded["input_path"])
                self.assertEqual(status["readiness_status"], "incomplete")

    def test_rcsb_on_demand_replaces_stale_scope_from_validated_files_and_discards_untrusted_parts(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            target = store / "downloads/rcsb/mmcif"
            scope_ids = {"1ABC", "2DEF", "3GHI"}
            explicit = store / "downloads/rcsb/ligand_bound_pdb_ids.txt"
            explicit.parent.mkdir(parents=True)
            explicit.write_text("\n".join(sorted(scope_ids)) + "\n", encoding="utf-8")
            for pdb_id in ("1ABC", "2DEF"):
                coordinate = target / pdb_id[1:3].lower() / f"{pdb_id.lower()}.cif.gz"
                coordinate.parent.mkdir(parents=True, exist_ok=True)
                with gzip.open(coordinate, "wt", encoding="utf-8") as handle:
                    handle.write(rcsb_mmcif_text(pdb_id))

            stale_manifest = store / "downloads/rcsb/ligand_bound_pdb_scope.tsv"
            stale_manifest.write_text("pdb_id\tstatus\n4OC1\tready\n", encoding="utf-8")
            installer.write_json_atomic(
                stale_manifest.with_suffix(".json"),
                {
                    "schema_version": "genoligand_rcsb_scope_snapshot_v1",
                    "scope_mode": "on_demand",
                    "scope_count": 1,
                    "scope_digest": installer.rcsb_scope_digest({"4OC1"}),
                    "ready_count": 1,
                },
            )
            installer.write_json_atomic(
                store / "install_progress.json",
                {"bytes_downloaded": 126425, "bytes_total": 153757, "status": "paused"},
            )
            obsolete = target / "gh/3ghi.cif.gz.part"
            obsolete.parent.mkdir(parents=True, exist_ok=True)
            obsolete.write_bytes(b"untrusted-partial")
            resumable = target / "jk/4jkl.cif.gz.part"
            resumable.parent.mkdir(parents=True, exist_ok=True)
            resumable.write_bytes(b"trusted-partial")
            installer.write_json_atomic(
                installer.partial_identity_path(resumable),
                {
                    "trusted": True,
                    "identity_digest": "a" * 64,
                    "url": "https://files.rcsb.org/download/4JKL.cif.gz",
                    "content_length": resumable.stat().st_size + 100,
                },
            )

            result = installer.sync_rcsb_ligand_bound_scope(
                {"source_url": "https://files.rcsb.org/download/{pdb_id}.cif.gz"},
                target,
                store,
                rcsb_mode="on-demand",
            )
            self.assertEqual(result["status"], "ready")
            self.assertEqual(result["scope_count"], 3)
            self.assertEqual(result["ready_count"], 2)
            self.assertEqual(result["missing_count"], 1)
            self.assertEqual(result["obsolete_partial_count_removed"], 1)
            self.assertEqual(result["resumable_partial_count"], 1)
            self.assertFalse(obsolete.exists())
            self.assertTrue(resumable.exists())
            self.assertEqual(
                {row["pdb_id"] for row in installer.read_tsv(stale_manifest)},
                scope_ids,
            )
            snapshot = installer.read_json_object(stale_manifest.with_suffix(".json"))
            self.assertTrue(installer.rcsb_scope_snapshot_matches_current_sources(store, snapshot))
            self.assertEqual(snapshot["scope_digest"], installer.rcsb_scope_digest(scope_ids))
            self.assertTrue((store / "downloads/rcsb/on_demand_policy.json").is_file())

    def test_rcsb_sync_keeps_submission_window_bounded(self) -> None:
        installer = self.installer
        scope = {f"{index % 10}{index:03X}" for index in range(25)}
        observed_pending: list[int] = []
        real_wait = installer.concurrent.futures.wait

        def tracked_wait(futures, **kwargs):
            observed_pending.append(len(futures))
            return real_wait(futures, **kwargs)

        snapshot = {
            "scope_count": len(scope),
            "ready_count": len(scope),
            "missing_count": 0,
            "coordinate_bytes": len(scope),
            "coordinate_digest": "a" * 64,
        }
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(
            os.environ, {"GENOLIGAND_RCSB_DOWNLOAD_WORKERS": "3"}
        ), mock.patch.object(
            installer, "ligand_bound_pdb_scope", return_value=scope
        ), mock.patch.object(
            installer, "valid_rcsb_coordinate", return_value=False
        ), mock.patch.object(
            installer, "copy_or_download", return_value={"status": "ready"}
        ), mock.patch.object(
            installer, "write_rcsb_scope_manifest", return_value=snapshot
        ), mock.patch.object(
            installer.concurrent.futures, "wait", side_effect=tracked_wait
        ):
            result = installer.sync_rcsb_ligand_bound_scope(
                {"source_url": "https://fixture/{pdb_id}.cif.gz"},
                Path(tmp) / "coordinates",
                Path(tmp),
                rcsb_mode="complete",
            )

        self.assertEqual(result["status"], "ready")
        self.assertGreater(len(observed_pending), 1)
        self.assertLessEqual(max(observed_pending), 6)

    def test_rcsb_on_demand_keeps_partial_cache_ready_with_zero_bulk_download(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            target = store / "downloads/rcsb/mmcif"
            coordinate = target / "ab/1abc.cif.gz"
            coordinate.parent.mkdir(parents=True)
            with gzip.open(coordinate, "wt", encoding="utf-8") as handle:
                handle.write(rcsb_mmcif_text("1ABC"))
            scope = {"1ABC", "2DEF"}
            explicit_scope = store / "downloads/rcsb/ligand_bound_pdb_ids.txt"
            explicit_scope.parent.mkdir(parents=True, exist_ok=True)
            explicit_scope.write_text("\n".join(sorted(scope)) + "\n", encoding="utf-8")
            plan = Path(tmp) / "source_sync_plan.tsv"
            installer.write_simple_tsv(
                plan,
                [
                    "source_id",
                    "source_label",
                    "sync_method",
                    "source_url",
                    "target_rel_path",
                    "ready_marker",
                    "release_or_date",
                    "trust_source",
                    "license_note",
                ],
                [
                    {
                        "source_id": "rcsb_ligand_bound",
                        "source_label": "RCSB ligand-bound structures",
                        "sync_method": "rcsb_ligand_bound_scope",
                        "source_url": "https://files.rcsb.org/download/{pdb_id}.cif.gz",
                        "target_rel_path": "downloads/rcsb/mmcif",
                        "ready_marker": "downloads/rcsb/mmcif",
                        "release_or_date": "current",
                        "trust_source": "publisher_https_mmcif_parse",
                        "license_note": "RCSB fixture usage policy",
                    }
                ],
            )
            with mock.patch.object(
                installer, "ligand_bound_pdb_scope", return_value=scope
            ), mock.patch.object(
                installer,
                "copy_or_download",
                side_effect=AssertionError("on-demand mode must not bulk-download missing scope"),
            ):
                result = installer.sync_external_sources(
                    plan,
                    store,
                    True,
                    rcsb_mode="on-demand",
                )
                estimate = installer.rcsb_capacity_estimate(
                    {"source_url": "https://files.rcsb.org/download/{pdb_id}.cif.gz"},
                    target,
                    store,
                    resolve_remote=True,
                    rcsb_mode="on-demand",
                )
            self.assertEqual(result["status"], "ready")
            execution = installer.read_tsv(store / "external_sync_execution_manifest.tsv")[0]
            self.assertEqual(execution["status"], "ready")
            self.assertEqual(execution["scope_mode"], "on_demand")
            self.assertEqual(execution["scope_ready_count"], "1")
            self.assertEqual(execution["scope_count"], "2")
            self.assertEqual(execution["scope_missing_count"], "1")
            self.assertIn("partial_on_demand", execution["notes"])
            self.assertEqual(estimate["download_bytes"], 0)
            self.assertEqual(estimate["action"], "reuse_partial_scope_on_demand")
            policy = json.loads(
                (store / "downloads/rcsb/on_demand_policy.json").read_text(encoding="utf-8")
            )
            self.assertEqual(
                policy,
                {
                    "schema_version": "genoligand_rcsb_on_demand_policy_v1",
                    "status": "ready",
                    "mode": "on_demand",
                    "official_base_url": "https://files.rcsb.org/download",
                    "coordinate_template": "https://files.rcsb.org/download/{pdb_id}.cif.gz",
                    "cache_root": "downloads/rcsb/mmcif",
                    "max_fetches_per_run": 12,
                    "max_cache_bytes": 2 * 1024 * 1024 * 1024,
                    "max_cache_files": 4096,
                    "validation": "gzip_mmcif_entry_and_atom_site",
                    "cached_ready_count": 1,
                    "scope_count": 2,
                },
            )
            explicit_scope.write_text("1ABC\n2DEF\n3GHI\n", encoding="utf-8")
            changed_scope = installer.rcsb_scope_snapshot_validation(
                store,
                execution=execution,
                source_summary=installer.resource_payload_summary(target),
                check_current_scope=True,
            )
            self.assertEqual(changed_scope["status"], "ready_partial_on_demand")
            self.assertTrue(changed_scope["refresh_needed"])
            explicit_scope.write_text("1ABC\n", encoding="utf-8")
            reduced_scope = installer.rcsb_scope_snapshot_validation(
                store,
                execution=execution,
                source_summary=installer.resource_payload_summary(target),
                check_current_scope=True,
            )
            self.assertEqual(reduced_scope["status"], "ready_partial_on_demand")
            self.assertTrue(reduced_scope["refresh_needed"])
            explicit_scope.write_text("1ABC\n2DEF\n3GHI\n", encoding="utf-8")
            summary = installer.source_sync_summary(
                plan,
                store,
                {**result, "failed": 0},
                rcsb_mode="on-demand",
            )
            self.assertEqual(summary["status"], "ready")
            self.assertEqual(summary["items"][0]["status"], "就绪")
            plan_row = installer.read_tsv(plan)[0]
            self.assertFalse(
                installer.external_plan_row_ready(
                    plan_row,
                    target,
                    target,
                    store,
                    execution,
                    rcsb_mode="complete",
                ),
                "complete mode must reject partial on-demand evidence",
            )

            source_catalog = Path(tmp) / "source_catalog.tsv"
            source_catalog.write_text("source_id\n", encoding="utf-8")
            manifest_result = installer.write_science_resource_manifests(
                source_catalog,
                plan,
                store,
            )
            self.assertEqual(manifest_result["status"], "ready")
            source_manifest = next(
                row
                for row in installer.read_tsv(store / "manifests/source_file_manifest.tsv")
                if row["source_id"] == "rcsb_ligand_bound"
            )
            self.assertEqual(source_manifest["status"], "ready")
            installer.write_simple_tsv(
                store / "source_requirements.tsv",
                ["source_id", "logical_source", "status", "repair_action"],
                [
                    {
                        "source_id": "rcsb_ligand_bound",
                        "logical_source": "rcsb_ligand_bound",
                        "status": "ready",
                        "repair_action": "download_and_index_rcsb_ligand_bound",
                    }
                ],
            )
            readiness = installer.evaluate_installer_science_store_readiness(store, persist=False)
            blocker_codes = {item["code"] for item in readiness["blockers"]}
            self.assertNotIn("source_required:rcsb_ligand_bound", blocker_codes)
            self.assertNotIn("rcsb_scope_incomplete", blocker_codes)
            self.assertTrue(readiness["rcsb_refresh_needed"])
            self.assertIn(
                "external_sync_execution_manifest.tsv",
                installer.SCIENCE_READINESS_EVIDENCE_PATHS,
            )

            execution_path = store / "external_sync_execution_manifest.tsv"
            execution_text = execution_path.read_text(encoding="utf-8")
            header_only = execution_text.splitlines()[0] + "\n"
            mutations = [
                ("manifest_deleted", None),
                ("execution_row_missing", header_only),
                ("execution_status_tampered", execution_text.replace("\tready\t", "\tfailed\t", 1)),
                ("execution_digest_tampered", execution_text.replace(execution["sha256"], "f" * 64, 1)),
            ]
            for label, content in mutations:
                with self.subTest(execution_evidence=label):
                    if content is None:
                        execution_path.unlink(missing_ok=True)
                    else:
                        execution_path.write_text(content, encoding="utf-8")
                    rejected = installer.evaluate_installer_science_store_readiness(
                        store,
                        persist=False,
                    )
                    rejected_codes = {item["code"] for item in rejected["blockers"]}
                    self.assertIn("source_required:rcsb_ligand_bound", rejected_codes)
                    self.assertIn("rcsb_scope_incomplete", rejected_codes)
            execution_path.write_text(execution_text, encoding="utf-8")

    def test_https_head_and_range_get_share_one_tls_context_across_workers(self) -> None:
        installer = self.installer

        class FakeContext:
            options = 0

        context = FakeContext()
        calls: list[tuple[str, str | None, object]] = []
        calls_lock = threading.Lock()

        def fake_urlopen(request, *, timeout: int, context: object):
            del timeout
            with calls_lock:
                calls.append((request.get_method(), request.get_header("Range"), context))
            return object()

        def request(index: int) -> None:
            if index % 2 == 0:
                value = installer.urllib.request.Request(
                    f"https://fixture.invalid/{index}", method="HEAD"
                )
            else:
                value = installer.urllib.request.Request(
                    f"https://fixture.invalid/{index}",
                    headers={"Range": "bytes=128-"},
                )
            installer.urlopen_with_compatible_tls(value, timeout=5)

        installer._COMPATIBLE_TLS_CONTEXT = None
        try:
            with mock.patch.object(
                installer.ssl, "create_default_context", return_value=context
            ) as create_context, mock.patch.object(
                installer.urllib.request, "urlopen", side_effect=fake_urlopen
            ):
                with installer.concurrent.futures.ThreadPoolExecutor(max_workers=20) as pool:
                    list(pool.map(request, range(200)))
            self.assertEqual(create_context.call_count, 1)
            self.assertEqual(len(calls), 200)
            self.assertTrue(any(method == "HEAD" for method, _, _ in calls))
            self.assertTrue(any(method == "GET" and byte_range == "bytes=128-" for method, byte_range, _ in calls))
            self.assertTrue(all(used_context is context for _, _, used_context in calls))
        finally:
            installer._COMPATIBLE_TLS_CONTEXT = None

    def test_remote_size_falls_back_to_content_range_and_rejects_unknown_total(self) -> None:
        installer = self.installer

        class RangeResponse:
            status = 206

            def __init__(self, content_range: str):
                self.headers = {"Content-Range": content_range, "Content-Length": "1"}

            def __enter__(self):
                return self

            def __exit__(self, _exc_type, _exc, _traceback):
                return False

        observed: list[str | None] = []

        def open_range(request, timeout: int):
            del timeout
            observed.append(request.get_header("Range"))
            return RangeResponse("bytes 0-0/987654")

        with mock.patch.object(
            installer, "remote_resource_identity", return_value={}
        ), mock.patch.object(installer, "urlopen_with_compatible_tls", side_effect=open_range):
            self.assertEqual(installer.remote_resource_size("https://fixture.invalid/data"), 987654)
        self.assertEqual(observed, ["bytes=0-0"])

        with mock.patch.object(
            installer, "remote_resource_identity", return_value={}
        ), mock.patch.object(
            installer,
            "urlopen_with_compatible_tls",
            return_value=RangeResponse("bytes 0-0/*"),
        ):
            self.assertEqual(installer.remote_resource_size("https://fixture.invalid/error"), 0)

    def test_capacity_preflight_blocks_before_large_staging(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            source = store / "downloads/metagenome/mgnify/mgy_clusters.fa.gz"
            db = store / "indexes/mmseqs/metagenome/target"
            source.parent.mkdir(parents=True)
            db.parent.mkdir(parents=True)
            source.write_bytes(b"x" * 1024)
            db.write_bytes(b"x" * 8192)
            plan = installer.installation_capacity_plan(
                store,
                Path(tmp) / "missing-catalog.tsv",
                Path(tmp) / "missing-sync.tsv",
                available_bytes=4096,
            )
            self.assertEqual(plan["status"], "blocked")
            self.assertGreater(plan["required_peak_bytes"], plan["available_bytes"])
            for item in plan["items"]:
                for field in (
                    "action",
                    "download_bytes",
                    "staging_bytes",
                    "reclaimable_bytes",
                    "required_peak_bytes",
                    "available_bytes",
                ):
                    self.assertIn(field, item)

    def test_capacity_reuses_verified_biolip_projection_with_partial_recovery_payload(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = root / "science_store"
            recovered = store / "downloads/biolip2/BioLiP.txt.gz"
            recovered.parent.mkdir(parents=True, exist_ok=True)
            recovered.write_bytes(b"recovered-annotation")
            sync_plan = root / "source_sync.tsv"
            sync_plan.write_text(
                "source_id\tsource_label\ttarget_rel_path\tready_marker\tsync_method\trelease_or_date\ttrust_source\n"
                "biolip2\tBioLiP2\tdownloads/biolip2\tdownloads/biolip2\thtml_links\tfixture-release\tfixture-trust\n",
                encoding="utf-8",
            )
            retention = {
                "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                "source_id": "biolip2",
                "source_local_path": "downloads/biolip2",
                "source_bytes": "123",
                "execution_sha256": "a" * 64,
                "release_identity": "fixture-release",
                "trust_source": "fixture-trust",
            }
            with mock.patch.object(
                installer,
                "source_retention_rows",
                return_value=[retention],
            ), mock.patch.object(
                installer,
                "retention_source_identity_and_derivatives_ready",
                return_value=True,
            ) as retained_ready, mock.patch.object(
                installer, "same_storage_volume", return_value=True
            ):
                plan = installer.installation_capacity_plan(
                    store,
                    root / "missing-catalog.tsv",
                    sync_plan,
                    available_bytes=100 * 1024 * 1024 * 1024,
                    scratch_dir=root / "scratch",
                )
                sync = installer.sync_external_sources(
                    sync_plan,
                    store,
                    skip_downloads=True,
                )
            biolip = next(item for item in plan["items"] if item["component"] == "biolip2")
            self.assertEqual(biolip["action"], "reuse_verified_index")
            self.assertNotIn("biolip2", plan["unresolved_components"])
            self.assertEqual(sync["status"], "ready")
            external_row = installer.read_tsv(store / "external_sync_execution_manifest.tsv")[0]
            self.assertEqual(external_row["status"], "ready")
            self.assertEqual(external_row["notes"], "verified_index_reused_source_payload_remains_pruned")
            self.assertGreaterEqual(retained_ready.call_count, 2)

    def test_pruned_alignment_authority_survives_without_unrelated_projection_checks(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            source_manifest = store / "manifests/source_file_manifest.tsv"
            installer.write_simple_tsv(
                source_manifest,
                [
                    "source_id",
                    "local_path",
                    "status",
                    "storage_state",
                    "file_count",
                    "bytes",
                    "sha256",
                    "release_identity",
                    "trust_source",
                ],
                [
                    {
                        "source_id": "biolip2",
                        "local_path": "downloads/biolip2",
                        "status": "ready",
                        "storage_state": "indexed_source_pruned",
                        "file_count": "4",
                        "bytes": "100",
                        "sha256": "a" * 64,
                        "release_identity": "fixture-release",
                        "trust_source": "fixture-trust",
                    }
                ],
            )
            mmseqs_manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            mmseqs_manifest.parent.mkdir(parents=True, exist_ok=True)
            mmseqs_manifest.write_text("source_id\nfixture\n", encoding="utf-8")
            installer.write_source_retention_rows(
                store,
                [
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": "biolip2",
                        "source_local_path": "downloads/biolip2",
                        "state": "pruned",
                        "source_files": "4",
                        "source_bytes": "100",
                        "source_manifest_sha256": "a" * 64,
                        "release_identity": "fixture-release",
                        "trust_source": "fixture-trust",
                        "payload_records_json": json.dumps(
                            [
                                {
                                    "path": "downloads/biolip2",
                                    "kind": "directory",
                                    "file_count": 4,
                                    "bytes": 100,
                                    "sha256": "a" * 64,
                                    "content_sha256": "",
                                }
                            ],
                            separators=(",", ":"),
                        ),
                        "derived_paths": "indexes/mmseqs/mmseqs_target_databases.tsv",
                    }
                ],
            )
            source_path = "downloads/biolip2/protein.fasta.gz"
            self.assertTrue(installer.retention_covers_alignment_source_path(store, source_path))
            restored = store / source_path
            restored.parent.mkdir(parents=True, exist_ok=True)
            restored.write_bytes(b"restored")
            self.assertFalse(installer.retention_covers_alignment_source_path(store, source_path))

    def test_rcsb_capacity_uses_observed_aggregate_size_instead_of_largest_file(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            target = store / "downloads/rcsb/mmcif"
            scope = {"1AAA", "2BBB", "3CCC", "4DDD"}
            sizes = {"1AAA": 100, "2BBB": 10_000}
            for pdb_id, size in sizes.items():
                path = target / pdb_id[1:3].lower() / f"{pdb_id.lower()}.cif.gz"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"x" * size)
            remote_sizes = {"3CCC": 120, "4DDD": 180}

            def coordinate_ready(path: Path, _expected: str = "") -> bool:
                return path.is_file()

            def remote_identity(url: str) -> dict[str, object]:
                pdb_id = Path(url).name.removesuffix(".cif.gz")
                return {"content_length": remote_sizes[pdb_id], "trusted": True}

            with mock.patch.object(installer, "ligand_bound_pdb_scope", return_value=scope), mock.patch.object(
                installer, "valid_rcsb_coordinate", side_effect=coordinate_ready
            ), mock.patch.object(installer, "remote_resource_identity", side_effect=remote_identity):
                estimate = installer.rcsb_capacity_estimate(
                    {"source_url": "https://fixture/{pdb_id}.cif.gz"},
                    target,
                    store,
                    resolve_remote=True,
                    rcsb_mode="complete",
                )

            self.assertEqual(estimate["missing_objects"], 2)
            self.assertEqual(estimate["observed_local_files"], 2)
            self.assertEqual(estimate["remote_sample_files"], 2)
            self.assertLess(estimate["download_bytes"], 2 * max(sizes.values()))
            self.assertEqual(estimate["action"], "download_missing_scope_empirical_estimate")

    def test_publisher_inventory_reuses_exact_plinder_and_replaces_changed_mgnify(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plinder = root / "plinder"
            plinder.mkdir()
            plinder_payload = b"PAR1fixturePAR1"
            (plinder / "annotation_table.parquet").write_bytes(plinder_payload)
            plinder_objects = [
                {
                    "relative_path": "annotation_table.parquet",
                    "size": len(plinder_payload),
                    "digest_algorithm": "md5",
                    "digest": hashlib.md5(plinder_payload).hexdigest(),  # noqa: S324 - publisher identity
                }
            ]
            plinder_estimate = installer.publisher_inventory_capacity(plinder_objects, plinder)
            self.assertEqual(plinder_estimate["action"], "reuse_after_publisher_identity")
            self.assertEqual(plinder_estimate["download_bytes"], 0)

            mgnify = root / "mgnify"
            mgnify.mkdir()
            (mgnify / "mgy_clusters.fa.gz").write_bytes(b"old-release")
            current_payload = b"current-release-is-larger"
            mgnify_objects = [
                {
                    "relative_path": "mgy_clusters.fa.gz",
                    "size": len(current_payload),
                    "digest_algorithm": "md5",
                    "digest": hashlib.md5(current_payload).hexdigest(),  # noqa: S324 - publisher identity
                }
            ]
            mgnify_estimate = installer.publisher_inventory_capacity(mgnify_objects, mgnify)
            self.assertEqual(mgnify_estimate["action"], "download_publisher_identity_changed")
            self.assertEqual(mgnify_estimate["download_bytes"], len(current_payload))

    def test_publisher_inventory_retries_and_reuses_only_exact_release_cache(self) -> None:
        installer = self.installer
        row = next(
            item
            for item in installer.read_tsv(ROOT / "configs/source_sync_plan.tsv")
            if item["source_id"] == "metagenome_potential"
        )
        payloads = {
            "LICENSE": b"license",
            "README.txt": b"readme",
            "md5sum.txt": b"checksums",
            "mgy_clusters.fa.gz": b"clusters",
        }
        objects = [
            {
                "relative_path": name,
                "size": len(payload),
                "digest_algorithm": "sha256",
                "digest": hashlib.sha256(payload).hexdigest(),
                "media_url": f"https://publisher.invalid/2024_04/{name}",
            }
            for name, payload in payloads.items()
        ]
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            calls = 0

            def transient_then_ready(_row: dict[str, str]) -> list[dict[str, object]]:
                nonlocal calls
                calls += 1
                if calls < 3:
                    raise urllib.error.URLError("TLS EOF")
                return objects

            with mock.patch.object(installer.time, "sleep"):
                fetched = installer.publisher_inventory_with_cache(row, store, transient_then_ready)
            self.assertEqual(fetched["source"], "remote")
            self.assertEqual(fetched["attempts"], 3)
            self.assertEqual(fetched["items"], objects)
            self.assertTrue((store / "manifests/publisher_inventory_cache.json").is_file())

            with mock.patch.object(installer.time, "sleep"):
                cached = installer.publisher_inventory_with_cache(
                    row,
                    store,
                    mock.Mock(side_effect=urllib.error.URLError("TLS EOF")),
                )
            self.assertEqual(cached["source"], "cache")
            self.assertEqual(cached["attempts"], installer.PUBLISHER_INVENTORY_ATTEMPTS)
            self.assertEqual(cached["items"], objects)
            self.assertIn("TLS EOF", cached["error"])

            changed = {
                **row,
                "source_url": row["source_url"].replace("2024_04", "2025_01"),
                "checksum_url": row["checksum_url"].replace("2024_04", "2025_01"),
                "release_or_date": "MGnify Proteins 2025_01 cluster representatives",
            }
            with mock.patch.object(installer.time, "sleep"):
                unavailable = installer.publisher_inventory_with_cache(
                    changed,
                    store,
                    mock.Mock(side_effect=urllib.error.URLError("TLS EOF")),
                )
            self.assertEqual(unavailable["source"], "unavailable")
            self.assertEqual(unavailable["items"], [])
            self.assertTrue(unavailable["error"])

    def test_unchecksummed_html_capacity_uses_complete_https_sizes_and_rejects_bad_pages(self) -> None:
        installer = self.installer
        row = next(
            item
            for item in installer.read_tsv(ROOT / "configs/source_sync_plan.tsv")
            if item["source_id"] == "biolip2"
        )
        names = [
            "BioLiP.txt.gz",
            "BioLiP_nr.txt.gz",
            "peptide.fasta.gz",
            "peptide_nr.fasta.gz",
            "protein.fasta.gz",
            "protein_nr.fasta.gz",
        ]
        objects = [
            {
                "relative_path": name,
                "size": index * 100,
                "digest_algorithm": "",
                "digest": "",
                "media_url": f"https://zhanggroup.org/BioLiP/data/{name}",
            }
            for index, name in enumerate(names, start=1)
        ]

        def plan_for(items: list[dict[str, object]]) -> tuple[dict[str, object], Path]:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            store = Path(temporary.name)
            sync_plan = store / "sync.tsv"
            installer.write_simple_tsv(sync_plan, list(row), [row])
            with mock.patch.object(installer, "html_links_inventory", return_value=items), mock.patch.object(
                installer.time, "sleep"
            ), mock.patch.object(installer, "ligand_similarity_index_ready", return_value=True):
                plan = installer.installation_capacity_plan(
                    store,
                    store / "missing-catalog.tsv",
                    sync_plan,
                    available_bytes=installer.DISK_SAFETY_MARGIN_BYTES + sum(int(item["size"]) for item in objects) + 1,
                    resolve_remote=True,
                )
            return plan, store

        ready_plan, ready_store = plan_for(objects)
        ready_item = next(item for item in ready_plan["items"] if item["component"] == "biolip2")
        self.assertEqual(ready_plan["status"], "ready")
        self.assertEqual(ready_item["download_bytes"], sum(int(item["size"]) for item in objects))
        self.assertEqual(ready_item["inventory_source"], "remote_capacity")
        self.assertFalse((ready_store / "manifests/publisher_inventory_cache.json").exists())

        wrong_page = [
            {
                "relative_path": "error.html",
                "size": 512,
                "digest_algorithm": "",
                "digest": "",
                "media_url": "https://zhanggroup.org/error.html",
            }
        ]
        for name, items in (("missing_link", objects[:-1]), ("wrong_html", wrong_page)):
            with self.subTest(name=name):
                blocked_plan, blocked_store = plan_for(items)
                blocked_item = next(
                    item for item in blocked_plan["items"] if item["component"] == "biolip2"
                )
                self.assertEqual(blocked_plan["status"], "blocked")
                self.assertIsNone(blocked_item["download_bytes"])
                self.assertIn("biolip2", blocked_plan["unresolved_components"])
                self.assertFalse((blocked_store / "manifests/publisher_inventory_cache.json").exists())

    def test_capacity_preflight_early_exits_are_terminal_and_replace_legacy_ready(self) -> None:
        installer = self.installer
        cases: list[tuple[str, object, int, str, str]] = [
            (
                "blocked",
                {
                    "schema_version": "genoligand_installation_capacity_plan_v1",
                    "status": "blocked",
                    "required_peak_bytes": 10,
                    "available_bytes": 20,
                    "unresolved_components": ["metagenome_potential"],
                },
                2,
                "blocked",
                "blocked",
            ),
            (
                "failed",
                RuntimeError("publisher TLS EOF"),
                2,
                "failed",
                "failed",
            ),
            (
                "dry_run_ready",
                {
                    "schema_version": "genoligand_installation_capacity_plan_v1",
                    "status": "ready",
                    "required_peak_bytes": 10,
                    "available_bytes": 20,
                    "unresolved_components": [],
                },
                0,
                "ready",
                "capacity_ready",
            ),
        ]
        for name, outcome, expected_code, progress_status, install_status in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                store = root / "science_store"
                store.mkdir()
                (store / "install_status.json").write_text(
                    '{"status":"ready","legacy_marker":"stale"}\n', encoding="utf-8"
                )
                args = [
                    str(INSTALLER),
                    "--workspace-root",
                    str(ROOT),
                    "--science-store",
                    str(store),
                    "--runtime-dir",
                    str(root / "runtime"),
                    "--dry-run",
                ]
                effect = outcome if isinstance(outcome, BaseException) else None
                with mock.patch.object(sys, "argv", args), mock.patch.object(
                    installer,
                    "installation_capacity_plan",
                    side_effect=effect,
                    return_value=None if effect else outcome,
                ):
                    self.assertEqual(installer.main(), expected_code)
                progress = json.loads((store / "install_progress.json").read_text(encoding="utf-8"))
                status = json.loads((store / "install_status.json").read_text(encoding="utf-8"))
                self.assertEqual(progress["status"], progress_status)
                self.assertEqual(progress["stage"], "容量预检")
                self.assertEqual(status["status"], install_status)
                self.assertNotIn("legacy_marker", status)
                self.assertFalse(status.get("install_ready", True))

    def test_plinder_capacity_resolves_gcs_scope_instead_of_remaining_unresolved(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            sync_plan = root / "sync.tsv"
            sync_plan.write_text(
                "source_id\tsync_method\tsource_url\ttarget_rel_path\tready_marker\tinclude_pattern\trelease_or_date\ttrust_source\n"
                "plinder\tgcs_prefix\tgs://plinder/2024-06/v2/index/\tdownloads/plinder/index\t"
                "downloads/plinder/index/annotation_table.parquet\t^annotation_table\\.parquet$\t"
                "PLINDER 2024-06 v2 index\tpublisher_versioned_gcs_parquet_schema\n",
                encoding="utf-8",
            )
            payload = b"PAR1fixturePAR1"
            target = root / "downloads/plinder/index"
            target.mkdir(parents=True)
            (target / "annotation_table.parquet").write_bytes(payload)
            objects = [
                {
                    "relative_path": "annotation_table.parquet",
                    "size": len(payload),
                    "digest_algorithm": "md5",
                    "digest": hashlib.md5(payload).hexdigest(),  # noqa: S324 - publisher identity
                }
            ]
            with mock.patch.object(installer, "gcs_prefix_inventory", return_value=objects):
                plan = installer.installation_capacity_plan(
                    root,
                    root / "missing-catalog.tsv",
                    sync_plan,
                    available_bytes=installer.DISK_SAFETY_MARGIN_BYTES + 1024,
                    resolve_remote=True,
                )
            item = next(item for item in plan["items"] if item["component"] == "plinder")
            self.assertEqual(item["action"], "reuse_after_publisher_identity")
            self.assertNotIn("plinder", plan["unresolved_components"])

    def test_runtime_manifest_and_install_ready_require_both_binaries(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime = root / "runtime"
            binary = root / installer.runtime_executable_name("gls-cli")
            binary.write_bytes(b"binary")
            status = {
                "binary": "gls-cli",
                "status": "ready",
                "path": str(binary),
                "sha256": hashlib.sha256(b"binary").hexdigest(),
                "version": "1.2.3",
                "commit": "abc",
                "build_profile": "release",
                "smoke": "ready",
            }
            result = installer.write_runtime_binary_manifest(runtime, [status])
            self.assertEqual(result["status"], "needs_repair")
            payload = {
                "status": "ready",
                "downloads": {"status": "ready"},
                "external_sources": {"status": "ready"},
                "resources": {"status": "ready"},
                "tool_environment": {"status": "ready"},
                "source_manifests": {"status": "ready"},
                "alignment_databases": {"status": "ready"},
                "forcefields": {"status": "ready"},
                "ligand_similarity_index": {"status": "ready"},
                "source_retention": {"status": "ready"},
                "runtime_binary_manifest": result,
                "science_store_readiness": {"status": "ready"},
            }
            self.assertFalse(installer.install_ready(payload))

    def test_runtime_launcher_persists_only_science_path_additions(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime = root / "runtime"
            inherited = os.pathsep.join([str(root / "system-bin"), str(root / "inherited-cache")])
            additions = [str(root / "worker-wrappers"), str(root / "cuda-bin")]
            science_env = {"PATH": os.pathsep.join([*additions, inherited])}
            changed_process_path = os.pathsep.join([str(root / "late-tool"), inherited])
            with mock.patch.dict(os.environ, {"PATH": changed_process_path}, clear=False):
                installer.write_runtime_launch_environment(
                    runtime,
                    root / "science-store",
                    root / "workspace",
                    "127.0.0.1",
                    "19090",
                    [str(runtime / "bin" / "gls-cli")],
                    [str(runtime / "bin" / "gls-server")],
                    str(runtime / "python"),
                    science_env,
                    inherited,
                )
            payload = json.loads((runtime / "launch_environment.json").read_text(encoding="utf-8"))
            self.assertEqual(payload["path_prepend"], [str(runtime / "bin"), *additions])
            launcher = (runtime / "run-genoligand-webui.py").read_text(encoding="utf-8")
            self.assertNotIn(str(root / "inherited-cache"), launcher)

    def test_openff_requires_fixed_digest_xml_and_loader(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            invalid = Path(tmp) / "openff-2.2.1.offxml"
            invalid.write_bytes(b"not-an-offxml")
            result = installer.validate_openff_forcefield(invalid, sys.executable)
            self.assertEqual(result["status"], "needs_repair")
            self.assertIn("xml", result["reason"])
            self.assertRegex(installer.OPENFF_FORCEFIELD_URL, r"/[0-9a-f]{40}/")
            self.assertEqual(len(installer.OPENFF_FORCEFIELD_SHA256), 64)

            last_good = Path(tmp) / "last-good.offxml"
            last_good.write_bytes(b"last-good-forcefield")
            cases = (
                (b"not-an-offxml", None, "validation_failed"),
                (b"<SMIRNOFF version='0.3'>", None, "validation_failed"),
                (b"<SMIRNOFF version='0.3'></SMIRNOFF>", "f" * 64, "checksum_failed"),
            )
            for payload, expected, expected_status in cases:
                candidate = Path(tmp) / "candidate.offxml"
                candidate.write_bytes(payload)
                result = installer.copy_or_download(
                    str(candidate),
                    last_good,
                    expected or hashlib.sha256(payload).hexdigest(),
                    validator=lambda path: installer.validate_openff_forcefield(path, sys.executable),
                )
                self.assertEqual(result["status"], expected_status)
                self.assertEqual(last_good.read_bytes(), b"last-good-forcefield")
                self.assertFalse(Path(f"{last_good}.part").exists())

    def test_openff_wsl_loader_receives_linux_forcefield_path(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            forcefield = Path(tmp) / "openff-2.2.1.offxml"
            forcefield.write_text(
                "<SMIRNOFF version='0.3'><A/><B/><C/><D/><E/></SMIRNOFF>",
                encoding="utf-8",
            )
            wrapper = Path(tmp) / "genoligand-wsl-python.cmd"
            completed = subprocess.CompletedProcess([], 0, "openff_forcefield_loaded\n", "")
            with (
                mock.patch.object(installer, "sha256_file", return_value=installer.OPENFF_FORCEFIELD_SHA256),
                mock.patch.object(installer, "run_command", return_value=completed) as run_command,
            ):
                result = installer.validate_openff_forcefield(forcefield, str(wrapper))

            self.assertEqual(result["status"], "ready")
            self.assertEqual(run_command.call_args.args[0][-1], installer.wsl_path(forcefield))

    def test_worker_environment_is_pinned_complete_and_version_gated(self) -> None:
        installer = self.installer
        components = {item["component"] for item in installer.PYTHON_TOOL_PACKAGES}
        self.assertTrue({"numpy", "pandas", "pyarrow", "jsonschema"}.issubset(components))
        self.assertTrue(all("==" in item["package"] or "@" in item["package"] for item in installer.PYTHON_TOOL_PACKAGES))
        self.assertFalse(installer.python_version_supported((3, 14)))
        self.assertFalse(installer.python_version_supported((3, 11)))
        self.assertTrue(installer.python_version_supported((3, 12)))
        for platform_id in ("windows_x86_64", "manylinux_2_34_x86_64"):
            locks = installer.worker_lock_paths(platform_id, "3.12", True)
            self.assertEqual(len(locks), 3)
            for lock in locks:
                installer.validate_hashed_requirements(lock)
                text = lock.read_text(encoding="utf-8")
                self.assertNotIn("git+", text)
                self.assertRegex(text, r"--hash=sha256:[0-9a-f]{64}")
            self.assertIn("pyarrow==24.0.0", locks[1].read_text(encoding="utf-8"))
        sources = installer.worker_source_locks()
        self.assertEqual(len(sources), 6)
        self.assertTrue(all(len(row["commit"]) == 40 and len(row["archive_sha256"]) == 64 for row in sources))
        self.assertFalse(any("HEAD" in row["commit"] or "latest" in row["git_url"] for row in sources))
        with tempfile.TemporaryDirectory() as tmp:
            incomplete = Path(tmp) / "incomplete.lock.txt"
            incomplete.write_text("numpy==2.2.6\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "requirements_lock_missing_hash"):
                installer.validate_hashed_requirements(incomplete)

    def test_manifest_writes_are_atomic_and_fingerprint_cache_persists(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = root / "manifest.tsv"
            manifest.write_text("old\n", encoding="utf-8")
            with self.assertRaises(RuntimeError):
                with installer.atomic_text_writer(manifest) as handle:
                    handle.write("new\n")
                    raise RuntimeError("injected")
            self.assertEqual(manifest.read_text(encoding="utf-8"), "old\n")

            source = root / "source.bin"
            source.write_bytes(b"source")
            cache = {}
            first = installer.resource_payload_summary(source, cache=cache)
            cache_path = root / "source_fingerprint_cache.json"
            installer.save_resource_summary_cache(cache_path, cache)
            loaded = installer.load_resource_summary_cache(cache_path)
            with mock.patch.object(installer, "sha256_file_with_progress", side_effect=AssertionError("rehash")):
                self.assertEqual(installer.resource_payload_summary(source, cache=loaded), first)

            for name in (
                "download_execution_manifest.tsv",
                "external_sync_execution_manifest.tsv",
                "source_file_manifest.tsv",
            ):
                target = root / name
                target.write_text("last-good\n", encoding="utf-8")
                with self.assertRaisesRegex(RuntimeError, "injected_manifest_interrupt"):
                    with installer.atomic_text_writer(target) as handle:
                        handle.write("partial-new-state\n")
                        raise RuntimeError("injected_manifest_interrupt")
                self.assertEqual(target.read_text(encoding="utf-8"), "last-good\n")
                attempt = json.loads(Path(f"{target}.attempt.json").read_text(encoding="utf-8"))
                self.assertEqual(attempt["status"], "failed")
                self.assertIn("injected_manifest_interrupt", attempt["error"])

            large_source = root / "mgy_clusters.fa.gz"
            create_sparse_file(large_source, 79_301_981_114)
            cache = {}
            installer.seed_resource_file_fingerprint(cache, large_source, "a" * 64)
            cache_path = root / "large_source_fingerprint_cache.json"
            installer.save_resource_summary_cache(cache_path, cache)
            loaded = installer.load_resource_summary_cache(cache_path)
            with mock.patch.object(installer, "sha256_file_with_progress", return_value="b" * 64) as hasher:
                installer.resource_payload_summary(large_source, cache=loaded)
                self.assertEqual(hasher.call_count, 0, "unchanged 79 GB source must not be rehashed")
                with large_source.open("r+b") as handle:
                    handle.write(b"x")
                installer.resource_payload_summary(large_source, cache=loaded)
                installer.resource_payload_summary(large_source, cache=loaded)
                self.assertLessEqual(hasher.call_count, 1, "changed 79 GB source must be hashed at most once")

    def test_first_download_domain_validation_and_auditable_trust(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            html = root / "payload.zip"
            html.write_text("<html>upstream error</html>", encoding="utf-8")
            row = bindingdb_catalog_row()
            invalid = installer.validate_catalog_payload(row, html)
            self.assertEqual(invalid["status"], "failed")
            self.assertEqual(invalid["reason"], "html_error_payload")
            wrong_compression = root / "wrong-compression.zip"
            wrong_compression.write_bytes(b"plain tabular payload")
            self.assertEqual(
                installer.validate_catalog_payload(row, wrong_compression)["reason"],
                "compression_mismatch:zip",
            )
            archive = root / "valid.zip"
            with zipfile.ZipFile(archive, "w") as handle:
                handle.writestr(
                    "BindingDB_All.tsv",
                    "Ligand SMILES\tTarget Name\tKi (nM)\nCCO\tTarget A\t10\nCCC\tTarget B\t20\n",
                )
            valid = installer.validate_catalog_payload(row, archive)
            self.assertEqual(valid["status"], "ready")
            self.assertEqual(valid["schema_version"], installer.SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION)
            self.assertEqual(valid["records"], 2)

            traversal = root / "traversal.zip"
            with zipfile.ZipFile(traversal, "w") as handle:
                handle.writestr(
                    "../BindingDB_All.tsv",
                    "Ligand SMILES\tTarget Name\tKi (nM)\nCCO\tTarget A\t10\nCCC\tTarget B\t20\n",
                )
            self.assertEqual(installer.validate_catalog_payload(row, traversal)["reason"], "zip_member_policy_failed")

            wrong_schema = root / "wrong-schema.zip"
            with zipfile.ZipFile(wrong_schema, "w") as handle:
                handle.writestr("BindingDB_All.tsv", "id\tvalue\n1\tx\n2\ty\n")
            self.assertEqual(installer.validate_catalog_payload(row, wrong_schema)["reason"], "bindingdb_schema_mismatch")

            wrong_release = bindingdb_catalog_row(
                url="https://www.bindingdb.org/rwd/bind/downloads/BindingDB_All_202605_tsv.zip"
            )
            self.assertEqual(
                installer.validate_catalog_payload(wrong_release, archive)["reason"],
                "release_identity_mismatch:bindingdb",
            )

            fasta = root / "uniprot.fasta.gz"
            with gzip.open(fasta, "wt", encoding="utf-8") as handle:
                handle.write(">sp|P00001|PROT_HUMAN Protein OS=Homo sapiens OX=9606 GN=GENE\nMPEPTIDE\n")
            uniprot = {
                "source_id": "uniprot_human_reviewed",
                "url": "https://rest.uniprot.org/uniprotkb/stream?compressed=true&format=fasta&query=(proteome:UP000005640) AND (reviewed:true)",
                "local_path": "downloads/uniprot/uniprot_human_reviewed.fasta.gz",
                "sha256": "",
                "release_or_date": "UniProt reviewed human proteome",
                "release_identity": "UP000005640_reviewed_stream",
                "trust_source": "publisher_https_etag_domain_validation",
                "media_type": "fasta",
                "compression": "gz",
                "min_records": "1",
            }
            self.assertEqual(installer.validate_catalog_payload(uniprot, fasta)["status"], "ready")
            bad_fasta = root / "bad.fasta.gz"
            with gzip.open(bad_fasta, "wt", encoding="utf-8") as handle:
                handle.write(">tr|P00001|PROT_MOUSE Protein OS=Mus musculus OX=10090\nMPEPTIDE\n")
            self.assertEqual(
                installer.validate_catalog_payload(uniprot, bad_fasta)["reason"],
                "uniprot_reviewed_human_header_mismatch",
            )

            binding_moad = next(
                row for row in installer.read_tsv(ROOT / "configs/source_catalog.tsv")
                if row["source_id"] == "binding_moad"
            )
            binding_moad_payload = root / "bindingmoad.csv"
            binding_moad_payload.write_text(
                "PDBfile,BindingMOAD ID,Name,Ligand ID,Measurement,Symbol,Magnitude,Units\n"
                "1ABC,1,Ligand A,LIG,Ki,=,10,nM\n"
                "2DEF,2,Ligand B,LIG,Kd,=,20,nM\n",
                encoding="utf-8",
            )
            self.assertEqual(
                installer.validate_catalog_payload(binding_moad, binding_moad_payload)["status"],
                "ready",
            )

            sqlite_source = root / "chembl_37.db"
            with closing(sqlite3.connect(sqlite_source)) as connection:
                connection.execute("CREATE TABLE molecule_dictionary (molregno INTEGER PRIMARY KEY)")
            chembl_archive = root / "chembl_37_sqlite.tar.gz"
            with tarfile.open(chembl_archive, "w:gz") as handle:
                handle.add(sqlite_source, arcname="chembl_37/chembl_37.db")
            chembl = {
                "source_id": "chembl",
                "url": "https://ftp.ebi.ac.uk/pub/databases/chembl/ChEMBLdb/releases/chembl_37/chembl_37_sqlite.tar.gz",
                "local_path": "downloads/chembl/chembl_sqlite.tar.gz",
                "sha256": installer.sha256_file(chembl_archive),
                "release_or_date": "CHEMBL37",
                "release_identity": "chembl_37_sqlite",
                "trust_source": "publisher_sha256",
                "media_type": "sqlite_tar",
                "compression": "tar.gz",
                "min_records": "1",
            }
            published = root / "published-chembl.tar.gz"
            result = installer.copy_or_download(
                str(chembl_archive),
                published,
                chembl["sha256"],
                validator=lambda path: installer.validate_catalog_payload(chembl, path),
            )
            self.assertEqual(result["status"], "ready")
            self.assertEqual(result["validation"]["release_identity"], "chembl_37_sqlite")
        rows = installer.read_tsv(ROOT / "configs/source_catalog.tsv")
        self.assertTrue(rows and all(row.get("trust_source") and row.get("release_identity") for row in rows))

    def test_all_remote_sources_bind_first_download_and_release_identity(self) -> None:
        installer = self.installer
        catalog = installer.read_tsv(ROOT / "configs/source_catalog.tsv")
        external = installer.read_tsv(ROOT / "configs/source_sync_plan.tsv")
        bindingdb = next(row for row in catalog if row["source_id"] == "bindingdb")
        self.assertEqual(
            bindingdb["sha256"],
            "f89c851bae81657c5509ccc13586676861b6ef9ae2e8424d57c393faaf9a9ed9",
        )
        self.assertEqual(
            bindingdb["trust_source"],
            "publisher_md5_verified_sha256_immutable_release_domain_validation",
        )
        expected = {
            "chembl",
            "bindingdb",
            "uniprot_human_reviewed",
            "binding_moad",
            "rcsb_ligand_bound",
            "biolip2",
            "plinder",
            "metagenome_potential",
        }
        self.assertEqual({row["source_id"] for row in [*catalog, *external]}, expected)
        self.assertEqual(
            installer.INDEXED_SOURCE_RETENTION_POLICIES["plinder"]["derived"],
            (
                "indexes/structure_targets.tsv",
                installer.STRUCTURE_TARGETS_MANIFEST_REL_PATH,
                "index_build_manifest.tsv",
            ),
        )
        self.assertEqual(
            installer.INDEXED_SOURCE_RETENTION_POLICIES["uniprot_human_reviewed"]["derived"],
            ("indexes/mmseqs/mmseqs_target_databases.tsv",),
        )
        self.assertEqual(
            installer.INDEXED_SOURCE_RETENTION_POLICIES["biolip2"]["derived"],
            (
                "indexes/structure_targets.tsv",
                installer.STRUCTURE_TARGETS_MANIFEST_REL_PATH,
                "index_build_manifest.tsv",
                "indexes/mmseqs/mmseqs_target_databases.tsv",
            ),
        )
        self.assertTrue(all(not installer.catalog_release_error(row) for row in catalog))
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            manifest = store / "download_execution_manifest.tsv"
            manifest.write_text(
                "source_id\turl\tlocal_path\tstatus\tobserved_bytes\tobserved_sha256\t"
                "source_version\trelease_identity\ttrust_source\n"
                + "".join(
                    f"{row['source_id']}\t{row['url']}\t{row['local_path']}\tready\t1\t{'a' * 64}\t"
                    f"prior-release\t{row['release_identity']}\t{row['trust_source']}\n"
                    for row in catalog
                ),
                encoding="utf-8",
            )
            reuse_arguments: list[tuple[object, object]] = []

            def completed_download(
                _url: str,
                _destination: Path,
                _expected: str | None,
                _progress=None,
                **kwargs: object,
            ) -> dict[str, object]:
                reuse_arguments.append((kwargs.get("observed_sha256"), kwargs.get("observed_bytes")))
                return {
                    "status": "ready",
                    "bytes": 1,
                    "sha256": "b" * 64,
                    "reused": False,
                    "validation": {
                        "schema_version": installer.SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION,
                        "records": 1,
                        "reason": "fixture_valid",
                    },
                }

            with mock.patch.object(installer, "copy_or_download", side_effect=completed_download):
                result = installer.prepare_downloads(ROOT / "configs/source_catalog.tsv", store)
            self.assertEqual(result["status"], "ready")
            self.assertEqual(reuse_arguments, [(None, None)] * len(catalog))
        for row in external:
            self.assertTrue(row.get("release_or_date"))
            self.assertTrue(row.get("trust_source"))
            original = installer.external_plan_digest(row)
            self.assertNotEqual(
                original,
                installer.external_plan_digest({**row, "release_or_date": f"{row['release_or_date']}-changed"}),
            )
            self.assertNotEqual(
                original,
                installer.external_plan_digest({**row, "trust_source": f"{row['trust_source']}_changed"}),
            )

        rows = {row["source_id"]: row for row in external}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            biolip = root / "BioLiP.txt.gz"
            with gzip.open(biolip, "wt", encoding="utf-8") as handle:
                handle.write("1ABC\tA\t1\tGLY\tLIG\tA\t100\tBS01\n")
            self.assertEqual(
                installer.validate_external_source_payload(rows["biolip2"], biolip)["status"],
                "ready",
            )

            plinder = root / "annotation_table.parquet"
            plinder.write_bytes(b"PAR1fixturePAR1")
            plinder_identity = {
                "relative_path": plinder.name,
                "size": plinder.stat().st_size,
                "digest_algorithm": "md5",
                "digest": hashlib.md5(plinder.read_bytes()).hexdigest(),  # noqa: S324 - publisher identity
            }
            self.assertEqual(
                installer.validate_external_source_payload(rows["plinder"], plinder, plinder_identity)["status"],
                "ready",
            )

            mgnify = root / "mgy_clusters.fa.gz"
            with gzip.open(mgnify, "wt", encoding="utf-8") as handle:
                handle.write(">MGYP000001 cluster representative\nMPEPTIDE\n")
            mgnify_identity = {
                "relative_path": mgnify.name,
                "size": mgnify.stat().st_size,
                "digest_algorithm": "md5",
                "digest": hashlib.md5(mgnify.read_bytes()).hexdigest(),  # noqa: S324 - publisher identity
            }
            destination = root / "published" / mgnify.name
            first = installer.copy_or_download(
                str(mgnify),
                destination,
                None,
                validator=lambda path: installer.validate_external_source_payload(
                    rows["metagenome_potential"], path, mgnify_identity
                ),
            )
            self.assertEqual(first["status"], "ready")
            last_good = destination.read_bytes()
            changed = root / "changed-mgy_clusters.fa.gz"
            with gzip.open(changed, "wt", encoding="utf-8") as handle:
                handle.write(">MGYP000002 changed representative\nMSEQUENCE\n")
            rejected = installer.copy_or_download(
                str(changed),
                destination,
                None,
                validator=lambda path: installer.validate_external_source_payload(
                    rows["metagenome_potential"],
                    path,
                    {**mgnify_identity, "size": changed.stat().st_size},
                ),
            )
            self.assertEqual(rejected["status"], "validation_failed")
            self.assertEqual(destination.read_bytes(), last_good)

            coordinate = root / "1abc.cif.gz"
            with gzip.open(coordinate, "wt", encoding="utf-8") as handle:
                handle.write(rcsb_mmcif_text("1ABC"))
            self.assertEqual(installer.rcsb_coordinate_validation(coordinate, "1ABC")["status"], "ready")

            no_atom_site = root / "no-atom-site.cif.gz"
            with gzip.open(no_atom_site, "wt", encoding="utf-8") as handle:
                handle.write("data_1abc\n_entry.id 1ABC\n#\n")
            self.assertEqual(
                installer.rcsb_coordinate_validation(no_atom_site, "1ABC")["reason"],
                "atom_site_coordinates_missing",
            )

            partial = root / "1abc.cif.gz.part"
            with gzip.open(partial, "wt", encoding="utf-8") as handle:
                handle.write(rcsb_mmcif_text("1ABC"))
            self.assertEqual(installer.rcsb_coordinate_validation(partial, "1ABC")["status"], "ready")

            empty_partial = root / "empty.cif.gz.part"
            empty_partial.touch()
            self.assertEqual(
                installer.rcsb_coordinate_validation(empty_partial, "1ABC")["reason"],
                "coordinate_missing_or_partial",
            )

            bad_partial = root / "bad.cif.gz.part"
            bad_partial.write_bytes(b"not-gzip")
            self.assertTrue(
                installer.rcsb_coordinate_validation(bad_partial, "1ABC")["reason"].startswith(
                    "mmcif_parse_failed:"
                )
            )

            mismatch_partial = root / "mismatch.cif.gz.part"
            with gzip.open(mismatch_partial, "wt", encoding="utf-8") as handle:
                handle.write(rcsb_mmcif_text("2ABC"))
            self.assertEqual(
                installer.rcsb_coordinate_validation(mismatch_partial, "1ABC")["reason"],
                "mmcif_id_mismatch:2ABC",
            )

    def test_hashed_install_command_and_manifest_trust_binding(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lock = root / "good.lock.txt"
            lock.write_text(f"example==1.0 --hash=sha256:{'a' * 64}\n", encoding="utf-8")
            completed = subprocess.CompletedProcess(["python"], 0, "", "")
            with mock.patch.object(installer, "run_command", return_value=completed) as runner:
                result = installer.install_hashed_requirements_local(Path("python"), [lock])
            self.assertEqual(result["status"], "ready")
            command = runner.call_args.args[0]
            self.assertIn("--require-hashes", command)
            self.assertIn(str(lock), command)

            store = root / "science-store"
            destination = store / "downloads/bindingdb/bindingdb_all.tsv.zip"
            destination.parent.mkdir(parents=True)
            with zipfile.ZipFile(destination, "w") as handle:
                handle.writestr(
                    "BindingDB_All.tsv",
                    "Ligand SMILES\tTarget Name\tKi (nM)\nCCO\tTarget A\t10\nCCC\tTarget B\t20\n",
                )
            catalog = root / "source_catalog.tsv"
            row = bindingdb_catalog_row()
            fields = list(row)
            catalog.write_text(
                "\t".join(fields) + "\n" + "\t".join(row[field] for field in fields) + "\n",
                encoding="utf-8",
            )
            sync_plan = root / "source_sync_plan.tsv"
            sync_plan.write_text("source_id\n", encoding="utf-8")
            (store / "download_execution_manifest.tsv").write_text(
                "source_id\tstatus\tsha256\trelease_identity\ttrust_source\tvalidation_schema\tvalidated_records\tvalidation_reason\n"
                f"bindingdb\tready\t{installer.sha256_file(destination)}\t{row['release_identity']}\t{row['trust_source']}\t"
                f"{installer.SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION}\t2\tdomain_schema_and_records_valid\n",
                encoding="utf-8",
            )
            manifests = installer.write_science_resource_manifests(catalog, sync_plan, store)
            self.assertEqual(manifests["status"], "ready")
            source = installer.read_tsv(store / "manifests/source_file_manifest.tsv")[0]
            self.assertEqual(source["status"], "ready")
            self.assertEqual(source["release_identity"], row["release_identity"])
            self.assertEqual(source["trust_source"], row["trust_source"])

            (store / "download_execution_manifest.tsv").write_text(
                "source_id\tstatus\tsha256\trelease_identity\ttrust_source\tvalidation_schema\tvalidated_records\tvalidation_reason\n"
                f"bindingdb\tready\t{installer.sha256_file(destination)}\tBindingDB_All_202605_tsv\t{row['trust_source']}\t"
                f"{installer.SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION}\t2\tdomain_schema_and_records_valid\n",
                encoding="utf-8",
            )
            manifests = installer.write_science_resource_manifests(catalog, sync_plan, store)
            self.assertEqual(manifests["status"], "needs_repair")
            self.assertEqual(installer.read_tsv(store / "manifests/source_file_manifest.tsv")[0]["status"], "untrusted")

    def test_chembl_archive_is_published_as_indexed_query_database(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store, chembl_dir, source_db, _archive, catalog = create_chembl_fixture(root)
            scratch = root / "scratch"
            source_before = installer.sha256_file(source_db)
            with mock.patch.object(
                installer.shutil,
                "copy2",
                side_effect=AssertionError("the source database must not be copied into the target database"),
            ):
                result = installer.publish_chembl_query_database(
                    store,
                    catalog,
                    {},
                    scratch_dir=scratch,
                )
            self.assertEqual(result["status"], "ready")
            self.assertEqual(installer.sha256_file(source_db), source_before)
            query_db = chembl_dir / "chembl_query.sqlite"
            with closing(sqlite3.connect(f"file:{query_db.as_posix()}?mode=ro", uri=True)) as connection:
                table_info = list(connection.execute("PRAGMA table_info(genoligand_onsite_activity)"))
                columns = [str(row[1]) for row in table_info]
                index_names = {
                    str(row[1])
                    for row in connection.execute("PRAGMA index_list(genoligand_onsite_activity)")
                }
                rows = set(
                    connection.execute(
                        "SELECT canonical_isomeric_smiles,target_id,source_target_id,standard_type,"
                        "standard_units,source_release,source_digest FROM genoligand_onsite_activity"
                    )
                )
                smiles_plan = " ".join(
                    str(row[-1])
                    for row in connection.execute(
                        "EXPLAIN QUERY PLAN SELECT * FROM genoligand_onsite_activity "
                        "WHERE canonical_isomeric_smiles = ?",
                        ("CCO",),
                    )
                )
            self.assertEqual(
                columns,
                [
                    "canonical_isomeric_smiles",
                    "target_id",
                    "source_target_id",
                    "target_name",
                    "organism",
                    "compound_name",
                    "standard_type",
                    "standard_relation",
                    "standard_value",
                    "standard_units",
                    "source_release",
                    "source_digest",
                ],
            )
            digest = result["source_digest"]
            self.assertEqual(
                rows,
                {
                    ("CCO", "P12345", "CHEMBL_T1", "IC50", "nM", "CHEMBL37", digest),
                    ("CCC", "P12345", "CHEMBL_T1", "Kd", "", "CHEMBL37", digest),
                    ("CCN", "CHEMBL_T2", "CHEMBL_T2", "Ki", "", "CHEMBL37", digest),
                },
            )
            defaults = {str(row[1]): row[4] for row in table_info}
            self.assertIsNotNone(defaults["source_release"])
            self.assertIsNotNone(defaults["source_digest"])
            self.assertIn("idx_genoligand_onsite_activity_smiles", smiles_plan)
            self.assertEqual(index_names, {"idx_genoligand_onsite_activity_smiles"})
            self.assertEqual(
                result["indexes"],
                ["idx_genoligand_onsite_activity_smiles"],
            )
            self.assertTrue(installer.chembl_query_database_ready(
                query_db, query_db.with_suffix(".manifest.json"), result["source_digest"], "CHEMBL37"
            ))
            published_manifest = json.loads(query_db.with_suffix(".manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(published_manifest["schema_version"], "genoligand_chembl_query_database_v1")
            self.assertEqual(published_manifest["database_bytes"], query_db.stat().st_size)
            self.assertEqual(published_manifest["database_sha256"], installer.sha256_file(query_db))
            self.assertEqual(published_manifest["rows"], 3)
            self.assertEqual(list(scratch.glob("chembl-query-rebuild-*")), [])
            self.assertEqual(list(chembl_dir.glob(".*.rebuild-*")), [])
            self.assertEqual(list(chembl_dir.glob(".*.backup-*")), [])
            query_db.chmod(0o666)
            with query_db.open("ab") as handle:
                handle.write(b"tampered")
            self.assertFalse(installer.chembl_query_database_ready(
                query_db, query_db.with_suffix(".manifest.json"), result["source_digest"], "CHEMBL37"
            ))

    def test_chembl_hard_interruptions_reconcile_every_publish_replace(self) -> None:
        installer = self.installer
        scenarios = [("upgrade", failpoint) for failpoint in range(1, 5)] + [
            ("first_install", failpoint) for failpoint in range(1, 3)
        ]
        for scenario, failpoint in scenarios:
            with self.subTest(scenario=scenario, failpoint=failpoint), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                store, chembl_dir, _source_db, _archive, catalog = create_chembl_fixture(root)
                query_db = chembl_dir / "chembl_query.sqlite"
                if scenario == "upgrade":
                    create_previous_chembl_query(query_db)
                completed = subprocess.run(
                    [
                        sys.executable,
                        str(Path(__file__).resolve()),
                        "--chembl-crash-child",
                        str(root),
                        str(failpoint),
                    ],
                    cwd=ROOT,
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                    errors="replace",
                    timeout=30,
                    check=False,
                )
                self.assertNotEqual(completed.returncode, 0, completed.stdout + completed.stderr)
                transaction = installer.chembl_query_transaction_path(query_db)
                self.assertTrue(transaction.is_file())
                recovered = installer.publish_chembl_query_database(
                    store,
                    catalog,
                    {},
                    scratch_dir=root / "scratch",
                )
                self.assertEqual(recovered["status"], "ready", recovered)
                manifest = query_db.with_suffix(".manifest.json")
                self.assertTrue(installer.chembl_query_database_ready(
                    query_db,
                    manifest,
                    recovered["source_digest"],
                    "CHEMBL37",
                ))
                self.assertFalse(transaction.exists())
                self.assertEqual(list(chembl_dir.glob(".*.rebuild-*")), [])
                self.assertEqual(list(chembl_dir.glob(".*.backup-*")), [])
                self.assertEqual(list((root / "scratch").glob("chembl-query-rebuild-*")), [])
                query_db.chmod(0o666)

    def test_chembl_ambiguous_transaction_preserves_all_recovery_evidence(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _store, chembl_dir, _source_db, _archive, _catalog = create_chembl_fixture(root)
            completed = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).resolve()),
                    "--chembl-crash-child",
                    str(root),
                    "1",
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
                check=False,
            )
            self.assertNotEqual(completed.returncode, 0, completed.stdout + completed.stderr)
            query_db = chembl_dir / "chembl_query.sqlite"
            manifest = query_db.with_suffix(".manifest.json")
            transaction = installer.chembl_query_transaction_path(query_db)
            payload = installer.read_json_object(transaction)
            paths = installer.chembl_transaction_paths(query_db, manifest, payload, root / "scratch")
            self.assertIsNotNone(paths)
            assert paths is not None
            with paths["staging_manifest"].open("ab") as handle:
                handle.write(b"tampered")
            preserved = {
                path: path.read_bytes()
                for path in paths.values()
                if path.is_file()
            }
            preserved[transaction] = transaction.read_bytes()
            recovery = installer.reconcile_chembl_query_transaction(
                query_db,
                manifest,
                root / "scratch",
            )
            self.assertEqual(recovery["status"], "needs_repair")
            self.assertIn("ambiguous", recovery["reason"])
            for path, content in preserved.items():
                self.assertTrue(path.is_file(), path)
                self.assertEqual(path.read_bytes(), content, path)

    def test_compact_ligand_index_supersedes_and_removes_legacy_query_databases(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            digests = write_ligand_source_manifests(installer, store)
            database, _manifest, _payload = write_ligand_similarity_fixture(
                installer,
                store,
                source_digests=digests,
            )
            self.assertTrue(installer.ligand_similarity_index_ready(store))

            legacy = [
                store / "downloads/chembl/chembl_query.sqlite",
                store / "downloads/chembl/chembl_query.manifest.json",
                store / "indexes/bindingdb/bindingdb_query.sqlite",
                store / "indexes/bindingdb/bindingdb_query.manifest.json",
            ]
            sidecars = [
                Path(f"{legacy[0]}-wal"),
                Path(f"{legacy[0]}-shm"),
                Path(f"{legacy[0]}-journal"),
                Path(f"{legacy[2]}-wal"),
                Path(f"{legacy[2]}-shm"),
                Path(f"{legacy[2]}-journal"),
            ]
            for sequence, path in enumerate([*legacy, *sidecars], start=1):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"x" * sequence)
            legacy[0].chmod(0o444)
            expected_reclaimed = sum(path.stat().st_size for path in [*legacy, *sidecars])

            readiness = installer.evaluate_installer_science_store_readiness(
                store,
                persist=False,
                check_install_status=False,
                check_source_retention=False,
            )
            self.assertIn(
                "superseded_ligand_query_database_present",
                {row["code"] for row in readiness["blockers"]},
            )
            cleanup = installer.prune_superseded_ligand_query_databases(store)
            self.assertEqual(cleanup["status"], "ready")
            self.assertEqual(cleanup["reclaimed_bytes"], expected_reclaimed)
            self.assertTrue(database.is_file())
            self.assertTrue(all(not path.exists() for path in [*legacy, *sidecars]))

            sidecars[0].parent.mkdir(parents=True, exist_ok=True)
            sidecars[0].write_bytes(b"orphaned-wal")
            sidecar_only = installer.evaluate_installer_science_store_readiness(
                store,
                persist=False,
                check_install_status=False,
                check_source_retention=False,
            )
            self.assertIn(
                "superseded_ligand_query_database_present",
                {row["code"] for row in sidecar_only["blockers"]},
            )
            self.assertEqual(
                installer.prune_superseded_ligand_query_databases(store)["status"],
                "ready",
            )
            self.assertFalse(sidecars[0].exists())

    def test_ligand_retention_migrates_both_sources_to_shared_compact_derivative(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            digests = write_ligand_source_manifests(installer, store)
            write_ligand_similarity_fixture(installer, store, source_digests=digests)
            installer.write_source_retention_rows(
                store,
                [
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": source_id,
                        "state": "pruned",
                        "source_manifest_sha256": digest,
                        "release_identity": "fixture-release",
                        "trust_source": "fixture_domain_validation",
                        "derived_paths": old_derived,
                        "pruned_bytes": "100",
                    }
                    for source_id, digest, old_derived in (
                        ("chembl", digests["chembl"], "downloads/chembl/chembl_query.sqlite"),
                        ("bindingdb", digests["bindingdb"], "indexes/bindingdb/bindingdb_query.sqlite"),
                    )
                ],
            )
            migration = installer.migrate_ligand_source_retention(store)
            self.assertEqual(migration["status"], "needs_repair")
            self.assertEqual(migration["updated"], 2)
            expected = ";".join(
                (
                    installer.LIGAND_SIMILARITY_DATABASE_REL_PATH,
                    installer.LIGAND_SIMILARITY_MANIFEST_REL_PATH,
                )
            )
            rows = installer.source_retention_rows(store)
            self.assertEqual({row["source_id"] for row in rows}, {"chembl", "bindingdb"})
            self.assertTrue(all(row["derived_paths"] == expected for row in rows))
            self.assertFalse(
                any(installer.retained_source_derivatives_ready(store, row) for row in rows)
            )

            installer.write_source_retention_rows(store, [rows[0]])
            incomplete = installer.migrate_ligand_source_retention(store)
            self.assertEqual(incomplete["status"], "needs_repair")

            for row in rows:
                row["state"] = "pruning"
            installer.write_source_retention_rows(store, rows)
            recoverable = installer.migrate_ligand_source_retention(store)
            self.assertEqual(recoverable["status"], "needs_repair")

    def test_pruned_ligand_retention_preserves_unrecognized_restored_archives(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            digests = write_ligand_source_manifests(installer, store)
            write_ligand_similarity_fixture(installer, store, source_digests=digests)
            payloads = {
                "chembl": "downloads/chembl/chembl_sqlite.tar.gz",
                "bindingdb": "downloads/bindingdb/bindingdb_all.tsv.zip",
            }
            retention_rows = []
            for source_id, relative in payloads.items():
                payload = store / relative
                payload.parent.mkdir(parents=True, exist_ok=True)
                payload.write_bytes(f"restored-{source_id}".encode("ascii"))
                retention_rows.append(
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": source_id,
                        "state": "pruned",
                        "source_manifest_sha256": digests[source_id],
                        "release_identity": "fixture-release",
                        "trust_source": "fixture_domain_validation",
                        "payload_records_json": json.dumps(
                            [{"path": relative, "kind": "file"}],
                            separators=(",", ":"),
                        ),
                        "derived_paths": ";".join(
                            (
                                installer.LIGAND_SIMILARITY_DATABASE_REL_PATH,
                                installer.LIGAND_SIMILARITY_MANIFEST_REL_PATH,
                            )
                        ),
                        "pruned_bytes": "100",
                    }
                )
            installer.write_source_retention_rows(store, retention_rows)
            policies = {
                source_id: {
                    "payloads": (relative,),
                    "derived": (
                        installer.LIGAND_SIMILARITY_DATABASE_REL_PATH,
                        installer.LIGAND_SIMILARITY_MANIFEST_REL_PATH,
                    ),
                }
                for source_id, relative in payloads.items()
            }
            with mock.patch.object(installer, "INDEXED_SOURCE_RETENTION_POLICIES", policies):
                result = installer.prune_indexed_source_payloads(store)
            self.assertEqual(result["status"], "needs_repair")
            self.assertEqual(result["reclaimed_bytes"], 0)
            self.assertTrue(all((store / relative).is_file() for relative in payloads.values()))

    def test_plinder_retention_keeps_projection_and_removes_canonical_intermediates(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = root / "science_store"
            source = store / "downloads/plinder/index/annotation_table.parquet"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"PAR1fixturePAR1")
            indexes = store / "indexes"
            indexes.mkdir(parents=True)
            structure = indexes / "structure_targets.tsv"
            structure.write_text(
                "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
                "sequence_tokens\tstructure_tokens\tevidence\n"
                "PLINDER\tPLINDER_SYSTEM\tPLINDER system\tunknown\tstructure_record\tfad\t"
                "chain_a\t3grt\tplinder_2024-06_v2_parquet\n",
                encoding="utf-8",
            )
            canonical = indexes / "plinder_targets.tsv"
            canonical.write_bytes(b"large canonical intermediate")
            canonical_manifest = indexes / "plinder_targets.manifest.json"
            canonical_manifest.write_text(
                json.dumps(
                    {
                        "status": "ready",
                        "source": {
                            "size_bytes": source.stat().st_size,
                            "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                        },
                        "canonical": {
                            "size_bytes": canonical.stat().st_size,
                            "sha256": hashlib.sha256(canonical.read_bytes()).hexdigest(),
                            "row_count": 2,
                        },
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            canonical_bytes = canonical.read_bytes()
            canonical_manifest_bytes = canonical_manifest.read_bytes()
            manifest_dir = store / "manifests"
            manifest_dir.mkdir(parents=True)
            source_summary = installer.resource_payload_summary(source.parent)
            (manifest_dir / "source_file_manifest.tsv").write_text(
                "source_id\tsource_label\tsource_class\tlocal_path\tstatus\tfile_count\tbytes\t"
                "sha256\tprovenance\trelease_identity\ttrust_source\n"
                "plinder\tPLINDER\tstructure_interaction\tdownloads/plinder/index\tready\t"
                f"{source_summary['file_count']}\t{source_summary['bytes']}\t{source_summary['sha256']}\t"
                "fixture\tplinder-v2\tpublisher_snapshot\n",
                encoding="utf-8",
            )
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                "structure_targets\tindexes/plinder_targets.tsv\tused\t2\t"
                "indexes/structure_targets.tsv\tplinder_canonical_rows;complete_source_scan\n",
                encoding="utf-8",
            )
            input_summary = installer.structure_contribution_input_summary(
                store,
                ["indexes/plinder_targets.tsv"],
            )
            structure_manifest = write_structure_publication_fixture(
                installer,
                store,
                [
                    {
                        "source": "PLINDER",
                        "source_id": "plinder",
                        "source_digest": source_summary["sha256"],
                        "release_identity": "plinder-v2",
                        "trust_source": "publisher_snapshot",
                        "rows": 2,
                        "source_paths": ["indexes/plinder_targets.tsv"],
                        "input_bytes": input_summary["input_bytes"],
                        "input_digest": input_summary["input_digest"],
                    }
                ],
            )
            policy = {
                "plinder": {
                    "payloads": ("downloads/plinder/index",),
                    "derived": (
                        "indexes/structure_targets.tsv",
                        installer.STRUCTURE_TARGETS_MANIFEST_REL_PATH,
                        "index_build_manifest.tsv",
                    ),
                }
            }
            with mock.patch.object(installer, "INDEXED_SOURCE_RETENTION_POLICIES", policy):
                result = installer.prune_indexed_source_payloads(store)
            self.assertEqual(result["status"], "ready")
            self.assertFalse(source.exists())
            self.assertFalse(canonical.exists())
            self.assertFalse(canonical_manifest.exists())
            self.assertTrue(structure.is_file())
            self.assertTrue(structure_manifest.is_file())
            retained = installer.source_retention_row(store, "plinder")
            self.assertEqual(
                retained["derived_paths"],
                "indexes/structure_targets.tsv;indexes/structure_targets.manifest.json;"
                "index_build_manifest.tsv",
            )
            retained["derived_paths"] = (
                "indexes/plinder_targets.tsv;indexes/plinder_targets.manifest.json;"
                "indexes/structure_targets.tsv"
            )
            retained["structure_input_bytes"] = ""
            retained["structure_input_digest"] = ""
            installer.write_source_retention_rows(store, [retained])
            canonical.write_bytes(canonical_bytes)
            canonical_manifest.write_bytes(canonical_manifest_bytes)
            with mock.patch.object(installer, "INDEXED_SOURCE_RETENTION_POLICIES", policy):
                migrated = installer.prune_indexed_source_payloads(store)
            self.assertEqual(migrated["status"], "ready")
            self.assertFalse(canonical.exists())
            self.assertFalse(canonical_manifest.exists())
            self.assertEqual(
                installer.source_retention_row(store, "plinder")["derived_paths"],
                "indexes/structure_targets.tsv;indexes/structure_targets.manifest.json;"
                "index_build_manifest.tsv",
            )
            migrated_retention = installer.source_retention_row(store, "plinder")
            self.assertEqual(
                migrated_retention["structure_input_bytes"],
                str(input_summary["input_bytes"]),
            )
            self.assertEqual(
                migrated_retention["structure_input_digest"],
                str(input_summary["input_digest"]),
            )
            reused = installer.prepare_plinder_canonical_index(root, store, "", "local", None)
            self.assertEqual(reused["status"], "ready")
            self.assertEqual(
                reused["reason"],
                "plinder_structure_projection_reused_after_source_payload_pruned",
            )
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_bytes(b"PAR1new-releasePAR1")
            canonical.write_bytes(canonical_bytes)
            canonical_manifest.write_bytes(canonical_manifest_bytes)
            with mock.patch.object(installer, "INDEXED_SOURCE_RETENTION_POLICIES", policy):
                replaced = installer.prune_indexed_source_payloads(store)
            self.assertEqual(replaced["status"], "needs_repair")
            self.assertTrue(source.is_file())
            self.assertTrue(canonical.is_file())
            self.assertTrue(canonical_manifest.is_file())

    def test_structure_publication_v1_is_rejected_until_rebuilt(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            source = store / "downloads/binding_moad/bindingmoad.csv"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"pdb_id,ligand\n1abc,ATP\n")
            indexes = store / "indexes"
            indexes.mkdir(parents=True)
            table = indexes / "structure_targets.tsv"
            table.write_text(
                "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
                "sequence_tokens\tstructure_tokens\tevidence\n"
                "Binding MOAD\t1ABC\tMOAD complex\tunknown\tstructure_record\tatp\tchain_a\t"
                "1abc\tbinding_moad\n",
                encoding="utf-8",
            )
            source_summary = installer.resource_payload_summary(source)
            source_manifest = store / "manifests/source_file_manifest.tsv"
            source_manifest.parent.mkdir(parents=True)
            installer.write_simple_tsv(
                source_manifest,
                [
                    "source_id",
                    "local_path",
                    "status",
                    "file_count",
                    "bytes",
                    "sha256",
                    "release_identity",
                    "trust_source",
                ],
                [
                    {
                        "source_id": "binding_moad",
                        "local_path": "downloads/binding_moad/bindingmoad.csv",
                        "status": "ready",
                        "file_count": str(source_summary["file_count"]),
                        "bytes": str(source_summary["bytes"]),
                        "sha256": str(source_summary["sha256"]),
                        "release_identity": "moad-fixture-v1",
                        "trust_source": "publisher_snapshot",
                    }
                ],
            )
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                "structure_targets\tdownloads/binding_moad/bindingmoad.csv\tused\t1\t"
                "indexes/structure_targets.tsv\tbinding_moad_complex_rows;complete_source_scan\n",
                encoding="utf-8",
            )
            manifest = write_structure_publication_fixture(
                installer,
                store,
                [
                    {
                        "source": "Binding MOAD",
                        "rows": 1,
                        "source_paths": ["downloads/binding_moad/bindingmoad.csv"],
                    }
                ],
            )
            legacy = json.loads(manifest.read_text(encoding="utf-8"))
            legacy["schema_version"] = installer.LEGACY_STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION
            manifest.write_text(json.dumps(legacy) + "\n", encoding="utf-8")

            self.assertFalse(installer.structure_targets_publication_ready(store, "Binding MOAD"))
            unchanged = json.loads(manifest.read_text(encoding="utf-8"))
            self.assertEqual(
                unchanged["schema_version"],
                installer.LEGACY_STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION,
            )
            self.assertTrue(source.is_file())

    def test_required_live_structure_input_is_scoped_to_requested_source(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            moad = store / "downloads/binding_moad/bindingmoad.csv"
            moad.parent.mkdir(parents=True)
            moad.write_bytes(b"pdb_id,ligand\n1abc,ATP\n")
            indexes = store / "indexes"
            indexes.mkdir(parents=True)
            table = indexes / "structure_targets.tsv"
            table.write_text(
                "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
                "sequence_tokens\tstructure_tokens\tevidence\n"
                "Binding MOAD\t1ABC\tMOAD complex\tunknown\tstructure_record\tatp\tchain_a\t"
                "1abc\tbinding_moad\n"
                "BioLiP2\t2DEF\tBioLiP target\tunknown\tstructure_record\tadp\tchain_b\t"
                "2def\tbiolip2\n",
                encoding="utf-8",
            )
            moad_summary = installer.resource_payload_summary(moad)
            moad_input = installer.structure_contribution_input_summary(
                store,
                ["downloads/binding_moad/bindingmoad.csv"],
            )
            bio_digest = "b" * 64
            installer.write_simple_tsv(
                store / "manifests/source_file_manifest.tsv",
                [
                    "source_id",
                    "local_path",
                    "status",
                    "storage_state",
                    "file_count",
                    "bytes",
                    "sha256",
                    "release_identity",
                    "trust_source",
                ],
                [
                    {
                        "source_id": "binding_moad",
                        "local_path": "downloads/binding_moad/bindingmoad.csv",
                        "status": "ready",
                        "storage_state": "materialized",
                        "file_count": str(moad_summary["file_count"]),
                        "bytes": str(moad_summary["bytes"]),
                        "sha256": str(moad_summary["sha256"]),
                        "release_identity": "moad-v1",
                        "trust_source": "moad-trust",
                    },
                    {
                        "source_id": "biolip2",
                        "local_path": "downloads/biolip2",
                        "status": "ready",
                        "storage_state": "indexed_source_pruned",
                        "file_count": "1",
                        "bytes": "10",
                        "sha256": bio_digest,
                        "release_identity": "bio-v1",
                        "trust_source": "bio-trust",
                    },
                ],
            )
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                "structure_targets\tdownloads/binding_moad/bindingmoad.csv\tused\t1\t"
                "indexes/structure_targets.tsv\tbinding_moad_complex_rows;complete_source_scan\n"
                "structure_targets\tdownloads/biolip2/BioLiP.txt.gz\tused\t1\t"
                "indexes/structure_targets.tsv\tbiolip2_interaction_rows;complete_source_scan\n",
                encoding="utf-8",
            )
            bio_input_digest = "c" * 64
            write_structure_publication_fixture(
                installer,
                store,
                [
                    {
                        "source": "Binding MOAD",
                        "source_id": "binding_moad",
                        "source_digest": moad_summary["sha256"],
                        "release_identity": "moad-v1",
                        "trust_source": "moad-trust",
                        "rows": 1,
                        "source_paths": ["downloads/binding_moad/bindingmoad.csv"],
                        "input_bytes": moad_input["input_bytes"],
                        "input_digest": moad_input["input_digest"],
                    },
                    {
                        "source": "BioLiP2",
                        "source_id": "biolip2",
                        "source_digest": bio_digest,
                        "release_identity": "bio-v1",
                        "trust_source": "bio-trust",
                        "rows": 1,
                        "source_paths": ["downloads/biolip2/BioLiP.txt.gz"],
                        "input_bytes": 10,
                        "input_digest": bio_input_digest,
                    },
                ],
            )
            installer.write_source_retention_rows(
                store,
                [
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": "biolip2",
                        "state": "pruned",
                        "source_manifest_sha256": bio_digest,
                        "release_identity": "bio-v1",
                        "trust_source": "bio-trust",
                        "structure_input_bytes": "10",
                        "structure_input_digest": bio_input_digest,
                    }
                ],
            )
            self.assertTrue(
                installer.structure_targets_publication_ready(
                    store,
                    "Binding MOAD",
                    require_live_inputs=True,
                )
            )
            self.assertFalse(
                installer.structure_targets_publication_ready(
                    store,
                    require_live_inputs=True,
                )
            )

    def test_replaced_structure_payload_is_preserved_by_cleanup(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            source = store / "downloads/binding_moad/bindingmoad.csv"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"pdb_id,ligand\n1abc,ATP\n")
            indexes = store / "indexes"
            indexes.mkdir(parents=True)
            table = indexes / "structure_targets.tsv"
            table.write_text(
                "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
                "sequence_tokens\tstructure_tokens\tevidence\n"
                "Binding MOAD\t1ABC\tMOAD complex\tunknown\tstructure_record\tatp\tchain_a\t"
                "1abc\tbinding_moad\n",
                encoding="utf-8",
            )
            source_summary = installer.resource_payload_summary(source)
            source_manifest = store / "manifests/source_file_manifest.tsv"
            source_manifest.parent.mkdir(parents=True)
            installer.write_simple_tsv(
                source_manifest,
                [
                    "source_id",
                    "local_path",
                    "status",
                    "file_count",
                    "bytes",
                    "sha256",
                    "release_identity",
                    "trust_source",
                ],
                [
                    {
                        "source_id": "binding_moad",
                        "local_path": "downloads/binding_moad/bindingmoad.csv",
                        "status": "ready",
                        "file_count": str(source_summary["file_count"]),
                        "bytes": str(source_summary["bytes"]),
                        "sha256": str(source_summary["sha256"]),
                        "release_identity": "moad-fixture-v1",
                        "trust_source": "publisher_snapshot",
                    }
                ],
            )
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                "structure_targets\tdownloads/binding_moad/bindingmoad.csv\tused\t1\t"
                "indexes/structure_targets.tsv\tbinding_moad_complex_rows;complete_source_scan\n",
                encoding="utf-8",
            )
            input_summary = installer.structure_contribution_input_summary(
                store,
                ["downloads/binding_moad/bindingmoad.csv"],
            )
            write_structure_publication_fixture(
                installer,
                store,
                [
                    {
                        "source": "Binding MOAD",
                        "source_id": "binding_moad",
                        "source_digest": source_summary["sha256"],
                        "release_identity": "moad-fixture-v1",
                        "trust_source": "publisher_snapshot",
                        "rows": 1,
                        "source_paths": ["downloads/binding_moad/bindingmoad.csv"],
                        "input_bytes": input_summary["input_bytes"],
                        "input_digest": input_summary["input_digest"],
                    }
                ],
            )
            self.assertTrue(installer.structure_targets_publication_ready(store, "Binding MOAD"))
            source_manifest_text = source_manifest.read_text(encoding="utf-8")
            source_manifest.write_text(
                source_manifest_text.replace("moad-fixture-v1", "moad-fixture-v2"),
                encoding="utf-8",
            )
            self.assertFalse(installer.structure_targets_publication_ready(store, "Binding MOAD"))
            source_manifest.write_text(source_manifest_text, encoding="utf-8")
            build_manifest = store / "index_build_manifest.tsv"
            build_manifest_text = build_manifest.read_text(encoding="utf-8")
            build_manifest.write_text(
                build_manifest_text.replace("\tused\t1\t", "\tused\t2\t"),
                encoding="utf-8",
            )
            self.assertFalse(installer.structure_targets_publication_ready(store, "Binding MOAD"))
            build_manifest.write_text(build_manifest_text, encoding="utf-8")
            policy = {
                "binding_moad": {
                    "payloads": ("downloads/binding_moad/bindingmoad.csv",),
                    "derived": (
                        "indexes/structure_targets.tsv",
                        installer.STRUCTURE_TARGETS_MANIFEST_REL_PATH,
                        "index_build_manifest.tsv",
                    ),
                }
            }
            with mock.patch.object(installer, "INDEXED_SOURCE_RETENTION_POLICIES", policy):
                first = installer.prune_indexed_source_payloads(store)
            self.assertEqual(first["status"], "ready")
            self.assertFalse(source.exists())

            source.write_bytes(b"pdb_id,ligand\n9xyz,NEW\n")
            with mock.patch.object(installer, "INDEXED_SOURCE_RETENTION_POLICIES", policy):
                second = installer.prune_indexed_source_payloads(store)
            self.assertEqual(second["status"], "needs_repair")
            self.assertTrue(source.is_file())
            self.assertEqual(source.read_bytes(), b"pdb_id,ligand\n9xyz,NEW\n")

    def test_biolip_structure_only_payload_rejects_unpublished_extra_files(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            annotation = store / "downloads/biolip2/BioLiP.txt.gz"
            annotation.parent.mkdir(parents=True)
            annotation.write_bytes(b"biolip-annotation")
            input_summary = installer.structure_contribution_input_summary(
                store,
                ["downloads/biolip2/BioLiP.txt.gz"],
            )
            manifest = store / installer.STRUCTURE_TARGETS_MANIFEST_REL_PATH
            manifest.parent.mkdir(parents=True)
            manifest.write_text(
                json.dumps(
                    {
                        "schema_version": installer.STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION,
                        "contributions": [
                            {
                                "source": "BioLiP2",
                                "source_id": "biolip2",
                                "source_digest": "a" * 64,
                                "release_identity": "biolip-fixture",
                                "trust_source": "publisher_inventory",
                                "rows": 1,
                                "source_paths": ["downloads/biolip2/BioLiP.txt.gz"],
                                "input_bytes": input_summary["input_bytes"],
                                "input_digest": input_summary["input_digest"],
                            }
                        ],
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            source_manifest = {
                "local_path": "downloads/biolip2",
                "status": "ready",
                "file_count": "6",
                "bytes": "999999",
                "sha256": "a" * 64,
                "release_identity": "biolip-fixture",
                "trust_source": "publisher_inventory",
            }

            summary = installer.resource_payload_summary(annotation.parent)
            record = {
                "path": "downloads/biolip2",
                "kind": "directory",
                "file_count": summary["file_count"],
                "bytes": summary["bytes"],
                "sha256": summary["sha256"],
            }
            self.assertTrue(
                installer.structure_source_payload_matches_manifest(
                    store,
                    "biolip2",
                    source_manifest,
                    [record],
                )
            )
            (annotation.parent / "protein.fasta.gz").write_bytes(b">P1\nMSEQ\n")
            changed = installer.resource_payload_summary(annotation.parent)
            record.update(
                {
                    "file_count": changed["file_count"],
                    "bytes": changed["bytes"],
                    "sha256": changed["sha256"],
                }
            )
            self.assertFalse(
                installer.structure_source_payload_matches_manifest(
                    store,
                    "biolip2",
                    source_manifest,
                    [record],
                )
            )

    def test_ligand_index_rejects_hash_manifest_and_internal_contract_corruption(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            digests = write_ligand_source_manifests(installer, store)
            database, manifest, payload = write_ligand_similarity_fixture(
                installer,
                store,
                source_digests=digests,
            )
            input_bytes = sum(int(row["bytes"]) for row in payload["inputs"].values())
            self.assertLess(database.stat().st_size, input_bytes)
            self.assertEqual(
                payload["reclaimed_candidate_bytes"],
                input_bytes - database.stat().st_size,
            )
            self.assertTrue(installer.ligand_similarity_index_ready(store))

            with database.open("ab") as handle:
                handle.write(b"tampered")
            self.assertFalse(installer.ligand_similarity_index_ready(store))

            database, manifest, payload = write_ligand_similarity_fixture(
                installer,
                store,
                source_digests=digests,
            )
            with closing(sqlite3.connect(database)) as connection:
                connection.execute("UPDATE metadata SET value = 'corrupt' WHERE key = 'status'")
                connection.commit()
            payload["database"]["sha256"] = installer.sha256_file(database)
            payload["database"]["bytes"] = database.stat().st_size
            installer.write_json_atomic(manifest, payload)
            self.assertFalse(installer.ligand_similarity_index_ready(store))

            _database, manifest, payload = write_ligand_similarity_fixture(
                installer,
                store,
                source_digests=digests,
            )
            payload["database"]["path"] = str(store / "wrong.sqlite")
            installer.write_json_atomic(manifest, payload)
            self.assertFalse(installer.ligand_similarity_index_ready(store))

            _database, manifest, payload = write_ligand_similarity_fixture(
                installer,
                store,
                source_digests=digests,
            )
            payload["fingerprint"]["encoding"] = "unsupported_encoding"
            installer.write_json_atomic(manifest, payload)
            self.assertFalse(installer.ligand_similarity_index_ready(store))

    def test_ligand_capacity_does_not_spend_space_reclaimed_after_downloads(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = root / "science_store"
            digests = write_ligand_source_manifests(installer, store)
            write_ligand_similarity_fixture(installer, store, source_digests=digests)
            legacy = [
                store / "downloads/chembl/chembl_query.sqlite",
                store / "indexes/bindingdb/bindingdb_query.sqlite",
            ]
            for path in legacy:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"x" * 1000)
            catalog = root / "catalog.tsv"
            catalog.write_text(
                "source_id\turl\tlocal_path\tsha256\trelease_identity\ttrust_source\n"
                "future\thttps://fixture.invalid/future.dat\tdownloads/future.dat\t\tfixture-release\tfixture_https\n",
                encoding="utf-8",
            )
            required = installer.DISK_SAFETY_MARGIN_BYTES + 1500
            with mock.patch.object(
                installer,
                "remote_resource_identity",
                return_value={"content_length": 1500},
            ), mock.patch.object(installer, "same_storage_volume", return_value=True):
                plan = installer.installation_capacity_plan(
                    store,
                    catalog,
                    root / "missing-sync.tsv",
                    available_bytes=required - 1,
                    scratch_dir=root / "scratch",
                    resolve_remote=True,
                )
            self.assertEqual(plan["status"], "blocked")
            self.assertEqual(plan["capacity_failures"], ["target"])
            cleanup = next(row for row in plan["items"] if row["component"] == "ligand_similarity_index")
            self.assertEqual(cleanup["action"], "remove_superseded_query_databases")
            self.assertEqual(cleanup["reclaimable_bytes"], 0)

    def test_ligand_capacity_reserves_compact_build_without_net_growth(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = root / "science_store"
            with mock.patch.object(installer, "same_storage_volume", return_value=True):
                plan = installer.installation_capacity_plan(
                    store,
                    root / "missing-catalog.tsv",
                    root / "missing-sync.tsv",
                    available_bytes=100 * 1024 * 1024 * 1024,
                    scratch_dir=root / "scratch",
                )
            item = next(row for row in plan["items"] if row["component"] == "ligand_similarity_index")
            self.assertGreaterEqual(item["staging_bytes"], 5 * 1024 * 1024 * 1024)
            self.assertFalse(item["net_growth_allowed"])
            self.assertEqual(plan["status"], "ready")

    def test_capacity_reuses_compact_index_without_budgeting_legacy_chembl_rebuild(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = root / "science_store"
            digests = write_ligand_source_manifests(installer, store)
            write_ligand_similarity_fixture(installer, store, source_digests=digests)
            archive = store / "downloads/chembl/chembl_sqlite.tar.gz"
            archive.parent.mkdir(parents=True, exist_ok=True)
            archive.write_bytes(b"published-source-archive")
            with mock.patch.object(installer, "same_storage_volume", return_value=True):
                plan = installer.installation_capacity_plan(
                    store,
                    root / "missing-catalog.tsv",
                    root / "missing-sync.tsv",
                    available_bytes=installer.DISK_SAFETY_MARGIN_BYTES + 1024,
                    scratch_dir=root / "scratch",
                )
            components = {row["component"] for row in plan["items"]}
            self.assertNotIn("chembl_query_database", components)
            self.assertNotIn("ligand_similarity_index", components)
            self.assertEqual(plan["status"], "ready")

    def test_legacy_chembl_database_validator_binds_manifest_identity(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store, chembl_dir, _source_db, _archive, catalog = create_chembl_fixture(root)
            summary_cache = {}
            published = installer.publish_chembl_query_database(
                store,
                catalog,
                summary_cache,
                scratch_dir=root / "scratch",
            )
            self.assertEqual(published["status"], "ready")
            manifest_dir = store / "manifests"
            manifest_dir.mkdir(parents=True)
            (manifest_dir / "source_file_manifest.tsv").write_text(
                "source_id\tstatus\tsha256\trelease_identity\ttrust_source\n"
                f"chembl\tready\t{published['source_digest']}\tCHEMBL37\tfixture_domain_validation\n",
                encoding="utf-8",
            )
            with mock.patch.object(
                installer,
                "sha256_file_with_progress",
                side_effect=AssertionError("unchanged ChEMBL database must use the persistent fingerprint cache"),
            ):
                self.assertTrue(
                    installer.chembl_query_database_ready(
                        chembl_dir / "chembl_query.sqlite",
                        chembl_dir / "chembl_query.manifest.json",
                        published["source_digest"],
                        "CHEMBL37",
                        summary_cache,
                    )
                )
            query_db = chembl_dir / "chembl_query.sqlite"
            query_db.chmod(0o666)
            with query_db.open("ab") as handle:
                handle.write(b"tampered")
            self.assertFalse(
                installer.chembl_query_database_ready(
                    query_db,
                    query_db.with_suffix(".manifest.json"),
                    published["source_digest"],
                    "CHEMBL37",
                    summary_cache,
                )
            )

    def test_chembl_capacity_keeps_scratch_and_target_volumes_independent(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store, _chembl_dir, source_db, archive, _catalog = create_chembl_fixture(root)
            scratch = root / "separate-scratch-volume"
            scratch_required = source_db.stat().st_size + installer.DISK_SAFETY_MARGIN_BYTES
            target_staging = installer.chembl_query_target_staging_bytes(
                source_db.stat().st_size,
                archive.stat().st_size,
            )

            def capacity(target_available: int, scratch_available: int) -> dict[str, object]:
                with mock.patch.object(installer, "same_storage_volume", return_value=False):
                    return installer.installation_capacity_plan(
                        store,
                        root / "missing-catalog.tsv",
                        root / "missing-sync.tsv",
                        available_bytes=target_available,
                        scratch_dir=scratch,
                        scratch_available_bytes=scratch_available,
                    )

            reference = capacity(100 * 1024 * 1024 * 1024, 100 * 1024 * 1024 * 1024)
            target_required = int(reference["required_peak_bytes"])
            scratch_blocked = capacity(target_required, scratch_required - 1)
            self.assertEqual(scratch_blocked["status"], "blocked")
            self.assertEqual(scratch_blocked["capacity_failures"], ["scratch"])
            target_blocked = capacity(target_required - 1, scratch_required)
            self.assertEqual(target_blocked["status"], "blocked")
            self.assertEqual(target_blocked["capacity_failures"], ["target"])
            ready = capacity(target_required, scratch_required)
            self.assertEqual(ready["status"], "ready")
            self.assertEqual(ready["required_peak_bytes"], target_required)
            self.assertEqual(ready["target_available_bytes"], target_required)
            self.assertEqual(ready["scratch_available_bytes"], scratch_required)
            self.assertFalse(ready["scratch_target_same_volume"])
            item = next(row for row in ready["items"] if row["component"] == "chembl_query_database")
            self.assertEqual(item["scratch_staging_bytes"], source_db.stat().st_size)
            self.assertEqual(item["target_staging_bytes"], target_staging)
            source = INSTALLER.read_text(encoding="utf-8")
            self.assertIn('parser.add_argument("--scratch-dir")', source)
            self.assertGreaterEqual(source.count("scratch_dir=scratch_dir"), 2)

    def test_chembl_publish_failure_restores_previous_database_and_cleans_staging(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store, chembl_dir, _source_db, _archive, catalog = create_chembl_fixture(root)
            query_db = chembl_dir / "chembl_query.sqlite"
            manifest = create_previous_chembl_query(query_db)
            old_database = query_db.read_bytes()
            old_manifest = manifest.read_text(encoding="utf-8")
            scratch = root / "scratch"
            real_replace = installer.os.replace

            def fail_database_publish(source: str | Path, target: str | Path) -> None:
                source_path = Path(source)
                target_path = Path(target)
                if target_path == query_db and source_path.name.startswith(f".{query_db.name}.rebuild-"):
                    raise OSError("fixture publish failure")
                real_replace(source, target)

            with mock.patch.object(installer.os, "replace", side_effect=fail_database_publish):
                result = installer.publish_chembl_query_database(
                    store,
                    catalog,
                    {},
                    scratch_dir=scratch,
                )
            self.assertEqual(result["status"], "needs_repair")
            self.assertEqual(query_db.read_bytes(), old_database)
            self.assertEqual(manifest.read_text(encoding="utf-8"), old_manifest)
            self.assertEqual(list(scratch.glob("chembl-query-rebuild-*")), [])
            self.assertEqual(list(chembl_dir.glob(".*.rebuild-*")), [])
            self.assertEqual(list(chembl_dir.glob(".*.backup-*")), [])
            query_db.chmod(0o666)

    def test_chembl_runtime_disk_guard_preserves_previous_database(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store, chembl_dir, source_db, archive, catalog = create_chembl_fixture(root)
            query_db = chembl_dir / "chembl_query.sqlite"
            manifest = create_previous_chembl_query(query_db)
            old_database = query_db.read_bytes()
            old_manifest = manifest.read_text(encoding="utf-8")
            target_budget = installer.chembl_query_target_staging_bytes(
                source_db.stat().st_size,
                archive.stat().st_size,
            )
            high_free = source_db.stat().st_size + target_budget + installer.DISK_SAFETY_MARGIN_BYTES + 1
            calls = 0

            def shrinking_disk(_path: Path):
                nonlocal calls
                calls += 1
                free = high_free if calls <= 2 else installer.DISK_SAFETY_MARGIN_BYTES
                return mock.Mock(free=free)

            scratch = root / "scratch"
            with mock.patch.object(installer.shutil, "disk_usage", side_effect=shrinking_disk):
                result = installer.publish_chembl_query_database(
                    store,
                    catalog,
                    {},
                    scratch_dir=scratch,
                )
            self.assertEqual(result["status"], "needs_repair")
            self.assertIn("chembl_target_disk_safety_margin_reached", result["reason"])
            self.assertEqual(query_db.read_bytes(), old_database)
            self.assertEqual(manifest.read_text(encoding="utf-8"), old_manifest)
            self.assertEqual(list(scratch.glob("chembl-query-rebuild-*")), [])
            self.assertEqual(list(chembl_dir.glob(".*.rebuild-*")), [])
            query_db.chmod(0o666)

    def test_final_install_state_ready_and_needs_repair_are_canonical(self) -> None:
        installer = self.installer
        cases = [
            ("ready", True, "ready", True),
            ("needs_repair", False, "needs_repair", False),
        ]
        for status, install_ready, readiness_status, readiness_ready in cases:
            with self.subTest(status=status), tempfile.TemporaryDirectory() as tmp:
                store = Path(tmp) / "science_store"
                progress = installer.install_progress_payload(
                    status="ready" if install_ready else "needs_repair",
                    stage="完成检查",
                    current_resource="安装/修复",
                    completed_steps=6,
                    total_steps=6,
                    failed_steps=0 if install_ready else 1,
                    message="final gate",
                )
                readiness = {
                    "schema_version": installer.SCIENCE_READINESS_SCHEMA_VERSION,
                    "status": readiness_status,
                    "ready": readiness_ready,
                    "blockers": [] if readiness_ready else [{"code": "fixture_blocker"}],
                }
                payload = {
                    "status": status,
                    "install_ready": install_ready,
                    "summary": "ready" if install_ready else "repair",
                    "next_action": "continue" if install_ready else "repair",
                    "access_url": "http://127.0.0.1:19090/",
                    "progress": progress,
                    "science_store_readiness": readiness,
                }
                canonical = installer.canonical_install_status_payload(payload)
                bound_readiness = installer.bind_science_readiness_to_install_status(
                    store,
                    readiness,
                    canonical,
                )
                with mock.patch.object(
                    installer,
                    "write_json_atomic",
                    wraps=installer.write_json_atomic,
                ) as writer:
                    installer.write_final_install_state(
                        store,
                        canonical,
                        progress,
                        bound_readiness,
                    )
                publication_order = [
                    (Path(call.args[0]).name, call.args[1]["status"])
                    for call in writer.call_args_list
                ]
                self.assertEqual(
                    publication_order,
                    [
                        ("install_status.json", "publishing"),
                        ("science_store_readiness.json", readiness_status),
                        ("install_progress.json", "ready" if install_ready else "needs_repair"),
                        ("install_status.json", status),
                    ],
                )

                persisted_status = json.loads(
                    (store / "install_status.json").read_text(encoding="utf-8")
                )
                persisted_progress = json.loads(
                    (store / "install_progress.json").read_text(encoding="utf-8")
                )
                persisted_readiness = json.loads(
                    (store / "science_store_readiness.json").read_text(encoding="utf-8")
                )
                status_evidence = next(
                    row
                    for row in persisted_readiness["evidence_manifest"]
                    if row["path"] == "install_status.json"
                )
                self.assertEqual(persisted_status["status"], status)
                self.assertEqual(persisted_status["install_ready"], install_ready)
                self.assertEqual(persisted_status["progress"], persisted_progress)
                self.assertEqual(persisted_progress["status"], "ready" if install_ready else "needs_repair")
                self.assertEqual(persisted_readiness["status"], readiness_status)
                self.assertEqual(
                    status_evidence["sha256"],
                    hashlib.sha256((store / "install_status.json").read_bytes()).hexdigest(),
                )

    def test_final_install_state_publishing_marker_is_fail_closed(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            installer.write_json_atomic(
                store / "install_status.json",
                {
                    "status": "publishing",
                    "install_ready": False,
                    "progress": {"status": "publishing"},
                },
            )
            readiness = installer.evaluate_installer_science_store_readiness(
                store,
                persist=False,
            )
            self.assertIn(
                "install_status_not_ready",
                {item["code"] for item in readiness["blockers"]},
            )

    def test_final_install_state_write_failure_restores_all_last_good_files(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            store.mkdir()
            paths = [
                store / "science_store_readiness.json",
                store / "install_progress.json",
                store / "install_status.json",
            ]
            for index, path in enumerate(paths, start=1):
                path.write_bytes(f'{{"last_good":{index}}}\n'.encode("utf-8"))
            last_good = {path: path.read_bytes() for path in paths}
            progress = installer.install_progress_payload(
                status="ready",
                stage="完成检查",
                completed_steps=6,
                total_steps=6,
                message="final gate",
            )
            readiness = {
                "schema_version": installer.SCIENCE_READINESS_SCHEMA_VERSION,
                "status": "ready",
                "ready": True,
                "blockers": [],
            }
            payload = {
                "status": "ready",
                "install_ready": True,
                "summary": "ready",
                "next_action": "continue",
                "access_url": "http://127.0.0.1:19090/",
                "progress": progress,
                "science_store_readiness": readiness,
            }
            canonical = installer.canonical_install_status_payload(payload)
            bound_readiness = installer.bind_science_readiness_to_install_status(
                store,
                readiness,
                canonical,
            )
            real_replace = installer.os.replace

            def fail_canonical_replace(source: object, destination: object) -> None:
                source_payload = json.loads(Path(source).read_text(encoding="utf-8"))
                if (
                    Path(destination) == store / "install_status.json"
                    and source_payload.get("status") == "ready"
                ):
                    raise OSError("simulated final status sharing violation")
                real_replace(source, destination)

            with mock.patch.object(installer.os, "replace", side_effect=fail_canonical_replace):
                with self.assertRaises(OSError):
                    installer.write_final_install_state(
                        store,
                        canonical,
                        progress,
                        bound_readiness,
                    )

            for path in paths:
                self.assertEqual(path.read_bytes(), last_good[path])
            self.assertTrue((store / "install_status.json.attempt.json").is_file())

    def test_trusted_http_identity_resumes_from_existing_offset(self) -> None:
        installer = self.installer
        content = b"0123456789abcdef"
        with tempfile.TemporaryDirectory() as tmp, resumable_server(content) as (url, etag, requests):
            destination = Path(tmp) / "resource.bin"
            partial = Path(f"{destination}.part")
            partial.write_bytes(content[:6])
            identity = installer.remote_resource_identity(url)
            installer.write_partial_identity(partial, identity)
            result = installer.copy_or_download(url, destination, None)
            self.assertEqual(result["status"], "ready")
            self.assertEqual(destination.read_bytes(), content)
            self.assertEqual(requests[0], ("bytes=6-", etag))
            self.assertFalse(partial.exists())
            self.assertFalse(installer.partial_identity_path(partial).exists())

        large_content = (b"0123456789abcdef" * 400_000)[:6_000_000]
        cutoff = 2_250_000
        with tempfile.TemporaryDirectory() as tmp, interrupting_resumable_server(
            large_content, cutoff
        ) as (url, etag, requests):
            destination = Path(tmp) / "large-resource.bin"
            result = installer.copy_or_download(url, destination, hashlib.sha256(large_content).hexdigest())
            self.assertEqual(result["status"], "ready")
            self.assertEqual(destination.stat().st_size, len(large_content))
            self.assertEqual(hashlib.sha256(destination.read_bytes()).hexdigest(), hashlib.sha256(large_content).hexdigest())
            self.assertEqual(requests[0], ("", ""))
            self.assertEqual(requests[1], (f"bytes={cutoff}-", etag))
            self.assertFalse(Path(f"{destination}.part").exists())
            self.assertFalse(installer.partial_identity_path(Path(f"{destination}.part")).exists())

        for changed_field in ("etag", "content_length"):
            content = b"new-publisher-object"
            with self.subTest(changed_field=changed_field), tempfile.TemporaryDirectory() as tmp, resumable_server(
                content
            ) as (url, _etag, requests):
                destination = Path(tmp) / "resource.bin"
                partial = Path(f"{destination}.part")
                partial.write_bytes(b"stale")
                current = installer.remote_resource_identity(url)
                stale = dict(current)
                stale[changed_field] = (
                    '"stale-etag"' if changed_field == "etag" else int(current["content_length"]) + 1
                )
                stale["identity_digest"] = "stale-identity"
                installer.write_partial_identity(partial, stale)
                result = installer.copy_or_download(url, destination, hashlib.sha256(content).hexdigest())
                self.assertEqual(result["status"], "ready")
                self.assertEqual(destination.read_bytes(), content)
                self.assertEqual(requests[0], ("", ""), "changed identity must restart instead of append")
                self.assertFalse(partial.exists())
                self.assertFalse(installer.partial_identity_path(partial).exists())

    def test_indexed_source_cleanup_publishes_retention_before_deleting_payload(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            source = store / "downloads/fixture/source.dat"
            derived = store / "indexes/fixture.ready"
            source.parent.mkdir(parents=True)
            derived.parent.mkdir(parents=True)
            source.write_bytes(b"source-payload")
            derived.write_text("ready\n", encoding="utf-8")
            manifest_dir = store / "manifests"
            manifest_dir.mkdir(parents=True)
            (manifest_dir / "source_file_manifest.tsv").write_text(
                "source_id\tsource_label\tsource_class\tlocal_path\tstatus\tfile_count\tbytes\tsha256\tprovenance\trelease_identity\ttrust_source\n"
                f"fixture\tFixture\tdirect_download\tdownloads/fixture/source.dat\tready\t1\t{source.stat().st_size}\t{'a' * 64}\thttps://example.invalid/source\tfixture-v1\tpublisher_digest\n",
                encoding="utf-8",
            )
            (store / "download_execution_manifest.tsv").write_text(
                "source_id\tsha256\n" f"fixture\t{'b' * 64}\n",
                encoding="utf-8",
            )
            policies = {
                "fixture": {
                    "payloads": ("downloads/fixture/source.dat",),
                    "derived": ("indexes/fixture.ready",),
                }
            }
            installer.write_source_retention_rows(
                store,
                [
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": "rcsb_ligand_bound",
                        "state": "runtime_cache_bounded",
                    }
                ],
            )
            with mock.patch.object(installer, "INDEXED_SOURCE_RETENTION_POLICIES", policies), mock.patch.object(
                installer, "indexed_source_derivatives_ready", return_value=True
            ):
                result = installer.prune_indexed_source_payloads(store)
            self.assertEqual(result["status"], "ready")
            self.assertFalse(source.exists())
            retention = installer.read_tsv(store / "manifests/source_retention_manifest.tsv")
            retention_by_id = {row["source_id"]: row for row in retention}
            self.assertEqual(retention_by_id["fixture"]["state"], "pruned")
            self.assertGreater(int(retention_by_id["fixture"]["pruned_bytes"]), 0)
            self.assertEqual(retention_by_id["rcsb_ligand_bound"]["state"], "runtime_cache_bounded")
            source_row = installer.read_tsv(manifest_dir / "source_file_manifest.tsv")[0]
            self.assertEqual(source_row["status"], "ready")
            self.assertEqual(source_row["storage_state"], "indexed_source_pruned")
            self.assertTrue(
                installer.retained_source_ready(
                    store,
                    "fixture",
                    "downloads/fixture/source.dat",
                    expected_release_identity="fixture-v1",
                    expected_trust_source="publisher_digest",
                    expected_sha256="b" * 64,
                )
            )
            self.assertFalse(
                installer.retained_source_ready(
                    store,
                    "fixture",
                    "downloads/fixture/source.dat",
                    expected_release_identity="fixture-v2",
                )
            )

    def test_interrupted_source_cleanup_reconciles_before_capacity(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            derived = store / "indexes/fixture.ready"
            derived.parent.mkdir(parents=True)
            derived.write_text("ready\n", encoding="utf-8")
            manifest_dir = store / "manifests"
            manifest_dir.mkdir(parents=True)
            (manifest_dir / "source_file_manifest.tsv").write_text(
                "source_id\tstatus\tstorage_state\tsha256\trelease_identity\ttrust_source\n"
                f"fixture\tready\tmaterialized\t{'a' * 64}\tfixture-v1\tpublisher_digest\n",
                encoding="utf-8",
            )
            installer.write_source_retention_rows(
                store,
                [
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": "fixture",
                        "source_local_path": "downloads/fixture/source.dat",
                        "state": "pruning",
                        "source_files": "1",
                        "source_bytes": "14",
                        "source_manifest_sha256": "a" * 64,
                        "execution_sha256": "b" * 64,
                        "release_identity": "fixture-v1",
                        "trust_source": "publisher_digest",
                        "payload_records_json": json.dumps(
                            [
                                {
                                    "path": "downloads/fixture/source.dat",
                                    "kind": "file",
                                    "bytes": 14,
                                }
                            ]
                        ),
                        "derived_paths": "indexes/fixture.ready",
                    }
                ],
            )
            with mock.patch.object(installer, "indexed_source_derivatives_ready", return_value=True):
                result = installer.reconcile_interrupted_source_retention(store)
            self.assertEqual(result["reconciled"], 1)
            retention = installer.read_tsv(store / "manifests/source_retention_manifest.tsv")[0]
            self.assertEqual(retention["state"], "pruned")
            self.assertEqual(retention["pruned_bytes"], "14")
            source_row = installer.read_tsv(manifest_dir / "source_file_manifest.tsv")[0]
            self.assertEqual(source_row["storage_state"], "indexed_source_pruned")

    def test_rcsb_retention_gate_supports_complete_mode_and_checks_live_cache_limits(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            manifest_dir = store / "manifests"
            manifest_dir.mkdir(parents=True)
            source_manifest = manifest_dir / "source_file_manifest.tsv"
            source_manifest.write_text(
                "source_id\tstatus\tstorage_state\tsha256\trelease_identity\ttrust_source\n"
                f"rcsb_ligand_bound\tready\tmaterialized\t{'a' * 64}\trcsb-current\tpublisher_https\n",
                encoding="utf-8",
            )
            scope = store / "downloads/rcsb/ligand_bound_pdb_scope.json"
            scope.parent.mkdir(parents=True)
            scope.write_text('{"scope_mode":"complete"}\n', encoding="utf-8")
            installer.write_source_retention_rows(
                store,
                [
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": "fixture",
                        "state": "pruned",
                    }
                ],
            )
            complete = installer.evaluate_installer_science_store_readiness(
                store,
                persist=False,
                check_install_status=False,
            )
            self.assertNotIn(
                "rcsb_runtime_cache_not_bounded",
                {item["code"] for item in complete["blockers"]},
            )

            source_manifest.write_text(
                "source_id\tstatus\tstorage_state\tsha256\trelease_identity\ttrust_source\n"
                f"rcsb_ligand_bound\tready\tbounded_runtime_cache\t{'a' * 64}\trcsb-current\tpublisher_https\n",
                encoding="utf-8",
            )
            scope.write_text('{"scope_mode":"on-demand"}\n', encoding="utf-8")
            cache_file = store / "downloads/rcsb/mmcif/ab/1abc.cif.gz"
            cache_file.parent.mkdir(parents=True)
            cache_file.write_bytes(b"oversized")
            (store / "downloads/rcsb/on_demand_policy.json").write_text(
                json.dumps(
                    {
                        "mode": "on_demand",
                        "max_cache_bytes": 1,
                        "max_cache_files": 1,
                    }
                ),
                encoding="utf-8",
            )
            installer.write_source_retention_rows(
                store,
                [
                    {
                        "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                        "source_id": "rcsb_ligand_bound",
                        "state": "runtime_cache_bounded",
                    }
                ],
            )
            with mock.patch.object(installer, "RCSB_ON_DEMAND_MAX_CACHE_BYTES", 1), mock.patch.object(
                installer,
                "RCSB_ON_DEMAND_MAX_CACHE_FILES",
                1,
            ):
                on_demand = installer.evaluate_installer_science_store_readiness(
                    store,
                    persist=False,
                    check_install_status=False,
                )
            self.assertIn(
                "rcsb_runtime_cache_not_bounded",
                {item["code"] for item in on_demand["blockers"]},
            )

    def test_rcsb_structure_publication_uses_bounded_retention_identity_after_prune(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            first = store / "downloads/rcsb/mmcif/ab/1abc.cif.gz"
            second = store / "downloads/rcsb/mmcif/de/2def.cif.gz"
            first.parent.mkdir(parents=True)
            second.parent.mkdir(parents=True)
            first.write_bytes(b"rcsb-1abc")
            second.write_bytes(b"rcsb-2def")
            indexes = store / "indexes"
            indexes.mkdir(parents=True)
            table = indexes / "structure_targets.tsv"
            table.write_text(
                "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
                "sequence_tokens\tstructure_tokens\tevidence\n"
                "RCSB\t1ABC\t1ABC\tunknown\tstructure_record\tunknown\tunknown\t1abc\trcsb\n"
                "RCSB\t2DEF\t2DEF\tunknown\tstructure_record\tunknown\tunknown\t2def\trcsb\n",
                encoding="utf-8",
            )
            source_summary = installer.resource_payload_summary(first.parents[1])
            source_manifest = store / "manifests/source_file_manifest.tsv"
            source_manifest.parent.mkdir(parents=True)
            fields = [
                "source_id",
                "local_path",
                "status",
                "storage_state",
                "file_count",
                "bytes",
                "sha256",
                "release_identity",
                "trust_source",
            ]
            source_row = {
                "source_id": "rcsb_ligand_bound",
                "local_path": "downloads/rcsb/mmcif",
                "status": "ready",
                "storage_state": "materialized",
                "file_count": str(source_summary["file_count"]),
                "bytes": str(source_summary["bytes"]),
                "sha256": str(source_summary["sha256"]),
                "release_identity": "rcsb-build-snapshot",
                "trust_source": "publisher_https_mmcif_parse",
            }
            installer.write_simple_tsv(source_manifest, fields, [source_row])
            source_paths = [
                "downloads/rcsb/mmcif/ab/1abc.cif.gz",
                "downloads/rcsb/mmcif/de/2def.cif.gz",
            ]
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\n"
                + "".join(
                    "structure_targets\t"
                    f"{path}\tused\t1\tindexes/structure_targets.tsv\t"
                    "rcsb_structure_file_rows;complete_source_scan\n"
                    for path in source_paths
                ),
                encoding="utf-8",
            )
            input_summary = installer.structure_contribution_input_summary(store, source_paths)
            write_structure_publication_fixture(
                installer,
                store,
                [
                    {
                        "source": "RCSB",
                        "source_id": "rcsb_ligand_bound",
                        "source_digest": source_summary["sha256"],
                        "release_identity": "rcsb-build-snapshot",
                        "trust_source": "publisher_https_mmcif_parse",
                        "rows": 2,
                        "source_paths": source_paths,
                        "input_bytes": input_summary["input_bytes"],
                        "input_digest": input_summary["input_digest"],
                    }
                ],
            )
            self.assertTrue(installer.structure_targets_publication_ready(store, "RCSB"))

            second.unlink()
            current = installer.resource_payload_summary(first.parents[1])
            source_row.update(
                {
                    "storage_state": "bounded_runtime_cache",
                    "file_count": str(current["file_count"]),
                    "bytes": str(current["bytes"]),
                    "sha256": str(current["sha256"]),
                }
            )
            installer.write_simple_tsv(source_manifest, fields, [source_row])
            retention = {
                "schema_version": installer.SOURCE_RETENTION_SCHEMA_VERSION,
                "source_id": "rcsb_ligand_bound",
                "state": "runtime_cache_bounded",
                "source_manifest_sha256": str(source_summary["sha256"]),
                "release_identity": "rcsb-build-snapshot",
                "trust_source": "publisher_https_mmcif_parse",
                "structure_input_bytes": str(input_summary["input_bytes"]),
                "structure_input_digest": str(input_summary["input_digest"]),
            }
            installer.write_source_retention_rows(store, [retention])
            self.assertTrue(installer.structure_targets_publication_ready(store, "RCSB"))
            self.assertFalse(
                installer.structure_targets_publication_ready(
                    store,
                    "RCSB",
                    require_live_inputs=True,
                )
            )
            retention["trust_source"] = "different_trust"
            installer.write_source_retention_rows(store, [retention])
            self.assertFalse(installer.structure_targets_publication_ready(store, "RCSB"))

    def test_indexed_source_cleanup_rejects_path_outside_downloads(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = root / "science_store"
            outside = root / "outside.dat"
            (store / "downloads").mkdir(parents=True)
            outside.write_bytes(b"keep")
            with self.assertRaises(OSError):
                installer.safe_remove_indexed_payload(store, "../outside.dat")
            self.assertEqual(outside.read_bytes(), b"keep")


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--chembl-crash-child":
        raise SystemExit(run_chembl_crash_child(Path(sys.argv[2]).resolve(), int(sys.argv[3])))
    if len(sys.argv) == 4 and sys.argv[1] == "--database-publish-crash-child":
        raise SystemExit(run_database_publish_crash_child(Path(sys.argv[2]).resolve(), sys.argv[3]))
    unittest.main(testRunner=unittest.TextTestRunner(stream=sys.stdout, verbosity=2))
