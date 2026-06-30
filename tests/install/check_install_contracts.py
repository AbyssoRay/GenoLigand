from __future__ import annotations

import sys
import importlib.util
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
    installer = load_installer_module()
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
        }
    )
    text = str(summary)
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
