from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


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

    print("install_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
