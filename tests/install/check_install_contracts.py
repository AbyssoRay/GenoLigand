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
    require("deferred_large_source" in installer_text, "installer must record oversized alignment sources without blocking first launch")
    require("GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES" in installer_text, "installer must allow explicit full alignment database indexing")
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
        calls: list[tuple[str, list[list[str]], str]] = []
        original_runner = installer.run_wsl_database_commands
        original_os_name = installer.os.name
        installer.run_wsl_database_commands = lambda distro, commands, success_note: calls.append((distro, commands, success_note)) or ("ready", success_note)
        installer.os.name = "nt"
        try:
            status, note = installer.build_mmseqs_database(None, source, tmp_path / "mmseqs" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "mmseqs_createdb_createindex_completed", "WSL MMseqs2 build must not require a Windows PATH executable")
            status, note = installer.build_foldseek_database(None, source, tmp_path / "foldseek" / "target", {}, "GenoLigandPhase08")
            require(status == "ready" and note == "foldseek_createdb_createindex_completed", "WSL Foldseek build must not require a Windows PATH executable")
            require(len(calls) == 2 and all(call[0] == "GenoLigandPhase08" for call in calls), "WSL database builds must target the selected project distro")
        finally:
            installer.run_wsl_database_commands = original_runner
            installer.os.name = original_os_name
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
        require(installer.existing_science_indexes_ready(store), "ready installed indexes must be reused")
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
        old_limit = installer.os.environ.get("GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES")
        installer.os.environ["GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES"] = "1024"
        try:
            require(installer.alignment_source_deferred(source), "oversized alignment source must be deferred by the installer")
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
            "alignment_databases": {"status": "ready", "ready": 2, "deferred": 1, "total": 3},
        }
    )
    text = str(summary)
    require(summary["alignment_databases"]["deferred"] == 1, "install summary must report deferred large alignment sources")
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
