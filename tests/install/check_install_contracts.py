from __future__ import annotations

import hashlib
import http.server
import importlib.util
import inspect
import json
import os
import sys
import tempfile
import threading
from contextlib import contextmanager
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def load_installer_module():
    path = ROOT / "scripts/setup/install_complete_scientific.py"
    spec = importlib.util.spec_from_file_location("install_complete_scientific", path)
    require(spec is not None and spec.loader is not None, "installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_large_placeholder(path: Path, size: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as handle:
        handle.seek(size)
        handle.write(b"x")


def write_database_index_fixture(prefix: Path) -> None:
    Path(f"{prefix}.idx").write_bytes(b"precomputed-index")
    Path(f"{prefix}.idx.dbtype").write_bytes(b"\x00\x00\x00\x00")
    Path(f"{prefix}.idx.index").write_text("0\t0\t17\n", encoding="utf-8")


def write_database_fixture(prefix: Path, *, empty_dbtype: bool = False, with_index: bool = True) -> None:
    prefix.parent.mkdir(parents=True, exist_ok=True)
    prefix.write_bytes(b"database-payload")
    Path(f"{prefix}.dbtype").write_bytes(b"" if empty_dbtype else b"\x00\x00\x00\x00")
    Path(f"{prefix}.index").write_text("0\t0\t16\n", encoding="utf-8")
    Path(f"{prefix}.lookup").write_text("0\tsmoke\t0\n", encoding="utf-8")
    if with_index:
        write_database_index_fixture(prefix)


def write_ready_alignment_manifests(
    installer,
    store: Path,
    mmseqs_source_rel: str = "downloads/metagenome/mgy.fa.gz",
) -> None:
    mmseqs_source = store / mmseqs_source_rel
    mmseqs_source.parent.mkdir(parents=True, exist_ok=True)
    if not mmseqs_source.exists():
        mmseqs_source.write_bytes(b">target\nAAAA\n")
    foldseek_source = store / "downloads/rcsb/mmcif"
    foldseek_source.mkdir(parents=True, exist_ok=True)
    if not (foldseek_source / "1abc.cif").exists():
        (foldseek_source / "1abc.cif").write_text("structure\n", encoding="utf-8")
    specs = [
        (
            "mmseqs",
            "metagenome",
            mmseqs_source,
            store / "indexes/mmseqs/metagenome/target",
            store / "indexes/mmseqs/mmseqs_target_databases.tsv",
        ),
        (
            "foldseek",
            "rcsb_mmcif",
            foldseek_source,
            store / "indexes/foldseek/rcsb_mmcif/target",
            store / "indexes/foldseek/foldseek_target_databases.tsv",
        ),
    ]
    for _tool, source_id, source, db_path, manifest in specs:
        write_database_fixture(db_path)
        summary = installer.resource_payload_summary(source)
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(
            "source_id\tsource_path\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\tnotes\n"
            f"{source_id}\t{installer.relative_path(store, source)}\t{summary['sha256']}\t{summary['file_count']}\t{summary['bytes']}\ttest-builder-1\t{installer.relative_path(store, db_path)}\tready\talready_available\n",
            encoding="utf-8",
        )


@contextmanager
def fake_database_tools(installer):
    original_run_command = installer.run_command
    original_which = installer.shutil.which

    def fake_run_command(command, env=None):
        stdout = "test-builder-1\n" if command[1] == "version" else ""
        return installer.subprocess.CompletedProcess(command, 0, stdout, "")

    installer.run_command = fake_run_command
    installer.shutil.which = lambda name, path=None: name
    try:
        yield
    finally:
        installer.run_command = original_run_command
        installer.shutil.which = original_which


@contextmanager
def local_download_server(content: bytes, ranged_response: str):
    requests: list[str] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
            range_header = self.headers.get("Range", "")
            requests.append(range_header)
            if range_header and ranged_response == "416":
                self.send_response(416)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            if range_header and ranged_response == "bad_206":
                body = b"invalid-resumed-suffix"
                self.send_response(206)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Content-Range", f"bytes 3-{2 + len(body)}/{3 + len(body)}")
                self.end_headers()
                self.wfile.write(body)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(content)))
            self.end_headers()
            self.wfile.write(content)

        def log_message(self, _format: str, *args: object) -> None:
            return

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/resource.bin", requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def check_database_readiness_contracts(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        db_path = root / "database" / "target"
        write_database_fixture(db_path, empty_dbtype=True)
        require(
            not installer.database_prefix_ready("mmseqs", db_path, "mmseqs", {}, None),
            "empty .dbtype must not be accepted as a ready database",
        )
        Path(f"{db_path}.dbtype").write_bytes(b"\x00\x00\x00\x00")
        Path(f"{db_path}.index").unlink()
        require(
            not installer.database_prefix_ready("mmseqs", db_path, "mmseqs", {}, None),
            "database missing its index companion must not be ready",
        )
        Path(f"{db_path}.index").write_text("0\t0\t16\n", encoding="utf-8")
        calls: list[list[str]] = []
        original_run_command = installer.run_command

        def failing_smoke(command, env=None):
            calls.append(command)
            return installer.subprocess.CompletedProcess(
                command,
                1 if command[1] == "search" else 0,
                "",
                "smoke failed" if command[1] == "search" else "",
            )

        installer.run_command = failing_smoke
        try:
            require(
                not installer.database_prefix_ready("mmseqs", db_path, "mmseqs", {}, None),
                "database with a failing local smoke query must not be ready",
            )
        finally:
            installer.run_command = original_run_command
        require(any(command[1] == "search" for command in calls), "database readiness must execute a local smoke search")
        require(
            all(command[1] != "createsubdb" for command in calls),
            "database smoke must not invoke createsubdb because it scans oversized lookup/index metadata",
        )
        smoke_search = next(command for command in calls if command[1] == "search")
        require(
            smoke_search[2] != str(db_path) and smoke_search[3] != str(db_path),
            "database smoke search must use bounded query and target subsets instead of scanning the full database",
        )
        smoke_source = inspect.getsource(installer.database_smoke_query)
        require(
            "copy_database_subset" in smoke_source and "lookup.read_text" not in smoke_source,
            "database smoke must build its bounded subset from streamed index offsets",
        )

        def successful_smoke(command, env=None):
            calls.append(command)
            if command[1] == "search":
                query = Path(command[2])
                target = Path(command[3])
                valid = all(
                    path.is_file() and path.stat().st_size > 0
                    for path in [query, Path(f"{query}.dbtype"), Path(f"{query}.index"), target]
                )
                return installer.subprocess.CompletedProcess(command, 0 if valid else 1, "", "")
            if command[1] == "view":
                index = Path(command[2])
                dbtype = Path(f"{index}.dbtype")
                valid = (
                    index.is_file()
                    and index.stat().st_size > 0
                    and dbtype.is_file()
                    and dbtype.read_bytes() == b"\x00\x00\x00\x00"
                    and Path(f"{index}.index").is_file()
                    and Path(f"{index}.index").stat().st_size > 0
                )
                return installer.subprocess.CompletedProcess(command, 0 if valid else 1, "", "")
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_command = successful_smoke
        try:
            require(
                installer.database_prefix_ready("foldseek", db_path, "foldseek", {}, None),
                "complete database with a successful local smoke query must be ready",
            )
        finally:
            installer.run_command = original_run_command
        index_view = next(command for command in calls if command[1] == "view")
        require(index_view[2] == f"{db_path}.idx", "database readiness must inspect the original precomputed index")

        index_dbtype = Path(f"{db_path}.idx.dbtype")
        index_dbtype.write_bytes(b"\xff\xff\xff\xff")
        installer.run_command = successful_smoke
        try:
            require(
                not installer.database_prefix_ready("mmseqs", db_path, "mmseqs", {}, None),
                "database with a corrupt original precomputed index must not be ready",
            )
        finally:
            installer.run_command = original_run_command
        index_dbtype.write_bytes(b"\x00\x00\x00\x00")

        lookup = Path(f"{db_path}.lookup")
        lookup_text = lookup.read_text(encoding="utf-8")
        lookup.write_text("garbage\n", encoding="utf-8")
        installer.run_command = successful_smoke
        try:
            require(
                not installer.database_prefix_ready("mmseqs", db_path, "mmseqs", {}, None),
                "database with malformed lookup metadata must not be ready",
            )
        finally:
            installer.run_command = original_run_command
        lookup.write_text(lookup_text, encoding="utf-8")

        Path(f"{db_path}.idx").unlink()
        Path(f"{db_path}.idx.dbtype").unlink()
        Path(f"{db_path}.idx.index").unlink()
        Path(f"{db_path}.idx.0").write_bytes(b"partial-index")
        Path(f"{db_path}.idx.index.0").write_bytes(b"")
        installer.run_command = successful_smoke
        try:
            require(
                not installer.database_prefix_ready("mmseqs", db_path, "mmseqs", {}, None),
                "partial segmented precomputed indexes must not be treated as an unindexed ready database",
            )
        finally:
            installer.run_command = original_run_command
        source = root / "source.fasta"
        source.write_text(">target\nAAAA\n", encoding="utf-8")
        Path(f"{db_path}.dbtype").write_bytes(b"")
        repair_calls: list[list[str]] = []

        def repair_database(command, env=None):
            repair_calls.append(command)
            if command[1] == "createdb":
                write_database_fixture(Path(command[3]), with_index=False)
            elif command[1] == "createindex":
                write_database_index_fixture(Path(command[2]))
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_command = repair_database
        try:
            status, _note = installer.build_mmseqs_database("mmseqs", source, db_path, {}, None)
        finally:
            installer.run_command = original_run_command
        require(status == "ready", "corrupt database artifacts must be rebuilt into a usable database")
        require(any(command[1] == "createdb" for command in repair_calls), "corrupt database repair must rerun createdb")


def check_external_sync_closure(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        store = root / "science_store"
        plan = root / "source_sync_plan.tsv"
        plan.write_text(
            "source_id\tsource_label\tsync_method\ttarget_rel_path\tready_marker\n"
            "complete\tComplete\tdirect\tdownloads/complete\tdownloads/complete/marker.dat\n"
            "partial\tPartial\tdirect\tdownloads/partial\tdownloads/partial/marker.dat\n",
            encoding="utf-8",
        )
        complete_marker = store / "downloads/complete/marker.dat"
        partial_marker = store / "downloads/partial/marker.dat"
        complete_marker.parent.mkdir(parents=True)
        partial_marker.parent.mkdir(parents=True)
        complete_marker.write_bytes(b"complete")
        partial_marker.write_bytes(b"partial")
        manifest = store / "external_sync_execution_manifest.tsv"
        plan_rows = installer.read_tsv(plan)
        manifest.write_text(
            "source_id\tsource_label\ttarget_rel_path\tstatus\tbytes\tsha256\tplan_digest\tnotes\n"
            f"complete\tComplete\tdownloads/complete\tready\t{complete_marker.stat().st_size}\t{installer.sha256_file(complete_marker)}\t{installer.external_plan_digest(plan_rows[0])}\tcompleted\n"
            f"partial\tPartial\tdownloads/partial\tfailed\t0\t\t{installer.external_plan_digest(plan_rows[1])}\tmissing_required_object\n",
            encoding="utf-8",
        )
        result = installer.sync_external_sources(plan, store, True)
        require(result["status"] == "needs_sync", "a failed required plan row must keep external sync incomplete")
        require(result["ready"] == 1 and result["needs_action"] == 1, "only proven successful plan rows may be reused")
        summary = installer.source_sync_summary(plan, store, {**result, "failed": 1})
        require(summary["status"] == "failed", "partial external sync must keep the source summary failed")
        require(summary["ready"] == 1, "an arbitrary payload marker must not hide a failed required row")
        changed_plan_row = {**plan_rows[0], "include_pattern": "new-required-object.dat"}
        completed_execution = installer.read_tsv(manifest)[0]
        require(
            not installer.external_plan_row_ready(
                changed_plan_row,
                store / "downloads/complete",
                complete_marker,
                store,
                completed_execution,
            ),
            "a changed required plan must invalidate the previous successful marker",
        )
        for field, value in [("max_depth", "9"), ("gcs_bucket", "other-bucket"), ("gcs_prefix", "other-prefix")]:
            changed = {**plan_rows[0], field: value}
            require(
                installer.external_plan_digest(changed) != installer.external_plan_digest(plan_rows[0]),
                f"external plan digest must include {field}",
            )
        payload = {
            "status": "ready",
            "downloads": {"status": "ready"},
            "external_sources": summary,
            "resources": {"status": "ready"},
            "tool_environment": {"status": "ready"},
            "source_manifests": {"status": "ready"},
            "alignment_databases": {"status": "ready"},
            "forcefields": {"status": "ready"},
        }
        require(not installer.install_ready(payload), "partial external sync must keep install readiness false")


def check_external_download_result_closure(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        original_extract = installer.extract_html_links
        original_hrefs = installer.list_html_hrefs
        original_copy = installer.copy_or_download
        original_urlopen = installer.urlopen_with_compatible_tls
        original_scope = installer.ligand_bound_pdb_scope

        class JsonResponse:
            def __enter__(self):
                return self

            def __exit__(self, _exc_type, _exc, _traceback):
                return False

            def read(self) -> bytes:
                return b'{"items":[{"name":"prefix/new.dat","mediaLink":"https://fixture/new.dat"}]}'

        installer.extract_html_links = lambda _url, _pattern: ["https://fixture/new.dat"]
        installer.list_html_hrefs = lambda _url: ["new.dat"]
        installer.copy_or_download = lambda *_args, **_kwargs: {
            "status": "checksum_failed",
            "bytes": 0,
            "sha256": "bad",
        }
        installer.urlopen_with_compatible_tls = lambda *_args, **_kwargs: JsonResponse()
        installer.ligand_bound_pdb_scope = lambda _store: {"1ABC"}
        try:
            html_links_target = root / "html-links"
            html_tree_target = root / "html-tree"
            gcs_target = root / "gcs"
            for target in (html_links_target, html_tree_target, gcs_target):
                target.mkdir()
                (target / "old.dat").write_bytes(b"old")
            html_links = installer.sync_html_links(
                {"source_url": "https://fixture/", "include_pattern": ".*"}, html_links_target
            )
            html_tree = installer.sync_html_tree(
                {"source_url": "https://fixture/", "include_pattern": ".*"}, html_tree_target
            )
            gcs = installer.sync_gcs_prefix(
                {"gcs_bucket": "fixture", "gcs_prefix": "prefix", "include_pattern": ".*"}, gcs_target
            )
            rcsb_target = root / "rcsb"
            rcsb_target.mkdir()
            (rcsb_target / "old.cif.gz").write_bytes(b"old")
            rcsb = installer.sync_rcsb_ligand_bound_scope(
                {"source_url": "https://fixture/{pdb_id}.cif.gz"}, rcsb_target, root
            )
        finally:
            installer.extract_html_links = original_extract
            installer.list_html_hrefs = original_hrefs
            installer.copy_or_download = original_copy
            installer.urlopen_with_compatible_tls = original_urlopen
            installer.ligand_bound_pdb_scope = original_scope
        require(html_links["status"] == "failed", "html-links sync must fail when a required download returns non-ready")
        require(html_tree["status"] == "failed", "html-tree sync must fail when a required download returns non-ready")
        require(gcs["status"] == "failed", "GCS sync must fail when a required download returns non-ready")
        require(rcsb["status"] == "failed", "RCSB scope sync must fail when a required download returns non-ready")


def check_alignment_source_digest_rebuild(installer, tool: str) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        store = Path(tmp) / "science_store"
        if tool == "mmseqs":
            source = store / "downloads/metagenome/source.fasta"
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            db_path = store / "indexes/mmseqs/metagenome_source/target"
            source.parent.mkdir(parents=True)
            source.write_text(">old\nAAAA\n", encoding="utf-8")
            source_id = "metagenome_source"
        else:
            source = store / "downloads/rcsb/mmcif"
            manifest = store / "indexes/foldseek/foldseek_target_databases.tsv"
            db_path = store / "indexes/foldseek/rcsb_mmcif/target"
            source.mkdir(parents=True)
            (source / "1abc.cif").write_text("old structure\n", encoding="utf-8")
            source_id = "rcsb_mmcif"
        write_database_fixture(db_path)
        old_summary = installer.resource_payload_summary(source)
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(
            "source_id\tsource_path\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\tnotes\n"
            f"{source_id}\t{installer.relative_path(store, source)}\t{old_summary['sha256']}\t{old_summary['file_count']}\t{old_summary['bytes']}\ttest-builder-1\t{installer.relative_path(store, db_path)}\tready\talready_available\n",
            encoding="utf-8",
        )
        if source.is_file():
            source.write_text(">new\nCCCC\n", encoding="utf-8")
        else:
            (source / "1abc.cif").write_text("new structure\n", encoding="utf-8")
        calls: list[list[str]] = []
        original_run_command = installer.run_command
        original_which = installer.shutil.which

        def fake_run_command(command, env=None):
            calls.append(command)
            if command[1] == "createdb":
                write_database_fixture(Path(command[3]), with_index=False)
            elif command[1] == "createindex":
                write_database_index_fixture(Path(command[2]))
            stdout = "test-builder-1\n" if command[1] == "version" else ""
            return installer.subprocess.CompletedProcess(command, 0, stdout, "")

        installer.run_command = fake_run_command
        installer.shutil.which = lambda _name, path=None: tool
        try:
            rows = (
                installer.build_mmseqs_target_databases(store, {}, None)
                if tool == "mmseqs"
                else installer.build_foldseek_target_database(store, {}, None)
            )
        finally:
            installer.run_command = original_run_command
            installer.shutil.which = original_which
        require(rows[0]["status"] == "ready", f"{tool} database must rebuild successfully after source change")
        require(any(command[1] == "createdb" for command in calls), f"{tool} source digest change must force createdb")
        manifest_rows = installer.read_tsv(manifest)
        current_summary = installer.resource_payload_summary(source)
        require(manifest_rows[0]["source_digest"] == current_summary["sha256"], f"{tool} manifest must record current source digest")
        require(int(manifest_rows[0]["source_files"]) == current_summary["file_count"], f"{tool} manifest must record source file count")
        require(int(manifest_rows[0]["source_bytes"]) == current_summary["bytes"], f"{tool} manifest must record source bytes")
        require(manifest_rows[0]["builder_version"] == "test-builder-1", f"{tool} manifest must record builder version")
        if tool == "mmseqs":
            require(manifest_rows[0]["source_namespace"] == "MGnify", "MMseqs manifest must record source namespace")
            require(manifest_rows[0]["source_class"] == "metagenome", "MMseqs manifest must record source class")


def check_plinder_canonical_digest_gate(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "annotation_table.parquet"
        canonical = root / "plinder_targets.tsv"
        manifest = root / "plinder_targets.manifest.json"
        source.write_bytes(b"PAR1fixturePAR1")
        canonical.write_text("source\ttarget_id\nPLINDER\tSYSTEM_1\n", encoding="utf-8")
        manifest.write_text(
            json.dumps(
                {
                    "status": "ready",
                    "source": {
                        "sha256": installer.sha256_file(source),
                        "size_bytes": source.stat().st_size,
                    },
                    "canonical": {
                        "row_count": 1,
                        "sha256": installer.sha256_file(canonical),
                        "size_bytes": canonical.stat().st_size,
                    },
                }
            ),
            encoding="utf-8",
        )
        require(
            installer.plinder_canonical_index_ready(source, canonical, manifest),
            "matching PLINDER source and canonical digests must be reusable",
        )
        source.write_bytes(b"PAR1changedPAR1")
        require(
            not installer.plinder_canonical_index_ready(source, canonical, manifest),
            "changed PLINDER source digest must invalidate the canonical index",
        )


def check_alignment_rebuild_failure_preserves_previous(installer, tool: str) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        store = Path(tmp) / "science_store"
        if tool == "mmseqs":
            source = store / "downloads/metagenome/source.fasta"
            source.parent.mkdir(parents=True)
            source.write_text(">old\nAAAA\n", encoding="utf-8")
            manifest = store / "indexes/mmseqs/mmseqs_target_databases.tsv"
            db_path = store / "indexes/mmseqs/metagenome_source/target"
            source_id = "metagenome_source"
        else:
            source = store / "downloads/rcsb/mmcif"
            source.mkdir(parents=True)
            (source / "1abc.cif").write_text("old structure\n", encoding="utf-8")
            manifest = store / "indexes/foldseek/foldseek_target_databases.tsv"
            db_path = store / "indexes/foldseek/rcsb_mmcif/target"
            source_id = "rcsb_mmcif"
        write_database_fixture(db_path)
        old_database = db_path.read_bytes()
        old_summary = installer.resource_payload_summary(source)
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text(
            "source_id\tsource_path\tsource_digest\tsource_files\tsource_bytes\tbuilder_version\tdb_path\tstatus\tnotes\n"
            f"{source_id}\t{installer.relative_path(store, source)}\t{old_summary['sha256']}\t{old_summary['file_count']}\t{old_summary['bytes']}\ttest-builder-1\t{installer.relative_path(store, db_path)}\tready\talready_available\n",
            encoding="utf-8",
        )
        if source.is_file():
            source.write_text(">new\nCCCC\n", encoding="utf-8")
        else:
            (source / "1abc.cif").write_text("new structure\n", encoding="utf-8")
        original_run_command = installer.run_command
        original_which = installer.shutil.which

        def failing_rebuild(command, env=None):
            if command[1] == "version":
                return installer.subprocess.CompletedProcess(command, 0, "test-builder-1\n", "")
            if command[1] == "createdb":
                raise OSError("fixture rebuild failed")
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_command = failing_rebuild
        installer.shutil.which = lambda _name, path=None: tool
        try:
            rows = (
                installer.build_mmseqs_target_databases(store, {}, None)
                if tool == "mmseqs"
                else installer.build_foldseek_target_database(store, {}, None)
            )
        finally:
            installer.run_command = original_run_command
            installer.shutil.which = original_which
        require(db_path.read_bytes() == old_database, f"{tool} failed rebuild must preserve the previous database")
        require(rows[0]["status"] == "failed", f"{tool} failed rebuild must return a failed row")
        require(installer.read_tsv(manifest)[0]["status"] == "failed", f"{tool} manifest must not remain ready after rebuild failure")


def check_corrupt_database_rebuild_preserves_previous(installer, tool: str) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "source.dat"
        source.write_text(">target\nMKTAYIAKQRQISFVKSHFSRQ\n" if tool == "mmseqs" else "structure", encoding="utf-8")
        db_path = root / tool / "target"
        write_database_fixture(db_path, empty_dbtype=True)
        old_database = db_path.read_bytes()
        original_run_command = installer.run_command

        def failing_rebuild(command, env=None):
            if command[1] == "createdb":
                raise OSError("fixture corrupt rebuild failed")
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_command = failing_rebuild
        try:
            status, _note = (
                installer.build_mmseqs_database(tool, source, db_path, {}, None)
                if tool == "mmseqs"
                else installer.build_foldseek_database(tool, source, db_path, {}, None)
            )
        finally:
            installer.run_command = original_run_command
        require(status == "failed", f"{tool} corrupt rebuild failure must be reported")
        require(db_path.read_bytes() == old_database, f"{tool} corrupt rebuild failure must preserve existing database files")


def check_atomic_publish_cleanup_status(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        live_db = root / "database" / "target"
        staged_db = root / ".database.rebuild-fixture" / "target"
        write_database_fixture(live_db)
        write_database_fixture(staged_db)
        staged_db.write_bytes(b"new-database")
        original_rmtree = installer.shutil.rmtree

        def fail_backup_cleanup(path, *args, **kwargs):
            if ".previous-" in Path(path).name:
                raise OSError("fixture backup cleanup failed")
            return original_rmtree(path, *args, **kwargs)

        installer.shutil.rmtree = fail_backup_cleanup
        try:
            cleanup_complete = installer.publish_database_directory(staged_db, live_db)
        finally:
            installer.shutil.rmtree = original_rmtree
        require(live_db.read_bytes() == b"new-database", "atomic publish must leave the new database live")
        require(cleanup_complete is False, "backup cleanup failure must be reported separately from publish success")
        for child in root.iterdir():
            if ".previous-" in child.name:
                original_rmtree(child)


def check_resource_summary_cache(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        source = Path(tmp) / "large-source.dat"
        source.write_bytes(b"first-content")
        cache = {}
        calls = 0
        original_hash = installer.sha256_file_with_progress

        def counted_hash(path, progress=None):
            nonlocal calls
            calls += 1
            return original_hash(path, progress)

        installer.sha256_file_with_progress = counted_hash
        try:
            source_token = installer.file_change_token(source, source.stat())
            first = installer.resource_payload_summary(source, cache=cache)
            second = installer.resource_payload_summary(source, cache=cache)
            expected_calls = 1 if source_token else 2
            require(first == second and calls == expected_calls, "source summary reuse must follow change-token availability")
            original_stat = source.stat()
            source.write_bytes(b"other-content")
            os.utime(source, ns=(original_stat.st_atime_ns, original_stat.st_mtime_ns))
            changed_token = installer.file_change_token(source, source.stat())
            if source_token:
                require(changed_token and changed_token != source_token, "source change token must detect restored-mtime writes")
            changed = installer.resource_payload_summary(source, cache=cache)
        finally:
            installer.sha256_file_with_progress = original_hash
        expected_calls += 1
        require(
            changed["sha256"] != first["sha256"] and calls == expected_calls,
            "source summary cache must invalidate after source changes",
        )

        directory = Path(tmp) / "source-directory"
        directory.mkdir()
        files = [directory / f"source-{index}.dat" for index in range(5)]
        for index, path in enumerate(files):
            path.write_bytes(f"content-{index}".encode("ascii"))
        installer.sha256_file_with_progress = counted_hash
        try:
            directory_start_calls = calls
            directory_tokens = [installer.file_change_token(path, path.stat()) for path in files]
            directory_cacheable = all(directory_tokens)
            first_directory = installer.resource_payload_summary(directory, cache=cache)
            second_directory = installer.resource_payload_summary(directory, cache=cache)
            expected_directory_calls = directory_start_calls + (5 if directory_cacheable else 10)
            require(
                first_directory == second_directory and calls == expected_directory_calls,
                "directory summary reuse must follow change-token availability",
            )
            original_stat = files[1].stat()
            files[1].write_bytes(b"changed-1")
            os.utime(files[1], ns=(original_stat.st_atime_ns, original_stat.st_mtime_ns))
            changed_directory_token = installer.file_change_token(files[1], files[1].stat())
            if directory_cacheable:
                require(
                    changed_directory_token and changed_directory_token != directory_tokens[1],
                    "directory change token must detect writes to every source position",
                )
            changed_directory = installer.resource_payload_summary(directory, cache=cache)
        finally:
            installer.sha256_file_with_progress = original_hash
        expected_directory_calls += 5
        require(
            changed_directory["sha256"] != first_directory["sha256"] and calls == expected_directory_calls,
            "directory cache must invalidate when any same-size source changes with restored mtime",
        )

        uncacheable = Path(tmp) / "uncacheable-source.dat"
        uncacheable.write_bytes(b"uncacheable")
        uncacheable_start_calls = calls
        original_change_token = installer.file_change_token
        installer.file_change_token = lambda _path, _stat: ""
        installer.sha256_file_with_progress = counted_hash
        try:
            first_uncacheable = installer.resource_payload_summary(uncacheable, cache=cache)
            second_uncacheable = installer.resource_payload_summary(uncacheable, cache=cache)
        finally:
            installer.sha256_file_with_progress = original_hash
            installer.file_change_token = original_change_token
        require(first_uncacheable == second_uncacheable, "uncacheable source summaries must remain deterministic")
        require(calls == uncacheable_start_calls + 2, "missing change tokens must disable digest cache reuse")
        require(str(uncacheable.resolve()) not in cache, "uncacheable source summaries must not remain in the digest cache")


def check_database_subset_io_bounds(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        prefix = root / "source" / "target"
        subset = root / "subset" / "target"
        prefix.parent.mkdir(parents=True)
        prefix.write_bytes(b"A" * 64)
        Path(f"{prefix}.dbtype").write_bytes(b"\x00\x00\x00\x00")
        index_rows = [f"{key}\t{key}\t1\n" for key in range(64)]
        index_rows.append("malformed-tail\n")
        Path(f"{prefix}.index").write_text("".join(index_rows), encoding="ascii")
        lookup_rows = [f"padding_{key}\tentry_{key}\t0\n" for key in range(4032)]
        lookup_rows.extend(f"{key}\tentry_{key}\t0\n" for key in range(64))
        lookup_rows.append("malformed-tail\n")
        Path(f"{prefix}.lookup").write_text("".join(lookup_rows), encoding="ascii")
        header = Path(f"{prefix}_h")
        header.write_bytes(b"H" * 64)
        Path(f"{header}.dbtype").write_bytes(b"\x00\x00\x00\x00")
        header_index_rows = [f"padding_{key}\t0\t1\n" for key in range(4032)]
        header_index_rows.extend(f"{key}\t{key}\t1\n" for key in range(64))
        header_index_rows.append("malformed-tail\n")
        Path(f"{header}.index").write_text("".join(header_index_rows), encoding="ascii")

        reads: dict[str, dict[str, int]] = {}
        original_path_open = installer.Path.open

        class BoundedTextReader:
            def __init__(self, handle, key):
                self.handle = handle
                self.key = key
                reads[key] = {"lines": 0, "bytes": 0}

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return self.handle.__exit__(exc_type, exc_value, traceback)

            def readline(self, size=-1):
                require(size == 4096, "database metadata reads must use the 4096-byte line bound")
                value = self.handle.readline(size)
                reads[self.key]["lines"] += 1
                reads[self.key]["bytes"] += len(value.encode("utf-8"))
                require(reads[self.key]["lines"] <= 4096, "database metadata reads must stop at 4096 lines")
                require(reads[self.key]["bytes"] <= 4096 * 4096, "database metadata reads must remain byte-bounded")
                return value

            def read(self, *args, **kwargs):
                raise AssertionError("database metadata must not be read into memory in one operation")

            def __getattr__(self, name):
                return getattr(self.handle, name)

        def monitored_path_open(path, *args, **kwargs):
            handle = original_path_open(path, *args, **kwargs)
            mode = args[0] if args else kwargs.get("mode", "r")
            key = str(path)
            if "r" in mode and "b" not in mode and (key.endswith(".index") or key.endswith(".lookup")):
                return BoundedTextReader(handle, key)
            return handle

        installer.Path.open = monitored_path_open
        try:
            copied = installer.copy_database_subset(prefix, subset)
        finally:
            installer.Path.open = original_path_open
        require(copied, "bounded database subset must copy valid records at the metadata scan limit")
        require(subset.stat().st_size == 64, "bounded database subset must not copy source data beyond selected records")
        require(len(Path(f"{subset}.index").read_text(encoding="ascii").splitlines()) == 64, "bounded subset must contain exactly 64 index rows")
        require(reads[str(Path(f"{prefix}.lookup"))]["lines"] == 4096, "lookup scan must stop after its bounded line budget")
        require(reads[str(Path(f"{header}.index"))]["lines"] == 4096, "companion index scan must stop after its bounded line budget")

        oversized = root / "oversized" / "target"
        oversized.parent.mkdir()
        oversized.write_bytes(b"A")
        Path(f"{oversized}.dbtype").write_bytes(b"\x00\x00\x00\x00")
        Path(f"{oversized}.index").write_text(f"0\t0\t{128 * 1024 * 1024 + 1}\n", encoding="ascii")
        Path(f"{oversized}.lookup").write_text("0\tentry_0\t0\n", encoding="ascii")
        oversized_payload_reads = 0

        class RejectPayloadRead:
            def __init__(self, handle):
                self.handle = handle

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return self.handle.__exit__(exc_type, exc_value, traceback)

            def read(self, *args, **kwargs):
                nonlocal oversized_payload_reads
                oversized_payload_reads += 1
                raise AssertionError("oversized database payload must be rejected before reading")

            def __getattr__(self, name):
                return getattr(self.handle, name)

        def reject_oversized_payload_open(path, *args, **kwargs):
            handle = original_path_open(path, *args, **kwargs)
            mode = args[0] if args else kwargs.get("mode", "r")
            return RejectPayloadRead(handle) if Path(path) == oversized and mode == "rb" else handle

        installer.Path.open = reject_oversized_payload_open
        try:
            oversized_copied = installer.copy_database_subset(oversized, root / "oversized-subset" / "target")
        finally:
            installer.Path.open = original_path_open
        require(
            not oversized_copied and oversized_payload_reads == 0,
            "database subset must reject payloads larger than 128 MiB before reading them",
        )


def check_download_integrity_contracts(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        store = root / "science_store"
        source = root / "fresh.bin"
        source.write_bytes(b"fresh-observed-content")
        destination = store / "downloads/source.bin"
        destination.parent.mkdir(parents=True)
        destination.write_bytes(b"stale-content")
        catalog = root / "source_catalog.tsv"
        catalog.write_text(
            "source_id\turl\tlocal_path\tsha256\trelease_or_date\n"
            f"observed\t{source}\tdownloads/source.bin\t\tfixture-v2\n",
            encoding="utf-8",
        )
        manifest = store / "download_execution_manifest.tsv"
        manifest.write_text(
            "source_id\turl\tlocal_path\tstatus\tbytes\tsha256\tobserved_bytes\tobserved_sha256\tsource_version\tcompleted_at\tnotes\n"
            f"observed\t{source}\tdownloads/source.bin\tready\t{source.stat().st_size}\t{installer.sha256_file(source)}\t{source.stat().st_size}\t{installer.sha256_file(source)}\tfixture-v2\t1\tcompleted\n",
            encoding="utf-8",
        )
        result = installer.prepare_downloads(catalog, store)
        require(result["status"] == "ready", "observed-digest mismatch must be recoverable by a full local copy")
        require(destination.read_bytes() == source.read_bytes(), "untrusted existing file must be replaced from the source")
        manifest_row = installer.read_tsv(manifest)[0]
        require(manifest_row["observed_sha256"] == installer.sha256_file(source), "successful no-SHA download must pin observed digest")
        require(int(manifest_row["observed_bytes"]) == source.stat().st_size, "successful no-SHA download must pin observed bytes")
        require(manifest_row["source_version"] == "fixture-v2", "download manifest must retain the source version")
        require(int(manifest_row["completed_at"]) > 0, "successful download must record completion time")
        source.write_bytes(b"changed-observed-content")
        refreshed = installer.prepare_downloads(catalog, store)
        require(refreshed["status"] == "ready", "changed local no-SHA source must be refreshed")
        require(destination.read_bytes() == source.read_bytes(), "observed reuse must compare the current local source content")
        expected_content = source.read_bytes()
        source.unlink()
        reused = installer.prepare_downloads(catalog, store)
        require(reused["status"] == "ready", "matching observed digest and bytes must permit local reuse")
        require(destination.read_bytes() == expected_content, "trusted observed file reuse must preserve content")
        require(
            installer.read_tsv(manifest)[0]["notes"] == "existing_observed_digest_matched",
            "observed reuse must be explicit in the download manifest",
        )

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "new.bin"
        destination = root / "existing.bin"
        source.write_bytes(b"new-content")
        destination.write_bytes(b"old-content")
        old_digest = installer.sha256_file(destination)
        installer.copy_or_download(str(source), destination, None, observed_sha256=old_digest)
        require(destination.read_bytes() == source.read_bytes(), "observed digest without observed bytes must not authorize reuse")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "source.bin"
        destination = root / "destination.bin"
        source.write_bytes(b"file-uri-content")
        first = installer.copy_or_download(source.as_uri(), destination, None)
        source.unlink()
        reused = installer.copy_or_download(
            source.as_uri(),
            destination,
            None,
            observed_sha256=str(first["sha256"]),
            observed_bytes=int(first["bytes"]),
        )
        require(reused["status"] == "ready" and reused.get("reused") is True, "missing file URI source must allow verified observed reuse")

    content = b"complete-http-payload"
    digest = hashlib.sha256(content).hexdigest()
    for ranged_response in ("416", "bad_206"):
        with tempfile.TemporaryDirectory() as tmp, local_download_server(content, ranged_response) as (url, requests):
            root = Path(tmp)
            destination = root / "resource.bin"
            partial = Path(f"{destination}.part")
            partial.write_bytes(b"bad")
            result = installer.copy_or_download(
                url,
                destination,
                None,
                observed_sha256=digest,
                observed_bytes=len(content),
            )
            require(result["status"] == "ready", f"bad partial after HTTP {ranged_response} must recover from zero")
            require(destination.read_bytes() == content, f"HTTP {ranged_response} recovery must publish fully verified content")
            require(requests[0].startswith("bytes="), f"HTTP {ranged_response} fixture must first exercise resume")
            require(requests[-1] == "", f"HTTP {ranged_response} recovery must retry once without Range")
            require(not list(root.glob("resource.bin.part.quarantine-*")), "successful full retry must remove quarantined partial data")
            require(not partial.exists(), "successful full retry must not retain a partial file")


def check_installation_status_rejects_unknown_external_state(installer) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        store = Path(tmp)
        (store / "install_status.json").write_text('{"status":"ready","missing_components":[]}', encoding="utf-8")
        completed = installer.subprocess.CompletedProcess(["fixture"], 0, "", "")
        ready = {"status": "ready"}
        result = installer.installation_status(
            store,
            completed,
            ready,
            {"status": "not_configured"},
            ready,
            ready,
            ready,
            ready,
            {"status": "not_configured"},
            ready,
        )
        require(result["status"] == "needs_repair", "unknown or unconfigured external state must not summarize as ready")


def main() -> int:
    ps1 = read("install.ps1")
    sh = read("install.sh")
    readme = read("README.md")

    require("install_complete_scientific.py" in ps1, "install.ps1 must call complete installer")
    require("install_complete_scientific.py" in sh, "install.sh must call complete installer")
    require("--start-webui" in ps1 and "--start-webui" in sh, "install entrypoints must start WebUI")
    require(".\\install.ps1" in readme, "README must show Windows/WSL2 install command")
    require("./install.sh" in readme, "README must show Linux install command")
    require((ROOT / "python/requirements-workers-cpu.txt").is_file(), "CPU worker requirements missing")
    require((ROOT / "python/requirements-workers-cuda.txt").is_file(), "CUDA worker requirements missing")
    require((ROOT / "python/environment-workers.yml").is_file(), "worker conda environment missing")
    sync_plan = read("configs/source_sync_plan.tsv")
    require("rcsb_ligand_bound_scope" in sync_plan, "RCSB install must use ligand-bound scoped downloads")
    require("pub/pdb/data/structures/divided/mmCIF" not in sync_plan, "RCSB install must not mirror the full mmCIF tree")
    require("gs://plinder/2024-06/v2/index/" in sync_plan, "PLINDER public storage source missing")
    installer_text = read("scripts/setup/install_complete_scientific.py")
    require("build_persistent_alignment_databases" in installer_text, "installer must build persistent MMseqs2/Foldseek target databases")
    require("mmseqs_target_databases.tsv" in installer_text, "installer must record persistent MMseqs2 database manifest")
    require("foldseek_target_databases.tsv" in installer_text, "installer must record persistent Foldseek database manifest")
    require("deferred_large_source" in installer_text, "installer must record explicitly deferred oversized alignment sources as needs_repair")
    require("GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES" in installer_text, "installer must allow an explicit emergency alignment database size cap")
    require("ensure_openff_forcefield_file" in installer_text, "installer must install the OpenFF SMIRNOFF offxml force field file")
    require("openff-2.2.1.offxml" in installer_text, "installer must use a pinned OpenFF Sage force field file")
    require("getattr(xmltodict, '__version__', 'installed')" in installer_text, "xmltodict probe must not require a __version__ attribute")
    require("existing_science_indexes_ready" in installer_text, "installer must reuse ready science indexes instead of rebuilding every install run")
    require("science_indexes_reused" in installer_text, "installer must record when existing science indexes are reused")
    installer = load_installer_module()
    check_database_readiness_contracts(installer)
    check_external_sync_closure(installer)
    check_external_download_result_closure(installer)
    check_alignment_source_digest_rebuild(installer, "mmseqs")
    check_alignment_source_digest_rebuild(installer, "foldseek")
    check_plinder_canonical_digest_gate(installer)
    check_alignment_rebuild_failure_preserves_previous(installer, "mmseqs")
    check_alignment_rebuild_failure_preserves_previous(installer, "foldseek")
    check_corrupt_database_rebuild_preserves_previous(installer, "mmseqs")
    check_corrupt_database_rebuild_preserves_previous(installer, "foldseek")
    check_atomic_publish_cleanup_status(installer)
    check_resource_summary_cache(installer)
    check_database_subset_io_bounds(installer)
    check_download_integrity_contracts(installer)
    check_installation_status_rejects_unknown_external_state(installer)
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        source = tmp_path / "targets.fasta"
        source.write_text(">a\nAAAA\n", encoding="utf-8")
        calls: list[tuple[str, list[str]]] = []
        original_runner = installer.run_wsl_database_command
        original_os_name = installer.os.name

        def fake_wsl_database_command(distro, command):
            calls.append((distro, command))
            if command[1] == "createdb":
                write_database_fixture(tmp_path / command[0] / "target", with_index=False)
            elif command[1] == "createindex":
                write_database_index_fixture(tmp_path / command[0] / "target")
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_wsl_database_command = fake_wsl_database_command
        installer.os.name = "nt"
        try:
            status, note = installer.build_mmseqs_database(None, source, tmp_path / "mmseqs" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "mmseqs_createdb_createindex_completed", "WSL MMseqs2 build must not require a Windows PATH executable")
            status, note = installer.build_foldseek_database(None, source, tmp_path / "foldseek" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "foldseek_createdb_createindex_completed", "WSL Foldseek build must not require a Windows PATH executable")
            require(len(calls) == 8 and all(call[0] == "GenoLigandPhase08" for call in calls), "WSL database builds and bounded index checks must target the selected project distro")
        finally:
            installer.run_wsl_database_command = original_runner
            installer.os.name = original_os_name
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        source = tmp_path / "targets.fasta"
        db_path = tmp_path / "mmseqs" / "target"
        source.write_text(">a\nAAAA\n", encoding="utf-8")
        original_run_command = installer.run_command

        def fake_run_command(command, env=None):
            if command[1] == "createdb":
                write_database_fixture(db_path, with_index=False)
                return installer.subprocess.CompletedProcess(command, 0, "", "")
            if command[1] == "createindex":
                return installer.subprocess.CompletedProcess(command, 1, "", "Cannot fit databases into 12G. Please use a computer with more main memory.")
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_command = fake_run_command
        try:
            status, note = installer.build_mmseqs_database("mmseqs", source, db_path, {}, None)
            require(status == "ready", "created MMseqs2 target database must remain usable when precomputed index exceeds memory")
            require("createdb_ready_createindex_not_precomputed" in note, "MMseqs2 manifest must record unindexed searchable database")
        finally:
            installer.run_command = original_run_command
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        calls: list[list[str]] = []
        original_wsl_home = installer.wsl_home
        original_run_wsl_command = installer.run_wsl_command

        def fake_run_wsl_command(distro, command):
            calls.append(command)
            if command[0] == "bash":
                return installer.subprocess.CompletedProcess(command, 0, "", "")
            if command[0] == "python3":
                return installer.subprocess.CompletedProcess(command, 0, "wsl_gpu=true;wsl_libcuda=true", "")
            if command[1] == "-c" and "payload" in command[2]:
                payload = '{"openmm_ready":true,"version":"8.5.2","platforms":["Reference","CPU","CUDA"],"cuda_ready":true,"plugin_dir":"/plugins","failure_reason":""}'
                return installer.subprocess.CompletedProcess(command, 0, payload, "")
            return installer.subprocess.CompletedProcess(command, 0, "installed\n", "")

        installer.wsl_home = lambda distro: "/home/tester"
        installer.run_wsl_command = fake_run_wsl_command
        try:
            result = installer.ensure_wsl_python_tool_environment(tmp_path / "env", "GenoLigandPhase08", True)
            require(result["status"] == "ready", "skip-install WSL mode must probe an existing worker environment")
            require(not any("-m" in call and "pip" in call for call in calls), "skip-install WSL mode must not run pip install")
        finally:
            installer.wsl_home = original_wsl_home
            installer.run_wsl_command = original_run_wsl_command
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        store = tmp_path / "science_store"
        (store / "indexes").mkdir(parents=True)
        (store / "install_status.json").write_text('{"status":"ready","missing_components":[]}\n', encoding="utf-8")
        (store / "source_requirements.tsv").write_text("source_id\tstatus\nknown_database\tready\n", encoding="utf-8")
        for rel in [
            "indexes/known_targets.tsv",
            "indexes/metagenome_targets.tsv",
            "indexes/structure_targets.tsv",
            "indexes/tool_versions.tsv",
        ]:
            (store / rel).write_text("id\tstatus\nitem\tready\n", encoding="utf-8")
        write_ready_alignment_manifests(installer, store)
        with fake_database_tools(installer):
            require(installer.existing_science_indexes_ready(store), "ready installed indexes must be reused")
        (store / "indexes/mmseqs/mmseqs_target_databases.tsv").write_text(
            "source_id\tsource_path\tdb_path\tstatus\tnotes\nmetagenome\tdownloads/metagenome/mgy.fa.gz\tindexes/mmseqs/metagenome/target\tdeferred_large_source\tsource_exceeds_sync_index_threshold\n",
            encoding="utf-8",
        )
        require(installer.base_science_indexes_ready(store), "base indexes should be reusable while repairing alignment databases")
        with fake_database_tools(installer):
            require(not installer.existing_science_indexes_ready(store), "deferred persistent alignment databases must not be reused as ready")
        write_ready_alignment_manifests(installer, store)
        (store / "downloads/metagenome").mkdir(parents=True, exist_ok=True)
        (store / "downloads/metagenome/mgy.fa.gz").write_bytes(b"x" * 2048)
        old_partial_limit = installer.os.environ.get("GENOLIGAND_PARTIAL_INDEX_LARGE_SOURCE_BYTES")
        installer.os.environ["GENOLIGAND_PARTIAL_INDEX_LARGE_SOURCE_BYTES"] = "1024"
        try:
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\nmetagenome_targets\tdownloads/metagenome/mgy.fa.gz\tused\t5000\tindexes/metagenome_targets.tsv\tfasta_sequence_rows\n",
                encoding="utf-8",
            )
            with fake_database_tools(installer):
                require(not installer.existing_science_indexes_ready(store), "partial legacy large-source slices must not be reused as ready")
        finally:
            if old_partial_limit is None:
                installer.os.environ.pop("GENOLIGAND_PARTIAL_INDEX_LARGE_SOURCE_BYTES", None)
            else:
                installer.os.environ["GENOLIGAND_PARTIAL_INDEX_LARGE_SOURCE_BYTES"] = old_partial_limit
        (store / "index_build_manifest.tsv").write_text(
            "index\tinput_path\tstatus\trows\toutput_path\tnotes\nmetagenome_targets\tdownloads/metagenome/mgy.fa.gz\tused\t50000\tindexes/metagenome_targets.tsv\tfasta_sequence_rows\n",
            encoding="utf-8",
        )
        (store / "indexes/known_targets.tsv").write_text("id\tstatus\n", encoding="utf-8")
        require(not installer.existing_science_indexes_ready(store), "empty installed indexes must not be reused")
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        store = tmp_path / "science_store"
        (store / "indexes").mkdir(parents=True)
        (store / "install_status.json").write_text('{"status":"ready","missing_components":[]}\n', encoding="utf-8")
        (store / "source_requirements.tsv").write_text("source_id\tstatus\nknown_database\tready\n", encoding="utf-8")
        for rel in [
            "indexes/known_targets.tsv",
            "indexes/metagenome_targets.tsv",
            "indexes/structure_targets.tsv",
            "indexes/tool_versions.tsv",
        ]:
            (store / rel).write_text("id\tstatus\nitem\tready\n", encoding="utf-8")
        large_size = installer.DEFAULT_PARTIAL_INDEX_LARGE_SOURCE_BYTES + 1
        write_large_placeholder(store / "downloads/bindingdb/bindingdb_all.tsv.zip", large_size)
        write_large_placeholder(store / "downloads/chembl/chembl_sqlite.tar.gz", large_size)
        write_large_placeholder(store / "downloads/metagenome/mgnify/mgy_clusters.fa.gz", large_size)
        write_ready_alignment_manifests(installer, store, "downloads/metagenome/mgnify/mgy_clusters.fa.gz")
        (store / "index_build_manifest.tsv").write_text(
            "\n".join(
                [
                    "index\tinput_path\tstatus\trows\toutput_path\tnotes",
                    "known_targets\tdownloads/bindingdb/bindingdb_all.tsv.zip\tused\t5000\tindexes/known_targets.tsv\tcanonical_or_source_table_rows",
                    "known_targets\tdownloads/chembl/chembl_sqlite.tar.gz\tused\t5000\tindexes/known_targets.tsv\tchembl_sqlite_archive_rows",
                    "metagenome_targets\tdownloads/metagenome/mgnify/mgy_clusters.fa.gz\tused\t5000\tindexes/metagenome_targets.tsv\tfasta_sequence_rows",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        require(hasattr(installer, "science_index_readiness_rows"), "installer must expose explicit science index readiness classification")
        readiness_rows = installer.science_index_readiness_rows(store)
        statuses = {(row["input_path"], row["readiness_status"]) for row in readiness_rows}
        require(
            ("downloads/bindingdb/bindingdb_all.tsv.zip", "projection_only") in statuses,
            "large BindingDB source with 5000-row TSV must be classified as projection_only",
        )
        require(
            ("downloads/chembl/chembl_sqlite.tar.gz", "projection_only") in statuses,
            "large ChEMBL sqlite archive with 5000-row TSV must be classified as projection_only",
        )
        require(
            ("downloads/metagenome/mgnify/mgy_clusters.fa.gz", "projection_only") in statuses,
            "large MGnify source with 5000-row TSV must be classified as projection_only",
        )
        with fake_database_tools(installer):
            require(not installer.existing_science_indexes_ready(store), "large-source projection TSVs must not make science indexes ready")
        readiness_manifest = store / "index_readiness_manifest.tsv"
        require(readiness_manifest.is_file(), "installer readiness check must persist explicit index readiness manifest")
        readiness_text = readiness_manifest.read_text(encoding="utf-8")
        require("projection_only" in readiness_text, "persisted index readiness manifest must expose projection_only state")
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        store = tmp_path / "science_store"
        (store / "indexes").mkdir(parents=True)
        (store / "install_status.json").write_text('{"status":"ready","missing_components":[]}\n', encoding="utf-8")
        (store / "source_requirements.tsv").write_text("source_id\tstatus\nknown_database\tready\n", encoding="utf-8")
        for rel in [
            "indexes/known_targets.tsv",
            "indexes/metagenome_targets.tsv",
            "indexes/structure_targets.tsv",
            "indexes/tool_versions.tsv",
        ]:
            (store / rel).write_text("id\tstatus\nitem\tready\n", encoding="utf-8")
        write_large_placeholder(store / "downloads/bindingdb/bindingdb_all.tsv.zip", 590_000_000)
        write_ready_alignment_manifests(installer, store, "downloads/metagenome/mgnify/mgy_clusters.fa.gz")
        (store / "index_build_manifest.tsv").write_text(
            "\n".join(
                [
                    "index\tinput_path\tstatus\trows\toutput_path\tnotes",
                    "known_targets\tdownloads/bindingdb/bindingdb_all.tsv.zip\tused\t5000\tindexes/known_targets.tsv\tcanonical_or_source_table_rows",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        readiness_rows = installer.science_index_readiness_rows(store)
        statuses = {(row["input_path"], row["readiness_status"]) for row in readiness_rows}
        require(
            ("downloads/bindingdb/bindingdb_all.tsv.zip", "projection_only") in statuses,
            "BindingDB 600 MB source with 5000-row TSV must be classified as projection_only",
        )
        with fake_database_tools(installer):
            require(not installer.existing_science_indexes_ready(store), "BindingDB 600 MB projection TSV must not make science indexes ready")
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        source = tmp_path / "source" / ("gls-server.exe" if installer.os.name == "nt" else "gls-server")
        runtime = tmp_path / "runtime"
        source.parent.mkdir()
        source.write_text("server", encoding="utf-8")
        calls: list[object] = []
        original_copy2 = installer.shutil.copy2
        original_run = installer.subprocess.run
        original_os_name = installer.os.name

        def copy2_once_locked(src, dst):
            if not calls:
                calls.append("locked")
                raise PermissionError("locked")
            calls.append("copied")
            return original_copy2(src, dst)

        def fake_run(command, **kwargs):
            calls.append(command)
            return installer.subprocess.CompletedProcess(command, 0, "stopped", "")

        installer.shutil.copy2 = copy2_once_locked
        installer.subprocess.run = fake_run
        installer.os.name = "nt"
        try:
            command, status = installer.publish_runtime_binary([str(source)], runtime, "gls-server")
            require(status["status"] == "ready" and Path(command[0]).exists(), "runtime publish must retry after locked exe is stopped")
            require(any(isinstance(call, list) and call[:3] == ["taskkill", "/F", "/IM"] for call in calls), "locked runtime publish must stop the old Windows process")
        finally:
            installer.shutil.copy2 = original_copy2
            installer.subprocess.run = original_run
            installer.os.name = original_os_name
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        source = tmp_path / "large.fasta.gz"
        source.write_bytes(b"x" * 2048)
        require(not installer.alignment_source_deferred(source), "default installer must not defer large alignment sources")
        old_limit = installer.os.environ.get("GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES")
        installer.os.environ["GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES"] = "1024"
        try:
            require(installer.alignment_source_deferred(source), "explicit emergency cap may defer oversized alignment sources")
        finally:
            if old_limit is None:
                installer.os.environ.pop("GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES", None)
            else:
                installer.os.environ["GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES"] = old_limit
    summary = installer.install_user_summary(
        {
            "status": "ready",
            "install_ready": True,
            "summary": "ready",
            "next_action": "continue",
            "source_manifests": {
                "total": 1,
                "ready": 1,
                "hash_ready": 1,
                "license_ready": 1,
                "source_file_manifest": "D:/hidden/source_file_manifest.tsv",
                "hash_manifest": "D:/hidden/hash_manifest.tsv",
                "license_manifest": "D:/hidden/license_manifest.tsv",
            },
            "tool_environment": {
                "status": "ready",
                "python": "D:/hidden/python-worker-env/python.exe",
                "manifest": "D:/hidden/tool_environment_manifest.tsv",
                "ready": 12,
                "total": 12,
            },
            "runtime_binaries": {
                "gls_cli": {"status": "ready", "path": "D:/hidden/gls-cli.exe"},
                "gls_server": {"status": "ready", "path": "D:/hidden/gls-server.exe"},
            },
            "launch_environment": {
                "launcher": "D:/hidden/start-genoligand-webui.cmd",
                "environment": "D:/hidden/launch_environment.json",
            },
            "debug": {"stdout": "raw"},
            "alignment_databases": {"status": "needs_repair", "ready": 2, "deferred": 1, "total": 3},
        }
    )
    text = str(summary)
    require(summary["alignment_databases"]["deferred"] == 1, "install summary must report deferred large alignment sources")
    require(not summary["alignment_databases"]["completed"], "deferred alignment sources must keep install summary incomplete")
    for forbidden in [
        "source_file_manifest",
        "hash_manifest",
        "license_manifest",
        "tool_environment_manifest",
        "python-worker-env",
        "D:/hidden",
        "stdout",
    ]:
        require(forbidden not in text, f"default install summary leaked internal field: {forbidden}")

    print("install_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
