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
import stat
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
        "component": "importlib_resources",
        "package": "importlib-resources==7.1.0",
        "import_name": "importlib_resources",
        "version_expr": "__import__('importlib.metadata', fromlist=['version']).version('importlib-resources')",
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
LIGAND_SIMILARITY_SCHEMA_VERSION = "genoligand_ligand_similarity_index_v2"
LIGAND_SIMILARITY_DATABASE_REL_PATH = "indexes/ligand_similarity/ligand_similarity.sqlite"
LIGAND_SIMILARITY_MANIFEST_REL_PATH = "indexes/ligand_similarity/ligand_similarity.manifest.json"
LIGAND_SIMILARITY_STANDARDIZATION = (
    "rdkit_cleanup_fragment_parent_reionize_canonical_tautomer_v1"
)
LIGAND_SIMILARITY_TAUTOMER_LIMIT = 100
LIGAND_SIMILARITY_SQLITE_VALIDATION_CACHE: set[tuple[str, int, str]] = set()
STRUCTURE_TARGETS_MANIFEST_REL_PATH = "indexes/structure_targets.manifest.json"
LEGACY_STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION = "genoligand_structure_targets_publication_v1"
STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION = "genoligand_structure_targets_publication_v2"
STRUCTURE_CONTRIBUTION_SOURCE_IDS = {
    "PLINDER": "plinder",
    "BioLiP2": "biolip2",
    "Binding MOAD": "binding_moad",
    "RCSB": "rcsb_ligand_bound",
}
STRUCTURE_TARGETS_PUBLICATION_VALIDATION_CACHE: dict[
    tuple[str, int, int, str, str, str, str, str], frozenset[str]
] = {}
SCIENCE_READINESS_EVIDENCE_PATHS = (
    "source_requirements.tsv",
    "manifests/source_file_manifest.tsv",
    "external_sync_execution_manifest.tsv",
    "install_status.json",
    "index_build_manifest.tsv",
    "index_readiness_manifest.tsv",
    "science_index_readiness.tsv",
    "indexes/known_targets.tsv",
    "indexes/metagenome_targets.tsv",
    "indexes/structure_targets.tsv",
    STRUCTURE_TARGETS_MANIFEST_REL_PATH,
    "indexes/tool_versions.tsv",
    "indexes/mmseqs/mmseqs_target_databases.tsv",
    "indexes/foldseek/foldseek_target_databases.tsv",
    "indexes/foldseek/rcsb_mmcif/indexed_pdb_ids.tsv",
    LIGAND_SIMILARITY_MANIFEST_REL_PATH,
    "manifests/source_retention_manifest.tsv",
    "downloads/rcsb/ligand_bound_pdb_scope.json",
    "downloads/rcsb/ligand_bound_pdb_scope.tsv",
    "downloads/rcsb/on_demand_policy.json",
)
RUNTIME_BINARY_MANIFEST_SCHEMA_VERSION = "genoligand_runtime_binary_manifest_v1"
SOURCE_FINGERPRINT_CACHE_SCHEMA_VERSION = "genoligand_source_fingerprint_cache_v1"
SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION = "genoligand_source_payload_validation_v1"
PUBLISHER_INVENTORY_CACHE_SCHEMA_VERSION = "genoligand_publisher_inventory_cache_v1"
PUBLISHER_INVENTORY_ATTEMPTS = 3
INSTALL_ATTEMPT_SCHEMA_VERSION = "genoligand_install_attempt_v1"
UNCHECKSUMMED_PUBLISHER_REQUIRED_PATHS = {
    "biolip2": {
        "BioLiP.txt.gz",
        "BioLiP_nr.txt.gz",
        "peptide.fasta.gz",
        "peptide_nr.fasta.gz",
        "protein.fasta.gz",
        "protein_nr.fasta.gz",
    },
}
PYTHON_MIN_VERSION = (3, 12)
PYTHON_MAX_VERSION_EXCLUSIVE = (3, 13)
DISK_SAFETY_MARGIN_BYTES = 5 * 1024 * 1024 * 1024
CHEMBL_QUERY_OUTPUT_MIN_BYTES = 256 * 1024 * 1024
CHEMBL_QUERY_OUTPUT_SOURCE_RATIO_NUMERATOR = 2
CHEMBL_QUERY_OUTPUT_SOURCE_RATIO_DENOMINATOR = 5
CHEMBL_QUERY_TRANSACTION_SCHEMA_VERSION = "genoligand_chembl_query_transaction_v1"
DATABASE_DIRECTORY_TRANSACTION_SCHEMA_VERSION = "genoligand_database_directory_transaction_v1"
SOURCE_RETENTION_SCHEMA_VERSION = "genoligand_source_retention_v1"
SOURCE_RETENTION_MANIFEST_REL_PATH = "manifests/source_retention_manifest.tsv"
RCSB_ON_DEMAND_MAX_CACHE_BYTES = 2 * 1024 * 1024 * 1024
RCSB_ON_DEMAND_MAX_CACHE_FILES = 4096
RCSB_ON_DEMAND_INITIAL_CACHE_FILES = 256
BIOLIP2_MMSEQS_SOURCE_PATHS = frozenset(
    {
        "downloads/biolip2/peptide.fasta.gz",
        "downloads/biolip2/peptide_nr.fasta.gz",
        "downloads/biolip2/protein.fasta.gz",
        "downloads/biolip2/protein_nr.fasta.gz",
    }
)
INDEXED_SOURCE_RETENTION_POLICIES: dict[str, dict[str, object]] = {
    "chembl": {
        "payloads": ("downloads/chembl/chembl_sqlite.tar.gz",),
        "derived": (
            LIGAND_SIMILARITY_DATABASE_REL_PATH,
            LIGAND_SIMILARITY_MANIFEST_REL_PATH,
        ),
    },
    "bindingdb": {
        "payloads": ("downloads/bindingdb/bindingdb_all.tsv.zip",),
        "derived": (
            LIGAND_SIMILARITY_DATABASE_REL_PATH,
            LIGAND_SIMILARITY_MANIFEST_REL_PATH,
        ),
    },
    "uniprot_human_reviewed": {
        "payloads": ("downloads/uniprot/uniprot_human_reviewed.fasta.gz",),
        "derived": ("indexes/mmseqs/mmseqs_target_databases.tsv",),
    },
    "binding_moad": {
        "payloads": ("downloads/binding_moad/bindingmoad.csv",),
        "derived": (
            "indexes/structure_targets.tsv",
            STRUCTURE_TARGETS_MANIFEST_REL_PATH,
            "index_build_manifest.tsv",
        ),
    },
    "biolip2": {
        "payloads": ("downloads/biolip2",),
        "derived": (
            "indexes/structure_targets.tsv",
            STRUCTURE_TARGETS_MANIFEST_REL_PATH,
            "index_build_manifest.tsv",
            "indexes/mmseqs/mmseqs_target_databases.tsv",
        ),
    },
    "plinder": {
        "payloads": ("downloads/plinder/index",),
        "derived": (
            "indexes/structure_targets.tsv",
            STRUCTURE_TARGETS_MANIFEST_REL_PATH,
            "index_build_manifest.tsv",
        ),
    },
    "metagenome_potential": {
        "payloads": ("downloads/metagenome/mgnify/mgy_clusters.fa.gz",),
        "derived": (
            "indexes/metagenome_targets.tsv",
            "indexes/mmseqs/mmseqs_target_databases.tsv",
        ),
    },
}
LEGACY_STRUCTURE_RETENTION_DERIVED_PATHS = {
    "binding_moad": frozenset(("indexes/structure_targets.tsv",)),
    "biolip2": frozenset(
        (
            "indexes/known_targets.tsv",
            "indexes/structure_targets.tsv",
            "indexes/mmseqs/mmseqs_target_databases.tsv",
        )
    ),
    "plinder": frozenset(
        (
            "indexes/plinder_targets.tsv",
            "indexes/plinder_targets.manifest.json",
            "indexes/structure_targets.tsv",
        )
    ),
}
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
WINDOWS_ATOMIC_REPLACE_RETRY_CODES = frozenset({5, 32, 33})
ATOMIC_REPLACE_RETRY_DELAYS = (0.01, 0.02, 0.04, 0.08, 0.16, 0.32, 0.4)
DOWNLOAD_RETRIES = 3
DOWNLOAD_TIMEOUT_SECONDS = 120
DOWNLOAD_USER_AGENT = "GenoLigand-science-store-installer/0.9"
RCSB_DOWNLOAD_WORKERS = 20
ProgressCallback = Callable[[str, int, int | None, str], None]
ResourceSignature = tuple[str, int, int, int, int, int, str]
ResourceSummaryCache = dict[str, tuple[tuple[ResourceSignature, ...], dict[str, object]]]
_COMPATIBLE_TLS_CONTEXT: ssl.SSLContext | None = None
_COMPATIBLE_TLS_CONTEXT_LOCK = threading.Lock()
_ATOMIC_PATH_LOCKS: dict[str, threading.Lock] = {}
_ATOMIC_PATH_LOCKS_GUARD = threading.Lock()


def compatible_tls_context() -> ssl.SSLContext:
    global _COMPATIBLE_TLS_CONTEXT
    context = _COMPATIBLE_TLS_CONTEXT
    if context is None:
        with _COMPATIBLE_TLS_CONTEXT_LOCK:
            context = _COMPATIBLE_TLS_CONTEXT
            if context is None:
                context = ssl.create_default_context()
                ignore_unexpected_eof = getattr(ssl, "OP_IGNORE_UNEXPECTED_EOF", None)
                if ignore_unexpected_eof is not None:
                    context.options |= ignore_unexpected_eof
                _COMPATIBLE_TLS_CONTEXT = context
    return context


def urlopen_with_compatible_tls(request: urllib.request.Request, timeout: int):
    url = getattr(request, "full_url", "")
    if str(url).lower().startswith("https://"):
        return urllib.request.urlopen(request, timeout=timeout, context=compatible_tls_context())
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


def default_installer_scratch_dir() -> Path:
    explicit = os.environ.get("GENOLIGAND_SCRATCH_DIR")
    if explicit:
        return Path(explicit)
    if os.name == "nt":
        local = os.environ.get("LOCALAPPDATA")
        if local:
            return Path(local) / "GenoLigand" / "installer-scratch"
    xdg_state = os.environ.get("XDG_STATE_HOME")
    if xdg_state:
        return Path(xdg_state) / "genoligand" / "installer-scratch"
    return Path.home() / ".local" / "state" / "genoligand" / "installer-scratch"


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


def require_clean_workspace_commit(root: Path) -> str:
    before = workspace_commit(root)
    if not before:
        raise BinaryBuildError("runtime_source_commit_unavailable")
    status = run_command(
        [
            "git",
            "-C",
            str(root),
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ]
    )
    if status.returncode != 0:
        raise BinaryBuildError("runtime_source_state_unavailable")
    if status.stdout:
        raise BinaryBuildError("runtime_source_tree_not_clean")
    after = workspace_commit(root)
    if after != before:
        raise BinaryBuildError("runtime_source_commit_changed")
    return before


def build_release_binary(root: Path, binary_name: str, cargo_package: str) -> Path | None:
    if not (root / "Cargo.toml").exists():
        return None
    expected_commit = require_clean_workspace_commit(root)
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
    if require_clean_workspace_commit(root) != expected_commit:
        raise BinaryBuildError("runtime_source_commit_changed_during_build")
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
    expected_commit = ""
    expected_version = ""
    if workspace_root is not None:
        expected_commit = require_clean_workspace_commit(workspace_root)
        expected_version = workspace_release_version(workspace_root)
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
        if workspace_root is not None:
            source_smoke = runtime_binary_smoke([str(source)], expected_version, expected_commit)
            if source_smoke["status"] != "ready":
                raise BinaryBuildError("runtime_binary_identity_mismatch")
            if require_clean_workspace_commit(workspace_root) != expected_commit:
                raise BinaryBuildError("runtime_source_commit_changed_before_publish")
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
            smoke = runtime_binary_smoke(published, expected_version, expected_commit)
            if smoke["status"] != "ready":
                raise BinaryBuildError("runtime_published_binary_identity_mismatch")
            status.update(
                {
                    "status": "ready",
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
    inherited_path: str,
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
    if science_path == inherited_path:
        science_path = ""
    elif inherited_path and science_path.endswith(os.pathsep + inherited_path):
        science_path = science_path[: -(len(inherited_path) + len(os.pathsep))]
    for entry in science_path.split(os.pathsep):
        if entry and entry not in path_prepend:
            path_prepend.append(entry)
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


def normalized_release_version(value: object) -> tuple[int, ...] | None:
    match = re.fullmatch(
        r"(?P<release>[0-9]+(?:\.[0-9]+)*)(?:\.dev-[0-9A-Za-z]+)?",
        str(value).strip(),
    )
    if not match:
        return None
    return tuple(int(part) for part in match.group("release").split("."))


def release_versions_compatible(expected: object, observed: object) -> bool:
    if str(expected) == str(observed):
        return True
    expected_release = normalized_release_version(expected)
    observed_release = normalized_release_version(observed)
    return expected_release is not None and expected_release == observed_release


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
    "cuda_context_ready": False,
    "cuda_context_platform": "",
    "cuda_context_failure": "",
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
    if payload["cuda_ready"]:
        context = None
        integrator = None
        try:
            system = openmm.System()
            system.addParticle(1.0)
            integrator = openmm.VerletIntegrator(0.001)
            context = openmm.Context(
                system,
                integrator,
                openmm.Platform.getPlatformByName("CUDA"),
            )
            context.setPositions([[0.0, 0.0, 0.0]])
            context.getState(getEnergy=True)
            payload["cuda_context_platform"] = context.getPlatform().getName()
            payload["cuda_context_ready"] = payload["cuda_context_platform"].lower() == "cuda"
        except Exception as exc:
            payload["cuda_context_failure"] = str(exc)
        finally:
            del context
            del integrator
    blocking_plugin_failures = [
        failure for failure in payload["plugin_load_failures"] if "opencl" not in failure.lower()
    ]
    if not payload["cuda_ready"]:
        payload["failure_reason"] = (
            "plugin_load_failures:" + "|".join(blocking_plugin_failures)
            if blocking_plugin_failures
            else "CUDA_platform_missing"
        )
    elif not payload["cuda_context_ready"]:
        payload["failure_reason"] = "CUDA_context_failed:" + (
            payload["cuda_context_failure"] or "CUDA_context_unavailable"
        )
    elif blocking_plugin_failures:
        payload["failure_reason"] = "plugin_load_failures:" + "|".join(blocking_plugin_failures)
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
        "cuda_context_ready": False,
        "cuda_context_platform": "",
        "cuda_context_failure": "",
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


def blocking_openmm_plugin_failures(probe: dict[str, object]) -> list[str]:
    failures = probe.get("plugin_load_failures", [])
    if not isinstance(failures, list):
        return [str(failures)] if failures else []
    return [str(failure) for failure in failures if "opencl" not in str(failure).lower()]


def openmm_cuda_probe_ready(probe: dict[str, object]) -> bool:
    context_ready = probe.get("cuda_context_ready", probe.get("cuda_ready"))
    return bool(
        probe.get("openmm_ready")
        and probe.get("cuda_ready")
        and context_ready
        and release_versions_compatible(OPENMM_CUDA_VERSION, probe.get("version", ""))
        and str(probe.get("plugin_dir", ""))
        and not blocking_openmm_plugin_failures(probe)
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
        f"cuda_context={'ready' if probe.get('cuda_context_ready', probe.get('cuda_ready')) else 'missing'}",
        f"cuda_context_platform={probe.get('cuda_context_platform', '')}",
        f"cuda_context_failure={probe.get('cuda_context_failure', '')}",
        f"openmm_plugin_dir={probe.get('plugin_dir', '')}",
        f"openmm_default_plugin_dir={probe.get('default_plugin_dir', '')}",
        f"loaded_plugins={','.join(str(item) for item in probe.get('loaded_plugins', []))}",
        f"plugin_load_failures={'|'.join(str(item) for item in probe.get('plugin_load_failures', []))}",
        f"blocking_plugin_load_failures={'|'.join(blocking_openmm_plugin_failures(probe))}",
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


def locked_source_checkout_commands(
    git: str,
    repository: str | Path,
    source: dict[str, str],
) -> list[list[str]]:
    repo = str(repository)
    return [
        [git, "-C", repo, "init", "--quiet"],
        [git, "-C", repo, "config", "core.autocrlf", "false"],
        [git, "-C", repo, "remote", "add", "origin", source["git_url"]],
        [git, "-C", repo, "fetch", "--quiet", "--depth", "1", "origin", source["commit"]],
        [git, "-C", repo, "checkout", "--quiet", "--detach", source["commit"]],
        [git, "-C", repo, "tag", "--force", source["version"], source["commit"]],
    ]


def run_locked_source_checkout_command(
    runner: Callable[[list[str]], subprocess.CompletedProcess[str]],
    command: list[str],
) -> subprocess.CompletedProcess[str]:
    transient_errors = (
        "gnutls_handshake() failed",
        "tls connection was non-properly terminated",
        "ssl_connect",
        "rpc failed",
        "early eof",
        "connection reset",
        "connection was reset",
        "curl 18",
        "curl 56",
        "remote end hung up unexpectedly",
    )
    max_attempts = 3 if "fetch" in command else 1
    for attempt in range(1, max_attempts + 1):
        completed = runner(command)
        if completed.returncode == 0:
            return completed
        output = f"{completed.stdout}\n{completed.stderr}".lower()
        if attempt == max_attempts or not any(marker in output for marker in transient_errors):
            return completed
        time.sleep(attempt)
    raise AssertionError("locked source checkout retry loop exhausted")


def locked_source_archive_command(
    git: str,
    repository: str | Path,
    archive: str | Path,
    commit: str,
) -> list[str]:
    return [
        git,
        "-c",
        "core.autocrlf=false",
        "-C",
        str(repository),
        "archive",
        "--format=tar",
        f"--output={archive}",
        commit,
    ]


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
    return not match or observed == "installed" or release_versions_compatible(match.group(1), observed)


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
            commands = locked_source_checkout_commands(git, repository, source)
            for command in commands:
                completed = run_locked_source_checkout_command(run_command, command)
                if completed.returncode != 0:
                    return {"status": "failed", "reason": f"locked_source_checkout_failed:{package}:{process_note(completed)}"}
            observed = run_command([git, "-C", str(repository), "rev-parse", f"{source['commit']}^{{commit}}"])
            if observed.returncode != 0 or observed.stdout.strip() != source["commit"]:
                return {"status": "failed", "reason": f"locked_source_commit_mismatch:{package}"}
            archive = build_root / package / "source.tar"
            archived = run_command(
                locked_source_archive_command(git, repository, archive, source["commit"])
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
        ["mkdir", "-p", "--", build_root],
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
                ["mkdir", "-p", "--", repository, wheel_dir],
            )
            if prepared.returncode != 0:
                return {"status": "failed", "reason": f"locked_source_prepare_failed:{package}"}
            commands = locked_source_checkout_commands("git", repository, source)
            for command in commands:
                completed = run_locked_source_checkout_command(
                    lambda current: run_wsl_command(distro, current), command
                )
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
                locked_source_archive_command("git", repository, archive, source["commit"]),
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
                ["find", wheel_dir, "-maxdepth", "1", "-type", "f", "-name", "*.whl", "-print"],
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
            ["rm", "-rf", "--", build_root],
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


def install_progress_payload(
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
    return payload


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
    science_store.mkdir(parents=True, exist_ok=True)
    payload = install_progress_payload(
        status=status,
        stage=stage,
        current_resource=current_resource,
        completed_steps=completed_steps,
        total_steps=total_steps,
        failed_steps=failed_steps,
        bytes_downloaded=bytes_downloaded,
        bytes_total=bytes_total,
        message=message,
    )
    write_json_atomic(science_store / "install_progress.json", payload)
    return payload


def write_capacity_attempt_state(
    science_store: Path,
    *,
    status: str,
    capacity_plan: dict[str, object] | None = None,
    error: str = "",
) -> None:
    missing = capacity_plan.get("unresolved_components", []) if capacity_plan else []
    missing_components = [str(value) for value in missing] if isinstance(missing, list) else []
    messages = {
        "running": "正在核对安装所需磁盘容量",
        "blocked": "磁盘容量或远端资源大小尚未满足安装条件",
        "failed": "容量预检执行失败",
        "capacity_ready": "容量预检已通过",
    }
    next_actions = {
        "running": "等待容量预检完成",
        "blocked": "补足磁盘容量或恢复发布方资源身份读取后重新运行安装/修复",
        "failed": "检查容量预检错误后重新运行安装/修复",
        "capacity_ready": "继续安装/修复",
    }
    now = int(time.time())
    write_json_atomic(
        science_store / "install_status.json",
        {
            "schema_version": INSTALL_ATTEMPT_SCHEMA_VERSION,
            "status": status,
            "install_ready": False,
            "stage": "容量预检",
            "summary": messages.get(status, status),
            "next_action": next_actions.get(status, "重新运行安装/修复"),
            "missing_components": missing_components,
            "capacity_plan": str(science_store / "installation_capacity_plan.json"),
            "required_peak_bytes": capacity_plan.get("required_peak_bytes") if capacity_plan else None,
            "available_bytes": capacity_plan.get("available_bytes") if capacity_plan else None,
            "error": error,
            "updated_at": now,
        },
    )
    write_install_progress(
        science_store,
        status="ready" if status == "capacity_ready" else status,
        stage="容量预检",
        current_resource="安装容量",
        completed_steps=1 if status == "capacity_ready" else 0,
        total_steps=1,
        failed_steps=1 if status in {"blocked", "failed"} else 0,
        message=messages.get(status, status),
    )


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


def atomic_path_lock(path: Path) -> threading.Lock:
    key = os.path.normcase(os.path.abspath(os.fspath(path)))
    with _ATOMIC_PATH_LOCKS_GUARD:
        return _ATOMIC_PATH_LOCKS.setdefault(key, threading.Lock())


def windows_atomic_replace_retryable(err: OSError) -> bool:
    return os.name == "nt" and getattr(err, "winerror", None) in WINDOWS_ATOMIC_REPLACE_RETRY_CODES


def fsync_directory_if_supported(directory: Path) -> None:
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
    try:
        descriptor = os.open(directory, flags)
    except OSError:
        return
    try:
        os.fsync(descriptor)
    except OSError:
        pass
    finally:
        os.close(descriptor)


def write_json_atomic_attempt(
    path: Path,
    temporary: Path,
    err: BaseException,
    replace_attempts: int,
) -> None:
    attempt = path.with_name(f"{path.name}.attempt.json")
    payload = {
        "status": "failed",
        "manifest": path.name,
        "error": str(err)[:500],
        "error_type": type(err).__name__,
        "winerror": getattr(err, "winerror", None),
        "errno": getattr(err, "errno", None),
        "replace_attempts": replace_attempts,
        "temporary": temporary.name,
        "failed_at": int(time.time()),
    }
    try:
        with attempt.open("w", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
    except OSError:
        pass


@contextmanager
def atomic_output_writer(path: Path, *, newline: str | None):
    path = Path(path)
    with atomic_path_lock(path):
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(
            f".{path.name}.{os.getpid()}.{threading.get_ident()}.{uuid.uuid4().hex}.tmp"
        )
        replace_attempts = 0
        try:
            with temporary.open("x", encoding="utf-8", newline=newline) as handle:
                yield handle
                handle.flush()
                os.fsync(handle.fileno())
            for delay in (*ATOMIC_REPLACE_RETRY_DELAYS, None):
                replace_attempts += 1
                try:
                    os.replace(temporary, path)
                    break
                except OSError as err:
                    if delay is None or not windows_atomic_replace_retryable(err):
                        raise
                    time.sleep(delay)
            fsync_directory_if_supported(path.parent)
            try:
                path.with_name(f"{path.name}.attempt.json").unlink()
            except FileNotFoundError:
                pass
        except BaseException as err:
            write_json_atomic_attempt(path, temporary, err, replace_attempts)
            raise
        finally:
            try:
                temporary.unlink()
            except (FileNotFoundError, OSError):
                pass


def write_json_atomic(path: Path, payload: dict[str, object]) -> None:
    with atomic_output_writer(path, newline=None) as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")


@contextmanager
def atomic_text_writer(path: Path):
    with atomic_output_writer(path, newline="") as handle:
        yield handle


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


def remote_resource_size(url: str) -> int:
    identity = remote_resource_identity(url)
    size = int(identity.get("content_length", 0) or 0)
    if size > 0:
        return size
    request = urllib.request.Request(
        url,
        headers={"User-Agent": DOWNLOAD_USER_AGENT, "Range": "bytes=0-0"},
    )
    try:
        with urlopen_with_compatible_tls(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
            content_range = str(response.headers.get("Content-Range", ""))
            match = re.fullmatch(r"bytes\s+0-0/(\d+)", content_range, re.IGNORECASE)
            if match and int(match.group(1)) > 0:
                return int(match.group(1))
            if getattr(response, "status", 200) == 200:
                content_length = str(response.headers.get("Content-Length", ""))
                if content_length.isdigit() and int(content_length) > 0:
                    return int(content_length)
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, OSError, ValueError):
        pass
    return 0


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


def partial_has_resumable_identity(partial: Path) -> bool:
    stored = read_json_object(partial_identity_path(partial))
    try:
        content_length = int(stored.get("content_length", 0) or 0)
        partial_bytes = partial.stat().st_size
    except (OSError, TypeError, ValueError):
        return False
    return bool(
        partial_bytes > 0
        and content_length > partial_bytes
        and stored.get("trusted") is True
        and re.fullmatch(r"[0-9a-f]{64}", str(stored.get("identity_digest", "")).lower())
        and str(stored.get("url", "")).lower().startswith("https://")
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


def chembl_query_target_staging_bytes(source_bytes: int, archive_bytes: int) -> int:
    projected_from_source = (
        max(0, int(source_bytes)) * CHEMBL_QUERY_OUTPUT_SOURCE_RATIO_NUMERATOR
        + CHEMBL_QUERY_OUTPUT_SOURCE_RATIO_DENOMINATOR
        - 1
    ) // CHEMBL_QUERY_OUTPUT_SOURCE_RATIO_DENOMINATOR
    return max(
        CHEMBL_QUERY_OUTPUT_MIN_BYTES,
        projected_from_source,
        max(0, int(archive_bytes)) * 2,
    )


def chembl_query_transaction_path(database: Path) -> Path:
    return database.with_name(f".{database.name}.transaction.json")


def chembl_file_identity(path: Path) -> tuple[int, str] | None:
    try:
        if not path.is_file():
            return None
        return path.stat().st_size, sha256_file(path)
    except OSError:
        return None


def chembl_identity_matches(
    identity: tuple[int, str] | None,
    payload: dict[str, object],
    prefix: str,
) -> bool:
    expected_bytes = payload.get(f"{prefix}_bytes")
    expected_sha256 = payload.get(f"{prefix}_sha256")
    return bool(
        identity is not None
        and type(expected_bytes) is int
        and expected_bytes >= 0
        and isinstance(expected_sha256, str)
        and re.fullmatch(r"[0-9a-f]{64}", expected_sha256)
        and identity == (expected_bytes, expected_sha256)
    )


def chembl_file_matches(path: Path, payload: dict[str, object], prefix: str) -> bool:
    return chembl_identity_matches(chembl_file_identity(path), payload, prefix)


def chembl_transaction_paths(
    database: Path,
    manifest: Path,
    payload: dict[str, object],
    scratch_root: Path | None = None,
) -> dict[str, Path] | None:
    token = payload.get("token")
    if (
        payload.get("schema_version") != CHEMBL_QUERY_TRANSACTION_SCHEMA_VERSION
        or not isinstance(token, str)
        or not re.fullmatch(r"[0-9]+-[0-9]+", token)
    ):
        return None
    paths = {
        "live_database": database,
        "live_manifest": manifest,
        "staging_database": database.with_name(f".{database.name}.rebuild-{token}"),
        "staging_manifest": manifest.with_name(f".{manifest.name}.rebuild-{token}"),
        "backup_database": database.with_name(f".{database.name}.backup-{token}"),
        "backup_manifest": manifest.with_name(f".{manifest.name}.backup-{token}"),
    }
    for field, path in paths.items():
        value = payload.get(field)
        if not isinstance(value, str) or Path(value).resolve() != path.resolve():
            return None
    scratch_value = payload.get("scratch_staging")
    if not isinstance(scratch_value, str) or Path(scratch_value).name != f"chembl-query-rebuild-{token}":
        return None
    if scratch_root is not None:
        scratch_staging = scratch_root / f"chembl-query-rebuild-{token}"
        if Path(scratch_value).resolve() != scratch_staging.resolve():
            return None
        paths["scratch_staging"] = scratch_staging
    return paths


def remove_chembl_transaction_file(path: Path) -> None:
    if not path.exists():
        return
    try:
        path.chmod(0o666)
    except OSError:
        pass
    path.unlink()


def cleanup_chembl_query_transaction(
    transaction: Path,
    paths: dict[str, Path],
) -> str:
    errors: list[str] = []
    staging_database = paths["staging_database"]
    for path in (
        staging_database,
        Path(f"{staging_database}-journal"),
        Path(f"{staging_database}-wal"),
        Path(f"{staging_database}-shm"),
        paths["staging_manifest"],
        paths["backup_database"],
        paths["backup_manifest"],
    ):
        try:
            remove_chembl_transaction_file(path)
        except OSError as err:
            errors.append(f"{path.name}:{exception_note(err)}")
    scratch_staging = paths.get("scratch_staging")
    if scratch_staging is not None and scratch_staging.exists():
        try:
            shutil.rmtree(scratch_staging)
        except OSError as err:
            errors.append(f"{scratch_staging.name}:{exception_note(err)}")
    if errors:
        return ";".join(errors)
    try:
        remove_chembl_transaction_file(transaction)
    except OSError as err:
        return f"{transaction.name}:{exception_note(err)}"
    return ""


def chembl_transaction_new_pair_ready(
    paths: dict[str, Path],
    payload: dict[str, object],
) -> bool:
    source_digest = payload.get("source_digest")
    source_release = payload.get("source_release")
    database_identity = chembl_file_identity(paths["live_database"])
    return bool(
        isinstance(source_digest, str)
        and isinstance(source_release, str)
        and chembl_identity_matches(database_identity, payload, "expected_database")
        and chembl_file_matches(paths["live_manifest"], payload, "expected_manifest")
        and chembl_query_database_ready(
            paths["live_database"],
            paths["live_manifest"],
            source_digest,
            source_release,
            observed_database_identity=database_identity,
        )
    )


def replace_chembl_transaction_file(source: Path, target: Path) -> None:
    if source == target:
        return
    if target.exists():
        try:
            target.chmod(0o666)
        except OSError:
            pass
    os.replace(source, target)


def reconcile_chembl_query_transaction(
    database: Path,
    manifest: Path,
    scratch_root: Path | None = None,
) -> dict[str, object]:
    transaction = chembl_query_transaction_path(database)
    if not transaction.exists():
        return {"status": "none", "reason": "chembl_query_transaction_absent"}
    if not transaction.is_file():
        return {
            "status": "needs_repair",
            "reason": "chembl_query_transaction_descriptor_not_file",
            "transaction": str(transaction),
        }
    payload = read_json_object(transaction)
    paths = chembl_transaction_paths(database, manifest, payload, scratch_root)
    if paths is None:
        return {
            "status": "needs_repair",
            "reason": "chembl_query_transaction_descriptor_invalid",
            "transaction": str(transaction),
        }
    if chembl_transaction_new_pair_ready(paths, payload):
        cleanup_error = cleanup_chembl_query_transaction(transaction, paths)
        if cleanup_error:
            return {
                "status": "needs_repair",
                "reason": f"chembl_query_transaction_cleanup_failed:{cleanup_error}",
                "transaction": str(transaction),
            }
        try:
            database.chmod(0o444)
        except OSError:
            pass
        return {"status": "ready", "reason": "chembl_query_transaction_live_pair_committed"}

    had_database = payload.get("had_live_database")
    had_manifest = payload.get("had_live_manifest")
    if type(had_database) is not bool or type(had_manifest) is not bool or had_database != had_manifest:
        return {
            "status": "needs_repair",
            "reason": "chembl_query_transaction_initial_pair_ambiguous",
            "transaction": str(transaction),
        }
    try:
        if had_database:
            database_source = next(
                (
                    path
                    for path in (paths["backup_database"], paths["live_database"])
                    if chembl_file_matches(path, payload, "previous_database")
                ),
                None,
            )
            manifest_source = next(
                (
                    path
                    for path in (paths["backup_manifest"], paths["live_manifest"])
                    if chembl_file_matches(path, payload, "previous_manifest")
                ),
                None,
            )
            if database_source is not None and manifest_source is not None:
                replace_chembl_transaction_file(database_source, paths["live_database"])
                replace_chembl_transaction_file(manifest_source, paths["live_manifest"])
                if not (
                    chembl_file_matches(paths["live_database"], payload, "previous_database")
                    and chembl_file_matches(paths["live_manifest"], payload, "previous_manifest")
                ):
                    raise OSError("restored ChEMBL pair does not match transaction descriptor")
                cleanup_error = cleanup_chembl_query_transaction(transaction, paths)
                if cleanup_error:
                    raise OSError(f"transaction cleanup failed:{cleanup_error}")
                try:
                    database.chmod(0o444)
                except OSError:
                    pass
                return {"status": "rolled_back", "reason": "chembl_query_transaction_previous_pair_restored"}
        else:
            database_source = next(
                (
                    path
                    for path in (paths["live_database"], paths["staging_database"])
                    if chembl_file_matches(path, payload, "expected_database")
                ),
                None,
            )
            manifest_source = next(
                (
                    path
                    for path in (paths["live_manifest"], paths["staging_manifest"])
                    if chembl_file_matches(path, payload, "expected_manifest")
                ),
                None,
            )
            if database_source is not None and manifest_source is not None:
                replace_chembl_transaction_file(database_source, paths["live_database"])
                replace_chembl_transaction_file(manifest_source, paths["live_manifest"])
                if not chembl_transaction_new_pair_ready(paths, payload):
                    raise OSError("forward-completed ChEMBL pair failed validation")
                cleanup_error = cleanup_chembl_query_transaction(transaction, paths)
                if cleanup_error:
                    raise OSError(f"transaction cleanup failed:{cleanup_error}")
                try:
                    database.chmod(0o444)
                except OSError:
                    pass
                return {"status": "ready", "reason": "chembl_query_transaction_first_install_completed"}
    except OSError as err:
        return {
            "status": "needs_repair",
            "reason": f"chembl_query_transaction_reconcile_failed:{exception_note(err)}",
            "transaction": str(transaction),
        }
    return {
        "status": "needs_repair",
        "reason": "chembl_query_transaction_state_ambiguous",
        "transaction": str(transaction),
    }


def chembl_query_database_ready(
    database: Path,
    manifest: Path,
    source_digest: str,
    source_release: str,
    summary_cache: ResourceSummaryCache | None = None,
    *,
    observed_database_identity: tuple[int, str] | None = None,
) -> bool:
    payload = read_json_object(manifest)
    if (
        payload.get("schema_version") != "genoligand_chembl_query_database_v1"
        or payload.get("status") != "ready"
        or payload.get("source_digest") != source_digest
        or payload.get("source_release") != source_release
        or payload.get("table") != "genoligand_onsite_activity"
        or type(payload.get("rows")) is not int
        or payload.get("rows") <= 0
        or type(payload.get("database_bytes")) is not int
        or payload.get("database_bytes") <= 0
        or not isinstance(payload.get("database_sha256"), str)
        or not re.fullmatch(r"[0-9a-f]{64}", str(payload.get("database_sha256")))
        or not database.is_file()
    ):
        return False
    try:
        if observed_database_identity is None:
            database_summary = resource_payload_summary(database, cache=summary_cache)
            observed_database_identity = (
                int(database_summary.get("bytes", 0) or 0),
                str(database_summary.get("content_sha256", "")),
            )
        if observed_database_identity != (
            payload.get("database_bytes"),
            payload.get("database_sha256"),
        ):
            return False
        uri = f"file:{database.as_posix()}?mode=ro"
        with closing(sqlite3.connect(uri, uri=True)) as connection:
            columns = {
                str(row[1])
                for row in connection.execute("PRAGMA table_info(genoligand_onsite_activity)")
            }
            required = {
                "canonical_isomeric_smiles",
                "target_id",
                "source_target_id",
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
            rows = int(connection.execute("SELECT COUNT(*) FROM genoligand_onsite_activity").fetchone()[0])
            return (
                required.issubset(columns)
                and "idx_genoligand_onsite_activity_smiles" in indexes
                and "idx_genoligand_onsite_activity_target" not in indexes
                and row is not None
                and rows == payload.get("rows")
            )
    except (OSError, sqlite3.Error):
        return False


def publish_chembl_query_database(
    science_store: Path,
    source_catalog: Path,
    summary_cache: ResourceSummaryCache | None = None,
    *,
    scratch_dir: Path | None = None,
) -> dict[str, object]:
    archive_path = science_store / "downloads/chembl/chembl_sqlite.tar.gz"
    database = science_store / "downloads/chembl/chembl_query.sqlite"
    manifest = database.with_suffix(".manifest.json")
    transaction = chembl_query_transaction_path(database)
    scratch_root = Path(scratch_dir) if scratch_dir is not None else science_store / "work/installer-scratch"
    catalog_row = next((row for row in read_tsv(source_catalog) if row.get("source_id") == "chembl"), {})
    source_release = catalog_row.get("release_identity") or catalog_row.get("release_or_date") or "unknown"
    recovery = reconcile_chembl_query_transaction(database, manifest, scratch_root)
    if recovery.get("status") == "needs_repair":
        return {
            "status": "needs_repair",
            "reason": str(recovery.get("reason", "chembl_query_transaction_reconcile_failed")),
            "database": str(database),
            "transaction": str(transaction),
        }
    if (
        database.exists() != manifest.exists()
        or (database.exists() and not database.is_file())
        or (manifest.exists() and not manifest.is_file())
    ):
        return {
            "status": "needs_repair",
            "reason": "chembl_query_live_pair_ambiguous",
            "database": str(database),
        }
    if not archive_path.is_file() or archive_path.stat().st_size <= 0:
        retained = source_retention_row(science_store, "chembl")
        if (
            retained.get("schema_version") == SOURCE_RETENTION_SCHEMA_VERSION
            and retained.get("state") == "pruned"
            and retained.get("source_local_path") == "downloads/chembl/chembl_sqlite.tar.gz"
            and chembl_query_database_ready(
                database,
                manifest,
                retained.get("source_manifest_sha256", ""),
                retained.get("release_identity", ""),
                summary_cache,
            )
        ):
            return {
                "status": "ready",
                "reason": "chembl_query_database_reused_after_source_payload_pruned",
                "database": str(database),
                "table": "genoligand_onsite_activity",
                "source_release": retained.get("release_identity", ""),
                "source_digest": retained.get("source_manifest_sha256", ""),
                "manifest": str(manifest),
            }
        return {"status": "needs_repair", "reason": "chembl_archive_missing", "database": str(database)}
    source_summary = resource_payload_summary(archive_path, cache=summary_cache)
    source_digest = str(source_summary.get("sha256", ""))
    if chembl_query_database_ready(database, manifest, source_digest, source_release, summary_cache):
        return {
            "status": "ready",
            "reason": "chembl_query_database_reused",
            "database": str(database),
            "table": "genoligand_onsite_activity",
            "source_release": source_release,
            "source_digest": source_digest,
            "manifest": str(manifest),
        }
    token = f"{os.getpid()}-{time.time_ns()}"
    scratch_staging = scratch_root / f"chembl-query-rebuild-{token}"
    extracted_database = scratch_staging / "chembl_source.sqlite"
    staging_database = database.with_name(f".{database.name}.rebuild-{token}")
    staging_manifest = manifest.with_name(f".{manifest.name}.rebuild-{token}")
    backup_database = database.with_name(f".{database.name}.backup-{token}")
    backup_manifest = manifest.with_name(f".{manifest.name}.backup-{token}")
    attempt_manifest = manifest.with_name(f"{manifest.name}.attempt.json")
    disk_guard_reason = ""

    def remove_staged_file(path: Path) -> None:
        remove_chembl_transaction_file(path)

    def target_disk_guard() -> int:
        nonlocal disk_guard_reason
        try:
            free = int(shutil.disk_usage(existing_path_for_disk_usage(database.parent)).free)
        except OSError as err:
            disk_guard_reason = f"chembl_target_disk_probe_failed:{exception_note(err)}"
            return 1
        if free <= DISK_SAFETY_MARGIN_BYTES:
            disk_guard_reason = f"chembl_target_disk_safety_margin_reached:free={free}"
            return 1
        return 0

    try:
        with tarfile.open(archive_path, "r:gz") as archive:
            candidates = [
                item
                for item in archive.getmembers()
                if item.isfile() and item.size > 0 and item.name.lower().endswith((".db", ".sqlite"))
            ]
            if len(candidates) != 1:
                return {"status": "needs_repair", "reason": "chembl_sqlite_member_ambiguous", "database": str(database)}
            source_bytes = int(candidates[0].size)
            target_staging_bytes = chembl_query_target_staging_bytes(source_bytes, archive_path.stat().st_size)
            scratch_available = int(shutil.disk_usage(existing_path_for_disk_usage(scratch_root)).free)
            target_available = int(shutil.disk_usage(existing_path_for_disk_usage(database.parent)).free)
            scratch_required = source_bytes + DISK_SAFETY_MARGIN_BYTES
            target_required = target_staging_bytes + DISK_SAFETY_MARGIN_BYTES
            shared_volume = same_storage_volume(scratch_root, database.parent)
            combined_required = source_bytes + target_staging_bytes + DISK_SAFETY_MARGIN_BYTES
            if shared_volume and combined_required > target_available:
                return {
                    "status": "needs_repair",
                    "reason": "chembl_query_database_insufficient_shared_volume_disk",
                    "required_peak_bytes": combined_required,
                    "available_bytes": target_available,
                    "scratch_required_peak_bytes": scratch_required,
                    "scratch_available_bytes": scratch_available,
                    "target_required_peak_bytes": target_required,
                    "target_available_bytes": target_available,
                    "database": str(database),
                }
            if not shared_volume and scratch_required > scratch_available:
                return {
                    "status": "needs_repair",
                    "reason": "chembl_query_database_insufficient_scratch_disk",
                    "required_peak_bytes": scratch_required,
                    "available_bytes": scratch_available,
                    "scratch_required_peak_bytes": scratch_required,
                    "scratch_available_bytes": scratch_available,
                    "target_required_peak_bytes": target_required,
                    "target_available_bytes": target_available,
                    "database": str(database),
                }
            if not shared_volume and target_required > target_available:
                return {
                    "status": "needs_repair",
                    "reason": "chembl_query_database_insufficient_target_disk",
                    "required_peak_bytes": target_required,
                    "available_bytes": target_available,
                    "scratch_required_peak_bytes": scratch_required,
                    "scratch_available_bytes": scratch_available,
                    "target_required_peak_bytes": target_required,
                    "target_available_bytes": target_available,
                    "database": str(database),
                }
            scratch_staging.mkdir(parents=True, exist_ok=False)
            source_handle = archive.extractfile(candidates[0])
            if source_handle is None:
                raise OSError("chembl sqlite archive member cannot be read")
            with source_handle, extracted_database.open("wb") as target_handle:
                shutil.copyfileobj(source_handle, target_handle, 1024 * 1024)
        target_uri = staging_database.resolve().as_uri() + "?mode=rwc"
        source_uri = extracted_database.resolve().as_uri() + "?mode=ro&immutable=1"
        with closing(sqlite3.connect(target_uri, uri=True)) as connection:
            connection.execute("PRAGMA journal_mode=OFF")
            connection.execute("PRAGMA synchronous=OFF")
            connection.execute("PRAGMA temp_store=FILE")
            connection.set_progress_handler(target_disk_guard, 250_000)
            connection.execute("ATTACH DATABASE ? AS chembl_source", (source_uri,))
            connection.execute(
                """
                CREATE TABLE genoligand_onsite_activity AS
                SELECT
                    cs.canonical_smiles AS canonical_isomeric_smiles,
                    COALESCE(target_accession.canonical_accession, td.chembl_id) AS target_id,
                    td.chembl_id AS source_target_id,
                    td.pref_name AS target_name,
                    td.organism AS organism,
                    md.pref_name AS compound_name,
                    act.standard_type AS standard_type,
                    act.standard_relation AS standard_relation,
                    act.standard_value AS standard_value,
                    COALESCE(act.standard_units, '') AS standard_units
                FROM chembl_source.activities AS act
                JOIN chembl_source.assays AS assay ON assay.assay_id = act.assay_id
                JOIN chembl_source.target_dictionary AS td ON td.tid = assay.tid
                JOIN chembl_source.molecule_dictionary AS md ON md.molregno = act.molregno
                JOIN chembl_source.compound_structures AS cs ON cs.molregno = act.molregno
                LEFT JOIN (
                    SELECT
                        tc.tid,
                        CASE
                            WHEN COUNT(DISTINCT TRIM(cseq.accession)) = 1
                            THEN MIN(TRIM(cseq.accession))
                            ELSE NULL
                        END AS canonical_accession
                    FROM chembl_source.target_components AS tc
                    JOIN chembl_source.component_sequences AS cseq
                      ON cseq.component_id = tc.component_id
                    WHERE cseq.accession IS NOT NULL
                      AND TRIM(cseq.accession) <> ''
                    GROUP BY tc.tid
                ) AS target_accession ON target_accession.tid = td.tid
                WHERE cs.canonical_smiles IS NOT NULL
                  AND TRIM(cs.canonical_smiles) <> ''
                  AND td.chembl_id IS NOT NULL
                  AND act.standard_type IN ('Ki', 'Kd', 'IC50', 'EC50')
                  AND act.standard_value IS NOT NULL
                  AND (act.standard_units IS NULL OR act.standard_units = '' OR act.standard_units = 'nM')
                """,
            )
            if target_disk_guard():
                raise OSError(disk_guard_reason)
            quoted_release = str(connection.execute("SELECT quote(?)", (source_release,)).fetchone()[0])
            quoted_digest = str(connection.execute("SELECT quote(?)", (source_digest,)).fetchone()[0])
            connection.execute(
                "ALTER TABLE genoligand_onsite_activity "
                f"ADD COLUMN source_release TEXT NOT NULL DEFAULT {quoted_release}"
            )
            connection.execute(
                "ALTER TABLE genoligand_onsite_activity "
                f"ADD COLUMN source_digest TEXT NOT NULL DEFAULT {quoted_digest}"
            )
            connection.execute(
                "CREATE INDEX idx_genoligand_onsite_activity_smiles "
                "ON genoligand_onsite_activity(canonical_isomeric_smiles)"
            )
            if target_disk_guard():
                raise OSError(disk_guard_reason)
            connection.commit()
            rows = int(connection.execute("SELECT COUNT(*) FROM genoligand_onsite_activity").fetchone()[0])
            if rows <= 0:
                raise sqlite3.DatabaseError("chembl query database contains no runtime activity rows")
            if connection.execute("PRAGMA quick_check").fetchone() != ("ok",):
                raise sqlite3.DatabaseError("chembl query database quick_check failed")
        payload = {
            "schema_version": "genoligand_chembl_query_database_v1",
            "status": "ready",
            "database": str(database),
            "database_bytes": staging_database.stat().st_size,
            "database_sha256": sha256_file(staging_database),
            "rows": rows,
            "table": "genoligand_onsite_activity",
            "canonical_query_column": "canonical_isomeric_smiles",
            "source_target_column": "source_target_id",
            "indexes": [
                "idx_genoligand_onsite_activity_smiles",
            ],
            "source_release": source_release,
            "source_digest": source_digest,
            "source_database_bytes": source_bytes,
            "target_staging_budget_bytes": target_staging_bytes,
            "activity_types": ["Ki", "Kd", "IC50", "EC50"],
            "activity_units": ["", "nM"],
        }
        write_json_atomic(staging_manifest, payload)
        expected_database = chembl_file_identity(staging_database)
        expected_manifest = chembl_file_identity(staging_manifest)
        previous_database = chembl_file_identity(database)
        previous_manifest = chembl_file_identity(manifest)
        if expected_database is None or expected_manifest is None:
            raise OSError("ChEMBL staging pair identity unavailable")
        if (previous_database is None) != (previous_manifest is None):
            raise OSError("ChEMBL live pair became ambiguous before publish")
        transaction_payload: dict[str, object] = {
            "schema_version": CHEMBL_QUERY_TRANSACTION_SCHEMA_VERSION,
            "status": "publishing",
            "token": token,
            "created_at": int(time.time()),
            "live_database": str(database.resolve()),
            "live_manifest": str(manifest.resolve()),
            "staging_database": str(staging_database.resolve()),
            "staging_manifest": str(staging_manifest.resolve()),
            "backup_database": str(backup_database.resolve()),
            "backup_manifest": str(backup_manifest.resolve()),
            "scratch_staging": str(scratch_staging.resolve()),
            "expected_database_bytes": expected_database[0],
            "expected_database_sha256": expected_database[1],
            "expected_manifest_bytes": expected_manifest[0],
            "expected_manifest_sha256": expected_manifest[1],
            "had_live_database": previous_database is not None,
            "had_live_manifest": previous_manifest is not None,
            "previous_database_bytes": previous_database[0] if previous_database else 0,
            "previous_database_sha256": previous_database[1] if previous_database else "",
            "previous_manifest_bytes": previous_manifest[0] if previous_manifest else 0,
            "previous_manifest_sha256": previous_manifest[1] if previous_manifest else "",
            "source_release": source_release,
            "source_digest": source_digest,
        }
        write_json_atomic(transaction, transaction_payload)
        if database.exists():
            try:
                database.chmod(0o666)
            except OSError:
                pass
            os.replace(database, backup_database)
        if manifest.exists():
            os.replace(manifest, backup_manifest)
        os.replace(staging_database, database)
        os.replace(staging_manifest, manifest)
        transaction_paths = chembl_transaction_paths(database, manifest, transaction_payload, scratch_root)
        if transaction_paths is None or not chembl_transaction_new_pair_ready(
            transaction_paths,
            transaction_payload,
        ):
            raise OSError("published ChEMBL pair failed transaction validation")
        cleanup_error = cleanup_chembl_query_transaction(transaction, transaction_paths)
        if cleanup_error:
            raise OSError(f"ChEMBL transaction cleanup failed:{cleanup_error}")
        try:
            database.chmod(0o444)
        except OSError:
            pass
        try:
            remove_staged_file(attempt_manifest)
        except OSError:
            pass
        seed_resource_file_fingerprint(summary_cache, database, str(payload["database_sha256"]))
        return {**payload, "manifest": str(manifest), "reason": "chembl_query_database_published"}
    except (OSError, sqlite3.Error, tarfile.TarError) as err:
        recovery = reconcile_chembl_query_transaction(database, manifest, scratch_root)
        try:
            write_json_atomic(
                attempt_manifest,
                {
                    "status": "failed",
                    "reason": disk_guard_reason or exception_note(err),
                    "source_digest": source_digest,
                },
            )
        except OSError:
            pass
        return {
            "status": "needs_repair",
            "reason": f"chembl_query_database_publish_failed:{disk_guard_reason or exception_note(err)}",
            "database": str(database),
            "transaction_recovery": recovery.get("status"),
        }
    finally:
        if not transaction.exists():
            for staged in (
                staging_database,
                Path(f"{staging_database}-journal"),
                Path(f"{staging_database}-wal"),
                Path(f"{staging_database}-shm"),
                staging_manifest,
            ):
                try:
                    remove_staged_file(staged)
                except OSError:
                    pass
        shutil.rmtree(scratch_staging, ignore_errors=True)


SOURCE_RETENTION_FIELDS = [
    "schema_version",
    "source_id",
    "source_local_path",
    "state",
    "source_files",
    "source_bytes",
    "source_manifest_sha256",
    "execution_sha256",
    "release_identity",
    "provenance",
    "trust_source",
    "payload_records_json",
    "derived_paths",
    "structure_input_bytes",
    "structure_input_digest",
    "pruned_bytes",
    "pruned_at",
    "notes",
]


def source_retention_manifest_path(science_store: Path) -> Path:
    return science_store / SOURCE_RETENTION_MANIFEST_REL_PATH


def source_retention_rows(science_store: Path) -> list[dict[str, str]]:
    return read_tsv(source_retention_manifest_path(science_store))


def source_retention_row(science_store: Path, source_id: str) -> dict[str, str]:
    return next(
        (row for row in source_retention_rows(science_store) if row.get("source_id") == source_id),
        {},
    )


def source_manifest_rows_by_id(science_store: Path) -> dict[str, dict[str, str]]:
    return {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "manifests/source_file_manifest.tsv")
        if row.get("source_id")
    }


def retention_payload_records(row: dict[str, str]) -> list[dict[str, object]]:
    try:
        value = json.loads(row.get("payload_records_json", "[]"))
    except json.JSONDecodeError:
        return []
    return [item for item in value if isinstance(item, dict)] if isinstance(value, list) else []


def retention_payload_record_valid(record: dict[str, object]) -> bool:
    relative = str(record.get("path", "")).replace("\\", "/")
    path = Path(relative)
    try:
        return bool(
            relative
            and not path.is_absolute()
            and ".." not in path.parts
            and str(record.get("kind", "")) in {"file", "directory"}
            and int(record.get("file_count", 0) or 0) > 0
            and int(record.get("bytes", 0) or 0) > 0
            and re.fullmatch(r"[0-9a-f]{64}", str(record.get("sha256", "")).lower())
            and (
                str(record.get("kind", "")) != "file"
                or re.fullmatch(
                    r"[0-9a-f]{64}",
                    str(record.get("content_sha256", "")).lower(),
                )
            )
        )
    except (TypeError, ValueError):
        return False


def retention_covers_source_path(science_store: Path, source_path: str) -> bool:
    normalized = source_path.replace("\\", "/").strip("/").casefold()
    if not normalized:
        return False
    for row in source_retention_rows(science_store):
        if (
            row.get("schema_version") != SOURCE_RETENTION_SCHEMA_VERSION
            or row.get("state") not in {"pruned", "pruning", "cleanup_failed"}
        ):
            continue
        for record in retention_payload_records(row):
            payload = str(record.get("path", "")).replace("\\", "/").strip("/").casefold()
            kind = str(record.get("kind", ""))
            if normalized == payload or (kind == "directory" and normalized.startswith(f"{payload}/")):
                return retention_source_identity_and_derivatives_ready(science_store, row)
    return False


def bindingdb_query_database_ready(
    science_store: Path,
    source_manifest: dict[str, str] | None = None,
    execution_sha256: str = "",
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    database = science_store / "indexes/bindingdb/bindingdb_query.sqlite"
    manifest = science_store / "indexes/bindingdb/bindingdb_query.manifest.json"
    payload = read_json_object(manifest)
    source = source_manifest or source_manifest_rows_by_id(science_store).get("bindingdb", {})
    try:
        database_summary = resource_payload_summary(database, cache=summary_cache)
        return bool(
            payload.get("schema_version") == "genoligand_bindingdb_query_database_v1"
            and payload.get("status") == "ready"
            and payload.get("table") == "genoligand_bindingdb_activity"
            and type(payload.get("rows")) is int
            and int(payload.get("rows", 0)) > 0
            and type(payload.get("database_bytes")) is int
            and int(payload.get("database_bytes", 0)) == int(database_summary.get("bytes", 0))
            and payload.get("database_sha256") == database_summary.get("content_sha256")
            and payload.get("source_digest") == source.get("sha256")
            and payload.get("source_release") == source.get("release_identity")
            and (
                not execution_sha256
                or payload.get("source_archive_sha256") == execution_sha256
            )
            and isinstance(payload.get("indexes"), list)
            and {
                "idx_bindingdb_activity_canonical_smiles",
                "idx_bindingdb_activity_compact_smiles",
            }.issubset(set(payload.get("indexes", [])))
        )
    except (OSError, TypeError, ValueError):
        return False


def ligand_similarity_index_ready(
    science_store: Path,
    source_manifests: dict[str, dict[str, str]] | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    database = science_store / LIGAND_SIMILARITY_DATABASE_REL_PATH
    manifest = science_store / LIGAND_SIMILARITY_MANIFEST_REL_PATH
    payload = read_json_object(manifest)
    sources = source_manifests or source_manifest_rows_by_id(science_store)
    database_meta = payload.get("database", {})
    inputs = payload.get("inputs", {})
    counts = payload.get("counts", {})
    fingerprint = payload.get("fingerprint", {})
    if not all(isinstance(value, dict) for value in (database_meta, inputs, counts, fingerprint)):
        return False
    try:
        summary = resource_payload_summary(database, cache=summary_cache)
        declared_path = str(database_meta.get("path", "")).replace("\\", "/").casefold()
        accepted_paths = {
            str(database.resolve()).replace("\\", "/").casefold(),
            wsl_path(database.resolve()).replace("\\", "/").casefold(),
        }
        if declared_path not in accepted_paths:
            return False
        input_bytes = 0
        expected_source_identity: dict[str, dict[str, str]] = {}
        for source_id in ("chembl", "bindingdb"):
            source = sources.get(source_id, {})
            source_input = inputs.get(source_id, {})
            if not isinstance(source_input, dict) or not (
                source.get("status") == "ready"
                and source_input.get("source_digest") == source.get("sha256")
                and source_input.get("source_release") == source.get("release_identity")
                and re.fullmatch(r"[0-9a-f]{64}", str(source_input.get("sha256", "")).lower())
                and type(source_input.get("bytes")) is int
                and int(source_input.get("bytes", 0)) > 0
            ):
                return False
            input_bytes += int(source_input["bytes"])
            expected_source_identity[source_id] = {
                "source_digest": str(source_input["source_digest"]),
                "source_release": str(source_input["source_release"]),
            }
        output_bytes = int(database_meta.get("bytes", 0))
        reclaimed = int(payload.get("reclaimed_candidate_bytes", -1))
        reference = fingerprint.get("reference", {})
        if not (
            payload.get("schema_version") == LIGAND_SIMILARITY_SCHEMA_VERSION
            and payload.get("status") == "ready"
            and output_bytes == int(summary.get("bytes", 0))
            and database_meta.get("sha256") == summary.get("content_sha256")
            and 0 < output_bytes < input_bytes
            and reclaimed == input_bytes - output_bytes
            and all(
                type(counts.get(name)) is int and int(counts[name]) > 0
                for name in ("molecules", "targets", "evidence")
            )
            and type(counts.get("hints")) is int
            and int(counts["hints"]) >= 0
            and fingerprint.get("type") == "morgan_ecfp4"
            and type(fingerprint.get("radius")) is int
            and fingerprint.get("radius") == 2
            and type(fingerprint.get("bits")) is int
            and fingerprint.get("bits") == 2048
            and fingerprint.get("include_chirality") is True
            and fingerprint.get("standardization") == LIGAND_SIMILARITY_STANDARDIZATION
            and type(fingerprint.get("tautomer_max_tautomers")) is int
            and fingerprint.get("tautomer_max_tautomers") == LIGAND_SIMILARITY_TAUTOMER_LIMIT
            and type(fingerprint.get("tautomer_max_transforms")) is int
            and fingerprint.get("tautomer_max_transforms") == LIGAND_SIMILARITY_TAUTOMER_LIMIT
            and type(fingerprint.get("lsh_bands")) is int
            and fingerprint.get("lsh_bands") == 0
            and fingerprint.get("search_algorithm") == "popcount_bitbound_exact_tanimoto"
            and fingerprint.get("bitbound_formula")
            == "ceil(t*q_popcount) <= db_popcount <= floor(q_popcount/t)"
            and reference
            == {"name": "FPSim2", "license": "MIT", "commit": "e03e669"}
            and fingerprint.get("encoding")
            in {"rdkit_binary_text", "zlib1_rdkit_binary_text"}
            and type(fingerprint.get("build_workers")) is int
            and 1 <= fingerprint.get("build_workers") <= 8
        ):
            return False
        validation_key = (str(database.resolve()), output_bytes, str(database_meta["sha256"]))
        if validation_key in LIGAND_SIMILARITY_SQLITE_VALIDATION_CACHE:
            return True
        uri = database.resolve().as_uri() + "?mode=ro&immutable=1"
        with closing(sqlite3.connect(uri, uri=True)) as connection:
            objects = {
                (str(row[0]), str(row[1]))
                for row in connection.execute(
                    "SELECT type, name FROM sqlite_master WHERE type IN ('table', 'index')"
                )
            }
            required_objects = {
                ("table", "molecule"),
                ("table", "target"),
                ("table", "evidence"),
                ("table", "structure_hint"),
                ("table", "metadata"),
                ("index", "idx_molecule_popcount"),
            }
            if not required_objects.issubset(objects) or any(
                kind == "table" and name.startswith("_build_") for kind, name in objects
            ):
                return False
            evidence_columns = {str(row[1]) for row in connection.execute("PRAGMA table_info(evidence)")}
            if not {
                "molecule_id",
                "target_fk",
                "activity_score",
                "activity",
                "relation_rank",
                "numeric_rank",
            }.issubset(evidence_columns):
                return False
            structure_hint_columns = {
                str(row[1]) for row in connection.execute("PRAGMA table_info(structure_hint)")
            }
            if not {
                "molecule_id",
                "target_fk",
                "ligand_code",
                "pdb_ids",
                "target_chain_pdb_ids",
            }.issubset(structure_hint_columns):
                return False
            metadata = dict(connection.execute("SELECT key, value FROM metadata"))
            stable_fingerprint = {
                key: fingerprint[key]
                for key in (
                    "type",
                    "radius",
                    "bits",
                    "include_chirality",
                    "standardization",
                    "tautomer_max_tautomers",
                    "tautomer_max_transforms",
                    "lsh_bands",
                    "search_algorithm",
                    "bitbound_formula",
                    "reference",
                    "encoding",
                )
            }
            actual_counts = {
                "molecules": int(connection.execute("SELECT COUNT(*) FROM molecule").fetchone()[0]),
                "targets": int(connection.execute("SELECT COUNT(*) FROM target").fetchone()[0]),
                "evidence": int(connection.execute("SELECT COUNT(*) FROM evidence").fetchone()[0]),
                "hints": int(
                    connection.execute("SELECT COUNT(*) FROM structure_hint").fetchone()[0]
                ),
            }
            if not (
                metadata.get("schema_version") == LIGAND_SIMILARITY_SCHEMA_VERSION
                and metadata.get("status") == "ready"
                and json.loads(metadata.get("counts", "{}")) == counts == actual_counts
                and json.loads(metadata.get("fingerprint", "{}")) == stable_fingerprint
                and json.loads(metadata.get("source_identity", "{}")) == expected_source_identity
                and connection.execute("PRAGMA quick_check").fetchone() == ("ok",)
                and connection.execute("PRAGMA foreign_key_check").fetchone() is None
            ):
                return False
        LIGAND_SIMILARITY_SQLITE_VALIDATION_CACHE.add(validation_key)
        return {
            ("table", "molecule"),
            ("table", "target"),
            ("table", "evidence"),
            ("table", "metadata"),
            ("index", "idx_molecule_popcount"),
        }.issubset(objects)
    except (json.JSONDecodeError, KeyError, OSError, sqlite3.Error, TypeError, ValueError):
        return False


def structure_contribution_source_paths_state(science_store: Path, source_paths: object) -> str:
    if not isinstance(source_paths, list) or not source_paths:
        return "invalid"
    store_root = science_store.resolve()
    states: list[bool] = []
    try:
        for value in source_paths:
            if not isinstance(value, str) or not value.strip():
                return "invalid"
            relative = Path(value.replace("\\", "/"))
            if relative.is_absolute() or ".." in relative.parts:
                return "invalid"
            candidate = science_store / relative
            resolved = candidate.resolve()
            if not resolved.is_relative_to(store_root) or candidate.is_symlink():
                return "invalid"
            states.append(path_has_payload(candidate))
    except (OSError, ValueError):
        return "invalid"
    if all(states):
        return "all"
    if not any(states):
        return "none"
    return "partial"


def structure_contribution_input_summary(
    science_store: Path,
    source_paths: object,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    if not isinstance(source_paths, list) or not source_paths:
        return {}
    store_root = science_store.resolve()
    files: dict[str, Path] = {}
    try:
        for value in source_paths:
            if not isinstance(value, str) or not value.strip():
                return {}
            relative = Path(value.replace("\\", "/"))
            if relative.is_absolute() or ".." in relative.parts:
                return {}
            candidate = science_store / relative
            resolved = candidate.resolve()
            if not resolved.is_relative_to(store_root) or candidate.is_symlink():
                return {}
            if candidate.is_file():
                candidates = [candidate]
            elif candidate.is_dir():
                candidates = sorted(
                    child
                    for child in candidate.rglob("*")
                    if child.is_file() and resource_manifest_file(child) and child.stat().st_size > 0
                )
            else:
                return {}
            if not candidates:
                return {}
            for child in candidates:
                child_resolved = child.resolve()
                if (
                    child.is_symlink()
                    or not child_resolved.is_relative_to(store_root)
                    or child.stat().st_size <= 0
                ):
                    return {}
                rel = child_resolved.relative_to(store_root).as_posix()
                files[rel] = child
        if not files:
            return {}
        digest = hashlib.sha256()
        total_bytes = 0
        for rel, child in sorted(files.items()):
            summary = resource_payload_summary(child, cache=summary_cache)
            size = int(summary.get("bytes", 0) or 0)
            content_sha256 = str(summary.get("content_sha256", "")).lower()
            if size <= 0 or not re.fullmatch(r"[0-9a-f]{64}", content_sha256):
                return {}
            digest.update(rel.encode("utf-8"))
            digest.update(str(size).encode("ascii"))
            digest.update(content_sha256.encode("ascii"))
            total_bytes += size
        return {
            "file_count": len(files),
            "input_bytes": total_bytes,
            "input_digest": digest.hexdigest(),
            "paths": sorted(files),
        }
    except (OSError, TypeError, ValueError):
        return {}


def structure_contribution_source_identity_matches(
    contribution: dict[str, object],
    source_manifest: dict[str, str],
) -> bool:
    return bool(
        source_manifest.get("status") == "ready"
        and str(contribution.get("source_digest", "")).lower()
        == source_manifest.get("sha256", "").lower()
        and contribution.get("release_identity") == source_manifest.get("release_identity")
        and contribution.get("trust_source") == source_manifest.get("trust_source")
    )


def structure_contribution_retention_identity_matches(
    contribution: dict[str, object],
    retention: dict[str, str],
) -> bool:
    try:
        return bool(
            retention.get("schema_version") == SOURCE_RETENTION_SCHEMA_VERSION
            and retention.get("state")
            in {"pruned", "pruning", "cleanup_failed", "runtime_cache_bounded"}
            and str(contribution.get("source_digest", "")).lower()
            == retention.get("source_manifest_sha256", "").lower()
            and contribution.get("release_identity") == retention.get("release_identity")
            and contribution.get("trust_source") == retention.get("trust_source")
            and int(contribution.get("input_bytes", 0))
            == int(retention.get("structure_input_bytes", 0) or 0)
            and str(contribution.get("input_digest", "")).lower()
            == retention.get("structure_input_digest", "").lower()
        )
    except (TypeError, ValueError):
        return False


def structure_build_contributions(science_store: Path) -> dict[str, tuple[int, tuple[str, ...]]]:
    contribution_notes = {
        "plinder_canonical_rows": "PLINDER",
        "biolip2_interaction_rows": "BioLiP2",
        "binding_moad_complex_rows": "Binding MOAD",
        "rcsb_structure_file_rows": "RCSB",
    }
    rows_by_source: dict[str, int] = {}
    paths_by_source: dict[str, set[str]] = {}
    try:
        for row in read_tsv(science_store / "index_build_manifest.tsv"):
            if (
                row.get("index") != "structure_targets"
                or row.get("status") != "used"
                or row.get("output_path", "").replace("\\", "/")
                != "indexes/structure_targets.tsv"
            ):
                continue
            notes = set(row.get("notes", "").split(";"))
            sources = {source for token, source in contribution_notes.items() if token in notes}
            if len(sources) != 1:
                return {}
            source = sources.pop()
            count = int(row.get("rows", 0) or 0)
            path = row.get("input_path", "").replace("\\", "/")
            if count <= 0 or not path:
                return {}
            rows_by_source[source] = rows_by_source.get(source, 0) + count
            paths_by_source.setdefault(source, set()).add(path)
    except (TypeError, ValueError):
        return {}
    return {
        source: (rows_by_source[source], tuple(sorted(paths)))
        for source, paths in paths_by_source.items()
    }


def structure_targets_publication_contribution(
    science_store: Path,
    required_source: str,
) -> dict[str, object]:
    manifest = read_json_object(science_store / STRUCTURE_TARGETS_MANIFEST_REL_PATH)
    contributions = manifest.get("contributions")
    if manifest.get("schema_version") != STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION or not isinstance(
        contributions, list
    ):
        return {}
    return next(
        (
            dict(contribution)
            for contribution in contributions
            if isinstance(contribution, dict) and contribution.get("source") == required_source
        ),
        {},
    )


def structure_targets_publication_ready(
    science_store: Path,
    required_source: str = "",
    summary_cache: ResourceSummaryCache | None = None,
    *,
    require_live_inputs: bool = False,
    retention_overrides: dict[str, dict[str, str]] | None = None,
) -> bool:
    table = science_store / "indexes/structure_targets.tsv"
    manifest_path = science_store / STRUCTURE_TARGETS_MANIFEST_REL_PATH
    if (
        not table.is_file()
        or table.is_symlink()
        or not manifest_path.is_file()
        or manifest_path.is_symlink()
    ):
        return False
    manifest = read_json_object(manifest_path)
    contributions = manifest.get("contributions")
    declared_rows = manifest.get("rows")
    declared_bytes = manifest.get("bytes")
    declared_sha256 = str(manifest.get("sha256", "")).lower()
    if not (
        manifest.get("schema_version") == STRUCTURE_TARGETS_PUBLICATION_SCHEMA_VERSION
        and manifest.get("status") == "ready"
        and manifest.get("path") == "indexes/structure_targets.tsv"
        and type(declared_rows) is int
        and declared_rows > 0
        and type(declared_bytes) is int
        and declared_bytes > 0
        and re.fullmatch(r"[0-9a-f]{64}", declared_sha256)
        and isinstance(contributions, list)
        and contributions
    ):
        return False
    source_manifest_rows = read_tsv(science_store / "manifests/source_file_manifest.tsv")
    source_rows = {
        source_id: matches[0]
        for source_id in STRUCTURE_CONTRIBUTION_SOURCE_IDS.values()
        if len(
            matches := [
                row
                for row in source_manifest_rows
                if row.get("source_id") == source_id and row.get("status") == "ready"
            ]
        )
        == 1
    }
    build_contributions = structure_build_contributions(science_store)
    retention_rows = {
        row.get("source_id", ""): row
        for row in source_retention_rows(science_store)
        if row.get("source_id")
    }
    if retention_overrides:
        retention_rows.update(retention_overrides)
    contribution_sources: set[str] = set()
    for contribution in contributions:
        if not isinstance(contribution, dict):
            return False
        source = str(contribution.get("source", ""))
        source_id = str(contribution.get("source_id", ""))
        rows = contribution.get("rows")
        source_paths = contribution.get("source_paths")
        source_digest = str(contribution.get("source_digest", "")).lower()
        release_identity = contribution.get("release_identity")
        trust_source = contribution.get("trust_source")
        input_bytes = contribution.get("input_bytes")
        input_digest = str(contribution.get("input_digest", "")).lower()
        if not (
            source in STRUCTURE_CONTRIBUTION_SOURCE_IDS
            and source_id == STRUCTURE_CONTRIBUTION_SOURCE_IDS[source]
            and source not in contribution_sources
            and type(rows) is int
            and rows > 0
            and isinstance(source_paths, list)
            and source_paths
            and all(isinstance(path, str) and path.strip() for path in source_paths)
            and re.fullmatch(r"[0-9a-f]{64}", source_digest)
            and isinstance(release_identity, str)
            and bool(release_identity)
            and isinstance(trust_source, str)
            and bool(trust_source)
            and type(input_bytes) is int
            and input_bytes > 0
            and re.fullmatch(r"[0-9a-f]{64}", input_digest)
        ):
            return False
        if build_contributions.get(source) != (
            rows,
            tuple(sorted(path.replace("\\", "/") for path in source_paths)),
        ):
            return False
        source_identity_ready = structure_contribution_source_identity_matches(
            contribution,
            source_rows.get(source_id, {}),
        )
        retention_identity_ready = structure_contribution_retention_identity_matches(
            contribution,
            retention_rows.get(source_id, {}),
        )
        if not source_identity_ready and not retention_identity_ready:
            return False
        input_state = structure_contribution_source_paths_state(science_store, source_paths)
        if input_state == "invalid" or (
            input_state == "partial" and not (source == "RCSB" and retention_identity_ready)
        ):
            return False
        live_input = (
            structure_contribution_input_summary(science_store, source_paths, summary_cache)
            if input_state == "all"
            else {}
        )
        if input_state == "all":
            if not (
                int(live_input.get("input_bytes", 0)) == input_bytes
                and str(live_input.get("input_digest", "")).lower() == input_digest
            ):
                return False
        elif (
            require_live_inputs and (not required_source or source == required_source)
        ) or not retention_identity_ready:
            return False
        contribution_sources.add(source)
    if set(build_contributions) != contribution_sources:
        return False
    if required_source and required_source not in contribution_sources:
        return False
    try:
        table_stat = table.stat()
        cache_key = (
            str(table.resolve()),
            table_stat.st_size,
            table_stat.st_mtime_ns,
            declared_sha256,
            sha256_file(manifest_path),
            sha256_file(science_store / "manifests/source_file_manifest.tsv")
            if (science_store / "manifests/source_file_manifest.tsv").is_file()
            else "",
            sha256_file(source_retention_manifest_path(science_store))
            if source_retention_manifest_path(science_store).is_file()
            else "",
            hashlib.sha256(
                json.dumps(
                    retention_overrides or {},
                    ensure_ascii=True,
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode("utf-8")
            ).hexdigest(),
        )
        cached_sources = STRUCTURE_TARGETS_PUBLICATION_VALIDATION_CACHE.get(cache_key)
        if cached_sources is not None:
            return not required_source or required_source in cached_sources
        if table_stat.st_size != declared_bytes:
            return False
        digest = hashlib.sha256()
        row_count = 0
        with table.open("rb") as handle:
            header = handle.readline()
            digest.update(header)
            if header.rstrip(b"\r\n") != (
                b"source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\t"
                b"sequence_tokens\tstructure_tokens\tevidence"
            ):
                return False
            for line in handle:
                digest.update(line)
                if line.strip():
                    row_count += 1
        if row_count != declared_rows or digest.hexdigest() != declared_sha256:
            return False
        STRUCTURE_TARGETS_PUBLICATION_VALIDATION_CACHE[cache_key] = frozenset(contribution_sources)
        return True
    except (OSError, TypeError, ValueError):
        return False


def mmseqs_namespace_rows_complete(
    rows: list[dict[str, str]],
    namespace: str,
    minimum: int,
) -> bool:
    identities = {
        (
            str(row.get("source_path", "")).replace("\\", "/").strip("/").casefold(),
            str(row.get("db_path", "")).replace("\\", "/").strip("/").casefold(),
        )
        for row in rows
        if row.get("source_namespace") == namespace
        and str(row.get("source_path", "")).strip()
        and str(row.get("db_path", "")).strip()
    }
    if namespace == "BioLiP2":
        source_paths = {source_path for source_path, _ in identities}
        database_paths = {database_path for _, database_path in identities}
        return bool(
            BIOLIP2_MMSEQS_SOURCE_PATHS.issubset(source_paths)
            and len(database_paths) >= len(BIOLIP2_MMSEQS_SOURCE_PATHS)
        )
    return len(identities) >= minimum


def structure_source_payload_matches_manifest(
    science_store: Path,
    source_id: str,
    source_manifest: dict[str, str],
    payload_records: list[dict[str, object]],
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    local_path = source_manifest.get("local_path", "").replace("\\", "/").strip("/")
    matching = next(
        (
            record
            for record in payload_records
            if str(record.get("path", "")).replace("\\", "/").strip("/") == local_path
        ),
        {},
    )
    try:
        exact = bool(
            matching
            and str(matching.get("sha256", "")).lower() == source_manifest.get("sha256", "").lower()
            and int(matching.get("bytes", 0) or 0) == int(source_manifest.get("bytes", 0) or 0)
            and int(matching.get("file_count", 0) or 0)
            == int(source_manifest.get("file_count", 0) or 0)
        )
    except (TypeError, ValueError):
        return False
    if exact:
        return True
    if source_id != "biolip2":
        return False
    contribution = structure_targets_publication_contribution(science_store, "BioLiP2")
    input_summary = structure_contribution_input_summary(
        science_store,
        contribution.get("source_paths"),
        summary_cache,
    )
    if not input_summary or not (
        int(input_summary.get("input_bytes", 0)) == int(contribution.get("input_bytes", 0) or 0)
        and str(input_summary.get("input_digest", "")).lower()
        == str(contribution.get("input_digest", "")).lower()
    ):
        return False
    live_files: set[str] = set()
    store_root = science_store.resolve()
    try:
        for record in payload_records:
            path = science_store / str(record.get("path", ""))
            candidates = [path] if path.is_file() else list(path.rglob("*")) if path.is_dir() else []
            for child in candidates:
                if child.is_file() and resource_manifest_file(child) and child.stat().st_size > 0:
                    resolved = child.resolve()
                    if child.is_symlink() or not resolved.is_relative_to(store_root):
                        return False
                    live_files.add(resolved.relative_to(store_root).as_posix())
    except (OSError, ValueError):
        return False
    return bool(live_files and live_files == set(input_summary.get("paths", [])))


def indexed_source_derivatives_ready(
    science_store: Path,
    source_id: str,
    source_manifest: dict[str, str],
    payload_records: list[dict[str, object]],
    derived_paths: tuple[str, ...] | list[str],
    summary_cache: ResourceSummaryCache | None = None,
    *,
    retention_overrides: dict[str, dict[str, str]] | None = None,
) -> bool:
    if (
        source_manifest.get("status") != "ready"
        or not re.fullmatch(r"[0-9a-f]{64}", source_manifest.get("sha256", "").lower())
        or not source_manifest.get("release_identity")
        or not source_manifest.get("trust_source")
        or not derived_paths
        or any(not path_has_payload(science_store / rel_path) for rel_path in derived_paths)
    ):
        return False
    execution_sha256 = ""
    if payload_records:
        execution_sha256 = str(payload_records[0].get("content_sha256", ""))
    live_payload = any(
        path_has_payload(science_store / str(record.get("path", "")))
        for record in payload_records
        if str(record.get("path", "")).strip()
    )
    if live_payload and not structure_source_payload_matches_manifest(
        science_store,
        source_id,
        source_manifest,
        payload_records,
        summary_cache,
    ):
        return False
    structure_sources = {
        "biolip2": "BioLiP2",
        "binding_moad": "Binding MOAD",
        "plinder": "PLINDER",
    }
    if source_id in structure_sources:
        if not structure_targets_publication_ready(
            science_store,
            structure_sources[source_id],
            summary_cache,
            require_live_inputs=live_payload,
            retention_overrides=retention_overrides,
        ):
            return False
        if source_id == "plinder":
            source = plinder_parquet_source(science_store)
            if source is not None and not plinder_canonical_index_ready(
                source,
                science_store / "indexes/plinder_targets.tsv",
                science_store / "indexes/plinder_targets.manifest.json",
            ):
                return False
    if source_id in {"chembl", "bindingdb"} and ligand_similarity_index_ready(
        science_store,
        source_manifest_rows_by_id(science_store),
        summary_cache,
    ):
        return True
    if source_id == "chembl":
        database = science_store / "downloads/chembl/chembl_query.sqlite"
        return chembl_query_database_ready(
            database,
            database.with_suffix(".manifest.json"),
            source_manifest.get("sha256", ""),
            source_manifest.get("release_identity", ""),
            summary_cache,
        )
    if source_id == "bindingdb":
        return bindingdb_query_database_ready(
            science_store,
            source_manifest,
            execution_sha256,
            summary_cache,
        )
    mmseqs_namespaces = {
        "uniprot_human_reviewed": ("UniProt", 1),
        "biolip2": ("BioLiP2", 4),
        "metagenome_potential": ("MGnify", 1),
    }
    if source_id in mmseqs_namespaces:
        namespace, minimum = mmseqs_namespaces[source_id]
        rows = [
            row
            for row in read_tsv(science_store / "indexes/mmseqs/mmseqs_target_databases.tsv")
            if row.get("source_namespace") == namespace
            and row.get("status") == "ready"
            and row.get("manifest_schema_version") == ALIGNMENT_MANIFEST_SCHEMA_VERSION
            and re.fullmatch(r"[0-9a-f]{64}", row.get("source_digest", "").lower())
            and (
                (db_path := resolve_manifest_path(science_store, row.get("db_path", ""))) is not None
                and database_artifacts_present(db_path)
            )
        ]
        if not mmseqs_namespace_rows_complete(rows, namespace, minimum):
            return False
    build_rows = read_tsv(science_store / "index_build_manifest.tsv")
    if source_id == "binding_moad":
        return any(
            row.get("index") == "structure_targets"
            and "binding_moad" in row.get("input_path", "").replace("\\", "/").lower()
            and row.get("status") == "used"
            for row in build_rows
        )
    if source_id == "biolip2":
        return any(
            row.get("index") == "structure_targets"
            and "biolip2" in row.get("input_path", "").replace("\\", "/").lower()
            and row.get("status") == "used"
            for row in build_rows
        )
    if source_id == "plinder":
        return any(
            row.get("index") == "structure_targets"
            and row.get("status") == "used"
            and row.get("input_path", "").replace("\\", "/")
            == "indexes/plinder_targets.tsv"
            and row.get("output_path", "").replace("\\", "/")
            == "indexes/structure_targets.tsv"
            and int(row.get("rows", 0) or 0) > 0
            and "plinder_canonical_rows" in row.get("notes", "").split(";")
            for row in build_rows
        )
    return True


def retention_source_identity_and_derivatives_ready(
    science_store: Path,
    row: dict[str, str],
    summary_cache: ResourceSummaryCache | None = None,
    *,
    retention_overrides: dict[str, dict[str, str]] | None = None,
) -> bool:
    if (
        row.get("schema_version") != SOURCE_RETENTION_SCHEMA_VERSION
        or not re.fullmatch(r"[0-9a-f]{64}", row.get("source_manifest_sha256", "").lower())
    ):
        return False
    source_manifest = source_manifest_rows_by_id(science_store).get(row.get("source_id", ""), {})
    if not (
        source_manifest.get("status") == "ready"
        and source_manifest.get("sha256") == row.get("source_manifest_sha256")
        and source_manifest.get("release_identity") == row.get("release_identity")
        and source_manifest.get("trust_source") == row.get("trust_source")
    ):
        return False
    derived = [value for value in row.get("derived_paths", "").split(";") if value]
    return indexed_source_derivatives_ready(
        science_store,
        row.get("source_id", ""),
        source_manifest,
        retention_payload_records(row),
        derived,
        summary_cache,
        retention_overrides=retention_overrides,
    )


def legacy_pruned_structure_retention_candidates(
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, dict[str, str]]:
    source_rows = source_manifest_rows_by_id(science_store)
    retention_rows = {
        row.get("source_id", ""): row
        for row in source_retention_rows(science_store)
        if row.get("source_id")
    }
    contribution_names = {
        "binding_moad": "Binding MOAD",
        "biolip2": "BioLiP2",
        "plinder": "PLINDER",
    }
    candidates: dict[str, dict[str, str]] = {}
    for source_id, legacy_derived in LEGACY_STRUCTURE_RETENTION_DERIVED_PATHS.items():
        policy = INDEXED_SOURCE_RETENTION_POLICIES.get(source_id)
        if policy is None:
            continue
        retained = retention_rows.get(source_id, {})
        source = source_rows.get(source_id, {})
        records = retention_payload_records(retained)
        contribution = structure_targets_publication_contribution(
            science_store,
            contribution_names[source_id],
        )
        expected_payloads = {str(value).replace("\\", "/") for value in policy["payloads"]}
        current_derived = frozenset(str(value) for value in policy["derived"])
        recorded_derived = frozenset(filter(None, retained.get("derived_paths", "").split(";")))
        if not (
            retained.get("schema_version") == SOURCE_RETENTION_SCHEMA_VERSION
            and retained.get("state") == "pruned"
            and source.get("status") == "ready"
            and source.get("storage_state") == "indexed_source_pruned"
            and source.get("local_path", "").replace("\\", "/")
            == retained.get("source_local_path", "").replace("\\", "/")
            and source.get("sha256", "").lower()
            == retained.get("source_manifest_sha256", "").lower()
            and source.get("release_identity") == retained.get("release_identity")
            and source.get("trust_source") == retained.get("trust_source")
            and source.get("file_count") == retained.get("source_files")
            and source.get("bytes") == retained.get("source_bytes")
            and len(records) == 1
            and {str(records[0].get("path", "")).replace("\\", "/")} == expected_payloads
            and retention_payload_record_valid(records[0])
            and str(records[0].get("file_count", "")) == retained.get("source_files")
            and str(records[0].get("bytes", "")) == retained.get("source_bytes")
            and str(records[0].get("sha256", "")).lower()
            == retained.get("source_manifest_sha256", "").lower()
            and not path_has_payload(science_store / str(records[0].get("path", "")))
            and recorded_derived in {legacy_derived, current_derived}
            and contribution.get("source_id") == source_id
            and contribution.get("source") == contribution_names[source_id]
        ):
            continue
        candidate = dict(retained)
        candidate["derived_paths"] = ";".join(str(value) for value in policy["derived"])
        candidate["structure_input_bytes"] = str(contribution.get("input_bytes", ""))
        candidate["structure_input_digest"] = str(contribution.get("input_digest", ""))
        candidate["notes"] = "legacy_pruned_structure_projection_migrated"
        if structure_contribution_retention_identity_matches(contribution, candidate):
            candidates[source_id] = candidate
    if not candidates or not structure_targets_publication_ready(
        science_store,
        summary_cache=summary_cache,
        retention_overrides=candidates,
    ):
        return {}
    if any(
        not retention_source_identity_and_derivatives_ready(
            science_store,
            candidate,
            summary_cache,
            retention_overrides=candidates,
        )
        for candidate in candidates.values()
    ):
        return {}
    return candidates


def retained_source_derivatives_ready(
    science_store: Path,
    row: dict[str, str],
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    records = retention_payload_records(row)
    return bool(
        row.get("state") == "pruned"
        and records
        and all(retention_payload_record_valid(record) for record in records)
        and retention_source_identity_and_derivatives_ready(
            science_store,
            row,
            summary_cache,
        )
        and all(
            not path_has_payload(science_store / str(record.get("path", "")))
            for record in records
            if str(record.get("path", "")).strip()
        )
    )


def retention_covers_alignment_source_path(science_store: Path, source_path: str) -> bool:
    normalized = source_path.replace("\\", "/").strip("/").casefold()
    if not normalized:
        return False
    source_rows = source_manifest_rows_by_id(science_store)
    for retained in source_retention_rows(science_store):
        source_id = retained.get("source_id", "")
        policy = INDEXED_SOURCE_RETENTION_POLICIES.get(source_id, {})
        source = source_rows.get(source_id, {})
        records = retention_payload_records(retained)
        if not (
            retained.get("schema_version") == SOURCE_RETENTION_SCHEMA_VERSION
            and retained.get("state") == "pruned"
            and source.get("status") == "ready"
            and source.get("storage_state") == "indexed_source_pruned"
            and source.get("sha256", "").lower()
            == retained.get("source_manifest_sha256", "").lower()
            and source.get("release_identity") == retained.get("release_identity")
            and source.get("trust_source") == retained.get("trust_source")
            and records
            and all(retention_payload_record_valid(record) for record in records)
            and all(
                not path_has_payload(science_store / str(record.get("path", "")))
                for record in records
            )
            and "indexes/mmseqs/mmseqs_target_databases.tsv"
            in {str(value).replace("\\", "/") for value in policy.get("derived", ())}
            and path_has_payload(science_store / "indexes/mmseqs/mmseqs_target_databases.tsv")
        ):
            continue
        for record in records:
            payload = str(record.get("path", "")).replace("\\", "/").strip("/").casefold()
            if normalized == payload or (
                record.get("kind") == "directory" and normalized.startswith(f"{payload}/")
            ):
                return True
    return False


def retained_source_ready(
    science_store: Path,
    source_id: str,
    expected_local_path: str = "",
    summary_cache: ResourceSummaryCache | None = None,
    *,
    expected_release_identity: str = "",
    expected_trust_source: str = "",
    expected_sha256: str = "",
) -> dict[str, str]:
    row = source_retention_row(science_store, source_id)
    if expected_local_path and row.get("source_local_path", "").replace("\\", "/") != expected_local_path.replace("\\", "/"):
        return {}
    if expected_release_identity and row.get("release_identity", "") != expected_release_identity:
        return {}
    if expected_trust_source and row.get("trust_source", "") != expected_trust_source:
        return {}
    if expected_sha256 and row.get("execution_sha256", "").lower() != expected_sha256.lower():
        return {}
    if row.get("state") != "pruned":
        return {}
    return row if retained_source_derivatives_ready(science_store, row, summary_cache) else {}


def retention_live_payloads_match_records(
    science_store: Path,
    row: dict[str, str],
    summary_cache: ResourceSummaryCache | None = None,
) -> bool:
    records = retention_payload_records(row)
    if not records or not all(retention_payload_record_valid(record) for record in records):
        return False
    for record in records:
        relative = str(record.get("path", ""))
        if not relative:
            return False
        path = science_store / relative
        if not path_has_payload(path):
            continue
        kind = "directory" if path.is_dir() else "file" if path.is_file() else ""
        summary = resource_payload_summary(path, cache=summary_cache)
        try:
            if not (
                kind == str(record.get("kind", ""))
                and int(summary.get("file_count", 0) or 0)
                == int(record.get("file_count", 0) or 0)
                and int(summary.get("bytes", 0) or 0) == int(record.get("bytes", 0) or 0)
                and str(summary.get("sha256", "")).lower()
                == str(record.get("sha256", "")).lower()
                and (
                    kind != "file"
                    or str(summary.get("content_sha256", "")).lower()
                    == str(record.get("content_sha256", "")).lower()
                )
            ):
                return False
        except (TypeError, ValueError):
            return False
    return True


def reconcile_interrupted_source_retention(
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    rows = source_retention_rows(science_store)
    changed = 0
    for row in rows:
        if row.get("state") not in {"pruning", "cleanup_failed"}:
            continue
        if not retention_source_identity_and_derivatives_ready(science_store, row, summary_cache):
            continue
        records = retention_payload_records(row)
        if not records or any(
            path_has_payload(science_store / str(record.get("path", ""))) for record in records
        ):
            continue
        recorded_bytes = sum(int(record.get("bytes", 0) or 0) for record in records)
        row["state"] = "pruned"
        row["pruned_bytes"] = str(max(int(row.get("pruned_bytes", "0") or 0), recorded_bytes))
        row["pruned_at"] = row.get("pruned_at", "") or str(int(time.time()))
        row["notes"] = "reconciled_after_source_payload_cleanup"
        changed += 1
    if changed:
        write_source_retention_rows(science_store, rows)
        update_source_storage_states(
            science_store,
            {
                row.get("source_id", ""): "indexed_source_pruned"
                for row in rows
                if row.get("state") == "pruned"
            },
        )
    return {"status": "ready", "reconciled": changed}


def write_source_retention_rows(science_store: Path, rows: list[dict[str, str]]) -> None:
    write_simple_tsv(
        source_retention_manifest_path(science_store),
        SOURCE_RETENTION_FIELDS,
        sorted(rows, key=lambda row: row.get("source_id", "")),
    )


def safe_remove_indexed_payload(science_store: Path, relative: str) -> int:
    store_root = science_store.resolve()
    downloads_root = (science_store / "downloads").resolve()
    target = science_store / relative
    resolved = target.resolve()
    try:
        resolved.relative_to(downloads_root)
        resolved.relative_to(store_root)
    except ValueError as err:
        raise OSError(f"source_retention_path_outside_downloads:{relative}") from err
    if resolved == downloads_root or target.is_symlink():
        raise OSError(f"source_retention_path_unsafe:{relative}")
    if target.is_dir() and any(child.is_symlink() for child in target.rglob("*")):
        raise OSError(f"source_retention_symlink_rejected:{relative}")
    reclaimed = directory_size(target)
    if target.is_dir():
        shutil.rmtree(target)
    elif target.exists():
        target.unlink()
    return reclaimed


def remove_plinder_canonical_intermediates(science_store: Path) -> int:
    indexes_root = (science_store / "indexes").resolve()
    reclaimed = 0
    for relative in ("indexes/plinder_targets.tsv", "indexes/plinder_targets.manifest.json"):
        target = science_store / relative
        if not target.exists():
            continue
        resolved = target.resolve()
        try:
            resolved.relative_to(indexes_root)
        except ValueError as err:
            raise OSError(f"plinder_intermediate_path_unsafe:{relative}") from err
        if target.is_symlink() or not target.is_file():
            raise OSError(f"plinder_intermediate_file_required:{relative}")
        reclaimed += target.stat().st_size
        target.unlink()
    return reclaimed


def update_source_storage_states(science_store: Path, states: dict[str, str]) -> None:
    manifest = science_store / "manifests/source_file_manifest.tsv"
    rows = read_tsv(manifest)
    fields = [
        "source_id",
        "source_label",
        "source_class",
        "local_path",
        "status",
        "storage_state",
        "file_count",
        "bytes",
        "sha256",
        "provenance",
        "release_identity",
        "trust_source",
    ]
    for row in rows:
        row["storage_state"] = states.get(row.get("source_id", ""), row.get("storage_state", "materialized"))
    write_simple_tsv(manifest, fields, rows)


def prune_indexed_source_payloads(
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    source_rows = source_manifest_rows_by_id(science_store)
    previous = {row.get("source_id", ""): row for row in source_retention_rows(science_store)}
    download_rows = {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "download_execution_manifest.tsv")
    }
    external_rows = {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "external_sync_execution_manifest.tsv")
    }
    legacy_structure_candidates = legacy_pruned_structure_retention_candidates(
        science_store,
        summary_cache,
    )
    prepared: list[dict[str, str]] = []
    preserved = [
        dict(row)
        for source_id, row in previous.items()
        if source_id not in INDEXED_SOURCE_RETENTION_POLICIES
    ]
    failures: list[str] = []
    for source_id, policy in INDEXED_SOURCE_RETENTION_POLICIES.items():
        old = previous.get(source_id, {})
        if source_id in legacy_structure_candidates:
            prepared.append(dict(legacy_structure_candidates[source_id]))
            continue
        if old:
            migrated = dict(old)
            migrated["derived_paths"] = ";".join(str(value) for value in policy["derived"])
            structure_source = {
                "binding_moad": "Binding MOAD",
                "biolip2": "BioLiP2",
                "plinder": "PLINDER",
            }.get(source_id, "")
            contribution = (
                structure_targets_publication_contribution(science_store, structure_source)
                if structure_source
                else {}
            )
            if contribution:
                migrated["structure_input_bytes"] = str(contribution.get("input_bytes", ""))
                migrated["structure_input_digest"] = str(contribution.get("input_digest", ""))
            if retained_source_derivatives_ready(science_store, migrated, summary_cache):
                prepared.append(migrated)
                continue
        if retained_source_derivatives_ready(science_store, old, summary_cache):
            prepared.append(dict(old))
            continue
        if old.get("state") in {"pruning", "cleanup_failed"} and retention_source_identity_and_derivatives_ready(
            science_store,
            old,
            summary_cache,
        ):
            if not retention_live_payloads_match_records(science_store, old, summary_cache):
                failures.append(f"{source_id}:source_payload_changed_during_cleanup_resume")
                continue
            resumed = dict(old)
            resumed["state"] = "pruning"
            resumed["notes"] = "resuming_verified_source_cleanup"
            prepared.append(resumed)
            continue
        source_manifest = source_rows.get(source_id, {})
        payload_records: list[dict[str, object]] = []
        for relative in policy["payloads"]:
            path = science_store / str(relative)
            if not path_has_payload(path):
                failures.append(f"{source_id}:source_payload_missing:{relative}")
                continue
            summary = resource_payload_summary(path, cache=summary_cache)
            payload_records.append(
                {
                    "path": str(relative).replace("\\", "/"),
                    "kind": "directory" if path.is_dir() else "file",
                    "file_count": int(summary.get("file_count", 0) or 0),
                    "bytes": int(summary.get("bytes", 0) or 0),
                    "sha256": str(summary.get("sha256", "")),
                    "content_sha256": str(summary.get("content_sha256", "")),
                }
            )
        derived = tuple(str(value) for value in policy["derived"])
        if len(payload_records) != len(policy["payloads"]):
            continue
        if not indexed_source_derivatives_ready(
            science_store,
            source_id,
            source_manifest,
            payload_records,
            derived,
            summary_cache,
        ):
            failures.append(f"{source_id}:derived_index_not_ready")
            continue
        execution = download_rows.get(source_id) or external_rows.get(source_id) or {}
        structure_source = {
            "biolip2": "BioLiP2",
            "binding_moad": "Binding MOAD",
            "plinder": "PLINDER",
        }.get(source_id, "")
        structure_contribution = (
            structure_targets_publication_contribution(science_store, structure_source)
            if structure_source
            else {}
        )
        prepared.append(
            {
                "schema_version": SOURCE_RETENTION_SCHEMA_VERSION,
                "source_id": source_id,
                "source_local_path": source_manifest.get("local_path", ""),
                "state": "pruning",
                "source_files": source_manifest.get("file_count", "0"),
                "source_bytes": source_manifest.get("bytes", "0"),
                "source_manifest_sha256": source_manifest.get("sha256", ""),
                "execution_sha256": execution.get("sha256", ""),
                "release_identity": source_manifest.get("release_identity", ""),
                "provenance": source_manifest.get("provenance", ""),
                "trust_source": source_manifest.get("trust_source", ""),
                "payload_records_json": json.dumps(payload_records, ensure_ascii=True, separators=(",", ":")),
                "derived_paths": ";".join(derived),
                "structure_input_bytes": str(structure_contribution.get("input_bytes", "")),
                "structure_input_digest": str(structure_contribution.get("input_digest", "")),
                "pruned_bytes": "0",
                "pruned_at": "",
                "notes": "validated_indexes_pending_source_cleanup",
            }
        )
    if failures or len(prepared) != len(INDEXED_SOURCE_RETENTION_POLICIES):
        return {
            "status": "needs_repair",
            "ready": len(prepared),
            "total": len(INDEXED_SOURCE_RETENTION_POLICIES),
            "reclaimed_bytes": 0,
            "failures": failures,
        }
    def persist_prepared() -> None:
        write_source_retention_rows(science_store, [*prepared, *preserved])

    persist_prepared()
    reclaimed_total = 0
    for row in prepared:
        if row.get("state") == "pruned":
            continue
        reclaimed = 0
        try:
            if not retention_live_payloads_match_records(science_store, row, summary_cache):
                raise OSError("source_payload_changed_before_cleanup")
            for record in retention_payload_records(row):
                if not retention_live_payloads_match_records(science_store, row, summary_cache):
                    raise OSError("source_payload_changed_during_cleanup")
                relative = str(record.get("path", ""))
                reclaimed += safe_remove_indexed_payload(science_store, relative)
                if summary_cache is not None:
                    prefix = str((science_store / relative).resolve()).casefold()
                    for key in [key for key in summary_cache if key.casefold() == prefix or key.casefold().startswith(f"{prefix}{os.sep}")]:
                        summary_cache.pop(key, None)
        except OSError as err:
            row["state"] = "cleanup_failed"
            row["notes"] = exception_note(err)
            persist_prepared()
            return {
                "status": "needs_repair",
                "ready": sum(item.get("state") == "pruned" for item in prepared),
                "total": len(prepared),
                "reclaimed_bytes": reclaimed_total,
                "failures": [f"{row.get('source_id')}:{exception_note(err)}"],
            }
        reclaimed_total += reclaimed
        row["state"] = "pruned"
        row["pruned_bytes"] = str(reclaimed)
        row["pruned_at"] = str(int(time.time()))
        row["notes"] = "source_payload_removed_after_verified_index_publish"
        persist_prepared()
    states = {row["source_id"]: "indexed_source_pruned" for row in prepared}
    update_source_storage_states(science_store, states)
    valid = all(retained_source_derivatives_ready(science_store, row, summary_cache) for row in prepared)
    cleanup_failures: list[str] = []
    plinder_retention = source_retention_row(science_store, "plinder")
    plinder_policy_paths = ";".join(
        str(value) for value in INDEXED_SOURCE_RETENTION_POLICIES.get("plinder", {}).get("derived", ())
    )
    if (
        valid
        and plinder_retention.get("derived_paths") == plinder_policy_paths
        and retained_source_derivatives_ready(science_store, plinder_retention, summary_cache)
        and structure_targets_publication_ready(science_store, "PLINDER", summary_cache)
    ):
        try:
            reclaimed_total += remove_plinder_canonical_intermediates(science_store)
        except OSError as err:
            cleanup_failures.append(f"plinder:{exception_note(err)}")
    return {
        "status": "ready" if valid and not cleanup_failures else "needs_repair",
        "ready": sum(row.get("state") == "pruned" for row in prepared),
        "total": len(prepared),
        "reclaimed_bytes": reclaimed_total,
        "manifest": str(source_retention_manifest_path(science_store)),
        "failures": cleanup_failures if valid else ["retained_derivative_validation_failed"],
    }


def rcsb_cache_usage(cache_root: Path) -> tuple[int, int]:
    files = 0
    bytes_total = 0
    if not cache_root.is_dir():
        return files, bytes_total
    for path in cache_root.rglob("*.cif.gz"):
        if not path.is_file() or path.is_symlink():
            continue
        try:
            size = path.stat().st_size
        except OSError:
            continue
        if size > 0:
            files += 1
            bytes_total += size
    return files, bytes_total


def prune_rcsb_on_demand_cache(
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    policy_path = science_store / "downloads/rcsb/on_demand_policy.json"
    policy = read_json_object(policy_path)
    if policy.get("mode") != "on_demand":
        return {"status": "ready", "reclaimed_bytes": 0, "reason": "rcsb_complete_mode_retained"}
    cache_root = science_store / "downloads/rcsb/mmcif"
    if not cache_root.is_dir():
        return {"status": "needs_repair", "reclaimed_bytes": 0, "reason": "rcsb_cache_missing"}
    if not structure_targets_publication_ready(
        science_store,
        "RCSB",
        summary_cache,
    ):
        return {
            "status": "needs_repair",
            "reclaimed_bytes": 0,
            "reason": "rcsb_structure_publication_identity_mismatch",
        }
    rcsb_contribution = structure_targets_publication_contribution(science_store, "RCSB")
    scope = ligand_bound_pdb_scope(science_store)
    if not scope:
        return {
            "status": "needs_repair",
            "reclaimed_bytes": 0,
            "reason": "rcsb_scope_missing_before_cache_prune",
        }
    files: list[tuple[Path, int, int]] = []
    discarded: list[tuple[Path, int, int]] = []
    for path in cache_root.rglob("*.cif.gz"):
        if not path.is_file() or path.is_symlink():
            continue
        stat = path.stat()
        pdb_id = path.name[:-7].upper() if path.name.lower().endswith(".cif.gz") else ""
        if (
            stat.st_size > 0
            and pdb_id in scope
            and valid_rcsb_coordinate(path, pdb_id)
        ):
            files.append((path, stat.st_size, stat.st_mtime_ns))
        else:
            discarded.append((path, stat.st_size, stat.st_mtime_ns))
    if not files:
        return {"status": "needs_repair", "reclaimed_bytes": 0, "reason": "rcsb_cache_empty"}
    files.sort(key=lambda item: (item[2], item[0].as_posix()), reverse=True)
    retained: set[Path] = set()
    retained_bytes = 0
    install_cache_files = min(RCSB_ON_DEMAND_INITIAL_CACHE_FILES, RCSB_ON_DEMAND_MAX_CACHE_FILES)
    for path, size, _mtime in files:
        if len(retained) >= install_cache_files:
            continue
        if retained and retained_bytes + size > RCSB_ON_DEMAND_MAX_CACHE_BYTES:
            continue
        retained.add(path)
        retained_bytes += size
    removed_bytes = 0
    cache_resolved = cache_root.resolve()
    for path, size, _mtime in [*files, *discarded]:
        if path in retained:
            continue
        try:
            path.resolve().relative_to(cache_resolved)
        except ValueError as err:
            return {
                "status": "needs_repair",
                "reclaimed_bytes": removed_bytes,
                "reason": f"rcsb_cache_path_outside_root:{exception_note(err)}",
            }
        path.unlink()
        removed_bytes += size
    for directory in sorted(
        (path for path in cache_root.rglob("*") if path.is_dir()),
        key=lambda path: len(path.parts),
        reverse=True,
    ):
        try:
            directory.rmdir()
        except OSError:
            pass
    if summary_cache is not None:
        prefix = str(cache_resolved).casefold()
        for key in [key for key in summary_cache if key.casefold() == prefix or key.casefold().startswith(f"{prefix}{os.sep}")]:
            summary_cache.pop(key, None)
    snapshot = write_rcsb_scope_manifest(
        science_store,
        cache_root,
        scope,
        summary_cache,
        scope_mode="on-demand",
    )
    write_rcsb_on_demand_policy(science_store, snapshot)
    foldseek_manifest = science_store / "indexes/foldseek/foldseek_target_databases.tsv"
    foldseek_rows = read_tsv(foldseek_manifest)
    scope_rows = read_tsv(science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv")
    current_scope_ids = {
        row.get("pdb_id", "").upper()
        for row in scope_rows
        if row.get("pdb_id")
    }
    cached_ids = {
        row.get("pdb_id", "").upper()
        for row in scope_rows
        if row.get("status") == "ready"
    }
    for row in foldseek_rows:
        if row.get("source_id") != "rcsb_mmcif" or row.get("status") != "ready":
            continue
        validated_manifest = resolve_manifest_path(
            science_store, row.get("indexed_validated_ids_manifest", "")
        )
        indexed_ids = (
            {
                item.get("pdb_id", "").upper()
                for item in read_tsv(validated_manifest)
                if item.get("pdb_id")
            }
            if validated_manifest is not None
            else set()
        )
        row.update(
            {
                "scope_digest": str(snapshot["scope_digest"]),
                "scope_coordinate_digest": str(snapshot["coordinate_digest"]),
                "scope_count": str(snapshot["scope_count"]),
                "scope_ready_count": str(snapshot["ready_count"]),
                "scope_missing_count": str(snapshot["missing_count"]),
                "scope_coverage": f"{row.get('indexed_pdb_count', 0)}/{snapshot['scope_count']}",
                "refresh_needed": str(
                    bool((cached_ids - indexed_ids) or (indexed_ids - current_scope_ids))
                ).lower(),
            }
        )
    if foldseek_rows:
        write_simple_tsv(foldseek_manifest, list(foldseek_rows[0]), foldseek_rows)
    execution_manifest = science_store / "external_sync_execution_manifest.tsv"
    execution_rows = read_tsv(execution_manifest)
    execution_fields = [
        "source_id",
        "source_label",
        "target_rel_path",
        "status",
        "bytes",
        "sha256",
        "plan_digest",
        "release_identity",
        "trust_source",
        "scope_mode",
        "scope_digest",
        "scope_ready_count",
        "scope_count",
        "scope_missing_count",
        "notes",
    ]
    updated_execution = False
    for row in execution_rows:
        if row.get("source_id") != "rcsb_ligand_bound":
            continue
        row.update(
            {
                "status": "ready",
                "bytes": str(snapshot["coordinate_bytes"]),
                "sha256": str(snapshot["coordinate_digest"]),
                "scope_mode": str(snapshot["scope_mode"]),
                "scope_digest": str(snapshot["scope_digest"]),
                "scope_ready_count": str(snapshot["ready_count"]),
                "scope_count": str(snapshot["scope_count"]),
                "scope_missing_count": str(snapshot["missing_count"]),
                "notes": "bounded_on_demand_coordinate_cache",
            }
        )
        updated_execution = True
    if not updated_execution:
        return {
            "status": "needs_repair",
            "reclaimed_bytes": removed_bytes,
            "reason": "rcsb_external_execution_row_missing",
        }
    write_simple_tsv(execution_manifest, execution_fields, execution_rows)
    source_manifest = science_store / "manifests/source_file_manifest.tsv"
    source_rows = read_tsv(source_manifest)
    source_summary = resource_payload_summary(cache_root, cache=summary_cache)
    source_fields = [
        "source_id",
        "source_label",
        "source_class",
        "local_path",
        "status",
        "storage_state",
        "file_count",
        "bytes",
        "sha256",
        "provenance",
        "release_identity",
        "trust_source",
    ]
    rcsb_source: dict[str, str] = {}
    for row in source_rows:
        if row.get("source_id") == "rcsb_ligand_bound":
            row.update(
                {
                    "status": "ready",
                    "storage_state": "bounded_runtime_cache",
                    "file_count": str(source_summary.get("file_count", 0)),
                    "bytes": str(source_summary.get("bytes", 0)),
                    "sha256": str(source_summary.get("sha256", "")),
                }
            )
            rcsb_source = row
    if not rcsb_source:
        return {
            "status": "needs_repair",
            "reclaimed_bytes": removed_bytes,
            "reason": "rcsb_source_manifest_row_missing",
        }
    write_simple_tsv(source_manifest, source_fields, source_rows)
    hash_manifest = science_store / "manifests/hash_manifest.tsv"
    hash_rows = read_tsv(hash_manifest)
    for row in hash_rows:
        if row.get("source_id") == "rcsb_ligand_bound":
            row.update(
                {
                    "status": "ready",
                    "bytes": str(source_summary.get("bytes", 0)),
                    "sha256": str(source_summary.get("sha256", "")),
                }
            )
    write_simple_tsv(
        hash_manifest,
        ["source_id", "local_path", "status", "bytes", "sha256"],
        hash_rows,
    )
    retention_rows = [
        row for row in source_retention_rows(science_store) if row.get("source_id") != "rcsb_ligand_bound"
    ]
    retention_rows.append(
        {
            "schema_version": SOURCE_RETENTION_SCHEMA_VERSION,
            "source_id": "rcsb_ligand_bound",
            "source_local_path": "downloads/rcsb/mmcif",
            "state": "runtime_cache_bounded",
            "source_files": str(source_summary.get("file_count", 0)),
            "source_bytes": str(source_summary.get("bytes", 0)),
            "source_manifest_sha256": str(rcsb_contribution.get("source_digest", "")),
            "execution_sha256": str(snapshot["coordinate_digest"]),
            "release_identity": str(rcsb_contribution.get("release_identity", "")),
            "provenance": rcsb_source.get("provenance", ""),
            "trust_source": str(rcsb_contribution.get("trust_source", "")),
            "payload_records_json": "[]",
            "derived_paths": ";".join(
                [
                    "indexes/foldseek/foldseek_target_databases.tsv",
                    "indexes/structure_targets.tsv",
                    "downloads/rcsb/on_demand_policy.json",
                ]
            ),
            "structure_input_bytes": str(rcsb_contribution.get("input_bytes", "")),
            "structure_input_digest": str(rcsb_contribution.get("input_digest", "")),
            "pruned_bytes": str(removed_bytes),
            "pruned_at": str(int(time.time())),
            "notes": "runtime_coordinate_cache_bounded_after_index_publish",
        }
    )
    write_source_retention_rows(science_store, retention_rows)
    return {
        "status": "ready",
        "reclaimed_bytes": removed_bytes,
        "cached_files": int(source_summary.get("file_count", 0)),
        "cached_bytes": int(source_summary.get("bytes", 0)),
        "max_cache_files": RCSB_ON_DEMAND_MAX_CACHE_FILES,
        "max_cache_bytes": RCSB_ON_DEMAND_MAX_CACHE_BYTES,
    }


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
                        "notes": "source_identity_and_domain_validation_required",
                    }
                )
                continue
            retained = retained_source_ready(
                science_store,
                source_id,
                local_path,
                summary_cache,
                expected_release_identity=release_identity,
                expected_trust_source=trust_source,
                expected_sha256=expected or "",
            )
            if retained:
                ready += 1
                retained_sha256 = retained.get("execution_sha256", "") or expected or ""
                retained_bytes = retained.get("source_bytes", "0")
                writer.writerow(
                    {
                        "source_id": source_id,
                        "url": url,
                        "local_path": local_path,
                        "status": "ready",
                        "bytes": retained_bytes,
                        "sha256": retained_sha256,
                        "expected_sha256": expected or "",
                        "observed_bytes": retained_bytes,
                        "observed_sha256": retained_sha256,
                        "source_version": source_version,
                        "release_identity": release_identity,
                        "trust_source": trust_source,
                        "validation_schema": previous.get(
                            "validation_schema", SOURCE_PAYLOAD_VALIDATION_SCHEMA_VERSION
                        ),
                        "validated_records": previous.get("validated_records", "1"),
                        "validation_reason": previous.get(
                            "validation_reason", "domain_schema_and_records_valid"
                        ),
                        "resource_identity": previous.get("resource_identity", "indexed_source_pruned"),
                        "etag": previous.get("etag", ""),
                        "content_length": previous.get("content_length", retained_bytes),
                        "completed_at": str(int(time.time())),
                        "notes": "verified_index_reused_source_payload_remains_pruned",
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
    science_store: Path | None = None,
) -> dict[str, object]:
    source_url = row.get("source_url") or row.get("download_url") or row.get("landing_url")
    include_pattern = row.get("include_pattern", "")
    if not source_url:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "html_source_url_required"}
    inventory_result = (
        publisher_inventory_with_cache(row, science_store, html_links_inventory)
        if row.get("checksum_url", "").strip()
        else {"items": [], "source": "not_required", "attempts": 0, "error": ""}
    )
    inventory = inventory_result["items"]
    if row.get("checksum_url", "").strip() and not inventory:
        return {
            "status": "failed",
            "bytes": directory_size(target),
            "sha256": directory_digest(target, summary_cache),
            "notes": f"publisher_inventory_unavailable:{inventory_result.get('error', '')}"[:1000],
        }
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
        "notes": f"html_links_synced:{len(links)}:inventory={inventory_result.get('source', 'not_required')}",
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
        size = remote_resource_size(link)
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


def normalized_publisher_inventory(
    row: dict[str, str],
    items: list[dict[str, object]],
) -> list[dict[str, object]]:
    normalized: list[dict[str, object]] = []
    for item in items:
        relative = safe_inventory_relative_path(str(item.get("relative_path", "")))
        try:
            size = int(item.get("size", 0) or 0)
        except (TypeError, ValueError) as err:
            raise ValueError("publisher_inventory_size_invalid") from err
        algorithm = str(item.get("digest_algorithm", "")).lower()
        digest = str(item.get("digest", "")).lower()
        media_url = str(item.get("media_url", ""))
        digest_length = 32 if algorithm == "md5" else 64 if algorithm == "sha256" else 0
        if (
            relative is None
            or size <= 0
            or digest_length == 0
            or not re.fullmatch(rf"[0-9a-f]{{{digest_length}}}", digest)
            or urllib.parse.urlparse(media_url).scheme != "https"
        ):
            raise ValueError("publisher_inventory_identity_incomplete")
        normalized.append(
            {
                "relative_path": relative.as_posix(),
                "size": size,
                "digest_algorithm": algorithm,
                "digest": digest,
                "media_url": media_url,
            }
        )
    if not normalized:
        raise ValueError("publisher_inventory_empty")
    if row.get("source_id", "").strip() == "metagenome_potential":
        required = {"LICENSE", "README.txt", "md5sum.txt", "mgy_clusters.fa.gz"}
        if {str(item["relative_path"]) for item in normalized} != required:
            raise ValueError("publisher_inventory_mgnify_release_incomplete")
    return sorted(normalized, key=lambda item: str(item["relative_path"]))


def publisher_inventory_with_cache(
    row: dict[str, str],
    science_store: Path | None,
    fetcher: Callable[[dict[str, str]], list[dict[str, object]]],
) -> dict[str, object]:
    errors: list[str] = []
    source_url = str(row.get("source_url") or row.get("download_url") or row.get("landing_url") or "")
    for attempt in range(1, PUBLISHER_INVENTORY_ATTEMPTS + 1):
        try:
            items = normalized_publisher_inventory(row, fetcher(row))
            if science_store is not None:
                cache_path = science_store / "manifests/publisher_inventory_cache.json"
                cache = load_json(cache_path)
                raw_entries = cache.get("entries", {})
                entries = dict(raw_entries) if isinstance(raw_entries, dict) else {}
                source_id = row.get("source_id", "").strip() or external_plan_digest(row)
                entries[source_id] = {
                    "plan_digest": external_plan_digest(row),
                    "release_identity": row.get("release_or_date", ""),
                    "trust_source": row.get("trust_source", ""),
                    "source_url": source_url,
                    "items": items,
                    "updated_at": int(time.time()),
                }
                write_json_atomic(
                    cache_path,
                    {
                        "schema_version": PUBLISHER_INVENTORY_CACHE_SCHEMA_VERSION,
                        "entries": entries,
                    },
                )
            return {
                "items": items,
                "source": "remote",
                "attempts": attempt,
                "error": "",
                "source_url": source_url,
            }
        except (OSError, ValueError, UnicodeError, json.JSONDecodeError) as err:
            errors.append(f"{type(err).__name__}:{err}")
            if attempt < PUBLISHER_INVENTORY_ATTEMPTS:
                time.sleep(attempt)

    if science_store is not None:
        cache = load_json(science_store / "manifests/publisher_inventory_cache.json")
        raw_entries = cache.get("entries", {})
        source_id = row.get("source_id", "").strip() or external_plan_digest(row)
        entry = raw_entries.get(source_id, {}) if isinstance(raw_entries, dict) else {}
        cached_items = entry.get("items", []) if isinstance(entry, dict) else []
        if (
            cache.get("schema_version") == PUBLISHER_INVENTORY_CACHE_SCHEMA_VERSION
            and isinstance(entry, dict)
            and entry.get("plan_digest") == external_plan_digest(row)
            and entry.get("release_identity") == row.get("release_or_date", "")
            and entry.get("trust_source") == row.get("trust_source", "")
            and isinstance(cached_items, list)
        ):
            try:
                items = normalized_publisher_inventory(row, cached_items)
            except ValueError:
                items = []
            if items:
                return {
                    "items": items,
                    "source": "cache",
                    "attempts": PUBLISHER_INVENTORY_ATTEMPTS,
                    "error": " | ".join(errors),
                    "source_url": source_url,
                }
    return {
        "items": [],
        "source": "unavailable",
        "attempts": PUBLISHER_INVENTORY_ATTEMPTS,
        "error": " | ".join(errors),
        "source_url": source_url,
    }


def normalized_publisher_capacity_inventory(
    row: dict[str, str],
    items: list[dict[str, object]],
) -> list[dict[str, object]]:
    source_url = str(row.get("source_url") or row.get("download_url") or row.get("landing_url") or "")
    source_host = (urllib.parse.urlparse(source_url).hostname or "").lower()
    normalized: list[dict[str, object]] = []
    for item in items:
        relative = safe_inventory_relative_path(str(item.get("relative_path", "")))
        try:
            size = int(item.get("size", 0) or 0)
        except (TypeError, ValueError) as err:
            raise ValueError("publisher_capacity_size_invalid") from err
        media_url = str(item.get("media_url", ""))
        parsed_media = urllib.parse.urlparse(media_url)
        if (
            relative is None
            or size <= 0
            or parsed_media.scheme != "https"
            or not source_host
            or (parsed_media.hostname or "").lower() != source_host
        ):
            raise ValueError("publisher_capacity_identity_incomplete")
        normalized.append(
            {
                "relative_path": relative.as_posix(),
                "size": size,
                "digest_algorithm": str(item.get("digest_algorithm", "")).lower(),
                "digest": str(item.get("digest", "")).lower(),
                "media_url": media_url,
            }
        )
    paths = [str(item["relative_path"]) for item in normalized]
    required = UNCHECKSUMMED_PUBLISHER_REQUIRED_PATHS.get(row.get("source_id", "").strip())
    if not normalized or len(paths) != len(set(paths)) or (required is not None and set(paths) != required):
        raise ValueError("publisher_capacity_release_links_incomplete")
    return sorted(normalized, key=lambda item: str(item["relative_path"]))


def publisher_capacity_inventory_with_retry(
    row: dict[str, str],
    fetcher: Callable[[dict[str, str]], list[dict[str, object]]],
) -> dict[str, object]:
    errors: list[str] = []
    source_url = str(row.get("source_url") or row.get("download_url") or row.get("landing_url") or "")
    for attempt in range(1, PUBLISHER_INVENTORY_ATTEMPTS + 1):
        try:
            return {
                "items": normalized_publisher_capacity_inventory(row, fetcher(row)),
                "source": "remote_capacity",
                "attempts": attempt,
                "error": "",
                "source_url": source_url,
            }
        except (OSError, ValueError, UnicodeError, json.JSONDecodeError) as err:
            errors.append(f"{type(err).__name__}:{err}")
            if attempt < PUBLISHER_INVENTORY_ATTEMPTS:
                time.sleep(attempt)
    return {
        "items": [],
        "source": "unavailable",
        "attempts": PUBLISHER_INVENTORY_ATTEMPTS,
        "error": " | ".join(errors),
        "source_url": source_url,
    }


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
    *,
    rcsb_mode: str = "on-demand",
) -> dict[str, object]:
    mode = normalize_rcsb_mode(rcsb_mode)
    pdb_ids = ligand_bound_pdb_scope(science_store)
    if not pdb_ids:
        return {
            "status": "requires_resource_bundle",
            "bytes": 0,
            "sha256": "",
            "notes": "ligand_bound_pdb_scope_missing",
        }
    target.mkdir(parents=True, exist_ok=True)
    if mode == "on-demand":
        resumable_partials, obsolete_partials = rcsb_partial_download_inventory(target)
        for partial in obsolete_partials:
            quarantine_partial(partial)
        existing_ready = any(
            valid_rcsb_coordinate(
                target / pdb_id[1:3].lower() / f"{pdb_id.lower()}.cif.gz",
                pdb_id,
            )
            for pdb_id in sorted(pdb_ids)
        )
        seeded = 0
        seed_failures: list[str] = []
        if not existing_ready:
            template = row.get("source_url") or "https://files.rcsb.org/download/{pdb_id}.cif.gz"
            seed_ids = sorted(pdb_ids)[:RCSB_ON_DEMAND_INITIAL_CACHE_FILES]

            def seed_one(pdb_id: str) -> tuple[str, bool, str]:
                destination = target / pdb_id[1:3].lower() / f"{pdb_id.lower()}.cif.gz"
                url = template.replace("{pdb_id}", pdb_id).replace("{pdb_id_lower}", pdb_id.lower())
                try:
                    result = copy_or_download(
                        url,
                        destination,
                        None,
                        validator=lambda path, expected=pdb_id: rcsb_coordinate_validation(path, expected),
                        summary_cache=summary_cache,
                    )
                    return pdb_id, result.get("status") == "ready", str(result.get("status", "unknown"))
                except Exception as err:  # noqa: BLE001 - retain per-coordinate seed diagnostics
                    return pdb_id, False, exception_note(err)

            workers = min(
                max(1, int(os.environ.get("GENOLIGAND_RCSB_DOWNLOAD_WORKERS", str(RCSB_DOWNLOAD_WORKERS)))),
                len(seed_ids),
            )
            with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
                for completed_seed, (pdb_id, ready, note) in enumerate(pool.map(seed_one, seed_ids), start=1):
                    if ready:
                        seeded += 1
                    else:
                        seed_failures.append(f"{pdb_id}:{note}")
                    if progress and (completed_seed == 1 or completed_seed % 25 == 0):
                        progress(
                            "downloading",
                            completed_seed,
                            len(seed_ids),
                            f"正在准备按需结构缓存 {completed_seed}/{len(seed_ids)}",
                        )
        snapshot = write_rcsb_scope_manifest(
            science_store,
            target,
            pdb_ids,
            summary_cache,
            scope_mode=mode,
        )
        snapshot["resumable_partial_count"] = len(resumable_partials)
        snapshot["obsolete_partial_count_removed"] = len(obsolete_partials)
        write_json_atomic(
            science_store / "downloads/rcsb/ligand_bound_pdb_scope.json",
            snapshot,
        )
        policy = write_rcsb_on_demand_policy(science_store, snapshot)
        if progress:
            progress(
                "completed",
                int(snapshot["ready_count"]),
                int(snapshot["scope_count"]),
                "RCSB 已切换为命中时补全，保留当前有效坐标缓存",
            )
        status = "ready" if int(snapshot["ready_count"]) > 0 else "requires_resource_bundle"
        return {
            "status": status,
            "bytes": snapshot["coordinate_bytes"],
            "sha256": snapshot["coordinate_digest"],
            "notes": (
                "partial_on_demand:"
                f"ready={snapshot['ready_count']};total={snapshot['scope_count']};"
                f"missing={snapshot['missing_count']};seeded={seeded};seed_failures={len(seed_failures)}"
                if status == "ready"
                else "on_demand_coordinate_cache_empty"
            ),
            "policy": str(policy),
            **snapshot,
        }
    (science_store / "downloads/rcsb/on_demand_policy.json").unlink(missing_ok=True)
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
    pool_workers = min(workers, total)
    pdb_id_iter = iter(sorted_ids)
    with concurrent.futures.ThreadPoolExecutor(max_workers=pool_workers) as pool:
        pending: set[concurrent.futures.Future[tuple[str, bool, str]]] = set()
        for _ in range(min(total, pool_workers * 2)):
            pending.add(pool.submit(sync_one, next(pdb_id_iter)))
        while pending:
            done, pending = concurrent.futures.wait(
                pending,
                return_when=concurrent.futures.FIRST_COMPLETED,
            )
            for future in done:
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
                        f"正在同步 RCSB 结构 {completed}/{total}，并行任务 {pool_workers}",
                    )
                    last_progress = now
                try:
                    pending.add(pool.submit(sync_one, next(pdb_id_iter)))
                except StopIteration:
                    pass
    snapshot = write_rcsb_scope_manifest(
        science_store,
        target,
        pdb_ids,
        summary_cache,
        scope_mode=mode,
    )
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


def rcsb_partial_download_inventory(target: Path) -> tuple[list[Path], list[Path]]:
    resumable: list[Path] = []
    obsolete: list[Path] = []
    if not target.is_dir():
        return resumable, obsolete
    for partial in sorted(target.rglob("*.part")):
        if partial.is_file() and partial_has_resumable_identity(partial):
            resumable.append(partial)
        elif partial.is_file():
            obsolete.append(partial)
    return resumable, obsolete


def rcsb_coordinate_validation(path: Path, expected_pdb_id: str = "") -> dict[str, object]:
    try:
        if not path.is_file() or path.stat().st_size <= 0:
            return {"status": "failed", "reason": "coordinate_missing_or_partial"}
        lowered_name = path.name.lower()
        opener = gzip.open if lowered_name.endswith((".gz", ".gz.part")) else open
        observed = ""
        entry_id = ""
        pending_entry_id = False
        has_atom_site = False
        has_x = False
        has_y = False
        has_z = False
        has_atoms = False
        uncompressed_bytes = 0
        with opener(path, "rt", encoding="utf-8", errors="strict") as handle:
            for line in handle:
                uncompressed_bytes += len(line.encode("utf-8"))
                if uncompressed_bytes > 2 * 1024 * 1024 * 1024:
                    return {"status": "failed", "reason": "mmcif_uncompressed_size_limit_exceeded"}
                stripped = line.strip()
                lowered = stripped.lower()
                if not observed:
                    match = re.fullmatch(r"data_([a-z0-9]{4})", lowered)
                    if match:
                        observed = match.group(1).upper()
                if pending_entry_id and stripped and not stripped.startswith("#"):
                    entry_id = stripped.strip("'\"").upper()
                    pending_entry_id = False
                if lowered == "_entry.id":
                    pending_entry_id = True
                elif lowered.startswith("_entry.id "):
                    entry_id = stripped.split(None, 1)[1].strip("'\"").upper()
                if lowered.startswith("_atom_site."):
                    has_atom_site = True
                if lowered == "_atom_site.cartn_x":
                    has_x = True
                elif lowered == "_atom_site.cartn_y":
                    has_y = True
                elif lowered == "_atom_site.cartn_z":
                    has_z = True
                if stripped.startswith(("ATOM ", "HETATM ")):
                    has_atoms = True
                if observed and has_atom_site and has_x and has_y and has_z and has_atoms:
                    break
        if not observed:
            return {"status": "failed", "reason": "mmcif_data_block_missing"}
        if expected_pdb_id and observed != expected_pdb_id.upper():
            return {"status": "failed", "reason": f"mmcif_id_mismatch:{observed}"}
        if expected_pdb_id and entry_id and entry_id != expected_pdb_id.upper():
            return {"status": "failed", "reason": f"mmcif_id_mismatch:{entry_id}"}
        if not (has_atom_site and has_x and has_y and has_z and has_atoms):
            return {"status": "failed", "reason": "atom_site_coordinates_missing"}
        return {
            "status": "ready",
            "reason": "mmcif_entry_and_atom_site_parse_passed",
            "pdb_id": observed,
        }
    except (EOFError, OSError, UnicodeError, gzip.BadGzipFile) as err:
        return {"status": "failed", "reason": f"mmcif_parse_failed:{type(err).__name__}"}


def valid_rcsb_coordinate(path: Path, expected_pdb_id: str = "") -> bool:
    return rcsb_coordinate_validation(path, expected_pdb_id).get("status") == "ready"


def write_rcsb_scope_manifest(
    science_store: Path,
    target: Path,
    pdb_ids: set[str],
    summary_cache: ResourceSummaryCache | None = None,
    *,
    scope_mode: str = "on-demand",
) -> dict[str, object]:
    mode = normalize_rcsb_mode(scope_mode)
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
    scope_digest = rcsb_scope_digest(pdb_ids)
    coordinate_digest = digest.hexdigest() if ready_count else ""
    manifest = science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv"
    write_simple_tsv(
        manifest,
        ["pdb_id", "status", "coordinate_rel_path", "size_bytes", "sha256"],
        rows,
    )
    snapshot = {
        "schema_version": "genoligand_rcsb_scope_snapshot_v1",
        "scope_mode": "on_demand" if mode == "on-demand" else "complete",
        "coverage": (
            "complete"
            if ready_count == len(rows)
            else "partial_on_demand"
            if mode == "on-demand"
            else "incomplete_complete"
        ),
        "refresh_needed": False,
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


def rcsb_scope_digest(pdb_ids: set[str]) -> str:
    return hashlib.sha256("\n".join(sorted(pdb_ids)).encode("ascii")).hexdigest()


def rcsb_scope_snapshot_matches_current_sources(
    science_store: Path,
    snapshot: dict[str, object],
) -> bool:
    current_scope = ligand_bound_pdb_scope(science_store)
    manifest_rows = read_tsv(science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv")
    manifest_scope = {
        row.get("pdb_id", "").upper()
        for row in manifest_rows
        if row.get("pdb_id", "")
    }
    try:
        snapshot_count = int(snapshot.get("scope_count", 0) or 0)
    except (TypeError, ValueError):
        return False
    if (
        not current_scope
        or snapshot.get("schema_version") != "genoligand_rcsb_scope_snapshot_v1"
        or snapshot_count != len(manifest_rows)
        or len(manifest_scope) != snapshot_count
        or snapshot.get("scope_digest") != rcsb_scope_digest(manifest_scope)
    ):
        return False
    snapshot_mode = str(snapshot.get("scope_mode", "")).replace("_", "-")
    return manifest_scope.issubset(current_scope) if snapshot_mode == "on-demand" else manifest_scope == current_scope


def normalize_rcsb_mode(value: str) -> str:
    normalized = str(value).strip().lower().replace("_", "-")
    if normalized not in {"on-demand", "complete"}:
        raise ValueError(f"unsupported_rcsb_mode:{value}")
    return normalized


def write_rcsb_on_demand_policy(
    science_store: Path,
    snapshot: dict[str, object],
) -> Path:
    policy = science_store / "downloads/rcsb/on_demand_policy.json"
    write_json_atomic(
        policy,
        {
            "schema_version": "genoligand_rcsb_on_demand_policy_v1",
            "status": "ready",
            "mode": "on_demand",
            "official_base_url": "https://files.rcsb.org/download",
            "coordinate_template": "https://files.rcsb.org/download/{pdb_id}.cif.gz",
            "cache_root": "downloads/rcsb/mmcif",
            "max_fetches_per_run": 12,
            "max_cache_bytes": RCSB_ON_DEMAND_MAX_CACHE_BYTES,
            "max_cache_files": RCSB_ON_DEMAND_MAX_CACHE_FILES,
            "validation": "gzip_mmcif_entry_and_atom_site",
            "cached_ready_count": int(snapshot.get("ready_count", 0) or 0),
            "scope_count": int(snapshot.get("scope_count", 0) or 0),
        },
    )
    return policy


def rcsb_scope_snapshot_validation(
    science_store: Path,
    *,
    expected_mode: str | None = None,
    execution: dict[str, str] | None = None,
    source_summary: dict[str, object] | None = None,
    check_current_scope: bool = False,
) -> dict[str, object]:
    def failed(reason: str) -> dict[str, object]:
        return {"status": "failed", "reason": reason}

    snapshot = read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
    rows = read_tsv(science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv")
    mode = str(snapshot.get("scope_mode", "")).replace("_", "-")
    if expected_mode is not None and mode != normalize_rcsb_mode(expected_mode):
        return failed("rcsb_scope_mode_mismatch")
    if mode not in {"on-demand", "complete"}:
        return failed("rcsb_scope_mode_invalid")
    if snapshot.get("schema_version") != "genoligand_rcsb_scope_snapshot_v1":
        return failed("rcsb_scope_schema_invalid")
    try:
        scope_count = int(snapshot.get("scope_count", 0) or 0)
        ready_count = int(snapshot.get("ready_count", 0) or 0)
        missing_count = int(snapshot.get("missing_count", 0) or 0)
        coordinate_files = int(snapshot.get("coordinate_files", 0) or 0)
        coordinate_bytes = int(snapshot.get("coordinate_bytes", 0) or 0)
    except (TypeError, ValueError):
        return failed("rcsb_scope_counts_invalid")
    if (
        scope_count <= 0
        or ready_count <= 0
        or missing_count != scope_count - ready_count
        or coordinate_files != ready_count
        or coordinate_bytes <= 0
        or len(rows) != scope_count
    ):
        return failed("rcsb_scope_counts_invalid")
    if mode == "complete" and (ready_count != scope_count or missing_count != 0):
        return failed("rcsb_complete_scope_incomplete")

    manifest_scope: set[str] = set()
    ready_rows: list[tuple[str, str, int, str]] = []
    cache_root = (science_store / "downloads/rcsb/mmcif").resolve()
    for row in rows:
        pdb_id = row.get("pdb_id", "").upper()
        if not valid_pdb_id(pdb_id) or pdb_id in manifest_scope:
            return failed("rcsb_scope_ids_invalid")
        manifest_scope.add(pdb_id)
        status = row.get("status", "")
        coordinate_rel = row.get("coordinate_rel_path", "")
        sha256 = row.get("sha256", "").lower()
        try:
            size = int(row.get("size_bytes", "0") or 0)
        except ValueError:
            return failed("rcsb_scope_coordinate_size_invalid")
        if status == "missing":
            if coordinate_rel or size != 0 or sha256:
                return failed("rcsb_scope_missing_row_invalid")
            continue
        if status != "ready" or size <= 0 or not re.fullmatch(r"[0-9a-f]{64}", sha256):
            return failed("rcsb_scope_ready_row_invalid")
        coordinate = resolve_manifest_path(science_store, coordinate_rel)
        if coordinate is None:
            return failed("rcsb_scope_coordinate_path_missing")
        try:
            cache_rel = coordinate.resolve().relative_to(cache_root).as_posix()
        except ValueError:
            return failed("rcsb_scope_coordinate_path_outside_cache")
        if not cache_rel.lower().endswith(".cif.gz"):
            return failed("rcsb_scope_coordinate_extension_invalid")
        ready_rows.append((pdb_id, cache_rel, size, sha256))
    if len(ready_rows) != ready_count or snapshot.get("scope_digest") != rcsb_scope_digest(manifest_scope):
        return failed("rcsb_scope_manifest_binding_invalid")

    coordinate_digest = hashlib.sha256()
    for _pdb_id, cache_rel, size, sha256 in sorted(ready_rows):
        coordinate_digest.update(cache_rel.encode("utf-8"))
        coordinate_digest.update(str(size).encode("ascii"))
        coordinate_digest.update(sha256.encode("ascii"))
    coordinate_sha256 = coordinate_digest.hexdigest()
    if snapshot.get("coordinate_digest") != coordinate_sha256:
        return failed("rcsb_scope_coordinate_digest_invalid")

    source_digest = hashlib.sha256()
    for _pdb_id, cache_rel, size, sha256 in sorted(ready_rows, key=lambda item: item[1]):
        source_digest.update(cache_rel.encode("utf-8"))
        source_digest.update(str(size).encode("ascii"))
        source_digest.update(sha256.encode("ascii"))
    source_sha256 = source_digest.hexdigest()

    current_scope_count = scope_count
    scope_changed = False
    if check_current_scope:
        current_scope = ligand_bound_pdb_scope(science_store)
        if not current_scope or (mode == "complete" and manifest_scope != current_scope):
            return failed("rcsb_scope_snapshot_stale")
        current_scope_count = len(current_scope)
        scope_changed = manifest_scope != current_scope

    if mode == "on-demand":
        policy = read_json_object(science_store / "downloads/rcsb/on_demand_policy.json")
        if not (
            policy.get("schema_version") == "genoligand_rcsb_on_demand_policy_v1"
            and policy.get("status") == "ready"
            and policy.get("mode") == "on_demand"
            and policy.get("official_base_url") == "https://files.rcsb.org/download"
            and policy.get("coordinate_template") == "https://files.rcsb.org/download/{pdb_id}.cif.gz"
            and policy.get("cache_root") == "downloads/rcsb/mmcif"
            and policy.get("max_fetches_per_run") == 12
            and policy.get("max_cache_bytes") == RCSB_ON_DEMAND_MAX_CACHE_BYTES
            and policy.get("max_cache_files") == RCSB_ON_DEMAND_MAX_CACHE_FILES
            and policy.get("validation") == "gzip_mmcif_entry_and_atom_site"
            and policy.get("cached_ready_count") == ready_count
            and policy.get("scope_count") == scope_count
        ):
            return failed("rcsb_on_demand_policy_invalid")

    if execution is not None:
        serialized_mode = "on_demand" if mode == "on-demand" else "complete"
        if not (
            execution.get("status") == "ready"
            and execution.get("scope_mode") == serialized_mode
            and execution.get("scope_digest") == snapshot.get("scope_digest")
            and execution.get("scope_ready_count") == str(ready_count)
            and execution.get("scope_count") == str(scope_count)
            and execution.get("scope_missing_count") == str(missing_count)
            and execution.get("bytes") == str(coordinate_bytes)
            and execution.get("sha256", "").lower() == coordinate_sha256
        ):
            return failed("rcsb_scope_execution_binding_invalid")

    if source_summary is not None:
        try:
            source_files = int(source_summary.get("file_count", 0) or 0)
            source_bytes = int(source_summary.get("bytes", 0) or 0)
        except (TypeError, ValueError):
            return failed("rcsb_scope_source_summary_invalid")
        if not (
            source_files == ready_count
            and source_bytes == coordinate_bytes
            and str(source_summary.get("sha256", "")).lower() == source_sha256
        ):
            return failed("rcsb_scope_source_summary_invalid")

    return {
        "status": "ready_partial_on_demand" if mode == "on-demand" else "ready_complete",
        "reason": "rcsb_scope_policy_coordinate_snapshot_validated",
        "scope_count": scope_count,
        "ready_count": ready_count,
        "missing_count": missing_count,
        "current_scope_count": current_scope_count,
        "coordinate_bytes": coordinate_bytes,
        "coordinate_digest": coordinate_sha256,
        "source_digest": source_sha256,
        "refresh_needed": scope_changed,
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
    summary_cache: ResourceSummaryCache | None = None,
    *,
    rcsb_mode: str = "on-demand",
) -> dict[str, object]:
    method = (row.get("sync_method") or "direct").strip().lower()
    if method == "html_links":
        return sync_html_links(row, target, progress, summary_cache, science_store)
    if method == "html_tree":
        return sync_html_tree(row, target, summary_cache)
    if method == "gcs_prefix":
        return sync_gcs_prefix(row, target, progress, summary_cache)
    if method == "rcsb_ligand_bound_scope":
        return sync_rcsb_ligand_bound_scope(
            row,
            target,
            science_store,
            progress,
            summary_cache,
            rcsb_mode=rcsb_mode,
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


def verified_external_retention(
    science_store: Path,
    plan_row: dict[str, str],
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, str]:
    source_id = plan_row.get("source_id", "")
    matches = [
        row
        for row in source_retention_rows(science_store)
        if row.get("source_id") == source_id
    ]
    if len(matches) != 1:
        return {}
    retained = matches[0]
    plan_identity_ready = bool(
        retained.get("source_local_path", "").replace("\\", "/")
        == plan_row.get("target_rel_path", "").replace("\\", "/")
        and retained.get("release_identity") == plan_row.get("release_or_date")
        and retained.get("trust_source") == plan_row.get("trust_source")
    )
    if not plan_identity_ready:
        return {}
    if retention_source_identity_and_derivatives_ready(
        science_store,
        retained,
        summary_cache,
    ):
        return retained
    return legacy_pruned_structure_retention_candidates(
        science_store,
        summary_cache,
    ).get(source_id, {})


def external_plan_row_ready(
    row: dict[str, str],
    target: Path | None,
    marker: Path | None,
    science_store: Path,
    execution: dict[str, str],
    summary_cache: ResourceSummaryCache | None = None,
    *,
    rcsb_mode: str = "on-demand",
) -> bool:
    method = (row.get("sync_method") or "direct").strip().lower()
    rcsb_scope_source = method == "rcsb_ligand_bound_scope"
    if (
        not rcsb_scope_source
        and verified_external_retention(science_store, row, summary_cache)
    ):
        return True
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
    retained = retained_source_ready(
        science_store,
        row.get("source_id", ""),
        row.get("target_rel_path", ""),
        summary_cache,
        expected_release_identity=row.get("release_or_date", ""),
        expected_trust_source=row.get("trust_source", ""),
    )
    if retained and not rcsb_scope_source:
        return True
    if not marker or not path_has_payload(marker):
        return False
    if rcsb_scope_source:
        mode = normalize_rcsb_mode(rcsb_mode)
        coordinate_summary = (
            resource_payload_summary(target, cache=summary_cache)
            if target is not None
            else {"file_count": 0, "bytes": 0, "sha256": ""}
        )
        validation = rcsb_scope_snapshot_validation(
            science_store,
            expected_mode=mode,
            execution=execution,
            source_summary=coordinate_summary,
        )
        return validation.get("status") == (
            "ready_partial_on_demand" if mode == "on-demand" else "ready_complete"
        )
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
    *,
    rcsb_mode: str = "on-demand",
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
                "scope_mode",
                "scope_digest",
                "scope_ready_count",
                "scope_count",
                "scope_missing_count",
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
                row,
                target,
                marker,
                science_store,
                previous,
                summary_cache,
                rcsb_mode=rcsb_mode,
            )
            if marker_ready:
                retained = retained_source_ready(science_store, source_id, target_rel, summary_cache)
                if not retained:
                    retained = verified_external_retention(science_store, row, summary_cache)
                evidence = external_evidence_path(row, target, marker)
                evidence_bytes = (
                    int(retained.get("source_bytes", "0") or 0)
                    if retained
                    else directory_size(evidence) if evidence else 0
                )
                evidence_sha256 = (
                    retained.get("execution_sha256", "")
                    if retained
                    else external_evidence_digest(row, evidence, summary_cache) if evidence else ""
                )
                snapshot = (
                    read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
                    if method == "rcsb_ligand_bound_scope"
                    else {}
                )
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
                        "scope_mode": str(snapshot.get("scope_mode", "")),
                        "scope_digest": str(snapshot.get("scope_digest", "")),
                        "scope_ready_count": str(snapshot.get("ready_count", "")),
                        "scope_count": str(snapshot.get("scope_count", "")),
                        "scope_missing_count": str(snapshot.get("missing_count", "")),
                        "notes": (
                            "verified_index_reused_source_payload_remains_pruned"
                            if retained
                            else
                            "partial_on_demand_previous_success_digest_matched"
                            if snapshot.get("scope_mode") == "on_demand"
                            else "previous_success_digest_matched"
                        ),
                    }
                )
                continue
            if skip_downloads and not (
                method == "rcsb_ligand_bound_scope"
                and normalize_rcsb_mode(rcsb_mode) == "on-demand"
            ):
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
                    rcsb_mode=rcsb_mode,
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
                        "scope_mode": str(result.get("scope_mode", "")),
                        "scope_digest": str(result.get("scope_digest", "")),
                        "scope_ready_count": str(result.get("ready_count", "")),
                        "scope_count": str(result.get("scope_count", "")),
                        "scope_missing_count": str(result.get("missing_count", "")),
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
    *,
    scratch_dir: Path | None = None,
    rcsb_mode: str = "on-demand",
) -> dict[str, object]:
    mmseqs_rows = build_mmseqs_target_databases(science_store, env, wsl_distro, summary_cache)
    foldseek_rows = build_foldseek_target_database(
        science_store,
        env,
        wsl_distro,
        summary_cache,
        scratch_dir=scratch_dir,
        rcsb_mode=rcsb_mode,
    )
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
    loader_path = (
        wsl_path(path)
        if Path(python_command).name.casefold() == "genoligand-wsl-python.cmd"
        else str(path)
    )
    completed = run_command([python_command, "-c", code, loader_path])
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
        recovery = reconcile_database_directory_publish(
            db_path,
            lambda prefix: database_prefix_ready("mmseqs", prefix, command, env, wsl_distro),
        )
        migrated = False
        if recovery.get("status") != "ready":
            status = "failed"
            notes = str(recovery.get("reason", "database_directory_recovery_needs_repair"))
        elif alignment_source_deferred(source):
            status, notes = "deferred_large_source", "source_exceeds_sync_index_threshold"
        else:
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
    current_ids = {row.get("source_id", "") for row in rows}
    for source_id, previous in previous_rows.items():
        if source_id in current_ids or not retention_covers_alignment_source_path(
            science_store, previous.get("source_path", "")
        ):
            continue
        db_path = resolve_manifest_path(science_store, previous.get("db_path", ""))
        if not (
            previous.get("status") == "ready"
            and previous.get("manifest_schema_version") == ALIGNMENT_MANIFEST_SCHEMA_VERSION
            and previous.get("builder_version") == builder_version
            and db_path is not None
            and database_prefix_ready("mmseqs", db_path, command, env, wsl_distro)
        ):
            continue
        carried = dict(previous)
        carried["notes"] = "verified_index_reused_after_source_payload_pruned"
        rows.append(carried)
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


def rcsb_scope_rows_for_ids(science_store: Path, pdb_ids: set[str]) -> dict[str, dict[str, str]]:
    selected: dict[str, dict[str, str]] = {}
    manifest = science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv"
    if not manifest.is_file():
        return selected
    with manifest.open("r", encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            normalized = {str(key): str(value or "").strip() for key, value in row.items()}
            pdb_id = normalized.get("pdb_id", "").upper()
            if pdb_id in pdb_ids:
                selected[pdb_id] = normalized
    return selected


def foldseek_mapping_pdb_id(value: str, row_kind: str, line_number: int) -> str:
    name = Path(value.strip().replace("\\", "/")).name
    match = re.match(r"^([0-9][A-Za-z0-9]{3})(?=[_.-]|$)", name)
    if not match or not valid_pdb_id(match.group(1)):
        raise ValueError(f"foldseek_{row_kind}_pdb_id_invalid:line={line_number}")
    return match.group(1).upper()


def foldseek_database_mapping_summary(db_path: Path) -> dict[str, object]:
    source_path = Path(f"{db_path}.source")
    index_path = Path(f"{db_path}.index")
    lookup_path = Path(f"{db_path}.lookup")
    digest = hashlib.sha256()
    sources: dict[int, tuple[str, str]] = {}
    previous_key = -1
    with source_path.open("r", encoding="utf-8", errors="strict") as handle:
        for line_number, line in enumerate(handle, start=1):
            fields = line.rstrip("\r\n").split("\t")
            if len(fields) < 2:
                raise ValueError(f"foldseek_source_row_incomplete:line={line_number}")
            source_key = int(fields[0])
            if source_key < 0 or source_key <= previous_key:
                raise ValueError(f"foldseek_source_key_order_invalid:line={line_number}")
            previous_key = source_key
            identity = fields[1].strip()
            sources[source_key] = (
                foldseek_mapping_pdb_id(identity, "source", line_number),
                identity,
            )
            digest.update(
                (json.dumps(["source", *fields], ensure_ascii=True, separators=(",", ":")) + "\n").encode(
                    "ascii"
                )
            )
    if not sources:
        raise ValueError("foldseek_source_rows_empty")

    lookup_keys: list[int] = []
    referenced_sources: set[int] = set()
    previous_key = -1
    with lookup_path.open("r", encoding="utf-8", errors="strict") as handle:
        for line_number, line in enumerate(handle, start=1):
            fields = line.rstrip("\r\n").split("\t")
            if len(fields) < 3:
                raise ValueError(f"foldseek_lookup_row_incomplete:line={line_number}")
            db_key = int(fields[0])
            source_key = int(fields[2])
            if db_key < 0 or db_key <= previous_key:
                raise ValueError(f"foldseek_lookup_key_order_invalid:line={line_number}")
            previous_key = db_key
            source = sources.get(source_key)
            if source is None:
                raise ValueError(f"foldseek_lookup_source_key_missing:line={line_number}")
            lookup_pdb_id = foldseek_mapping_pdb_id(fields[1], "lookup", line_number)
            if lookup_pdb_id != source[0]:
                raise ValueError(f"foldseek_lookup_source_identity_mismatch:line={line_number}")
            lookup_keys.append(db_key)
            referenced_sources.add(source_key)
            digest.update(
                (json.dumps(["lookup", *fields], ensure_ascii=True, separators=(",", ":")) + "\n").encode(
                    "ascii"
                )
            )
    if not lookup_keys:
        raise ValueError("foldseek_lookup_rows_empty")
    unreferenced = sorted(set(sources) - referenced_sources)
    if unreferenced:
        raise ValueError(f"foldseek_source_unreferenced:key={unreferenced[0]}")

    index_keys: list[int] = []
    previous_key = -1
    with index_path.open("r", encoding="ascii", errors="strict") as handle:
        for line_number, line in enumerate(handle, start=1):
            fields = line.rstrip("\r\n").split("\t")
            if len(fields) < 3:
                raise ValueError(f"foldseek_index_row_incomplete:line={line_number}")
            db_key = int(fields[0])
            offset = int(fields[1])
            length = int(fields[2])
            if db_key < 0 or db_key <= previous_key:
                raise ValueError(f"foldseek_index_key_order_invalid:line={line_number}")
            if offset < 0 or length <= 0:
                raise ValueError(f"foldseek_index_bounds_invalid:line={line_number}")
            previous_key = db_key
            index_keys.append(db_key)
            digest.update(
                (json.dumps(["index", *fields], ensure_ascii=True, separators=(",", ":")) + "\n").encode(
                    "ascii"
                )
            )
    if index_keys != lookup_keys:
        raise ValueError("foldseek_index_lookup_key_mismatch")
    return {
        "indexed_entries": len(index_keys),
        "mapping_sha256": digest.hexdigest(),
        "pdb_ids": {source[0] for source in sources.values()},
        "source_count": len(sources),
    }


def foldseek_database_entry_count(db_path: Path) -> int:
    return int(foldseek_database_mapping_summary(db_path)["indexed_entries"])


def foldseek_subset_digest(
    listing_sha256: str,
    validated_ids_sha256: str,
    mapping_sha256: str,
    indexed_entries: int,
    indexed_pdb_count: int,
    coordinate_bytes: int,
) -> str:
    return hashlib.sha256(
        (
            f"{listing_sha256}\n{validated_ids_sha256}\n{mapping_sha256}\n"
            f"{indexed_entries}\n{indexed_pdb_count}\n{coordinate_bytes}\n"
        ).encode("ascii")
    ).hexdigest()


def foldseek_partial_manifest_validation(
    science_store: Path,
    row: dict[str, str],
    db_path: Path,
    command: str | None = None,
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    *,
    probe_database: bool = True,
) -> dict[str, object]:
    if (
        row.get("manifest_schema_version") != ALIGNMENT_MANIFEST_SCHEMA_VERSION
        or row.get("scope_status") != "ready_partial_on_demand"
        or row.get("scope_mode") != "on_demand"
    ):
        return {"status": "failed", "reason": "foldseek_partial_scope_mode_invalid"}
    snapshot = read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
    scope_validation = rcsb_scope_snapshot_validation(science_store, expected_mode="on-demand")
    if scope_validation.get("status") != "ready_partial_on_demand":
        return {
            "status": "failed",
            "reason": f"foldseek_partial_scope_snapshot_invalid:{scope_validation.get('reason', 'unknown')}",
        }
    listing = Path(f"{db_path}.source")
    validated_manifest = resolve_manifest_path(science_store, row.get("indexed_validated_ids_manifest", ""))
    if not listing.is_file() or validated_manifest is None or not validated_manifest.is_file():
        return {"status": "failed", "reason": "foldseek_partial_evidence_missing"}
    try:
        listing_sha256 = sha256_file(listing)
        validated_ids_sha256 = sha256_file(validated_manifest)
        if listing_sha256 != row.get("indexed_source_listing_sha256", ""):
            return {"status": "failed", "reason": "foldseek_partial_source_listing_digest_mismatch"}
        if validated_ids_sha256 != row.get("indexed_validated_ids_sha256", ""):
            return {"status": "failed", "reason": "foldseek_partial_validated_ids_digest_mismatch"}
        mapping = foldseek_database_mapping_summary(db_path)
        mapping_sha256 = str(mapping["mapping_sha256"])
        if mapping_sha256 != row.get("indexed_mapping_sha256", ""):
            return {"status": "failed", "reason": "foldseek_partial_mapping_digest_mismatch"}
        indexed_rows = read_tsv(validated_manifest)
        indexed_by_id: dict[str, dict[str, str]] = {}
        coordinate_bytes = 0
        for indexed in indexed_rows:
            pdb_id = indexed.get("pdb_id", "").upper()
            if (
                not valid_pdb_id(pdb_id)
                or pdb_id in indexed_by_id
                or indexed.get("status") != "validated"
                or not re.fullmatch(r"[0-9a-f]{64}", indexed.get("sha256", "").lower())
            ):
                return {"status": "failed", "reason": "foldseek_partial_validated_ids_invalid"}
            size = int(indexed.get("size_bytes", "0"))
            if size <= 0:
                return {"status": "failed", "reason": "foldseek_partial_coordinate_size_invalid"}
            indexed_by_id[pdb_id] = indexed
            coordinate_bytes += size
        if not indexed_by_id:
            return {"status": "failed", "reason": "foldseek_partial_validated_ids_empty"}
        if set(mapping["pdb_ids"]) != set(indexed_by_id):
            return {"status": "failed", "reason": "foldseek_partial_source_validated_ids_mismatch"}
        scope_rows = rcsb_scope_rows_for_ids(science_store, set(indexed_by_id))
        retired_indexed_ids = set(indexed_by_id) - set(scope_rows)
        indexed_entries = int(mapping["indexed_entries"])
        indexed_pdb_count = len(indexed_by_id)
        subset_digest = foldseek_subset_digest(
            listing_sha256,
            validated_ids_sha256,
            mapping_sha256,
            indexed_entries,
            indexed_pdb_count,
            coordinate_bytes,
        )
        if (
            str(snapshot.get("scope_mode", "")).replace("_", "-") != "on-demand"
            or row.get("scope_digest") != str(snapshot.get("scope_digest", ""))
            or row.get("scope_coordinate_digest") != str(snapshot.get("coordinate_digest", ""))
            or row.get("scope_count") != str(snapshot.get("scope_count", ""))
            or row.get("scope_ready_count") != str(snapshot.get("ready_count", ""))
            or row.get("scope_missing_count") != str(snapshot.get("missing_count", ""))
            or row.get("source_digest") != subset_digest
            or row.get("indexed_subset_digest") != subset_digest
            or row.get("source_files") != str(indexed_pdb_count)
            or row.get("source_bytes") != str(coordinate_bytes)
            or row.get("indexed_pdb_count") != str(indexed_pdb_count)
            or row.get("indexed_entries") != str(indexed_entries)
        ):
            return {"status": "failed", "reason": "foldseek_partial_manifest_binding_mismatch"}
        if probe_database and not database_prefix_ready("foldseek", db_path, command, env, wsl_distro):
            return {"status": "failed", "reason": "foldseek_partial_database_smoke_failed"}
        cached_ids = {
            row.get("pdb_id", "").upper()
            for row in read_tsv(science_store / "downloads/rcsb/ligand_bound_pdb_scope.tsv")
            if row.get("status") == "ready"
        }
        return {
            "status": "ready",
            "reason": "foldseek_partial_manifest_and_database_validated",
            "indexed_source_listing_sha256": listing_sha256,
            "indexed_validated_ids_manifest": relative_path(science_store, validated_manifest),
            "indexed_validated_ids_sha256": validated_ids_sha256,
            "indexed_mapping_sha256": mapping_sha256,
            "indexed_subset_digest": subset_digest,
            "indexed_pdb_count": indexed_pdb_count,
            "indexed_entries": indexed_entries,
            "indexed_coordinate_bytes": coordinate_bytes,
            "refresh_needed": bool(
                (cached_ids - set(indexed_by_id)) or retired_indexed_ids
            ),
        }
    except (OSError, UnicodeError, ValueError) as err:
        return {"status": "failed", "reason": f"foldseek_partial_validation_failed:{exception_note(err)}"}


def foldseek_legacy_partial_migration(
    science_store: Path,
    db_path: Path,
    command: str | None,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    scope_snapshot: dict[str, object],
    validated_manifest: Path,
) -> dict[str, object]:
    listing = Path(f"{db_path}.source")
    scope_validation = rcsb_scope_snapshot_validation(science_store, expected_mode="on-demand")
    if scope_validation.get("status") != "ready_partial_on_demand":
        return {
            "status": "failed",
            "reason": f"on_demand_scope_snapshot_invalid:{scope_validation.get('reason', 'unknown')}",
        }
    try:
        cached_ready_count = int(scope_snapshot.get("ready_count", 0) or 0)
    except (TypeError, ValueError):
        cached_ready_count = 0
    if str(scope_snapshot.get("scope_mode", "")).replace("_", "-") != "on-demand" or cached_ready_count <= 0:
        return {"status": "failed", "reason": "on_demand_scope_snapshot_missing"}
    if not listing.is_file() or listing.stat().st_size <= 0:
        return {"status": "failed", "reason": "legacy_foldseek_source_listing_missing"}
    try:
        mapping = foldseek_database_mapping_summary(db_path)
    except (OSError, UnicodeError, ValueError) as err:
        return {"status": "failed", "reason": f"legacy_foldseek_database_mapping_invalid:{exception_note(err)}"}
    pdb_ids = set(mapping["pdb_ids"])
    if not pdb_ids:
        return {"status": "failed", "reason": "legacy_foldseek_source_listing_empty"}
    scope_rows = rcsb_scope_rows_for_ids(science_store, pdb_ids)
    if set(scope_rows) != pdb_ids:
        return {"status": "failed", "reason": "legacy_foldseek_indexed_ids_outside_scope"}

    rows: list[dict[str, str]] = []
    coordinate_bytes = 0
    for pdb_id in sorted(pdb_ids):
        scope_row = scope_rows[pdb_id]
        coordinate_rel_path = scope_row.get("coordinate_rel_path", "")
        coordinate = science_store / coordinate_rel_path
        try:
            size = int(scope_row.get("size_bytes", "0"))
        except ValueError:
            size = 0
        coordinate_sha256 = scope_row.get("sha256", "").lower()
        actual_sha256 = sha256_file(coordinate) if coordinate.is_file() else ""
        if (
            scope_row.get("status") != "ready"
            or not coordinate_rel_path
            or size <= 0
            or not re.fullmatch(r"[0-9a-f]{64}", coordinate_sha256)
            or not coordinate.is_file()
            or coordinate.stat().st_size != size
            or actual_sha256 != coordinate_sha256
        ):
            return {
                "status": "failed",
                "reason": f"legacy_foldseek_indexed_coordinate_invalid:{pdb_id}",
            }
        coordinate_bytes += size
        rows.append(
            {
                "pdb_id": pdb_id,
                "coordinate_rel_path": coordinate_rel_path,
                "size_bytes": str(size),
                "sha256": actual_sha256,
                "status": "validated",
            }
        )
    if not database_prefix_ready("foldseek", db_path, command, env, wsl_distro):
        return {"status": "failed", "reason": "legacy_foldseek_database_smoke_failed"}
    indexed_entries = int(mapping["indexed_entries"])
    mapping_sha256 = str(mapping["mapping_sha256"])

    write_simple_tsv(
        validated_manifest,
        ["pdb_id", "coordinate_rel_path", "size_bytes", "sha256", "status"],
        rows,
    )
    listing_sha256 = sha256_file(listing)
    validated_ids_sha256 = sha256_file(validated_manifest)
    subset_digest = foldseek_subset_digest(
        listing_sha256,
        validated_ids_sha256,
        mapping_sha256,
        indexed_entries,
        len(pdb_ids),
        coordinate_bytes,
    )
    return {
        "status": "ready",
        "reason": "legacy_foldseek_partial_subset_validated",
        "indexed_source_listing_sha256": listing_sha256,
        "indexed_validated_ids_manifest": relative_path(science_store, validated_manifest),
        "indexed_validated_ids_sha256": validated_ids_sha256,
        "indexed_mapping_sha256": mapping_sha256,
        "indexed_subset_digest": subset_digest,
        "indexed_pdb_count": len(pdb_ids),
        "indexed_entries": indexed_entries,
        "indexed_coordinate_bytes": coordinate_bytes,
        "refresh_needed": cached_ready_count > len(pdb_ids),
    }


def build_foldseek_target_database(
    science_store: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
    summary_cache: ResourceSummaryCache | None = None,
    *,
    scratch_dir: Path | None = None,
    rcsb_mode: str = "on-demand",
) -> list[dict[str, str]]:
    mode = normalize_rcsb_mode(rcsb_mode)
    index_root = science_store / "indexes" / "foldseek"
    manifest = index_root / "foldseek_target_databases.tsv"
    previous_rows = {row.get("source_id", ""): row for row in read_tsv(manifest)}
    index_root.mkdir(parents=True, exist_ok=True)
    command = shutil.which("foldseek", path=(env or os.environ).get("PATH"))
    builder_version = database_builder_version("foldseek", command, env, wsl_distro)
    source = science_store / "downloads" / "rcsb" / "mmcif"
    db_path = index_root / "rcsb_mmcif" / "target"
    validated_subset_manifest = index_root / "rcsb_mmcif/indexed_pdb_ids.tsv"
    scope_snapshot = read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
    previous = previous_rows.get("rcsb_mmcif", {})
    recovery = reconcile_database_directory_publish(
        db_path,
        lambda prefix: database_prefix_ready("foldseek", prefix, command, env, wsl_distro),
    )
    partial = {"status": "not_applicable", "reason": "partial_migration_not_selected"}
    if recovery.get("status") != "ready":
        partial = {
            "status": "recovery_failed",
            "reason": str(recovery.get("reason", "database_directory_recovery_needs_repair")),
        }
    elif mode == "on-demand" and database_artifacts_present(db_path):
        if previous.get("scope_status") == "ready_partial_on_demand":
            partial = foldseek_partial_manifest_validation(
                science_store,
                previous,
                db_path,
                command,
                env,
                wsl_distro,
            )
        if partial.get("status") != "ready":
            partial = foldseek_legacy_partial_migration(
                science_store,
                db_path,
                command,
                env,
                wsl_distro,
                scope_snapshot,
                validated_subset_manifest,
            )
    if partial.get("status") == "ready":
        source_fields = {
            "manifest_schema_version": ALIGNMENT_MANIFEST_SCHEMA_VERSION,
            "source_digest": str(partial["indexed_subset_digest"]),
            "source_files": str(partial["indexed_pdb_count"]),
            "source_bytes": str(partial["indexed_coordinate_bytes"]),
            "builder_version": builder_version,
        }
        status = "ready"
        notes = (
            "ready_partial_on_demand:manifest_database_probe_reused"
            if previous.get("indexed_subset_digest") == partial.get("indexed_subset_digest")
            else "ready_partial_on_demand:legacy_database_probe_and_subset_validation_passed"
        )
    elif partial.get("status") == "recovery_failed":
        source_fields = {
            "manifest_schema_version": ALIGNMENT_MANIFEST_SCHEMA_VERSION,
            "source_digest": previous.get("source_digest", ""),
            "source_files": previous.get("source_files", ""),
            "source_bytes": previous.get("source_bytes", ""),
            "builder_version": builder_version,
        }
        status = "failed"
        notes = str(partial["reason"])
        partial = {
            "indexed_source_listing_sha256": previous.get("indexed_source_listing_sha256", ""),
            "indexed_validated_ids_manifest": previous.get("indexed_validated_ids_manifest", ""),
            "indexed_validated_ids_sha256": previous.get("indexed_validated_ids_sha256", ""),
            "indexed_mapping_sha256": previous.get("indexed_mapping_sha256", ""),
            "indexed_subset_digest": previous.get("indexed_subset_digest", ""),
            "indexed_pdb_count": previous.get("indexed_pdb_count", ""),
            "indexed_entries": previous.get("indexed_entries", ""),
            "indexed_coordinate_bytes": previous.get("source_bytes", ""),
            "refresh_needed": True,
        }
    else:
        validated_subset_manifest.unlink(missing_ok=True)
        source_summary = resource_payload_summary(source, cache=summary_cache)
        source_fields = alignment_source_manifest_fields(source_summary, builder_version)
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
        scope_validation = rcsb_scope_snapshot_validation(
            science_store,
            expected_mode=mode,
            source_summary=source_summary,
            check_current_scope=True,
        )
        scope_usable = scope_validation.get("status") == (
            "ready_partial_on_demand" if mode == "on-demand" else "ready_complete"
        )
        if not scope_usable:
            status, notes = "failed", "rcsb_scope_snapshot_unusable_or_digest_mismatch"
        else:
            status, notes = build_foldseek_database(
                command,
                source,
                db_path,
                env,
                wsl_distro,
                force_rebuild=force_rebuild,
                scratch_dir=scratch_dir,
            )
            if status == "ready" and migrated:
                notes = "legacy_manifest_migrated_after_database_smoke"
        indexed_entries = source_fields["source_files"]
        if status == "ready":
            try:
                indexed_entries = str(foldseek_database_entry_count(db_path))
            except (OSError, UnicodeError, ValueError) as err:
                status = "failed"
                notes = f"foldseek_database_index_invalid:{exception_note(err)}"
        partial = {
            "indexed_source_listing_sha256": "",
            "indexed_validated_ids_manifest": "",
            "indexed_validated_ids_sha256": "",
            "indexed_mapping_sha256": "",
            "indexed_subset_digest": source_fields["source_digest"],
            "indexed_pdb_count": source_fields["source_files"],
            "indexed_entries": indexed_entries,
            "indexed_coordinate_bytes": source_fields["source_bytes"],
            "refresh_needed": False,
        }
    rows = [
        {
            "source_id": "rcsb_mmcif",
            "source_namespace": "RCSB",
            "source_class": "structure",
            "source_path": relative_path(science_store, source),
            **source_fields,
            "scope_digest": str(scope_snapshot.get("scope_digest", "")),
            "scope_coordinate_digest": str(scope_snapshot.get("coordinate_digest", "")),
            "scope_mode": "on_demand" if mode == "on-demand" else "complete",
            "scope_status": (
                "ready_partial_on_demand"
                if partial.get("status") == "ready"
                else "ready_complete"
                if mode == "complete" and status == "ready"
                else "ready_on_demand"
                if status == "ready"
                else "needs_repair"
            ),
            "scope_count": str(scope_snapshot.get("scope_count", 0)),
            "scope_ready_count": str(scope_snapshot.get("ready_count", 0)),
            "scope_missing_count": str(scope_snapshot.get("missing_count", 0)),
            "scope_coverage": (
                f"{partial.get('indexed_pdb_count', 0)}/{scope_snapshot.get('scope_count', 0)}"
            ),
            "refresh_needed": str(bool(partial.get("refresh_needed", False))).lower(),
            "indexed_source_listing_sha256": str(partial.get("indexed_source_listing_sha256", "")),
            "indexed_validated_ids_manifest": str(partial.get("indexed_validated_ids_manifest", "")),
            "indexed_validated_ids_sha256": str(partial.get("indexed_validated_ids_sha256", "")),
            "indexed_mapping_sha256": str(partial.get("indexed_mapping_sha256", "")),
            "indexed_subset_digest": str(partial.get("indexed_subset_digest", "")),
            "indexed_pdb_count": str(partial.get("indexed_pdb_count", "")),
            "indexed_entries": str(partial.get("indexed_entries", "")),
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
            "scope_coordinate_digest",
            "scope_mode",
            "scope_status",
            "scope_count",
            "scope_ready_count",
            "scope_missing_count",
            "scope_coverage",
            "refresh_needed",
            "indexed_source_listing_sha256",
            "indexed_validated_ids_manifest",
            "indexed_validated_ids_sha256",
            "indexed_mapping_sha256",
            "indexed_subset_digest",
            "indexed_pdb_count",
            "indexed_entries",
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
    scratch_dir: Path | None = None,
) -> tuple[str, str]:
    if not path_has_payload(source):
        return "failed", "source_missing"
    if command is None and not (os.name == "nt" and wsl_distro):
        return "failed", "foldseek_executable_missing"
    artifacts_present = database_artifacts_present(db_path)
    if artifacts_present and not force_rebuild and database_prefix_ready("foldseek", db_path, command, env, wsl_distro):
        return "ready", "already_available"
    return rebuild_foldseek_database(
        command,
        source,
        db_path,
        env,
        wsl_distro,
        scratch_dir=scratch_dir,
    )


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
    *,
    scratch_dir: Path | None = None,
) -> tuple[str, str]:
    scratch_root = Path(scratch_dir) if scratch_dir is not None else db_path.parent
    estimate = foldseek_staging_estimate(source, db_path)
    try:
        scratch_available = int(shutil.disk_usage(existing_path_for_disk_usage(scratch_root)).free)
    except OSError as err:
        return "failed", f"foldseek_scratch_disk_probe_failed_previous_preserved:{exception_note(err)}"
    scratch_required = int(estimate["scratch_staging_bytes"]) + DISK_SAFETY_MARGIN_BYTES
    if scratch_required > scratch_available:
        return (
            "failed",
            "foldseek_scratch_capacity_insufficient_previous_preserved:"
            f"required={scratch_required};available={scratch_available}",
        )

    token = f"{os.getpid()}-{time.time_ns()}"
    scratch_parent = scratch_root / f"foldseek-rcsb-rebuild-{token}"
    scratch_db = scratch_parent / db_path.name
    target_staged_db = staged_database_path(db_path)
    try:
        scratch_parent.mkdir(parents=True, exist_ok=False)
        status, notes = create_foldseek_database_artifacts(
            command,
            source,
            scratch_db,
            env,
            wsl_distro,
        )
        if status != "ready":
            return "failed", f"rebuild_failed_previous_preserved:{notes}"
        shutil.rmtree(scratch_parent / "tmp", ignore_errors=True)
        if not database_prefix_ready("foldseek", scratch_db, command, env, wsl_distro):
            return "failed", "rebuild_failed_previous_preserved:foldseek_scratch_database_smoke_failed"

        publish_note = "precomputed_index_retained"
        target_available = int(shutil.disk_usage(existing_path_for_disk_usage(db_path.parent)).free)
        staged_bytes = directory_size(scratch_parent)
        if staged_bytes + DISK_SAFETY_MARGIN_BYTES > target_available:
            if not remove_foldseek_precomputed_index(scratch_db):
                return "failed", "rebuild_failed_previous_preserved:insufficient_target_disk_for_compact_publish"
            if not database_prefix_ready("foldseek", scratch_db, command, env, wsl_distro):
                return "failed", "rebuild_failed_previous_preserved:foldseek_compact_database_smoke_failed"
            publish_note = "precomputed_index_removed_for_target_capacity"
            staged_bytes = directory_size(scratch_parent)
            target_available = int(shutil.disk_usage(existing_path_for_disk_usage(db_path.parent)).free)
        if staged_bytes + DISK_SAFETY_MARGIN_BYTES > target_available:
            return "failed", "rebuild_failed_previous_preserved:insufficient_target_disk_for_compact_publish"

        copy_foldseek_database_with_capacity_guard(scratch_parent, target_staged_db.parent)
        if not database_prefix_ready("foldseek", target_staged_db, command, env, wsl_distro):
            return "failed", "rebuild_failed_previous_preserved:foldseek_target_staging_smoke_failed"
        cleanup_complete = publish_database_directory(target_staged_db, db_path)
        cleanup_note = "" if cleanup_complete else ";previous_backup_cleanup_pending"
        volume_note = (
            "cross_volume_scratch"
            if not same_storage_volume(scratch_root, db_path.parent)
            else "same_volume_scratch"
        )
        return (
            "ready",
            f"atomic_rebuild_completed:{notes};{volume_note};{publish_note}{cleanup_note}",
        )
    except Exception as err:  # noqa: BLE001 - preserve the current database and report repair failure
        return "failed", f"rebuild_failed_previous_preserved:{exception_note(err)}"
    finally:
        shutil.rmtree(scratch_parent, ignore_errors=True)
        shutil.rmtree(target_staged_db.parent, ignore_errors=True)


def create_foldseek_database_artifacts(
    command: str | None,
    source: Path,
    db_path: Path,
    env: dict[str, str] | None,
    wsl_distro: str | None,
) -> tuple[str, str]:
    db_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = db_path.parent / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    if os.name == "nt" and wsl_distro:
        created = run_wsl_database_command(
            wsl_distro,
            ["foldseek", "createdb", wsl_path(source), wsl_path(db_path)],
        )
        if created.returncode != 0:
            return "failed", process_note(created)
        indexed = run_wsl_database_command(
            wsl_distro,
            ["foldseek", "createindex", wsl_path(db_path), wsl_path(tmp)],
        )
    elif command is not None:
        created = run_command([command, "createdb", str(source), str(db_path)], env=env)
        if created.returncode != 0:
            return "failed", process_note(created)
        indexed = run_command([command, "createindex", str(db_path), str(tmp)], env=env)
    else:
        return "failed", "foldseek_executable_missing"
    if indexed.returncode != 0:
        remove_foldseek_precomputed_index(db_path)
        return "ready", f"foldseek_createdb_ready_createindex_not_precomputed:{process_note(indexed)}"
    return "ready", "foldseek_createdb_createindex_completed"


def foldseek_database_artifact_files(db_path: Path) -> list[Path]:
    if not db_path.parent.is_dir():
        return []
    return sorted(
        path
        for path in db_path.parent.iterdir()
        if path.is_file()
        and (
            path.name == db_path.name
            or path.name.startswith(f"{db_path.name}.")
            or path.name.startswith(f"{db_path.name}_")
        )
    )


def foldseek_compact_database_size(db_path: Path) -> int:
    index_prefix = f"{db_path.name}.idx"
    return sum(
        path.stat().st_size
        for path in foldseek_database_artifact_files(db_path)
        if path.name != index_prefix and not path.name.startswith(f"{index_prefix}.")
    )


def foldseek_staging_estimate(
    source: Path,
    db_path: Path,
    previous: dict[str, str] | None = None,
) -> dict[str, int]:
    source_files = max(1, file_count(source))
    source_bytes = max(1, directory_size(source))
    try:
        previous_files = max(0, int((previous or {}).get("source_files", "0")))
    except ValueError:
        previous_files = 0

    def scaled(value: int) -> int:
        if value <= 0:
            return 0
        return (value * source_files + max(1, previous_files) - 1) // max(1, previous_files)

    existing_full = directory_size(db_path.parent)
    existing_compact = foldseek_compact_database_size(db_path)
    empirical_full = scaled(existing_full) if previous_files else existing_full
    empirical_compact = scaled(existing_compact) if previous_files else existing_compact
    target_staging = max(empirical_compact, source_bytes // 32, source_files * 4 * 1024)
    scratch_staging = max(
        empirical_full,
        target_staging * 8,
        source_bytes // 4,
        source_files * 32 * 1024,
    )
    return {
        "source_files": source_files,
        "source_bytes": source_bytes,
        "scratch_staging_bytes": scratch_staging,
        "target_staging_bytes": target_staging,
    }


def remove_foldseek_precomputed_index(db_path: Path) -> bool:
    index_prefix = f"{db_path.name}.idx"
    removed = False
    for path in foldseek_database_artifact_files(db_path):
        if path.name != index_prefix and not path.name.startswith(f"{index_prefix}."):
            continue
        path.unlink(missing_ok=True)
        removed = True
    return removed


def copy_foldseek_database_with_capacity_guard(source: Path, destination: Path) -> None:
    files = sorted(path for path in source.rglob("*") if path.is_file())
    remaining = sum(path.stat().st_size for path in files)
    destination.mkdir(parents=True, exist_ok=False)

    def guard() -> None:
        available = int(shutil.disk_usage(existing_path_for_disk_usage(destination)).free)
        if remaining + DISK_SAFETY_MARGIN_BYTES > available:
            raise OSError(
                "foldseek_target_disk_safety_margin_reached:"
                f"remaining={remaining};available={available}"
            )

    guard()
    for source_file in files:
        relative = source_file.relative_to(source)
        destination_file = destination / relative
        destination_file.parent.mkdir(parents=True, exist_ok=True)
        with source_file.open("rb") as source_handle, destination_file.open("xb") as destination_handle:
            while True:
                block = source_handle.read(8 * 1024 * 1024)
                if not block:
                    break
                guard()
                destination_handle.write(block)
                remaining -= len(block)
        shutil.copystat(source_file, destination_file)
    guard()


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


def database_directory_transaction_path(db_path: Path) -> Path:
    live_parent = db_path.parent
    return live_parent.parent / f".{live_parent.name}.publish-transaction.json"


def database_directory_structure_ready(db_path: Path) -> bool:
    companions = [db_path, Path(f"{db_path}.dbtype"), Path(f"{db_path}.index"), Path(f"{db_path}.lookup")]
    try:
        return bool(
            all(path.is_file() and path.stat().st_size > 0 for path in companions)
            and Path(f"{db_path}.dbtype").stat().st_size == 4
        )
    except OSError:
        return False


def database_directory_transaction_paths(
    db_path: Path,
    payload: dict[str, object],
) -> dict[str, Path] | None:
    try:
        live_parent = db_path.parent.resolve()
        root = live_parent.parent
        staged_parent = Path(str(payload.get("staged_parent", ""))).resolve()
        backup_parent = Path(str(payload.get("backup_parent", ""))).resolve()
        recorded_live = Path(str(payload.get("live_parent", ""))).resolve()
    except (OSError, RuntimeError, ValueError):
        return None
    if (
        payload.get("schema_version") != DATABASE_DIRECTORY_TRANSACTION_SCHEMA_VERSION
        or payload.get("database_name") != db_path.name
        or recorded_live != live_parent
        or staged_parent.parent != root
        or backup_parent.parent != root
        or not staged_parent.name.startswith(f".{live_parent.name}.rebuild-")
        or not backup_parent.name.startswith(f".{live_parent.name}.previous-")
    ):
        return None
    return {
        "live_parent": live_parent,
        "staged_parent": staged_parent,
        "backup_parent": backup_parent,
    }


def reconcile_database_directory_publish(
    db_path: Path,
    validator: Callable[[Path], bool] | None = None,
) -> dict[str, str]:
    live_parent = db_path.parent.resolve()
    root = live_parent.parent
    transaction = database_directory_transaction_path(db_path)
    rebuild_candidates = sorted(root.glob(f".{live_parent.name}.rebuild-*"))
    backup_candidates = sorted(root.glob(f".{live_parent.name}.previous-*"))
    if not transaction.exists() and not rebuild_candidates and not backup_candidates:
        return {"status": "ready", "reason": "no_database_directory_transaction"}

    payload = read_json_object(transaction) if transaction.exists() else {}
    recorded = database_directory_transaction_paths(db_path, payload) if transaction.exists() else None
    if transaction.exists() and recorded is None:
        return {"status": "needs_repair", "reason": "database_directory_transaction_descriptor_invalid"}
    if recorded is not None:
        for kind, candidate in (
            ("rebuild", recorded["staged_parent"]),
            ("backup", recorded["backup_parent"]),
        ):
            candidates = rebuild_candidates if kind == "rebuild" else backup_candidates
            if candidate.exists() and candidate not in candidates:
                candidates.append(candidate)
                candidates.sort()
        allowed_rebuild = {recorded["staged_parent"]}
        allowed_backup = {recorded["backup_parent"]}
        if set(rebuild_candidates) - allowed_rebuild or set(backup_candidates) - allowed_backup:
            return {"status": "needs_repair", "reason": "database_directory_transaction_candidates_ambiguous"}
    if len(rebuild_candidates) > 1 or len(backup_candidates) > 1:
        return {"status": "needs_repair", "reason": "database_directory_transaction_candidates_ambiguous"}

    probe = validator or database_directory_structure_ready

    def parent_ready(parent: Path) -> bool:
        try:
            return parent.is_dir() and bool(probe(parent / db_path.name))
        except (OSError, RuntimeError, ValueError):
            return False

    staged_parent = rebuild_candidates[0] if rebuild_candidates else None
    backup_parent = backup_candidates[0] if backup_candidates else None
    live_ready = parent_ready(live_parent)
    staged_ready = bool(staged_parent and parent_ready(staged_parent))
    backup_ready = bool(backup_parent and parent_ready(backup_parent))

    if live_parent.exists() and not live_ready:
        return {"status": "needs_repair", "reason": "database_directory_live_candidate_invalid"}
    if live_ready:
        try:
            if staged_parent is not None:
                shutil.rmtree(staged_parent)
            if backup_parent is not None:
                shutil.rmtree(backup_parent)
            transaction.unlink(missing_ok=True)
        except OSError as err:
            return {
                "status": "needs_repair",
                "reason": f"database_directory_recovery_cleanup_failed:{exception_note(err)}",
            }
        return {"status": "ready", "reason": "database_directory_live_candidate_retained"}

    selected: Path | None = backup_parent if backup_ready else staged_parent if staged_ready else None
    if selected is None:
        return {"status": "needs_repair", "reason": "database_directory_recovery_candidate_missing_or_invalid"}
    try:
        selected.replace(live_parent)
        remaining = staged_parent if selected == backup_parent else backup_parent
        if remaining is not None:
            shutil.rmtree(remaining)
        transaction.unlink(missing_ok=True)
    except OSError as err:
        return {
            "status": "needs_repair",
            "reason": f"database_directory_recovery_publish_failed:{exception_note(err)}",
        }
    return {
        "status": "ready",
        "reason": (
            "database_directory_previous_candidate_restored"
            if selected == backup_parent
            else "database_directory_staged_candidate_published"
        ),
    }


def database_publish_hard_exit(stage: str) -> None:
    if os.environ.get("GENOLIGAND_TEST_DATABASE_PUBLISH_HARD_EXIT_STAGE", "").strip() == stage:
        os._exit(97)


def publish_database_directory(staged_db: Path, db_path: Path) -> bool:
    staged_parent = staged_db.parent
    live_parent = db_path.parent
    backup = live_parent.with_name(f".{live_parent.name}.previous-{os.getpid()}-{time.time_ns()}")
    transaction = database_directory_transaction_path(db_path)
    if not database_directory_structure_ready(staged_db):
        raise ValueError("staged_database_directory_invalid")
    if staged_parent.parent.resolve() != live_parent.parent.resolve() or not staged_parent.name.startswith(
        f".{live_parent.name}.rebuild-"
    ):
        raise ValueError("staged_database_directory_path_invalid")
    other_rebuilds = [
        candidate
        for candidate in live_parent.parent.glob(f".{live_parent.name}.rebuild-*")
        if candidate.resolve() != staged_parent.resolve()
    ]
    if (
        transaction.exists()
        or other_rebuilds
        or any(live_parent.parent.glob(f".{live_parent.name}.previous-*"))
    ):
        raise RuntimeError("database_directory_transaction_requires_reconciliation")
    write_json_atomic(
        transaction,
        {
            "schema_version": DATABASE_DIRECTORY_TRANSACTION_SCHEMA_VERSION,
            "status": "prepared",
            "database_name": db_path.name,
            "live_parent": str(live_parent.resolve()),
            "staged_parent": str(staged_parent.resolve()),
            "backup_parent": str(backup.resolve()),
            "updated_at": int(time.time()),
        },
    )
    database_publish_hard_exit("after_descriptor")
    if live_parent.exists():
        live_parent.replace(backup)
    write_json_atomic(
        transaction,
        {
            **read_json_object(transaction),
            "status": "previous_moved",
            "updated_at": int(time.time()),
        },
    )
    database_publish_hard_exit("after_backup")
    try:
        staged_parent.replace(live_parent)
    except Exception:
        if backup.exists() and not live_parent.exists():
            backup.replace(live_parent)
        if database_directory_structure_ready(db_path):
            transaction.unlink(missing_ok=True)
        raise
    write_json_atomic(
        transaction,
        {
            **read_json_object(transaction),
            "status": "live_published",
            "updated_at": int(time.time()),
        },
    )
    database_publish_hard_exit("after_live")
    if backup.exists():
        try:
            shutil.rmtree(backup)
        except OSError:
            return False
    transaction.unlink(missing_ok=True)
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


def legacy_mmseqs_createdb_matches_source(source: Path, db_path: Path) -> bool:
    header = db_path.with_name(f"{db_path.name}_h")
    companions = [
        db_path,
        Path(f"{db_path}.dbtype"),
        Path(f"{db_path}.index"),
        Path(f"{db_path}.lookup"),
        Path(f"{db_path}.source"),
        header,
        Path(f"{header}.dbtype"),
        Path(f"{header}.index"),
    ]
    try:
        if any(not path.is_file() or path.stat().st_size <= 0 for path in companions):
            return False
        if Path(f"{db_path}.dbtype").stat().st_size != 4 or Path(f"{header}.dbtype").stat().st_size != 4:
            return False
        marker_rows = [
            line.split("\t", 1)
            for line in Path(f"{db_path}.source").read_text(encoding="utf-8", errors="strict").splitlines()
            if line.strip()
        ]
        if len(marker_rows) != 1 or len(marker_rows[0]) != 2:
            return False
        int(marker_rows[0][0])
        marker_name = Path(marker_rows[0][1].strip().replace("\\", "/")).name
        return bool(marker_name) and marker_name == source.name
    except (OSError, UnicodeError, ValueError):
        return False


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
    if not paths_match:
        return False
    provenance_fields = ("source_digest", "source_files", "source_bytes", "builder_version")
    provided_provenance = [row.get(field, "") for field in provenance_fields]
    if any(provided_provenance):
        expected = alignment_source_manifest_fields(
            source_summary or resource_payload_summary(source),
            builder_version,
        )
        if any(not value or value != expected[field] for field, value in zip(provenance_fields, provided_provenance)):
            return False
    elif tool != "mmseqs":
        return False
    if tool == "mmseqs" and not legacy_mmseqs_createdb_matches_source(source, db_path):
        return False
    return database_prefix_ready(tool, db_path, command, env, wsl_distro)


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
    previous_source_rows = source_manifest_rows_by_id(science_store)
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
                "storage_state",
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

            retained = retained_source_ready(
                science_store,
                source_id,
                local_rel,
                summary_cache,
                expected_release_identity=row.get("release_identity", ""),
                expected_trust_source=row.get("trust_source", ""),
                expected_sha256=row.get("sha256", ""),
            )
            if retained:
                previous_source = previous_source_rows.get(source_id, {})
                file_count_value = int(retained.get("source_files", "0") or 0)
                bytes_count = int(retained.get("source_bytes", "0") or 0)
                digest = retained.get("source_manifest_sha256", "")
                exists = file_count_value > 0 and bytes_count > 0 and bool(digest)
                evidence_digest = retained.get("execution_sha256", "")
                storage_state = "indexed_source_pruned"
                if not row["provenance"]:
                    row["provenance"] = previous_source.get("provenance", "")
            else:
                summary = resource_payload_summary(local_path, source_progress, cache=summary_cache)
                file_count_value = int(summary["file_count"])
                bytes_count = int(summary["bytes"])
                digest = str(summary["sha256"])
                exists = file_count_value > 0 and bytes_count > 0
                evidence_digest = str(summary.get("content_sha256", "")) if local_path.is_file() else digest
                storage_state = "materialized"
            rcsb_validation = (
                rcsb_scope_snapshot_validation(
                    science_store,
                    execution=external_executions.get(source_id, {}),
                    source_summary=summary,
                )
                if source_id == "rcsb_ligand_bound" and row["source_class"] != "direct_download"
                else {"status": "not_applicable"}
            )
            execution_digest_valid = bool(
                row["execution_sha256"].lower() == evidence_digest.lower()
                or rcsb_validation.get("status") in {"ready_partial_on_demand", "ready_complete"}
            )
            trusted = bool(
                row["release_identity"]
                and row["trust_source"]
                and row["execution_status"] == "ready"
                and execution_digest_valid
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
                "storage_state": storage_state,
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
    *,
    rcsb_mode: str = "on-demand",
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
            row,
            target,
            marker,
            science_store,
            execution or {},
            summary_cache,
            rcsb_mode=rcsb_mode,
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


def prepare_ligand_similarity_index(
    workspace_root: Path,
    science_store: Path,
    python_command: str,
    python_backend: str,
    wsl_distro: str | None,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    database = science_store / LIGAND_SIMILARITY_DATABASE_REL_PATH
    manifest = science_store / LIGAND_SIMILARITY_MANIFEST_REL_PATH
    sources = source_manifest_rows_by_id(science_store)
    if ligand_similarity_index_ready(science_store, sources, summary_cache):
        payload = read_json_object(manifest)
        return {
            "status": "ready",
            "rebuilt": False,
            "reason": "ligand_similarity_index_reused",
            "database": str(database),
            "manifest": str(manifest),
            "counts": payload.get("counts", {}),
            "database_bytes": payload.get("database", {}).get("bytes", 0),
            "reclaimed_candidate_bytes": payload.get("reclaimed_candidate_bytes", 0),
        }
    chembl_database = science_store / "downloads/chembl/chembl_query.sqlite"
    chembl_manifest = chembl_database.with_suffix(".manifest.json")
    bindingdb_database = science_store / "indexes/bindingdb/bindingdb_query.sqlite"
    bindingdb_manifest = bindingdb_database.with_suffix(".manifest.json")
    required = (chembl_database, chembl_manifest, bindingdb_database, bindingdb_manifest)
    if any(not path.is_file() for path in required):
        return {
            "status": "failed",
            "rebuilt": False,
            "reason": "ligand_similarity_source_query_database_missing",
            "missing": [str(path) for path in required if not path.is_file()],
        }
    worker = workspace_root / "python/genoligand_workers/ligand_similarity_index.py"
    if not worker.is_file():
        return {
            "status": "failed",
            "rebuilt": False,
            "reason": "ligand_similarity_worker_missing",
        }
    database.parent.mkdir(parents=True, exist_ok=True)
    arguments = [
        "build",
        "--chembl-db",
        str(chembl_database),
        "--chembl-manifest",
        str(chembl_manifest),
        "--bindingdb-db",
        str(bindingdb_database),
        "--bindingdb-manifest",
        str(bindingdb_manifest),
        "--output-db",
        str(database),
        "--manifest",
        str(manifest),
    ]
    if python_backend == "wsl":
        if not wsl_distro:
            return {"status": "failed", "rebuilt": False, "reason": "ligand_similarity_wsl_distro_missing"}
        home = wsl_home(wsl_distro)
        if not home:
            return {"status": "failed", "rebuilt": False, "reason": "ligand_similarity_wsl_home_missing"}
        worker_python = f"{home}/.local/state/genoligand/python-worker-env/bin/python"
        command = [
            worker_python,
            wsl_path(worker),
            "build",
            "--chembl-db",
            wsl_path(chembl_database),
            "--chembl-manifest",
            wsl_path(chembl_manifest),
            "--bindingdb-db",
            wsl_path(bindingdb_database),
            "--bindingdb-manifest",
            wsl_path(bindingdb_manifest),
            "--output-db",
            wsl_path(database),
            "--manifest",
            wsl_path(manifest),
        ]
        temporary_root = wsl_path(database.parent)
        script = (
            f"SQLITE_TMPDIR={bash_words([temporary_root])} "
            f"TMPDIR={bash_words([temporary_root])} "
            f"PYTHONPATH={bash_words([wsl_path(workspace_root / 'python')])} "
            f"{bash_words(command)}"
        )
        completed = run_wsl_user_shell(wsl_distro, script)
    elif python_command:
        env = dict(os.environ)
        env["PYTHONPATH"] = str(workspace_root / "python")
        env["SQLITE_TMPDIR"] = str(database.parent)
        env["TMPDIR"] = str(database.parent)
        completed = run_command([python_command, str(worker), *arguments], env=env)
    else:
        return {"status": "failed", "rebuilt": False, "reason": "ligand_similarity_python_missing"}
    stale_validation_keys = {
        key
        for key in LIGAND_SIMILARITY_SQLITE_VALIDATION_CACHE
        if key[0] == str(database.resolve())
    }
    LIGAND_SIMILARITY_SQLITE_VALIDATION_CACHE.difference_update(stale_validation_keys)
    validated = completed.returncode == 0 and ligand_similarity_index_ready(
        science_store,
        sources,
        summary_cache,
    )
    if not validated:
        return {
            "status": "failed",
            "rebuilt": False,
            "reason": (
                process_note(completed)
                if completed.returncode != 0
                else "ligand_similarity_validation_failed"
            ),
        }
    payload = read_json_object(manifest)
    return {
        "status": "ready",
        "rebuilt": True,
        "reason": "ligand_similarity_index_built",
        "database": str(database),
        "manifest": str(manifest),
        "counts": payload.get("counts", {}),
        "database_bytes": payload.get("database", {}).get("bytes", 0),
        "reclaimed_candidate_bytes": payload.get("reclaimed_candidate_bytes", 0),
    }


def migrate_ligand_source_retention(
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    if not ligand_similarity_index_ready(science_store, summary_cache=summary_cache):
        return {"status": "needs_repair", "updated": 0, "reason": "ligand_similarity_index_not_ready"}
    rows = source_retention_rows(science_store)
    expected = ";".join((LIGAND_SIMILARITY_DATABASE_REL_PATH, LIGAND_SIMILARITY_MANIFEST_REL_PATH))
    updated = 0
    selected: list[dict[str, str]] = []
    for row in rows:
        if row.get("source_id") not in {"chembl", "bindingdb"}:
            continue
        selected.append(row)
        if row.get("derived_paths") != expected:
            row["derived_paths"] = expected
            row["notes"] = "source_payload_removed_after_verified_compact_ligand_index_publish"
            updated += 1
    if updated:
        write_source_retention_rows(science_store, rows)
    selected_ids = {row.get("source_id", "") for row in selected}
    if selected_ids and selected_ids != {"chembl", "bindingdb"}:
        return {
            "status": "needs_repair",
            "updated": updated,
            "rows": len(selected),
            "reason": "ligand_source_retention_incomplete",
        }
    valid = all(
        retained_source_derivatives_ready(science_store, row, summary_cache)
        if row.get("state") == "pruned"
        else bool(
            row.get("state") in {"pruning", "cleanup_failed"}
            and retention_source_identity_and_derivatives_ready(science_store, row, summary_cache)
            and retention_live_payloads_match_records(science_store, row, summary_cache)
        )
        for row in selected
    )
    return {
        "status": "ready" if valid or not selected else "needs_repair",
        "updated": updated,
        "rows": len(selected),
        "reason": "ligand_source_retention_migrated" if updated else "ligand_source_retention_current",
    }


def superseded_ligand_query_paths(science_store: Path) -> list[Path]:
    chembl_database = science_store / "downloads/chembl/chembl_query.sqlite"
    chembl_manifest = chembl_database.with_suffix(".manifest.json")
    bindingdb_database = science_store / "indexes/bindingdb/bindingdb_query.sqlite"
    bindingdb_manifest = bindingdb_database.with_suffix(".manifest.json")
    compact_database = science_store / LIGAND_SIMILARITY_DATABASE_REL_PATH
    compact_manifest = science_store / LIGAND_SIMILARITY_MANIFEST_REL_PATH
    candidates: list[Path] = [
        chembl_manifest.with_name(f"{chembl_manifest.name}.attempt.json"),
        chembl_query_transaction_path(chembl_database),
        chembl_database.parent / ".chembl_query.sha256_cache",
        bindingdb_database.parent / ".chembl_query.sha256_cache",
    ]
    for base in (chembl_database, chembl_manifest, bindingdb_database, bindingdb_manifest):
        candidates.extend(Path(f"{base}{suffix}") for suffix in ("", "-wal", "-shm", "-journal"))
    prefixes = (
        (chembl_database.parent, (f".{chembl_database.name}.", f".{chembl_manifest.name}.")),
        (
            bindingdb_database.parent,
            (
                ".bindingdb_query_",
                ".bindingdb_query_manifest_",
                f".{bindingdb_database.name}.",
                f".{bindingdb_manifest.name}.",
            ),
        ),
        (
            compact_database.parent,
            (f".{compact_database.name}.", f".{compact_manifest.name}."),
        ),
    )
    for parent, names in prefixes:
        if parent.is_dir():
            candidates.extend(
                path for path in parent.iterdir() if any(path.name.startswith(prefix) for prefix in names)
            )
    return [path for path in dict.fromkeys(candidates) if path.exists()]


def make_cleanup_path_writable(path: Path) -> None:
    if path.is_dir():
        root = path.resolve()
        for current, directories, files in os.walk(path, topdown=True, followlinks=False):
            current_path = Path(current)
            safe_directories: list[str] = []
            for name in directories:
                child = current_path / name
                if child.is_symlink() or os.path.isjunction(child):
                    raise OSError(f"superseded_ligand_nested_link_unsafe:{child}")
                resolved = child.resolve()
                if not resolved.is_relative_to(root):
                    raise OSError(f"superseded_ligand_nested_path_unsafe:{child}")
                child.chmod(child.stat().st_mode | stat.S_IWRITE | stat.S_IREAD)
                safe_directories.append(name)
            directories[:] = safe_directories
            for name in files:
                child = current_path / name
                if child.is_symlink() or os.path.isjunction(child):
                    raise OSError(f"superseded_ligand_nested_link_unsafe:{child}")
                resolved = child.resolve()
                if not resolved.is_relative_to(root):
                    raise OSError(f"superseded_ligand_nested_path_unsafe:{child}")
                child.chmod(child.stat().st_mode | stat.S_IWRITE | stat.S_IREAD)
    path.chmod(path.stat().st_mode | stat.S_IWRITE | stat.S_IREAD)


def prune_superseded_ligand_query_databases(
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    if not ligand_similarity_index_ready(science_store, summary_cache=summary_cache):
        return {"status": "needs_repair", "reclaimed_bytes": 0, "reason": "ligand_similarity_index_not_ready"}
    root = science_store.resolve()
    candidates = superseded_ligand_query_paths(science_store)
    reclaimed = 0
    removed: list[str] = []
    try:
        for path in dict.fromkeys(candidates):
            if not path.exists():
                continue
            resolved = path.resolve()
            if (
                resolved == root
                or not resolved.is_relative_to(root)
                or path.is_symlink()
                or os.path.isjunction(path)
            ):
                raise OSError(f"superseded_ligand_path_unsafe:{path}")
            make_cleanup_path_writable(path)
            if path.is_dir():
                reclaimed += sum(item.stat().st_size for item in path.rglob("*") if item.is_file())
                shutil.rmtree(path)
            elif path.is_file():
                reclaimed += path.stat().st_size
                path.unlink()
            else:
                raise OSError(f"superseded_ligand_path_invalid:{path}")
            removed.append(str(path))
            if summary_cache is not None:
                prefix = str(resolved).casefold()
                for key in [
                    key
                    for key in summary_cache
                    if key.casefold() == prefix or key.casefold().startswith(f"{prefix}{os.sep}")
                ]:
                    summary_cache.pop(key, None)
    except OSError as err:
        return {
            "status": "needs_repair",
            "reclaimed_bytes": reclaimed,
            "removed": removed,
            "reason": exception_note(err),
        }
    for parent in (
        science_store / "downloads/chembl",
        science_store / "indexes/bindingdb",
    ):
        if parent.is_dir() and not any(parent.iterdir()):
            parent.rmdir()
    remaining = [str(path) for path in superseded_ligand_query_paths(science_store)]
    return {
        "status": "ready" if not remaining else "needs_repair",
        "reclaimed_bytes": reclaimed,
        "removed": removed,
        "remaining": remaining,
        "reason": "superseded_ligand_query_databases_removed",
    }


def prepare_plinder_canonical_index(
    workspace_root: Path,
    science_store: Path,
    python_command: str,
    python_backend: str,
    wsl_distro: str | None,
) -> dict[str, object]:
    source = plinder_parquet_source(science_store)
    if source is None:
        retained = retained_source_ready(science_store, "plinder", "downloads/plinder/index")
        if retained:
            projection = science_store / "indexes" / "structure_targets.tsv"
            rows = max(
                (
                    int(row.get("rows", 0) or 0)
                    for row in read_tsv(science_store / "index_build_manifest.tsv")
                    if row.get("index") == "structure_targets"
                    and "plinder_canonical_rows" in row.get("notes", "").split(";")
                ),
                default=0,
            )
            return {
                "status": "ready",
                "rebuilt": False,
                "reason": "plinder_structure_projection_reused_after_source_payload_pruned",
                "rows": rows,
                "projection": str(projection),
            }
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
            if tool == "foldseek" and row.get("scope_status") == "ready_partial_on_demand":
                if row.get("builder_version") != builder_version:
                    return False
                validation = foldseek_partial_manifest_validation(
                    science_store,
                    row,
                    db_path,
                    command,
                    env,
                    wsl_distro,
                )
                if validation.get("status") != "ready":
                    return False
                continue
            if not path_has_payload(source) and retention_covers_source_path(
                science_store, row.get("source_path", "")
            ):
                if (
                    row.get("builder_version") != builder_version
                    or not re.fullmatch(r"[0-9a-f]{64}", row.get("source_digest", "").lower())
                    or not database_prefix_ready(tool, db_path, command, env, wsl_distro)
                ):
                    return False
                continue
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


def metagenome_alignment_authority_key(row: dict[str, str]) -> tuple[str, str, str]:
    notes = str(row.get("notes", ""))
    digest = next(
        (
            token.removeprefix("source_digest=").lower()
            for token in notes.split(";")
            if token.startswith("source_digest=")
        ),
        "",
    )
    return (
        str(row.get("input_path", "")).replace("\\", "/"),
        str(row.get("output_path", "")).replace("\\", "/"),
        digest,
    )


def authoritative_mmseqs_databases(
    science_store: Path,
    build_rows: list[dict[str, str]],
    namespaces: dict[str, tuple[str, str]],
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> set[tuple[str, str, str]]:
    manifest_rel = "indexes/mmseqs/mmseqs_target_databases.tsv"
    manifest_rows = read_tsv(science_store / manifest_rel)
    command = shutil.which("mmseqs", path=(env or os.environ).get("PATH"))
    builder_version = database_builder_version("mmseqs", command, env, wsl_distro)
    validated: set[tuple[str, str, str]] = set()
    for build_row in build_rows:
        if build_row.get("status") != "alignment_database_used":
            continue
        input_path = str(build_row.get("input_path", "")).replace("\\", "/")
        output_path = str(build_row.get("output_path", "")).replace("\\", "/")
        notes = str(build_row.get("notes", ""))
        for row in manifest_rows:
            expected = namespaces.get(str(row.get("source_namespace", "")))
            source_path = str(row.get("source_path", "")).replace("\\", "/")
            db_path_text = str(row.get("db_path", "")).replace("\\", "/")
            source_digest = str(row.get("source_digest", "")).lower()
            if (
                expected is None
                or build_row.get("index") != expected[0]
                or row.get("status") != "ready"
                or row.get("source_class") != expected[1]
                or source_path != input_path
                or db_path_text != output_path
                or f"source_digest={source_digest}" not in notes.split(";")
                or "full_searchable_alignment_database" not in notes.split(";")
                or "consumer=mmseqs_alignment_worker" not in notes.split(";")
                or f"database_manifest={manifest_rel}" not in notes.split(";")
            ):
                continue
            source = resolve_manifest_path(science_store, source_path)
            db_path = resolve_manifest_path(science_store, db_path_text)
            if source is None or db_path is None:
                continue
            if not path_has_payload(source) and retention_covers_alignment_source_path(
                science_store, source_path
            ):
                if (
                    row.get("builder_version") != builder_version
                    or not re.fullmatch(r"[0-9a-f]{64}", source_digest)
                ):
                    continue
            else:
                source_summary = resource_payload_summary(source, cache=summary_cache)
                if not alignment_source_manifest_matches(row, source_summary, builder_version):
                    continue
            if database_prefix_ready("mmseqs", db_path, command, env, wsl_distro):
                validated.add((input_path, output_path, source_digest))
                break
    return validated


def authoritative_metagenome_mmseqs_databases(
    science_store: Path,
    build_rows: list[dict[str, str]],
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> set[tuple[str, str, str]]:
    return authoritative_mmseqs_databases(
        science_store,
        build_rows,
        {"MGnify": ("metagenome_targets", "metagenome")},
        env,
        wsl_distro,
        summary_cache,
    )


def authoritative_known_mmseqs_databases(
    science_store: Path,
    build_rows: list[dict[str, str]],
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    summary_cache: ResourceSummaryCache | None = None,
) -> set[tuple[str, str, str]]:
    return authoritative_mmseqs_databases(
        science_store,
        build_rows,
        {
            "UniProt": ("known_targets", "known"),
            "BioLiP2": ("known_targets", "known"),
        },
        env,
        wsl_distro,
        summary_cache,
    )


def mmseqs_namespaces_complete(
    science_store: Path,
    authorities: set[tuple[str, str, str]],
    required_counts: dict[str, int],
) -> bool:
    rows = read_tsv(science_store / "indexes/mmseqs/mmseqs_target_databases.tsv")
    for namespace, minimum in required_counts.items():
        namespace_rows = [row for row in rows if row.get("source_namespace") == namespace]
        if not mmseqs_namespace_rows_complete(namespace_rows, namespace, minimum) or any(
            (
                str(row.get("source_path", "")).replace("\\", "/"),
                str(row.get("db_path", "")).replace("\\", "/"),
                str(row.get("source_digest", "")).lower(),
            )
            not in authorities
            for row in namespace_rows
        ):
            return False
    return True


def refresh_science_index_authority_manifest(
    science_store: Path,
    summary_cache: ResourceSummaryCache | None = None,
) -> dict[str, object]:
    build_manifest = science_store / "index_build_manifest.tsv"
    build_rows = read_tsv(build_manifest)
    database_manifest_rel = "indexes/mmseqs/mmseqs_target_databases.tsv"
    database_rows = read_tsv(science_store / database_manifest_rel)
    authorities = []
    authority_specs = {
        "UniProt": ("known_targets", "known"),
        "BioLiP2": ("known_targets", "known"),
        "MGnify": ("metagenome_targets", "metagenome"),
    }
    for row in database_rows:
        spec = authority_specs.get(str(row.get("source_namespace", "")))
        digest = str(row.get("source_digest", "")).lower()
        db_path_text = str(row.get("db_path", "")).replace("\\", "/")
        db_path = resolve_manifest_path(science_store, db_path_text)
        companions = (
            []
            if db_path is None
            else [db_path, Path(f"{db_path}.dbtype"), Path(f"{db_path}.index"), Path(f"{db_path}.lookup")]
        )
        if (
            row.get("manifest_schema_version") == ALIGNMENT_MANIFEST_SCHEMA_VERSION
            and row.get("status") == "ready"
            and spec is not None
            and row.get("source_class") == spec[1]
            and str(row.get("source_path", "")).strip()
            and re.fullmatch(r"[0-9a-f]{64}", digest)
            and row.get("builder_version")
            and db_path_text
            and companions
            and all(path.is_file() and path.stat().st_size > 0 for path in companions)
        ):
            authorities.append(row)
    updated_rows = 0
    legacy_ligand_paths = {
        "downloads/chembl/chembl_query.sqlite",
        "downloads/chembl/chembl_query.manifest.json",
        "indexes/bindingdb/bindingdb_query.sqlite",
        "indexes/bindingdb/bindingdb_query.manifest.json",
    }
    retained_build_rows = [
        row
        for row in build_rows
        if str(row.get("input_path", "")).replace("\\", "/") not in legacy_ligand_paths
        and str(row.get("output_path", "")).replace("\\", "/") not in legacy_ligand_paths
    ]
    updated_rows += len(build_rows) - len(retained_build_rows)
    build_rows = retained_build_rows
    if ligand_similarity_index_ready(science_store, summary_cache=summary_cache):
        compact_manifest = read_json_object(science_store / LIGAND_SIMILARITY_MANIFEST_REL_PATH)
        compact_inputs = compact_manifest.get("inputs", {})
        source_note = ";".join(
            f"{source}_digest={str(compact_inputs.get(source, {}).get('source_digest', ''))}"
            for source in ("chembl", "bindingdb")
        )
        compact_row = {
            "index": "ligand_similarity",
            "input_path": "manifests/source_file_manifest.tsv",
            "status": "used",
            "rows": str(compact_manifest.get("counts", {}).get("evidence", 0)),
            "output_path": LIGAND_SIMILARITY_DATABASE_REL_PATH,
            "notes": (
                "compact_exact_tanimoto_authority;consumer=ligand_similarity_index;"
                f"{source_note}"
            ),
        }
        existing = next((row for row in build_rows if row.get("index") == "ligand_similarity"), None)
        if existing is None:
            build_rows.append(compact_row)
            updated_rows += 1
        elif any(existing.get(key, "") != value for key, value in compact_row.items()):
            existing.update(compact_row)
            updated_rows += 1
    expected_authority_keys: set[tuple[str, str]] = set()
    for authority in authorities:
        namespace = str(authority.get("source_namespace", ""))
        index_id = authority_specs[namespace][0]
        input_path = str(authority.get("source_path", "")).replace("\\", "/")
        digest = str(authority.get("source_digest", "")).lower()
        db_path_text = str(authority.get("db_path", "")).replace("\\", "/")
        expected = {
            "index": index_id,
            "input_path": input_path,
            "status": "alignment_database_used",
            "rows": "0",
            "output_path": db_path_text,
            "notes": (
                "full_searchable_alignment_database;consumer=mmseqs_alignment_worker;"
                f"source_namespace={namespace};source_digest={digest};"
                f"database_manifest={database_manifest_rel}"
            ),
        }
        expected_authority_keys.add((index_id, input_path))
        existing = next(
            (
                row
                for row in build_rows
                if row.get("index") == index_id
                and row.get("status") == "alignment_database_used"
                and str(row.get("input_path", "")).replace("\\", "/") == input_path
            ),
            None,
        )
        if existing is None:
            build_rows.append(expected)
            updated_rows += 1
        elif any(str(existing.get(key, "")) != value for key, value in expected.items()):
            existing.update(expected)
            updated_rows += 1
    retained_authority_rows = [
        row
        for row in build_rows
        if not (
            row.get("status") == "alignment_database_used"
            and row.get("index") in {"known_targets", "metagenome_targets"}
            and (
                str(row.get("index", "")),
                str(row.get("input_path", "")).replace("\\", "/"),
            )
            not in expected_authority_keys
        )
    ]
    updated_rows += len(build_rows) - len(retained_authority_rows)
    build_rows = retained_authority_rows
    if updated_rows:
        write_simple_tsv(
            build_manifest,
            ["index", "input_path", "status", "rows", "output_path", "notes"],
            build_rows,
        )
    return {
        "status": "ready" if authorities else "needs_repair",
        "updated_rows": updated_rows,
        "reason": "full_mmseqs_authority_bound" if authorities else "ready_mmseqs_authority_missing",
    }


def metagenome_compatibility_table_ready(path: Path) -> bool:
    if not path.is_file():
        return False
    try:
        lines = [line for line in path.read_text(encoding="utf-8-sig").splitlines() if line.strip()]
    except (OSError, UnicodeError):
        return False
    return lines == [
        "source\ttarget_id\ttarget_name\torganism\ttarget_class\tligand_tokens\tsequence_tokens\tstructure_tokens\tevidence"
    ]


def standard_header_only_projection_ready(
    path: Path,
    index_id: str,
    build_rows: list[dict[str, str]],
    alignment_ready: bool,
    *,
    compact_ready: bool = True,
) -> bool:
    return bool(
        alignment_ready
        and compact_ready
        and metagenome_compatibility_table_ready(path)
        and not any(
            row.get("index") == index_id and row.get("status") in {"used", "preserved"}
            for row in build_rows
        )
    )


def science_index_readiness_rows(
    science_store: Path,
    *,
    known_alignment_authorities: set[tuple[str, str, str]] | None = None,
    metagenome_alignment_authorities: set[tuple[str, str, str]] | None = None,
) -> list[dict[str, str]]:
    known_alignment_authorities = known_alignment_authorities or set()
    metagenome_alignment_authorities = metagenome_alignment_authorities or set()
    source_rows = read_tsv(science_store / "index_build_manifest.tsv")
    compact_ligand_authority = any(
        row.get("index") == "ligand_similarity"
        and row.get("status") == "used"
        and row.get("output_path", "").replace("\\", "/")
        == LIGAND_SIMILARITY_DATABASE_REL_PATH
        and "compact_exact_tanimoto_authority" in row.get("notes", "").split(";")
        for row in source_rows
    )
    superseded_bindingdb_inputs = {
        f"downloads/bindingdb/bindingdb_all.{suffix}"
        for suffix in ("tsv", "tsv.gz", "tsv.zip", "txt", "txt.gz", "txt.zip")
    }
    selected_indexes = {
        str(row.get("index", ""))
        for row in source_rows
        if str(row.get("status", "")) == "used"
        or (
            str(row.get("status", "")) == "ready"
            and "published_chembl_query_used_by_onsite_matcher" in str(row.get("notes", ""))
        )
        or (
            str(row.get("status", "")) == "alignment_database_used"
            and (
                (
                    str(row.get("index", "")) == "known_targets"
                    and metagenome_alignment_authority_key(row) in known_alignment_authorities
                )
                or (
                    str(row.get("index", "")) == "metagenome_targets"
                    and metagenome_alignment_authority_key(row)
                    in metagenome_alignment_authorities
                )
            )
        )
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
        compact_superseded = bool(
            compact_ligand_authority
            and raw_status == "superseded"
            and row.get("index") == "known_targets"
            and input_path.replace("\\", "/").casefold() in superseded_bindingdb_inputs
            and output_path.replace("\\", "/") == LIGAND_SIMILARITY_DATABASE_REL_PATH
            and "compact_ligand_similarity_is_authoritative" in notes.split(";")
        )
        optional_missing = bool(
            raw_status
            in {"missing", "not_found", "not_present", "not_applicable", "not_selected", "empty"}
            and str(row.get("index", "")) in selected_indexes
        )
        if compact_superseded:
            readiness_status = "not_selected"
            reason = "superseded_by_compact_ligand_similarity"
        elif optional_missing:
            readiness_status = "not_selected"
            reason = "alternate_candidate_selected"
        elif raw_status == "alignment_database_used":
            index_kind = "alignment_database"
            authority_key = metagenome_alignment_authority_key(row)
            if (
                (
                    str(row.get("index", "")) == "known_targets"
                    and authority_key in known_alignment_authorities
                )
                or (
                    str(row.get("index", "")) == "metagenome_targets"
                    and authority_key in metagenome_alignment_authorities
                )
            ):
                readiness_status = "full_index"
                reason = "full_searchable_alignment_database_ready"
            else:
                readiness_status = "incomplete"
                reason = "alignment_database_authority_not_ready"
        elif raw_status == "ready" and "published_chembl_query_used_by_onsite_matcher" in notes:
            index_kind = "consumer_database"
            readiness_status = "full_index"
            reason = "specialized_consumer_database_ready"
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


def partial_legacy_index_manifest_present(
    science_store: Path,
    rows: list[dict[str, str]] | None = None,
) -> bool:
    rows = rows if rows is not None else science_index_readiness_rows(science_store)
    write_science_index_readiness_manifest(science_store, rows)
    return any(row.get("readiness_status") in {"projection_only", "incomplete"} for row in rows)


def science_readiness_evidence(
    science_store: Path,
    digest_overrides: dict[str, str] | None = None,
) -> tuple[list[dict[str, str]], str]:
    overrides = digest_overrides or {}
    evidence = [
        {
            "path": rel_path,
            "sha256": overrides.get(rel_path) or sha256_file(science_store / rel_path),
        }
        for rel_path in SCIENCE_READINESS_EVIDENCE_PATHS
        if rel_path in overrides or (science_store / rel_path).is_file()
    ]
    digest_input = "".join(f"{row['path']}\t{row['sha256']}\n" for row in evidence)
    return evidence, hashlib.sha256(digest_input.encode("utf-8")).hexdigest()


def evaluate_installer_science_store_readiness(
    science_store: Path,
    *,
    persist: bool = True,
    summary_cache: ResourceSummaryCache | None = None,
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    check_install_status: bool = True,
    check_source_retention: bool = True,
) -> dict[str, object]:
    blockers: list[dict[str, str]] = []
    advisories: list[dict[str, str]] = []
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
    external_execution_rows = {
        row.get("source_id", ""): row
        for row in read_tsv(science_store / "external_sync_execution_manifest.tsv")
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
        rcsb_source_validation = (
            rcsb_scope_snapshot_validation(
                science_store,
                execution=external_execution_rows.get("rcsb_ligand_bound", {}),
                source_summary={
                    "file_count": source_manifest.get("file_count", ""),
                    "bytes": source_manifest.get("bytes", ""),
                    "sha256": source_digest,
                },
            )
            if logical == "rcsb_ligand_bound"
            else {"status": "not_applicable"}
        )
        rcsb_source_ready = rcsb_source_validation.get("status") in {
            "ready_partial_on_demand",
            "ready_complete",
        } and bool(
            release_identity == external_execution_rows.get("rcsb_ligand_bound", {}).get("release_identity", "")
            and trust_source == external_execution_rows.get("rcsb_ligand_bound", {}).get("trust_source", "")
        )
        source_manifest_ready = bool(
            (
                rcsb_source_ready
                if logical == "rcsb_ligand_bound"
                else source_manifest.get("status") == "ready"
            )
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
                "source_manifest_status": (
                    "ready_partial_on_demand"
                    if rcsb_source_validation.get("status") == "ready_partial_on_demand"
                    else source_manifest.get("status", "missing")
                ),
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

    ligand_similarity_ready = ligand_similarity_index_ready(
        science_store,
        source_manifest_rows,
        summary_cache,
    )
    if not ligand_similarity_ready:
        block(
            "ligand_similarity_index_not_ready",
            "ligand_similarity_index",
            f"{LIGAND_SIMILARITY_DATABASE_REL_PATH} and manifest failed identity, schema, or compactness validation",
            "prepare_ligand_similarity_index",
        )
    else:
        superseded = [str(path.relative_to(science_store)) for path in superseded_ligand_query_paths(science_store)]
        if superseded:
            block(
                "superseded_ligand_query_database_present",
                "ligand_similarity_index",
                ";".join(superseded),
                "prune_superseded_ligand_query_databases",
            )

    retention_manifest = source_retention_manifest_path(science_store)
    retention_required = any(
        row.get("storage_state") in {"indexed_source_pruned", "bounded_runtime_cache"}
        for row in source_manifest_rows.values()
    )
    if check_source_retention and retention_manifest.is_file():
        retention_rows = {
            row.get("source_id", ""): row for row in source_retention_rows(science_store)
        }
        for source_id in INDEXED_SOURCE_RETENTION_POLICIES:
            retained = retention_rows.get(source_id, {})
            if not retained_source_derivatives_ready(science_store, retained, summary_cache):
                block(
                    f"source_retention_invalid:{source_id}",
                    "source_retention",
                    source_id,
                    "restore_source_and_rebuild_index",
                )
        rcsb_retention = retention_rows.get("rcsb_ligand_bound", {})
        rcsb_policy = read_json_object(science_store / "downloads/rcsb/on_demand_policy.json")
        rcsb_snapshot = read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
        rcsb_mode = str(rcsb_snapshot.get("scope_mode", "")).replace("_", "-")
        cache_files, cache_bytes = rcsb_cache_usage(science_store / "downloads/rcsb/mmcif")
        rcsb_retention_ready = rcsb_mode == "complete" or (
            rcsb_mode == "on-demand"
            and rcsb_retention.get("schema_version") == SOURCE_RETENTION_SCHEMA_VERSION
            and rcsb_retention.get("state") == "runtime_cache_bounded"
            and rcsb_policy.get("max_cache_bytes") == RCSB_ON_DEMAND_MAX_CACHE_BYTES
            and rcsb_policy.get("max_cache_files") == RCSB_ON_DEMAND_MAX_CACHE_FILES
            and cache_bytes <= RCSB_ON_DEMAND_MAX_CACHE_BYTES
            and cache_files <= RCSB_ON_DEMAND_MAX_CACHE_FILES
        )
        if not rcsb_retention_ready:
            block(
                "rcsb_runtime_cache_not_bounded",
                "source_retention",
                "downloads/rcsb/mmcif",
                "prune_rcsb_on_demand_cache",
            )
    elif check_source_retention and retention_required:
        block(
            "source_retention_missing",
            "source_retention",
            SOURCE_RETENTION_MANIFEST_REL_PATH,
            "restore_source_retention_evidence",
        )

    if check_install_status:
        status_payload = load_json(science_store / "install_status.json")
        status_progress = status_payload.get("progress")
        canonical_ready = status_payload.get("status") == "ready"
        canonical_final_status = bool(
            status_payload.get("status") in {"ready", "needs_repair"}
            and isinstance(status_payload.get("install_ready"), bool)
            and canonical_ready == status_payload.get("install_ready")
            and isinstance(status_progress, dict)
            and status_progress.get("status") == ("ready" if canonical_ready else "needs_repair")
        )
        missing = status_payload.get("missing_components")
        if not canonical_final_status and (
            status_payload.get("status") != "ready" or (isinstance(missing, list) and missing)
        ):
            block(
                "install_status_not_ready",
                "install_status",
                str(status_payload.get("status", "missing")),
                "prepare_science_store",
            )

    index_build_rows = read_tsv(science_store / "index_build_manifest.tsv")
    known_alignment_authorities = authoritative_known_mmseqs_databases(
        science_store,
        index_build_rows,
        env,
        wsl_distro,
        summary_cache,
    )
    metagenome_alignment_authorities = authoritative_metagenome_mmseqs_databases(
        science_store,
        index_build_rows,
        env,
        wsl_distro,
        summary_cache,
    )
    metagenome_alignment_rows = [
        row
        for row in index_build_rows
        if row.get("index") == "metagenome_targets"
        and row.get("status") == "alignment_database_used"
    ]
    metagenome_alignment_ready = bool(metagenome_alignment_rows) and all(
        metagenome_alignment_authority_key(row) in metagenome_alignment_authorities
        for row in metagenome_alignment_rows
    )
    known_alignment_ready = mmseqs_namespaces_complete(
        science_store,
        known_alignment_authorities,
        {"UniProt": 1, "BioLiP2": 4},
    )
    metagenome_alignment_ready = metagenome_alignment_ready and mmseqs_namespaces_complete(
        science_store,
        metagenome_alignment_authorities,
        {"MGnify": 1},
    )
    index_rows = science_index_readiness_rows(
        science_store,
        known_alignment_authorities=known_alignment_authorities,
        metagenome_alignment_authorities=metagenome_alignment_authorities,
    )
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
        path = science_store / rel_path
        if rel_path == "indexes/known_targets.tsv" and standard_header_only_projection_ready(
            path,
            "known_targets",
            index_build_rows,
            known_alignment_ready,
            compact_ready=ligand_similarity_index_ready(science_store, summary_cache=summary_cache),
        ):
            continue
        if rel_path == "indexes/metagenome_targets.tsv" and standard_header_only_projection_ready(
            path,
            "metagenome_targets",
            index_build_rows,
            metagenome_alignment_ready,
        ):
            continue
        if not tsv_has_data_rows(path):
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
                required.extend(["scope_digest", "scope_mode", "scope_status"])
                if row.get("scope_status") == "ready_partial_on_demand":
                    required.extend(
                        [
                            "indexed_source_listing_sha256",
                            "indexed_validated_ids_manifest",
                            "indexed_validated_ids_sha256",
                            "indexed_mapping_sha256",
                            "indexed_subset_digest",
                            "indexed_pdb_count",
                            "indexed_entries",
                        ]
                    )
            manifest_invalid = bool(
                row.get("status") != "ready"
                or row.get("manifest_schema_version") != ALIGNMENT_MANIFEST_SCHEMA_VERSION
                or any(not row.get(field, "") for field in required)
            )
            if manifest_invalid:
                block(
                    f"alignment_manifest_invalid:{tool}:{row.get('source_id', 'unknown')}",
                    "alignment",
                    rel_path,
                    "migrate_or_rebuild_alignment_database",
                )
                continue
            if tool == "foldseek" and row.get("scope_status") == "ready_partial_on_demand":
                db_path = resolve_manifest_path(science_store, row.get("db_path", ""))
                validation = (
                    foldseek_partial_manifest_validation(
                        science_store,
                        row,
                        db_path,
                        probe_database=False,
                    )
                    if db_path is not None
                    else {"status": "failed", "reason": "foldseek_partial_database_path_missing"}
                )
                if validation.get("status") != "ready":
                    block(
                        f"alignment_manifest_invalid:{tool}:{row.get('source_id', 'unknown')}",
                        "alignment",
                        str(validation.get("reason", "foldseek_partial_validation_failed")),
                        "migrate_or_rebuild_alignment_database",
                    )

    rcsb_root = science_store / "downloads/rcsb/mmcif"
    rcsb_snapshot = read_json_object(science_store / "downloads/rcsb/ligand_bound_pdb_scope.json")
    rcsb_source_summary = resource_payload_summary(rcsb_root, cache=summary_cache)
    rcsb_partials = [
        path
        for path in rcsb_root.rglob("*")
        if path.is_file() and (path.name.lower().endswith(".part") or ".part.quarantine-" in path.name.lower())
    ] if rcsb_root.is_dir() else []
    rcsb_resumable_partials = [path for path in rcsb_partials if partial_has_resumable_identity(path)]
    rcsb_obsolete_partials = [path for path in rcsb_partials if path not in rcsb_resumable_partials]
    try:
        rcsb_scope_count = int(rcsb_snapshot.get("scope_count", 0) or 0)
        rcsb_ready_count = int(rcsb_snapshot.get("ready_count", 0) or 0)
        rcsb_missing_count = int(rcsb_snapshot.get("missing_count", 0) or 0)
    except (TypeError, ValueError):
        rcsb_scope_count = 0
        rcsb_ready_count = 0
        rcsb_missing_count = 0
    rcsb_mode = str(rcsb_snapshot.get("scope_mode", "")).replace("_", "-")
    rcsb_execution = external_execution_rows.get("rcsb_ligand_bound", {})
    rcsb_validation = rcsb_scope_snapshot_validation(
        science_store,
        execution=rcsb_execution,
        check_current_scope=True,
    )
    snapshot_common_valid = bool(
        rcsb_validation.get("status") in {"ready_partial_on_demand", "ready_complete"}
        and int(rcsb_source_summary.get("file_count", 0) or 0) >= rcsb_ready_count
        and int(rcsb_source_summary.get("bytes", 0) or 0) >= int(rcsb_snapshot.get("coordinate_bytes", 0) or 0)
    )
    rcsb_refresh_needed = bool(
        snapshot_common_valid
        and (
            rcsb_validation.get("refresh_needed")
            or rcsb_snapshot.get("coordinate_files") != rcsb_source_summary.get("file_count")
            or rcsb_snapshot.get("coordinate_bytes") != rcsb_source_summary.get("bytes")
            or rcsb_validation.get("source_digest") != rcsb_source_summary.get("sha256")
        )
    )
    if rcsb_mode == "on-demand":
        rcsb_scope_valid = snapshot_common_valid and rcsb_validation.get("status") == "ready_partial_on_demand"
    else:
        rcsb_scope_valid = bool(
            snapshot_common_valid
            and rcsb_mode == "complete"
            and rcsb_validation.get("status") == "ready_complete"
            and rcsb_ready_count == rcsb_scope_count
            and not rcsb_refresh_needed
        )
    if not rcsb_scope_valid:
        block(
            "rcsb_scope_incomplete",
            "rcsb",
            f"ready={rcsb_snapshot.get('ready_count', 0)} scope={rcsb_snapshot.get('scope_count', 0)}",
            "sync_rcsb_ligand_bound_scope",
        )
    if rcsb_refresh_needed and rcsb_mode == "on-demand":
        advisories.append(
            {
                "code": "rcsb_foldseek_refresh_needed",
                "component": "rcsb",
                "evidence": "cached coordinate set changed after the published snapshot",
                "action": "refresh_foldseek_when_convenient",
            }
        )
    if rcsb_resumable_partials and rcsb_mode == "on-demand":
        advisories.append(
            {
                "code": "rcsb_partial_download_preserved",
                "component": "rcsb",
                "evidence": f"identity_protected_partial_files={len(rcsb_resumable_partials)}",
                "action": "resume_matching_coordinate_on_demand",
            }
        )
    if rcsb_obsolete_partials and rcsb_mode == "on-demand":
        advisories.append(
            {
                "code": "rcsb_partial_download_obsolete",
                "component": "rcsb",
                "evidence": f"untrusted_partial_files={len(rcsb_obsolete_partials)}",
                "action": "remove_untrusted_partial_downloads",
            }
        )
    elif rcsb_partials and rcsb_mode != "on-demand":
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
        "rcsb_scope_mode": rcsb_mode,
        "rcsb_refresh_needed": rcsb_refresh_needed,
        "advisories": advisories,
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
    readiness = evaluate_installer_science_store_readiness(
        science_store,
        persist=False,
        summary_cache=summary_cache,
        env=env,
        wsl_distro=wsl_distro,
        check_install_status=False,
        check_source_retention=False,
    )
    non_alignment_blockers = [
        item
        for item in readiness.get("blockers", [])
        if isinstance(item, dict) and item.get("component") != "alignment"
    ]
    if non_alignment_blockers:
        return False
    if not alignment_database_manifests_ready(science_store, env, wsl_distro, summary_cache):
        return False
    index_rows = readiness.get("indexes", [])
    if partial_legacy_index_manifest_present(
        science_store,
        index_rows if isinstance(index_rows, list) else None,
    ):
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
    ligand_similarity_index = payload.get("ligand_similarity_index")
    source_retention = payload.get("source_retention")
    science_store_readiness = payload.get("science_store_readiness")
    runtime_binary_manifest = payload.get("runtime_binary_manifest")
    download_status = downloads.get("status") if isinstance(downloads, dict) else None
    external_status = external_sources.get("status") if isinstance(external_sources, dict) else None
    resource_status = resources.get("status") if isinstance(resources, dict) else None
    tool_status = tool_environment.get("status") if isinstance(tool_environment, dict) else None
    source_manifest_status = source_manifests.get("status") if isinstance(source_manifests, dict) else None
    alignment_status = alignment_databases.get("status") if isinstance(alignment_databases, dict) else None
    forcefield_status = forcefields.get("status") if isinstance(forcefields, dict) else None
    ligand_similarity_status = (
        ligand_similarity_index.get("status") if isinstance(ligand_similarity_index, dict) else None
    )
    source_retention_status = (
        source_retention.get("status") if isinstance(source_retention, dict) else None
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
        and ligand_similarity_status == "ready"
        and source_retention_status == "ready"
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


def serialized_json_bytes(payload: dict[str, object]) -> bytes:
    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    if os.linesep != "\n":
        text = text.replace("\n", os.linesep)
    return text.encode("utf-8")


def canonical_install_status_payload(payload: dict[str, object]) -> dict[str, object]:
    status = install_user_summary(payload)
    ready = bool(payload.get("install_ready", False))
    progress = status.get("progress")
    readiness = payload.get("science_store_readiness")
    expected_progress = "ready" if ready else "needs_repair"
    if (status.get("status") == "ready") != ready:
        raise ValueError("canonical_install_status_ready_mismatch")
    if not isinstance(progress, dict) or progress.get("status") != expected_progress:
        raise ValueError("canonical_install_status_progress_mismatch")
    if ready and (
        not isinstance(readiness, dict)
        or readiness.get("status") != "ready"
        or readiness.get("ready") is not True
    ):
        raise ValueError("canonical_install_status_readiness_mismatch")
    return status


def bind_science_readiness_to_install_status(
    science_store: Path,
    readiness: dict[str, object],
    install_status: dict[str, object],
) -> dict[str, object]:
    install_status_digest = hashlib.sha256(serialized_json_bytes(install_status)).hexdigest()
    evidence, evidence_digest = science_readiness_evidence(
        science_store,
        {"install_status.json": install_status_digest},
    )
    return {
        **readiness,
        "evidence_manifest": evidence,
        "evidence_digest": evidence_digest,
    }


def restore_json_bytes(path: Path, previous: bytes | None) -> None:
    if previous is None:
        path.unlink(missing_ok=True)
        return
    with atomic_output_writer(path, newline="") as handle:
        handle.write(previous.decode("utf-8"))


def write_final_install_state(
    science_store: Path,
    install_status: dict[str, object],
    progress: dict[str, object],
    readiness: dict[str, object],
) -> None:
    ready = bool(install_status.get("install_ready", False))
    expected_progress = "ready" if ready else "needs_repair"
    if (install_status.get("status") == "ready") != ready:
        raise ValueError("final_install_status_ready_mismatch")
    if install_status.get("progress") != progress or progress.get("status") != expected_progress:
        raise ValueError("final_install_progress_mismatch")
    if ready and (readiness.get("status") != "ready" or readiness.get("ready") is not True):
        raise ValueError("final_install_readiness_mismatch")
    expected_status_digest = hashlib.sha256(serialized_json_bytes(install_status)).hexdigest()
    readiness_status_digest = next(
        (
            str(row.get("sha256", ""))
            for row in readiness.get("evidence_manifest", [])
            if isinstance(row, dict) and row.get("path") == "install_status.json"
        ),
        "",
    )
    if readiness_status_digest != expected_status_digest:
        raise ValueError("final_install_status_evidence_mismatch")

    records = [
        (science_store / "science_store_readiness.json", readiness),
        (science_store / "install_progress.json", progress),
        (science_store / "install_status.json", install_status),
    ]
    previous = {path: path.read_bytes() if path.is_file() else None for path, _payload in records}
    published: list[Path] = []
    publishing_status = {
        **install_status,
        "status": "publishing",
        "install_ready": False,
        "summary": "正在发布最终安装状态。",
        "next_action": "等待最终状态发布完成",
        "progress": {
            **progress,
            "status": "publishing",
            "message": "正在发布最终安装状态",
        },
    }
    try:
        status_path = science_store / "install_status.json"
        write_json_atomic(status_path, publishing_status)
        published.append(status_path)
        for path, record in records:
            write_json_atomic(path, record)
            if path not in published:
                published.append(path)
    except BaseException:
        attempts = {
            path.with_name(f"{path.name}.attempt.json"): path.with_name(
                f"{path.name}.attempt.json"
            ).read_bytes()
            for path, _record in records
            if path.with_name(f"{path.name}.attempt.json").is_file()
        }
        for path in reversed(published):
            restore_json_bytes(path, previous[path])
        for path, content in attempts.items():
            restore_json_bytes(path, content)
        raise


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
    download_status = str(download_result.get("status", "unknown"))
    external_status = str(
        external_source_summary.get("status") or external_sync_result.get("status", "unknown")
    )
    resource_status = str(resource_result.get("status", "unknown"))
    tool_status = str(tool_environment_result.get("status", "unknown"))
    alignment_status = str(alignment_database_result.get("status", "unknown"))
    forcefield_status = str(forcefield_result.get("status", "unknown"))
    source_manifest_status = str(source_manifest_result.get("status", "unknown"))
    current_components_ready = bool(
        resource_status == "ready"
        and external_status == "ready"
        and tool_status == "ready"
        and alignment_status == "ready"
        and forcefield_status == "ready"
        and source_manifest_status == "ready"
    )
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
        and current_components_ready
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
    else:
        status = "ready"
        summary = "科学仓库已就绪。"
        next_action = "打开 WebUI 并输入研究对象"
    return {
        "status": status,
        "summary": summary,
        "next_action": next_action,
        "missing_components_count": 0 if status == "ready" else 1,
    }


def existing_path_for_disk_usage(path: Path) -> Path:
    current = path
    while not current.exists() and current.parent != current:
        current = current.parent
    return current


def same_storage_volume(left: Path, right: Path) -> bool:
    left_existing = existing_path_for_disk_usage(left).resolve()
    right_existing = existing_path_for_disk_usage(right).resolve()
    if os.name == "nt":
        return left_existing.drive.casefold() == right_existing.drive.casefold()
    return left_existing.stat().st_dev == right_existing.stat().st_dev


def capacity_item(
    component: str,
    action: str,
    download_bytes: int | None,
    staging_bytes: int,
    reclaimable_bytes: int,
    available_bytes: int,
) -> dict[str, object]:
    known_download_bytes = max(0, int(download_bytes)) if download_bytes is not None else 0
    required_peak = (
        max(0, known_download_bytes + staging_bytes - reclaimable_bytes) + DISK_SAFETY_MARGIN_BYTES
        if download_bytes is not None
        else None
    )
    return {
        "component": component,
        "action": action,
        "download_bytes": known_download_bytes if download_bytes is not None else None,
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
    rcsb_mode: str = "on-demand",
) -> dict[str, object]:
    mode = normalize_rcsb_mode(rcsb_mode)
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
    if mode == "on-demand":
        local_mean = (
            (sum(ready_sizes) + len(ready_sizes) - 1) // len(ready_sizes)
            if ready_sizes
            else 0
        )
        return {
            "action": "reuse_partial_scope_on_demand",
            "download_bytes": 0,
            "scope_mode": "on_demand",
            "scope_objects": len(scope),
            "missing_objects": len(missing_ids),
            "observed_local_files": len(ready_sizes),
            "observed_local_mean_bytes": local_mean,
            "remote_sample_files": 0,
            "remote_sample_mean_bytes": 0,
            "estimated_object_bytes": 0,
        }
    if not missing_ids:
        scope_ready = bool(scope)
        return {
            "action": "reuse_after_scope_validation" if scope_ready else "resolve_remote_size_before_download",
            "download_bytes": 0 if scope_ready else None,
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
        download_bytes = None
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
    scratch_dir: Path | None = None,
    scratch_available_bytes: int | None = None,
    resolve_remote: bool = False,
    summary_cache: ResourceSummaryCache | None = None,
    env: dict[str, str] | None = None,
    wsl_distro: str | None = None,
    rcsb_mode: str = "on-demand",
) -> dict[str, object]:
    scratch_root = Path(scratch_dir) if scratch_dir is not None else default_installer_scratch_dir()
    shared_scratch_volume = same_storage_volume(science_store, scratch_root)
    available = (
        int(available_bytes)
        if available_bytes is not None
        else int(shutil.disk_usage(existing_path_for_disk_usage(science_store)).free)
    )
    scratch_available = (
        int(scratch_available_bytes)
        if scratch_available_bytes is not None
        else available
        if shared_scratch_volume
        else int(shutil.disk_usage(existing_path_for_disk_usage(scratch_root)).free)
    )
    items: list[dict[str, object]] = []
    for row in read_tsv(source_catalog):
        destination = science_store / row.get("local_path", "")
        expected = row.get("sha256", "")
        retained = retained_source_ready(
            science_store,
            row.get("source_id", ""),
            row.get("local_path", ""),
            summary_cache,
            expected_release_identity=row.get("release_identity", ""),
            expected_trust_source=row.get("trust_source", ""),
            expected_sha256=expected,
        )
        destination_valid = bool(retained) or (
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
            action = "reuse_verified_index" if retained else "reuse"
            download_bytes = 0
        else:
            identity = remote_resource_identity(row.get("url", "")) if resolve_remote else {}
            remote_bytes = int(identity.get("content_length", 0) or 0)
            download_bytes = remote_bytes if remote_bytes > 0 else None
            action = "download" if download_bytes is not None else "resolve_remote_size_before_download"
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
        if verified_external_retention(science_store, row, summary_cache):
            items.append(capacity_item(source_id, "reuse_verified_index", 0, 0, 0, available))
            continue
        if external_plan_row_ready(
            row,
            target,
            marker,
            science_store,
            previous_external.get(source_id, {}),
            summary_cache,
            rcsb_mode=rcsb_mode,
        ):
            items.append(capacity_item(source_id, "reuse_after_digest", 0, 0, 0, available))
            continue
        download_bytes: int | None = None
        action = "resolve_remote_size_before_download"
        capacity_evidence: dict[str, object] = {}
        if method == "html_links" and resolve_remote:
            inventory_result = (
                publisher_inventory_with_cache(row, science_store, html_links_inventory)
                if row.get("checksum_url", "").strip()
                else publisher_capacity_inventory_with_retry(row, html_links_inventory)
            )
            capacity_evidence = {
                "inventory_source": inventory_result["source"],
                "inventory_attempts": inventory_result["attempts"],
                "inventory_error": inventory_result["error"],
                "source_url": inventory_result["source_url"],
            }
            inventory = inventory_result["items"]
            if inventory:
                estimate = publisher_inventory_capacity(inventory, target)
                download_bytes = int(estimate["download_bytes"])
                action = str(estimate["action"])
                capacity_evidence.update(estimate)
        elif method == "gcs_prefix" and resolve_remote:
            try:
                estimate = publisher_inventory_capacity(gcs_prefix_inventory(row), target)
                action = str(estimate["action"])
                download_bytes = (
                    int(estimate["download_bytes"])
                    if action != "resolve_remote_size_before_download"
                    else None
                )
                capacity_evidence = estimate
            except (OSError, urllib.error.URLError, urllib.error.HTTPError, ValueError, json.JSONDecodeError):
                pass
        elif method == "rcsb_ligand_bound_scope":
            estimate = rcsb_capacity_estimate(
                row,
                target,
                science_store,
                resolve_remote=resolve_remote,
                rcsb_mode=rcsb_mode,
            )
            raw_download_bytes = estimate["download_bytes"]
            download_bytes = int(raw_download_bytes) if raw_download_bytes is not None else None
            action = str(estimate["action"])
            capacity_evidence = estimate
        elif method == "direct" and resolve_remote:
            identity = remote_resource_identity(direct_source_location(row))
            remote_bytes = int(identity.get("content_length", 0) or 0)
            download_bytes = remote_bytes if remote_bytes > 0 else None
            action = "download" if download_bytes is not None else action
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
    alignment_specs: list[tuple[str, str, Path, Path]] = [
        (
            "mmseqs",
            source_id,
            source,
            science_store / "indexes/mmseqs" / safe_file_stem(source_id) / "target",
        )
        for source_id, source in sequence_database_sources(science_store)
    ]
    alignment_specs.append(
        (
            "foldseek",
            "rcsb_mmcif",
            science_store / "downloads/rcsb/mmcif",
            science_store / "indexes/foldseek/rcsb_mmcif/target",
        )
    )
    for tool, source_id, source, db_path in alignment_specs:
        if not path_has_payload(source):
            continue
        previous = manifest_rows.get(source_id, {})
        recovery = reconcile_database_directory_publish(db_path)
        if recovery.get("status") != "ready":
            items.append(
                capacity_item(
                    f"alignment:{source_id}",
                    str(recovery.get("reason", "database_directory_recovery_needs_repair")),
                    0,
                    0,
                    0,
                    available,
                )
            )
            continue
        db_bytes = directory_size(db_path.parent)
        command = shutil.which(tool, path=(env or os.environ).get("PATH"))
        builder_version = database_builder_version(tool, command, env, wsl_distro)
        migrated = legacy_alignment_manifest_can_migrate(
            tool,
            previous,
            science_store,
            source,
            db_path,
            command,
            env,
            wsl_distro,
            builder_version=builder_version,
        )
        partial_candidate = bool(
            tool == "foldseek"
            and normalize_rcsb_mode(rcsb_mode) == "on-demand"
            and database_artifacts_present(db_path)
            and Path(f"{db_path}.source").is_file()
        )
        if partial_candidate:
            action = "migrate_partial_on_demand_after_probe"
            staging = 0
        elif previous.get("status") == "ready" and previous.get("manifest_schema_version") == ALIGNMENT_MANIFEST_SCHEMA_VERSION:
            action = "reuse_after_probe"
            staging = 0
        elif migrated:
            action = "migrate_after_probe"
            staging = 0
        else:
            action = "rebuild"
            staging = max(db_bytes, directory_size(source) * 2)
        if tool == "foldseek":
            estimate = foldseek_staging_estimate(source, db_path, previous)
            target_staging = int(estimate["target_staging_bytes"]) if action == "rebuild" else 0
            scratch_staging = int(estimate["scratch_staging_bytes"]) if action == "rebuild" else 0
            item = capacity_item(
                f"alignment:{source_id}",
                action,
                0,
                target_staging,
                0,
                available,
            )
            item.update(
                {
                    "scratch_staging_bytes": scratch_staging,
                    "scratch_available_bytes": scratch_available,
                    "scratch_required_peak_bytes": (
                        scratch_staging + DISK_SAFETY_MARGIN_BYTES if scratch_staging else 0
                    ),
                    "target_staging_bytes": target_staging,
                    "target_available_bytes": available,
                    "target_required_peak_bytes": (
                        target_staging + DISK_SAFETY_MARGIN_BYTES if target_staging else 0
                    ),
                    "scratch_target_same_volume": shared_scratch_volume,
                    "source_files_for_estimate": estimate["source_files"],
                    "source_bytes_for_estimate": estimate["source_bytes"],
                }
            )
            items.append(item)
        else:
            items.append(capacity_item(f"alignment:{source_id}", action, 0, staging, 0, available))

    ligand_ready = ligand_similarity_index_ready(science_store, summary_cache=summary_cache)
    chembl_archive = science_store / "downloads/chembl/chembl_sqlite.tar.gz"
    chembl_query = science_store / "downloads/chembl/chembl_query.sqlite"
    chembl_manifest = chembl_query.with_suffix(".manifest.json")
    chembl_ready = False
    if chembl_archive.is_file() and chembl_archive.stat().st_size > 0 and chembl_query.is_file():
        catalog_row = next((row for row in read_tsv(source_catalog) if row.get("source_id") == "chembl"), {})
        source_release = catalog_row.get("release_identity") or catalog_row.get("release_or_date") or "unknown"
        source_digest = str(resource_payload_summary(chembl_archive, cache=summary_cache).get("sha256", ""))
        chembl_ready = chembl_query_database_ready(
            chembl_query,
            chembl_manifest,
            source_digest,
            source_release,
            summary_cache,
        )
    if (
        not ligand_ready
        and chembl_archive.is_file()
        and chembl_archive.stat().st_size > 0
        and not chembl_ready
    ):
        source_staging = 0
        try:
            with tarfile.open(chembl_archive, "r:gz") as archive:
                source_staging = max(
                    (
                        int(item.size)
                        for item in archive.getmembers()
                        if item.isfile() and item.name.lower().endswith((".db", ".sqlite"))
                    ),
                    default=0,
                )
        except tarfile.TarError:
            source_staging = chembl_archive.stat().st_size * 4
        target_staging = chembl_query_target_staging_bytes(source_staging, chembl_archive.stat().st_size)
        chembl_item = capacity_item(
            "chembl_query_database",
            "extract_to_scratch_and_build_filtered_query_database",
            0,
            target_staging,
            0,
            available,
        )
        chembl_item.update(
            {
                "scratch_staging_bytes": source_staging,
                "scratch_available_bytes": scratch_available,
                "scratch_required_peak_bytes": source_staging + DISK_SAFETY_MARGIN_BYTES,
                "target_staging_bytes": target_staging,
                "target_available_bytes": available,
                "target_required_peak_bytes": target_staging + DISK_SAFETY_MARGIN_BYTES,
                "scratch_target_same_volume": shared_scratch_volume,
            }
        )
        items.append(chembl_item)

    ligand_sources = [
        science_store / "downloads/chembl/chembl_query.sqlite",
        science_store / "indexes/bindingdb/bindingdb_query.sqlite",
    ]
    legacy_ligand_bytes = sum(path.stat().st_size for path in ligand_sources if path.is_file())
    if not ligand_ready:
        staging = max(legacy_ligand_bytes * 2, 5 * 1024 * 1024 * 1024)
        item = capacity_item(
            "ligand_similarity_index",
            "build_compact_index_then_remove_legacy_query_databases",
            0,
            staging,
            0,
            available,
        )
        item.update(
            {
                "source_query_bytes": legacy_ligand_bytes,
                "post_publish_reclaimable_bytes": legacy_ligand_bytes,
                "target_staging_bytes": staging,
                "target_required_peak_bytes": staging + DISK_SAFETY_MARGIN_BYTES,
                "net_growth_allowed": False,
            }
        )
        items.append(item)
    elif legacy_ligand_bytes:
        item = capacity_item(
            "ligand_similarity_index",
            "remove_superseded_query_databases",
            0,
            0,
            0,
            available,
        )
        item["post_publish_reclaimable_bytes"] = legacy_ligand_bytes
        items.append(item)

    unresolved = [item for item in items if item["action"] == "resolve_remote_size_before_download"]
    known_download = sum(
        int(item["download_bytes"])
        for item in items
        if item["download_bytes"] is not None
    )
    peak_staging = max((int(item["staging_bytes"]) for item in items), default=0)
    scratch_peak_staging = max((int(item.get("scratch_staging_bytes", 0)) for item in items), default=0)
    reclaimable = sum(int(item["reclaimable_bytes"]) for item in items)
    target_required_peak = max(0, known_download + peak_staging - reclaimable) + DISK_SAFETY_MARGIN_BYTES
    scratch_required_peak = (
        scratch_peak_staging + DISK_SAFETY_MARGIN_BYTES if scratch_peak_staging > 0 else 0
    )
    required_peak = (
        max(0, known_download + peak_staging + scratch_peak_staging - reclaimable)
        + DISK_SAFETY_MARGIN_BYTES
        if shared_scratch_volume
        else target_required_peak
    )
    capacity_failures = []
    if required_peak > available:
        capacity_failures.append("target")
    if not shared_scratch_volume and scratch_required_peak > scratch_available:
        capacity_failures.append("scratch")
    status = "blocked" if unresolved or capacity_failures else "ready"
    payload = {
        "schema_version": "genoligand_installation_capacity_plan_v1",
        "status": status,
        "items": items,
        "download_bytes": None if unresolved else known_download,
        "known_download_bytes": known_download,
        "staging_bytes": peak_staging,
        "target_staging_bytes": peak_staging,
        "scratch_staging_bytes": scratch_peak_staging,
        "reclaimable_bytes": reclaimable,
        "required_peak_bytes": required_peak,
        "target_required_peak_bytes": target_required_peak,
        "scratch_required_peak_bytes": scratch_required_peak,
        "required_peak_is_lower_bound": bool(unresolved),
        "available_bytes": available,
        "target_available_bytes": available,
        "scratch_available_bytes": scratch_available,
        "scratch_target_same_volume": shared_scratch_volume,
        "capacity_failures": capacity_failures,
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
    parser.add_argument("--scratch-dir")
    parser.add_argument("--rcsb-mode", choices=["on-demand", "complete"], default="on-demand")
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
    scratch_dir = (
        Path(args.scratch_dir).resolve()
        if args.scratch_dir
        else default_installer_scratch_dir().resolve()
    )
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
    write_capacity_attempt_state(science_store, status="running")
    try:
        resource_summary_cache = load_resource_summary_cache(fingerprint_cache_path)
        reconcile_interrupted_source_retention(science_store, resource_summary_cache)
        capacity_plan = installation_capacity_plan(
            science_store,
            source_catalog,
            source_sync_plan,
            scratch_dir=scratch_dir,
            resolve_remote=not args.no_download,
            summary_cache=resource_summary_cache,
            wsl_distro=args.wsl_distro,
            rcsb_mode=args.rcsb_mode,
        )
        save_resource_summary_cache(fingerprint_cache_path, resource_summary_cache)
    except Exception as err:  # noqa: BLE001 - persist a terminal preflight result
        error = f"{type(err).__name__}:{err}"
        write_capacity_attempt_state(science_store, status="failed", error=error)
        print(
            json.dumps(
                {
                    "schema_version": "genoligand_installation_capacity_plan_v1",
                    "status": "failed",
                    "error": error,
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 2
    if capacity_plan.get("status") != "ready":
        terminal_status = "failed" if capacity_plan.get("status") == "failed" else "blocked"
        write_capacity_attempt_state(science_store, status=terminal_status, capacity_plan=capacity_plan)
        print(json.dumps(capacity_plan, ensure_ascii=False, indent=2))
        return 2
    if args.dry_run:
        write_capacity_attempt_state(science_store, status="capacity_ready", capacity_plan=capacity_plan)
        print(json.dumps(capacity_plan, ensure_ascii=False, indent=2))
        return 0
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
    inherited_path = os.environ.get("PATH", "")
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
        rcsb_mode=args.rcsb_mode,
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
        scratch_dir=scratch_dir,
        rcsb_mode=args.rcsb_mode,
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
    existing_ligand_similarity_ready = ligand_similarity_index_ready(
        science_store,
        summary_cache=resource_summary_cache,
    )
    pre_prepare_ligand_retention = (
        migrate_ligand_source_retention(science_store, resource_summary_cache)
        if existing_ligand_similarity_ready
        else {
            "status": "not_applicable",
            "updated": 0,
            "reason": "ligand_similarity_index_not_ready",
        }
    )
    chembl_query_result = (
        {
            "status": "ready",
            "reason": "chembl_query_superseded_by_compact_ligand_index",
            "database": str(science_store / LIGAND_SIMILARITY_DATABASE_REL_PATH),
        }
        if existing_ligand_similarity_ready
        else publish_chembl_query_database(
            science_store,
            source_catalog,
            resource_summary_cache,
            scratch_dir=scratch_dir,
        )
        if alignment_database_result.get("status") == "ready"
        else {
            "status": "needs_repair",
            "reason": "chembl_query_deferred_until_alignment_ready",
        }
    )
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
        current_resource="紧凑配体相似性索引",
        completed_steps=5,
        total_steps=6,
        message="正在合并去重配体库并建立精确相似性索引",
    )
    ligand_similarity_result = (
        prepare_ligand_similarity_index(
            workspace_root,
            science_store,
            tool_python,
            python_backend,
            selected_wsl,
            resource_summary_cache,
        )
        if completed.returncode == 0 and chembl_query_result.get("status") == "ready"
        else {
            "status": "failed",
            "rebuilt": False,
            "reason": "ligand_similarity_deferred_until_source_indexes_ready",
        }
    )
    ligand_retention_migration = (
        pre_prepare_ligand_retention
        if ligand_similarity_result.get("status") == "ready"
        and pre_prepare_ligand_retention.get("status") == "ready"
        else migrate_ligand_source_retention(science_store, resource_summary_cache)
        if ligand_similarity_result.get("status") == "ready"
        else {"status": "needs_repair", "updated": 0, "reason": "ligand_similarity_index_not_ready"}
    )
    ligand_query_cleanup = (
        prune_superseded_ligand_query_databases(science_store, resource_summary_cache)
        if ligand_similarity_result.get("status") == "ready"
        and ligand_retention_migration.get("status") == "ready"
        else {"status": "needs_repair", "reclaimed_bytes": 0, "reason": "ligand_storage_migration_not_ready"}
    )
    index_authority_refresh = refresh_science_index_authority_manifest(
        science_store,
        resource_summary_cache,
    )
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
    pre_retention_readiness = evaluate_installer_science_store_readiness(
        science_store,
        persist=False,
        summary_cache=resource_summary_cache,
        env=command_env,
        wsl_distro=selected_wsl,
        check_install_status=False,
        check_source_retention=False,
    )
    source_retention_result = (
        prune_indexed_source_payloads(science_store, resource_summary_cache)
        if completed.returncode == 0
        and alignment_database_result.get("status") == "ready"
        and chembl_query_result.get("status") == "ready"
        and ligand_similarity_result.get("status") == "ready"
        and ligand_retention_migration.get("status") == "ready"
        and ligand_query_cleanup.get("status") == "ready"
        and forcefield_result.get("status") == "ready"
        and pre_retention_readiness.get("status") == "ready"
        else {
            "status": "needs_repair",
            "ready": 0,
            "total": len(INDEXED_SOURCE_RETENTION_POLICIES),
            "reclaimed_bytes": 0,
            "failures": ["source_cleanup_deferred_until_all_indexes_are_ready"],
        }
    )
    rcsb_cache_result = (
        prune_rcsb_on_demand_cache(science_store, resource_summary_cache)
        if source_retention_result.get("status") == "ready"
        else {
            "status": "needs_repair",
            "reclaimed_bytes": 0,
            "reason": "rcsb_cache_cleanup_deferred_until_source_cleanup_ready",
        }
    )
    source_retention_result["rcsb_cache"] = rcsb_cache_result
    source_retention_result["ligand_query_cleanup"] = ligand_query_cleanup
    source_retention_result["ligand_retention_migration"] = ligand_retention_migration
    source_retention_result["reclaimed_bytes"] = int(
        source_retention_result.get("reclaimed_bytes", 0) or 0
    ) + int(rcsb_cache_result.get("reclaimed_bytes", 0) or 0) + int(
        ligand_query_cleanup.get("reclaimed_bytes", 0) or 0
    )
    if (
        rcsb_cache_result.get("status") != "ready"
        or ligand_query_cleanup.get("status") != "ready"
        or ligand_retention_migration.get("status") != "ready"
    ):
        source_retention_result["status"] = "needs_repair"
    save_resource_summary_cache(fingerprint_cache_path, resource_summary_cache)
    external_source_summary = source_sync_summary(
        source_sync_plan,
        science_store,
        external_sync_result,
        resource_summary_cache,
        rcsb_mode=args.rcsb_mode,
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
        persist=False,
        summary_cache=resource_summary_cache,
        env=command_env,
        wsl_distro=selected_wsl,
        check_install_status=False,
    )
    save_resource_summary_cache(fingerprint_cache_path, resource_summary_cache)
    worker_wrapper = Path(tool_python) if tool_python and Path(tool_python).is_file() else None
    runtime_binary_manifest = write_runtime_binary_manifest(
        runtime_dir,
        [cli_runtime_status, server_runtime_status],
        worker_wrapper,
    )
    if (
        ligand_similarity_result.get("status") != "ready"
        or ligand_query_cleanup.get("status") != "ready"
        or source_retention_result.get("status") != "ready"
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
        "ligand_similarity_index": ligand_similarity_result,
        "ligand_storage_migration": {
            "retention": ligand_retention_migration,
            "cleanup": ligand_query_cleanup,
        },
        "source_retention": source_retention_result,
        "index_authority_refresh": index_authority_refresh,
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
    payload["progress"] = install_progress_payload(
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
        inherited_path,
    )
    payload["launch_environment"] = launch_environment
    canonical_install_status = canonical_install_status_payload(payload)
    science_store_readiness = bind_science_readiness_to_install_status(
        science_store,
        science_store_readiness,
        canonical_install_status,
    )
    payload["science_store_readiness"] = science_store_readiness
    try:
        write_final_install_state(
            science_store,
            canonical_install_status,
            payload["progress"],
            science_store_readiness,
        )
    except (OSError, UnicodeError, ValueError) as err:
        release_install_lock(install_lock)
        install_lock = None
        print(
            json.dumps(
                {
                    "status": "failed",
                    "summary": "最终安装状态发布失败。",
                    "next_action": "修复状态文件写入错误后重新运行安装/修复",
                    "error": exception_note(err),
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 2
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
