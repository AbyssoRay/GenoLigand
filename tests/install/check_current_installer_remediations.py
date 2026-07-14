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
import zipfile
from contextlib import closing, contextmanager
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "scripts/setup/install_complete_scientific.py"


def load_installer():
    spec = importlib.util.spec_from_file_location("current_installer_remediations", INSTALLER)
    if spec is None or spec.loader is None:
        raise RuntimeError("installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def bindingdb_catalog_row(**overrides: str) -> dict[str, str]:
    row = {
        "source_id": "bindingdb",
        "url": "https://www.bindingdb.org/rwd/bind/downloads/BindingDB_All_202606_tsv.zip",
        "local_path": "downloads/bindingdb/bindingdb_all.tsv.zip",
        "sha256": "",
        "release_or_date": "2026-06",
        "release_identity": "BindingDB_All_202606_tsv",
        "trust_source": "publisher_immutable_release_domain_validation",
        "media_type": "bindingdb_tsv",
        "compression": "zip",
        "min_records": "2",
        "license_note": "BindingDB license",
    }
    row.update(overrides)
    return row


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


class CurrentInstallerRemediations(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.installer = load_installer()

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
                handle.write(f"bindingdb\tready\t{'b' * 64}\tBindingDB_All_202606_tsv\tfixture_domain_validation\n")
            result = installer.evaluate_installer_science_store_readiness(store, persist=False)
            bindingdb = next(item for item in result["sources"] if item["logical_source"] == "bindingdb")
            self.assertEqual(bindingdb["source_manifest_status"], "ready")
            self.assertEqual(bindingdb["source_digest"], "b" * 64)
            self.assertEqual(bindingdb["release_identity"], "BindingDB_All_202606_tsv")

    def test_alignment_manifest_schema_and_legacy_probe_migration(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            source = store / "downloads/source.fa"
            db = store / "indexes/mmseqs/source/target"
            source.parent.mkdir(parents=True)
            db.parent.mkdir(parents=True)
            source.write_text(">id\nAAAA\n", encoding="utf-8")
            legacy = {
                "source_id": "source",
                "source_path": "downloads/source.fa",
                "db_path": "indexes/mmseqs/source/target",
                "status": "ready",
            }
            summary = installer.resource_payload_summary(source)
            with mock.patch.object(installer, "database_prefix_ready", return_value=True):
                self.assertFalse(
                    installer.legacy_alignment_manifest_can_migrate(
                        "mmseqs", legacy, store, source, db, "mmseqs", {}, None, summary, "builder-1"
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
            fields = installer.alignment_source_manifest_fields(
                summary, "builder-1"
            )
            self.assertEqual(fields["manifest_schema_version"], installer.ALIGNMENT_MANIFEST_SCHEMA_VERSION)
            for name in ("source_digest", "source_files", "source_bytes", "builder_version"):
                self.assertTrue(fields[name])

    def test_rcsb_scope_excludes_partial_and_binds_coordinate_snapshot(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp)
            root = store / "downloads/rcsb/mmcif"
            coordinate = root / "ab/1abc.cif.gz"
            coordinate.parent.mkdir(parents=True)
            with gzip.open(coordinate, "wt", encoding="utf-8") as handle:
                handle.write("data_1abc\n_entry.id 1ABC\n")
            (root / "ab/1abc.cif.gz.part").write_bytes(b"partial")
            snapshot = installer.write_rcsb_scope_manifest(store, root, {"1ABC", "2DEF"})
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
            readiness = installer.evaluate_installer_science_store_readiness(store, persist=False)
            self.assertIn("rcsb_partial_present", {item["code"] for item in readiness["blockers"]})

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
                "version": "0.9.72",
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
                "runtime_binary_manifest": result,
                "science_store_readiness": {"status": "ready"},
            }
            self.assertFalse(installer.install_ready(payload))

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
                handle.write("data_1abc\n_entry.id 1ABC\n#\n")
            self.assertEqual(installer.rcsb_coordinate_validation(coordinate, "1ABC")["status"], "ready")

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
                    INSERT INTO activities VALUES (1, 1, 'IC50', '=', 7.0, 'nM');
                    INSERT INTO assays VALUES (1, 1);
                    INSERT INTO target_dictionary VALUES (1, 'CHEMBL_T1', 'Target', 'Homo sapiens');
                    INSERT INTO molecule_dictionary VALUES (1, 'Compound');
                    INSERT INTO compound_structures VALUES (1, 'CCO');
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
            result = installer.publish_chembl_query_database(store, catalog, {})
            self.assertEqual(result["status"], "ready")
            query_db = chembl_dir / "chembl_query.sqlite"
            with closing(sqlite3.connect(f"file:{query_db.as_posix()}?mode=ro", uri=True)) as connection:
                row = connection.execute(
                    "SELECT canonical_isomeric_smiles,target_id,source_release "
                    "FROM genoligand_onsite_activity"
                ).fetchone()
            self.assertEqual(row, ("CCO", "CHEMBL_T1", "CHEMBL37"))
            self.assertTrue(installer.chembl_query_database_ready(
                query_db, query_db.with_suffix(".manifest.json"), result["source_digest"], "CHEMBL37"
            ))

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


if __name__ == "__main__":
    unittest.main(testRunner=unittest.TextTestRunner(stream=sys.stdout, verbosity=2))
