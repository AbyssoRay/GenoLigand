from __future__ import annotations

import hashlib
import http.server
import importlib.util
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


def write_database_fixture(prefix: Path, *, empty_dbtype: bool = False) -> None:
    prefix.parent.mkdir(parents=True, exist_ok=True)
    prefix.write_bytes(b"database-payload")
    Path(f"{prefix}.dbtype").write_bytes(b"" if empty_dbtype else b"\x00\x00\x00\x00")
    Path(f"{prefix}.index").write_text("0\t0\t16\n", encoding="utf-8")
    Path(f"{prefix}.lookup").write_text("0\tsmoke\t0\n", encoding="utf-8")


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
        smoke_search = next(command for command in calls if command[1] == "search")
        require(
            smoke_search[2] != str(db_path) and smoke_search[3] != str(db_path),
            "database smoke search must use bounded query and target subsets instead of scanning the full database",
        )

        def successful_smoke(command, env=None):
            calls.append(command)
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_command = successful_smoke
        try:
            require(
                installer.database_prefix_ready("foldseek", db_path, "foldseek", {}, None),
                "complete database with a successful local smoke query must be ready",
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
                write_database_fixture(db_path)
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
                write_database_fixture(db_path)
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
            require(list(root.glob("resource.bin.part.quarantine-*")), "invalid partial must be isolated before full retry")


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
    check_alignment_source_digest_rebuild(installer, "mmseqs")
    check_alignment_source_digest_rebuild(installer, "foldseek")
    check_download_integrity_contracts(installer)
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
                write_database_fixture(tmp_path / command[0] / "target")
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_wsl_database_command = fake_wsl_database_command
        installer.os.name = "nt"
        try:
            status, note = installer.build_mmseqs_database(None, source, tmp_path / "mmseqs" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "mmseqs_createdb_createindex_completed", "WSL MMseqs2 build must not require a Windows PATH executable")
            status, note = installer.build_foldseek_database(None, source, tmp_path / "foldseek" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "foldseek_createdb_createindex_completed", "WSL Foldseek build must not require a Windows PATH executable")
            require(len(calls) == 8 and all(call[0] == "GenoLigandPhase08" for call in calls), "WSL database builds and smoke queries must target the selected project distro")
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
                write_database_fixture(db_path)
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
