from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Sequence


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def default_science_store() -> Path:
    explicit = os.environ.get("GENOLIGAND_SCIENCE_STORE")
    if explicit:
        return Path(explicit)
    data_root = os.environ.get("GENOLIGAND_DATA_ROOT")
    if data_root:
        return Path(data_root) / "science_store"
    if os.name == "nt":
        d_root = Path("D:/GenoLigandData/science_store")
        if Path("D:/").exists():
            return d_root
        local = os.environ.get("LOCALAPPDATA")
        if local:
            return Path(local) / "GenoLigand" / "science_store"
    xdg_data = os.environ.get("XDG_DATA_HOME")
    if xdg_data:
        return Path(xdg_data) / "genoligand" / "science_store"
    home = Path.home()
    return home / ".local" / "share" / "genoligand" / "science_store"


def default_runtime_dir() -> Path:
    explicit = os.environ.get("GENOLIGAND_RUNTIME_DIR")
    if explicit:
        return Path(explicit)
    if os.name == "nt":
        local = os.environ.get("LOCALAPPDATA")
        if local:
            return Path(local) / "GenoLigand" / "runtime"
    xdg_state = os.environ.get("XDG_STATE_HOME")
    if xdg_state:
        return Path(xdg_state) / "genoligand" / "runtime"
    return Path.home() / ".local" / "state" / "genoligand" / "runtime"


def command_from_string(value: str) -> list[str]:
    import shlex

    return shlex.split(value)


def resolve_binary_command(
    explicit: str | None,
    env_name: str,
    binary_name: str,
    cargo_package: str,
    root: Path,
) -> list[str]:
    if explicit:
        return command_from_string(explicit)
    env_value = os.environ.get(env_name)
    if env_value:
        return command_from_string(env_value)
    if (root / "Cargo.toml").exists():
        return ["cargo", "run", "-p", cargo_package, "--"]
    executable = f"{binary_name}.exe" if os.name == "nt" else binary_name
    built = root / "target" / "debug" / executable
    if built.exists():
        return [str(built)]
    on_path = shutil.which(binary_name)
    if on_path:
        return [on_path]
    return ["cargo", "run", "-p", cargo_package, "--"]


def runtime_backend() -> str:
    if os.name == "nt":
        return "windows_wsl2_capable"
    if "microsoft" in platform.release().lower():
        return "wsl2_linux"
    return "native_linux"


def run_command(command: Sequence[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(command),
        check=False,
        text=True,
        capture_output=True,
        env=env,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Prepare or validate the GenoLigand local science store."
    )
    parser.add_argument("--workspace-root", default=str(repo_root()))
    parser.add_argument("--science-store", default=str(default_science_store()))
    parser.add_argument("--runtime-dir", default=str(default_runtime_dir()))
    parser.add_argument("--gls-cli")
    parser.add_argument("--gls-server")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default="19090")
    parser.add_argument("--start-webui", action="store_true")
    args = parser.parse_args()

    workspace_root = Path(args.workspace_root).resolve()
    science_store = Path(args.science_store).resolve()
    runtime_dir = Path(args.runtime_dir).resolve()
    cli_command = resolve_binary_command(
        args.gls_cli,
        "GENOLIGAND_GLS_CLI",
        "gls-cli",
        "gls-cli",
        workspace_root,
    )
    command = cli_command + [
        "install",
        "prepare-science-store",
        "--workspace-root",
        str(workspace_root),
        "--science-store",
        str(science_store),
    ]
    completed = run_command(command)
    payload = {
        "status": "prepared" if completed.returncode == 0 else "failed",
        "command": command,
        "returncode": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
        "runtime_backend": runtime_backend(),
        "workspace_root": str(workspace_root),
        "science_store": str(science_store),
        "runtime_dir": str(runtime_dir),
        "access_url": f"http://{args.host}:{args.port}/",
    }
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    if completed.returncode != 0 or not args.start_webui:
        return completed.returncode

    runtime_dir.mkdir(parents=True, exist_ok=True)
    server_command = resolve_binary_command(
        args.gls_server,
        "GENOLIGAND_GLS_SERVER",
        "gls-server",
        "gls-server",
        workspace_root,
    )
    server_command += [
        "--host",
        args.host,
        "--port",
        str(args.port),
        "--artifact-dir",
        str(runtime_dir),
        "--workspace-root",
        str(workspace_root),
    ]
    env = os.environ.copy()
    env["GENOLIGAND_SCIENCE_STORE"] = str(science_store)
    print(
        json.dumps(
            {
                "status": "starting_webui",
                "command": server_command,
                "access_url": f"http://{args.host}:{args.port}/",
                "science_store": str(science_store),
                "runtime_dir": str(runtime_dir),
            },
            ensure_ascii=False,
            indent=2,
        )
    )
    return subprocess.call(server_command, env=env)


if __name__ == "__main__":
    raise SystemExit(main())
