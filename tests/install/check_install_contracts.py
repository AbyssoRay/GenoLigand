from __future__ import annotations

import sys
import importlib.util
import tempfile
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
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        source = tmp_path / "targets.fasta"
        source.write_text(">a\nAAAA\n", encoding="utf-8")
        calls: list[tuple[str, list[str]]] = []
        original_runner = installer.run_wsl_database_command
        original_os_name = installer.os.name

        def fake_wsl_database_command(distro, command):
            calls.append((distro, command))
            return installer.subprocess.CompletedProcess(command, 0, "", "")

        installer.run_wsl_database_command = fake_wsl_database_command
        installer.os.name = "nt"
        try:
            status, note = installer.build_mmseqs_database(None, source, tmp_path / "mmseqs" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "mmseqs_createdb_createindex_completed", "WSL MMseqs2 build must not require a Windows PATH executable")
            status, note = installer.build_foldseek_database(None, source, tmp_path / "foldseek" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "foldseek_createdb_createindex_completed", "WSL Foldseek build must not require a Windows PATH executable")
            require(len(calls) == 4 and all(call[0] == "GenoLigandPhase08" for call in calls), "WSL database builds must target the selected project distro")
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
                db_path.parent.mkdir(parents=True, exist_ok=True)
                db_path.with_suffix(".dbtype").write_text("0\n", encoding="utf-8")
                return installer.subprocess.CompletedProcess(command, 0, "", "")
            return installer.subprocess.CompletedProcess(command, 1, "", "Cannot fit databases into 12G. Please use a computer with more main memory.")

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
        (store / "indexes/mmseqs").mkdir(parents=True)
        (store / "indexes/foldseek").mkdir(parents=True)
        (store / "indexes/mmseqs/mmseqs_target_databases.tsv").write_text(
            "source_id\tsource_path\tdb_path\tstatus\tnotes\nmetagenome\tdownloads/metagenome/mgy.fa.gz\tindexes/mmseqs/metagenome/target\tready\talready_available\n",
            encoding="utf-8",
        )
        (store / "indexes/foldseek/foldseek_target_databases.tsv").write_text(
            "source_id\tsource_path\tdb_path\tstatus\tnotes\nrcsb_mmcif\tdownloads/rcsb/mmcif\tindexes/foldseek/rcsb_mmcif/target\tready\talready_available\n",
            encoding="utf-8",
        )
        require(installer.existing_science_indexes_ready(store), "ready installed indexes must be reused")
        (store / "indexes/mmseqs/mmseqs_target_databases.tsv").write_text(
            "source_id\tsource_path\tdb_path\tstatus\tnotes\nmetagenome\tdownloads/metagenome/mgy.fa.gz\tindexes/mmseqs/metagenome/target\tdeferred_large_source\tsource_exceeds_sync_index_threshold\n",
            encoding="utf-8",
        )
        require(installer.base_science_indexes_ready(store), "base indexes should be reusable while repairing alignment databases")
        require(not installer.existing_science_indexes_ready(store), "deferred persistent alignment databases must not be reused as ready")
        (store / "indexes/mmseqs/mmseqs_target_databases.tsv").write_text(
            "source_id\tsource_path\tdb_path\tstatus\tnotes\nmetagenome\tdownloads/metagenome/mgy.fa.gz\tindexes/mmseqs/metagenome/target\tready\talready_available\n",
            encoding="utf-8",
        )
        (store / "downloads/metagenome").mkdir(parents=True)
        (store / "downloads/metagenome/mgy.fa.gz").write_bytes(b"x" * 2048)
        old_partial_limit = installer.os.environ.get("GENOLIGAND_PARTIAL_INDEX_LARGE_SOURCE_BYTES")
        installer.os.environ["GENOLIGAND_PARTIAL_INDEX_LARGE_SOURCE_BYTES"] = "1024"
        try:
            (store / "index_build_manifest.tsv").write_text(
                "index\tinput_path\tstatus\trows\toutput_path\tnotes\nmetagenome_targets\tdownloads/metagenome/mgy.fa.gz\tused\t5000\tindexes/metagenome_targets.tsv\tfasta_sequence_rows\n",
                encoding="utf-8",
            )
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
