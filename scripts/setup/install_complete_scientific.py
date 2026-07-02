from __future__ import annotations

import argparse
import concurrent.futures
import csv
import hashlib
import json
import os
import platform
import re
import shutil
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Callable, Sequence

PYTHON_TOOL_PACKAGES: list[dict[str, str]] = [
    {
        "component": "rdkit",
        "package": "rdkit",
        "import_name": "rdkit",
        "version_expr": "rdkit.__version__",
    },
    {
        "component": "openmm",
        "package": "openmm",
        "import_name": "openmm",
        "version_expr": "openmm.version.version",
    },
    {
        "component": "openmmforcefields",
        "package": "openmmforcefields",
        "import_name": "openmmforcefields",
        "version_expr": "'installed'",
    },
    {
        "component": "pdbfixer",
        "package": "git+https://github.com/openmm/pdbfixer.git",
        "import_name": "pdbfixer",
        "version_expr": "'installed'",
    },
    {
        "component": "openff_toolkit",
        "package": "openff-toolkit",
        "import_name": "openff.toolkit",
        "version_expr": "'installed'",
    },
    {
        "component": "openff_interchange",
        "package": "git+https://github.com/openforcefield/openff-interchange.git",
        "import_name": "openff.interchange",
        "version_expr": "openff.interchange.__version__",
    },
    {
        "component": "openff_utilities",
        "package": "git+https://github.com/openforcefield/openff-utilities.git",
        "import_name": "openff.utilities",
        "version_expr": "openff.utilities.__version__",
    },
    {
        "component": "openff_units",
        "package": "git+https://github.com/openforcefield/openff-units.git",
        "import_name": "openff.units",
        "version_expr": "openff.units.__version__",
    },
    {
        "component": "xmltodict",
        "package": "xmltodict",
        "import_name": "xmltodict",
        "version_expr": "getattr(xmltodict, '__version__', 'installed')",
    },
    {
        "component": "python_constraint",
        "package": "python-constraint",
        "import_name": "constraint",
        "version_expr": "'installed'",
    },
    {
        "component": "lxml",
        "package": "lxml",
        "import_name": "lxml",
        "version_expr": "'installed'",
    },
    {
        "component": "cachetools",
        "package": "cachetools",
        "import_name": "cachetools",
        "version_expr": "cachetools.__version__",
    },
    {
        "component": "networkx",
        "package": "networkx",
        "import_name": "networkx",
        "version_expr": "networkx.__version__",
    },
    {
        "component": "pydantic",
        "package": "pydantic",
        "import_name": "pydantic",
        "version_expr": "pydantic.__version__",
    },
    {
        "component": "mdtraj",
        "package": "mdtraj",
        "import_name": "mdtraj",
        "version_expr": "mdtraj.__version__",
    },
    {
        "component": "mdanalysis",
        "package": "MDAnalysis",
        "import_name": "MDAnalysis",
        "version_expr": "MDAnalysis.__version__",
    },
    {
        "component": "openbabel",
        "package": "openbabel-wheel",
        "import_name": "openbabel",
        "version_expr": "'installed'",
    },
]

OPENMM_CUDA_PIP_PACKAGE = "openmm-cuda-12"
OPENFF_FORCEFIELD_FILE = "openff-2.2.1.offxml"
OPENFF_FORCEFIELD_URL = (
    "https://raw.githubusercontent.com/openforcefield/openff-forcefields/main/"
    "openforcefields/offxml/openff-2.2.1.offxml"
)
DEFAULT_ALIGNMENT_DB_MAX_SOURCE_BYTES = 5 * 1024 * 1024 * 1024
RESERVED_WSL_DISTROS = {"Ubuntu-24.04"}
DOWNLOAD_RETRIES = 3
DOWNLOAD_TIMEOUT_SECONDS = 120
DOWNLOAD_USER_AGENT = "GenoLigand-science-store-installer/0.9"
RCSB_DOWNLOAD_WORKERS = 20
ProgressCallback = Callable[[str, int, int | None, str], None]


def urlopen_with_compatible_tls(request: urllib.request.Request, timeout: int):
    url = getattr(request, "full_url", "")
    if str(url).lower().startswith("https://"):
        context = ssl.create_default_context()
        ignore_unexpected_eof = getattr(ssl, "OP_IGNORE_UNEXPECTED_EOF", None)
        if ignore_unexpected_eof is not None:
            context.options |= ignore_unexpected_eof
        return urllib.request.urlopen(request, timeout=timeout, context=context)
    return urllib.request.urlopen(request, timeout=timeout)

WSL_COMMAND_WRAPPERS = [
    "java",
    "nextflow",
    "mmseqs",
    "foldseek",
    "vina",
    "obabel",
    "rsync",
]

COMMAND_TOOL_SPECS: list[dict[str, object]] = [
    {
        "component": "java",
        "command": "java",
        "args": ["-version"],
        "apt": ["default-jre"],
        "dnf": ["java-17-openjdk"],
        "yum": ["java-17-openjdk"],
        "pacman": ["jre-openjdk"],
    },
    {
        "component": "nextflow",
        "command": "nextflow",
        "args": ["-version"],
        "apt": [],
        "dnf": [],
        "yum": [],
        "pacman": [],
    },
    {
        "component": "openbabel",
        "command": "obabel",
        "args": ["-V"],
        "apt": ["openbabel"],
        "dnf": ["openbabel"],
        "yum": ["openbabel"],
        "pacman": ["openbabel"],
    },
    {
        "component": "mmseqs2",
        "command": "mmseqs",
        "args": ["version"],
        "apt": ["mmseqs2"],
        "dnf": ["mmseqs2"],
        "yum": ["mmseqs2"],
        "pacman": ["mmseqs2"],
    },
    {
        "component": "foldseek",
        "command": "foldseek",
        "args": ["version"],
        "apt": ["foldseek"],
        "dnf": ["foldseek"],
        "yum": ["foldseek"],
        "pacman": ["foldseek"],
    },
    {
        "component": "autodock_vina",
        "command": "vina",
        "args": ["--version"],
        "apt": ["autodock-vina"],
        "dnf": ["autodock-vina"],
        "yum": ["autodock-vina"],
        "pacman": ["autodock-vina"],
    },
]


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def configure_stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if callable(reconfigure):
            reconfigure(encoding="utf-8", errors="replace")


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


def default_python_env_dir(runtime_dir: Path) -> Path:
    explicit = os.environ.get("GENOLIGAND_PYTHON_ENV")
    if explicit:
        return Path(explicit)
    return runtime_dir / "python-worker-env"


def command_from_string(value: str) -> list[str]:
    import shlex

    text = value.strip()
    unquoted = text.strip('"')
    if unquoted and Path(unquoted).exists():
        return [unquoted]
    if os.name == "nt":
        return [part.strip('"') for part in shlex.split(text, posix=False)]
    return shlex.split(text)


def host_toolchain_path_entries() -> list[str]:
    entries: list[Path] = []
    if os.name == "nt":
        user_profile = os.environ.get("USERPROFILE")
        if user_profile:
            entries.append(Path(user_profile) / ".cargo" / "bin")
    else:
        entries.append(Path.home() / ".cargo" / "bin")
    return [str(path) for path in entries if path.exists()]


def prepend_process_path(entries: Sequence[str]) -> None:
    existing = os.environ.get("PATH", "")
    clean_entries = [entry for entry in entries if entry]
    if clean_entries:
        os.environ["PATH"] = os.pathsep.join([*clean_entries, existing])


def run_package_install(command: Sequence[str]) -> bool:
    completed = run_command(command)
    return completed.returncode == 0


def ensure_cargo_available() -> str:
    prepend_process_path(host_toolchain_path_entries())
    cargo = shutil.which("cargo")
    if cargo:
        return cargo
    if os.name == "nt":
        winget = shutil.which("winget")
        if winget:
            run_package_install(
                [
                    winget,
                    "install",
                    "--id",
                    "Rustlang.Rustup",
                    "--exact",
                    "--source",
                    "winget",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ]
            )
            prepend_process_path(host_toolchain_path_entries())
            cargo = shutil.which("cargo")
            if cargo:
                return cargo
        return ""
    sudo = [] if os.geteuid() == 0 else ["sudo"]
    installers = [
        (["apt-get"], [*sudo, "apt-get", "update"], [*sudo, "apt-get", "install", "-y", "cargo", "rustc", "build-essential", "pkg-config"]),
        (["dnf"], [], [*sudo, "dnf", "install", "-y", "cargo", "rust", "gcc", "pkg-config"]),
        (["yum"], [], [*sudo, "yum", "install", "-y", "cargo", "rust", "gcc", "pkg-config"]),
        (["pacman"], [], [*sudo, "pacman", "-Sy", "--noconfirm", "rust", "gcc", "pkgconf"]),
    ]
    for probes, update, install in installers:
        if shutil.which(probes[0]):
            if update:
                run_package_install(update)
            run_package_install(install)
            cargo = shutil.which("cargo")
            if cargo:
                return cargo
    return ""


def build_release_binary(root: Path, binary_name: str, cargo_package: str) -> Path | None:
    if not (root / "Cargo.toml").exists():
        return None
    cargo = ensure_cargo_available()
    if not cargo:
        return None
    executable = f"{binary_name}.exe" if os.name == "nt" else binary_name
    binary = root / "target" / "release" / executable
    completed = run_command([cargo, "build", "--release", "-p", cargo_package])
    if completed.returncode == 0 and binary.exists():
        return binary
    return binary if binary.exists() else None


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
    executable = f"{binary_name}.exe" if os.name == "nt" else binary_name
    for candidate in [
        root / "bin" / executable,
        root / "tools" / "bin" / executable,
    ]:
        if candidate.exists():
            return [str(candidate)]
    built = build_release_binary(root, binary_name, cargo_package)
    if built:
        return [str(built)]
    release_binary = root / "target" / "release" / executable
    if release_binary.exists():
        return [str(release_binary)]
    if os.environ.get("GENOLIGAND_ALLOW_DEBUG_BINARIES") == "1":
        debug_binary = root / "target" / "debug" / executable
        if debug_binary.exists():
            return [str(debug_binary)]
    on_path = shutil.which(binary_name)
    if on_path:
        return [on_path]
    return [binary_name]


def runtime_executable_name(binary_name: str) -> str:
    return f"{binary_name}.exe" if os.name == "nt" else binary_name


def stop_runtime_binary_process(binary_name: str) -> dict[str, object]:
    executable = runtime_executable_name(binary_name)
    if os.name == "nt":
        command = ["taskkill", "/F", "/IM", executable]
    else:
        command = ["pkill", "-f", executable]
    completed = subprocess.run(command, capture_output=True, text=True, check=False)
    return {
        "command": command,
        "returncode": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
    }


def copy_runtime_binary(source: Path, target: Path, binary_name: str) -> dict[str, object] | None:
    try:
        shutil.copy2(source, target)
        return None
    except PermissionError:
        stop_result = stop_runtime_binary_process(binary_name)
        time.sleep(1)
        shutil.copy2(source, target)
        return stop_result


def publish_runtime_binary(command: list[str], runtime_dir: Path, binary_name: str) -> tuple[list[str], dict[str, object]]:
    bin_dir = runtime_dir / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    executable = runtime_executable_name(binary_name)
    status: dict[str, object] = {
        "binary": binary_name,
        "bin_dir": str(bin_dir),
        "published": False,
        "command": command,
    }
    if not command:
        status["status"] = "missing_command"
        return command, status
    source = Path(command[0])
    if len(command) == 1 and source.exists() and source.is_file():
        target = bin_dir / executable
        stop_result = None
        if source.resolve() != target.resolve():
            stop_result = copy_runtime_binary(source, target, binary_name)
        try:
            target.chmod(target.stat().st_mode | 0o755)
        except OSError:
            pass
        published = [str(target)]
        status.update({"status": "ready", "published": True, "path": str(target), "command": published})
        if stop_result:
            status["stopped_locked_process"] = stop_result
        return published, status
    status["status"] = "external_command"
    return command, status


def command_env_value(command: list[str]) -> str:
    if len(command) == 1:
        return command[0]
    return subprocess.list2cmdline(command) if os.name == "nt" else " ".join(shell_quote(part) for part in command)


def shell_quote(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"


def write_runtime_launch_environment(
    runtime_dir: Path,
    science_store: Path,
    workspace_root: Path,
    host: str,
    port: str,
    cli_command: list[str],
    server_command: list[str],
    tool_python: str,
    science_env: dict[str, str],
) -> dict[str, object]:
    runtime_dir.mkdir(parents=True, exist_ok=True)
    env = {
        "GENOLIGAND_SCIENCE_STORE": str(science_store),
        "GENOLIGAND_GLS_CLI": command_env_value(cli_command),
        "GENOLIGAND_GLS_SERVER": command_env_value(server_command[:1]),
    }
    if tool_python:
        env["GENOLIGAND_PYTHON"] = tool_python
    path_prepend = [str(runtime_dir / "bin")]
    science_path = science_env.get("PATH", "")
    if science_path:
        path_prepend.append(science_path)
    launch_json = runtime_dir / "launch_environment.json"
    launch_payload = {
        "status": "ready",
        "runtime_dir": str(runtime_dir),
        "workspace_root": str(workspace_root),
        "science_store": str(science_store),
        "host": host,
        "port": port,
        "environment": env,
        "path_prepend": path_prepend,
        "server_command": server_command,
    }
    launch_json.write_text(json.dumps(launch_payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if os.name == "nt":
        launcher = runtime_dir / "start-genoligand-webui.cmd"
        lines = ["@echo off", "setlocal"]
        for key, value in env.items():
            lines.append(f'set "{key}={value}"')
        lines.append(f'set "PATH={";".join(path_prepend)};%PATH%"')
        lines.append(subprocess.list2cmdline(server_command))
        launcher.write_text("\r\n".join(lines) + "\r\n", encoding="utf-8")
    else:
        launcher = runtime_dir / "start-genoligand-webui.sh"
        lines = ["#!/usr/bin/env sh", "set -eu"]
        for key, value in env.items():
            lines.append(f"export {key}={shell_quote(value)}")
        lines.append(f"export PATH={shell_quote(':'.join(path_prepend))}:$PATH")
        lines.append(" ".join(shell_quote(part) for part in server_command))
        launcher.write_text("\n".join(lines) + "\n", encoding="utf-8")
        launcher.chmod(0o755)
    return {
        "status": "ready",
        "launcher": str(launcher),
        "environment": str(launch_json),
        "runtime_dir": str(runtime_dir),
        "bin_dir": str(runtime_dir / "bin"),
    }


def runtime_backend() -> str:
    if os.name == "nt":
        return "windows_wsl2_capable"
    if "microsoft" in platform.release().lower():
        return "wsl2_linux"
    return "native_linux"


def tool_path_entries(workspace_root: Path, extra_entries: Sequence[Path] | None = None) -> list[str]:
    entries: list[Path] = [
        workspace_root / "bin",
        workspace_root / "tools" / "bin",
    ]
    if extra_entries:
        entries.extend(extra_entries)
    explicit = os.environ.get("GENOLIGAND_TOOL_ROOT")
    if explicit:
        root = Path(explicit)
        entries.extend([root, root / "bin"])
    if os.name == "nt":
        entries.extend(
            [
                Path("D:/user/codex/tools"),
                Path("D:/user/codex/tools/bin"),
                Path("D:/GenoLigandTools/bin"),
            ]
        )
    return [str(path) for path in entries if path.exists()]


def cuda_toolkit_candidates() -> list[Path]:
    candidates: list[Path] = []
    for key in ("CUDA_PATH", "CUDA_HOME", "CUDA_ROOT"):
        value = os.environ.get(key)
        if value:
            candidates.append(Path(value))
    if os.name == "nt":
        candidates.append(Path("D:/Program Files/CUDA"))
        program_files = os.environ.get("ProgramFiles")
        if program_files:
            candidates.append(Path(program_files) / "NVIDIA GPU Computing Toolkit" / "CUDA")
    return candidates


def detected_cuda_toolkit() -> Path | None:
    for path in cuda_toolkit_candidates():
        nvcc = path / "bin" / ("nvcc.exe" if os.name == "nt" else "nvcc")
        if path.is_dir() and (nvcc.is_file() or (path / "version.json").is_file()):
            return path
    return None


def cuda_toolkit_summary() -> dict[str, object]:
    path = detected_cuda_toolkit()
    if not path:
        return {
            "status": "missing",
            "path": "",
            "nvcc": "missing",
            "notes": "CUDA Toolkit path not detected",
        }
    nvcc = path / "bin" / ("nvcc.exe" if os.name == "nt" else "nvcc")
    return {
        "status": "ready",
        "path": str(path),
        "nvcc": "present" if nvcc.is_file() else "missing",
        "notes": "CUDA Toolkit path detected",
    }


def environment_for_science_tools(
    workspace_root: Path,
    extra_entries: Sequence[Path] | None = None,
) -> dict[str, str]:
    env = os.environ.copy()
    entries = tool_path_entries(workspace_root, extra_entries)
    cuda = detected_cuda_toolkit()
    if cuda:
        cuda_bin = cuda / "bin"
        if cuda_bin.exists():
            entries.append(str(cuda_bin))
        env.setdefault("CUDA_PATH", str(cuda))
        env.setdefault("CUDA_HOME", str(cuda))
    if entries:
        env["PATH"] = os.pathsep.join([*entries, env.get("PATH", "")])
    return env


def run_command(command: Sequence[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    command_list = list(command)
    try:
        return subprocess.run(
            command_list,
            check=False,
            text=True,
            capture_output=True,
            env=env,
        )
    except FileNotFoundError as err:
        return subprocess.CompletedProcess(
            command_list,
            127,
            "",
            f"command_not_found:{err}",
        )


def run_command_bytes(command: Sequence[str]) -> subprocess.CompletedProcess[bytes]:
    command_list = list(command)
    try:
        return subprocess.run(
            command_list,
            check=False,
            capture_output=True,
        )
    except FileNotFoundError as err:
        return subprocess.CompletedProcess(
            command_list,
            127,
            b"",
            f"command_not_found:{err}".encode("utf-8"),
        )


def decode_process_bytes(data: bytes) -> str:
    if not data:
        return ""
    null_ratio = data.count(b"\x00") / max(1, len(data))
    encodings = ("utf-16le", "utf-8", "gbk") if null_ratio > 0.1 else ("utf-8", "gbk", "utf-16le")
    for encoding in encodings:
        try:
            text = data.decode(encoding)
        except UnicodeDecodeError:
            continue
        if text.strip():
            return text.replace("\x00", "")
    return data.decode("utf-8", errors="replace").replace("\x00", "")


def list_wsl_distributions() -> list[str]:
    if os.name != "nt":
        return []
    completed = run_command_bytes(["wsl.exe", "-l", "-q"])
    text = decode_process_bytes(completed.stdout)
    names = []
    for line in text.splitlines():
        name = line.replace("\x00", "").strip().lstrip("*").strip()
        if name and name.upper() != "NAME":
            names.append(name)
    return names


def selected_project_wsl_distro(explicit: str | None) -> str | None:
    if os.name != "nt":
        return None
    names = list_wsl_distributions()
    requested = explicit or os.environ.get("GENOLIGAND_WSL_DISTRO", "")
    if requested:
        return requested if requested in names and requested not in RESERVED_WSL_DISTROS else None
    for name in names:
        if name not in RESERVED_WSL_DISTROS and name.lower().startswith("genoligand"):
            return name
    return None


def wsl_home(distro: str) -> str:
    completed = run_command_bytes(["wsl.exe", "-d", distro, "--", "bash", "-lc", "printf %s \"$HOME\""])
    if completed.returncode != 0:
        return ""
    return decode_process_bytes(completed.stdout).strip()


def run_wsl_command(distro: str, command: Sequence[str]) -> subprocess.CompletedProcess[str]:
    completed = run_command_bytes(["wsl.exe", "-d", distro, "--", *command])
    return subprocess.CompletedProcess(
        completed.args,
        completed.returncode,
        decode_process_bytes(completed.stdout),
        decode_process_bytes(completed.stderr),
    )


def run_wsl_root_shell(distro: str, script: str) -> subprocess.CompletedProcess[str]:
    completed = run_command_bytes(["wsl.exe", "-d", distro, "--user", "root", "--", "bash", "-lc", script])
    return subprocess.CompletedProcess(
        completed.args,
        completed.returncode,
        decode_process_bytes(completed.stdout),
        decode_process_bytes(completed.stderr),
    )


def run_wsl_user_shell(distro: str, script: str) -> subprocess.CompletedProcess[str]:
    completed = run_command_bytes(["wsl.exe", "-d", distro, "--", "bash", "-lc", script])
    return subprocess.CompletedProcess(
        completed.args,
        completed.returncode,
        decode_process_bytes(completed.stdout),
        decode_process_bytes(completed.stderr),
    )


def wsl_runtime_path_prefix() -> str:
    return "$HOME/.local/state/genoligand/python-worker-env/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin"


def write_wsl_python_wrapper(wrapper: Path, distro: str, python_path: str) -> None:
    wrapper.parent.mkdir(parents=True, exist_ok=True)
    safe_distro = distro.replace('"', "")
    safe_python = python_path.replace('"', "")
    wrapper.write_text(
        (
            "@echo off\n"
            f'wsl.exe -d {safe_distro} -- env LD_LIBRARY_PATH=/usr/lib/wsl/lib:/usr/local/cuda/lib64 "{safe_python}" %*\n'
        ),
        encoding="utf-8",
    )


def write_wsl_command_wrappers(wrapper_dir: Path, distro: str) -> dict[str, object]:
    wrapper_dir.mkdir(parents=True, exist_ok=True)
    safe_distro = distro.replace('"', "")
    wrappers = []
    for command_name in WSL_COMMAND_WRAPPERS:
        wrapper = wrapper_dir / f"{command_name}.cmd"
        wrapper.write_text(
            (
                "@echo off\n"
                f'wsl.exe -d {safe_distro} -- bash -lc '
                f'"export PATH=\\"{wsl_runtime_path_prefix()}:$PATH\\"; exec {command_name} %*"\n'
            ),
            encoding="utf-8",
        )
        wrappers.append(command_name)
    return {
        "status": "ready",
        "backend": "wsl",
        "wsl_distro": distro,
        "wrapper_dir": str(wrapper_dir),
        "wrappers": wrappers,
    }


def venv_python(env_dir: Path) -> Path:
    if os.name == "nt":
        return env_dir / "Scripts" / "python.exe"
    return env_dir / "bin" / "python"


def probe_python_import(python: Path, import_name: str, version_expr: str) -> dict[str, object]:
    code = f"import {import_name}; print({version_expr})"
    completed = run_command([str(python), "-c", code])
    text = f"{completed.stdout}{completed.stderr}".strip()
    if completed.returncode == 0:
        version = next((line.strip() for line in text.splitlines() if line.strip()), "installed")
        return {"ready": True, "version": version, "notes": "import_ok"}
    return {
        "ready": False,
        "version": "",
        "notes": next((line.strip() for line in text.splitlines() if line.strip()), "import_failed"),
    }


def python_tool_total() -> int:
    return len(PYTHON_TOOL_PACKAGES) + 1


OPENMM_CUDA_PROBE_CODE = r"""
import json
import os
import pathlib
import site

payload = {
    "openmm_ready": False,
    "version": "",
    "platforms": [],
    "cuda_ready": False,
    "plugin_dir": "",
    "failure_reason": "",
}
try:
    import openmm

    payload["openmm_ready"] = True
    payload["version"] = getattr(openmm.version, "version", getattr(openmm, "__version__", "installed"))
    payload["platforms"] = [
        openmm.Platform.getPlatform(index).getName()
        for index in range(openmm.Platform.getNumPlatforms())
    ]
    payload["cuda_ready"] = any(name.lower() == "cuda" for name in payload["platforms"])
    roots = []
    try:
        roots.extend(site.getsitepackages())
    except Exception:
        pass
    try:
        roots.append(str(pathlib.Path(openmm.__file__).resolve().parent.parent))
    except Exception:
        pass
    for root in roots:
        candidate = pathlib.Path(root) / "OpenMM.libs" / "lib" / "plugins"
        if candidate.exists():
            payload["plugin_dir"] = str(candidate)
            break
except Exception as exc:
    payload["failure_reason"] = f"openmm_probe_failed:{exc}"
print(json.dumps(payload, sort_keys=True))
"""


def parse_openmm_cuda_probe(completed: subprocess.CompletedProcess[str]) -> dict[str, object]:
    text = f"{completed.stdout}{completed.stderr}".strip()
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict):
            return payload
    return {
        "openmm_ready": False,
        "version": "",
        "platforms": [],
        "cuda_ready": False,
        "plugin_dir": "",
        "failure_reason": next((line.strip() for line in text.splitlines() if line.strip()), "openmm_cuda_probe_failed"),
    }


def probe_local_openmm_cuda(python: Path) -> dict[str, object]:
    return parse_openmm_cuda_probe(run_command([str(python), "-c", OPENMM_CUDA_PROBE_CODE]))


def probe_wsl_openmm_cuda(distro: str, python_path: str) -> dict[str, object]:
    return parse_openmm_cuda_probe(run_wsl_command(distro, [python_path, "-c", OPENMM_CUDA_PROBE_CODE]))


def openmm_cuda_notes(
    probe: dict[str, object],
    repair_action: str,
    runtime_notes: str = "",
) -> str:
    platforms = ",".join(str(item) for item in probe.get("platforms", []) if str(item))
    cuda = "ready" if probe.get("cuda_ready") else "missing"
    reason = "ready" if probe.get("cuda_ready") else str(probe.get("failure_reason") or "openmm_platform_list_does_not_include_CUDA")
    parts = [
        f"platforms={platforms}",
        f"cuda={cuda}",
        f"cuda_failure_reason={reason}",
        f"openmm_plugin_dir={probe.get('plugin_dir', '')}",
        f"repair_action={repair_action}",
    ]
    if runtime_notes:
        parts.append(runtime_notes)
    return ";".join(parts)


def openmm_cuda_manifest_row(
    python: str,
    probe: dict[str, object],
    repair_action: str,
    runtime_notes: str = "",
) -> dict[str, object]:
    ready = bool(probe.get("cuda_ready"))
    status = "ready" if ready else "missing"
    if not probe.get("openmm_ready"):
        status = "failed"
    return {
        "component": "openmm_cuda",
        "status": status,
        "python": python,
        "package": OPENMM_CUDA_PIP_PACKAGE,
        "import_name": "openmm",
        "version": str(probe.get("version") or ""),
        "notes": openmm_cuda_notes(probe, repair_action, runtime_notes),
    }


def ensure_local_openmm_cuda(python: Path, skip_install: bool) -> dict[str, object]:
    probe = probe_local_openmm_cuda(python)
    repair_action = "not_needed" if probe.get("cuda_ready") else "pending"
    if not probe.get("cuda_ready") and not skip_install:
        installed = run_command([str(python), "-m", "pip", "install", OPENMM_CUDA_PIP_PACKAGE])
        repair_action = "pip_install_openmm_cuda_12_succeeded" if installed.returncode == 0 else "pip_install_openmm_cuda_12_failed"
        probe = probe_local_openmm_cuda(python)
        if not probe.get("cuda_ready") and installed.returncode != 0:
            probe["failure_reason"] = installed.stderr.strip() or installed.stdout.strip() or probe.get("failure_reason", "")
    elif not probe.get("cuda_ready"):
        repair_action = "tool_install_skipped"
    return openmm_cuda_manifest_row(str(python), probe, repair_action)


def probe_wsl_cuda_runtime(distro: str) -> str:
    code = (
        "import pathlib, shutil, subprocess; "
        "gpu = bool(shutil.which('nvidia-smi')) and subprocess.run(['nvidia-smi','-L'], capture_output=True, text=True).returncode == 0; "
        "libcuda = pathlib.Path('/usr/lib/wsl/lib/libcuda.so').exists() or pathlib.Path('/usr/lib/wsl/lib/libcuda.so.1').exists(); "
        "print('wsl_gpu=' + str(gpu).lower() + ';wsl_driver=' + str(gpu).lower() + ';wsl_libcuda=' + str(libcuda).lower())"
    )
    completed = run_wsl_command(distro, ["python3", "-c", code])
    return (completed.stdout or completed.stderr or "wsl_cuda_probe_failed").strip().replace("\n", " ")


def ensure_wsl_openmm_cuda(distro: str, python_path: str, wrapper: Path, skip_install: bool) -> dict[str, object]:
    runtime_notes = probe_wsl_cuda_runtime(distro)
    probe = probe_wsl_openmm_cuda(distro, python_path)
    repair_action = "not_needed" if probe.get("cuda_ready") else "pending"
    if not probe.get("cuda_ready") and not skip_install:
        installed = run_wsl_command(distro, [python_path, "-m", "pip", "install", OPENMM_CUDA_PIP_PACKAGE])
        repair_action = "pip_install_openmm_cuda_12_succeeded" if installed.returncode == 0 else "pip_install_openmm_cuda_12_failed"
        probe = probe_wsl_openmm_cuda(distro, python_path)
        if not probe.get("cuda_ready") and installed.returncode != 0:
            probe["failure_reason"] = installed.stderr.strip() or installed.stdout.strip() or probe.get("failure_reason", "")
    elif not probe.get("cuda_ready"):
        repair_action = "tool_install_skipped"
    return openmm_cuda_manifest_row(str(wrapper), probe, repair_action, runtime_notes)


def ensure_python_tool_environment(
    env_dir: Path,
    skip_install: bool,
    backend: str = "local",
    wsl_distro: str | None = None,
) -> dict[str, object]:
    if backend == "wsl":
        if not wsl_distro:
            return {
                "status": "failed",
                "backend": "wsl",
                "python": "",
                "manifest": str(env_dir / "tool_environment_manifest.tsv"),
                "ready": 0,
                "total": python_tool_total(),
                "environment": "missing_wsl_distribution",
            }
        return ensure_wsl_python_tool_environment(env_dir, wsl_distro, skip_install)
    return ensure_local_python_tool_environment(env_dir, skip_install)


def ensure_local_python_tool_environment(env_dir: Path, skip_install: bool) -> dict[str, object]:
    manifest = env_dir / "tool_environment_manifest.tsv"
    python = venv_python(env_dir)
    env_dir.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, object]] = []
    create_status = "existing" if python.exists() else "pending"
    if skip_install:
        create_status = "skipped"
        python = Path(os.environ.get("GENOLIGAND_PYTHON", sys.executable))
    elif not python.exists():
        created = run_command([sys.executable, "-m", "venv", str(env_dir)])
        if created.returncode != 0:
            write_python_tool_manifest(
                manifest,
                [
                    {
                        "component": "python_worker_env",
                        "status": "failed",
                        "python": str(python),
                        "package": "venv",
                        "import_name": "",
                        "version": "",
                        "notes": created.stderr.strip() or created.stdout.strip() or "venv_create_failed",
                    }
                ],
            )
            return {
                "status": "failed",
                "python": str(python),
                    "manifest": str(manifest),
                    "ready": 0,
                    "total": python_tool_total(),
                }
        create_status = "created"
    if not skip_install:
        run_command([str(python), "-m", "pip", "install", "--upgrade", "pip"])
    ready = 0
    for spec in PYTHON_TOOL_PACKAGES:
        probe = probe_python_import(python, spec["import_name"], spec["version_expr"])
        status = "ready" if probe["ready"] else "missing"
        notes = str(probe["notes"])
        if not probe["ready"] and not skip_install:
            installed = run_command([str(python), "-m", "pip", "install", spec["package"]])
            probe = probe_python_import(python, spec["import_name"], spec["version_expr"])
            status = "ready" if probe["ready"] else "failed"
            notes = (
                str(probe["notes"])
                if probe["ready"]
                else installed.stderr.strip() or installed.stdout.strip() or str(probe["notes"])
            )
        if probe["ready"]:
            ready += 1
        rows.append(
            {
                "component": spec["component"],
                "status": status,
                "python": str(python),
                "package": spec["package"],
                "import_name": spec["import_name"],
                "version": str(probe["version"]),
                "notes": notes,
            }
        )
    cuda_row = ensure_local_openmm_cuda(python, skip_install)
    if cuda_row["status"] == "ready":
        ready += 1
    rows.append(cuda_row)
    write_python_tool_manifest(manifest, rows)
    total = python_tool_total()
    return {
        "status": "ready" if ready == total else "needs_repair" if not skip_install else "skipped",
        "backend": "local",
        "python": str(python),
        "manifest": str(manifest),
        "ready": ready,
        "total": total,
        "environment": create_status,
    }


def ensure_wsl_python_tool_environment(env_dir: Path, distro: str, skip_install: bool) -> dict[str, object]:
    manifest = env_dir / "tool_environment_manifest.tsv"
    wrapper = env_dir / "genoligand-wsl-python.cmd"
    env_dir.mkdir(parents=True, exist_ok=True)
    home = wsl_home(distro)
    if not home:
        write_python_tool_manifest(
            manifest,
            [
                {
                    "component": "python_worker_env",
                    "status": "failed",
                    "python": str(wrapper),
                    "package": "wsl",
                    "import_name": "",
                    "version": "",
                    "notes": "wsl_home_unavailable",
                }
            ],
        )
        return {
            "status": "failed",
            "backend": "wsl",
            "wsl_distro": distro,
            "python": str(wrapper),
                    "manifest": str(manifest),
                    "ready": 0,
                    "total": python_tool_total(),
                    "environment": "wsl_unavailable",
                }
    python_path = f"{home}/.local/state/genoligand/python-worker-env/bin/python"
    write_wsl_python_wrapper(wrapper, distro, python_path)
    python_exists = run_wsl_command(distro, ["bash", "-lc", f"test -x {bash_words([python_path])}"])
    if skip_install and python_exists.returncode != 0:
        rows = [
            {
                "component": spec["component"],
                "status": "skipped",
                "python": str(wrapper),
                "package": spec["package"],
                "import_name": spec["import_name"],
                "version": "",
                "notes": "tool_install_skipped",
            }
            for spec in PYTHON_TOOL_PACKAGES
        ]
        rows.append(
            {
                "component": "openmm_cuda",
                "status": "skipped",
                "python": str(wrapper),
                "package": OPENMM_CUDA_PIP_PACKAGE,
                "import_name": "openmm",
                "version": "",
                "notes": "tool_install_skipped",
            }
        )
        write_python_tool_manifest(manifest, rows)
        return {
            "status": "skipped",
            "backend": "wsl",
            "wsl_distro": distro,
            "python": str(wrapper),
            "manifest": str(manifest),
            "ready": 0,
            "total": python_tool_total(),
            "environment": "skipped",
        }
    if not skip_install:
        run_wsl_root_shell(
            distro,
            (
                "set -u; "
                "if command -v apt-get >/dev/null 2>&1; then "
                "apt-get update; "
                "for pkg in python3 python3-venv python3-pip ca-certificates curl unzip; do apt-get install -y \"$pkg\" || true; done; "
                "elif command -v dnf >/dev/null 2>&1; then "
                "dnf install -y python3 python3-pip ca-certificates curl unzip || true; "
                "elif command -v yum >/dev/null 2>&1; then "
                "yum install -y python3 python3-pip ca-certificates curl unzip || true; "
                "elif command -v pacman >/dev/null 2>&1; then "
                "pacman -Sy --noconfirm python python-pip ca-certificates curl unzip || true; "
                "fi"
            ),
        )
        create = run_wsl_command(
            distro,
            [
                "bash",
                "-lc",
                "if [ ! -x ~/.local/state/genoligand/python-worker-env/bin/python ]; then python3 -m venv ~/.local/state/genoligand/python-worker-env; fi && ~/.local/state/genoligand/python-worker-env/bin/python -m pip install --upgrade pip",
            ],
        )
        if create.returncode != 0:
            write_python_tool_manifest(
                manifest,
                [
                    {
                        "component": "python_worker_env",
                        "status": "failed",
                        "python": str(wrapper),
                        "package": "venv",
                        "import_name": "",
                        "version": "",
                        "notes": create.stderr.strip() or create.stdout.strip() or "wsl_venv_create_failed",
                    }
                ],
            )
            return {
                "status": "failed",
                "backend": "wsl",
                "wsl_distro": distro,
                "python": str(wrapper),
                    "manifest": str(manifest),
                    "ready": 0,
                    "total": python_tool_total(),
                    "environment": "failed",
                }
    ready = 0
    rows: list[dict[str, object]] = []
    for spec in PYTHON_TOOL_PACKAGES:
        probe = probe_wsl_python_import(distro, python_path, spec["import_name"], spec["version_expr"])
        status = "ready" if probe["ready"] else "missing"
        notes = str(probe["notes"])
        if not probe["ready"] and not skip_install:
            installed = run_wsl_command(
                distro,
                [python_path, "-m", "pip", "install", spec["package"]],
            )
            probe = probe_wsl_python_import(distro, python_path, spec["import_name"], spec["version_expr"])
            status = "ready" if probe["ready"] else "failed"
            notes = (
                str(probe["notes"])
                if probe["ready"]
                else installed.stderr.strip() or installed.stdout.strip() or str(probe["notes"])
            )
        if probe["ready"]:
            ready += 1
        rows.append(
            {
                "component": spec["component"],
                "status": status,
                "python": str(wrapper),
                "package": spec["package"],
                "import_name": spec["import_name"],
                "version": str(probe["version"]),
                "notes": notes,
            }
        )
    cuda_row = ensure_wsl_openmm_cuda(distro, python_path, wrapper, skip_install)
    if cuda_row["status"] == "ready":
        ready += 1
    rows.append(cuda_row)
    write_python_tool_manifest(manifest, rows)
    total = python_tool_total()
    return {
        "status": "ready" if ready == total else "needs_repair",
        "backend": "wsl",
        "wsl_distro": distro,
        "python": str(wrapper),
        "manifest": str(manifest),
        "ready": ready,
        "total": total,
        "environment": "created_or_existing",
    }


def probe_wsl_python_import(distro: str, python_path: str, import_name: str, version_expr: str) -> dict[str, object]:
    code = f"import {import_name}; print({version_expr})"
    completed = run_wsl_command(distro, [python_path, "-c", code])
    text = f"{completed.stdout}{completed.stderr}".strip()
    if completed.returncode == 0:
        version = next((line.strip() for line in text.splitlines() if line.strip()), "installed")
        return {"ready": True, "version": version, "notes": "import_ok"}
    return {
        "ready": False,
        "version": "",
        "notes": next((line.strip() for line in text.splitlines() if line.strip()), "import_failed"),
    }


def bash_words(words: Sequence[str]) -> str:
    import shlex

    return " ".join(shlex.quote(word) for word in words)


def command_tool_path_entries(env_dir: Path) -> list[str]:
    entries: list[Path] = []
    if os.name == "nt":
        entries.extend([env_dir / "Scripts", env_dir / "command-wrappers"])
    else:
        entries.extend([env_dir / "bin", Path.home() / ".local" / "bin"])
    return [str(path) for path in entries if path.exists()]


def command_tool_env(workspace_root: Path, env_dir: Path, extra_entries: Sequence[Path] | None = None) -> dict[str, str]:
    entries = [Path(path) for path in command_tool_path_entries(env_dir)]
    if extra_entries:
        entries.extend(extra_entries)
    return environment_for_science_tools(workspace_root, entries)


def probe_local_command_tool(spec: dict[str, object], workspace_root: Path, env_dir: Path) -> dict[str, object]:
    command = str(spec["command"])
    args = [str(arg) for arg in spec.get("args", [])]
    env = command_tool_env(workspace_root, env_dir)
    completed = run_command([command, *args], env=env)
    text = f"{completed.stdout}{completed.stderr}".strip()
    if completed.returncode == 0:
        return {
            "component": spec["component"],
            "status": "ready",
            "command": command,
            "version": next((line.strip() for line in text.splitlines() if line.strip()), "installed"),
            "notes": "command_detected",
        }
    return {
        "component": spec["component"],
        "status": "missing",
        "command": command,
        "version": "",
        "notes": next((line.strip() for line in text.splitlines() if line.strip()), "command_missing"),
    }


def probe_wsl_command_tool(spec: dict[str, object], distro: str) -> dict[str, object]:
    command = str(spec["command"])
    args = [str(arg) for arg in spec.get("args", [])]
    script = f'export PATH="{wsl_runtime_path_prefix()}:$PATH"; {bash_words([command, *args])}'
    completed = run_wsl_user_shell(distro, script)
    text = f"{completed.stdout}{completed.stderr}".strip()
    if completed.returncode == 0:
        return {
            "component": spec["component"],
            "status": "ready",
            "command": command,
            "version": next((line.strip() for line in text.splitlines() if line.strip()), "installed"),
            "notes": "command_detected",
        }
    return {
        "component": spec["component"],
        "status": "missing",
        "command": command,
        "version": "",
        "notes": next((line.strip() for line in text.splitlines() if line.strip()), "command_missing"),
    }


def missing_command_tools(
    rows: Sequence[dict[str, object]],
    specs: Sequence[dict[str, object]] = COMMAND_TOOL_SPECS,
) -> list[dict[str, object]]:
    missing = {str(row.get("component", "")) for row in rows if row.get("status") != "ready"}
    return [spec for spec in specs if str(spec.get("component", "")) in missing]


def write_command_tool_manifest(path: Path, rows: Sequence[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            fieldnames=["component", "status", "command", "version", "notes"],
        )
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {
                    key: str(row.get(key, ""))
                    .replace("\t", " ")
                    .replace("\r", " ")
                    .replace("\n", " ")
                    for key in writer.fieldnames
                }
            )


def package_names_for(specs: Sequence[dict[str, object]], manager: str) -> list[str]:
    packages: list[str] = []
    for spec in specs:
        for package in spec.get(manager, []):
            text = str(package).strip()
            if text and text not in packages:
                packages.append(text)
    return packages


def install_nextflow_wsl(distro: str) -> subprocess.CompletedProcess[str]:
    script = (
        f"set -u; mkdir -p \"$HOME/.local/bin\"; export PATH=\"{wsl_runtime_path_prefix()}:$PATH\"; "
        "if ! command -v nextflow >/dev/null 2>&1; then "
        "curl -fsSL https://get.nextflow.io -o /tmp/genoligand-nextflow-install && "
        "(cd \"$HOME/.local/bin\" && bash /tmp/genoligand-nextflow-install) && "
        "chmod +x \"$HOME/.local/bin/nextflow\"; "
        "fi"
    )
    return run_wsl_user_shell(distro, script)


def install_vina_user_wsl(distro: str) -> subprocess.CompletedProcess[str]:
    python_path = "$HOME/.local/state/genoligand/python-worker-env/bin/python"
    script = (
        f"set -u; export PATH=\"{wsl_runtime_path_prefix()}:$PATH\"; "
        f"if ! command -v vina >/dev/null 2>&1 && [ -x \"{python_path}\" ]; then "
        f"{python_path} -m pip install vina || {python_path} -m pip install --break-system-packages vina || true; "
        "fi"
    )
    return run_wsl_user_shell(distro, script)


def install_wsl_command_tools(distro: str, specs: Sequence[dict[str, object]]) -> str:
    apt_packages = package_names_for(specs, "apt")
    dnf_packages = package_names_for(specs, "dnf")
    yum_packages = package_names_for(specs, "yum")
    pacman_packages = package_names_for(specs, "pacman")
    root_script = (
        "set -u; "
        "if command -v apt-get >/dev/null 2>&1; then "
        "apt-get update; "
        "for pkg in python3 python3-venv python3-pip ca-certificates curl unzip "
        f"{' '.join(apt_packages)}; do apt-get install -y \"$pkg\" || true; done; "
        "elif command -v dnf >/dev/null 2>&1; then "
        f"dnf install -y python3 python3-pip ca-certificates curl unzip {' '.join(dnf_packages)} || true; "
        "elif command -v yum >/dev/null 2>&1; then "
        f"yum install -y python3 python3-pip ca-certificates curl unzip {' '.join(yum_packages)} || true; "
        "elif command -v pacman >/dev/null 2>&1; then "
        f"pacman -Sy --noconfirm python python-pip ca-certificates curl unzip {' '.join(pacman_packages)} || true; "
        "else exit 9; fi"
    )
    root = run_wsl_root_shell(distro, root_script)
    nextflow = install_nextflow_wsl(distro)
    return " | ".join(
        part
        for part in [
            next((line.strip() for line in f"{root.stdout}{root.stderr}".splitlines() if line.strip()), ""),
            next((line.strip() for line in f"{nextflow.stdout}{nextflow.stderr}".splitlines() if line.strip()), ""),
        ]
        if part
    )


def install_local_command_tools(specs: Sequence[dict[str, object]]) -> str:
    if os.name == "nt":
        winget = shutil.which("winget")
        if not winget:
            return "winget_unavailable"
        java = run_package_install(
            [
                winget,
                "install",
                "--id",
                "EclipseAdoptium.Temurin.17.JRE",
                "--exact",
                "--source",
                "winget",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]
        )
        return "winget_java_attempted" if java else "winget_java_failed"
    sudo = [] if os.geteuid() == 0 else ["sudo"]
    attempts: list[str] = []
    for manager, update, install_prefix in [
        ("apt", [*sudo, "apt-get", "update"], [*sudo, "apt-get", "install", "-y"]),
        ("dnf", [], [*sudo, "dnf", "install", "-y"]),
        ("yum", [], [*sudo, "yum", "install", "-y"]),
        ("pacman", [], [*sudo, "pacman", "-Sy", "--noconfirm"]),
    ]:
        command = "apt-get" if manager == "apt" else manager
        if shutil.which(command):
            packages = package_names_for(specs, manager)
            base_packages = ["python3", "python3-pip", "ca-certificates", "curl", "unzip"]
            if manager == "pacman":
                base_packages = ["python", "python-pip", "ca-certificates", "curl", "unzip"]
            if update:
                run_package_install(update)
            for package in [*base_packages, *packages]:
                if package:
                    run_package_install([*install_prefix, package])
            attempts.append(f"{manager}_packages_attempted")
            break
    if shutil.which("curl") and not shutil.which("nextflow"):
        target = Path.home() / ".local" / "bin"
        target.mkdir(parents=True, exist_ok=True)
        installer = Path("/tmp/genoligand-nextflow-install")
        downloaded = run_command(["curl", "-fsSL", "https://get.nextflow.io", "-o", str(installer)])
        if downloaded.returncode == 0:
            run_command(["bash", str(installer)], env={**os.environ, "NXF_HOME": str(target)})
            candidate = Path("nextflow")
            if candidate.exists():
                shutil.move(str(candidate), str(target / "nextflow"))
                (target / "nextflow").chmod(0o755)
    return ";".join(attempts) if attempts else "no_package_manager"


def ensure_command_tool_environment(
    workspace_root: Path,
    env_dir: Path,
    skip_install: bool,
    backend: str = "local",
    wsl_distro: str | None = None,
) -> dict[str, object]:
    manifest = env_dir / "command_tool_environment_manifest.tsv"
    if backend == "wsl":
        if not wsl_distro:
            rows = [
                {
                    "component": spec["component"],
                    "status": "failed",
                    "command": spec["command"],
                    "version": "",
                    "notes": "missing_wsl_distribution",
                }
                for spec in COMMAND_TOOL_SPECS
            ]
            write_command_tool_manifest(manifest, rows)
            return {
                "status": "failed",
                "backend": "wsl",
                "wsl_distro": "",
                "manifest": str(manifest),
                "ready": 0,
                "total": len(rows),
            }
        rows = [probe_wsl_command_tool(spec, wsl_distro) for spec in COMMAND_TOOL_SPECS]
        if any(row["status"] != "ready" for row in rows) and not skip_install:
            notes = install_wsl_command_tools(wsl_distro, missing_command_tools(rows))
            install_vina_user_wsl(wsl_distro)
            rows = [probe_wsl_command_tool(spec, wsl_distro) for spec in COMMAND_TOOL_SPECS]
            for row in rows:
                if row["status"] != "ready" and notes:
                    row["notes"] = f"{row['notes']}; install_attempt:{notes}"
        write_command_tool_manifest(manifest, rows)
        ready = sum(1 for row in rows if row["status"] == "ready")
        return {
            "status": "ready" if ready == len(rows) else "needs_repair" if not skip_install else "skipped",
            "backend": "wsl",
            "wsl_distro": wsl_distro,
            "manifest": str(manifest),
            "ready": ready,
            "total": len(rows),
        }
    rows = [probe_local_command_tool(spec, workspace_root, env_dir) for spec in COMMAND_TOOL_SPECS]
    if any(row["status"] != "ready" for row in rows) and not skip_install:
        notes = install_local_command_tools(missing_command_tools(rows))
        rows = [probe_local_command_tool(spec, workspace_root, env_dir) for spec in COMMAND_TOOL_SPECS]
        for row in rows:
            if row["status"] != "ready" and notes:
                row["notes"] = f"{row['notes']}; install_attempt:{notes}"
    write_command_tool_manifest(manifest, rows)
    ready = sum(1 for row in rows if row["status"] == "ready")
    return {
        "status": "ready" if ready == len(rows) else "needs_repair" if not skip_install else "skipped",
        "backend": "local",
        "manifest": str(manifest),
        "ready": ready,
        "total": len(rows),
    }


def combine_tool_environment_result(
    python_result: dict[str, object],
    command_result: dict[str, object],
) -> dict[str, object]:
    python_ready = int(python_result.get("ready", 0) or 0)
    python_total = int(python_result.get("total", 0) or 0)
    command_ready = int(command_result.get("ready", 0) or 0)
    command_total = int(command_result.get("total", 0) or 0)
    status = "ready" if python_result.get("status") == "ready" and command_result.get("status") == "ready" else "needs_repair"
    return {
        **python_result,
        "status": status,
        "ready": python_ready + command_ready,
        "total": python_total + command_total,
        "python_environment": python_result,
        "command_tools": command_result,
        "command_tool_manifest": command_result.get("manifest", ""),
    }


def write_python_tool_manifest(path: Path, rows: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            fieldnames=[
                "component",
                "status",
                "python",
                "package",
                "import_name",
                "version",
                "notes",
            ],
        )
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {
                    key: str(row.get(key, ""))
                    .replace("\t", " ")
                    .replace("\r", " ")
                    .replace("\n", " ")
                    for key in writer.fieldnames
                }
            )


def load_json(path: Path) -> dict[str, object]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {"status": "unreadable"}


def read_tsv(path: Path) -> list[dict[str, str]]:
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        cleaned: list[dict[str, str]] = []
        for row in csv.DictReader(handle, delimiter="\t"):
            cleaned_row: dict[str, str] = {}
            for key, value in row.items():
                if key is None:
                    continue
                if isinstance(value, list):
                    cleaned_row[key] = " ".join(value).strip()
                else:
                    cleaned_row[key] = (value or "").strip()
            cleaned.append(cleaned_row)
        return cleaned


def write_install_progress(
    science_store: Path,
    *,
    status: str,
    stage: str,
    current_resource: str = "",
    completed_steps: int = 0,
    total_steps: int = 0,
    failed_steps: int = 0,
    bytes_downloaded: int = 0,
    bytes_total: int | None = None,
    message: str = "",
) -> dict[str, object]:
    progress_dir = science_store
    progress_dir.mkdir(parents=True, exist_ok=True)
    if bytes_total and bytes_total > 0:
        percent = min(100, round((bytes_downloaded / bytes_total) * 100))
    elif total_steps > 0:
        percent = min(100, round((completed_steps / total_steps) * 100))
    else:
        percent = 0
    payload: dict[str, object] = {
        "status": status,
        "stage": stage,
        "current_resource": current_resource,
        "completed_steps": completed_steps,
        "total_steps": total_steps,
        "failed_steps": failed_steps,
        "bytes_downloaded": bytes_downloaded,
        "bytes_total": bytes_total if bytes_total is not None else "",
        "percent": percent,
        "message": message,
        "updated_at": int(time.time()),
    }
    (science_store / "install_progress.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return payload


def process_exists(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def read_json_object(path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return value if isinstance(value, dict) else {}


def acquire_install_lock(science_store: Path) -> Path:
    lock_path = science_store / "install.lock.json"
    science_store.mkdir(parents=True, exist_ok=True)
    while True:
        existing = read_json_object(lock_path)
        existing_pid = int(existing.get("pid", 0) or 0)
        if existing_pid and process_exists(existing_pid) and existing_pid != os.getpid():
            write_install_progress(
                science_store,
                status="running",
                stage="等待安装",
                current_resource="安装/修复",
                completed_steps=0,
                total_steps=6,
                message="检测到安装/修复正在运行，正在等待当前任务完成。",
            )
            time.sleep(30)
            continue
        try:
            with lock_path.open("x", encoding="utf-8") as handle:
                json.dump(
                    {
                        "pid": os.getpid(),
                        "started_at": int(time.time()),
                        "command": " ".join(sys.argv),
                    },
                    handle,
                    ensure_ascii=False,
                    indent=2,
                )
                handle.write("\n")
            return lock_path
        except FileExistsError:
            try:
                lock_path.unlink()
            except OSError:
                time.sleep(5)


def release_install_lock(lock_path: Path | None) -> None:
    if not lock_path:
        return
    existing = read_json_object(lock_path)
    existing_pid = int(existing.get("pid", 0) or 0)
    if existing_pid in (0, os.getpid()):
        try:
            lock_path.unlink()
        except OSError:
            pass


def default_source_catalog(workspace_root: Path, science_store: Path) -> Path:
    store_catalog = science_store / "source_catalog.tsv"
    if store_catalog.exists():
        return store_catalog
    repo_catalog = workspace_root / "configs" / "source_catalog.tsv"
    if repo_catalog.exists():
        return repo_catalog
    return store_catalog


def default_source_sync_plan(workspace_root: Path, science_store: Path) -> Path:
    store_plan = science_store / "source_sync_plan.tsv"
    if store_plan.exists():
        return store_plan
    repo_plan = workspace_root / "configs" / "source_sync_plan.tsv"
    if repo_plan.exists():
        return repo_plan
    return store_plan


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_file_with_progress(
    path: Path,
    progress: Callable[[int], None] | None = None,
) -> str:
    digest = hashlib.sha256()
    read_bytes = 0
    last_progress = 0.0
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
            read_bytes += len(chunk)
            now = time.time()
            if progress and (now - last_progress >= 10 or read_bytes == path.stat().st_size):
                progress(read_bytes)
                last_progress = now
    return digest.hexdigest()


def copy_with_progress(
    source,
    handle,
    *,
    bytes_done: int = 0,
    bytes_total: int | None = None,
    progress: ProgressCallback | None = None,
) -> int:
    copied = bytes_done
    while True:
        chunk = source.read(1024 * 1024)
        if not chunk:
            break
        handle.write(chunk)
        copied += len(chunk)
        if progress:
            progress("downloading", copied, bytes_total, "正在下载数据库资源")
    return copied


def download_with_external_tool(
    url: str,
    partial: Path,
    progress: ProgressCallback | None = None,
) -> bool:
    curl = shutil.which("curl.exe") or shutil.which("curl")
    if not curl:
        return False
    command = [
        curl,
        "-L",
        "--fail",
        "--retry",
        "5",
        "--retry-delay",
        "5",
        "--connect-timeout",
        "60",
        "--output",
        str(partial),
        url,
    ]
    if partial.exists() and partial.stat().st_size > 0:
        command.insert(2, "-C")
        command.insert(3, "-")
    if progress:
        progress(
            "downloading",
            partial.stat().st_size if partial.exists() else 0,
            None,
            "内置下载中断，正在使用系统下载器继续下载",
        )
    completed = subprocess.run(command, capture_output=True, text=True, check=False)
    if completed.returncode == 0 and partial.exists() and partial.stat().st_size > 0:
        if progress:
            progress("downloaded", partial.stat().st_size, None, "系统下载器已完成资源下载")
        return True
    if progress:
        message = completed.stderr.strip() or completed.stdout.strip() or "系统下载器未完成下载"
        progress("retrying", partial.stat().st_size if partial.exists() else 0, None, message[:180])
    return False


def copy_or_download(
    url: str,
    destination: Path,
    expected_sha256: str | None,
    progress: ProgressCallback | None = None,
) -> dict[str, object]:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() and destination.stat().st_size > 0:
        if progress:
            progress(
                "verifying",
                destination.stat().st_size,
                destination.stat().st_size,
                "正在校验已下载资源",
            )
        actual_sha256 = sha256_file(destination)
        if not expected_sha256 or actual_sha256.lower() == expected_sha256.lower():
            if progress:
                progress(
                    "completed",
                    destination.stat().st_size,
                    destination.stat().st_size,
                    "资源已存在并通过校验",
                )
            return {
                "status": "ready",
                "bytes": destination.stat().st_size,
                "sha256": actual_sha256,
            }
        if progress:
            progress(
                "retrying",
                destination.stat().st_size,
                destination.stat().st_size,
                "已下载资源校验不一致，正在重新下载",
            )
    partial = destination.with_suffix(destination.suffix + ".part")
    if url.startswith("file://"):
        source = Path(urllib.request.url2pathname(url[7:]))
        total = source.stat().st_size if source.exists() else None
        if progress:
            progress("copying", 0, total, "正在复制本地资源")
        with source.open("rb") as source_handle, partial.open("wb") as target_handle:
            copy_with_progress(source_handle, target_handle, bytes_total=total, progress=progress)
    elif Path(url).exists():
        source = Path(url)
        total = source.stat().st_size
        if progress:
            progress("copying", 0, total, "正在复制本地资源")
        with source.open("rb") as source_handle, partial.open("wb") as target_handle:
            copy_with_progress(source_handle, target_handle, bytes_total=total, progress=progress)
    else:
        last_error: Exception | None = None
        for attempt in range(1, DOWNLOAD_RETRIES + 1):
            existing = partial.stat().st_size if partial.exists() else 0
            if progress:
                progress("downloading", existing, None, f"正在下载数据库资源，第 {attempt} 次尝试")
            request = urllib.request.Request(
                url,
                headers={"User-Agent": DOWNLOAD_USER_AGENT},
            )
            if existing:
                request.add_header("Range", f"bytes={existing}-")
            try:
                with urlopen_with_compatible_tls(
                    request,
                    timeout=DOWNLOAD_TIMEOUT_SECONDS,
                ) as response:
                    status = getattr(response, "status", 200)
                    mode = "ab" if existing and status == 206 else "wb"
                    content_length = response.headers.get("Content-Length")
                    response_bytes = int(content_length) if content_length and content_length.isdigit() else None
                    if status == 206 and response_bytes is not None:
                        total = existing + response_bytes
                        start = existing
                    else:
                        total = response_bytes
                        start = 0
                    with partial.open(mode) as handle:
                        copy_with_progress(
                            response,
                            handle,
                            bytes_done=start,
                            bytes_total=total,
                            progress=progress,
                        )
                last_error = None
                break
            except urllib.error.HTTPError as err:
                if existing and err.code == 416:
                    last_error = None
                    break
                last_error = err
            except (urllib.error.URLError, TimeoutError, OSError) as err:
                last_error = err
            if progress:
                progress("retrying", partial.stat().st_size if partial.exists() else 0, None, "下载中断，准备重试")
            if attempt < DOWNLOAD_RETRIES:
                time.sleep(min(2 * attempt, 5))
        if last_error is not None:
            if not download_with_external_tool(url, partial, progress):
                raise last_error
    if progress:
        progress("verifying", partial.stat().st_size if partial.exists() else 0, partial.stat().st_size if partial.exists() else None, "正在校验资源")
    actual_sha256 = sha256_file(partial)
    if expected_sha256 and actual_sha256.lower() != expected_sha256.lower():
        if progress:
            progress("failed", partial.stat().st_size, partial.stat().st_size, "完整性校验失败")
        return {
            "status": "checksum_failed",
            "bytes": partial.stat().st_size,
            "sha256": actual_sha256,
        }
    partial.replace(destination)
    if progress:
        progress("completed", destination.stat().st_size, destination.stat().st_size, "资源已完成")
    return {
        "status": "ready",
        "bytes": destination.stat().st_size,
        "sha256": actual_sha256,
    }


def prepare_downloads(catalog: Path, science_store: Path) -> dict[str, object]:
    manifest = science_store / "download_execution_manifest.tsv"
    rows: list[dict[str, str]] = []
    if catalog.exists():
        with catalog.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            for row in reader:
                rows.append({key: (value or "").strip() for key, value in row.items()})
    manifest.parent.mkdir(parents=True, exist_ok=True)
    write_install_progress(
        science_store,
        status="running",
        stage="下载数据库",
        current_resource="准备下载清单",
        completed_steps=0,
        total_steps=len(rows),
        message="正在准备数据库下载",
    )
    with manifest.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            fieldnames=[
                "source_id",
                "url",
                "local_path",
                "status",
                "bytes",
                "sha256",
                "notes",
            ],
        )
        writer.writeheader()
        if not rows:
            writer.writerow(
                {
                    "source_id": "catalog",
                    "url": str(catalog),
                    "local_path": "",
                    "status": "not_configured",
                    "bytes": "0",
                    "sha256": "",
                    "notes": "source_catalog_tsv_missing_or_empty",
                }
            )
            write_install_progress(
                science_store,
                status="needs_repair",
                stage="下载数据库",
                current_resource="下载清单",
                completed_steps=0,
                total_steps=0,
                message="缺少数据库下载清单",
            )
            return {
                "status": "not_configured",
                "catalog": str(catalog),
                "manifest": str(manifest),
                "ready": 0,
                "failed": 0,
            }
        ready = 0
        failed = 0
        for row in rows:
            source_id = row.get("source_id", "")
            source_label = row.get("source_label") or source_id or "数据库资源"
            url = row.get("url", "")
            local_path = row.get("local_path", "")
            expected = row.get("sha256", "") or None
            completed = ready + failed
            if not source_id or not url or not local_path:
                failed += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="下载数据库",
                    current_resource=source_label,
                    completed_steps=ready + failed,
                    total_steps=len(rows),
                    failed_steps=failed,
                    message="数据库来源记录不完整",
                )
                writer.writerow(
                    {
                        "source_id": source_id or "missing_source_id",
                        "url": url,
                        "local_path": local_path,
                        "status": "invalid_catalog_row",
                        "bytes": "0",
                        "sha256": "",
                        "notes": "source_id_url_local_path_required",
                    }
                )
                continue
            destination = science_store / local_path
            try:
                def progress_callback(status: str, bytes_done: int, bytes_total: int | None, message: str) -> None:
                    write_install_progress(
                        science_store,
                        status="running" if status not in {"failed", "completed"} else status,
                        stage="下载数据库",
                        current_resource=source_label,
                        completed_steps=completed,
                        total_steps=len(rows),
                        failed_steps=failed,
                        bytes_downloaded=bytes_done,
                        bytes_total=bytes_total,
                        message=message,
                    )

                result = copy_or_download(url, destination, expected, progress_callback)
                if result["status"] == "ready":
                    ready += 1
                else:
                    failed += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="下载数据库",
                    current_resource=source_label,
                    completed_steps=ready + failed,
                    total_steps=len(rows),
                    failed_steps=failed,
                    bytes_downloaded=int(result.get("bytes", 0) or 0),
                    bytes_total=int(result.get("bytes", 0) or 0),
                    message="数据库资源处理完成" if result["status"] == "ready" else "数据库资源处理失败",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "url": url,
                        "local_path": local_path,
                        "status": str(result["status"]),
                        "bytes": str(result["bytes"]),
                        "sha256": str(result["sha256"]),
                        "notes": "download_or_copy_completed",
                    }
                )
            except Exception as err:  # noqa: BLE001 - report install diagnostics, do not hide failures
                failed += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="下载数据库",
                    current_resource=source_label,
                    completed_steps=ready + failed,
                    total_steps=len(rows),
                    failed_steps=failed,
                    message="数据库资源下载失败",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "url": url,
                        "local_path": local_path,
                        "status": "failed",
                        "bytes": "0",
                        "sha256": "",
                        "notes": str(err).replace("\t", " ").replace("\n", " "),
                    }
                )
        result_status = "ready" if failed == 0 else "failed"
        write_install_progress(
            science_store,
            status="ready" if result_status == "ready" else "needs_repair",
            stage="下载数据库",
            current_resource="数据库下载",
            completed_steps=ready + failed,
            total_steps=len(rows),
            failed_steps=failed,
            message="数据库下载完成" if result_status == "ready" else "数据库下载存在失败项",
        )
        return {
            "status": result_status,
            "catalog": str(catalog),
            "manifest": str(manifest),
            "ready": ready,
            "failed": failed,
        }


def direct_source_location(row: dict[str, str]) -> str:
    for key in ("download_url", "source_path", "source_url"):
        value = row.get(key, "").strip()
        if value:
            return value
    landing = row.get("landing_url", "").strip()
    if landing.startswith("file://") or Path(landing).exists():
        return landing
    return ""


def directory_digest(path: Path) -> str:
    digest = hashlib.sha256()
    if path.is_file():
        digest.update(path.name.encode("utf-8"))
        digest.update(str(path.stat().st_size).encode("ascii"))
        digest.update(sha256_file(path).encode("ascii"))
        return digest.hexdigest()
    if path.is_dir():
        for child in sorted(p for p in path.rglob("*") if p.is_file()):
            rel = child.relative_to(path).as_posix()
            digest.update(rel.encode("utf-8"))
            digest.update(str(child.stat().st_size).encode("ascii"))
            digest.update(sha256_file(child).encode("ascii"))
    return digest.hexdigest()


def extract_html_links(page_url: str, include_pattern: str) -> list[str]:
    request = urllib.request.Request(page_url, headers={"User-Agent": DOWNLOAD_USER_AGENT})
    with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
        html = response.read().decode("utf-8", errors="replace")
    pattern = re.compile(include_pattern) if include_pattern else None
    hrefs = re.findall(r"""href\s*=\s*["']?([^"'\s>]+)""", html, flags=re.IGNORECASE)
    urls = []
    seen = set()
    for href in hrefs:
        if href.startswith("?") or href.startswith("#") or href.endswith("/"):
            continue
        if pattern and not pattern.search(href):
            continue
        absolute = urllib.parse.urljoin(page_url, href)
        if absolute not in seen:
            seen.add(absolute)
            urls.append(absolute)
    return urls


def sync_html_links(
    row: dict[str, str],
    target: Path,
    progress: ProgressCallback | None = None,
) -> dict[str, object]:
    source_url = row.get("source_url") or row.get("download_url") or row.get("landing_url")
    include_pattern = row.get("include_pattern", "")
    if not source_url:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "html_source_url_required"}
    links = extract_html_links(source_url, include_pattern)
    if not links:
        return {"status": "failed", "bytes": 0, "sha256": "", "notes": "no_matching_links"}
    target.mkdir(parents=True, exist_ok=True)
    failures = []
    for index, link in enumerate(links, start=1):
        destination = target / Path(urllib.parse.urlparse(link).path).name
        if progress:
            progress("downloading", index - 1, len(links), f"正在同步资源 {index}/{len(links)}")
        try:
            copy_or_download(link, destination, row.get("sha256", "") or None, progress)
        except Exception as err:  # noqa: BLE001 - report per-source install failure
            failures.append(f"{link}:{err}")
    if failures:
        return {"status": "failed", "bytes": directory_size(target), "sha256": directory_digest(target), "notes": "|".join(failures)[:1000]}
    return {
        "status": "ready" if path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target),
        "notes": f"html_links_synced:{len(links)}",
    }


def list_html_hrefs(page_url: str) -> list[str]:
    parsed = urllib.parse.urlparse(page_url)
    if parsed.scheme == "file":
        local = Path(urllib.request.url2pathname(parsed.path))
        if local.is_dir():
            local = local / "index.html"
        html = local.read_text(encoding="utf-8", errors="replace")
        return re.findall(r"""href\s*=\s*["']?([^"'\s>]+)""", html, flags=re.IGNORECASE)
    request = urllib.request.Request(page_url, headers={"User-Agent": DOWNLOAD_USER_AGENT})
    with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
        html = response.read().decode("utf-8", errors="replace")
    return re.findall(r"""href\s*=\s*["']?([^"'\s>]+)""", html, flags=re.IGNORECASE)


def sync_html_tree(row: dict[str, str], target: Path) -> dict[str, object]:
    source_url = row.get("source_url") or row.get("download_url") or row.get("landing_url")
    include_pattern = row.get("include_pattern", "")
    pattern = re.compile(include_pattern) if include_pattern else None
    max_depth = int(row.get("max_depth") or "3")
    if not source_url:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "html_tree_source_url_required"}
    root_url = source_url if source_url.endswith("/") else f"{source_url}/"
    target.mkdir(parents=True, exist_ok=True)
    downloaded = 0
    failures = []
    queue: list[tuple[str, Path, int]] = [(root_url, target, 0)]
    seen_dirs = set()
    while queue:
        current_url, current_target, depth = queue.pop(0)
        if current_url in seen_dirs:
            continue
        seen_dirs.add(current_url)
        try:
            hrefs = list_html_hrefs(current_url)
        except Exception as err:  # noqa: BLE001 - report source failure
            failures.append(f"{current_url}:{err}")
            continue
        for href in hrefs:
            if href.startswith("?") or href.startswith("#") or href.startswith("../") or href == "/":
                continue
            absolute = urllib.parse.urljoin(current_url, href)
            parsed_path = urllib.parse.urlparse(href).path
            name = Path(parsed_path.rstrip("/")).name
            if not name:
                continue
            if href.endswith("/"):
                if depth + 1 <= max_depth:
                    queue.append((absolute if absolute.endswith("/") else f"{absolute}/", current_target / name, depth + 1))
                continue
            if pattern and not pattern.search(href):
                continue
            destination = current_target / name
            try:
                copy_or_download(absolute, destination, row.get("sha256", "") or None)
                downloaded += 1
            except Exception as err:  # noqa: BLE001 - report per-file failure
                failures.append(f"{absolute}:{err}")
    if failures:
        return {"status": "failed", "bytes": directory_size(target), "sha256": directory_digest(target), "notes": "|".join(failures)[:1000]}
    return {
        "status": "ready" if downloaded and path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target),
        "notes": f"html_tree_files_synced:{downloaded}",
    }


def parse_gcs_location(row: dict[str, str]) -> tuple[str, str] | None:
    explicit_bucket = row.get("gcs_bucket", "").strip()
    explicit_prefix = row.get("gcs_prefix", "").strip()
    if explicit_bucket:
        return explicit_bucket, explicit_prefix
    location = (row.get("source_url") or row.get("download_url") or row.get("landing_url") or "").strip()
    if not location.startswith("gs://"):
        return None
    without_scheme = location[5:]
    bucket, _, prefix = without_scheme.partition("/")
    return bucket, prefix


def sync_gcs_prefix(
    row: dict[str, str],
    target: Path,
    progress: ProgressCallback | None = None,
) -> dict[str, object]:
    parsed = parse_gcs_location(row)
    if not parsed:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "gcs_bucket_or_gs_url_required"}
    bucket, prefix = parsed
    include_pattern = row.get("include_pattern", "")
    pattern = re.compile(include_pattern) if include_pattern else None
    target.mkdir(parents=True, exist_ok=True)
    page_token = ""
    downloaded = 0
    failures = []
    while True:
        query = urllib.parse.urlencode({"prefix": prefix, "maxResults": "1000", **({"pageToken": page_token} if page_token else {})})
        api_url = f"https://storage.googleapis.com/storage/v1/b/{urllib.parse.quote(bucket)}/o?{query}"
        request = urllib.request.Request(api_url, headers={"User-Agent": DOWNLOAD_USER_AGENT})
        with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
            payload = json.loads(response.read().decode("utf-8"))
        for item in payload.get("items", []):
            name = item.get("name", "")
            if not name or name.endswith("/"):
                continue
            rel_name = name[len(prefix) :].lstrip("/") if prefix and name.startswith(prefix) else name
            if pattern and not pattern.search(rel_name):
                continue
            media_url = item.get("mediaLink") or f"https://storage.googleapis.com/{bucket}/{urllib.parse.quote(name)}"
            destination = target / rel_name
            if progress:
                progress("downloading", downloaded, None, f"正在同步对象 {rel_name}")
            try:
                copy_or_download(media_url, destination, None, progress)
                downloaded += 1
            except Exception as err:  # noqa: BLE001 - report per-source install failure
                failures.append(f"{name}:{err}")
        page_token = payload.get("nextPageToken", "")
        if not page_token:
            break
    if failures:
        return {"status": "failed", "bytes": directory_size(target), "sha256": directory_digest(target), "notes": "|".join(failures)[:1000]}
    return {
        "status": "ready" if downloaded and path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target),
        "notes": f"gcs_objects_synced:{downloaded}",
    }


def sync_rsync(row: dict[str, str], target: Path, env: dict[str, str] | None) -> dict[str, object]:
    source_url = row.get("source_url") or row.get("download_url") or row.get("landing_url")
    if not source_url:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "rsync_source_url_required"}
    target.mkdir(parents=True, exist_ok=True)
    command = ["rsync", "-rlpt", "-z", "--delete", source_url, str(target)]
    completed = subprocess.run(command, capture_output=True, text=True, env=env, timeout=None)
    if completed.returncode != 0:
        notes = (completed.stderr or completed.stdout or "rsync_failed").replace("\t", " ").replace("\n", " ")
        return {"status": "failed", "bytes": directory_size(target), "sha256": directory_digest(target), "notes": notes[:1000]}
    return {
        "status": "ready" if path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target),
        "notes": "rsync_synced",
    }


def sync_rcsb_ligand_bound_scope(
    row: dict[str, str],
    target: Path,
    science_store: Path,
    progress: ProgressCallback | None = None,
) -> dict[str, object]:
    pdb_ids = ligand_bound_pdb_scope(science_store)
    if not pdb_ids:
        return {
            "status": "requires_resource_bundle",
            "bytes": 0,
            "sha256": "",
            "notes": "ligand_bound_pdb_scope_missing",
        }
    target.mkdir(parents=True, exist_ok=True)
    template = row.get("source_url") or "https://files.rcsb.org/download/{pdb_id}.cif.gz"
    downloaded = 0
    failures: list[str] = []
    sorted_ids = sorted(pdb_ids)
    total = len(sorted_ids)
    workers = max(1, int(os.environ.get("GENOLIGAND_RCSB_DOWNLOAD_WORKERS", str(RCSB_DOWNLOAD_WORKERS))))

    def sync_one(pdb_id: str) -> tuple[str, bool, str]:
        upper = pdb_id.upper()
        destination = target / upper[1:3].lower() / f"{upper.lower()}.cif.gz"
        if path_has_payload(destination):
            return upper, True, "already_available"
        url = template.replace("{pdb_id}", upper).replace("{pdb_id_lower}", upper.lower())
        try:
            copy_or_download(url, destination, None)
            return upper, True, "downloaded"
        except Exception as err:  # noqa: BLE001 - keep per-entry provenance in install manifest
            return upper, False, str(err)

    last_progress = 0.0
    completed = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=min(workers, total)) as pool:
        futures = [pool.submit(sync_one, pdb_id) for pdb_id in sorted_ids]
        for future in concurrent.futures.as_completed(futures):
            upper, ok, note = future.result()
            completed += 1
            if ok:
                downloaded += 1
            else:
                failures.append(f"{upper}:{note}")
            now = time.time()
            if progress and (completed == 1 or completed % 25 == 0 or now - last_progress >= 30):
                progress(
                    "downloading",
                    completed,
                    total,
                    f"正在同步 RCSB 结构 {completed}/{total}，并行任务 {min(workers, total)}",
                )
                last_progress = now
    scope_manifest = target.parent / "ligand_bound_pdb_scope.tsv"
    scope_manifest.parent.mkdir(parents=True, exist_ok=True)
    with scope_manifest.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=["pdb_id", "status"])
        writer.writeheader()
        for pdb_id in sorted(pdb_ids):
            upper = pdb_id.upper()
            destination = target / upper[1:3].lower() / f"{upper.lower()}.cif.gz"
            writer.writerow({"pdb_id": upper, "status": "ready" if path_has_payload(destination) else "missing"})
    if failures:
        return {
            "status": "failed",
            "bytes": directory_size(target),
            "sha256": directory_digest(target),
            "notes": "|".join(failures)[:1000],
        }
    return {
        "status": "ready" if downloaded and path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target),
        "notes": f"rcsb_ligand_bound_entries_synced:{downloaded}",
    }


def ligand_bound_pdb_scope(science_store: Path) -> set[str]:
    pdb_ids: set[str] = set()
    explicit = science_store / "downloads/rcsb/ligand_bound_pdb_ids.txt"
    if explicit.exists():
        pdb_ids.update(extract_pdb_ids(explicit.read_text(encoding="utf-8", errors="replace")))
    structure_index = science_store / "indexes/structure_targets.tsv"
    if structure_index.exists():
        pdb_ids.update(extract_pdb_ids_from_structure_index(structure_index))
    for path in [
        science_store / "downloads/binding_moad/bindingmoad.csv",
        science_store / "downloads/binding_moad/BindingMOAD_data_for_PDB_1_31_2022.csv",
        science_store / "downloads/biolip2/BioLiP.txt",
        science_store / "downloads/biolip2/BioLiP.txt.gz",
    ]:
        if path.exists():
            try:
                pdb_ids.update(extract_pdb_ids_from_source_file(path))
            except OSError:
                continue
    explicit_filter = os.environ.get("GENOLIGAND_RCSB_PDB_IDS", "").strip()
    if explicit_filter:
        requested = extract_pdb_ids(explicit_filter)
        pdb_ids = {pdb_id for pdb_id in pdb_ids if pdb_id in requested} | requested
    limit_text = os.environ.get("GENOLIGAND_RCSB_SCOPE_LIMIT", "").strip()
    valid_ids = sorted(pdb_id for pdb_id in pdb_ids if valid_pdb_id(pdb_id))
    if limit_text:
        try:
            limit = int(limit_text)
        except ValueError:
            limit = 0
        if limit > 0:
            valid_ids = valid_ids[:limit]
    return set(valid_ids)


def extract_pdb_ids_from_structure_index(path: Path) -> set[str]:
    ids: set[str] = set()
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            for key in ("target_id", "target_name", "structure_tokens", "evidence"):
                ids.update(extract_pdb_ids(row.get(key, "")))
    return ids


def read_text_or_gzip(path: Path) -> str:
    if path.suffix == ".gz":
        import gzip

        with gzip.open(path, "rt", encoding="utf-8", errors="replace") as handle:
            return handle.read()
    return path.read_text(encoding="utf-8", errors="replace")


def extract_pdb_ids(text: str) -> set[str]:
    return {
        match.group(0).upper()
        for match in re.finditer(r"\b[0-9][A-Za-z0-9]{3}\b", text)
        if valid_pdb_id(match.group(0))
    }


def valid_pdb_id(value: str) -> bool:
    text = value.strip().upper()
    return bool(re.fullmatch(r"[0-9][A-Z0-9]{3}", text)) and any(char.isalpha() for char in text)


def extract_pdb_ids_from_source_file(path: Path) -> set[str]:
    if path.suffix.lower() == ".csv":
        text = read_text_or_gzip(path)
        reader = csv.DictReader(text.splitlines())
        ids: set[str] = set()
        for row in reader:
            for key in ("pdbFile", "pdb_id", "pdb", "PDB"):
                value = (row.get(key) or "").strip()
                if valid_pdb_id(value):
                    ids.add(value.upper())
                    break
        return ids
    ids: set[str] = set()
    for line in read_text_or_gzip(path).splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        first = re.split(r"[\t, ]+", line.strip(), maxsplit=1)[0]
        if valid_pdb_id(first):
            ids.add(first.upper())
    return ids


def sync_external_row(
    row: dict[str, str],
    target: Path,
    science_store: Path,
    env: dict[str, str] | None,
    progress: ProgressCallback | None = None,
) -> dict[str, object]:
    method = (row.get("sync_method") or "direct").strip().lower()
    if method == "html_links":
        return sync_html_links(row, target, progress)
    if method == "html_tree":
        return sync_html_tree(row, target)
    if method == "gcs_prefix":
        return sync_gcs_prefix(row, target, progress)
    if method == "rcsb_ligand_bound_scope":
        return sync_rcsb_ligand_bound_scope(row, target, science_store, progress)
    if method == "rsync":
        return sync_rsync(row, target, env)
    location = direct_source_location(row)
    if not location:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "direct_download_or_local_source_required"}
    destination = external_destination(target, location)
    result = copy_or_download(location, destination, row.get("sha256", "") or None, progress)
    return {
        "status": result["status"],
        "bytes": result["bytes"],
        "sha256": result["sha256"],
        "notes": f"{method}_resource_synced",
    }


def rcsb_scope_has_missing_coordinates(science_store: Path, target: Path | None) -> bool:
    if target is None:
        return True
    pdb_ids = ligand_bound_pdb_scope(science_store)
    if not pdb_ids:
        return False
    for pdb_id in pdb_ids:
        upper = pdb_id.upper()
        destination = target / upper[1:3].lower() / f"{upper.lower()}.cif.gz"
        if not path_has_payload(destination):
            return True
    return False


def sync_external_sources(
    plan: Path,
    science_store: Path,
    skip_downloads: bool,
    env: dict[str, str] | None = None,
) -> dict[str, object]:
    manifest = science_store / "external_sync_execution_manifest.tsv"
    rows = read_tsv(plan)
    manifest.parent.mkdir(parents=True, exist_ok=True)
    write_install_progress(
        science_store,
        status="running",
        stage="同步大型资源",
        current_resource="准备资源同步",
        completed_steps=0,
        total_steps=len(rows),
        message="正在准备大型资源同步",
    )
    with manifest.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            fieldnames=[
                "source_id",
                "source_label",
                "target_rel_path",
                "status",
                "bytes",
                "sha256",
                "notes",
            ],
        )
        writer.writeheader()
        if not rows:
            writer.writerow(
                {
                    "source_id": "external_sources",
                    "source_label": "大型科学资源",
                    "target_rel_path": "",
                    "status": "not_configured",
                    "bytes": "0",
                    "sha256": "",
                    "notes": "source_sync_plan_tsv_missing_or_empty",
                }
            )
            write_install_progress(
                science_store,
                status="needs_repair",
                stage="同步大型资源",
                current_resource="资源同步清单",
                completed_steps=0,
                total_steps=0,
                message="缺少大型资源同步清单",
            )
            return {
                "status": "not_configured",
                "manifest": str(manifest),
                "ready": 0,
                "failed": 0,
                "needs_action": 0,
                "total": 0,
            }
        ready = 0
        failed = 0
        needs_action = 0
        for row in rows:
            source_id = row.get("source_id", "")
            label = row.get("source_label") or source_id or "科学资源"
            target_rel = row.get("target_rel_path", "")
            ready_marker = row.get("ready_marker") or target_rel
            target = science_store / target_rel if target_rel else None
            marker = science_store / ready_marker if ready_marker else target
            method = (row.get("sync_method") or "direct").strip().lower()
            marker_ready = bool(marker and path_has_payload(marker))
            if marker_ready and method == "rcsb_ligand_bound_scope":
                marker_ready = not rcsb_scope_has_missing_coordinates(science_store, target)
            if marker_ready:
                ready += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="同步大型资源",
                    current_resource=label,
                    completed_steps=ready + failed + needs_action,
                    total_steps=len(rows),
                    failed_steps=failed,
                    bytes_downloaded=directory_size(marker),
                    bytes_total=directory_size(marker),
                    message="资源已可用",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "source_label": label,
                        "target_rel_path": target_rel,
                        "status": "ready",
                        "bytes": str(directory_size(marker)),
                        "sha256": "",
                        "notes": "already_available",
                    }
                )
                continue
            if skip_downloads:
                needs_action += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="同步大型资源",
                    current_resource=label,
                    completed_steps=ready + failed + needs_action,
                    total_steps=len(rows),
                    failed_steps=failed,
                    message="本次未执行资源同步",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "source_label": label,
                        "target_rel_path": target_rel,
                        "status": "skipped",
                        "bytes": "0",
                        "sha256": "",
                        "notes": "download_skipped",
                    }
                )
                continue
            if not target:
                needs_action += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="同步大型资源",
                    current_resource=label,
                    completed_steps=ready + failed + needs_action,
                    total_steps=len(rows),
                    failed_steps=failed,
                    message="资源目标未配置",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "source_label": label,
                        "target_rel_path": target_rel,
                        "status": "requires_resource_bundle",
                        "bytes": "0",
                        "sha256": "",
                        "notes": "target_rel_path_required",
                    }
                )
                continue
            try:
                completed_before = ready + failed + needs_action

                def progress_callback(
                    status_text: str,
                    bytes_done: int,
                    bytes_total: int | None,
                    message: str,
                ) -> None:
                    write_install_progress(
                        science_store,
                        status="running" if status_text not in {"failed", "completed"} else status_text,
                        stage="同步大型资源",
                        current_resource=label,
                        completed_steps=completed_before,
                        total_steps=len(rows),
                        failed_steps=failed,
                        bytes_downloaded=bytes_done,
                        bytes_total=bytes_total,
                        message=message,
                    )

                write_install_progress(
                    science_store,
                    status="running",
                    stage="同步大型资源",
                    current_resource=label,
                    completed_steps=completed_before,
                    total_steps=len(rows),
                    failed_steps=failed,
                    message=f"正在同步 {label}",
                )
                result = sync_external_row(row, target, science_store, env, progress_callback)
                if result["status"] == "ready":
                    ready += 1
                elif result["status"] == "requires_resource_bundle":
                    needs_action += 1
                else:
                    failed += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="同步大型资源",
                    current_resource=label,
                    completed_steps=ready + failed + needs_action,
                    total_steps=len(rows),
                    failed_steps=failed,
                    bytes_downloaded=int(result.get("bytes", 0) or 0),
                    bytes_total=int(result.get("bytes", 0) or 0) or None,
                    message="资源同步完成" if result["status"] == "ready" else "资源需要继续处理",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "source_label": label,
                        "target_rel_path": target_rel,
                        "status": str(result["status"]),
                        "bytes": str(result["bytes"]),
                        "sha256": str(result["sha256"]),
                        "notes": str(result["notes"]),
                    }
                )
            except Exception as err:  # noqa: BLE001 - install must report actionable status
                failed += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="同步大型资源",
                    current_resource=label,
                    completed_steps=ready + failed + needs_action,
                    total_steps=len(rows),
                    failed_steps=failed,
                    message="资源同步失败",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "source_label": label,
                        "target_rel_path": target_rel,
                        "status": "failed",
                        "bytes": "0",
                        "sha256": "",
                        "notes": str(err).replace("\t", " ").replace("\n", " "),
                    }
                )
        total = len(rows)
        status = "ready" if ready == total and failed == 0 else "failed" if failed else "needs_sync"
        write_install_progress(
            science_store,
            status="ready" if status == "ready" else "needs_repair",
            stage="同步大型资源",
            current_resource="大型资源同步",
            completed_steps=ready + failed + needs_action,
            total_steps=total,
            failed_steps=failed,
            message="大型资源同步完成" if status == "ready" else "大型资源仍需处理",
        )
        return {
            "status": status,
            "manifest": str(manifest),
            "ready": ready,
            "failed": failed,
            "needs_action": needs_action,
            "total": total,
        }


def build_persistent_alignment_databases(
    science_store: Path,
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
) -> dict[str, object]:
    mmseqs_rows = build_mmseqs_target_databases(science_store, env, wsl_distro)
    foldseek_rows = build_foldseek_target_database(science_store, env, wsl_distro)
    all_rows = [*mmseqs_rows, *foldseek_rows]
    ready = sum(1 for row in all_rows if row.get("status") == "ready")
    failed = sum(1 for row in all_rows if row.get("status") == "failed")
    deferred = sum(1 for row in all_rows if row.get("status") == "deferred_large_source")
    total = len(all_rows)
    status = "ready" if ready and failed == 0 else "failed" if failed else "needs_repair"
    return {
        "status": status,
        "ready": ready,
        "failed": failed,
        "deferred": deferred,
        "total": total,
        "mmseqs_manifest": str(science_store / "indexes/mmseqs/mmseqs_target_databases.tsv"),
        "foldseek_manifest": str(science_store / "indexes/foldseek/foldseek_target_databases.tsv"),
    }


def ensure_openff_forcefield_file(science_store: Path) -> dict[str, object]:
    target_dir = science_store / "indexes" / "forcefields"
    target_dir.mkdir(parents=True, exist_ok=True)
    destination = target_dir / OPENFF_FORCEFIELD_FILE
    manifest = target_dir / "forcefield_manifest.tsv"
    if not path_has_payload(destination):
        result = copy_or_download(OPENFF_FORCEFIELD_URL, destination, None)
    else:
        result = {
            "status": "ready",
            "bytes": destination.stat().st_size,
            "sha256": sha256_file(destination),
        }
    row = {
        "forcefield_id": "openff_sage_2_2_1",
        "file": destination.name,
        "status": str(result.get("status", "failed")),
        "bytes": str(result.get("bytes", 0)),
        "sha256": str(result.get("sha256", "")),
        "source_url": OPENFF_FORCEFIELD_URL,
        "license": "CC-BY-4.0",
        "notes": "OpenFF Sage 2.2.1 SMIRNOFF force field",
    }
    write_simple_tsv(
        manifest,
        ["forcefield_id", "file", "status", "bytes", "sha256", "source_url", "license", "notes"],
        [row],
    )
    return {
        "status": row["status"],
        "file": str(destination),
        "manifest": str(manifest),
        "sha256": row["sha256"],
        "license": row["license"],
    }


def build_mmseqs_target_databases(science_store: Path, env: dict[str, str] | None, wsl_distro: str | None) -> list[dict[str, str]]:
    index_root = science_store / "indexes" / "mmseqs"
    manifest = index_root / "mmseqs_target_databases.tsv"
    index_root.mkdir(parents=True, exist_ok=True)
    command = shutil.which("mmseqs", path=(env or os.environ).get("PATH"))
    rows: list[dict[str, str]] = []
    for label, source in sequence_database_sources(science_store):
        db_path = index_root / safe_file_stem(label) / "target"
        if alignment_source_deferred(source):
            status, notes = "deferred_large_source", "source_exceeds_sync_index_threshold"
        else:
            status, notes = build_mmseqs_database(command, source, db_path, env, wsl_distro)
        rows.append(
            {
                "source_id": label,
                "source_path": relative_path(science_store, source),
                "db_path": relative_path(science_store, db_path),
                "status": status,
                "notes": notes,
            }
        )
    write_simple_tsv(manifest, ["source_id", "source_path", "db_path", "status", "notes"], rows)
    return rows


def alignment_source_deferred(source: Path) -> bool:
    limit = alignment_db_max_source_bytes()
    if limit <= 0:
        return False
    try:
        return source.stat().st_size > limit
    except OSError:
        return False


def alignment_db_max_source_bytes() -> int:
    raw = os.environ.get("GENOLIGAND_ALIGNMENT_DB_MAX_SOURCE_BYTES", "").strip()
    if not raw:
        return DEFAULT_ALIGNMENT_DB_MAX_SOURCE_BYTES
    try:
        return max(0, int(raw))
    except ValueError:
        return DEFAULT_ALIGNMENT_DB_MAX_SOURCE_BYTES


def build_foldseek_target_database(science_store: Path, env: dict[str, str] | None, wsl_distro: str | None) -> list[dict[str, str]]:
    index_root = science_store / "indexes" / "foldseek"
    manifest = index_root / "foldseek_target_databases.tsv"
    index_root.mkdir(parents=True, exist_ok=True)
    command = shutil.which("foldseek", path=(env or os.environ).get("PATH"))
    source = science_store / "downloads" / "rcsb" / "mmcif"
    db_path = index_root / "rcsb_mmcif" / "target"
    status, notes = build_foldseek_database(command, source, db_path, env, wsl_distro)
    rows = [
        {
            "source_id": "rcsb_mmcif",
            "source_path": relative_path(science_store, source),
            "db_path": relative_path(science_store, db_path),
            "status": status,
            "notes": notes,
        }
    ]
    write_simple_tsv(manifest, ["source_id", "source_path", "db_path", "status", "notes"], rows)
    return rows


def sequence_database_sources(science_store: Path) -> list[tuple[str, Path]]:
    roots = [
        ("uniprot", science_store / "downloads" / "uniprot"),
        ("metagenome", science_store / "downloads" / "metagenome"),
        ("biolip2", science_store / "downloads" / "biolip2"),
    ]
    suffixes = (".fa", ".faa", ".fasta", ".fa.gz", ".faa.gz", ".fasta.gz")
    sources: list[tuple[str, Path]] = []
    for label, root in roots:
        if not root.exists():
            continue
        for path in sorted(root.rglob("*")):
            name = path.name.lower()
            if path.is_file() and name.endswith(suffixes) and not name.endswith(".part"):
                sources.append((f"{label}_{safe_file_stem(path.stem)}", path))
    return sources


def build_mmseqs_database(
    command: str | None,
    source: Path,
    db_path: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> tuple[str, str]:
    if not path_has_payload(source):
        return "failed", "source_missing"
    if database_prefix_ready(db_path):
        return "ready", "already_available"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = db_path.parent / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    if os.name == "nt" and wsl_distro:
        return run_wsl_database_commands(
            wsl_distro,
            [
                ["mmseqs", "createdb", wsl_path(source), wsl_path(db_path)],
                ["mmseqs", "createindex", wsl_path(db_path), wsl_path(tmp)],
            ],
            "mmseqs_createdb_createindex_completed",
        )
    if command is None:
        return "failed", "mmseqs_executable_missing"
    created = run_command([command, "createdb", str(source), str(db_path)], env=env)
    if created.returncode != 0:
        return "failed", process_note(created)
    indexed = run_command([command, "createindex", str(db_path), str(tmp)], env=env)
    if indexed.returncode != 0:
        return "failed", process_note(indexed)
    return "ready", "mmseqs_createdb_createindex_completed"


def build_foldseek_database(
    command: str | None,
    source: Path,
    db_path: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> tuple[str, str]:
    if not path_has_payload(source):
        return "failed", "source_missing"
    if database_prefix_ready(db_path):
        return "ready", "already_available"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = db_path.parent / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    if os.name == "nt" and wsl_distro:
        return run_wsl_database_commands(
            wsl_distro,
            [
                ["foldseek", "createdb", wsl_path(source), wsl_path(db_path)],
                ["foldseek", "createindex", wsl_path(db_path), wsl_path(tmp)],
            ],
            "foldseek_createdb_createindex_completed",
        )
    if command is None:
        return "failed", "foldseek_executable_missing"
    created = run_command([command, "createdb", str(source), str(db_path)], env=env)
    if created.returncode != 0:
        return "failed", process_note(created)
    indexed = run_command([command, "createindex", str(db_path), str(tmp)], env=env)
    if indexed.returncode != 0:
        return "failed", process_note(indexed)
    return "ready", "foldseek_createdb_createindex_completed"


def run_wsl_database_commands(
    distro: str,
    commands: list[list[str]],
    success_note: str,
) -> tuple[str, str]:
    script_parts = [f'export PATH="{wsl_runtime_path_prefix()}:$PATH"']
    for command in commands:
        script_parts.append(bash_words(command))
    completed = run_wsl_user_shell(distro, " && ".join(script_parts))
    if completed.returncode != 0:
        return "failed", process_note(completed)
    return "ready", success_note


def wsl_path(path: Path) -> str:
    text = str(path)
    if len(text) >= 3 and text[1] == ":" and text[2] in "\\/":
        drive = text[0].lower()
        rest = text[3:].replace("\\", "/")
        return f"/mnt/{drive}/{rest}"
    return text.replace("\\", "/")


def database_prefix_ready(prefix: Path) -> bool:
    return Path(str(prefix) + ".dbtype").exists() or prefix.with_suffix(".dbtype").exists()


def safe_file_stem(value: str) -> str:
    text = re.sub(r"[^A-Za-z0-9_.-]+", "_", value.strip())
    return text.strip("._-")[:80] or "target"


def relative_path(root: Path, path: Path) -> str:
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return str(path)


def process_note(completed: subprocess.CompletedProcess[str]) -> str:
    text = (completed.stderr or completed.stdout or "").strip()
    text = text.replace("\t", " ").replace("\r", " ").replace("\n", " ")
    return text[:240] or f"exit_code_{completed.returncode}"


def write_simple_tsv(path: Path, fieldnames: list[str], rows: list[dict[str, str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {
                    key: str(row.get(key, "")).replace("\t", " ").replace("\r", " ").replace("\n", " ")
                    for key in fieldnames
                }
            )


def external_destination(target: Path, location: str) -> Path:
    if target.suffix:
        return target
    source_name = Path(urllib.parse.urlparse(location).path).name
    if source_name:
        return target / source_name
    return target / "external_resource"


def directory_size(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    if not path.exists():
        return 0
    total = 0
    for child in path.rglob("*"):
        if child.is_file():
            total += child.stat().st_size
    return total


def path_has_payload(path: Path) -> bool:
    if path.is_file():
        return path.stat().st_size > 0
    if not path.is_dir():
        return False
    return any(child.is_file() for child in path.rglob("*"))


def file_count(path: Path) -> int:
    if path.is_file():
        return 1
    if not path.is_dir():
        return 0
    return sum(1 for child in path.rglob("*") if child.is_file())


def resource_payload_summary(
    path: Path,
    progress: Callable[[int, int | None, str], None] | None = None,
) -> dict[str, object]:
    if path.is_file():
        files = [path]
        root = path.parent
    elif path.is_dir():
        files = sorted(child for child in path.rglob("*") if child.is_file())
        root = path
    else:
        return {"file_count": 0, "bytes": 0, "sha256": ""}
    total_bytes = sum(child.stat().st_size for child in files)
    if not files or total_bytes <= 0:
        return {"file_count": 0, "bytes": 0, "sha256": ""}
    digest = hashlib.sha256()
    bytes_done = 0
    if progress:
        progress(0, total_bytes, "正在记录来源文件")
    for child in files:
        rel = child.name if path.is_file() else child.relative_to(root).as_posix()
        size = child.stat().st_size
        digest.update(rel.encode("utf-8"))
        digest.update(str(size).encode("ascii"))

        def file_progress(read_bytes: int, *, completed_before: int = bytes_done) -> None:
            if progress:
                progress(completed_before + read_bytes, total_bytes, f"正在校验 {rel}")

        digest.update(sha256_file_with_progress(child, file_progress).encode("ascii"))
        bytes_done += size
        if progress:
            progress(bytes_done, total_bytes, f"已记录 {rel}")
    return {"file_count": len(files), "bytes": total_bytes, "sha256": digest.hexdigest()}


def write_science_resource_manifests(
    source_catalog: Path,
    source_sync_plan: Path,
    science_store: Path,
    progress: Callable[[str, int, int | None, str], None] | None = None,
) -> dict[str, object]:
    manifest_dir = science_store / "manifests"
    manifest_dir.mkdir(parents=True, exist_ok=True)
    source_file_manifest = manifest_dir / "source_file_manifest.tsv"
    hash_manifest = manifest_dir / "hash_manifest.tsv"
    license_manifest = manifest_dir / "license_manifest.tsv"
    source_rows: list[dict[str, str]] = []
    for row in read_tsv(source_catalog):
        source_rows.append(
            {
                "source_id": row.get("source_id", ""),
                "source_label": row.get("source_id", ""),
                "source_class": "direct_download",
                "release_or_date": row.get("release_or_date", ""),
                "license_note": row.get("license_note", ""),
                "local_path": row.get("local_path", ""),
                "provenance": row.get("url", ""),
            }
        )
    for row in read_tsv(source_sync_plan):
        source_rows.append(
            {
                "source_id": row.get("source_id", ""),
                "source_label": row.get("source_label", row.get("source_id", "")),
                "source_class": row.get("source_class", "external_sync"),
                "release_or_date": row.get("release_or_date", ""),
                "license_note": row.get("license_note", ""),
                "local_path": row.get("ready_marker") or row.get("target_rel_path", ""),
                "provenance": row.get("source_url", ""),
            }
        )
    total = 0
    ready = 0
    hash_ready = 0
    license_ready = 0
    with source_file_manifest.open("w", encoding="utf-8", newline="") as source_handle, hash_manifest.open(
        "w", encoding="utf-8", newline=""
    ) as hash_handle, license_manifest.open("w", encoding="utf-8", newline="") as license_handle:
        source_writer = csv.DictWriter(
            source_handle,
            delimiter="\t",
            fieldnames=[
                "source_id",
                "source_label",
                "source_class",
                "local_path",
                "status",
                "file_count",
                "bytes",
                "sha256",
                "provenance",
            ],
        )
        hash_writer = csv.DictWriter(
            hash_handle,
            delimiter="\t",
            fieldnames=["source_id", "local_path", "status", "bytes", "sha256"],
        )
        license_writer = csv.DictWriter(
            license_handle,
            delimiter="\t",
            fieldnames=[
                "source_id",
                "source_label",
                "release_or_date",
                "license_note",
                "status",
            ],
        )
        source_writer.writeheader()
        hash_writer.writeheader()
        license_writer.writeheader()
        total_sources = len(source_rows)
        for index, row in enumerate(source_rows, start=1):
            source_id = row["source_id"] or "unknown_source"
            local_rel = row["local_path"]
            local_path = science_store / local_rel if local_rel else science_store / "__missing_local_path__"
            if progress:
                progress(source_id, index - 1, total_sources, f"正在记录来源 {index}/{total_sources}")

            def source_progress(bytes_done: int, bytes_total: int | None, message: str) -> None:
                if progress:
                    progress(source_id, bytes_done, bytes_total, f"{message} ({index}/{total_sources})")

            summary = resource_payload_summary(local_path, source_progress)
            file_count_value = int(summary["file_count"])
            bytes_count = int(summary["bytes"])
            digest = str(summary["sha256"])
            exists = file_count_value > 0 and bytes_count > 0
            status = "ready" if exists else "missing"
            license_status = "recorded" if row["license_note"] else "missing"
            total += 1
            if exists:
                ready += 1
            if digest:
                hash_ready += 1
            if license_status == "recorded":
                license_ready += 1
            common = {
                "source_id": source_id,
                "source_label": row["source_label"] or source_id,
                "source_class": row["source_class"],
                "local_path": local_rel,
                "status": status,
                "file_count": str(file_count_value),
                "bytes": str(bytes_count),
                "sha256": digest,
                "provenance": row["provenance"],
            }
            source_writer.writerow(common)
            hash_writer.writerow(
                {
                    "source_id": source_id,
                    "local_path": local_rel,
                    "status": "ready" if digest else "missing",
                    "bytes": str(bytes_count),
                    "sha256": digest,
                }
            )
            license_writer.writerow(
                {
                    "source_id": source_id,
                    "source_label": row["source_label"] or source_id,
                    "release_or_date": row["release_or_date"],
                    "license_note": row["license_note"],
                    "status": license_status,
                }
            )
    status = "ready" if total > 0 and ready == total and hash_ready == total and license_ready == total else "needs_repair"
    return {
        "status": status,
        "total": total,
        "ready": ready,
        "hash_ready": hash_ready,
        "license_ready": license_ready,
        "source_file_manifest": str(source_file_manifest),
        "hash_manifest": str(hash_manifest),
        "license_manifest": str(license_manifest),
    }


def download_summary(download_result: dict[str, object]) -> dict[str, object]:
    status = str(download_result.get("status", "unknown"))
    messages = {
        "ready": "科学资源下载步骤已完成。",
        "failed": "部分科学资源下载或校验失败。",
        "not_configured": "安装包未包含科学资源下载清单，科学仓库尚未准备完成。",
        "skipped": "本次已跳过下载步骤。",
    }
    actions = {
        "ready": "继续",
        "failed": "检查网络、磁盘空间后重新运行安装/修复",
        "not_configured": "使用包含科学资源清单的完整安装包重新运行安装/修复",
        "skipped": "如资源缺失，请重新运行安装/修复",
    }
    return {
        "status": status,
        "ready": download_result.get("ready", 0),
        "failed": download_result.get("failed", 0),
        "message": messages.get(status, "科学资源下载状态需要检查。"),
        "next_action": actions.get(status, "重新运行安装/修复"),
    }


def source_sync_summary(
    plan: Path,
    science_store: Path,
    sync_result: dict[str, object],
) -> dict[str, object]:
    rows = read_tsv(plan)
    if not rows:
        return {
            "status": "not_configured",
            "ready": 0,
            "total": 0,
            "needs_action": 0,
            "message": "安装包未包含大型科学资源同步清单。",
            "next_action": "使用完整安装包重新运行安装/修复",
            "items": [],
        }
    execution_rows = {
        row.get("source_id", ""): row for row in read_tsv(science_store / "external_sync_execution_manifest.tsv")
    }
    items: list[dict[str, object]] = []
    for row in rows:
        target_rel = row.get("ready_marker") or row.get("target_rel_path") or ""
        target = science_store / target_rel if target_rel else None
        ready = bool(target and path_has_payload(target))
        execution = execution_rows.get(row.get("source_id", ""))
        execution_status = execution.get("status", "") if execution else ""
        needs_bundle = execution_status == "requires_resource_bundle"
        items.append(
            {
                "label": row.get("source_label") or row.get("source_id") or "科学资源",
                "status": "就绪" if ready else "需要完整资源包" if needs_bundle else "需要同步",
                "ready": ready,
                "next_action": (
                    "使用包含该资源的完整安装包，或在同步清单中提供可直接读取的本地路径/下载地址"
                    if needs_bundle
                    else row.get("user_next_action") or "运行安装/修复以同步科学资源"
                ),
            }
        )
    ready_count = sum(1 for item in items if item["ready"])
    total = len(items)
    needs_action = total - ready_count
    failed = int(sync_result.get("failed", 0) or 0)
    return {
        "status": "ready" if total and needs_action == 0 else "failed" if failed else "needs_sync",
        "ready": ready_count,
        "total": total,
        "needs_action": needs_action,
        "message": "大型科学资源同步状态已检查并记录。",
        "next_action": (
            "继续"
            if needs_action == 0
            else "补齐完整资源包或同步清单后重新运行安装/修复"
        ),
        "items": items,
    }


def friendly_source_status(raw: str) -> str:
    return {
        "ready": "就绪",
        "downloaded_needs_index": "已下载，需重建索引",
        "missing": "需要准备",
        "download_required": "需要下载",
        "index_required": "需要重建索引",
        "waiting_for_download": "等待下载",
    }.get(raw, "需要检查")


def friendly_source_action(raw: str, status: str) -> str:
    if status == "ready":
        return "继续"
    return {
        "download_and_index_chembl": "准备 ChEMBL 活性数据",
        "download_and_index_bindingdb": "准备 BindingDB 活性数据",
        "download_and_index_uniprot_human": "准备 UniProt 人类蛋白索引",
        "download_and_index_rcsb_ligand_bound": "准备 RCSB 结构数据",
        "download_and_index_biolip2": "准备 BioLiP2 互作数据",
        "download_and_index_plinder": "准备 PLINDER 结构数据",
        "download_and_index_binding_moad": "准备 Binding MOAD 结构数据",
        "download_and_index_metagenome_sources": "准备宏基因组序列索引",
        "install_or_repair_scientific_tools": "准备计算工具",
        "build_known_database_index": "重建已知蛋白索引",
        "build_metagenome_potential_index": "重建宏基因组索引",
    }.get(raw, "准备科学资源")


def resource_summary(science_store: Path) -> dict[str, object]:
    rows = read_tsv(science_store / "source_requirements.tsv")
    items: list[dict[str, object]] = []
    for row in rows:
        raw_status = row.get("status", "")
        raw_action = row.get("repair_action", "")
        ready = raw_status == "ready"
        items.append(
            {
                "label": row.get("label", row.get("source_id", "科学资源")),
                "status": friendly_source_status(raw_status),
                "ready": ready,
                "next_action": friendly_source_action(raw_action, raw_status),
            }
        )
    ready_count = sum(1 for item in items if item["ready"])
    return {
        "status": "ready" if items and ready_count == len(items) else "needs_repair",
        "ready": ready_count,
        "total": len(items),
        "needs_action": len(items) - ready_count,
        "items": items,
    }


def tsv_has_data_rows(path: Path) -> bool:
    return path.exists() and bool(read_tsv(path))


def existing_science_indexes_ready(science_store: Path) -> bool:
    status_payload = load_json(science_store / "install_status.json")
    missing = status_payload.get("missing_components", [])
    if status_payload.get("status") != "ready" or (isinstance(missing, list) and missing):
        return False
    source_rows = read_tsv(science_store / "source_requirements.tsv")
    if not source_rows or any(row.get("status") != "ready" for row in source_rows):
        return False
    for rel_path in [
        "indexes/known_targets.tsv",
        "indexes/metagenome_targets.tsv",
        "indexes/structure_targets.tsv",
        "indexes/tool_versions.tsv",
    ]:
        if not tsv_has_data_rows(science_store / rel_path):
            return False
    return True


def install_ready(payload: dict[str, object]) -> bool:
    downloads = payload.get("downloads")
    external_sources = payload.get("external_sources")
    resources = payload.get("resources")
    tool_environment = payload.get("tool_environment")
    source_manifests = payload.get("source_manifests")
    alignment_databases = payload.get("alignment_databases")
    forcefields = payload.get("forcefields")
    download_status = downloads.get("status") if isinstance(downloads, dict) else None
    external_status = external_sources.get("status") if isinstance(external_sources, dict) else None
    resource_status = resources.get("status") if isinstance(resources, dict) else None
    tool_status = tool_environment.get("status") if isinstance(tool_environment, dict) else None
    source_manifest_status = source_manifests.get("status") if isinstance(source_manifests, dict) else None
    alignment_status = alignment_databases.get("status") if isinstance(alignment_databases, dict) else None
    forcefield_status = forcefields.get("status") if isinstance(forcefields, dict) else None
    download_ok = download_status == "ready" or (
        download_status == "skipped"
        and resource_status == "ready"
        and external_status == "ready"
        and source_manifest_status == "ready"
    )
    return (
        payload.get("status") == "ready"
        and resource_status == "ready"
        and external_status == "ready"
        and tool_status == "ready"
        and alignment_status == "ready"
        and forcefield_status == "ready"
        and download_ok
        and source_manifest_status == "ready"
    )


def source_record_summary(source_manifest_result: dict[str, object]) -> dict[str, object]:
    total = int(source_manifest_result.get("total", 0) or 0)
    ready = int(source_manifest_result.get("ready", 0) or 0)
    integrity_ready = int(source_manifest_result.get("hash_ready", 0) or 0)
    license_ready = int(source_manifest_result.get("license_ready", 0) or 0)
    completed = total > 0 and ready >= total and integrity_ready >= total and license_ready >= total
    return {
        "completed": completed,
        "ready": ready,
        "total": total,
        "integrity_ready": integrity_ready,
        "license_ready": license_ready,
        "record_sets": [
            {"label": "来源文件记录", "ready": ready, "total": total, "completed": total > 0 and ready >= total},
            {"label": "完整性记录", "ready": integrity_ready, "total": total, "completed": total > 0 and integrity_ready >= total},
            {"label": "许可证记录", "ready": license_ready, "total": total, "completed": total > 0 and license_ready >= total},
        ],
        "next_action": "继续" if completed else "运行安装/修复",
    }


def tool_environment_user_summary(tool_environment_result: dict[str, object]) -> dict[str, object]:
    status = str(tool_environment_result.get("status", "unknown"))
    ready = int(tool_environment_result.get("ready", 0) or 0)
    total = int(tool_environment_result.get("total", 0) or 0)
    return {
        "completed": status == "ready",
        "ready": ready,
        "total": total,
        "next_action": "继续" if status == "ready" else "运行安装/修复",
    }


def alignment_database_user_summary(alignment_result: dict[str, object]) -> dict[str, object]:
    status = str(alignment_result.get("status", "unknown"))
    ready = int(alignment_result.get("ready", 0) or 0)
    total = int(alignment_result.get("total", 0) or 0)
    deferred = int(alignment_result.get("deferred", 0) or 0)
    return {
        "completed": status == "ready",
        "ready": ready,
        "deferred": deferred,
        "total": total,
        "next_action": "继续" if status == "ready" else "运行安装/修复",
    }


def forcefield_user_summary(forcefield_result: dict[str, object]) -> dict[str, object]:
    status = str(forcefield_result.get("status", "unknown"))
    return {
        "completed": status == "ready",
        "ready": 1 if status == "ready" else 0,
        "total": 1,
        "next_action": "继续" if status == "ready" else "运行安装/修复",
    }


def runtime_user_summary(payload: dict[str, object]) -> dict[str, object]:
    binaries = payload.get("runtime_binaries")
    launch_environment = payload.get("launch_environment")
    if not isinstance(binaries, dict):
        binaries = {}
    if not isinstance(launch_environment, dict):
        launch_environment = {}
    def binary_ready(name: str) -> bool:
        item = binaries.get(name)
        return isinstance(item, dict) and item.get("status") == "ready"

    launcher = str(launch_environment.get("launcher", ""))
    return {
        "gls_cli_ready": binary_ready("gls_cli"),
        "gls_server_ready": binary_ready("gls_server"),
        "launcher": Path(launcher).name if launcher else "",
        "next_action": "使用 start-genoligand-webui 启动 WebUI",
    }


def install_user_summary(payload: dict[str, object]) -> dict[str, object]:
    return {
        "status": payload.get("status", "unknown"),
        "install_ready": bool(payload.get("install_ready", False)),
        "summary": payload.get("summary", ""),
        "next_action": payload.get("next_action", "运行安装/修复"),
        "access_url": payload.get("access_url", ""),
        "progress": payload.get("progress", {}),
        "downloads": payload.get("downloads", {}),
        "external_sources": payload.get("external_sources", {}),
        "resources": payload.get("resources", {}),
        "source_records": source_record_summary(payload.get("source_manifests", {}) if isinstance(payload.get("source_manifests"), dict) else {}),
        "tool_environment": tool_environment_user_summary(payload.get("tool_environment", {}) if isinstance(payload.get("tool_environment"), dict) else {}),
        "cuda_toolkit": payload.get("cuda_toolkit", {}),
        "alignment_databases": alignment_database_user_summary(payload.get("alignment_databases", {}) if isinstance(payload.get("alignment_databases"), dict) else {}),
        "forcefields": forcefield_user_summary(payload.get("forcefields", {}) if isinstance(payload.get("forcefields"), dict) else {}),
        "runtime": runtime_user_summary(payload),
    }


def installation_status(
    science_store: Path,
    completed: subprocess.CompletedProcess[str],
    download_result: dict[str, object],
    external_sync_result: dict[str, object],
    tool_environment_result: dict[str, object],
    alignment_database_result: dict[str, object],
    forcefield_result: dict[str, object],
    source_manifest_result: dict[str, object],
    external_source_summary: dict[str, object],
    resource_result: dict[str, object],
) -> dict[str, object]:
    status_payload = load_json(science_store / "install_status.json")
    store_status = str(status_payload.get("status", "")).strip()
    download_status = str(download_result.get("status", "unknown"))
    external_status = str(
        external_source_summary.get("status") or external_sync_result.get("status", "unknown")
    )
    resource_status = str(resource_result.get("status", "unknown"))
    tool_status = str(tool_environment_result.get("status", "unknown"))
    alignment_status = str(alignment_database_result.get("status", "unknown"))
    forcefield_status = str(forcefield_result.get("status", "unknown"))
    source_manifest_status = str(source_manifest_result.get("status", "unknown"))
    missing = status_payload.get("missing_components", [])
    missing_count = len(missing) if isinstance(missing, list) else 0
    if completed.returncode != 0:
        status = "failed"
        summary = "安装/修复未完成。"
        next_action = "检查安装环境后重新运行安装/修复"
    elif download_status == "not_configured":
        status = "needs_repair"
        summary = "安装包未包含科学资源下载清单，科学仓库尚未准备完成。"
        next_action = "使用包含科学资源清单的完整安装包重新运行安装/修复"
    elif download_status == "failed":
        status = "needs_repair"
        summary = "部分科学资源下载或校验失败，科学仓库尚未准备完成。"
        next_action = "检查网络、磁盘空间后重新运行安装/修复"
    elif (
        download_status == "skipped"
        and store_status == "ready"
        and missing_count == 0
        and resource_status == "ready"
        and external_status == "ready"
        and tool_status == "ready"
        and source_manifest_status == "ready"
    ):
        status = "ready"
        summary = "科学仓库已就绪。"
        next_action = "打开 WebUI 并输入研究对象"
    elif download_status == "skipped":
        status = "needs_repair"
        summary = "本次未执行科学资源下载，科学仓库不能标记为完成。"
        next_action = "保持联网后重新运行安装/修复"
    elif tool_status != "ready":
        status = "needs_repair"
        summary = "科学计算工具尚未准备完成。"
        next_action = "保持联网后重新运行安装/修复，或检查工具安装日志"
    elif alignment_status != "ready":
        status = "needs_repair"
        summary = "本地序列和结构比对数据库尚未准备完成。"
        next_action = "保持资源和工具可用后重新运行安装/修复"
    elif forcefield_status != "ready":
        status = "needs_repair"
        summary = "OpenFF 小分子力场文件尚未准备完成。"
        next_action = "保持联网后重新运行安装/修复"
    elif external_status == "needs_sync":
        status = "needs_repair"
        summary = "大型科学资源尚未闭合，科学仓库尚未准备完成。"
        next_action = "使用完整资源包，或在同步清单中提供可直接读取的本地路径/下载地址后重新运行安装/修复"
    elif external_status == "failed":
        status = "needs_repair"
        summary = "部分大型科学资源同步失败，科学仓库尚未准备完成。"
        next_action = "检查资源包、网络和磁盘空间后重新运行安装/修复"
    elif source_manifest_status != "ready":
        status = "needs_repair"
        summary = "科学资源来源、完整性或许可证记录尚未闭合。"
        next_action = "补齐资源文件、来源记录和许可证记录后重新运行安装/修复"
    elif store_status == "ready":
        status = "ready"
        summary = "科学仓库已就绪。"
        next_action = "打开 WebUI 并输入研究对象"
    else:
        status = "needs_repair"
        summary = "安装/修复已执行，但仍有资源需要准备。"
        next_action = "保持联网和足够磁盘空间后重新运行安装/修复"
    return {
        "status": status,
        "summary": summary,
        "next_action": next_action,
        "missing_components_count": missing_count,
    }


def main() -> int:
    configure_stdio()
    parser = argparse.ArgumentParser(
        description="Prepare or validate the GenoLigand local science store."
    )
    parser.add_argument("--workspace-root", default=str(repo_root()))
    parser.add_argument("--science-store", default=str(default_science_store()))
    parser.add_argument("--runtime-dir", default=str(default_runtime_dir()))
    parser.add_argument("--gls-cli")
    parser.add_argument("--gls-server")
    parser.add_argument("--source-catalog")
    parser.add_argument("--python-env-dir")
    parser.add_argument("--python-backend", choices=["auto", "local", "wsl"], default="auto")
    parser.add_argument("--wsl-distro")
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--no-tool-install", action="store_true")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default="19090")
    parser.add_argument("--start-webui", action="store_true")
    parser.add_argument("--debug-output", action="store_true")
    args = parser.parse_args()

    workspace_root = Path(args.workspace_root).resolve()
    science_store = Path(args.science_store).resolve()
    runtime_dir = Path(args.runtime_dir).resolve()
    runtime_dir.mkdir(parents=True, exist_ok=True)
    python_env_dir = (
        Path(args.python_env_dir).resolve()
        if args.python_env_dir
        else default_python_env_dir(runtime_dir).resolve()
    )
    prepend_process_path(host_toolchain_path_entries())
    install_lock = acquire_install_lock(science_store)
    write_install_progress(
        science_store,
        status="running",
        stage="准备安装",
        current_resource="安装/修复",
        completed_steps=0,
        total_steps=6,
        message="正在准备安装/修复",
    )
    selected_wsl = selected_project_wsl_distro(args.wsl_distro)
    python_backend = "local"
    if args.python_backend == "wsl":
        python_backend = "wsl"
    elif args.python_backend == "auto" and selected_wsl:
        python_backend = "wsl"
    command_wrapper_result: dict[str, object] = {"status": "not_used", "backend": python_backend}
    extra_tool_paths: list[Path] = []
    if python_backend == "wsl" and selected_wsl:
        wrapper_dir = python_env_dir / "command-wrappers"
        command_wrapper_result = write_wsl_command_wrappers(wrapper_dir, selected_wsl)
        extra_tool_paths.append(wrapper_dir)
    science_env = environment_for_science_tools(workspace_root, extra_tool_paths)
    source_catalog = Path(args.source_catalog).resolve() if args.source_catalog else default_source_catalog(workspace_root, science_store)
    source_sync_plan = default_source_sync_plan(workspace_root, science_store)
    download_result = (
        {
            "status": "skipped",
            "catalog": str(source_catalog),
            "manifest": "",
            "ready": 0,
            "failed": 0,
        }
        if args.no_download
        else prepare_downloads(source_catalog, science_store)
    )
    if args.no_download:
        write_install_progress(
            science_store,
            status="running",
            stage="下载数据库",
            current_resource="数据库下载",
            completed_steps=1,
            total_steps=6,
            message="本次跳过数据库下载",
        )
    external_sync_result = sync_external_sources(
        source_sync_plan,
        science_store,
        args.no_download,
        science_env,
    )
    write_install_progress(
        science_store,
        status="running",
        stage="记录来源",
        current_resource="来源、完整性和许可记录",
        completed_steps=3,
        total_steps=6,
        message="正在记录数据库来源和许可信息",
    )
    def source_manifest_progress(
        source_id: str,
        bytes_done: int,
        bytes_total: int | None,
        message: str,
    ) -> None:
        write_install_progress(
            science_store,
            status="running",
            stage="记录来源",
            current_resource=source_id,
            completed_steps=3,
            total_steps=6,
            bytes_downloaded=bytes_done,
            bytes_total=bytes_total,
            message=message,
        )

    source_manifest_result = write_science_resource_manifests(
        source_catalog,
        source_sync_plan,
        science_store,
        source_manifest_progress,
    )
    write_install_progress(
        science_store,
        status="running",
        stage="准备工具",
        current_resource="科学计算环境",
        completed_steps=4,
        total_steps=6,
        message="正在检查科学计算工具",
    )
    python_tool_environment_result = ensure_python_tool_environment(
        python_env_dir,
        args.no_tool_install,
        python_backend,
        selected_wsl,
    )
    write_install_progress(
        science_store,
        status="running",
        stage="准备工具",
        current_resource="命令行科学工具",
        completed_steps=4,
        total_steps=6,
        message="正在检查并补齐命令行科学工具",
    )
    command_tool_environment_result = ensure_command_tool_environment(
        workspace_root,
        python_env_dir,
        args.no_tool_install,
        python_backend,
        selected_wsl,
    )
    tool_environment_result = combine_tool_environment_result(
        python_tool_environment_result,
        command_tool_environment_result,
    )
    write_install_progress(
        science_store,
        status="running",
        stage="构建程序",
        current_resource="GenoLigand CLI",
        completed_steps=4,
        total_steps=6,
        message="正在构建优化版 GenoLigand 程序",
    )
    cli_command = resolve_binary_command(
        args.gls_cli,
        "GENOLIGAND_GLS_CLI",
        "gls-cli",
        "gls-cli",
        workspace_root,
    )
    cli_command, cli_runtime_status = publish_runtime_binary(cli_command, runtime_dir, "gls-cli")
    command = cli_command + [
        "install",
        "prepare-science-store",
        "--workspace-root",
        str(workspace_root),
        "--science-store",
        str(science_store),
    ]
    command_env = dict(science_env)
    tool_python = str(tool_environment_result.get("python", "")).strip()
    if tool_python:
        command_env["GENOLIGAND_PYTHON"] = tool_python
    write_install_progress(
        science_store,
        status="running",
        stage="构建索引",
        current_resource="科学仓库索引",
        completed_steps=5,
        total_steps=6,
        message="正在构建数据库索引",
    )
    if existing_science_indexes_ready(science_store):
        write_install_progress(
            science_store,
            status="running",
            stage="构建索引",
            current_resource="科学仓库索引",
            completed_steps=5,
            total_steps=6,
            message="已复用现有数据库索引",
        )
        completed = subprocess.CompletedProcess(command, 0, "science_indexes_reused\n", "")
    else:
        completed = run_command(command, env=command_env)
    write_install_progress(
        science_store,
        status="running",
        stage="构建索引",
        current_resource="序列和结构比对数据库",
        completed_steps=5,
        total_steps=6,
        message="正在构建本地 MMseqs2 和 Foldseek 目标数据库",
    )
    alignment_database_result = build_persistent_alignment_databases(science_store, command_env, selected_wsl)
    write_install_progress(
        science_store,
        status="running",
        stage="构建索引",
        current_resource="OpenFF 小分子力场",
        completed_steps=5,
        total_steps=6,
        message="正在准备 OpenFF SMIRNOFF 力场文件",
    )
    forcefield_result = ensure_openff_forcefield_file(science_store)
    external_source_summary = source_sync_summary(
        source_sync_plan,
        science_store,
        external_sync_result,
    )
    resource_result = resource_summary(science_store)
    install_status = installation_status(
        science_store,
        completed,
        download_result,
        external_sync_result,
        tool_environment_result,
        alignment_database_result,
        forcefield_result,
        source_manifest_result,
        external_source_summary,
        resource_result,
    )
    payload: dict[str, object] = {
        **install_status,
        "returncode": completed.returncode,
        "downloads": download_summary(download_result),
        "external_sources": external_source_summary,
        "source_manifests": source_manifest_result,
        "resources": resource_result,
        "tool_environment": tool_environment_result,
        "alignment_databases": alignment_database_result,
        "forcefields": forcefield_result,
        "cuda_toolkit": cuda_toolkit_summary(),
        "runtime_binaries": {
            "gls_cli": cli_runtime_status,
        },
        "runtime_backend": runtime_backend(),
        "access_url": f"http://{args.host}:{args.port}/",
    }
    payload["install_ready"] = install_ready(payload)
    final_progress_status = "ready" if payload["install_ready"] else "needs_repair"
    payload["progress"] = write_install_progress(
        science_store,
        status=final_progress_status,
        stage="完成检查",
        current_resource="安装/修复",
        completed_steps=6,
        total_steps=6,
        failed_steps=0 if payload["install_ready"] else 1,
        message=str(payload.get("summary", "")),
    )
    if args.debug_output:
        payload["debug"] = {
            "command": command,
            "stdout": completed.stdout.strip(),
            "stderr": completed.stderr.strip(),
            "workspace_root": str(workspace_root),
            "science_store": str(science_store),
            "runtime_dir": str(runtime_dir),
            "python_env_dir": str(python_env_dir),
            "python_backend": python_backend,
            "wsl_distro": selected_wsl or "",
            "command_wrappers": command_wrapper_result,
            "tool_path_entries": tool_path_entries(workspace_root, extra_tool_paths),
            "download_manifest": download_result.get("manifest", ""),
            "external_sync_manifest": external_sync_result.get("manifest", ""),
            "tool_environment_manifest": tool_environment_result.get("manifest", ""),
            "source_catalog": download_result.get("catalog", ""),
            "source_sync_plan": str(source_sync_plan),
        }
    exit_code = 0 if payload["install_ready"] else 2
    if completed.returncode != 0:
        exit_code = completed.returncode
    server_base_command = resolve_binary_command(
        args.gls_server,
        "GENOLIGAND_GLS_SERVER",
        "gls-server",
        "gls-server",
        workspace_root,
    )
    server_base_command, server_runtime_status = publish_runtime_binary(
        server_base_command,
        runtime_dir,
        "gls-server",
    )
    server_command = server_base_command + [
        "--host",
        args.host,
        "--port",
        str(args.port),
        "--artifact-dir",
        str(runtime_dir),
        "--workspace-root",
        str(workspace_root),
    ]
    launch_environment = write_runtime_launch_environment(
        runtime_dir,
        science_store,
        workspace_root,
        args.host,
        str(args.port),
        cli_command,
        server_command,
        tool_python,
        science_env,
    )
    payload["runtime_binaries"]["gls_server"] = server_runtime_status
    payload["launch_environment"] = launch_environment
    output_payload = payload if args.debug_output else install_user_summary(payload)
    print(json.dumps(output_payload, ensure_ascii=False, indent=2))
    release_install_lock(install_lock)
    install_lock = None
    if not args.start_webui:
        return exit_code
    if exit_code != 0:
        print(
            json.dumps(
                {
                    "status": "starting_webui_setup_required",
                    "summary": "安装尚未闭合，WebUI 将以修复状态启动。运行流程会保持阻塞，直到数据库、索引和工具准备完成。",
                    "next_action": payload.get("next_action", "在 WebUI 中运行安装/修复"),
                    "access_url": f"http://{args.host}:{args.port}/",
                },
                ensure_ascii=False,
                indent=2,
            )
        )

    env = dict(science_env)
    env["GENOLIGAND_SCIENCE_STORE"] = str(science_store)
    env["GENOLIGAND_GLS_CLI"] = command_env_value(cli_command)
    if tool_python:
        env["GENOLIGAND_PYTHON"] = tool_python
    print(
        json.dumps(
                {
                    "status": "starting_webui",
                    "summary": "WebUI 正在启动。",
                    "access_url": f"http://{args.host}:{args.port}/",
                    "launcher": Path(str(launch_environment.get("launcher", ""))).name,
                },
            ensure_ascii=False,
            indent=2,
        )
    )
    return subprocess.call(server_command, env=env)


if __name__ == "__main__":
    raise SystemExit(main())
