from __future__ import annotations

import argparse
import atexit
import base64
import concurrent.futures
import csv
import gzip
import hashlib
import http.client
import io
import json
import os
import platform
import re
import shutil
import sqlite3
import ssl
import subprocess
import sys
import tarfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zipfile
from contextlib import closing, contextmanager
from pathlib import Path
from typing import Callable, Sequence

PYTHON_TOOL_PACKAGES: list[dict[str, str]] = [
    {
        "component": "numpy",
        "package": "numpy==2.2.6",
        "import_name": "numpy",
        "version_expr": "numpy.__version__",
    },
    {
        "component": "pandas",
        "package": "pandas==2.2.3",
        "import_name": "pandas",
        "version_expr": "pandas.__version__",
    },
    {
        "component": "pyarrow",
        "package": "pyarrow==24.0.0",
        "import_name": "pyarrow",
        "version_expr": "pyarrow.__version__",
    },
    {
        "component": "jsonschema",
        "package": "jsonschema==4.26.0",
        "import_name": "jsonschema",
        "version_expr": "jsonschema.__version__",
    },
    {
        "component": "rdkit",
        "package": "rdkit==2025.9.6",
        "import_name": "rdkit",
        "version_expr": "rdkit.__version__",
    },
    {
        "component": "openmm",
        "package": "openmm==8.5.2",
        "import_name": "openmm",
        "version_expr": "openmm.version.version",
    },
    {
        "component": "openmmforcefields",
        "package": "openmmforcefields@6d91c7099e56e95e6f4b790f234889f663780b9c",
        "import_name": "openmmforcefields",
        "version_expr": "'installed'",
    },
    {
        "component": "pdbfixer",
        "package": "pdbfixer@5e658a4fb8d2b90d65ca8408c845cbf104ce7099",
        "import_name": "pdbfixer",
        "version_expr": "'installed'",
    },
    {
        "component": "openff_toolkit",
        "package": "openff-toolkit@f63576a20fe8014c6b1d4c008bd43774c66a35c0",
        "import_name": "openff.toolkit",
        "version_expr": "'installed'",
    },
    {
        "component": "openff_interchange",
        "package": "openff-interchange@190706c681e9564ea5ba82f0cc0de8a954e27f83",
        "import_name": "openff.interchange",
        "version_expr": "openff.interchange.__version__",
    },
    {
        "component": "openff_utilities",
        "package": "openff-utilities@ef92f3e661e4dcb1d7b8be80ec12352c0ff349b6",
        "import_name": "openff.utilities",
        "version_expr": "openff.utilities.__version__",
    },
    {
        "component": "openff_units",
        "package": "openff-units@17e8b6bf4e8c855ea22d2c9e9f57a57cb496c99e",
        "import_name": "openff.units",
        "version_expr": "openff.units.__version__",
    },
    {
        "component": "xmltodict",
        "package": "xmltodict==1.0.2",
        "import_name": "xmltodict",
        "version_expr": "getattr(xmltodict, '__version__', 'installed')",
    },
    {
        "component": "python_constraint",
        "package": "python-constraint==1.4.0",
        "import_name": "constraint",
        "version_expr": "'installed'",
    },
    {
        "component": "lxml",
        "package": "lxml==6.0.2",
        "import_name": "lxml",
        "version_expr": "'installed'",
    },
    {
        "component": "cachetools",
        "package": "cachetools==7.1.4",
        "import_name": "cachetools",
        "version_expr": "cachetools.__version__",
    },
    {
        "component": "networkx",
        "package": "networkx==3.6.1",
        "import_name": "networkx",
        "version_expr": "networkx.__version__",
    },
    {
        "component": "pydantic",
        "package": "pydantic==2.13.4",
        "import_name": "pydantic",
        "version_expr": "pydantic.__version__",
    },
    {
        "component": "mdtraj",
        "package": "mdtraj==1.11.1.post1",
        "import_name": "mdtraj",
        "version_expr": "mdtraj.__version__",
    },
    {
        "component": "mdanalysis",
        "package": "MDAnalysis==2.10.0",
        "import_name": "MDAnalysis",
        "version_expr": "MDAnalysis.__version__",
    },
    {
        "component": "openbabel",
        "package": "openbabel-wheel==3.1.1.22",
        "import_name": "openbabel",
        "version_expr": "'installed'",
    },
]

OPENMM_RUNTIME_VERSION = "8.5.2"
OPENMM_CUDA_VERSION = "8.5.2"
OPENMM_CUDA_PIP_PACKAGE = f"openmm-cuda-12=={OPENMM_CUDA_VERSION}"
OPENFF_FORCEFIELD_FILE = "openff-2.2.1.offxml"
OPENFF_FORCEFIELD_URL = (
    "https://raw.githubusercontent.com/openforcefield/openff-forcefields/"
    "3fabe581c3c0ca98ae662f1d3e265ff15cdcbca0/"
    "openforcefields/offxml/openff-2.2.1.offxml"
)
OPENFF_FORCEFIELD_SHA256 = "1b24deb47970bae2d179a5b4e023d4a57c9c78614fe431f1670e3f75e0012c3a"
OPENFF_FORCEFIELD_RELEASE = "openff-forcefields-2024.09.0"
ALIGNMENT_MANIFEST_SCHEMA_VERSION = "genoligand_alignment_database_manifest_v2"
SCIENCE_READINESS_SCHEMA_VERSION = "genoligand_science_store_readiness_v1"
SCIENCE_READINESS_EVIDENCE_PATHS = (
    "source_requirements.tsv",
    "manifests/source_file_manifest.tsv",
    "install_status.json",
    "index_build_manifest.tsv",
    "index_readiness_manifest.tsv",
    "science_index_readiness.tsv",
    "indexes/known_targets.tsv",
    "indexes/metagenome_targets.tsv",
    "indexes/structure_targets.tsv",
    "indexes/tool_versions.tsv",
    "indexes/mmseqs/mmseqs_target_databases.tsv",
    "indexes/foldseek/foldseek_target_databases.tsv",
    "downloads/rcsb/ligand_bound_pdb_scope.json",
    "downloads/rcsb/ligand_bound_pdb_scope.tsv",
)
RUNTIME_BINARY_MANIFEST_SCHEMA_VERSION = "genoligand_runtime_binary_manifest_v1"
SOURCE_FINGERPRINT_CACHE_SCHEMA_VERSION = "genoligand_source_fingerprint_cache_v1"
SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION = "genoligand_source_payload_validation_v1"
PYTHON_MIN_VERSION = (3, 12)
PYTHON_MAX_VERSION_EXCLUSIVE = (3, 13)
DISK_SAFETY_MARGIN_BYTES = 5 * 1024 * 1024 * 1024
RCSB_CAPACITY_REMOTE_SAMPLE_SIZE = 32
RCSB_CAPACITY_AGGREGATE_MARGIN_NUMERATOR = 3
RCSB_CAPACITY_AGGREGATE_MARGIN_DENOMINATOR = 2
REQUIRED_LOGICAL_SOURCES: dict[str, str] = {
    "chembl": "download_and_index_chembl",
    "bindingdb": "download_and_index_bindingdb",
    "uniprot_human_reviewed": "download_and_index_uniprot_human",
    "rcsb_ligand_bound": "download_and_index_rcsb_ligand_bound",
    "biolip2": "download_and_index_biolip2",
    "plinder": "download_and_index_plinder",
    "binding_moad": "download_and_index_binding_moad",
    "metagenome_potential": "download_and_index_metagenome_sources",
}
REQUIRED_TOOL_IDS = {
    "java",
    "nextflow",
    "rdkit",
    "openbabel",
    "mmseqs2",
    "foldseek",
    "autodock_vina",
    "openmm",
    "pdbfixer",
    "openff_toolkit",
    "mdtraj",
    "mdanalysis",
}
REQUIRED_INDEX_IDS = {"known_targets", "metagenome_targets", "structure_targets"}
DEFAULT_ALIGNMENT_DB_MAX_SOURCE_BYTES = 0
DEFAULT_PARTIAL_INDEX_LARGE_SOURCE_BYTES = 1 * 1024 * 1024 * 1024
DEFAULT_BINDINGDB_PARTIAL_INDEX_LARGE_SOURCE_BYTES = 500 * 1024 * 1024
RESERVED_WSL_DISTROS = {"Ubuntu-24.04"}
INSTALL_LOCK_HEARTBEAT_SECONDS = 15.0
INSTALL_LOCK_WAIT_SECONDS = 5.0
INSTALL_LOCK_INCOMPLETE_GRACE_SECONDS = 10.0
PROCESS_START_TOLERANCE_SECONDS = 2.0
DOWNLOAD_RETRIES = 3
DOWNLOAD_TIMEOUT_SECONDS = 120
DOWNLOAD_USER_AGENT = "GenoLigand-science-store-installer/0.9"
RCSB_DOWNLOAD_WORKERS = 20
ProgressCallback = Callable[[str, int, int | None, str], None]
ResourceSignature = tuple[str, int, int, int, int, int, str]
ResourceSummaryCache = dict[str, tuple[tuple[ResourceSignature, ...], dict[str, object]]]


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


class BinaryBuildError(RuntimeError):
    pass


def build_release_binary(root: Path, binary_name: str, cargo_package: str) -> Path | None:
    if not (root / "Cargo.toml").exists():
        return None
    cargo = ensure_cargo_available()
    if not cargo:
        raise BinaryBuildError(f"cannot build {binary_name}: cargo is unavailable")
    executable = f"{binary_name}.exe" if os.name == "nt" else binary_name
    binary = root / "target" / "release" / executable
    completed = run_command([cargo, "build", "--release", "-p", cargo_package])
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout or "cargo build failed").strip()
        raise BinaryBuildError(f"cannot build {binary_name} (exit {completed.returncode}): {detail}")
    if not binary.is_file():
        raise BinaryBuildError(f"cargo reported success but did not produce {binary}")
    return binary


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
    built = build_release_binary(root, binary_name, cargo_package)
    if built:
        return [str(built)]
    for candidate in [
        root / "bin" / executable,
        root / "tools" / "bin" / executable,
    ]:
        if candidate.exists():
            return [str(candidate)]
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


def runtime_instance_record_path(runtime_dir: Path, binary_name: str) -> Path:
    return runtime_dir / f"{binary_name}.instance.json"


def terminate_runtime_pid(pid: int) -> tuple[list[str], subprocess.CompletedProcess[str]]:
    if os.name == "nt":
        command = ["taskkill", "/PID", str(pid), "/T", "/F"]
    else:
        command = ["kill", "-TERM", "--", str(pid)]
    completed = subprocess.run(command, capture_output=True, text=True, check=False)
    return command, completed


def stop_runtime_binary_process(runtime_dir: Path, binary_name: str) -> dict[str, object]:
    record_path = runtime_instance_record_path(runtime_dir, binary_name)
    record = read_json_object(record_path)
    instance_id = str(record.get("instance_id", ""))
    recorded_binary = str(record.get("binary", ""))
    try:
        pid = int(record.get("pid", 0) or 0)
    except (TypeError, ValueError):
        pid = 0
    if not instance_id or recorded_binary != binary_name or pid <= 0 or pid == os.getpid():
        return {
            "status": "missing_instance_pid",
            "instance_id": instance_id,
            "pid": pid,
            "returncode": 1,
            "stdout": "",
            "stderr": "runtime instance record is missing or invalid",
        }
    if not process_identity_matches(pid, record.get("started_at")):
        return {
            "status": "stale_instance_record",
            "instance_id": instance_id,
            "pid": pid,
            "returncode": 1,
            "stdout": "",
            "stderr": "recorded runtime process no longer exists",
        }
    command, completed = terminate_runtime_pid(pid)
    return {
        "status": "stopped" if completed.returncode == 0 else "stop_failed",
        "instance_id": instance_id,
        "pid": pid,
        "command": command,
        "returncode": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
    }


def copy_runtime_binary(
    source: Path,
    target: Path,
    runtime_dir: Path,
    binary_name: str,
) -> dict[str, object] | None:
    try:
        shutil.copy2(source, target)
        return None
    except PermissionError:
        stop_result = stop_runtime_binary_process(runtime_dir, binary_name)
        if stop_result.get("returncode") != 0:
            raise
        time.sleep(1)
        shutil.copy2(source, target)
        return stop_result


def workspace_release_version(workspace_root: Path) -> str:
    cargo = workspace_root / "Cargo.toml"
    if not cargo.is_file():
        return ""
    text = cargo.read_text(encoding="utf-8")
    workspace = re.search(r"(?ms)^\[workspace\.package\].*?^version\s*=\s*\"([^\"]+)\"", text)
    if workspace:
        return workspace.group(1)
    package = re.search(r"(?ms)^\[package\].*?^version\s*=\s*\"([^\"]+)\"", text)
    return package.group(1) if package else ""


def workspace_commit(workspace_root: Path) -> str:
    completed = run_command(["git", "-C", str(workspace_root), "rev-parse", "HEAD"])
    value = completed.stdout.strip()
    return value if completed.returncode == 0 and re.fullmatch(r"[0-9a-f]{40}", value) else ""


def runtime_binary_smoke(
    command: list[str], expected_version: str, expected_commit: str
) -> dict[str, str]:
    try:
        completed = run_command([*command, "--version"])
    except (OSError, ValueError) as err:
        return {
            "status": "failed",
            "version": "",
            "commit": "",
            "notes": exception_note(err),
        }
    output = f"{completed.stdout}\n{completed.stderr}".strip()
    version_match = re.search(r"\b(\d+\.\d+\.\d+)\b", output)
    version = version_match.group(1) if version_match else ""
    commit_match = re.search(r"\b([0-9a-f]{40})\b", output.lower())
    commit = commit_match.group(1) if commit_match else ""
    ready = (
        completed.returncode == 0
        and bool(version)
        and bool(commit)
        and (not expected_version or version == expected_version)
        and (not expected_commit or commit == expected_commit)
    )
    return {
        "status": "ready" if ready else "failed",
        "version": version,
        "commit": commit,
        "notes": output.replace("\t", " ").replace("\n", " ")[:240],
    }


def publish_runtime_binary(
    command: list[str],
    runtime_dir: Path,
    binary_name: str,
    workspace_root: Path | None = None,
) -> tuple[list[str], dict[str, object]]:
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
            stop_result = copy_runtime_binary(source, target, runtime_dir, binary_name)
        try:
            target.chmod(target.stat().st_mode | 0o755)
        except OSError:
            pass
        published = [str(target)]
        status.update({"status": "ready", "published": True, "path": str(target), "command": published})
        status["sha256"] = sha256_file(target)
        status["build_profile"] = "release" if "release" in {part.lower() for part in source.parts} else "external"
        if workspace_root is not None:
            expected_version = workspace_release_version(workspace_root)
            expected_commit = workspace_commit(workspace_root)
            smoke = runtime_binary_smoke(published, expected_version, expected_commit)
            status.update(
                {
                    "status": "ready" if smoke["status"] == "ready" else "needs_repair",
                    "version": smoke["version"],
                    "expected_version": expected_version,
                    "commit": smoke["commit"],
                    "expected_commit": expected_commit,
                    "smoke": smoke["status"],
                    "smoke_notes": smoke["notes"],
                }
            )
        if stop_result:
            status["stopped_locked_process"] = stop_result
        return published, status
    status["status"] = "external_command"
    return command, status


def write_runtime_binary_manifest(
    runtime_dir: Path,
    rows: list[dict[str, object]],
    worker_wrapper: Path | None = None,
) -> dict[str, object]:
    records = [dict(row) for row in rows]
    if worker_wrapper is not None:
        exists = worker_wrapper.is_file() and worker_wrapper.stat().st_size > 0
        records.append(
            {
                "binary": "python_worker_wrapper",
                "status": "ready" if exists else "needs_repair",
                "path": str(worker_wrapper),
                "sha256": sha256_file(worker_wrapper) if exists else "",
                "version": "worker-v2",
                "expected_version": "worker-v2",
                "commit": records[0].get("commit", "") if records else "",
                "build_profile": "generated-wrapper",
                "smoke": "ready" if exists else "failed",
            }
        )
    required = {"gls-cli", "gls-server", "python_worker_wrapper"}
    by_name = {str(row.get("binary", "")): row for row in records}
    ready = all(
        name in by_name
        and by_name[name].get("status") == "ready"
        and by_name[name].get("sha256")
        and by_name[name].get("version")
        and by_name[name].get("commit")
        and by_name[name].get("commit") == by_name[name].get("expected_commit", by_name[name].get("commit"))
        and by_name[name].get("smoke") == "ready"
        for name in required
    )
    fieldnames = [
        "binary",
        "status",
        "path",
        "sha256",
        "version",
        "expected_version",
        "commit",
        "expected_commit",
        "build_profile",
        "smoke",
        "smoke_notes",
    ]
    manifest = runtime_dir / "runtime_binary_manifest.tsv"
    write_simple_tsv(
        manifest,
        fieldnames,
        [{key: str(value) for key, value in row.items()} for row in records],
    )
    digest = sha256_file(manifest)
    payload = {
        "schema_version": RUNTIME_BINARY_MANIFEST_SCHEMA_VERSION,
        "status": "ready" if ready else "needs_repair",
        "manifest": str(manifest),
        "manifest_digest": digest,
        "binaries": records,
    }
    write_json_atomic(runtime_dir / "runtime_binary_manifest.json", payload)
    return payload


def write_runtime_instance_record(
    runtime_dir: Path,
    binary_name: str,
    process: subprocess.Popen[object],
) -> tuple[Path, str]:
    instance_id = uuid.uuid4().hex
    record_path = runtime_instance_record_path(runtime_dir, binary_name)
    write_json_atomic(
        record_path,
        {
            "instance_id": instance_id,
            "binary": binary_name,
            "pid": process.pid,
            "started_at": process_start_time(process.pid) or time.time(),
            "created_at": time.time(),
        },
    )
    return record_path, instance_id


def clear_runtime_instance_record(record_path: Path, instance_id: str) -> None:
    current = read_json_object(record_path)
    if current.get("instance_id") != instance_id:
        return
    try:
        record_path.unlink()
    except FileNotFoundError:
        pass


def run_runtime_instance(
    command: list[str],
    runtime_dir: Path,
    binary_name: str,
    env: dict[str, str],
) -> int:
    process = subprocess.Popen(command, env=env)
    record_path, instance_id = write_runtime_instance_record(runtime_dir, binary_name, process)
    try:
        return process.wait()
    finally:
        clear_runtime_instance_record(record_path, instance_id)


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
    runtime_launcher = runtime_dir / "run-genoligand-webui.py"
    runtime_launcher.write_text(
        "\n".join(
            [
                "from __future__ import annotations",
                "",
                "import json",
                "import os",
                "import subprocess",
                "import time",
                "import uuid",
                "from pathlib import Path",
                "",
                f"command = {server_command!r}",
                f"configured_env = {env!r}",
                f"path_prepend = {path_prepend!r}",
                f"runtime_dir = Path({str(runtime_dir)!r})",
                "environment = os.environ.copy()",
                "environment.update(configured_env)",
                "environment['PATH'] = os.pathsep.join([*path_prepend, environment.get('PATH', '')])",
                "process = subprocess.Popen(command, env=environment)",
                "instance_id = uuid.uuid4().hex",
                "record_path = runtime_dir / 'gls-server.instance.json'",
                "record = {'instance_id': instance_id, 'binary': 'gls-server', 'pid': process.pid, 'started_at': time.time(), 'created_at': time.time()}",
                "temporary = record_path.with_name(f'.{record_path.name}.{instance_id}.tmp')",
                "temporary.write_text(json.dumps(record, indent=2) + '\\n', encoding='utf-8')",
                "os.replace(temporary, record_path)",
                "try:",
                "    returncode = process.wait()",
                "finally:",
                "    try:",
                "        current = json.loads(record_path.read_text(encoding='utf-8'))",
                "    except (OSError, json.JSONDecodeError):",
                "        current = {}",
                "    if current.get('instance_id') == instance_id:",
                "        record_path.unlink(missing_ok=True)",
                "raise SystemExit(returncode)",
                "",
            ]
        ),
        encoding="utf-8",
    )
    launch_payload["runtime_launcher"] = str(runtime_launcher)
    write_json_atomic(launch_json, launch_payload)
    launcher_command = [sys.executable, str(runtime_launcher)]
    if os.name == "nt":
        launcher = runtime_dir / "start-genoligand-webui.cmd"
        lines = ["@echo off", "setlocal"]
        lines.append(subprocess.list2cmdline(launcher_command))
        launcher.write_text("\r\n".join(lines) + "\r\n", encoding="utf-8")
    else:
        launcher = runtime_dir / "start-genoligand-webui.sh"
        lines = ["#!/usr/bin/env sh", "set -eu"]
        lines.append("exec " + " ".join(shell_quote(part) for part in launcher_command))
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


def gpu_md_required(explicit: bool = False) -> bool:
    configured = os.environ.get("GENOLIGAND_REQUIRE_GPU_MD", "").strip().lower()
    return explicit or configured in {"1", "true", "yes", "on"}


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
    if completed.returncode != 0:
        return []
    text = decode_process_bytes(completed.stdout)
    names = []
    for line in text.splitlines():
        name = line.replace("\x00", "").strip().lstrip("*").strip()
        if name and name.upper() != "NAME":
            names.append(name)
    return names


def reserved_wsl_distro(name: str) -> bool:
    normalized = name.strip().casefold()
    return any(normalized == reserved.casefold() for reserved in RESERVED_WSL_DISTROS)


def wsl_distribution_usable(distro: str) -> bool:
    completed = run_wsl_command(
        distro,
        ["bash", "-lc", "test -n \"$HOME\" && command -v sh >/dev/null 2>&1 && printf GENOLIGAND_WSL_READY"],
    )
    return completed.returncode == 0 and "GENOLIGAND_WSL_READY" in completed.stdout


def selected_project_wsl_distro(explicit: str | None) -> str | None:
    if os.name != "nt":
        return None
    names = list_wsl_distributions()
    requested = explicit or os.environ.get("GENOLIGAND_WSL_DISTRO", "")
    if requested:
        selected = next((name for name in names if name.casefold() == requested.casefold()), None)
        if selected and not reserved_wsl_distro(selected) and wsl_distribution_usable(selected):
            return selected
        return None
    for name in names:
        if not reserved_wsl_distro(name) and wsl_distribution_usable(name):
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


def python_tool_total(require_cuda: bool = False) -> int:
    return len(PYTHON_TOOL_PACKAGES) + int(require_cuda)


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
    "default_plugin_dir": "",
    "loaded_plugins": [],
    "plugin_load_failures": [],
    "failure_reason": "",
}
try:
    import openmm

    payload["openmm_ready"] = True
    payload["version"] = getattr(openmm.version, "version", getattr(openmm, "__version__", "installed"))
    candidates = []
    try:
        payload["default_plugin_dir"] = openmm.Platform.getDefaultPluginsDirectory()
        candidates.append(pathlib.Path(payload["default_plugin_dir"]))
    except Exception:
        pass
    try:
        candidates.extend(pathlib.Path(root) / "OpenMM.libs" / "lib" / "plugins" for root in site.getsitepackages())
    except Exception:
        pass
    try:
        candidates.append(pathlib.Path(openmm.__file__).resolve().parent.parent / "OpenMM.libs" / "lib" / "plugins")
    except Exception:
        pass
    seen = set()
    for candidate in candidates:
        text = str(candidate)
        if text in seen or not candidate.is_dir():
            continue
        seen.add(text)
        if any("cuda" in child.name.lower() for child in candidate.iterdir() if child.is_file()):
            payload["plugin_dir"] = text
        try:
            payload["loaded_plugins"].extend(openmm.Platform.loadPluginsFromDirectory(text))
        except Exception as exc:
            payload["plugin_load_failures"].append(f"{text}:{exc}")
    try:
        payload["plugin_load_failures"].extend(openmm.Platform.getPluginLoadFailures())
    except Exception:
        pass
    payload["plugin_load_failures"] = sorted(set(payload["plugin_load_failures"]))
    payload["platforms"] = [
        openmm.Platform.getPlatform(index).getName()
        for index in range(openmm.Platform.getNumPlatforms())
    ]
    payload["cuda_ready"] = any(name.lower() == "cuda" for name in payload["platforms"])
    if not payload["cuda_ready"]:
        payload["failure_reason"] = (
            "plugin_load_failures:" + "|".join(payload["plugin_load_failures"])
            if payload["plugin_load_failures"]
            else "CUDA_platform_missing"
        )
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
        "default_plugin_dir": "",
        "loaded_plugins": [],
        "plugin_load_failures": [],
        "failure_reason": next((line.strip() for line in text.splitlines() if line.strip()), "openmm_cuda_probe_failed"),
    }


def probe_local_openmm_cuda(python: Path) -> dict[str, object]:
    return parse_openmm_cuda_probe(run_command([str(python), "-c", OPENMM_CUDA_PROBE_CODE]))


def probe_wsl_openmm_cuda(distro: str, python_path: str) -> dict[str, object]:
    return parse_openmm_cuda_probe(run_wsl_command(distro, [python_path, "-c", OPENMM_CUDA_PROBE_CODE]))


def openmm_cuda_probe_ready(probe: dict[str, object]) -> bool:
    return bool(
        probe.get("openmm_ready")
        and probe.get("cuda_ready")
        and str(probe.get("version", "")) == OPENMM_CUDA_VERSION
        and str(probe.get("plugin_dir", ""))
        and not probe.get("plugin_load_failures")
    )


def openmm_cuda_notes(
    probe: dict[str, object],
    repair_action: str,
    runtime_notes: str = "",
) -> str:
    platforms = ",".join(str(item) for item in probe.get("platforms", []) if str(item))
    cuda_ready = openmm_cuda_probe_ready(probe)
    cuda = "ready" if cuda_ready else "missing"
    reason = "ready" if cuda_ready else str(probe.get("failure_reason") or "openmm_cuda_probe_incomplete")
    parts = [
        f"platforms={platforms}",
        f"cuda={cuda}",
        f"cuda_failure_reason={reason}",
        f"openmm_plugin_dir={probe.get('plugin_dir', '')}",
        f"openmm_default_plugin_dir={probe.get('default_plugin_dir', '')}",
        f"loaded_plugins={','.join(str(item) for item in probe.get('loaded_plugins', []))}",
        f"plugin_load_failures={'|'.join(str(item) for item in probe.get('plugin_load_failures', []))}",
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
    ready = openmm_cuda_probe_ready(probe)
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


def ensure_local_openmm_cuda(
    python: Path,
    skip_install: bool,
    required: bool = False,
) -> dict[str, object]:
    probe = probe_local_openmm_cuda(python)
    if not required:
        return openmm_cuda_manifest_row(str(python), probe, "not_required")
    repair_action = "hashed_cuda_lock_ready" if openmm_cuda_probe_ready(probe) else (
        "tool_install_skipped" if skip_install else "hashed_cuda_lock_did_not_create_cuda_context"
    )
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


def ensure_wsl_openmm_cuda(
    distro: str,
    python_path: str,
    wrapper: Path,
    skip_install: bool,
    required: bool = False,
) -> dict[str, object]:
    runtime_notes = probe_wsl_cuda_runtime(distro)
    probe = probe_wsl_openmm_cuda(distro, python_path)
    if not required:
        return openmm_cuda_manifest_row(str(wrapper), probe, "not_required", runtime_notes)
    repair_action = "hashed_cuda_lock_ready" if openmm_cuda_probe_ready(probe) else (
        "tool_install_skipped" if skip_install else "hashed_cuda_lock_did_not_create_cuda_context"
    )
    return openmm_cuda_manifest_row(str(wrapper), probe, repair_action, runtime_notes)


def ensure_python_tool_environment(
    env_dir: Path,
    skip_install: bool,
    backend: str = "local",
    wsl_distro: str | None = None,
    require_cuda: bool = False,
) -> dict[str, object]:
    if backend == "wsl":
        if not wsl_distro:
            return {
                "status": "failed",
                "backend": "wsl",
                "python": "",
                "manifest": str(env_dir / "tool_environment_manifest.tsv"),
                "ready": 0,
                "total": python_tool_total(require_cuda),
                "environment": "missing_wsl_distribution",
            }
        return ensure_wsl_python_tool_environment(env_dir, wsl_distro, skip_install, require_cuda)
    return ensure_local_python_tool_environment(env_dir, skip_install, require_cuda)


def python_version_supported(version: tuple[int, int] | Sequence[int]) -> bool:
    value = (int(version[0]), int(version[1]))
    return PYTHON_MIN_VERSION <= value < PYTHON_MAX_VERSION_EXCLUSIVE


def python_version_probe(command: list[str]) -> dict[str, object]:
    code = "import json,sys; print(json.dumps({'executable':sys.executable,'version':[sys.version_info.major,sys.version_info.minor]}))"
    completed = run_command([*command, "-c", code])
    if completed.returncode != 0:
        return {"supported": False, "reason": process_note(completed)}
    try:
        payload = json.loads(completed.stdout.strip().splitlines()[-1])
        version = tuple(int(item) for item in payload["version"][:2])
    except (IndexError, KeyError, TypeError, ValueError, json.JSONDecodeError):
        return {"supported": False, "reason": "python_version_probe_invalid"}
    return {
        "supported": python_version_supported(version),
        "version": f"{version[0]}.{version[1]}",
        "executable": str(payload.get("executable", "")),
        "reason": "supported" if python_version_supported(version) else "worker_python_3.12_required",
    }


def select_supported_local_python() -> Path | None:
    commands: list[list[str]] = []
    explicit = os.environ.get("GENOLIGAND_PYTHON", "").strip()
    if explicit:
        commands.append([explicit])
    commands.append([sys.executable])
    if os.name == "nt":
        commands.append(["py", "-3.12"])
    commands.append(["python3.12"])
    seen: set[tuple[str, ...]] = set()
    for command in commands:
        key = tuple(command)
        if key in seen:
            continue
        seen.add(key)
        probe = python_version_probe(command)
        executable = str(probe.get("executable", ""))
        if probe.get("supported") and executable:
            return Path(executable)
    return None


def worker_lock_catalog() -> dict[str, dict[str, str]]:
    path = repo_root() / "python/requirements-workers.lock.txt"
    rows = read_tsv(path)
    catalog = {row.get("artifact_id", ""): row for row in rows if row.get("artifact_id")}
    required = {
        "bootstrap",
        "runtime_windows",
        "runtime_manylinux",
        "cuda12_windows",
        "cuda12_manylinux",
    }
    if set(catalog) != required:
        raise ValueError("worker_lock_catalog_incomplete")
    return catalog


def validate_hashed_requirements(path: Path) -> None:
    entries: list[list[str]] = []
    current: list[str] = []
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped.startswith("--hash="):
            if not current:
                raise ValueError(f"requirements_lock_orphan_hash:{path.name}:{line_number}")
            current.append(stripped)
            continue
        if current:
            entries.append(current)
        current = [stripped]
    if current:
        entries.append(current)
    if not entries:
        raise ValueError(f"requirements_lock_empty:{path.name}")
    for entry in entries:
        requirement = entry[0].rstrip("\\").strip()
        if "git+" in requirement.lower() or re.search(r"(?:latest|head)(?:\W|$)", requirement, re.IGNORECASE):
            raise ValueError(f"requirements_lock_mutable_source:{path.name}:{requirement}")
        if not re.match(r"^[A-Za-z0-9_.-]+==[^\s;\\]+(?:\s*;.*)?$", requirement):
            raise ValueError(f"requirements_lock_unpinned:{path.name}:{requirement}")
        hashes = [item for line in entry for item in re.findall(r"--hash=sha256:([0-9a-f]{64})", line)]
        if not hashes:
            raise ValueError(f"requirements_lock_missing_hash:{path.name}:{requirement}")


def locked_requirement_version(path: Path, package: str) -> str:
    normalized = re.sub(r"[-_.]+", "-", package).lower()
    for raw in path.read_text(encoding="utf-8").splitlines():
        match = re.match(r"^([A-Za-z0-9_.-]+)==([^\s;\\]+)", raw.strip())
        if match and re.sub(r"[-_.]+", "-", match.group(1)).lower() == normalized:
            return match.group(2)
    return ""


def worker_platform_id() -> str:
    machine = platform.machine().strip().lower()
    if machine not in {"amd64", "x86_64"}:
        raise ValueError(f"worker_platform_unsupported_architecture:{machine or 'unknown'}")
    if os.name == "nt":
        return "windows_x86_64"
    libc_name, libc_version = platform.libc_ver()
    if libc_name.lower() != "glibc" or version_tuple(libc_version) < (2, 34):
        raise ValueError(f"worker_platform_requires_glibc_2_34:{libc_name}:{libc_version}")
    return "manylinux_2_34_x86_64"


def version_tuple(value: str) -> tuple[int, ...]:
    return tuple(int(item) for item in re.findall(r"\d+", value))


def worker_lock_paths(platform_id: str, python_version: str, require_cuda: bool) -> list[Path]:
    if python_version != "3.12":
        raise ValueError(f"worker_lock_unsupported_python:{python_version}")
    artifact_id = {
        "windows_x86_64": "runtime_windows",
        "manylinux_2_34_x86_64": "runtime_manylinux",
    }.get(platform_id)
    if artifact_id is None:
        raise ValueError(f"worker_lock_unsupported_platform:{platform_id}")
    catalog = worker_lock_catalog()
    selected = [catalog["bootstrap"], catalog[artifact_id]]
    if require_cuda:
        selected.append(catalog["cuda12_windows" if platform_id == "windows_x86_64" else "cuda12_manylinux"])
    paths: list[Path] = []
    for row in selected:
        path = repo_root() / "python" / row["file"]
        expected = row.get("sha256", "").lower()
        if not re.fullmatch(r"[0-9a-f]{64}", expected) or sha256_file(path).lower() != expected:
            raise ValueError(f"worker_lock_artifact_digest_mismatch:{path.name}")
        validate_hashed_requirements(path)
        paths.append(path)
    runtime_version = locked_requirement_version(paths[1], "openmm")
    if runtime_version != OPENMM_RUNTIME_VERSION:
        raise ValueError(f"worker_runtime_openmm_version_mismatch:{runtime_version or 'missing'}")
    if require_cuda:
        cuda_version = locked_requirement_version(paths[-1], "openmm-cuda-12")
        if cuda_version != runtime_version or cuda_version != OPENMM_CUDA_VERSION:
            raise ValueError(
                f"worker_cuda_openmm_version_mismatch:runtime={runtime_version}:cuda={cuda_version or 'missing'}"
            )
    return paths


def worker_source_locks() -> list[dict[str, str]]:
    rows = read_tsv(repo_root() / "python/requirements-workers-sources.lock.tsv")
    expected = {
        "openff-utilities",
        "openff-units",
        "openff-toolkit",
        "openff-interchange",
        "pdbfixer",
        "openmmforcefields",
    }
    if {row.get("package", "") for row in rows} != expected:
        raise ValueError("worker_source_lock_incomplete")
    for row in rows:
        if (
            not re.fullmatch(r"[a-z0-9-]+", row.get("package", ""))
            or not re.fullmatch(r"[0-9]+(?:\.[0-9]+)*(?:(?:a|b|rc|\.dev|\.post)[0-9]+)*", row.get("version", ""))
            or not row.get("git_url", "").startswith("https://github.com/")
            or not row.get("git_url", "").endswith(".git")
            or not re.fullmatch(r"[0-9a-f]{40}", row.get("commit", ""))
            or not re.fullmatch(r"[0-9a-f]{64}", row.get("archive_sha256", ""))
        ):
            raise ValueError(f"worker_source_lock_invalid:{row.get('package', 'unknown')}")
    return rows


def environment_lock_digest(platform_id: str, python_version: str, require_cuda: bool) -> str:
    paths = [
        repo_root() / "python/requirements-workers.lock.txt",
        repo_root() / "python/requirements-workers-sources.lock.tsv",
        *worker_lock_paths(platform_id, python_version, require_cuda),
    ]
    digest = hashlib.sha256()
    for path in paths:
        digest.update(path.name.encode("utf-8"))
        digest.update(path.read_bytes())
    return digest.hexdigest()


def locked_probe_ready(spec: dict[str, str], probe: dict[str, object]) -> bool:
    if not probe.get("ready"):
        return False
    match = re.search(r"==([A-Za-z0-9_.+-]+)", spec.get("package", ""))
    observed = str(probe.get("version", ""))
    return not match or observed == "installed" or observed == match.group(1)


def install_hashed_requirements_local(python: Path, locks: list[Path]) -> dict[str, str]:
    for lock in locks:
        completed = run_command(
            [
                str(python),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--require-hashes",
                "--requirement",
                str(lock),
            ]
        )
        if completed.returncode != 0:
            return {"status": "failed", "reason": f"hashed_lock_install_failed:{lock.name}:{process_note(completed)}"}
    return {"status": "ready", "reason": "hashed_lock_install_completed"}


def force_reinstall_hashed_cuda_lock_local(python: Path, lock: Path) -> dict[str, str]:
    completed = run_command(
        [
            str(python),
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--require-hashes",
            "--force-reinstall",
            "--no-deps",
            "--requirement",
            str(lock),
        ]
    )
    if completed.returncode != 0:
        return {"status": "failed", "reason": f"hashed_cuda_force_reinstall_failed:{lock.name}:{process_note(completed)}"}
    return {"status": "ready", "reason": "hashed_cuda_lock_force_reinstalled"}


def install_locked_sources_local(python: Path, env_dir: Path) -> dict[str, str]:
    git = shutil.which("git")
    if not git:
        return {"status": "failed", "reason": "git_required_for_locked_worker_sources"}
    build_root = env_dir / f"locked-source-build-{uuid.uuid4().hex}"
    try:
        for source in worker_source_locks():
            package = source["package"]
            repository = build_root / package / "repository"
            wheel_dir = build_root / package / "wheels"
            repository.mkdir(parents=True, exist_ok=True)
            wheel_dir.mkdir(parents=True, exist_ok=True)
            commands = [
                [git, "-C", str(repository), "init", "--quiet"],
                [git, "-C", str(repository), "remote", "add", "origin", source["git_url"]],
                [git, "-C", str(repository), "fetch", "--quiet", "--depth", "1", "origin", source["commit"]],
                [git, "-C", str(repository), "checkout", "--quiet", "--detach", source["commit"]],
                [git, "-C", str(repository), "tag", "--force", source["version"], source["commit"]],
            ]
            for command in commands:
                completed = run_command(command)
                if completed.returncode != 0:
                    return {"status": "failed", "reason": f"locked_source_checkout_failed:{package}:{process_note(completed)}"}
            observed = run_command([git, "-C", str(repository), "rev-parse", f"{source['commit']}^{{commit}}"])
            if observed.returncode != 0 or observed.stdout.strip() != source["commit"]:
                return {"status": "failed", "reason": f"locked_source_commit_mismatch:{package}"}
            archive = build_root / package / "source.tar"
            archived = run_command(
                [git, "-C", str(repository), "archive", "--format=tar", f"--output={archive}", source["commit"]]
            )
            if (
                archived.returncode != 0
                or not archive.is_file()
                or sha256_file(archive).lower() != source["archive_sha256"]
            ):
                return {"status": "failed", "reason": f"locked_source_archive_digest_mismatch:{package}"}
            built = run_command(
                [
                    str(python),
                    "-m",
                    "pip",
                    "wheel",
                    "--disable-pip-version-check",
                    "--no-deps",
                    "--no-build-isolation",
                    "--wheel-dir",
                    str(wheel_dir),
                    str(repository),
                ]
            )
            wheels = list(wheel_dir.glob("*.whl"))
            if built.returncode != 0 or len(wheels) != 1:
                return {"status": "failed", "reason": f"locked_source_wheel_build_failed:{package}:{process_note(built)}"}
            wheel = wheels[0]
            expected_wheel_prefix = f"{package.replace('-', '_')}-{source['version'].replace('-', '_')}-".lower()
            if not wheel.name.lower().startswith(expected_wheel_prefix):
                return {"status": "failed", "reason": f"locked_source_wheel_version_mismatch:{package}:{wheel.name}"}
            requirement = build_root / package / "wheel-requirement.txt"
            with atomic_text_writer(requirement) as handle:
                handle.write(f"{package} @ {wheel.resolve().as_uri()} --hash=sha256:{sha256_file(wheel)}\n")
            installed = run_command(
                [
                    str(python),
                    "-m",
                    "pip",
                    "install",
                    "--disable-pip-version-check",
                    "--require-hashes",
                    "--no-deps",
                    "--requirement",
                    str(requirement),
                ]
            )
            if installed.returncode != 0:
                return {"status": "failed", "reason": f"locked_source_wheel_install_failed:{package}:{process_note(installed)}"}
        return {"status": "ready", "reason": "locked_source_wheels_installed"}
    finally:
        shutil.rmtree(build_root, ignore_errors=True)


def install_hashed_requirements_wsl(
    distro: str,
    python_path: str,
    locks: list[Path],
) -> dict[str, str]:
    for lock in locks:
        completed = run_wsl_command(
            distro,
            [
                python_path,
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--require-hashes",
                "--requirement",
                wsl_path(lock),
            ],
        )
        if completed.returncode != 0:
            return {"status": "failed", "reason": f"hashed_lock_install_failed:{lock.name}:{process_note(completed)}"}
    return {"status": "ready", "reason": "hashed_lock_install_completed"}


def force_reinstall_hashed_cuda_lock_wsl(
    distro: str,
    python_path: str,
    lock: Path,
) -> dict[str, str]:
    completed = run_wsl_command(
        distro,
        [
            python_path,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--require-hashes",
            "--force-reinstall",
            "--no-deps",
            "--requirement",
            wsl_path(lock),
        ],
    )
    if completed.returncode != 0:
        return {"status": "failed", "reason": f"hashed_cuda_force_reinstall_failed:{lock.name}:{process_note(completed)}"}
    return {"status": "ready", "reason": "hashed_cuda_lock_force_reinstalled"}


def install_locked_sources_wsl(
    distro: str,
    python_path: str,
    home: str,
) -> dict[str, str]:
    build_root = f"{home}/.local/state/genoligand/locked-source-build-{uuid.uuid4().hex}"
    created = run_wsl_command(
        distro,
        ["bash", "-lc", 'mkdir -p -- "$1"', "create-locked-source-root", build_root],
    )
    if created.returncode != 0:
        return {"status": "failed", "reason": f"locked_source_build_root_failed:{process_note(created)}"}
    try:
        for source in worker_source_locks():
            package = source["package"]
            package_root = f"{build_root}/{package}"
            repository = f"{package_root}/repository"
            wheel_dir = f"{package_root}/wheels"
            prepared = run_wsl_command(
                distro,
                ["bash", "-lc", 'mkdir -p -- "$1" "$2"', "prepare-locked-source", repository, wheel_dir],
            )
            if prepared.returncode != 0:
                return {"status": "failed", "reason": f"locked_source_prepare_failed:{package}"}
            commands = [
                ["git", "-C", repository, "init", "--quiet"],
                ["git", "-C", repository, "remote", "add", "origin", source["git_url"]],
                ["git", "-C", repository, "fetch", "--quiet", "--depth", "1", "origin", source["commit"]],
                ["git", "-C", repository, "checkout", "--quiet", "--detach", source["commit"]],
                ["git", "-C", repository, "tag", "--force", source["version"], source["commit"]],
            ]
            for command in commands:
                completed = run_wsl_command(distro, command)
                if completed.returncode != 0:
                    return {"status": "failed", "reason": f"locked_source_checkout_failed:{package}:{process_note(completed)}"}
            observed = run_wsl_command(
                distro,
                ["git", "-C", repository, "rev-parse", f"{source['commit']}^{{commit}}"],
            )
            if observed.returncode != 0 or observed.stdout.strip() != source["commit"]:
                return {"status": "failed", "reason": f"locked_source_commit_mismatch:{package}"}
            archive = f"{package_root}/source.tar"
            archived = run_wsl_command(
                distro,
                ["git", "-C", repository, "archive", "--format=tar", f"--output={archive}", source["commit"]],
            )
            digest_code = (
                "import hashlib,pathlib,sys; "
                "print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())"
            )
            digest = run_wsl_command(distro, [python_path, "-c", digest_code, archive])
            if (
                archived.returncode != 0
                or digest.returncode != 0
                or digest.stdout.strip().lower() != source["archive_sha256"]
            ):
                return {"status": "failed", "reason": f"locked_source_archive_digest_mismatch:{package}"}
            built = run_wsl_command(
                distro,
                [
                    python_path,
                    "-m",
                    "pip",
                    "wheel",
                    "--disable-pip-version-check",
                    "--no-deps",
                    "--no-build-isolation",
                    "--wheel-dir",
                    wheel_dir,
                    repository,
                ],
            )
            found = run_wsl_command(
                distro,
                ["bash", "-lc", 'find "$1" -maxdepth 1 -type f -name "*.whl" -print', "find-locked-wheel", wheel_dir],
            )
            wheels = [line.strip() for line in found.stdout.splitlines() if line.strip()]
            if built.returncode != 0 or found.returncode != 0 or len(wheels) != 1:
                return {"status": "failed", "reason": f"locked_source_wheel_build_failed:{package}:{process_note(built)}"}
            wheel = wheels[0]
            expected_wheel_prefix = f"{package.replace('-', '_')}-{source['version'].replace('-', '_')}-".lower()
            if not wheel.rsplit("/", 1)[-1].lower().startswith(expected_wheel_prefix):
                return {"status": "failed", "reason": f"locked_source_wheel_version_mismatch:{package}:{wheel}"}
            wheel_digest = run_wsl_command(distro, [python_path, "-c", digest_code, wheel])
            if wheel_digest.returncode != 0 or not re.fullmatch(r"[0-9a-f]{64}", wheel_digest.stdout.strip()):
                return {"status": "failed", "reason": f"locked_source_wheel_digest_failed:{package}"}
            requirement = f"{package_root}/wheel-requirement.txt"
            requirement_code = (
                "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text(sys.argv[2]+'\\n',encoding='utf-8')"
            )
            requirement_line = f"{package} @ file://{wheel} --hash=sha256:{wheel_digest.stdout.strip()}"
            written = run_wsl_command(
                distro,
                [python_path, "-c", requirement_code, requirement, requirement_line],
            )
            installed = run_wsl_command(
                distro,
                [
                    python_path,
                    "-m",
                    "pip",
                    "install",
                    "--disable-pip-version-check",
                    "--require-hashes",
                    "--no-deps",
                    "--requirement",
                    requirement,
                ],
            )
            if written.returncode != 0 or installed.returncode != 0:
                return {"status": "failed", "reason": f"locked_source_wheel_install_failed:{package}:{process_note(installed)}"}
        return {"status": "ready", "reason": "locked_source_wheels_installed"}
    finally:
        run_wsl_command(
            distro,
            ["bash", "-lc", 'rm -rf -- "$1"', "cleanup-locked-source-root", build_root],
        )


def ensure_local_python_tool_environment(
    env_dir: Path,
    skip_install: bool,
    require_cuda: bool = False,
) -> dict[str, object]:
    manifest = env_dir / "tool_environment_manifest.tsv"
    python = venv_python(env_dir)
    env_dir.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, object]] = []
    create_status = "existing" if python.exists() else "pending"
    base_python = select_supported_local_python()
    if skip_install:
        create_status = "skipped"
        python = Path(os.environ.get("GENOLIGAND_PYTHON", sys.executable))
    elif not python.exists():
        if base_python is None:
            return {
                "status": "failed",
                "backend": "local",
                "python": "",
                "manifest": str(manifest),
                "ready": 0,
                "total": python_tool_total(require_cuda),
                "environment": "supported_python_3_12_missing",
            }
        created = run_command([str(base_python), "-m", "venv", str(env_dir)])
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
                    "total": python_tool_total(require_cuda),
                }
        create_status = "created"
    version_probe = python_version_probe([str(python)])
    if not version_probe.get("supported"):
        write_python_tool_manifest(
            manifest,
            [
                {
                    "component": "python_worker_env",
                    "status": "failed",
                    "python": str(python),
                    "package": "python==3.12.*",
                    "import_name": "sys",
                    "version": str(version_probe.get("version", "unknown")),
                    "notes": str(version_probe.get("reason", "unsupported_python")),
                }
            ],
        )
        return {
            "status": "failed",
            "backend": "local",
            "python": str(python),
            "manifest": str(manifest),
            "ready": 0,
            "total": python_tool_total(require_cuda),
            "environment": "unsupported_python",
        }
    platform_id = ""
    lock_digest = ""
    installation = {"status": "skipped", "reason": "tool_install_skipped"}
    if not skip_install:
        try:
            platform_id = worker_platform_id()
            python_version = str(version_probe["version"])
            locks = worker_lock_paths(platform_id, python_version, require_cuda)
            lock_digest = environment_lock_digest(platform_id, python_version, require_cuda)
        except (OSError, UnicodeError, ValueError) as err:
            return {
                "status": "failed",
                "backend": "local",
                "python": str(python),
                "manifest": str(manifest),
                "ready": 0,
                "total": python_tool_total(require_cuda),
                "environment": f"worker_lock_validation_failed:{err}",
            }
        initial = [
            (spec, probe_python_import(python, spec["import_name"], spec["version_expr"]))
            for spec in PYTHON_TOOL_PACKAGES
        ]
        initial_cuda = probe_local_openmm_cuda(python) if require_cuda else {}
        lock_state = read_json_object(env_dir / "worker_environment_lock.json")
        current = bool(
            all(locked_probe_ready(spec, item) for spec, item in initial)
            and (not require_cuda or openmm_cuda_probe_ready(initial_cuda))
            and lock_state.get("schema_version") == "genoligand_worker_environment_lock_v1"
            and lock_state.get("lock_digest") == lock_digest
            and lock_state.get("platform") == platform_id
            and lock_state.get("python") == python_version
            and lock_state.get("profile") == ("cuda" if require_cuda else "cpu")
        )
        if current:
            installation = {"status": "ready", "reason": "worker_environment_lock_reused"}
        else:
            installation = install_hashed_requirements_local(python, locks)
            if (
                installation["status"] == "ready"
                and require_cuda
                and not openmm_cuda_probe_ready(probe_local_openmm_cuda(python))
            ):
                installation = force_reinstall_hashed_cuda_lock_local(python, locks[-1])
            if installation["status"] == "ready":
                installation = install_locked_sources_local(python, env_dir)
    ready = 0
    for spec in PYTHON_TOOL_PACKAGES:
        probe = probe_python_import(python, spec["import_name"], spec["version_expr"])
        exact = locked_probe_ready(spec, probe)
        status = "ready" if exact else "missing"
        notes = str(probe["notes"])
        if installation["status"] == "failed":
            notes = installation["reason"]
        if probe.get("ready") and not exact:
            notes = f"locked_version_mismatch:{probe.get('version', '')}"
        if not exact and not skip_install:
            status = "failed"
        if exact:
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
    cuda_row = ensure_local_openmm_cuda(python, skip_install, require_cuda)
    if require_cuda and cuda_row["status"] == "ready":
        ready += 1
    rows.append(cuda_row)
    write_python_tool_manifest(manifest, rows)
    total = python_tool_total(require_cuda)
    if not skip_install and ready == total and installation["status"] == "ready":
        write_json_atomic(
            env_dir / "worker_environment_lock.json",
            {
                "schema_version": "genoligand_worker_environment_lock_v1",
                "status": "ready",
                "profile": "cuda" if require_cuda else "cpu",
                "platform": platform_id,
                "python": str(version_probe["version"]),
                "lock_digest": lock_digest,
                "source_commits": {row["package"]: row["commit"] for row in worker_source_locks()},
            },
        )
    return {
        "status": (
            "ready"
            if ready == total and (skip_install or installation["status"] == "ready")
            else "needs_repair" if not skip_install else "skipped"
        ),
        "backend": "local",
        "python": str(python),
        "manifest": str(manifest),
        "ready": ready,
        "total": total,
        "environment": create_status,
        "lock_digest": lock_digest,
    }


def ensure_wsl_python_tool_environment(
    env_dir: Path,
    distro: str,
    skip_install: bool,
    require_cuda: bool = False,
) -> dict[str, object]:
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
                    "total": python_tool_total(require_cuda),
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
            "total": python_tool_total(require_cuda),
            "environment": "skipped",
        }
    if not skip_install:
        run_wsl_root_shell(
            distro,
            (
                "set -u; "
                "if command -v apt-get >/dev/null 2>&1; then "
                "apt-get update; "
                "for pkg in python3.12 python3.12-venv git ca-certificates curl unzip; do apt-get install -y \"$pkg\" || true; done; "
                "elif command -v dnf >/dev/null 2>&1; then "
                "dnf install -y python3.12 git ca-certificates curl unzip || true; "
                "elif command -v yum >/dev/null 2>&1; then "
                "yum install -y python3.12 git ca-certificates curl unzip || true; "
                "elif command -v pacman >/dev/null 2>&1; then "
                "pacman -Sy --noconfirm python git ca-certificates curl unzip || true; "
                "fi"
            ),
        )
        create = run_wsl_command(
            distro,
            [
                "bash",
                "-lc",
                "if [ ! -x ~/.local/state/genoligand/python-worker-env/bin/python ]; then python3.12 -m venv ~/.local/state/genoligand/python-worker-env; fi",
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
                    "total": python_tool_total(require_cuda),
                    "environment": "failed",
                }
    version_code = (
        "import json,platform,sys; libc=platform.libc_ver(); payload={"
        "'supported':sys.version_info[:2]==(3,12) and platform.machine().lower() in {'x86_64','amd64'} "
        "and libc[0].lower()=='glibc' and tuple(int(x) for x in libc[1].split('.')[:2]) >= (2,34),"
        "'version':[sys.version_info.major,sys.version_info.minor],"
        "'machine':platform.machine(),'libc':list(libc)}; print(json.dumps(payload))"
    )
    version_completed = run_wsl_command(distro, [python_path, "-c", version_code])
    try:
        version_payload = json.loads(version_completed.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        version_payload = {"supported": False, "version": []}
    if version_completed.returncode != 0 or not version_payload.get("supported"):
        write_python_tool_manifest(
            manifest,
            [
                {
                    "component": "python_worker_env",
                    "status": "failed",
                    "python": str(wrapper),
                    "package": "python==3.12.*; platform_machine=='x86_64'; glibc>=2.34",
                    "import_name": "sys",
                    "version": ".".join(str(item) for item in version_payload.get("version", [])),
                    "notes": "worker_python_3.12_manylinux_2_34_x86_64_required",
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
            "total": python_tool_total(require_cuda),
            "environment": "unsupported_python",
        }
    platform_id = "manylinux_2_34_x86_64"
    python_version = ".".join(str(item) for item in version_payload.get("version", [])[:2])
    lock_digest = ""
    installation = {"status": "skipped", "reason": "tool_install_skipped"}
    if not skip_install:
        try:
            locks = worker_lock_paths(platform_id, python_version, require_cuda)
            lock_digest = environment_lock_digest(platform_id, python_version, require_cuda)
        except (OSError, UnicodeError, ValueError) as err:
            return {
                "status": "failed",
                "backend": "wsl",
                "wsl_distro": distro,
                "python": str(wrapper),
                "manifest": str(manifest),
                "ready": 0,
                "total": python_tool_total(require_cuda),
                "environment": f"worker_lock_validation_failed:{err}",
            }
        initial = [
            (spec, probe_wsl_python_import(distro, python_path, spec["import_name"], spec["version_expr"]))
            for spec in PYTHON_TOOL_PACKAGES
        ]
        initial_cuda = probe_wsl_openmm_cuda(distro, python_path) if require_cuda else {}
        lock_state = read_json_object(env_dir / "worker_environment_lock.json")
        current = bool(
            all(locked_probe_ready(spec, item) for spec, item in initial)
            and (not require_cuda or openmm_cuda_probe_ready(initial_cuda))
            and lock_state.get("schema_version") == "genoligand_worker_environment_lock_v1"
            and lock_state.get("lock_digest") == lock_digest
            and lock_state.get("platform") == platform_id
            and lock_state.get("python") == python_version
            and lock_state.get("profile") == ("cuda" if require_cuda else "cpu")
        )
        if current:
            installation = {"status": "ready", "reason": "worker_environment_lock_reused"}
        else:
            installation = install_hashed_requirements_wsl(distro, python_path, locks)
            if (
                installation["status"] == "ready"
                and require_cuda
                and not openmm_cuda_probe_ready(probe_wsl_openmm_cuda(distro, python_path))
            ):
                installation = force_reinstall_hashed_cuda_lock_wsl(distro, python_path, locks[-1])
            if installation["status"] == "ready":
                installation = install_locked_sources_wsl(distro, python_path, home)
    ready = 0
    rows: list[dict[str, object]] = []
    for spec in PYTHON_TOOL_PACKAGES:
        probe = probe_wsl_python_import(distro, python_path, spec["import_name"], spec["version_expr"])
        exact = locked_probe_ready(spec, probe)
        status = "ready" if exact else "missing"
        notes = str(probe["notes"])
        if installation["status"] == "failed":
            notes = installation["reason"]
        if probe.get("ready") and not exact:
            notes = f"locked_version_mismatch:{probe.get('version', '')}"
        if not exact and not skip_install:
            status = "failed"
        if exact:
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
    cuda_row = ensure_wsl_openmm_cuda(distro, python_path, wrapper, skip_install, require_cuda)
    if require_cuda and cuda_row["status"] == "ready":
        ready += 1
    rows.append(cuda_row)
    write_python_tool_manifest(manifest, rows)
    total = python_tool_total(require_cuda)
    if not skip_install and ready == total and installation["status"] == "ready":
        write_json_atomic(
            env_dir / "worker_environment_lock.json",
            {
                "schema_version": "genoligand_worker_environment_lock_v1",
                "status": "ready",
                "profile": "cuda" if require_cuda else "cpu",
                "platform": platform_id,
                "python": python_version,
                "lock_digest": lock_digest,
                "source_commits": {row["package"]: row["commit"] for row in worker_source_locks()},
            },
        )
    return {
        "status": "ready" if ready == total and (skip_install or installation["status"] == "ready") else "needs_repair",
        "backend": "wsl",
        "wsl_distro": distro,
        "python": str(wrapper),
        "manifest": str(manifest),
        "ready": ready,
        "total": total,
        "environment": "created_or_existing",
        "lock_digest": lock_digest,
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
    write_simple_tsv(
        path,
        ["component", "status", "python", "package", "import_name", "version", "notes"],
        [{key: str(value) for key, value in row.items()} for row in rows],
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
    write_json_atomic(science_store / "install_progress.json", payload)
    return payload


def process_exists(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        try:
            import ctypes
            from ctypes import wintypes

            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
            kernel32.OpenProcess.restype = wintypes.HANDLE
            kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
            kernel32.CloseHandle.restype = wintypes.BOOL
            handle = kernel32.OpenProcess(0x1000, False, pid)
            if handle:
                kernel32.CloseHandle(handle)
                return True
            return ctypes.get_last_error() == 5
        except (AttributeError, OSError, ValueError):
            return False
    try:
        os.kill(pid, 0)
        return True
    except PermissionError:
        return True
    except OSError:
        return False


def process_start_time(pid: int) -> float | None:
    if pid <= 0:
        return None
    if os.name == "nt":
        try:
            import ctypes
            from ctypes import wintypes

            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
            kernel32.OpenProcess.restype = wintypes.HANDLE
            kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
            kernel32.CloseHandle.restype = wintypes.BOOL
            filetime_pointer = ctypes.POINTER(wintypes.FILETIME)
            kernel32.GetProcessTimes.argtypes = [
                wintypes.HANDLE,
                filetime_pointer,
                filetime_pointer,
                filetime_pointer,
                filetime_pointer,
            ]
            kernel32.GetProcessTimes.restype = wintypes.BOOL
            handle = kernel32.OpenProcess(0x1000, False, pid)
            if not handle:
                return None
            try:
                created = wintypes.FILETIME()
                exited = wintypes.FILETIME()
                kernel = wintypes.FILETIME()
                user = wintypes.FILETIME()
                if not kernel32.GetProcessTimes(
                    handle,
                    ctypes.byref(created),
                    ctypes.byref(exited),
                    ctypes.byref(kernel),
                    ctypes.byref(user),
                ):
                    return None
                ticks = (created.dwHighDateTime << 32) | created.dwLowDateTime
                return (ticks - 116_444_736_000_000_000) / 10_000_000
            finally:
                kernel32.CloseHandle(handle)
        except (AttributeError, OSError, ValueError):
            return None
    proc_stat = Path(f"/proc/{pid}/stat")
    proc_system = Path("/proc/stat")
    if not proc_stat.is_file() or not proc_system.is_file():
        return None
    try:
        fields = proc_stat.read_text(encoding="utf-8").rsplit(")", 1)[1].split()
        start_ticks = int(fields[19])
        boot_time = next(
            int(line.split()[1])
            for line in proc_system.read_text(encoding="utf-8").splitlines()
            if line.startswith("btime ")
        )
        return boot_time + start_ticks / int(os.sysconf("SC_CLK_TCK"))
    except (IndexError, OSError, StopIteration, ValueError):
        return None


def process_identity_matches(pid: int, started_at: object) -> bool:
    if not process_exists(pid):
        return False
    try:
        expected = float(started_at or 0)
    except (TypeError, ValueError):
        expected = 0
    actual = process_start_time(pid)
    if expected <= 0 or actual is None:
        return True
    return abs(actual - expected) <= PROCESS_START_TOLERANCE_SECONDS


def read_json_object(path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return value if isinstance(value, dict) else {}


def write_json_atomic(path: Path, payload: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(
        f".{path.name}.{os.getpid()}.{threading.get_ident()}.{uuid.uuid4().hex}.tmp"
    )
    try:
        with temporary.open("x", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


@contextmanager
def atomic_text_writer(path: Path):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(
        f".{path.name}.{os.getpid()}.{threading.get_ident()}.{uuid.uuid4().hex}.tmp"
    )
    try:
        with temporary.open("x", encoding="utf-8", newline="") as handle:
            yield handle
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        attempt = path.with_name(f"{path.name}.attempt.json")
        try:
            attempt.unlink()
        except FileNotFoundError:
            pass
    except BaseException as err:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
        write_json_atomic(
            path.with_name(f"{path.name}.attempt.json"),
            {
                "status": "failed",
                "manifest": path.name,
                "error": str(err)[:500],
                "failed_at": int(time.time()),
            },
        )
        raise


def write_text_atomic(path: Path, value: str) -> None:
    with atomic_text_writer(path) as handle:
        handle.write(value)


def create_json_new(path: Path, payload: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
    except BaseException:
        try:
            path.unlink()
        except FileNotFoundError:
            pass
        raise


def install_lock_is_stale(payload: dict[str, object], lock_path: Path | None = None) -> bool:
    try:
        pid = int(payload.get("pid", 0) or 0)
    except (TypeError, ValueError):
        pid = 0
    if pid > 0:
        if not process_exists(pid):
            return True
        if not payload.get("owner_id"):
            return False
        return not process_identity_matches(pid, payload.get("started_at"))
    if lock_path is None:
        return True
    try:
        age = max(0.0, time.time() - lock_path.stat().st_mtime)
    except FileNotFoundError:
        return True
    return age >= INSTALL_LOCK_INCOMPLETE_GRACE_SECONDS


def reclaim_stale_install_lock(lock_path: Path) -> bool:
    claim_path = lock_path.with_name(f"{lock_path.name}.reclaim")
    owner_id = uuid.uuid4().hex
    started_at = process_start_time(os.getpid()) or time.time()
    claim = {
        "owner_id": owner_id,
        "pid": os.getpid(),
        "started_at": started_at,
        "heartbeat_at": time.time(),
    }
    try:
        create_json_new(claim_path, claim)
    except FileExistsError:
        existing_claim = read_json_object(claim_path)
        if install_lock_is_stale(existing_claim, claim_path):
            try:
                claim_path.unlink()
            except FileNotFoundError:
                pass
        return False
    try:
        current = read_json_object(lock_path)
        if not install_lock_is_stale(current, lock_path):
            return False
        try:
            lock_path.unlink()
        except FileNotFoundError:
            pass
        return True
    finally:
        current_claim = read_json_object(claim_path)
        if current_claim.get("owner_id") == owner_id:
            try:
                claim_path.unlink()
            except FileNotFoundError:
                pass


class InstallLock:
    def __init__(self, path: Path, owner_id: str) -> None:
        self.path = path
        self.owner_id = owner_id
        self._stopped = threading.Event()
        self._released = False
        self._release_guard = threading.Lock()
        self._thread = threading.Thread(target=self._heartbeat, name="genoligand-install-lock", daemon=True)
        self._thread.start()
        atexit.register(self.release)

    def _heartbeat(self) -> None:
        while not self._stopped.wait(max(0.01, INSTALL_LOCK_HEARTBEAT_SECONDS)):
            current = read_json_object(self.path)
            if current.get("owner_id") != self.owner_id:
                return
            current["heartbeat_at"] = time.time()
            try:
                write_json_atomic(self.path, current)
            except OSError:
                continue

    def release(self) -> None:
        with self._release_guard:
            if self._released:
                return
            self._released = True
            self._stopped.set()
            if threading.current_thread() is not self._thread:
                self._thread.join()
            current = read_json_object(self.path)
            if current.get("owner_id") != self.owner_id:
                return
            try:
                self.path.unlink()
            except FileNotFoundError:
                pass


def acquire_install_lock(science_store: Path) -> InstallLock:
    lock_path = science_store / "install.lock.json"
    science_store.mkdir(parents=True, exist_ok=True)
    owner_id = uuid.uuid4().hex
    started_at = process_start_time(os.getpid()) or time.time()
    while True:
        heartbeat_at = time.time()
        payload = {
            "owner_id": owner_id,
            "pid": os.getpid(),
            "started_at": started_at,
            "heartbeat_at": heartbeat_at,
            "command": " ".join(sys.argv),
        }
        try:
            create_json_new(lock_path, payload)
            return InstallLock(lock_path, owner_id)
        except FileExistsError:
            existing = read_json_object(lock_path)
            if install_lock_is_stale(existing, lock_path) and reclaim_stale_install_lock(lock_path):
                continue
            time.sleep(max(0.01, INSTALL_LOCK_WAIT_SECONDS))


def release_install_lock(lock: InstallLock | Path | None) -> None:
    if not lock:
        return
    if isinstance(lock, InstallLock):
        lock.release()
        return
    lock_path = lock
    existing = read_json_object(lock_path)
    existing_pid = int(existing.get("pid", 0) or 0)
    if existing_pid == os.getpid() and process_identity_matches(existing_pid, existing.get("started_at")):
        try:
            lock_path.unlink()
        except FileNotFoundError:
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


def file_digest(path: Path, algorithm: str) -> str:
    digest = hashlib.new(algorithm)
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


def partial_identity_path(partial: Path) -> Path:
    return partial.with_name(f"{partial.name}.identity.json")


def destination_identity_path(destination: Path) -> Path:
    return destination.with_name(f"{destination.name}.identity.json")


def remote_resource_identity(url: str) -> dict[str, object]:
    if not url.lower().startswith(("http://", "https://")):
        return {}
    request = urllib.request.Request(url, headers={"User-Agent": DOWNLOAD_USER_AGENT}, method="HEAD")
    try:
        with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
            headers = response.headers
            length_text = str(headers.get("Content-Length", "")).strip()
            length = int(length_text) if length_text.isdigit() else 0
            etag = str(headers.get("ETag", "")).strip()
            last_modified = str(headers.get("Last-Modified", "")).strip()
            accept_ranges = str(headers.get("Accept-Ranges", "")).strip().lower()
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, OSError, ValueError):
        return {}
    trusted = bool(etag and length > 0) or bool(last_modified and length > 0)
    identity_payload = {
        "url": url,
        "etag": etag,
        "last_modified": last_modified,
        "content_length": length,
        "accept_ranges": accept_ranges,
    }
    identity_payload["identity_digest"] = hashlib.sha256(
        json.dumps(identity_payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    identity_payload["trusted"] = trusted
    return identity_payload


def write_partial_identity(partial: Path, identity: dict[str, object]) -> None:
    if identity.get("trusted"):
        write_json_atomic(partial_identity_path(partial), identity)


def partial_identity_matches(partial: Path, identity: dict[str, object]) -> bool:
    stored = read_json_object(partial_identity_path(partial))
    return bool(
        identity.get("trusted")
        and stored.get("identity_digest")
        and stored.get("identity_digest") == identity.get("identity_digest")
    )


def clear_partial_identity(partial: Path) -> None:
    try:
        partial_identity_path(partial).unlink()
    except FileNotFoundError:
        pass


def copy_or_download(
    url: str,
    destination: Path,
    expected_sha256: str | None,
    progress: ProgressCallback | None = None,
    *,
    observed_sha256: str | None = None,
    observed_bytes: int | None = None,
    validator: Callable[[Path], dict[str, object]] | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_suffix(destination.suffix + ".part")
    cleanup_partial_quarantines(partial)
    local_source: Path | None = None
    declared_local_source: Path | None = None
    if url.startswith("file://"):
        declared_local_source = Path(urllib.request.url2pathname(url[7:]))
        if declared_local_source.is_file():
            local_source = declared_local_source
    else:
        candidate = Path(url)
        if candidate.is_file():
            local_source = candidate
    remote_identity = remote_resource_identity(url) if local_source is None else {}
    observed_trusted = bool(observed_sha256) and observed_bytes is not None and observed_bytes > 0
    trusted_sha256 = expected_sha256 or (observed_sha256 if observed_trusted else None)
    trusted_bytes = observed_bytes if not expected_sha256 and observed_trusted else None
    if expected_sha256 is None and local_source is not None:
        trusted_sha256 = sha256_file(local_source)
        trusted_bytes = local_source.stat().st_size
    if trusted_bytes is None and remote_identity.get("trusted"):
        trusted_bytes = int(remote_identity.get("content_length", 0) or 0) or None
    if destination.exists() and destination.stat().st_size > 0:
        if progress:
            progress(
                "verifying",
                destination.stat().st_size,
                destination.stat().st_size,
                "正在校验已下载资源",
            )
        cached_summary = resource_payload_summary(destination, cache=summary_cache)
        actual_sha256 = str(cached_summary.get("content_sha256", "")) or sha256_file(destination)
        bytes_match = trusted_bytes is None or destination.stat().st_size == trusted_bytes
        published_identity = read_json_object(destination_identity_path(destination))
        identity_match = bool(
            remote_identity.get("trusted")
            and bytes_match
            and published_identity.get("identity_digest") == remote_identity.get("identity_digest")
            and published_identity.get("content_sha256") == actual_sha256
        )
        digest_match = bool(trusted_sha256 and actual_sha256.lower() == trusted_sha256.lower())
        if bytes_match and (digest_match or identity_match):
            validation = validator(destination) if validator is not None else {"status": "ready"}
            if validation.get("status") != "ready":
                actual_sha256 = ""
            else:
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
                    "sha256": sha256_file(destination),
                    "reused": True,
                    "etag": str(remote_identity.get("etag", "")),
                    "content_length": int(remote_identity.get("content_length", 0) or 0),
                    "resource_identity": str(remote_identity.get("identity_digest", "")),
                    "validation": validation,
                }
        if progress:
            progress(
                "retrying",
                destination.stat().st_size,
                destination.stat().st_size,
                "已下载资源校验不一致，正在重新下载",
            )
    if declared_local_source is not None and local_source is None:
        raise FileNotFoundError(f"local resource source is missing: {declared_local_source}")
    if local_source is not None:
        source = local_source
        total = source.stat().st_size if source.exists() else None
        if progress:
            progress("copying", 0, total, "正在复制本地资源")
        with source.open("rb") as source_handle, partial.open("wb") as target_handle:
            copy_with_progress(source_handle, target_handle, bytes_total=total, progress=progress)
    else:
        partial_ready_for_resume = bool(
            partial.exists()
            and partial.stat().st_size > 0
            and (
                partial_identity_matches(partial, remote_identity)
                if remote_identity.get("trusted")
                else trusted_sha256
            )
        )
        if partial.exists() and partial.stat().st_size > 0 and not partial_ready_for_resume:
            quarantine_partial(partial)
        if remote_identity.get("trusted"):
            write_partial_identity(partial, remote_identity)
        resumed = partial.exists() and partial.stat().st_size > 0
        outcome = download_http_resource(
            url,
            partial,
            resumed,
            trusted_sha256,
            trusted_bytes,
            progress,
            remote_identity,
        )
        if outcome == "retry_full":
            if remote_identity.get("trusted"):
                write_partial_identity(partial, remote_identity)
            outcome = download_http_resource(
                url,
                partial,
                False,
                trusted_sha256,
                trusted_bytes,
                progress,
                remote_identity,
            )
        if outcome != "downloaded":
            raise OSError(f"resource download failed: {url}")
    if progress:
        progress("verifying", partial.stat().st_size if partial.exists() else 0, partial.stat().st_size if partial.exists() else None, "正在校验资源")
    if not partial.is_file() or partial.stat().st_size <= 0:
        if progress:
            progress("failed", 0, trusted_bytes, "下载资源为空")
        return {"status": "empty_download", "bytes": 0, "sha256": ""}
    actual_sha256 = sha256_file(partial)
    bytes_match = trusted_bytes is None or partial.stat().st_size == trusted_bytes
    if trusted_sha256 and (not bytes_match or actual_sha256.lower() != trusted_sha256.lower()):
        quarantine_partial(partial)
        if progress:
            progress("failed", 0, trusted_bytes, "完整性校验失败")
        return {
            "status": "checksum_failed",
            "bytes": 0,
            "sha256": actual_sha256,
        }
    if not bytes_match:
        quarantine_partial(partial)
        return {"status": "length_failed", "bytes": 0, "sha256": actual_sha256}
    validation = validator(partial) if validator is not None else {"status": "ready", "reason": "not_required"}
    if validation.get("status") != "ready":
        quarantine_partial(partial)
        return {
            "status": "validation_failed",
            "bytes": 0,
            "sha256": actual_sha256,
            "validation": validation,
        }
    partial.replace(destination)
    clear_partial_identity(partial)
    if remote_identity.get("trusted"):
        write_json_atomic(
            destination_identity_path(destination),
            {**remote_identity, "content_sha256": actual_sha256},
        )
    seed_resource_file_fingerprint(summary_cache, destination, actual_sha256)
    if progress:
        progress("completed", destination.stat().st_size, destination.stat().st_size, "资源已完成")
    return {
        "status": "ready",
        "bytes": destination.stat().st_size,
        "sha256": actual_sha256,
        "reused": False,
        "etag": str(remote_identity.get("etag", "")),
        "content_length": int(remote_identity.get("content_length", 0) or 0),
        "resource_identity": str(remote_identity.get("identity_digest", "")),
        "validation": validation,
    }


def quarantine_partial(partial: Path) -> Path | None:
    if not partial.exists():
        return None
    partial.unlink()
    clear_partial_identity(partial)
    cleanup_partial_quarantines(partial)
    return None


def cleanup_partial_quarantines(partial: Path) -> None:
    for quarantine in partial.parent.glob(f"{partial.name}.quarantine-*"):
        if quarantine.is_file():
            quarantine.unlink()


def download_http_resource(
    url: str,
    partial: Path,
    resume: bool,
    trusted_sha256: str | None,
    trusted_bytes: int | None,
    progress: ProgressCallback | None,
    resource_identity: dict[str, object] | None = None,
) -> str:
    last_error: Exception | None = None
    for attempt in range(1, DOWNLOAD_RETRIES + 1):
        existing = partial.stat().st_size if resume and partial.exists() else 0
        if not resume and partial.exists():
            partial.unlink()
        if progress:
            progress("downloading", existing, None, f"正在下载数据库资源，第 {attempt} 次尝试")
        request = urllib.request.Request(url, headers={"User-Agent": DOWNLOAD_USER_AGENT})
        if existing:
            request.add_header("Range", f"bytes={existing}-")
            if resource_identity:
                if_range = str(resource_identity.get("etag") or resource_identity.get("last_modified") or "")
                if if_range:
                    request.add_header("If-Range", if_range)
        try:
            with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
                status = getattr(response, "status", 200)
                mode = "ab" if existing and status == 206 else "wb"
                content_length = response.headers.get("Content-Length")
                response_bytes = int(content_length) if content_length and content_length.isdigit() else None
                total = existing + response_bytes if status == 206 and response_bytes is not None else response_bytes
                start = existing if status == 206 else 0
                if status == 206:
                    content_range = str(response.headers.get("Content-Range", ""))
                    match = re.fullmatch(r"bytes (\d+)-(\d+)/(\d+|\*)", content_range)
                    if not match or int(match.group(1)) != existing:
                        quarantine_partial(partial)
                        return "retry_full"
                    if trusted_bytes is not None and match.group(3).isdigit() and int(match.group(3)) != trusted_bytes:
                        quarantine_partial(partial)
                        return "retry_full"
                expected_etag = str((resource_identity or {}).get("etag", ""))
                response_etag = str(response.headers.get("ETag", ""))
                if expected_etag and response_etag and response_etag != expected_etag:
                    quarantine_partial(partial)
                    return "retry_full"
                with partial.open(mode) as handle:
                    copy_with_progress(
                        response,
                        handle,
                        bytes_done=start,
                        bytes_total=total,
                        progress=progress,
                    )
            if trusted_bytes is not None and partial.stat().st_size < trusted_bytes:
                if resource_identity and resource_identity.get("trusted"):
                    resume = True
                    continue
                return "failed"
            if trusted_bytes is not None and partial.stat().st_size > trusted_bytes:
                quarantine_partial(partial)
                return "retry_full"
            if resume and not download_matches(partial, trusted_sha256, trusted_bytes):
                quarantine_partial(partial)
                return "retry_full"
            return "downloaded"
        except urllib.error.HTTPError as err:
            if existing and err.code == 416:
                if download_matches(partial, trusted_sha256, trusted_bytes):
                    return "downloaded"
                quarantine_partial(partial)
                return "retry_full"
            last_error = err
        except (urllib.error.URLError, TimeoutError, OSError, http.client.IncompleteRead) as err:
            last_error = err
            if partial.is_file() and partial.stat().st_size > 0 and resource_identity and resource_identity.get("trusted"):
                resume = True
        if progress:
            progress("retrying", partial.stat().st_size if partial.exists() else 0, None, "下载中断，准备重试")
        if attempt < DOWNLOAD_RETRIES:
            time.sleep(min(2 * attempt, 5))
    if last_error is not None and download_with_external_tool(url, partial, progress):
        if (trusted_sha256 or trusted_bytes is not None) and not download_matches(
            partial, trusted_sha256, trusted_bytes
        ):
            quarantine_partial(partial)
            return "retry_full" if resume else "failed"
        if partial.is_file() and partial.stat().st_size > 0:
            return "downloaded"
    return "failed"


def download_matches(path: Path, trusted_sha256: str | None, trusted_bytes: int | None) -> bool:
    if not path.is_file() or path.stat().st_size <= 0:
        return False
    if trusted_bytes is not None and path.stat().st_size != trusted_bytes:
        return False
    return not trusted_sha256 or sha256_file(path).lower() == trusted_sha256.lower()


def source_validation_result(
    row: dict[str, str],
    status: str,
    reason: str,
    records: int = 0,
) -> dict[str, object]:
    return {
        "schema_version": SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION,
        "source_id": row.get("source_id", ""),
        "status": status,
        "reason": reason,
        "records": records,
        "release_identity": row.get("release_identity", ""),
        "trust_source": row.get("trust_source", ""),
    }


def catalog_release_error(row: dict[str, str]) -> str:
    required = ("source_id", "url", "local_path", "release_or_date", "release_identity", "trust_source", "media_type", "compression")
    missing = [field for field in required if not row.get(field, "").strip()]
    if missing:
        return f"catalog_release_fields_missing:{','.join(missing)}"
    expected_sha256 = row.get("sha256", "").strip().lower()
    if expected_sha256 and not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
        return "catalog_sha256_invalid"
    source_id = row["source_id"].strip()
    url = urllib.parse.unquote_plus(row["url"].strip())
    local_path = row["local_path"].strip()
    release = row["release_or_date"].strip()
    identity = row["release_identity"].strip()
    media_type = row["media_type"].strip().lower()
    compression = row["compression"].strip().lower()
    if source_id == "chembl":
        match = re.fullmatch(r"chembl_(\d+)_sqlite", identity, re.IGNORECASE)
        if (
            not match
            or release.upper() != f"CHEMBL{match.group(1)}"
            or f"/chembl_{match.group(1)}/" not in url.lower()
            or media_type != "sqlite_tar"
            or compression not in {"tar.gz", "tgz"}
        ):
            return "release_identity_mismatch:chembl"
    elif source_id == "bindingdb":
        match = re.fullmatch(r"BindingDB_All_(\d{6})_tsv", identity)
        expected_release = f"{match.group(1)[:4]}-{match.group(1)[4:]}" if match else ""
        if (
            not match
            or release != expected_release
            or f"BindingDB_All_{match.group(1)}_tsv.zip" not in url
            or f"bindingdb_all.tsv.zip" not in local_path.lower()
            or media_type != "bindingdb_tsv"
            or compression != "zip"
        ):
            return "release_identity_mismatch:bindingdb"
    elif source_id == "uniprot_human_reviewed":
        match = re.fullmatch(r"(UP\d{9})_reviewed_stream", identity)
        lowered = url.lower()
        if (
            not match
            or f"proteome:{match.group(1).lower()}" not in lowered
            or "reviewed:true" not in lowered
            or media_type != "fasta"
            or compression != "gz"
        ):
            return "release_identity_mismatch:uniprot_human_reviewed"
    elif source_id == "binding_moad":
        match = re.fullmatch(r"BindingMOAD_git_([0-9a-f]{40})", identity)
        date_match = re.search(r"_(\d{1,2})_(\d{1,2})_(\d{4})\.csv(?:$|\?)", url, re.IGNORECASE)
        embedded_date = (
            f"{date_match.group(3)}-{int(date_match.group(1)):02d}-{int(date_match.group(2)):02d}"
            if date_match
            else ""
        )
        if (
            not match
            or f"/{match.group(1)}/" not in url
            or release != embedded_date
            or media_type != "csv"
            or compression not in {"none", "uncompressed"}
        ):
            return "release_identity_mismatch:binding_moad"
    return ""


def safe_archive_member(name: str) -> bool:
    normalized = name.replace("\\", "/")
    return bool(normalized and not normalized.startswith("/") and ".." not in normalized.split("/"))


def normalized_header(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", value.strip().lower()).strip("_")


def numeric_measurement(value: str) -> float | None:
    match = re.search(r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?", value)
    if not match:
        return None
    parsed = float(match.group(0))
    return parsed if parsed > 0 else None


def validate_delimited_payload(
    row: dict[str, str],
    handle: io.TextIOBase,
    delimiter: str,
    min_records: int,
) -> dict[str, object]:
    reader = csv.reader(handle, delimiter=delimiter)
    try:
        raw_header = next(reader)
    except StopIteration:
        return source_validation_result(row, "failed", "tabular_header_missing")
    header = [normalized_header(item.lstrip("\ufeff")) for item in raw_header]
    if len(header) < 2 or any(not item for item in header) or len(set(header)) != len(header):
        return source_validation_result(row, "failed", "tabular_header_invalid")
    source_id = row.get("source_id", "")
    if source_id == "bindingdb":
        smiles = next((index for index, name in enumerate(header) if name == "ligand_smiles"), None)
        target = next((index for index, name in enumerate(header) if name.startswith("target_name")), None)
        affinity = [
            index
            for index, name in enumerate(header)
            if re.match(r"^(ki|kd|ic50|ec50)(?:_|$)", name)
        ]
        if smiles is None or target is None or not affinity:
            return source_validation_result(row, "failed", "bindingdb_schema_mismatch")
    elif source_id == "binding_moad":
        required = {"pdbfile", "bindingmoad_id", "name", "ligand_id", "measurement", "symbol", "magnitude", "units"}
        if not required.issubset(header):
            return source_validation_result(row, "failed", "binding_moad_schema_mismatch")
    records = 0
    max_rows = max(100, min_records * 20)
    for scanned, values in enumerate(reader, start=1):
        if scanned > max_rows:
            break
        if len(values) != len(header):
            return source_validation_result(row, "failed", "tabular_row_width_mismatch", records)
        if source_id == "bindingdb":
            valid = bool(
                values[smiles].strip()
                and values[target].strip()
                and any(numeric_measurement(values[index]) is not None for index in affinity)
            )
        elif source_id == "binding_moad":
            values_by_name = dict(zip(header, values, strict=True))
            valid = bool(
                re.fullmatch(r"[0-9][A-Za-z0-9]{3}", values_by_name["pdbfile"].strip())
                and values_by_name["measurement"].strip().lower() in {"ki", "kd", "ic50", "ec50"}
                and numeric_measurement(values_by_name["magnitude"]) is not None
                and values_by_name["units"].strip().lower() in {"fm", "pm", "nm", "um", "mm", "m"}
            )
        else:
            valid = any(value.strip() for value in values)
        if valid:
            records += 1
            if records >= min_records:
                return source_validation_result(row, "ready", "domain_schema_and_records_valid", records)
    return source_validation_result(
        row,
        "failed",
        f"record_count_below_minimum:{records}<{min_records}",
        records,
    )


def validate_fasta_payload(
    row: dict[str, str],
    handle: io.TextIOBase,
    min_records: int,
) -> dict[str, object]:
    records = 0
    header = ""
    has_sequence = False
    source_id = row.get("source_id", "")
    for line_number, raw in enumerate(handle, start=1):
        if len(raw) > 1_048_576:
            return source_validation_result(row, "failed", "fasta_line_too_long", records)
        line = raw.strip()
        if not line:
            continue
        if line.startswith(">"):
            if header:
                if not has_sequence:
                    return source_validation_result(row, "failed", f"fasta_sequence_missing:line_{line_number}", records)
                records += 1
                if records >= min_records:
                    return source_validation_result(row, "ready", "domain_schema_and_records_valid", records)
            header = line
            has_sequence = False
            if source_id == "uniprot_human_reviewed" and not (
                re.match(r"^>sp\|[A-Z0-9]+\|", header)
                and " OS=Homo sapiens " in f" {header} "
                and re.search(r"(?:^|\s)OX=9606(?:\s|$)", header)
            ):
                return source_validation_result(row, "failed", "uniprot_reviewed_human_header_mismatch", records)
        else:
            if not header or not re.fullmatch(r"[A-Za-z*.-]+", line):
                return source_validation_result(row, "failed", f"fasta_sequence_invalid:line_{line_number}", records)
            has_sequence = True
    if header and has_sequence:
        records += 1
    if records < min_records:
        return source_validation_result(row, "failed", f"record_count_below_minimum:{records}<{min_records}", records)
    return source_validation_result(row, "ready", "domain_schema_and_records_valid", records)


def validate_catalog_payload(row: dict[str, str], path: Path) -> dict[str, object]:
    release_error = catalog_release_error(row)
    if release_error:
        return source_validation_result(row, "failed", release_error)
    try:
        if not path.is_file() or path.stat().st_size <= 0:
            return source_validation_result(row, "failed", "payload_empty")
        with path.open("rb") as handle:
            prefix = handle.read(4096).lstrip()
        lowered = prefix.lower()
        if lowered.startswith((b"<html", b"<!doctype html", b"<?xml")) and b"<html" in lowered:
            return source_validation_result(row, "failed", "html_error_payload")
        compression = row["compression"].strip().lower()
        media_type = row["media_type"].strip().lower()
        min_records = max(1, int(row.get("min_records", "1") or "1"))
        if compression == "zip" and not prefix.startswith((b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08")):
            return source_validation_result(row, "failed", "compression_mismatch:zip")
        if compression in {"gz", "tar.gz", "tgz"} and not prefix.startswith(b"\x1f\x8b"):
            return source_validation_result(row, "failed", f"compression_mismatch:{compression}")
        if compression in {"none", "uncompressed"} and prefix.startswith((b"PK", b"\x1f\x8b")):
            return source_validation_result(row, "failed", "compression_mismatch:uncompressed")
        if compression == "zip":
            with zipfile.ZipFile(path) as archive:
                members = [item for item in archive.infolist() if not item.is_dir() and item.file_size > 0]
                if not members or any(not safe_archive_member(item.filename) or item.flag_bits & 0x1 for item in members):
                    return source_validation_result(row, "failed", "zip_member_policy_failed")
                if archive.testzip() is not None:
                    return source_validation_result(row, "failed", "zip_crc_failed")
                if media_type == "bindingdb_tsv":
                    candidates = [item for item in members if item.filename.lower().endswith((".tsv", ".txt"))]
                    if not candidates:
                        return source_validation_result(row, "failed", "bindingdb_tsv_member_missing")
                    selected = max(candidates, key=lambda item: item.file_size)
                    with archive.open(selected, "r") as binary, io.TextIOWrapper(
                        binary, encoding="utf-8-sig", errors="strict", newline=""
                    ) as text:
                        return validate_delimited_payload(row, text, "\t", min_records)
                return source_validation_result(row, "failed", f"unsupported_zip_media_type:{media_type}")
        if compression in {"tar.gz", "tgz"}:
            with tarfile.open(path, "r:gz") as archive:
                members = [item for item in archive.getmembers() if item.isfile() and item.size > 0]
                if not members or any(not safe_archive_member(item.name) for item in members):
                    return source_validation_result(row, "failed", "tar_member_policy_failed")
                if media_type == "sqlite_tar":
                    candidates = [item for item in members if item.name.lower().endswith((".db", ".sqlite"))]
                    if not candidates:
                        return source_validation_result(row, "failed", "sqlite_archive_database_missing")
                    extracted = archive.extractfile(max(candidates, key=lambda item: item.size))
                    if extracted is None or extracted.read(16) != b"SQLite format 3\x00":
                        return source_validation_result(row, "failed", "sqlite_archive_header_invalid")
                    return source_validation_result(row, "ready", "domain_schema_and_records_valid", 1)
                return source_validation_result(row, "failed", f"unsupported_tar_media_type:{media_type}")
        if compression == "gz":
            with gzip.open(path, "rt", encoding="utf-8", errors="strict", newline="") as handle:
                if media_type == "fasta":
                    return validate_fasta_payload(row, handle, min_records)
                if media_type in {"csv", "tsv"}:
                    return validate_delimited_payload(row, handle, "," if media_type == "csv" else "\t", min_records)
                return source_validation_result(row, "failed", f"unsupported_gzip_media_type:{media_type}")
        if compression in {"none", "uncompressed"} and media_type in {"csv", "tsv"}:
            with path.open("r", encoding="utf-8-sig", errors="strict", newline="") as handle:
                return validate_delimited_payload(row, handle, "," if media_type == "csv" else "\t", min_records)
        return source_validation_result(row, "failed", f"unsupported_media_contract:{media_type}:{compression}")
    except (OSError, UnicodeError, ValueError, csv.Error, gzip.BadGzipFile, tarfile.TarError, zipfile.BadZipFile) as err:
        return source_validation_result(row, "failed", f"payload_validation_failed:{type(err).__name__}")


def catalog_trust_ready(row: dict[str, str]) -> bool:
    return not catalog_release_error(row)


def chembl_query_database_ready(
    database: Path,
    manifest: Path,
    source_digest: str,
    source_release: str,
) -> bool:
    payload = read_json_object(manifest)
    if (
        payload.get("status") != "ready"
        or payload.get("source_digest") != source_digest
        or payload.get("source_release") != source_release
        or payload.get("table") != "genoligand_onsite_activity"
        or not database.is_file()
        or database.stat().st_size <= 0
    ):
        return False
    try:
        uri = f"file:{database.as_posix()}?mode=ro"
        with closing(sqlite3.connect(uri, uri=True)) as connection:
            columns = {
                str(row[1])
                for row in connection.execute("PRAGMA table_info(genoligand_onsite_activity)")
            }
            required = {
                "canonical_isomeric_smiles",
                "target_id",
                "target_name",
                "organism",
                "compound_name",
                "standard_type",
                "standard_relation",
                "standard_value",
                "standard_units",
                "source_release",
                "source_digest",
            }
            indexes = {
                str(row[1])
                for row in connection.execute("PRAGMA index_list(genoligand_onsite_activity)")
            }
            row = connection.execute(
                "SELECT canonical_isomeric_smiles, target_id FROM genoligand_onsite_activity LIMIT 1"
            ).fetchone()
            return required.issubset(columns) and "idx_genoligand_onsite_activity_smiles" in indexes and row is not None
    except sqlite3.Error:
        return False


def publish_chembl_query_database(
    science_store: Path,
    source_catalog: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    archive_path = science_store / "downloads/chembl/chembl_sqlite.tar.gz"
    database = science_store / "downloads/chembl/chembl_query.sqlite"
    manifest = database.with_suffix(".manifest.json")
    catalog_row = next((row for row in read_tsv(source_catalog) if row.get("source_id") == "chembl"), {})
    source_release = catalog_row.get("release_identity") or catalog_row.get("release_or_date") or "unknown"
    if not archive_path.is_file() or archive_path.stat().st_size <= 0:
        return {"status": "needs_repair", "reason": "chembl_archive_missing", "database": str(database)}
    source_summary = resource_payload_summary(archive_path, cache=summary_cache)
    source_digest = str(source_summary.get("sha256", ""))
    if chembl_query_database_ready(database, manifest, source_digest, source_release):
        return {
            "status": "ready",
            "reason": "chembl_query_database_reused",
            "database": str(database),
            "table": "genoligand_onsite_activity",
            "source_release": source_release,
            "source_digest": source_digest,
            "manifest": str(manifest),
        }
    staging = database.parent / f".chembl-query.rebuild-{os.getpid()}-{time.time_ns()}"
    staging_database = staging / "chembl_query.sqlite"
    extracted_database = staging / "chembl_source.sqlite"
    try:
        with tarfile.open(archive_path, "r:gz") as archive:
            candidates = [
                item
                for item in archive.getmembers()
                if item.isfile() and item.size > 0 and item.name.lower().endswith((".db", ".sqlite"))
            ]
            if len(candidates) != 1:
                return {"status": "needs_repair", "reason": "chembl_sqlite_member_ambiguous", "database": str(database)}
            required = candidates[0].size * 2 + DISK_SAFETY_MARGIN_BYTES
            if required > shutil.disk_usage(existing_path_for_disk_usage(database)).free:
                return {
                    "status": "needs_repair",
                    "reason": "chembl_query_database_insufficient_disk",
                    "required_peak_bytes": required,
                    "available_bytes": shutil.disk_usage(existing_path_for_disk_usage(database)).free,
                    "database": str(database),
                }
            staging.mkdir(parents=True, exist_ok=False)
            source_handle = archive.extractfile(candidates[0])
            if source_handle is None:
                raise OSError("chembl sqlite archive member cannot be read")
            with source_handle, extracted_database.open("wb") as target_handle:
                shutil.copyfileobj(source_handle, target_handle, 1024 * 1024)
        shutil.copy2(extracted_database, staging_database)
        with closing(sqlite3.connect(staging_database)) as connection:
            connection.execute("PRAGMA journal_mode=OFF")
            connection.execute("PRAGMA synchronous=OFF")
            connection.execute("PRAGMA temp_store=FILE")
            connection.execute("DROP TABLE IF EXISTS genoligand_onsite_activity")
            connection.execute(
                """
                CREATE TABLE genoligand_onsite_activity AS
                SELECT
                    cs.canonical_smiles AS canonical_isomeric_smiles,
                    td.chembl_id AS target_id,
                    td.pref_name AS target_name,
                    td.organism AS organism,
                    md.pref_name AS compound_name,
                    act.standard_type AS standard_type,
                    act.standard_relation AS standard_relation,
                    act.standard_value AS standard_value,
                    act.standard_units AS standard_units,
                    ? AS source_release,
                    ? AS source_digest
                FROM activities AS act
                JOIN assays AS assay ON assay.assay_id = act.assay_id
                JOIN target_dictionary AS td ON td.tid = assay.tid
                JOIN molecule_dictionary AS md ON md.molregno = act.molregno
                JOIN compound_structures AS cs ON cs.molregno = act.molregno
                WHERE cs.canonical_smiles IS NOT NULL
                  AND TRIM(cs.canonical_smiles) <> ''
                  AND td.chembl_id IS NOT NULL
                """,
                (source_release, source_digest),
            )
            connection.execute(
                "CREATE INDEX idx_genoligand_onsite_activity_smiles "
                "ON genoligand_onsite_activity(canonical_isomeric_smiles)"
            )
            connection.execute(
                "CREATE INDEX idx_genoligand_onsite_activity_target "
                "ON genoligand_onsite_activity(target_id)"
            )
            connection.commit()
            if connection.execute("PRAGMA quick_check").fetchone() != ("ok",):
                raise sqlite3.DatabaseError("chembl query database quick_check failed")
        if database.exists():
            try:
                database.chmod(0o666)
            except OSError:
                pass
        os.replace(staging_database, database)
        try:
            database.chmod(0o444)
        except OSError:
            pass
        payload = {
            "schema_version": "genoligand_chembl_query_database_v1",
            "status": "ready",
            "database": str(database),
            "database_bytes": database.stat().st_size,
            "database_sha256": sha256_file(database),
            "table": "genoligand_onsite_activity",
            "canonical_query_column": "canonical_isomeric_smiles",
            "index": "idx_genoligand_onsite_activity_smiles",
            "source_release": source_release,
            "source_digest": source_digest,
        }
        write_json_atomic(manifest, payload)
        return {**payload, "manifest": str(manifest), "reason": "chembl_query_database_published"}
    except (OSError, sqlite3.Error, tarfile.TarError) as err:
        write_json_atomic(
            manifest.with_name(f"{manifest.name}.attempt.json"),
            {"status": "failed", "reason": exception_note(err), "source_digest": source_digest},
        )
        return {
            "status": "needs_repair",
            "reason": f"chembl_query_database_publish_failed:{exception_note(err)}",
            "database": str(database),
        }
    finally:
        shutil.rmtree(staging, ignore_errors=True)


def prepare_downloads(
    catalog: Path,
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    manifest = science_store / "download_execution_manifest.tsv"
    previous_rows = {
        (row.get("source_id", ""), row.get("url", ""), row.get("local_path", "")): row
        for row in read_tsv(manifest)
        if row.get("status") == "ready"
    }
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
    with atomic_text_writer(manifest) as handle:
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
                "expected_sha256",
                "observed_bytes",
                "observed_sha256",
                "source_version",
                "release_identity",
                "trust_source",
                "validation_schema",
                "validated_records",
                "validation_reason",
                "resource_identity",
                "etag",
                "content_length",
                "completed_at",
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
            source_version = row.get("release_or_date", "")
            release_identity = row.get("release_identity", "")
            trust_source = row.get("trust_source", "")
            previous = previous_rows.get((source_id, url, local_path), {})
            previous_version = previous.get("source_version", "")
            observed_sha256 = previous.get("observed_sha256", "") or previous.get("sha256", "")
            observed_bytes_text = previous.get("observed_bytes", "") or previous.get("bytes", "")
            observed_bytes = int(observed_bytes_text) if observed_bytes_text.isdigit() else None
            if (
                expected
                or previous_version != source_version
                or previous.get("release_identity", "") != release_identity
                or previous.get("trust_source", "") != trust_source
                or not observed_sha256
                or not observed_bytes
            ):
                observed_sha256 = ""
                observed_bytes = None
            completed = ready + failed
            if not source_id or not url or not local_path or not catalog_trust_ready(row):
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
                        "release_identity": release_identity,
                        "trust_source": trust_source,
                        "notes": "source_id_url_local_path_and_auditable_trust_required",
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

                result = copy_or_download(
                    url,
                    destination,
                    expected,
                    progress_callback,
                    observed_sha256=observed_sha256 or None,
                    observed_bytes=observed_bytes,
                    validator=lambda path, catalog_row=row: validate_catalog_payload(catalog_row, path),
                    summary_cache=summary_cache,
                )
                if result["status"] == "ready":
                    ready += 1
                    seed_resource_file_fingerprint(
                        summary_cache,
                        destination,
                        str(result.get("sha256", "")),
                    )
                else:
                    failed += 1
                validation = result.get("validation", {})
                if not isinstance(validation, dict):
                    validation = {}
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
                        "expected_sha256": expected or "",
                        "observed_bytes": str(result["bytes"]),
                        "observed_sha256": str(result["sha256"]),
                        "source_version": source_version,
                        "release_identity": release_identity,
                        "trust_source": trust_source,
                        "validation_schema": str(validation.get("schema_version", "")),
                        "validated_records": str(validation.get("records", 0)),
                        "validation_reason": str(validation.get("reason", "")),
                        "resource_identity": str(result.get("resource_identity", "")),
                        "etag": str(result.get("etag", "")),
                        "content_length": str(result.get("content_length", 0)),
                        "completed_at": str(int(time.time())),
                        "notes": "existing_observed_digest_matched" if result.get("reused") else "download_or_copy_completed",
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
                        "release_identity": release_identity,
                        "trust_source": trust_source,
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


def directory_digest(path: Path, cache: ResourceSummaryCache | None = None) -> str:
    return str(resource_payload_summary(path, cache=cache).get("sha256", ""))


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


def validate_external_payload(path: Path) -> dict[str, object]:
    try:
        if not path.is_file() or path.stat().st_size <= 0:
            return {"status": "failed", "reason": "external_payload_empty"}
        lower = path.name.lower()
        with path.open("rb") as handle:
            prefix = handle.read(4096).lstrip()
            if prefix.lower().startswith((b"<html", b"<!doctype html")):
                return {"status": "failed", "reason": "external_html_error_payload"}
            if lower.endswith(".parquet"):
                handle.seek(-4, os.SEEK_END)
                if prefix[:4] != b"PAR1" or handle.read(4) != b"PAR1":
                    return {"status": "failed", "reason": "parquet_magic_invalid"}
        if lower.endswith(".gz"):
            with gzip.open(path, "rb") as handle:
                if not handle.read(4096):
                    return {"status": "failed", "reason": "gzip_payload_empty"}
        return {"status": "ready", "reason": "external_domain_validation_passed"}
    except (OSError, gzip.BadGzipFile):
        return {"status": "failed", "reason": "external_payload_parse_failed"}


def validate_external_source_payload(
    row: dict[str, str],
    path: Path,
    publisher_object: dict[str, object] | None = None,
) -> dict[str, object]:
    base = validate_external_payload(path)
    if base.get("status") != "ready":
        return base
    if not row.get("release_or_date", "").strip() or not row.get("trust_source", "").strip():
        return {"status": "failed", "reason": "external_release_identity_missing"}
    relative_name = path.name.removesuffix(".part")
    if publisher_object is not None:
        relative = safe_inventory_relative_path(str(publisher_object.get("relative_path", "")))
        size = int(publisher_object.get("size", 0) or 0)
        algorithm = str(publisher_object.get("digest_algorithm", "")).lower()
        expected = str(publisher_object.get("digest", "")).lower()
        if (
            relative is None
            or size <= 0
            or path.stat().st_size != size
            or algorithm not in {"md5", "sha256"}
            or not re.fullmatch(rf"[0-9a-f]{{{32 if algorithm == 'md5' else 64}}}", expected)
            or file_digest(path, algorithm).lower() != expected
        ):
            return {"status": "failed", "reason": "publisher_object_identity_mismatch"}
        relative_name = relative.as_posix()
    source_id = row.get("source_id", "").strip()
    lower_name = relative_name.lower()
    if source_id == "plinder":
        if not lower_name.endswith(".parquet"):
            return {"status": "failed", "reason": "plinder_parquet_required"}
        try:
            with path.open("rb") as handle:
                prefix = handle.read(4)
                handle.seek(-4, os.SEEK_END)
                suffix = handle.read(4)
        except OSError:
            return {"status": "failed", "reason": "plinder_parquet_parse_failed"}
        if prefix != b"PAR1" or suffix != b"PAR1":
            return {"status": "failed", "reason": "plinder_parquet_magic_invalid"}
    if source_id == "metagenome_potential" and lower_name == "mgy_clusters.fa.gz":
        try:
            with gzip.open(path, "rt", encoding="utf-8", errors="strict", newline="") as handle:
                validation = validate_fasta_payload(row, handle, 1)
        except (OSError, UnicodeError, gzip.BadGzipFile):
            return {"status": "failed", "reason": "mgnify_fasta_parse_failed"}
        if validation.get("status") != "ready":
            return {"status": "failed", "reason": f"mgnify_{validation.get('reason', 'fasta_invalid')}"}
    if source_id == "biolip2" and lower_name.endswith("biolip.txt.gz"):
        try:
            with gzip.open(path, "rt", encoding="utf-8", errors="strict") as handle:
                found = any(
                    re.fullmatch(r"[0-9][A-Za-z0-9]{3}", re.split(r"[\t ]+", line.strip(), maxsplit=1)[0])
                    for _, line in zip(range(200), handle, strict=False)
                    if line.strip() and not line.startswith("#")
                )
        except (OSError, UnicodeError, gzip.BadGzipFile):
            found = False
        if not found:
            return {"status": "failed", "reason": "biolip_annotation_schema_mismatch"}
    if source_id == "biolip2" and lower_name.endswith(".fasta.gz"):
        try:
            with gzip.open(path, "rt", encoding="utf-8", errors="strict", newline="") as handle:
                validation = validate_fasta_payload(row, handle, 1)
        except (OSError, UnicodeError, gzip.BadGzipFile):
            return {"status": "failed", "reason": "biolip_fasta_parse_failed"}
        if validation.get("status") != "ready":
            return {"status": "failed", "reason": f"biolip_{validation.get('reason', 'fasta_invalid')}"}
    if source_id == "biolip2" and lower_name.endswith("ligand.tsv"):
        try:
            with path.open("r", encoding="utf-8-sig", errors="strict", newline="") as handle:
                rows = [line.rstrip("\r\n").split("\t") for _, line in zip(range(2), handle, strict=False)]
        except (OSError, UnicodeError):
            rows = []
        if len(rows) < 2 or len(rows[0]) < 2 or len(rows[1]) != len(rows[0]):
            return {"status": "failed", "reason": "biolip_ligand_schema_mismatch"}
    return {
        "status": "ready",
        "reason": "external_source_identity_and_domain_valid",
        "release_identity": row.get("release_or_date", ""),
        "trust_source": row.get("trust_source", ""),
    }


def sync_html_links(
    row: dict[str, str],
    target: Path,
    progress: ProgressCallback | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    source_url = row.get("source_url") or row.get("download_url") or row.get("landing_url")
    include_pattern = row.get("include_pattern", "")
    if not source_url:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "html_source_url_required"}
    inventory = html_links_inventory(row) if row.get("checksum_url", "").strip() else []
    if inventory:
        if inventory and all(publisher_object_matches(target, item) for item in inventory):
            return {
                "status": "ready",
                "bytes": directory_size(target),
                "sha256": directory_digest(target, summary_cache),
                "notes": "html_links_publisher_identity_reused",
            }
    links = [str(item.get("media_url", "")) for item in inventory] or extract_html_links(source_url, include_pattern)
    if not links:
        return {"status": "failed", "bytes": 0, "sha256": "", "notes": "no_matching_links"}
    target.mkdir(parents=True, exist_ok=True)
    failures = []
    for index, link in enumerate(links, start=1):
        publisher_object = inventory[index - 1] if inventory else None
        relative = (
            safe_inventory_relative_path(str(publisher_object.get("relative_path", "")))
            if publisher_object is not None
            else None
        )
        destination = target / (relative or Path(urllib.parse.urlparse(link).path).name)
        if progress:
            progress("downloading", index - 1, len(links), f"正在同步资源 {index}/{len(links)}")
        try:
            result = copy_or_download(
                link,
                destination,
                (
                    str(publisher_object.get("digest", ""))
                    if publisher_object is not None and publisher_object.get("digest_algorithm") == "sha256"
                    else row.get("sha256", "") or None
                ),
                progress,
                validator=lambda path, source=row, identity=publisher_object: validate_external_source_payload(
                    source, path, identity
                ),
                summary_cache=summary_cache,
            )
            if result.get("status") != "ready":
                failures.append(f"{link}:status={result.get('status', 'unknown')}")
            else:
                seed_resource_file_fingerprint(summary_cache, destination, str(result.get("sha256", "")))
        except Exception as err:  # noqa: BLE001 - report per-source install failure
            failures.append(f"{link}:{err}")
    if failures:
        return {"status": "failed", "bytes": directory_size(target), "sha256": directory_digest(target, summary_cache), "notes": "|".join(failures)[:1000]}
    return {
        "status": "ready" if path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target, summary_cache),
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


def sync_html_tree(
    row: dict[str, str],
    target: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
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
                result = copy_or_download(
                    absolute,
                    destination,
                    row.get("sha256", "") or None,
                    validator=lambda path, source=row: validate_external_source_payload(source, path),
                    summary_cache=summary_cache,
                )
                if result.get("status") == "ready":
                    downloaded += 1
                    seed_resource_file_fingerprint(summary_cache, destination, str(result.get("sha256", "")))
                else:
                    failures.append(f"{absolute}:status={result.get('status', 'unknown')}")
            except Exception as err:  # noqa: BLE001 - report per-file failure
                failures.append(f"{absolute}:{err}")
    if failures:
        return {"status": "failed", "bytes": directory_size(target), "sha256": directory_digest(target, summary_cache), "notes": "|".join(failures)[:1000]}
    return {
        "status": "ready" if downloaded and path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target, summary_cache),
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


def safe_inventory_relative_path(value: str) -> Path | None:
    path = Path(value.replace("\\", "/"))
    if path.is_absolute() or not path.parts or any(part in {"", ".", ".."} for part in path.parts):
        return None
    return path


def publisher_object_matches(target: Path, item: dict[str, object]) -> bool:
    relative = safe_inventory_relative_path(str(item.get("relative_path", "")))
    size = int(item.get("size", 0) or 0)
    algorithm = str(item.get("digest_algorithm", "")).lower()
    expected = str(item.get("digest", "")).lower()
    if relative is None or size <= 0 or algorithm not in {"md5", "sha256"} or not expected:
        return False
    path = target / relative
    return bool(
        path.is_file()
        and path.stat().st_size == size
        and file_digest(path, algorithm).lower() == expected
    )


def publisher_inventory_capacity(items: list[dict[str, object]], target: Path) -> dict[str, object]:
    matched = 0
    changed = 0
    download_bytes = 0
    unresolved = 0
    for item in items:
        size = int(item.get("size", 0) or 0)
        relative = safe_inventory_relative_path(str(item.get("relative_path", "")))
        destination = target / relative if relative is not None else None
        if publisher_object_matches(target, item):
            matched += 1
            continue
        if destination is not None and destination.is_file():
            changed += 1
        if size > 0:
            download_bytes += size
        else:
            unresolved += 1
    if not items or unresolved:
        action = "resolve_remote_size_before_download"
    elif matched == len(items):
        action = "reuse_after_publisher_identity"
    elif changed:
        action = "download_publisher_identity_changed"
    else:
        action = "download_missing_publisher_objects"
    return {
        "action": action,
        "download_bytes": download_bytes,
        "remote_objects": len(items),
        "matched_objects": matched,
        "changed_objects": changed,
        "unresolved_objects": unresolved,
    }


def gcs_prefix_inventory(row: dict[str, str]) -> list[dict[str, object]]:
    parsed = parse_gcs_location(row)
    if not parsed:
        return []
    bucket, prefix = parsed
    pattern_text = row.get("include_pattern", "")
    pattern = re.compile(pattern_text) if pattern_text else None
    page_token = ""
    items: list[dict[str, object]] = []
    while True:
        query = urllib.parse.urlencode(
            {"prefix": prefix, "maxResults": "1000", **({"pageToken": page_token} if page_token else {})}
        )
        api_url = f"https://storage.googleapis.com/storage/v1/b/{urllib.parse.quote(bucket)}/o?{query}"
        request = urllib.request.Request(api_url, headers={"User-Agent": DOWNLOAD_USER_AGENT})
        with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
            payload = json.loads(response.read().decode("utf-8"))
        for source in payload.get("items", []):
            name = str(source.get("name", ""))
            if not name or name.endswith("/"):
                continue
            relative = name[len(prefix) :].lstrip("/") if prefix and name.startswith(prefix) else name
            if safe_inventory_relative_path(relative) is None or (pattern and not pattern.search(relative)):
                continue
            md5_text = str(source.get("md5Hash", ""))
            try:
                md5_digest = base64.b64decode(md5_text, validate=True).hex() if md5_text else ""
            except (ValueError, TypeError):
                md5_digest = ""
            media_url = source.get("mediaLink") or (
                f"https://storage.googleapis.com/{bucket}/{urllib.parse.quote(name)}"
            )
            items.append(
                {
                    "relative_path": relative,
                    "size": int(source.get("size", 0) or 0),
                    "digest_algorithm": "md5" if md5_digest else "",
                    "digest": md5_digest,
                    "media_url": str(media_url),
                }
            )
        page_token = str(payload.get("nextPageToken", ""))
        if not page_token:
            break
    return sorted(items, key=lambda item: str(item["relative_path"]))


def read_small_remote_payload(url: str, limit: int = 1024 * 1024) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": DOWNLOAD_USER_AGENT})
    with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
        payload = response.read(limit + 1)
    if len(payload) > limit:
        raise ValueError(f"remote_identity_payload_exceeds_limit:{url}")
    return payload


def parse_md5_manifest(payload: bytes) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in payload.decode("utf-8", errors="strict").splitlines():
        match = re.match(r"^([0-9a-fA-F]{32})\s+\*?(.+?)\s*$", line)
        if match:
            checksums[Path(match.group(2)).name] = match.group(1).lower()
    return checksums


def html_links_inventory(row: dict[str, str]) -> list[dict[str, object]]:
    source_url = row.get("source_url") or row.get("download_url") or row.get("landing_url")
    if not source_url:
        return []
    links = extract_html_links(source_url, row.get("include_pattern", ""))
    checksum_url = row.get("checksum_url", "").strip()
    checksum_payload = read_small_remote_payload(checksum_url) if checksum_url else b""
    checksums = parse_md5_manifest(checksum_payload) if checksum_payload else {}
    checksum_name = Path(urllib.parse.urlparse(checksum_url).path).name if checksum_url else ""
    items: list[dict[str, object]] = []
    for link in links:
        name = Path(urllib.parse.urlparse(link).path).name
        identity = remote_resource_identity(link)
        size = int(identity.get("content_length", 0) or 0)
        algorithm = "md5" if name in checksums else ""
        digest = checksums.get(name, "")
        if checksum_payload and name == checksum_name:
            algorithm = "sha256"
            digest = hashlib.sha256(checksum_payload).hexdigest()
        elif not digest and 0 < size <= 1024 * 1024:
            payload = read_small_remote_payload(link)
            algorithm = "sha256"
            digest = hashlib.sha256(payload).hexdigest()
        items.append(
            {
                "relative_path": name,
                "size": size,
                "digest_algorithm": algorithm,
                "digest": digest,
                "media_url": link,
            }
        )
    return sorted(items, key=lambda item: str(item["relative_path"]))


def sync_gcs_prefix(
    row: dict[str, str],
    target: Path,
    progress: ProgressCallback | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    parsed = parse_gcs_location(row)
    if not parsed:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "gcs_bucket_or_gs_url_required"}
    inventory = gcs_prefix_inventory(row)
    if inventory and all(publisher_object_matches(target, item) for item in inventory):
        return {
            "status": "ready",
            "bytes": directory_size(target),
            "sha256": directory_digest(target, summary_cache),
            "notes": "gcs_objects_publisher_identity_reused",
        }
    if not inventory:
        return {
            "status": "failed",
            "bytes": directory_size(target),
            "sha256": directory_digest(target, summary_cache),
            "notes": "gcs_publisher_inventory_missing",
        }
    target.mkdir(parents=True, exist_ok=True)
    downloaded = 0
    failures = []
    for item in inventory:
        relative = safe_inventory_relative_path(str(item.get("relative_path", "")))
        if relative is None:
            failures.append("unsafe_inventory_path")
            continue
        media_url = str(item.get("media_url", ""))
        destination = target / relative
        if progress:
            progress("downloading", downloaded, len(inventory), f"正在同步对象 {relative.as_posix()}")
        try:
            result = copy_or_download(
                media_url,
                destination,
                str(item.get("digest", "")) if item.get("digest_algorithm") == "sha256" else None,
                progress,
                validator=lambda path, source=row, identity=item: validate_external_source_payload(
                    source, path, identity
                ),
                summary_cache=summary_cache,
            )
            if result.get("status") == "ready":
                downloaded += 1
                seed_resource_file_fingerprint(summary_cache, destination, str(result.get("sha256", "")))
            else:
                failures.append(f"{relative.as_posix()}:status={result.get('status', 'unknown')}")
        except Exception as err:  # noqa: BLE001 - report per-source install failure
            failures.append(f"{relative.as_posix()}:{err}")
    if failures:
        return {"status": "failed", "bytes": directory_size(target), "sha256": directory_digest(target, summary_cache), "notes": "|".join(failures)[:1000]}
    return {
        "status": "ready" if downloaded and path_has_payload(target) else "failed",
        "bytes": directory_size(target),
        "sha256": directory_digest(target, summary_cache),
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
    summary_cache: ResourceSummaryCache | None = None,
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
        if valid_rcsb_coordinate(destination, upper):
            return upper, True, "already_available"
        url = template.replace("{pdb_id}", upper).replace("{pdb_id_lower}", upper.lower())
        try:
            result = copy_or_download(
                url,
                destination,
                None,
                validator=lambda path, expected=upper: rcsb_coordinate_validation(path, expected),
                summary_cache=summary_cache,
            )
            if result.get("status") == "ready":
                return upper, True, "downloaded"
            return upper, False, f"status={result.get('status', 'unknown')}"
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
    snapshot = write_rcsb_scope_manifest(science_store, target, pdb_ids, summary_cache)
    if failures or snapshot["missing_count"]:
        return {
            "status": "failed",
            "bytes": snapshot["coordinate_bytes"],
            "sha256": snapshot["coordinate_digest"],
            "notes": ("|".join(failures) or f"scope_missing:{snapshot['missing_count']}")[:1000],
            **snapshot,
        }
    return {
        "status": "ready" if snapshot["ready_count"] == snapshot["scope_count"] else "failed",
        "bytes": snapshot["coordinate_bytes"],
        "sha256": snapshot["coordinate_digest"],
        "notes": f"rcsb_ligand_bound_entries_synced:{downloaded}",
        **snapshot,
    }


def rcsb_coordinate_validation(path: Path, expected_pdb_id: str = "") -> dict[str, object]:
    try:
        if path.name.lower().endswith(".part") or not path.is_file() or path.stat().st_size <= 0:
            return {"status": "failed", "reason": "coordinate_missing_or_partial"}
        opener = gzip.open if path.name.lower().endswith(".gz") else open
        with opener(path, "rt", encoding="utf-8", errors="strict") as handle:
            prefix = handle.read(4096)
        match = re.search(r"(?im)^data_([a-z0-9]{4})\s*$", prefix)
        if not match:
            return {"status": "failed", "reason": "mmcif_data_block_missing"}
        observed = match.group(1).upper()
        if expected_pdb_id and observed != expected_pdb_id.upper():
            return {"status": "failed", "reason": f"mmcif_id_mismatch:{observed}"}
        return {"status": "ready", "reason": "mmcif_parse_passed", "pdb_id": observed}
    except (OSError, UnicodeError, gzip.BadGzipFile) as err:
        return {"status": "failed", "reason": f"mmcif_parse_failed:{type(err).__name__}"}


def valid_rcsb_coordinate(path: Path, expected_pdb_id: str = "") -> bool:
    return rcsb_coordinate_validation(path, expected_pdb_id).get("status") == "ready"


def write_rcsb_scope_manifest(
    science_store: Path,
    target: Path,
    pdb_ids: set[str],
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    rows: list[dict[str, str]] = []
    digest = hashlib.sha256()
    coordinate_bytes = 0
    ready_count = 0
    for pdb_id in sorted(pdb_ids):
        upper = pdb_id.upper()
        coordinate = target / upper[1:3].lower() / f"{upper.lower()}.cif.gz"
        ready = valid_rcsb_coordinate(coordinate, upper)
        size = coordinate.stat().st_size if ready else 0
        sha256 = (
            str(resource_payload_summary(coordinate, cache=summary_cache).get("content_sha256", ""))
            if ready
            else ""
        )
        rel = relative_path(science_store, coordinate) if ready else ""
        if ready:
            ready_count += 1
            coordinate_bytes += size
            digest.update(coordinate.relative_to(target).as_posix().encode("utf-8"))
            digest.update(str(size).encode("ascii"))
            digest.update(sha256.encode("ascii"))
        rows.append(
            {
                "pdb_id": upper,
                "status": "ready" if ready else "missing",
                "coordinate_rel_path": rel,
                "size_bytes": str(size),
                "sha256": sha256,
            }
        )
    scope_digest = hashlib.sha256("\n".join(sorted(pdb_ids)).encode("ascii")).hexdigest()
    coordinate_digest = digest.hexdigest() if ready_count else ""
    manifest = science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv"
    write_simple_tsv(
        manifest,
        ["pdb_id", "status", "coordinate_rel_path", "size_bytes", "sha256"],
        rows,
    )
    snapshot = {
        "schema_version": "genoligand_rcsb_scope_snapshot_v1",
        "scope_count": len(rows),
        "ready_count": ready_count,
        "missing_count": len(rows) - ready_count,
        "scope_digest": scope_digest,
        "coordinate_files": ready_count,
        "coordinate_bytes": coordinate_bytes,
        "coordinate_digest": coordinate_digest,
        "manifest": str(manifest),
    }
    write_json_atomic(manifest.with_suffix(".json"), snapshot)
    return snapshot


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
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    method = (row.get("sync_method") or "direct").strip().lower()
    if method == "html_links":
        return sync_html_links(row, target, progress, summary_cache)
    if method == "html_tree":
        return sync_html_tree(row, target, summary_cache)
    if method == "gcs_prefix":
        return sync_gcs_prefix(row, target, progress, summary_cache)
    if method == "rcsb_ligand_bound_scope":
        return sync_rcsb_ligand_bound_scope(
            row, target, science_store, progress, summary_cache
        )
    if method == "rsync":
        return sync_rsync(row, target, env)
    location = direct_source_location(row)
    if not location:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "direct_download_or_local_source_required"}
    destination = external_destination(target, location)
    result = copy_or_download(
        location,
        destination,
        row.get("sha256", "") or None,
        progress,
        validator=lambda path, source=row: validate_external_source_payload(source, path),
        summary_cache=summary_cache,
    )
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
        if not valid_rcsb_coordinate(destination, upper):
            return True
    return False


def external_evidence_path(row: dict[str, str], target: Path | None, marker: Path | None) -> Path | None:
    method = (row.get("sync_method") or "direct").strip().lower()
    if method == "direct" and target is not None:
        location = direct_source_location(row)
        if location:
            return external_destination(target, location)
        return marker
    return target if target and path_has_payload(target) else marker


def external_plan_digest(row: dict[str, str]) -> str:
    identity_fields = [
        "source_id",
        "source_class",
        "sync_method",
        "source_url",
        "source_path",
        "download_url",
        "landing_url",
        "target_rel_path",
        "ready_marker",
        "include_pattern",
        "checksum_url",
        "max_depth",
        "gcs_bucket",
        "gcs_prefix",
        "release_or_date",
        "trust_source",
        "sha256",
    ]
    payload = json.dumps(
        {key: row.get(key, "") for key in identity_fields},
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def external_evidence_digest(
    row: dict[str, str],
    evidence: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> str:
    summary = resource_payload_summary(evidence, cache=summary_cache)
    if (row.get("sync_method") or "direct").strip().lower() == "direct" and evidence.is_file():
        return str(summary.get("content_sha256", ""))
    return str(summary.get("sha256", ""))


def external_plan_row_ready(
    row: dict[str, str],
    target: Path | None,
    marker: Path | None,
    science_store: Path,
    execution: dict[str, str],
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    if execution.get("status") != "ready" or execution.get("target_rel_path", "") != row.get("target_rel_path", ""):
        return False
    if execution.get("plan_digest", "") != external_plan_digest(row):
        return False
    if (
        not row.get("release_or_date", "").strip()
        or not row.get("trust_source", "").strip()
        or execution.get("release_identity", "") != row.get("release_or_date", "")
        or execution.get("trust_source", "") != row.get("trust_source", "")
    ):
        return False
    if not marker or not path_has_payload(marker):
        return False
    if (row.get("sync_method") or "direct").strip().lower() == "rcsb_ligand_bound_scope":
        if rcsb_scope_has_missing_coordinates(science_store, target):
            return False
    evidence = external_evidence_path(row, target, marker)
    if not evidence or not path_has_payload(evidence):
        return False
    expected_sha256 = execution.get("sha256", "").strip()
    expected_bytes = execution.get("bytes", "").strip()
    return (
        bool(expected_sha256)
        and expected_bytes.isdigit()
        and int(expected_bytes) == directory_size(evidence)
        and expected_sha256.lower() == external_evidence_digest(row, evidence, summary_cache).lower()
    )


def sync_external_sources(
    plan: Path,
    science_store: Path,
    skip_downloads: bool,
    env: dict[str, str] | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    manifest = science_store / "external_sync_execution_manifest.tsv"
    rows = read_tsv(plan)
    previous_rows = {row.get("source_id", ""): row for row in read_tsv(manifest)}
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
    with atomic_text_writer(manifest) as handle:
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
                "plan_digest",
                "release_identity",
                "trust_source",
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
            previous = previous_rows.get(source_id, {})
            marker_ready = external_plan_row_ready(
                row, target, marker, science_store, previous, summary_cache
            )
            if marker_ready:
                evidence = external_evidence_path(row, target, marker)
                evidence_bytes = directory_size(evidence) if evidence else 0
                evidence_sha256 = external_evidence_digest(row, evidence, summary_cache) if evidence else ""
                ready += 1
                write_install_progress(
                    science_store,
                    status="running",
                    stage="同步大型资源",
                    current_resource=label,
                    completed_steps=ready + failed + needs_action,
                    total_steps=len(rows),
                    failed_steps=failed,
                    bytes_downloaded=evidence_bytes,
                    bytes_total=evidence_bytes,
                    message="资源已可用",
                )
                writer.writerow(
                    {
                        "source_id": source_id,
                        "source_label": label,
                        "target_rel_path": target_rel,
                        "status": "ready",
                        "bytes": str(evidence_bytes),
                        "sha256": evidence_sha256,
                        "plan_digest": external_plan_digest(row),
                        "release_identity": row.get("release_or_date", ""),
                        "trust_source": row.get("trust_source", ""),
                        "notes": "previous_success_digest_matched",
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
                        "release_identity": row.get("release_or_date", ""),
                        "trust_source": row.get("trust_source", ""),
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
                        "release_identity": row.get("release_or_date", ""),
                        "trust_source": row.get("trust_source", ""),
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
                result = sync_external_row(
                    row,
                    target,
                    science_store,
                    env,
                    progress_callback,
                    summary_cache,
                )
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
                        "plan_digest": external_plan_digest(row),
                        "release_identity": row.get("release_or_date", ""),
                        "trust_source": row.get("trust_source", ""),
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
                        "release_identity": row.get("release_or_date", ""),
                        "trust_source": row.get("trust_source", ""),
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
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    mmseqs_rows = build_mmseqs_target_databases(science_store, env, wsl_distro, summary_cache)
    foldseek_rows = build_foldseek_target_database(science_store, env, wsl_distro, summary_cache)
    all_rows = [*mmseqs_rows, *foldseek_rows]
    ready = sum(1 for row in all_rows if row.get("status") == "ready")
    failed = sum(1 for row in all_rows if row.get("status") == "failed")
    deferred = sum(1 for row in all_rows if row.get("status") == "deferred_large_source")
    total = len(all_rows)
    status = "ready" if total > 0 and ready == total and failed == 0 and deferred == 0 else "failed" if failed else "needs_repair"
    return {
        "status": status,
        "ready": ready,
        "failed": failed,
        "deferred": deferred,
        "total": total,
        "mmseqs_manifest": str(science_store / "indexes/mmseqs/mmseqs_target_databases.tsv"),
        "foldseek_manifest": str(science_store / "indexes/foldseek/foldseek_target_databases.tsv"),
    }


def alignment_worker_consumer_smoke(
    workspace_root: Path,
    science_store: Path,
    python_command: str,
    python_backend: str,
    wsl_distro: str | None,
) -> dict[str, object]:
    if python_backend != "wsl" and not python_command:
        return {"status": "failed", "reason": "alignment_worker_python_missing"}
    code = (
        "from pathlib import Path; from genoligand_workers.alignment_worker import "
        "mmseqs_target_databases,foldseek_target_database; import sys; "
        "store=Path(sys.argv[1]); mm=mmseqs_target_databases(store); fs=foldseek_target_database(store); "
        "assert mm and fs; print('alignment_worker_manifest_consumer_ready')"
    )
    if python_backend == "wsl":
        if not wsl_distro:
            return {"status": "failed", "reason": "alignment_worker_wsl_distro_missing"}
        home = wsl_home(wsl_distro)
        if not home:
            return {"status": "failed", "reason": "alignment_worker_wsl_home_missing"}
        python_path = f"{home}/.local/state/genoligand/python-worker-env/bin/python"
        script = (
            f"PYTHONPATH={bash_words([wsl_path(workspace_root / 'python')])} "
            f"{bash_words([python_path, '-c', code, wsl_path(science_store)])}"
        )
        completed = run_wsl_user_shell(wsl_distro, script)
    else:
        env = dict(os.environ)
        env["PYTHONPATH"] = str(workspace_root / "python")
        completed = run_command([python_command, "-c", code, str(science_store)], env=env)
    ready = completed.returncode == 0 and "alignment_worker_manifest_consumer_ready" in completed.stdout
    return {
        "status": "ready" if ready else "failed",
        "reason": "current_worker_consumer_smoke_passed" if ready else process_note(completed),
    }


def validate_openff_forcefield(path: Path, python_command: str) -> dict[str, object]:
    try:
        import xml.etree.ElementTree as element_tree

        root = element_tree.parse(path).getroot()
        root_name = root.tag.rsplit("}", 1)[-1]
        if root_name != "SMIRNOFF" or not root.attrib.get("version") or len(list(root)) < 5:
            return {"status": "needs_repair", "reason": "offxml_structure_invalid", "loader_smoke": "not_run"}
    except (OSError, element_tree.ParseError):
        return {"status": "needs_repair", "reason": "offxml_xml_parse_failed", "loader_smoke": "not_run"}
    actual = sha256_file(path)
    if actual.lower() != OPENFF_FORCEFIELD_SHA256:
        return {
            "status": "needs_repair",
            "reason": "offxml_sha256_mismatch",
            "loader_smoke": "not_run",
            "actual_sha256": actual,
        }
    code = (
        "from openff.toolkit.typing.engines.smirnoff import ForceField; "
        "ForceField(__import__('sys').argv[1]); print('openff_forcefield_loaded')"
    )
    completed = run_command([python_command, "-c", code, str(path)])
    if completed.returncode != 0 or "openff_forcefield_loaded" not in completed.stdout:
        return {
            "status": "needs_repair",
            "reason": "openff_forcefield_loader_failed",
            "loader_smoke": process_note(completed),
            "actual_sha256": actual,
        }
    return {
        "status": "ready",
        "reason": "fixed_digest_xml_and_loader_passed",
        "loader_smoke": "ready",
        "actual_sha256": actual,
    }


def ensure_openff_forcefield_file(
    science_store: Path,
    python_command: str | None = None,
) -> dict[str, object]:
    target_dir = science_store / "indexes" / "forcefields"
    target_dir.mkdir(parents=True, exist_ok=True)
    destination = target_dir / OPENFF_FORCEFIELD_FILE
    manifest = target_dir / "forcefield_manifest.tsv"
    python = python_command or os.environ.get("GENOLIGAND_PYTHON") or sys.executable
    validation = validate_openff_forcefield(destination, python) if path_has_payload(destination) else {
        "status": "needs_repair",
        "reason": "offxml_missing",
        "loader_smoke": "not_run",
    }
    if validation.get("status") != "ready":
        result = copy_or_download(
            OPENFF_FORCEFIELD_URL,
            destination,
            OPENFF_FORCEFIELD_SHA256,
            validator=lambda path: validate_openff_forcefield(path, python),
        )
        validation = validate_openff_forcefield(destination, python) if result.get("status") == "ready" else {
            "status": "needs_repair",
            "reason": str(result.get("status", "download_failed")),
            "loader_smoke": "failed",
        }
    else:
        result = {
            "status": "ready",
            "bytes": destination.stat().st_size,
            "sha256": sha256_file(destination),
            "reused": True,
        }
    status = "ready" if result.get("status") == "ready" and validation.get("status") == "ready" else "needs_repair"
    row = {
        "forcefield_id": "openff_sage_2_2_1",
        "file": destination.name,
        "status": status,
        "bytes": str(result.get("bytes", 0)),
        "expected_sha256": OPENFF_FORCEFIELD_SHA256,
        "actual_sha256": str(validation.get("actual_sha256", result.get("sha256", ""))),
        "release": OPENFF_FORCEFIELD_RELEASE,
        "source_url": OPENFF_FORCEFIELD_URL,
        "loader_smoke": str(validation.get("loader_smoke", "failed")),
        "license": "CC-BY-4.0",
        "notes": str(validation.get("reason", "OpenFF Sage 2.2.1 SMIRNOFF force field")),
    }
    write_simple_tsv(
        manifest,
        [
            "forcefield_id",
            "file",
            "status",
            "bytes",
            "expected_sha256",
            "actual_sha256",
            "release",
            "source_url",
            "loader_smoke",
            "license",
            "notes",
        ],
        [row],
    )
    return {
        "status": row["status"],
        "file": str(destination),
        "manifest": str(manifest),
        "sha256": row["actual_sha256"],
        "expected_sha256": row["expected_sha256"],
        "loader_smoke": row["loader_smoke"],
        "license": row["license"],
    }


def build_mmseqs_target_databases(
    science_store: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    summary_cache: ResourceSummaryCache | None = None,
) -> list[dict[str, str]]:
    index_root = science_store / "indexes" / "mmseqs"
    manifest = index_root / "mmseqs_target_databases.tsv"
    previous_rows = {row.get("source_id", ""): row for row in read_tsv(manifest)}
    index_root.mkdir(parents=True, exist_ok=True)
    command = shutil.which("mmseqs", path=(env or os.environ).get("PATH"))
    builder_version = database_builder_version("mmseqs", command, env, wsl_distro)
    rows: list[dict[str, str]] = []
    for label, source in sequence_database_sources(science_store):
        db_path = index_root / safe_file_stem(label) / "target"
        source_summary = resource_payload_summary(source, cache=summary_cache)
        source_fields = alignment_source_manifest_fields(source_summary, builder_version)
        source_namespace, source_class = mmseqs_source_provenance(label)
        previous = previous_rows.get(label, {})
        migrated = legacy_alignment_manifest_can_migrate(
            "mmseqs",
            previous,
            science_store,
            source,
            db_path,
            command,
            env,
            wsl_distro,
            source_summary,
            builder_version,
        )
        force_rebuild = database_artifacts_present(db_path) and not (
            alignment_source_manifest_matches(previous, source_summary, builder_version) or migrated
        )
        if alignment_source_deferred(source):
            status, notes = "deferred_large_source", "source_exceeds_sync_index_threshold"
        else:
            status, notes = build_mmseqs_database(
                command,
                source,
                db_path,
                env,
                wsl_distro,
                force_rebuild=force_rebuild,
            )
            if status == "ready" and migrated:
                notes = "legacy_manifest_migrated_after_database_smoke"
        rows.append(
            {
                "source_id": label,
                "source_namespace": source_namespace,
                "source_class": source_class,
                "source_path": relative_path(science_store, source),
                **source_fields,
                "db_path": relative_path(science_store, db_path),
                "status": status,
                "notes": notes,
            }
        )
    write_simple_tsv(
        manifest,
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
        rows,
    )
    return rows


def mmseqs_source_provenance(source_id: str) -> tuple[str, str]:
    normalized = source_id.strip().lower()
    if normalized.startswith("uniprot_") or normalized == "uniprot":
        return "UniProt", "known"
    if normalized.startswith("biolip2_") or normalized == "biolip2":
        return "BioLiP2", "known"
    if normalized.startswith("metagenome_") or normalized == "metagenome":
        return "MGnify", "metagenome"
    raise ValueError(f"unsupported_mmseqs_source_provenance:{source_id}")


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


def build_foldseek_target_database(
    science_store: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    summary_cache: ResourceSummaryCache | None = None,
) -> list[dict[str, str]]:
    index_root = science_store / "indexes" / "foldseek"
    manifest = index_root / "foldseek_target_databases.tsv"
    previous_rows = {row.get("source_id", ""): row for row in read_tsv(manifest)}
    index_root.mkdir(parents=True, exist_ok=True)
    command = shutil.which("foldseek", path=(env or os.environ).get("PATH"))
    builder_version = database_builder_version("foldseek", command, env, wsl_distro)
    source = science_store / "downloads" / "rcsb" / "mmcif"
    db_path = index_root / "rcsb_mmcif" / "target"
    scope_snapshot = read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
    source_summary = resource_payload_summary(source, cache=summary_cache)
    source_fields = alignment_source_manifest_fields(source_summary, builder_version)
    previous = previous_rows.get("rcsb_mmcif", {})
    migrated = legacy_alignment_manifest_can_migrate(
        "foldseek",
        previous,
        science_store,
        source,
        db_path,
        command,
        env,
        wsl_distro,
        source_summary,
        builder_version,
    )
    force_rebuild = database_artifacts_present(db_path) and not (
        alignment_source_manifest_matches(previous, source_summary, builder_version) or migrated
    )
    scope_complete = bool(
        scope_snapshot.get("scope_count")
        and scope_snapshot.get("ready_count") == scope_snapshot.get("scope_count")
        and scope_snapshot.get("coordinate_digest") == source_summary.get("sha256")
    )
    if not scope_complete:
        status, notes = "failed", "rcsb_scope_snapshot_incomplete_or_digest_mismatch"
    else:
        status, notes = build_foldseek_database(
            command,
            source,
            db_path,
            env,
            wsl_distro,
            force_rebuild=force_rebuild,
        )
        if status == "ready" and migrated:
            notes = "legacy_manifest_migrated_after_database_smoke"
    rows = [
        {
            "source_id": "rcsb_mmcif",
            "source_namespace": "RCSB",
            "source_class": "structure",
            "source_path": relative_path(science_store, source),
            **source_fields,
            "scope_digest": str(scope_snapshot.get("scope_digest", "")),
            "db_path": relative_path(science_store, db_path),
            "status": status,
            "notes": notes,
        }
    ]
    write_simple_tsv(
        manifest,
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
            "scope_digest",
            "db_path",
            "status",
            "notes",
        ],
        rows,
    )
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
    *,
    force_rebuild: bool = False,
) -> tuple[str, str]:
    if not path_has_payload(source):
        return "failed", "source_missing"
    if command is None and not (os.name == "nt" and wsl_distro):
        return "failed", "mmseqs_executable_missing"
    artifacts_present = database_artifacts_present(db_path)
    if artifacts_present and not force_rebuild and database_prefix_ready("mmseqs", db_path, command, env, wsl_distro):
        return "ready", "already_available"
    if artifacts_present:
        if not database_staging_capacity_ready(source, db_path):
            return "failed", "insufficient_disk_for_atomic_rebuild"
        return rebuild_mmseqs_database(command, source, db_path, env, wsl_distro)
    if not database_staging_capacity_ready(source, db_path):
        return "failed", "insufficient_disk_for_database_build"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = db_path.parent / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    if os.name == "nt" and wsl_distro:
        created = run_wsl_database_command(wsl_distro, ["mmseqs", "createdb", wsl_path(source), wsl_path(db_path)])
        if created.returncode != 0:
            return "failed", process_note(created)
        indexed = run_wsl_database_command(wsl_distro, ["mmseqs", "createindex", wsl_path(db_path), wsl_path(tmp)])
        if indexed.returncode != 0:
            return database_ready_after_index_failure("mmseqs", db_path, indexed, command, env, wsl_distro)
        return database_ready_after_build("mmseqs", db_path, command, env, wsl_distro, "mmseqs_createdb_createindex_completed")
    if command is None:
        return "failed", "mmseqs_executable_missing"
    created = run_command([command, "createdb", str(source), str(db_path)], env=env)
    if created.returncode != 0:
        return "failed", process_note(created)
    indexed = run_command([command, "createindex", str(db_path), str(tmp)], env=env)
    if indexed.returncode != 0:
        return database_ready_after_index_failure("mmseqs", db_path, indexed, command, env, wsl_distro)
    return database_ready_after_build("mmseqs", db_path, command, env, wsl_distro, "mmseqs_createdb_createindex_completed")


def build_foldseek_database(
    command: str | None,
    source: Path,
    db_path: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    *,
    force_rebuild: bool = False,
) -> tuple[str, str]:
    if not path_has_payload(source):
        return "failed", "source_missing"
    if command is None and not (os.name == "nt" and wsl_distro):
        return "failed", "foldseek_executable_missing"
    artifacts_present = database_artifacts_present(db_path)
    if artifacts_present and not force_rebuild and database_prefix_ready("foldseek", db_path, command, env, wsl_distro):
        return "ready", "already_available"
    if artifacts_present:
        if not database_staging_capacity_ready(source, db_path):
            return "failed", "insufficient_disk_for_atomic_rebuild"
        return rebuild_foldseek_database(command, source, db_path, env, wsl_distro)
    if not database_staging_capacity_ready(source, db_path):
        return "failed", "insufficient_disk_for_database_build"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = db_path.parent / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    if os.name == "nt" and wsl_distro:
        created = run_wsl_database_command(wsl_distro, ["foldseek", "createdb", wsl_path(source), wsl_path(db_path)])
        if created.returncode != 0:
            return "failed", process_note(created)
        indexed = run_wsl_database_command(wsl_distro, ["foldseek", "createindex", wsl_path(db_path), wsl_path(tmp)])
        if indexed.returncode != 0:
            return database_ready_after_index_failure("foldseek", db_path, indexed, command, env, wsl_distro)
        return database_ready_after_build("foldseek", db_path, command, env, wsl_distro, "foldseek_createdb_createindex_completed")
    if command is None:
        return "failed", "foldseek_executable_missing"
    created = run_command([command, "createdb", str(source), str(db_path)], env=env)
    if created.returncode != 0:
        return "failed", process_note(created)
    indexed = run_command([command, "createindex", str(db_path), str(tmp)], env=env)
    if indexed.returncode != 0:
        return database_ready_after_index_failure("foldseek", db_path, indexed, command, env, wsl_distro)
    return database_ready_after_build("foldseek", db_path, command, env, wsl_distro, "foldseek_createdb_createindex_completed")


def rebuild_mmseqs_database(
    command: str | None,
    source: Path,
    db_path: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> tuple[str, str]:
    staged_db = staged_database_path(db_path)
    try:
        status, notes = build_mmseqs_database(command, source, staged_db, env, wsl_distro)
        if status != "ready":
            return "failed", f"rebuild_failed_previous_preserved:{notes}"
        cleanup_complete = publish_database_directory(staged_db, db_path)
        cleanup_note = "" if cleanup_complete else ";previous_backup_cleanup_pending"
        return "ready", f"atomic_rebuild_completed:{notes}{cleanup_note}"
    except Exception as err:  # noqa: BLE001 - preserve the current database and report repair failure
        return "failed", f"rebuild_failed_previous_preserved:{exception_note(err)}"
    finally:
        if staged_db.parent.exists():
            shutil.rmtree(staged_db.parent)


def rebuild_foldseek_database(
    command: str | None,
    source: Path,
    db_path: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> tuple[str, str]:
    staged_db = staged_database_path(db_path)
    try:
        status, notes = build_foldseek_database(command, source, staged_db, env, wsl_distro)
        if status != "ready":
            return "failed", f"rebuild_failed_previous_preserved:{notes}"
        cleanup_complete = publish_database_directory(staged_db, db_path)
        cleanup_note = "" if cleanup_complete else ";previous_backup_cleanup_pending"
        return "ready", f"atomic_rebuild_completed:{notes}{cleanup_note}"
    except Exception as err:  # noqa: BLE001 - preserve the current database and report repair failure
        return "failed", f"rebuild_failed_previous_preserved:{exception_note(err)}"
    finally:
        if staged_db.parent.exists():
            shutil.rmtree(staged_db.parent)


def staged_database_path(db_path: Path) -> Path:
    parent = db_path.parent
    staged_parent = parent.with_name(f".{parent.name}.rebuild-{os.getpid()}-{time.time_ns()}")
    return staged_parent / db_path.name


def database_staging_capacity_ready(source: Path, db_path: Path) -> bool:
    available = shutil.disk_usage(existing_path_for_disk_usage(db_path)).free
    current_bytes = directory_size(db_path.parent)
    source_bytes = directory_size(source)
    required = max(current_bytes, source_bytes * 2) + DISK_SAFETY_MARGIN_BYTES
    return required <= available


def publish_database_directory(staged_db: Path, db_path: Path) -> bool:
    staged_parent = staged_db.parent
    live_parent = db_path.parent
    backup = live_parent.with_name(f".{live_parent.name}.previous-{os.getpid()}-{time.time_ns()}")
    if backup.exists():
        shutil.rmtree(backup)
    if live_parent.exists():
        live_parent.replace(backup)
    try:
        staged_parent.replace(live_parent)
    except Exception:
        if backup.exists() and not live_parent.exists():
            backup.replace(live_parent)
        raise
    if backup.exists():
        try:
            shutil.rmtree(backup)
        except OSError:
            return False
    return True


def exception_note(err: Exception) -> str:
    return str(err).replace("\t", " ").replace("\r", " ").replace("\n", " ")[:240] or type(err).__name__


def database_ready_after_index_failure(
    tool: str,
    db_path: Path,
    indexed: subprocess.CompletedProcess[str],
    command: str | None,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> tuple[str, str]:
    if database_prefix_ready(tool, db_path, command, env, wsl_distro):
        return "ready", f"{tool}_createdb_ready_createindex_not_precomputed:{process_note(indexed)}"
    return "failed", process_note(indexed)


def database_ready_after_build(
    tool: str,
    db_path: Path,
    command: str | None,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    success_note: str,
) -> tuple[str, str]:
    if database_prefix_ready(tool, db_path, command, env, wsl_distro):
        return "ready", success_note
    return "failed", f"{tool}_database_smoke_failed"


def run_wsl_database_command(distro: str, command: list[str]) -> subprocess.CompletedProcess[str]:
    script = f'export PATH="{wsl_runtime_path_prefix()}:$PATH" && {bash_words(command)}'
    return run_wsl_user_shell(distro, script)


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


def database_prefix_ready(
    tool: str,
    prefix: Path,
    command: str | None,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> bool:
    companions = [prefix, Path(f"{prefix}.dbtype"), Path(f"{prefix}.index"), Path(f"{prefix}.lookup")]
    if any(not path.is_file() or path.stat().st_size <= 0 for path in companions):
        return False
    if Path(f"{prefix}.dbtype").stat().st_size != 4:
        return False
    index_companions = {Path(f"{prefix}.idx"), Path(f"{prefix}.idx.dbtype"), Path(f"{prefix}.idx.index")}
    index_artifacts = {path for path in prefix.parent.glob(f"{prefix.name}.idx*") if path.is_file()}
    if index_artifacts and index_artifacts != index_companions:
        return False
    index_present = bool(index_artifacts)
    if index_present and any(path.stat().st_size <= 0 for path in index_companions):
        return False
    if index_present and Path(f"{prefix}.idx.dbtype").stat().st_size != 4:
        return False
    return database_smoke_query(tool, prefix, command, env, wsl_distro, validate_index=index_present)


def database_smoke_query(
    tool: str,
    prefix: Path,
    command: str | None,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    *,
    validate_index: bool = False,
) -> bool:
    smoke_root = prefix.parent / f".smoke-{tool}-{os.getpid()}-{time.time_ns()}"
    smoke_root.mkdir(parents=True, exist_ok=False)
    subset = smoke_root / "subset"
    result = smoke_root / "result"
    tmp = smoke_root / "tmp"
    try:
        if not copy_database_subset(prefix, subset):
            return False
        if os.name == "nt" and wsl_distro:
            commands = [
                [tool, "search", wsl_path(subset), wsl_path(subset), wsl_path(result), wsl_path(tmp), "--threads", "1", "--max-seqs", "1"],
            ]
            if validate_index:
                commands.append(
                    [tool, "view", f"{wsl_path(prefix)}.idx", "--id-list", "0", "--idx-entry-type", "0"]
                )
            return all(run_wsl_database_command(wsl_distro, item).returncode == 0 for item in commands)
        if command is None:
            return False
        local_commands = [
            [command, "search", str(subset), str(subset), str(result), str(tmp), "--threads", "1", "--max-seqs", "1"],
        ]
        if validate_index:
            local_commands.append(
                [command, "view", f"{prefix}.idx", "--id-list", "0", "--idx-entry-type", "0"]
            )
        return all(run_command(item, env=env).returncode == 0 for item in local_commands)
    except (OSError, UnicodeError):
        return False
    finally:
        shutil.rmtree(smoke_root, ignore_errors=True)


def copy_database_subset(prefix: Path, subset: Path) -> bool:
    max_records = 64
    max_index_lines = 4096
    max_bytes = 128 * 1024 * 1024
    try:
        base_entries = database_index_entries(Path(f"{prefix}.index"), None, max_records, max_index_lines)
        if not base_entries:
            return False
        ordered_keys = [entry[0] for entry in base_entries]
        selected_keys = set(ordered_keys)
        lookup_rows = database_lookup_rows(Path(f"{prefix}.lookup"), ordered_keys, max_index_lines)
        if not lookup_rows:
            return False
        copied_bytes = copy_database_component(prefix, subset, base_entries, max_bytes)
        for suffix in ("_h", "_ss", "_ss_h", "_ca"):
            source_component = Path(f"{prefix}{suffix}")
            if not source_component.exists() or source_component.stat().st_size == 0:
                continue
            entries = database_index_entries(
                Path(f"{source_component}.index"), selected_keys, max_records, max_index_lines
            )
            if not entries or {entry[0] for entry in entries} != selected_keys:
                return False
            copied_bytes += copy_database_component(
                source_component,
                Path(f"{subset}{suffix}"),
                entries,
                max_bytes - copied_bytes,
            )
        Path(f"{subset}.lookup").write_text("".join(lookup_rows), encoding="utf-8")
        return copied_bytes <= max_bytes
    except (OSError, UnicodeError, ValueError):
        return False


def database_index_entries(
    index_path: Path,
    selected_keys: set[str] | None,
    max_records: int,
    max_lines: int,
) -> list[tuple[str, int, int]]:
    entries: list[tuple[str, int, int]] = []
    wanted = set(selected_keys) if selected_keys is not None else None
    with index_path.open("r", encoding="ascii", errors="strict") as handle:
        for _ in range(max_lines):
            line = handle.readline(4096)
            if not line:
                break
            if len(line) == 4096 and not line.endswith("\n"):
                raise ValueError("database index line exceeds smoke limit")
            fields = line.rstrip("\r\n").split("\t")
            if len(fields) < 3:
                raise ValueError("database index row is incomplete")
            key, offset_text, length_text = fields[:3]
            if wanted is not None and key not in wanted:
                continue
            offset = int(offset_text)
            length = int(length_text)
            if offset < 0 or length <= 0:
                raise ValueError("database index row has invalid bounds")
            entries.append((key, offset, length))
            if wanted is None and len(entries) >= max_records:
                break
            if wanted is not None:
                wanted.discard(key)
                if not wanted:
                    break
    return entries


def database_lookup_rows(lookup_path: Path, ordered_keys: list[str], max_lines: int) -> list[str]:
    wanted = set(ordered_keys)
    rows: dict[str, str] = {}
    with lookup_path.open("r", encoding="utf-8", errors="strict") as handle:
        for _ in range(max_lines):
            line = handle.readline(4096)
            if not line:
                break
            if len(line) == 4096 and not line.endswith("\n"):
                raise ValueError("database lookup line exceeds smoke limit")
            fields = line.rstrip("\r\n").split("\t")
            if len(fields) < 3:
                raise ValueError("database lookup row is incomplete")
            key = fields[0]
            if key in wanted:
                rows[key] = line if line.endswith("\n") else f"{line}\n"
                wanted.discard(key)
                if not wanted:
                    break
    return [rows[key] for key in ordered_keys] if not wanted else []


def copy_database_component(
    source: Path,
    destination: Path,
    entries: list[tuple[str, int, int]],
    max_bytes: int,
) -> int:
    dbtype = Path(f"{source}.dbtype")
    if not dbtype.is_file() or dbtype.stat().st_size != 4:
        raise ValueError("database component type is invalid")
    total_bytes = sum(length for _key, _offset, length in entries)
    if total_bytes <= 0 or total_bytes > max_bytes:
        raise ValueError("database smoke subset exceeds byte limit")
    destination.parent.mkdir(parents=True, exist_ok=True)
    new_offset = 0
    index_rows: list[str] = []
    with source.open("rb") as source_handle, destination.open("wb") as destination_handle:
        for key, offset, length in entries:
            source_handle.seek(offset)
            payload = source_handle.read(length)
            if len(payload) != length:
                raise ValueError("database component record is truncated")
            destination_handle.write(payload)
            index_rows.append(f"{key}\t{new_offset}\t{length}\n")
            new_offset += length
    shutil.copyfile(dbtype, Path(f"{destination}.dbtype"))
    Path(f"{destination}.index").write_text("".join(index_rows), encoding="ascii")
    return total_bytes


def database_builder_version(
    tool: str,
    command: str | None,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> str:
    if os.name == "nt" and wsl_distro:
        completed = run_wsl_database_command(wsl_distro, [tool, "version"])
    elif command is not None:
        completed = run_command([command, "version"], env=env)
    else:
        return "missing"
    text = f"{completed.stdout}{completed.stderr}".strip()
    return next((line.strip() for line in text.splitlines() if line.strip()), "unknown") if completed.returncode == 0 else "unknown"


def alignment_source_manifest_fields(source_summary: dict[str, object], builder_version: str) -> dict[str, str]:
    return {
        "manifest_schema_version": ALIGNMENT_MANIFEST_SCHEMA_VERSION,
        "source_digest": str(source_summary.get("sha256", "")),
        "source_files": str(source_summary.get("file_count", 0)),
        "source_bytes": str(source_summary.get("bytes", 0)),
        "builder_version": builder_version,
    }


def alignment_source_manifest_matches(
    row: dict[str, str],
    source_summary: dict[str, object],
    builder_version: str,
) -> bool:
    expected = alignment_source_manifest_fields(source_summary, builder_version)
    return bool(row) and all(row.get(key, "") == value for key, value in expected.items())


def legacy_alignment_manifest_can_migrate(
    tool: str,
    row: dict[str, str],
    science_store: Path,
    source: Path,
    db_path: Path,
    command: str | None,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    source_summary: dict[str, object] | None = None,
    builder_version: str = "",
) -> bool:
    if not row or row.get("status") != "ready" or row.get("manifest_schema_version"):
        return False
    expected = alignment_source_manifest_fields(
        source_summary or resource_payload_summary(source),
        builder_version,
    )
    provenance_fields = ("source_digest", "source_files", "source_bytes", "builder_version")
    if any(not row.get(field, "") or row.get(field, "") != expected[field] for field in provenance_fields):
        return False
    manifest_source = resolve_manifest_path(science_store, row.get("source_path", ""))
    manifest_db = resolve_manifest_path(science_store, row.get("db_path", ""))
    try:
        paths_match = bool(
            manifest_source
            and manifest_db
            and manifest_source.resolve() == source.resolve()
            and manifest_db.resolve() == db_path.resolve()
        )
    except OSError:
        return False
    return paths_match and database_prefix_ready(tool, db_path, command, env, wsl_distro)


def database_artifacts_present(db_path: Path) -> bool:
    return db_path.parent.is_dir() and any(
        child.name == db_path.name or child.name.startswith(f"{db_path.name}.") for child in db_path.parent.iterdir()
    )


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
    with atomic_text_writer(path) as handle:
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
        return path.stat().st_size if resource_manifest_file(path) else 0
    if not path.exists():
        return 0
    total = 0
    for child in path.rglob("*"):
        if child.is_file() and resource_manifest_file(child):
            total += child.stat().st_size
    return total


def path_has_payload(path: Path) -> bool:
    if path.is_file():
        return resource_manifest_file(path) and path.stat().st_size > 0
    if not path.is_dir():
        return False
    return any(resource_manifest_file(child) and child.stat().st_size > 0 for child in path.rglob("*") if child.is_file())


def file_count(path: Path) -> int:
    if path.is_file():
        return int(resource_manifest_file(path) and path.stat().st_size > 0)
    if not path.is_dir():
        return 0
    return sum(
        1
        for child in path.rglob("*")
        if child.is_file() and resource_manifest_file(child) and child.stat().st_size > 0
    )


def resource_manifest_file(path: Path) -> bool:
    name = path.name.lower()
    return not (
        name.endswith(".part")
        or ".part.quarantine-" in name
        or ".rebuild-" in path.as_posix()
        or ".previous-" in path.as_posix()
        or name.endswith(".attempt.json")
        or name.endswith(".identity.json")
        or name.endswith(".tmp")
    )


def seed_resource_file_fingerprint(
    cache: ResourceSummaryCache | None,
    path: Path,
    content_sha256: str,
) -> None:
    if cache is None or not path.is_file() or not content_sha256:
        return
    stat = path.stat()
    token = file_change_token(path, stat)
    if not token:
        return
    signature: tuple[ResourceSignature, ...] = (
        (path.name, stat.st_size, stat.st_mtime_ns, stat.st_ctime_ns, stat.st_dev, stat.st_ino, token),
    )
    digest = hashlib.sha256()
    digest.update(path.name.encode("utf-8"))
    digest.update(str(stat.st_size).encode("ascii"))
    digest.update(content_sha256.encode("ascii"))
    cache[str(path.resolve())] = (
        signature,
        {
            "file_count": 1,
            "bytes": stat.st_size,
            "sha256": digest.hexdigest(),
            "content_sha256": content_sha256,
        },
    )


def file_change_token(path: Path, stat: os.stat_result) -> str:
    if os.name != "nt":
        return f"ctime:{stat.st_ctime_ns}"
    try:
        import ctypes
        import msvcrt

        device_io_control = ctypes.windll.kernel32.DeviceIoControl
        device_io_control.argtypes = [
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.POINTER(ctypes.c_uint32),
            ctypes.c_void_p,
        ]
        device_io_control.restype = ctypes.c_int
        output = ctypes.create_string_buffer(1024)
        returned = ctypes.c_uint32()
        with path.open("rb") as handle:
            succeeded = device_io_control(
                ctypes.c_void_p(msvcrt.get_osfhandle(handle.fileno())),
                0x000900EB,  # FSCTL_READ_FILE_USN_DATA
                None,
                0,
                output,
                len(output),
                ctypes.byref(returned),
                None,
            )
        if not succeeded or returned.value < 32:
            return ""
        major_version = int.from_bytes(output.raw[4:6], "little")
        usn_offset = 24 if major_version == 2 else 40 if major_version in {3, 4} else -1
        if usn_offset < 0 or returned.value < usn_offset + 8:
            return ""
        usn = int.from_bytes(output.raw[usn_offset : usn_offset + 8], "little", signed=True)
        return f"usn:{major_version}:{usn}"
    except (AttributeError, OSError, ValueError):
        return ""


def resource_payload_summary(
    path: Path,
    progress: Callable[[int, int | None, str], None] | None = None,
    *,
    cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    if path.is_file():
        files = [path] if resource_manifest_file(path) and path.stat().st_size > 0 else []
        root = path.parent
    elif path.is_dir():
        files = sorted(
            child
            for child in path.rglob("*")
            if child.is_file() and resource_manifest_file(child) and child.stat().st_size > 0
        )
        root = path
    else:
        return {"file_count": 0, "bytes": 0, "sha256": ""}
    signature_rows: list[ResourceSignature] = []
    for child in files:
        stat = child.stat()
        signature_rows.append(
            (
                child.name if path.is_file() else child.relative_to(root).as_posix(),
                stat.st_size,
                stat.st_mtime_ns,
                stat.st_ctime_ns,
                stat.st_dev,
                stat.st_ino,
                file_change_token(child, stat),
            )
        )
    signature = tuple(signature_rows)
    cacheable = all(entry[6] for entry in signature)
    cache_key = str(path.resolve())
    cached = cache.get(cache_key) if cache is not None else None
    if cacheable and cached is not None and cached[0] == signature:
        return dict(cached[1])
    total_bytes = sum(entry[1] for entry in signature)
    if not files or total_bytes <= 0:
        return {"file_count": 0, "bytes": 0, "sha256": ""}
    digest = hashlib.sha256()
    bytes_done = 0
    if progress:
        progress(0, total_bytes, "正在记录来源文件")
    for child, entry in zip(files, signature, strict=True):
        rel = entry[0]
        size = entry[1]
        digest.update(rel.encode("utf-8"))
        digest.update(str(size).encode("ascii"))

        def file_progress(read_bytes: int, *, completed_before: int = bytes_done) -> None:
            if progress:
                progress(completed_before + read_bytes, total_bytes, f"正在校验 {rel}")

        child_signature = (entry,)
        child_key = str(child.resolve())
        child_cached = cache.get(child_key) if cache is not None else None
        content_sha256 = ""
        if child_cached is not None and child_cached[0] == child_signature:
            content_sha256 = str(child_cached[1].get("content_sha256", ""))
        if not content_sha256:
            content_sha256 = sha256_file_with_progress(child, file_progress)
        digest.update(content_sha256.encode("ascii"))
        if cache is not None and entry[6]:
            child_digest = hashlib.sha256()
            child_digest.update(child.name.encode("utf-8"))
            child_digest.update(str(size).encode("ascii"))
            child_digest.update(content_sha256.encode("ascii"))
            cache[child_key] = (
                child_signature,
                {
                    "file_count": 1,
                    "bytes": size,
                    "sha256": child_digest.hexdigest(),
                    "content_sha256": content_sha256,
                },
            )
        bytes_done += size
        if progress:
            progress(bytes_done, total_bytes, f"已记录 {rel}")
    summary: dict[str, object] = {"file_count": len(files), "bytes": total_bytes, "sha256": digest.hexdigest()}
    if path.is_file() and files:
        summary["content_sha256"] = content_sha256
    if cache is not None and cacheable:
        cache[cache_key] = (signature, dict(summary))
    elif cache is not None:
        cache.pop(cache_key, None)
    return summary


def load_resource_summary_cache(path: Path) -> ResourceSummaryCache:
    payload = read_json_object(path)
    if payload.get("schema_version") != SOURCE_FINGERPRINT_CACHE_SCHEMA_VERSION:
        return {}
    entries = payload.get("entries")
    if not isinstance(entries, dict):
        return {}
    cache: ResourceSummaryCache = {}
    for key, value in entries.items():
        if not isinstance(key, str) or not isinstance(value, dict):
            continue
        raw_signature = value.get("signature")
        raw_summary = value.get("summary")
        if not isinstance(raw_signature, list) or not isinstance(raw_summary, dict):
            continue
        try:
            signature = tuple(
                (
                    str(item[0]),
                    int(item[1]),
                    int(item[2]),
                    int(item[3]),
                    int(item[4]),
                    int(item[5]),
                    str(item[6]),
                )
                for item in raw_signature
                if isinstance(item, list) and len(item) == 7
            )
            summary = {
                "file_count": int(raw_summary.get("file_count", 0)),
                "bytes": int(raw_summary.get("bytes", 0)),
                "sha256": str(raw_summary.get("sha256", "")),
            }
            if raw_summary.get("content_sha256"):
                summary["content_sha256"] = str(raw_summary.get("content_sha256", ""))
        except (TypeError, ValueError):
            continue
        if signature and summary["sha256"]:
            cache[key] = (signature, summary)
    return cache


def save_resource_summary_cache(path: Path, cache: ResourceSummaryCache) -> None:
    write_json_atomic(
        path,
        {
            "schema_version": SOURCE_FINGERPRINT_CACHE_SCHEMA_VERSION,
            "updated_at": int(time.time()),
            "entries": {
                key: {
                    "signature": [list(item) for item in signature],
                    "summary": summary,
                }
                for key, (signature, summary) in sorted(cache.items())
                if signature and str(summary.get("sha256", ""))
            },
        },
    )


def write_science_resource_manifests(
    source_catalog: Path,
    source_sync_plan: Path,
    science_store: Path,
    progress: Callable[[str, int, int | None, str], None] | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    manifest_dir = science_store / "manifests"
    manifest_dir.mkdir(parents=True, exist_ok=True)
    source_file_manifest = manifest_dir / "source_file_manifest.tsv"
    hash_manifest = manifest_dir / "hash_manifest.tsv"
    license_manifest = manifest_dir / "license_manifest.tsv"
    download_executions = {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "download_execution_manifest.tsv")
        if row.get("source_id")
    }
    external_executions = {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "external_sync_execution_manifest.tsv")
        if row.get("source_id")
    }
    source_rows: list[dict[str, str]] = []
    for row in read_tsv(source_catalog):
        execution = download_executions.get(row.get("source_id", ""), {})
        source_rows.append(
            {
                "source_id": row.get("source_id", ""),
                "source_label": row.get("source_id", ""),
                "source_class": "direct_download",
                "release_or_date": row.get("release_or_date", ""),
                "release_identity": row.get("release_identity", "") or row.get("release_or_date", ""),
                "trust_source": row.get("trust_source", ""),
                "license_note": row.get("license_note", ""),
                "local_path": row.get("local_path", ""),
                "provenance": row.get("url", ""),
                "execution_status": execution.get("status", ""),
                "execution_sha256": execution.get("sha256", ""),
                "execution_release_identity": execution.get("release_identity", ""),
                "execution_trust_source": execution.get("trust_source", ""),
                "execution_validation_schema": execution.get("validation_schema", ""),
                "execution_validated_records": execution.get("validated_records", ""),
                "execution_validation_reason": execution.get("validation_reason", ""),
            }
        )
    for row in read_tsv(source_sync_plan):
        execution = external_executions.get(row.get("source_id", ""), {})
        source_rows.append(
            {
                "source_id": row.get("source_id", ""),
                "source_label": row.get("source_label", row.get("source_id", "")),
                "source_class": row.get("source_class", "external_sync"),
                "release_or_date": row.get("release_or_date", ""),
                "release_identity": row.get("release_or_date", ""),
                "trust_source": row.get("trust_source", ""),
                "license_note": row.get("license_note", ""),
                "local_path": row.get("target_rel_path") or row.get("ready_marker", ""),
                "provenance": row.get("source_url", ""),
                "execution_status": execution.get("status", ""),
                "execution_sha256": execution.get("sha256", ""),
                "execution_release_identity": execution.get("release_identity", ""),
                "execution_trust_source": execution.get("trust_source", ""),
                "execution_validation_schema": "external_sync_contract",
                "execution_validated_records": "1",
                "execution_validation_reason": "external_sync_evidence_validated",
            }
        )
    total = 0
    ready = 0
    hash_ready = 0
    license_ready = 0
    trust_ready = 0
    with atomic_text_writer(source_file_manifest) as source_handle, atomic_text_writer(
        hash_manifest
    ) as hash_handle, atomic_text_writer(license_manifest) as license_handle:
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
                "release_identity",
                "trust_source",
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

            summary = resource_payload_summary(local_path, source_progress, cache=summary_cache)
            file_count_value = int(summary["file_count"])
            bytes_count = int(summary["bytes"])
            digest = str(summary["sha256"])
            exists = file_count_value > 0 and bytes_count > 0
            evidence_digest = str(summary.get("content_sha256", "")) if local_path.is_file() else digest
            trusted = bool(
                row["release_identity"]
                and row["trust_source"]
                and row["execution_status"] == "ready"
                and row["execution_sha256"].lower() == evidence_digest.lower()
                and row["execution_release_identity"] == row["release_identity"]
                and row["execution_trust_source"] == row["trust_source"]
                and (
                    row["source_class"] != "direct_download"
                    or (
                        row["execution_validation_schema"] == SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION
                        and row["execution_validated_records"].isdigit()
                        and int(row["execution_validated_records"]) > 0
                        and row["execution_validation_reason"] == "domain_schema_and_records_valid"
                    )
                )
            )
            status = "ready" if exists and trusted else "untrusted" if exists else "missing"
            license_status = "recorded" if row["license_note"] else "missing"
            total += 1
            if exists:
                ready += 1
            if digest:
                hash_ready += 1
            if license_status == "recorded":
                license_ready += 1
            if trusted:
                trust_ready += 1
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
                "release_identity": row["release_identity"],
                "trust_source": row["trust_source"],
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
    status = (
        "ready"
        if total > 0
        and ready == total
        and hash_ready == total
        and license_ready == total
        and trust_ready == total
        else "needs_repair"
    )
    return {
        "status": status,
        "total": total,
        "ready": ready,
        "hash_ready": hash_ready,
        "license_ready": license_ready,
        "trust_ready": trust_ready,
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
    summary_cache: ResourceSummaryCache | None = None,
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
        target_rel = row.get("target_rel_path") or ""
        marker_rel = row.get("ready_marker") or target_rel
        target = science_store / target_rel if target_rel else None
        marker = science_store / marker_rel if marker_rel else target
        execution = execution_rows.get(row.get("source_id", ""))
        execution_status = execution.get("status", "") if execution else ""
        ready = external_plan_row_ready(
            row, target, marker, science_store, execution or {}, summary_cache
        )
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
        "status": "failed" if failed else "ready" if total and needs_action == 0 else "needs_sync",
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


def plinder_parquet_source(science_store: Path) -> Path | None:
    root = science_store / "downloads" / "plinder"
    if not root.exists():
        return None
    candidates = sorted(
        path
        for path in root.rglob("*.parquet")
        if path.name.lower() in {
            "annotation_table.parquet",
            "annotation_table_nonredundant.parquet",
        }
    )
    preferred = [path for path in candidates if path.name.lower() == "annotation_table.parquet"]
    return (preferred or candidates or [None])[0]


def plinder_canonical_index_ready(
    source: Path,
    canonical: Path,
    manifest_path: Path,
) -> bool:
    if not source.is_file() or not canonical.is_file() or not manifest_path.is_file():
        return False
    manifest = load_json(manifest_path)
    source_meta = manifest.get("source")
    canonical_meta = manifest.get("canonical")
    if (
        manifest.get("status") != "ready"
        or not isinstance(source_meta, dict)
        or not isinstance(canonical_meta, dict)
        or int(canonical_meta.get("row_count", 0) or 0) <= 0
    ):
        return False
    try:
        return (
            int(source_meta.get("size_bytes", -1)) == source.stat().st_size
            and str(source_meta.get("sha256", "")).lower() == sha256_file(source).lower()
            and int(canonical_meta.get("size_bytes", -1)) == canonical.stat().st_size
            and str(canonical_meta.get("sha256", "")).lower() == sha256_file(canonical).lower()
        )
    except (OSError, TypeError, ValueError):
        return False


def prepare_plinder_canonical_index(
    workspace_root: Path,
    science_store: Path,
    python_command: str,
    python_backend: str,
    wsl_distro: str | None,
) -> dict[str, object]:
    source = plinder_parquet_source(science_store)
    if source is None:
        return {"status": "not_applicable", "rebuilt": False, "reason": "plinder_source_absent"}
    canonical = science_store / "indexes" / "plinder_targets.tsv"
    manifest = science_store / "indexes" / "plinder_targets.manifest.json"
    if plinder_canonical_index_ready(source, canonical, manifest):
        payload = load_json(manifest)
        canonical_meta = payload.get("canonical", {})
        return {
            "status": "ready",
            "rebuilt": False,
            "reason": "plinder_canonical_reused",
            "rows": canonical_meta.get("row_count", 0) if isinstance(canonical_meta, dict) else 0,
            "manifest": str(manifest),
        }
    worker = workspace_root / "python" / "genoligand_workers" / "plinder_index_worker.py"
    if not worker.is_file():
        return {"status": "failed", "rebuilt": False, "reason": "plinder_worker_missing"}
    canonical.parent.mkdir(parents=True, exist_ok=True)
    arguments = [
        "--source",
        str(source),
        "--canonical",
        str(canonical),
        "--manifest",
        str(manifest),
    ]
    if python_backend == "wsl":
        if not wsl_distro:
            return {"status": "failed", "rebuilt": False, "reason": "plinder_wsl_distro_missing"}
        home = wsl_home(wsl_distro)
        if not home:
            return {"status": "failed", "rebuilt": False, "reason": "plinder_wsl_home_missing"}
        python_path = f"{home}/.local/state/genoligand/python-worker-env/bin/python"
        completed = run_wsl_command(
            wsl_distro,
            [
                python_path,
                wsl_path(worker),
                "--source",
                wsl_path(source),
                "--canonical",
                wsl_path(canonical),
                "--manifest",
                wsl_path(manifest),
            ],
        )
    elif python_command:
        completed = run_command([python_command, str(worker), *arguments])
    else:
        return {"status": "failed", "rebuilt": False, "reason": "plinder_python_missing"}
    if completed.returncode != 0 or not plinder_canonical_index_ready(source, canonical, manifest):
        return {
            "status": "failed",
            "rebuilt": False,
            "reason": process_note(completed) or "plinder_canonical_validation_failed",
        }
    payload = load_json(manifest)
    canonical_meta = payload.get("canonical", {})
    return {
        "status": "ready",
        "rebuilt": True,
        "reason": "plinder_canonical_built",
        "rows": canonical_meta.get("row_count", 0) if isinstance(canonical_meta, dict) else 0,
        "manifest": str(manifest),
    }


def alignment_database_manifests_ready(
    science_store: Path,
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    for tool, rel_path in [
        ("mmseqs", "indexes/mmseqs/mmseqs_target_databases.tsv"),
        ("foldseek", "indexes/foldseek/foldseek_target_databases.tsv"),
    ]:
        rows = read_tsv(science_store / rel_path)
        if not rows or any(row.get("status") != "ready" for row in rows):
            return False
        command = shutil.which(tool, path=(env or os.environ).get("PATH"))
        builder_version = database_builder_version(tool, command, env, wsl_distro)
        for row in rows:
            source = resolve_manifest_path(science_store, row.get("source_path", ""))
            db_path = resolve_manifest_path(science_store, row.get("db_path", ""))
            if source is None or db_path is None:
                return False
            source_summary = resource_payload_summary(source, cache=summary_cache)
            if not alignment_source_manifest_matches(row, source_summary, builder_version):
                return False
            if not database_prefix_ready(tool, db_path, command, env, wsl_distro):
                return False
    return True


def resolve_manifest_path(science_store: Path, value: str) -> Path | None:
    if not value.strip():
        return None
    path = Path(value)
    return path if path.is_absolute() else science_store / path


def science_index_readiness_rows(science_store: Path) -> list[dict[str, str]]:
    source_rows = read_tsv(science_store / "index_build_manifest.tsv")
    used_indexes = {
        str(row.get("index", ""))
        for row in source_rows
        if str(row.get("status", "")) == "used"
    }
    rows: list[dict[str, str]] = []
    for row in source_rows:
        notes = str(row.get("notes", ""))
        input_path = str(row.get("input_path", ""))
        output_path = str(row.get("output_path", ""))
        source = science_store / input_path
        try:
            row_count = int(str(row.get("rows", "0") or "0"))
        except ValueError:
            row_count = 0
        source_scope = "full_searchable_source" if large_index_source(source) else "summary_source"
        index_kind = "ui_projection_tsv" if output_path.endswith(".tsv") else "derived_index"
        raw_status = str(row.get("status", ""))
        optional_missing = bool(
            raw_status in {"missing", "not_found", "not_applicable", "not_selected", "empty"}
            and str(row.get("index", "")) in used_indexes
            and any(token in notes.lower() for token in ("missing", "not_found", "optional", "candidate", "empty"))
        )
        if optional_missing:
            readiness_status = "not_selected"
            reason = "alternate_candidate_selected"
        elif raw_status != "used":
            readiness_status = "incomplete"
            reason = "index_build_status_not_used"
        elif "index_row_limit_reached" in notes:
            readiness_status = "projection_only"
            reason = "index_row_limit_reached"
        elif row_count <= 10_000 and large_index_source(source):
            readiness_status = "projection_only"
            reason = "large_source_small_tsv_projection"
        else:
            readiness_status = "full_index"
            reason = "current_index_rows_available"
        rows.append(
            {
                "index": str(row.get("index", "")),
                "input_path": input_path,
                "output_path": output_path,
                "rows": str(row_count),
                "source_scope": source_scope,
                "index_kind": index_kind,
                "readiness_status": readiness_status,
                "reason": reason,
                "notes": notes,
            }
        )
    return rows


def write_science_index_readiness_manifest(science_store: Path, rows: list[dict[str, str]] | None = None) -> None:
    write_simple_tsv(
        science_store / "index_readiness_manifest.tsv",
        [
            "index",
            "input_path",
            "output_path",
            "rows",
            "source_scope",
            "index_kind",
            "readiness_status",
            "reason",
            "notes",
        ],
        rows if rows is not None else science_index_readiness_rows(science_store),
    )


def partial_legacy_index_manifest_present(science_store: Path) -> bool:
    rows = science_index_readiness_rows(science_store)
    write_science_index_readiness_manifest(science_store, rows)
    return any(row.get("readiness_status") in {"projection_only", "incomplete"} for row in rows)


def science_readiness_evidence(science_store: Path) -> tuple[list[dict[str, str]], str]:
    evidence = [
        {"path": rel_path, "sha256": sha256_file(science_store / rel_path)}
        for rel_path in SCIENCE_READINESS_EVIDENCE_PATHS
        if (science_store / rel_path).is_file()
    ]
    digest_input = "".join(f"{row['path']}\t{row['sha256']}\n" for row in evidence)
    return evidence, hashlib.sha256(digest_input.encode("utf-8")).hexdigest()


def evaluate_installer_science_store_readiness(
    science_store: Path,
    *,
    persist: bool = True,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    blockers: list[dict[str, str]] = []
    blocker_codes: set[str] = set()

    def block(code: str, component: str, evidence: str, repair_action: str) -> None:
        if code in blocker_codes:
            return
        blocker_codes.add(code)
        blockers.append(
            {
                "code": code,
                "component": component,
                "evidence": evidence,
                "repair_action": repair_action,
            }
        )

    source_rows = read_tsv(science_store / "source_requirements.tsv")
    source_manifest_rows = {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "manifests/source_file_manifest.tsv")
        if row.get("source_id", "")
    }
    grouped: dict[str, list[dict[str, str]]] = {}
    for row in source_rows:
        logical = row.get("logical_source") or row.get("source_id") or "unknown_source"
        grouped.setdefault(logical, []).append(row)
    sources: list[dict[str, object]] = []
    if not source_rows:
        block(
            "source_requirements_empty",
            "source_requirements",
            "source_requirements.tsv has no data rows",
            "prepare_science_store",
        )
    for logical, repair_action in REQUIRED_LOGICAL_SOURCES.items():
        if logical not in grouped:
            grouped[logical] = []
            block(
                f"source_required:{logical}",
                "source_requirements",
                f"required logical source {logical} is absent",
                repair_action,
            )
    for logical, candidates in sorted(grouped.items()):
        selected = next((row for row in candidates if row.get("status") == "ready"), None)
        source_manifest = source_manifest_rows.get(logical, {})
        source_digest = source_manifest.get("sha256", "").strip().lower()
        release_identity = source_manifest.get("release_identity", "").strip()
        trust_source = source_manifest.get("trust_source", "").strip()
        source_manifest_ready = bool(
            source_manifest.get("status") == "ready"
            and re.fullmatch(r"[0-9a-f]{64}", source_digest)
            and release_identity
            and trust_source
        )
        candidate_rows = []
        for row in candidates:
            status = row.get("status", "missing")
            if selected is not None and row is not selected and status != "ready":
                status = "not_selected"
            candidate_rows.append({"source_id": row.get("source_id", ""), "status": status})
        required = logical in REQUIRED_LOGICAL_SOURCES
        source_status = "ready" if selected is not None and (not required or source_manifest_ready) else "needs_repair"
        sources.append(
            {
                "logical_source": logical,
                "status": source_status,
                "selected_candidate": selected.get("source_id", "") if selected else "",
                "candidates": candidate_rows,
                "source_manifest_status": source_manifest.get("status", "missing"),
                "source_digest": source_digest,
                "release_identity": release_identity,
                "trust_source": trust_source,
            }
        )
        if selected is None and candidates:
            repair = next((row.get("repair_action", "") for row in candidates if row.get("repair_action")), "prepare_source")
            block(
                f"source_required:{logical}",
                "source_requirements",
                f"logical source {logical} has no ready candidate",
                repair,
            )
        if required and not source_manifest_ready:
            missing_fields = []
            if source_manifest.get("status") != "ready":
                missing_fields.append("status")
            if not re.fullmatch(r"[0-9a-f]{64}", source_digest):
                missing_fields.append("sha256")
            if not release_identity:
                missing_fields.append("release_identity")
            if not trust_source:
                missing_fields.append("trust_source")
            block(
                f"source_required:{logical}",
                "source_manifest",
                f"manifests/source_file_manifest.tsv invalid fields: {','.join(missing_fields)}",
                REQUIRED_LOGICAL_SOURCES[logical],
            )

    status_payload = load_json(science_store / "install_status.json")
    missing = status_payload.get("missing_components")
    if status_payload.get("status") != "ready" or (isinstance(missing, list) and missing):
        block(
            "install_status_not_ready",
            "install_status",
            str(status_payload.get("status", "missing")),
            "prepare_science_store",
        )

    index_rows = science_index_readiness_rows(science_store)
    write_science_index_readiness_manifest(science_store, index_rows)
    if not index_rows:
        block("index_build_manifest_empty", "indexes", "index_build_manifest.tsv has no data rows", "build_indexes")
    present_indexes = {row.get("index", "") for row in index_rows}
    for index_id in sorted(REQUIRED_INDEX_IDS - present_indexes):
        block(f"index_required:{index_id}", "indexes", f"required index {index_id} is absent", "build_indexes")
    for row in index_rows:
        status = row.get("readiness_status", "incomplete")
        if status in {"projection_only", "incomplete"}:
            block(
                f"index_{status}:{row.get('index', 'unknown')}",
                "indexes",
                f"{row.get('input_path', '')}:{row.get('reason', '')}",
                "build_full_searchable_index",
            )
    for rel_path in [
        "indexes/known_targets.tsv",
        "indexes/metagenome_targets.tsv",
        "indexes/structure_targets.tsv",
        "indexes/tool_versions.tsv",
    ]:
        if not tsv_has_data_rows(science_store / rel_path):
            block(f"required_table_empty:{rel_path}", "indexes", rel_path, "prepare_science_store")
    tool_rows = {row.get("tool", ""): row for row in read_tsv(science_store / "indexes/tool_versions.tsv")}
    for tool_id in sorted(REQUIRED_TOOL_IDS):
        if tool_id not in tool_rows or tool_rows[tool_id].get("status") != "ready":
            block(f"tool_required:{tool_id}", "tools", tool_id, "install_or_repair_scientific_tools")

    for tool, rel_path in [
        ("mmseqs", "indexes/mmseqs/mmseqs_target_databases.tsv"),
        ("foldseek", "indexes/foldseek/foldseek_target_databases.tsv"),
    ]:
        rows = read_tsv(science_store / rel_path)
        if not rows:
            block(f"alignment_manifest_empty:{tool}", "alignment", rel_path, "build_alignment_database")
            continue
        for row in rows:
            required = [
                "manifest_schema_version",
                "source_namespace",
                "source_class",
                "source_digest",
                "source_files",
                "source_bytes",
                "builder_version",
                "db_path",
            ]
            if tool == "foldseek":
                required.append("scope_digest")
            if (
                row.get("status") != "ready"
                or row.get("manifest_schema_version") != ALIGNMENT_MANIFEST_SCHEMA_VERSION
                or any(not row.get(field, "") for field in required)
            ):
                block(
                    f"alignment_manifest_invalid:{tool}:{row.get('source_id', 'unknown')}",
                    "alignment",
                    rel_path,
                    "migrate_or_rebuild_alignment_database",
                )

    rcsb_root = science_store / "downloads/rcsb/mmcif"
    rcsb_snapshot = read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
    rcsb_scope_rows = read_tsv(science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv")
    rcsb_source_summary = resource_payload_summary(rcsb_root, cache=summary_cache)
    rcsb_partials = [
        path
        for path in rcsb_root.rglob("*")
        if path.is_file() and (path.name.lower().endswith(".part") or ".part.quarantine-" in path.name.lower())
    ] if rcsb_root.is_dir() else []
    try:
        rcsb_scope_count = int(rcsb_snapshot.get("scope_count", 0) or 0)
    except (TypeError, ValueError):
        rcsb_scope_count = 0
    if (
        rcsb_snapshot.get("schema_version") != "genoligand_rcsb_scope_snapshot_v1"
        or not rcsb_scope_count
        or rcsb_snapshot.get("scope_count") != rcsb_snapshot.get("ready_count")
        or len(rcsb_scope_rows) != rcsb_scope_count
        or not re.fullmatch(r"[0-9a-f]{64}", str(rcsb_snapshot.get("coordinate_digest", "")).lower())
        or rcsb_snapshot.get("coordinate_files") != rcsb_source_summary.get("file_count")
        or rcsb_snapshot.get("coordinate_bytes") != rcsb_source_summary.get("bytes")
        or rcsb_snapshot.get("coordinate_digest") != rcsb_source_summary.get("sha256")
    ):
        block(
            "rcsb_scope_incomplete",
            "rcsb",
            f"ready={rcsb_snapshot.get('ready_count', 0)} scope={rcsb_snapshot.get('scope_count', 0)}",
            "sync_rcsb_ligand_bound_scope",
        )
    if rcsb_partials:
        block(
            "rcsb_partial_present",
            "rcsb",
            f"partial_files={len(rcsb_partials)}",
            "resume_or_clean_rcsb_partial_downloads",
        )

    blockers.sort(key=lambda item: (item["code"], item["evidence"]))
    evidence_manifest, evidence_digest = science_readiness_evidence(science_store)
    payload: dict[str, object] = {
        "schema_version": SCIENCE_READINESS_SCHEMA_VERSION,
        "status": "ready" if not blockers else "needs_repair",
        "ready": not blockers,
        "generated_at": int(time.time()),
        "evidence_manifest": evidence_manifest,
        "evidence_digest": evidence_digest,
        "sources": sources,
        "indexes": index_rows,
        "blockers": blockers,
        "repair_actions": sorted({item["repair_action"] for item in blockers if item["repair_action"]}),
    }
    if persist:
        write_json_atomic(science_store / "science_store_readiness.json", payload)
    return payload


def large_index_source(path: Path) -> bool:
    try:
        return path.exists() and path.stat().st_size > partial_index_large_source_bytes(path)
    except OSError:
        return False


def partial_index_large_source_bytes(path: Path | None = None) -> int:
    raw = os.environ.get("GENOLIGAND_PARTIAL_INDEX_LARGE_SOURCE_BYTES", "").strip()
    if not raw:
        if path is not None and is_bindingdb_bulk_source(path):
            return DEFAULT_BINDINGDB_PARTIAL_INDEX_LARGE_SOURCE_BYTES
        return DEFAULT_PARTIAL_INDEX_LARGE_SOURCE_BYTES
    try:
        return max(0, int(raw))
    except ValueError:
        return DEFAULT_PARTIAL_INDEX_LARGE_SOURCE_BYTES


def is_bindingdb_bulk_source(path: Path) -> bool:
    normalized = str(path).replace("\\", "/").lower()
    return "/bindingdb/" in normalized and "bindingdb_all" in normalized


def existing_science_indexes_ready(
    science_store: Path,
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    readiness = evaluate_installer_science_store_readiness(science_store, summary_cache=summary_cache)
    non_alignment_blockers = [
        item
        for item in readiness.get("blockers", [])
        if isinstance(item, dict) and item.get("component") != "alignment"
    ]
    if non_alignment_blockers:
        return False
    if not alignment_database_manifests_ready(science_store, env, wsl_distro, summary_cache):
        return False
    if partial_legacy_index_manifest_present(science_store):
        return False
    return True


def base_science_indexes_ready(science_store: Path) -> bool:
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
    chembl_query_database = payload.get("chembl_query_database")
    science_store_readiness = payload.get("science_store_readiness")
    runtime_binary_manifest = payload.get("runtime_binary_manifest")
    download_status = downloads.get("status") if isinstance(downloads, dict) else None
    external_status = external_sources.get("status") if isinstance(external_sources, dict) else None
    resource_status = resources.get("status") if isinstance(resources, dict) else None
    tool_status = tool_environment.get("status") if isinstance(tool_environment, dict) else None
    source_manifest_status = source_manifests.get("status") if isinstance(source_manifests, dict) else None
    alignment_status = alignment_databases.get("status") if isinstance(alignment_databases, dict) else None
    forcefield_status = forcefields.get("status") if isinstance(forcefields, dict) else None
    chembl_query_status = (
        chembl_query_database.get("status") if isinstance(chembl_query_database, dict) else None
    )
    readiness_status = (
        science_store_readiness.get("status") if isinstance(science_store_readiness, dict) else None
    )
    runtime_status = (
        runtime_binary_manifest.get("status") if isinstance(runtime_binary_manifest, dict) else None
    )
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
        and chembl_query_status == "ready"
        and readiness_status == "ready"
        and runtime_status == "ready"
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
    completed = status == "ready" and total > 0 and ready == total and deferred == 0
    return {
        "completed": completed,
        "ready": ready,
        "deferred": deferred,
        "total": total,
        "next_action": "继续" if completed else "运行安装/修复",
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
        and alignment_status == "ready"
        and forcefield_status == "ready"
        and source_manifest_status == "ready"
    ):
        status = "ready"
        summary = "科学仓库已就绪。"
        next_action = "打开 WebUI 并输入研究对象"
    elif download_status == "skipped":
        status = "needs_repair"
        summary = "本次未执行科学资源下载，科学仓库不能标记为完成。"
        next_action = "保持联网后重新运行安装/修复"
    elif download_status != "ready":
        status = "needs_repair"
        summary = "科学资源下载状态尚未闭合。"
        next_action = "重新运行安装/修复并完成资源校验"
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
    elif external_status != "ready":
        status = "needs_repair"
        summary = "大型科学资源同步状态尚未闭合。"
        next_action = "补齐同步清单和资源后重新运行安装/修复"
    elif source_manifest_status != "ready":
        status = "needs_repair"
        summary = "科学资源来源、完整性或许可证记录尚未闭合。"
        next_action = "补齐资源文件、来源记录和许可证记录后重新运行安装/修复"
    elif resource_status != "ready":
        status = "needs_repair"
        summary = "科学资源索引仍有缺项。"
        next_action = "补齐科学资源后重新运行安装/修复"
    elif store_status == "ready" and missing_count == 0:
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


def existing_path_for_disk_usage(path: Path) -> Path:
    current = path
    while not current.exists() and current.parent != current:
        current = current.parent
    return current


def capacity_item(
    component: str,
    action: str,
    download_bytes: int,
    staging_bytes: int,
    reclaimable_bytes: int,
    available_bytes: int,
) -> dict[str, object]:
    required_peak = max(0, download_bytes + staging_bytes - reclaimable_bytes) + DISK_SAFETY_MARGIN_BYTES
    return {
        "component": component,
        "action": action,
        "download_bytes": max(0, int(download_bytes)),
        "staging_bytes": max(0, int(staging_bytes)),
        "reclaimable_bytes": max(0, int(reclaimable_bytes)),
        "required_peak_bytes": required_peak,
        "available_bytes": int(available_bytes),
    }


def evenly_spaced_sample(values: list[str], limit: int) -> list[str]:
    if limit <= 0 or not values:
        return []
    if len(values) <= limit:
        return values
    if limit == 1:
        return [values[len(values) // 2]]
    indexes = {
        (position * (len(values) - 1)) // (limit - 1)
        for position in range(limit)
    }
    return [values[index] for index in sorted(indexes)]


def rcsb_capacity_estimate(
    row: dict[str, str],
    target: Path,
    science_store: Path,
    *,
    resolve_remote: bool,
) -> dict[str, object]:
    scope = sorted(ligand_bound_pdb_scope(science_store))
    ready_sizes: list[int] = []
    missing_ids: list[str] = []
    for pdb_id in scope:
        upper = pdb_id.upper()
        coordinate = target / upper[1:3].lower() / f"{upper.lower()}.cif.gz"
        if valid_rcsb_coordinate(coordinate, upper):
            ready_sizes.append(coordinate.stat().st_size)
        else:
            missing_ids.append(upper)
    if not missing_ids:
        return {
            "action": "reuse_after_scope_validation" if scope else "resolve_remote_size_before_download",
            "download_bytes": 0,
            "scope_objects": len(scope),
            "missing_objects": 0,
            "observed_local_files": len(ready_sizes),
            "observed_local_mean_bytes": (sum(ready_sizes) + len(ready_sizes) - 1) // len(ready_sizes)
            if ready_sizes
            else 0,
            "remote_sample_files": 0,
            "remote_sample_mean_bytes": 0,
            "estimated_object_bytes": 0,
        }
    remote_sizes: list[int] = []
    if resolve_remote:
        template = row.get("source_url") or "https://files.rcsb.org/download/{pdb_id}.cif.gz"
        sample = evenly_spaced_sample(missing_ids, RCSB_CAPACITY_REMOTE_SAMPLE_SIZE)

        def resolve_size(pdb_id: str) -> int:
            url = template.replace("{pdb_id}", pdb_id).replace("{pdb_id_lower}", pdb_id.lower())
            return int(remote_resource_identity(url).get("content_length", 0) or 0)

        with concurrent.futures.ThreadPoolExecutor(max_workers=min(8, len(sample))) as pool:
            remote_sizes = [size for size in pool.map(resolve_size, sample) if size > 0]
    local_mean = (sum(ready_sizes) + len(ready_sizes) - 1) // len(ready_sizes) if ready_sizes else 0
    remote_mean = (sum(remote_sizes) + len(remote_sizes) - 1) // len(remote_sizes) if remote_sizes else 0
    observed_mean = max(local_mean, remote_mean)
    if observed_mean <= 0:
        action = "resolve_remote_size_before_download"
        estimated_object_bytes = 0
        download_bytes = 0
    else:
        estimated_object_bytes = (
            observed_mean * RCSB_CAPACITY_AGGREGATE_MARGIN_NUMERATOR
            + RCSB_CAPACITY_AGGREGATE_MARGIN_DENOMINATOR
            - 1
        ) // RCSB_CAPACITY_AGGREGATE_MARGIN_DENOMINATOR
        download_bytes = len(missing_ids) * estimated_object_bytes
        action = "download_missing_scope_empirical_estimate"
    return {
        "action": action,
        "download_bytes": download_bytes,
        "scope_objects": len(scope),
        "missing_objects": len(missing_ids),
        "observed_local_files": len(ready_sizes),
        "observed_local_mean_bytes": local_mean,
        "remote_sample_files": len(remote_sizes),
        "remote_sample_mean_bytes": remote_mean,
        "estimated_object_bytes": estimated_object_bytes,
    }


def installation_capacity_plan(
    science_store: Path,
    source_catalog: Path,
    source_sync_plan: Path,
    *,
    available_bytes: int | None = None,
    resolve_remote: bool = False,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    available = (
        int(available_bytes)
        if available_bytes is not None
        else int(shutil.disk_usage(existing_path_for_disk_usage(science_store)).free)
    )
    items: list[dict[str, object]] = []
    for row in read_tsv(source_catalog):
        destination = science_store / row.get("local_path", "")
        expected = row.get("sha256", "")
        destination_valid = (
            destination.is_file()
            and destination.stat().st_size > 0
            and (
                not expected
                or str(resource_payload_summary(destination, cache=summary_cache).get("content_sha256", "")).lower()
                == expected.lower()
            )
            and validate_catalog_payload(row, destination).get("status") == "ready"
        )
        if destination_valid:
            action = "reuse"
            download_bytes = 0
        else:
            identity = remote_resource_identity(row.get("url", "")) if resolve_remote else {}
            download_bytes = int(identity.get("content_length", 0) or 0)
            action = "download" if download_bytes else "resolve_remote_size_before_download"
        partial = destination.with_suffix(destination.suffix + ".part")
        reclaimable = partial.stat().st_size if partial.is_file() and not partial_identity_path(partial).is_file() else 0
        items.append(
            capacity_item(
                row.get("source_id", "direct_download"),
                action,
                download_bytes,
                0,
                reclaimable,
                available,
            )
        )

    previous_external = {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "external_sync_execution_manifest.tsv")
    }
    for row in read_tsv(source_sync_plan):
        source_id = row.get("source_id", "external_source")
        target = science_store / row.get("target_rel_path", "")
        marker = science_store / (row.get("ready_marker") or row.get("target_rel_path", ""))
        method = row.get("sync_method", "direct").strip().lower()
        if external_plan_row_ready(
            row,
            target,
            marker,
            science_store,
            previous_external.get(source_id, {}),
            summary_cache,
        ):
            items.append(capacity_item(source_id, "reuse_after_digest", 0, 0, 0, available))
            continue
        download_bytes = 0
        action = "resolve_remote_size_before_download"
        capacity_evidence: dict[str, object] = {}
        if method == "html_links" and resolve_remote:
            try:
                estimate = publisher_inventory_capacity(html_links_inventory(row), target)
                download_bytes = int(estimate["download_bytes"])
                action = str(estimate["action"])
                capacity_evidence = estimate
            except (OSError, urllib.error.URLError, urllib.error.HTTPError):
                pass
        elif method == "gcs_prefix" and resolve_remote:
            try:
                estimate = publisher_inventory_capacity(gcs_prefix_inventory(row), target)
                download_bytes = int(estimate["download_bytes"])
                action = str(estimate["action"])
                capacity_evidence = estimate
            except (OSError, urllib.error.URLError, urllib.error.HTTPError, ValueError, json.JSONDecodeError):
                pass
        elif method == "rcsb_ligand_bound_scope":
            estimate = rcsb_capacity_estimate(
                row,
                target,
                science_store,
                resolve_remote=resolve_remote,
            )
            download_bytes = int(estimate["download_bytes"])
            action = str(estimate["action"])
            capacity_evidence = estimate
        elif method == "direct" and resolve_remote:
            identity = remote_resource_identity(direct_source_location(row))
            download_bytes = int(identity.get("content_length", 0) or 0)
            action = "download" if download_bytes else action
        item = capacity_item(source_id, action, download_bytes, 0, 0, available)
        item.update({key: value for key, value in capacity_evidence.items() if key not in {"action", "download_bytes"}})
        items.append(item)

    manifest_rows = {
        row.get("source_id", ""): row
        for rel in (
            "indexes/mmseqs/mmseqs_target_databases.tsv",
            "indexes/foldseek/foldseek_target_databases.tsv",
        )
        for row in read_tsv(science_store / rel)
    }
    alignment_specs: list[tuple[str, Path, Path]] = [
        (source_id, source, science_store / "indexes/mmseqs" / safe_file_stem(source_id) / "target")
        for source_id, source in sequence_database_sources(science_store)
    ]
    alignment_specs.append(
        (
            "rcsb_mmcif",
            science_store / "downloads/rcsb/mmcif",
            science_store / "indexes/foldseek/rcsb_mmcif/target",
        )
    )
    for source_id, source, db_path in alignment_specs:
        if not path_has_payload(source):
            continue
        previous = manifest_rows.get(source_id, {})
        db_bytes = directory_size(db_path.parent)
        if previous.get("status") == "ready" and previous.get("manifest_schema_version") == ALIGNMENT_MANIFEST_SCHEMA_VERSION:
            action = "reuse_after_probe"
            staging = 0
        elif previous.get("status") == "ready" and db_bytes:
            action = "migrate_after_probe"
            staging = 0
        else:
            action = "rebuild"
            staging = max(db_bytes, directory_size(source) * 2)
        items.append(capacity_item(f"alignment:{source_id}", action, 0, staging, 0, available))

    chembl_archive = science_store / "downloads/chembl/chembl_sqlite.tar.gz"
    chembl_query = science_store / "downloads/chembl/chembl_query.sqlite"
    if chembl_archive.is_file() and chembl_archive.stat().st_size > 0 and not chembl_query.is_file():
        staging = 0
        try:
            with tarfile.open(chembl_archive, "r:gz") as archive:
                staging = max((item.size for item in archive.getmembers() if item.isfile() and item.name.endswith(".db")), default=0)
        except tarfile.TarError:
            staging = chembl_archive.stat().st_size * 4
        items.append(capacity_item("chembl_query_database", "extract_and_index", 0, staging * 2, 0, available))

    unresolved = [item for item in items if item["action"] == "resolve_remote_size_before_download"]
    total_download = sum(int(item["download_bytes"]) for item in items)
    peak_staging = max((int(item["staging_bytes"]) for item in items), default=0)
    reclaimable = sum(int(item["reclaimable_bytes"]) for item in items)
    required_peak = max(0, total_download + peak_staging - reclaimable) + DISK_SAFETY_MARGIN_BYTES
    status = "blocked" if unresolved or required_peak > available else "ready"
    payload = {
        "schema_version": "genoligand_installation_capacity_plan_v1",
        "status": status,
        "items": items,
        "download_bytes": total_download,
        "staging_bytes": peak_staging,
        "reclaimable_bytes": reclaimable,
        "required_peak_bytes": required_peak,
        "available_bytes": available,
        "safety_margin_bytes": DISK_SAFETY_MARGIN_BYTES,
        "unresolved_components": [str(item["component"]) for item in unresolved],
    }
    science_store.mkdir(parents=True, exist_ok=True)
    write_json_atomic(science_store / "installation_capacity_plan.json", payload)
    return payload


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
    parser.add_argument("--require-gpu-md", action="store_true")
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--no-tool-install", action="store_true")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default="19090")
    parser.add_argument("--start-webui", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--debug-output", action="store_true")
    args = parser.parse_args()
    require_cuda = gpu_md_required(args.require_gpu_md)

    workspace_root = Path(args.workspace_root).resolve()
    science_store = Path(args.science_store).resolve()
    runtime_dir = Path(args.runtime_dir).resolve()
    runtime_dir.mkdir(parents=True, exist_ok=True)
    python_env_dir = (
        Path(args.python_env_dir).resolve()
        if args.python_env_dir
        else default_python_env_dir(runtime_dir).resolve()
    )
    source_catalog = Path(args.source_catalog).resolve() if args.source_catalog else default_source_catalog(workspace_root, science_store)
    source_sync_plan = default_source_sync_plan(workspace_root, science_store)
    fingerprint_cache_path = science_store / "manifests/source_fingerprint_cache.json"
    resource_summary_cache = load_resource_summary_cache(fingerprint_cache_path)
    capacity_plan = installation_capacity_plan(
        science_store,
        source_catalog,
        source_sync_plan,
        resolve_remote=not args.no_download,
        summary_cache=resource_summary_cache,
    )
    save_resource_summary_cache(fingerprint_cache_path, resource_summary_cache)
    if args.dry_run or capacity_plan.get("status") != "ready":
        print(json.dumps(capacity_plan, ensure_ascii=False, indent=2))
        return 0 if args.dry_run and capacity_plan.get("status") == "ready" else 2
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
    download_result = (
        {
            "status": "skipped",
            "catalog": str(source_catalog),
            "manifest": "",
            "ready": 0,
            "failed": 0,
        }
        if args.no_download
        else prepare_downloads(source_catalog, science_store, resource_summary_cache)
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
        resource_summary_cache,
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
        resource_summary_cache,
    )
    chembl_query_result = publish_chembl_query_database(
        science_store,
        source_catalog,
        resource_summary_cache,
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
        require_cuda,
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
    try:
        cli_command = resolve_binary_command(
            args.gls_cli,
            "GENOLIGAND_GLS_CLI",
            "gls-cli",
            "gls-cli",
            workspace_root,
        )
        cli_command, cli_runtime_status = publish_runtime_binary(
            cli_command, runtime_dir, "gls-cli", workspace_root
        )
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
            workspace_root,
        )
    except BinaryBuildError as err:
        progress = write_install_progress(
            science_store,
            status="failed",
            stage="构建程序",
            current_resource="GenoLigand release binaries",
            completed_steps=4,
            total_steps=6,
            failed_steps=1,
            message=str(err),
        )
        print(
            json.dumps(
                {
                    "status": "failed",
                    "summary": "GenoLigand release 构建失败，安装已停止。",
                    "next_action": "修复构建错误后重新运行安装/修复",
                    "progress": progress,
                    "build_error": str(err),
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        release_install_lock(install_lock)
        return 2
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
    plinder_index_result = prepare_plinder_canonical_index(
        workspace_root,
        science_store,
        tool_python,
        python_backend,
        selected_wsl,
    )
    write_install_progress(
        science_store,
        status="running",
        stage="构建索引",
        current_resource="科学仓库索引",
        completed_steps=5,
        total_steps=6,
        message="正在构建数据库索引",
    )
    if plinder_index_result.get("status") == "failed":
        completed = subprocess.CompletedProcess(
            command,
            2,
            "",
            str(plinder_index_result.get("reason", "plinder_canonical_failed")),
        )
    elif plinder_index_result.get("rebuilt"):
        completed = run_command(command, env=command_env)
    elif existing_science_indexes_ready(science_store, command_env, selected_wsl, resource_summary_cache):
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
    alignment_database_result = build_persistent_alignment_databases(
        science_store,
        command_env,
        selected_wsl,
        resource_summary_cache,
    )
    alignment_consumer_result = alignment_worker_consumer_smoke(
        workspace_root,
        science_store,
        tool_python,
        python_backend,
        selected_wsl,
    )
    alignment_database_result["consumer_smoke"] = alignment_consumer_result
    if alignment_consumer_result.get("status") != "ready":
        alignment_database_result["status"] = "needs_repair"
    write_install_progress(
        science_store,
        status="running",
        stage="构建索引",
        current_resource="OpenFF 小分子力场",
        completed_steps=5,
        total_steps=6,
        message="正在准备 OpenFF SMIRNOFF 力场文件",
    )
    forcefield_result = ensure_openff_forcefield_file(science_store, tool_python)
    save_resource_summary_cache(fingerprint_cache_path, resource_summary_cache)
    external_source_summary = source_sync_summary(
        source_sync_plan,
        science_store,
        external_sync_result,
        resource_summary_cache,
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
    science_store_readiness = evaluate_installer_science_store_readiness(
        science_store,
        summary_cache=resource_summary_cache,
    )
    worker_wrapper = Path(tool_python) if tool_python and Path(tool_python).is_file() else None
    runtime_binary_manifest = write_runtime_binary_manifest(
        runtime_dir,
        [cli_runtime_status, server_runtime_status],
        worker_wrapper,
    )
    if (
        chembl_query_result.get("status") != "ready"
        or science_store_readiness.get("status") != "ready"
        or runtime_binary_manifest.get("status") != "ready"
    ):
        install_status = {
            **install_status,
            "status": "needs_repair",
            "summary": "安装门禁仍有需要修复的组件。",
            "next_action": "根据 readiness blocker 完成修复后重新运行安装/修复",
        }
    payload: dict[str, object] = {
        **install_status,
        "returncode": completed.returncode,
        "downloads": download_summary(download_result),
        "external_sources": external_source_summary,
        "source_manifests": source_manifest_result,
        "resources": resource_result,
        "tool_environment": tool_environment_result,
        "alignment_databases": alignment_database_result,
        "plinder_index": plinder_index_result,
        "forcefields": forcefield_result,
        "chembl_query_database": chembl_query_result,
        "science_store_readiness": science_store_readiness,
        "installation_capacity": capacity_plan,
        "cuda_toolkit": cuda_toolkit_summary(),
        "gpu_md_required": require_cuda,
        "runtime_binaries": {
            "gls_cli": cli_runtime_status,
            "gls_server": server_runtime_status,
        },
        "runtime_binary_manifest": runtime_binary_manifest,
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
    return run_runtime_instance(server_command, runtime_dir, "gls-server", env)


if __name__ == "__main__":
    raise SystemExit(main())
