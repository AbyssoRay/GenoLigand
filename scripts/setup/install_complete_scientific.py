from __future__ import annotations

import argparse
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
from typing import Sequence

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

RESERVED_WSL_DISTROS = {"Ubuntu-24.04"}
DOWNLOAD_RETRIES = 3
DOWNLOAD_TIMEOUT_SECONDS = 120
DOWNLOAD_USER_AGENT = "GenoLigand-science-store-installer/0.9"


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
        root / "target" / "release" / executable,
        root / "target" / "debug" / executable,
    ]:
        if candidate.exists():
            return [str(candidate)]
    on_path = shutil.which(binary_name)
    if on_path:
        return [on_path]
    if (root / "Cargo.toml").exists():
        cargo = shutil.which("cargo")
        if cargo:
            return [cargo, "run", "-p", cargo_package, "--"]
    return ["cargo", "run", "-p", cargo_package, "--"]


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


def environment_for_science_tools(
    workspace_root: Path,
    extra_entries: Sequence[Path] | None = None,
) -> dict[str, str]:
    env = os.environ.copy()
    entries = tool_path_entries(workspace_root, extra_entries)
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
    for encoding in ("utf-16le", "utf-8", "gbk"):
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


def write_wsl_python_wrapper(wrapper: Path, distro: str, python_path: str) -> None:
    wrapper.parent.mkdir(parents=True, exist_ok=True)
    safe_distro = distro.replace('"', "")
    safe_python = python_path.replace('"', "")
    wrapper.write_text(
        f'@echo off\r\nwsl.exe -d "{safe_distro}" -- "{safe_python}" %*\r\n',
        encoding="utf-8",
    )


def write_wsl_command_wrappers(wrapper_dir: Path, distro: str) -> dict[str, object]:
    wrapper_dir.mkdir(parents=True, exist_ok=True)
    safe_distro = distro.replace('"', "")
    wrappers = []
    for command_name in WSL_COMMAND_WRAPPERS:
        wrapper = wrapper_dir / f"{command_name}.cmd"
        wrapper.write_text(
            f'@echo off\r\nwsl.exe -d "{safe_distro}" -- {command_name} %*\r\n',
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
                "total": len(PYTHON_TOOL_PACKAGES),
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
                "total": len(PYTHON_TOOL_PACKAGES),
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
    write_python_tool_manifest(manifest, rows)
    total = len(PYTHON_TOOL_PACKAGES)
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
            "total": len(PYTHON_TOOL_PACKAGES),
            "environment": "wsl_unavailable",
        }
    python_path = f"{home}/.local/state/genoligand/python-worker-env/bin/python"
    write_wsl_python_wrapper(wrapper, distro, python_path)
    if skip_install:
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
        write_python_tool_manifest(manifest, rows)
        return {
            "status": "skipped",
            "backend": "wsl",
            "wsl_distro": distro,
            "python": str(wrapper),
            "manifest": str(manifest),
            "ready": 0,
            "total": len(PYTHON_TOOL_PACKAGES),
            "environment": "skipped",
        }
    create = run_wsl_command(
        distro,
        [
            "bash",
            "-lc",
            "python3 -m venv ~/.local/state/genoligand/python-worker-env && ~/.local/state/genoligand/python-worker-env/bin/python -m pip install --upgrade pip",
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
            "total": len(PYTHON_TOOL_PACKAGES),
            "environment": "failed",
        }
    ready = 0
    rows: list[dict[str, object]] = []
    for spec in PYTHON_TOOL_PACKAGES:
        probe = probe_wsl_python_import(distro, python_path, spec["import_name"], spec["version_expr"])
        status = "ready" if probe["ready"] else "missing"
        notes = str(probe["notes"])
        if not probe["ready"]:
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
    write_python_tool_manifest(manifest, rows)
    total = len(PYTHON_TOOL_PACKAGES)
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


def copy_or_download(url: str, destination: Path, expected_sha256: str | None) -> dict[str, object]:
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_suffix(destination.suffix + ".part")
    if url.startswith("file://"):
        source = Path(urllib.request.url2pathname(url[7:]))
        shutil.copyfile(source, partial)
    elif Path(url).exists():
        shutil.copyfile(Path(url), partial)
    else:
        last_error: Exception | None = None
        for attempt in range(1, DOWNLOAD_RETRIES + 1):
            existing = partial.stat().st_size if partial.exists() else 0
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
                    with partial.open(mode) as handle:
                        shutil.copyfileobj(response, handle)
                last_error = None
                break
            except urllib.error.HTTPError as err:
                if existing and err.code == 416:
                    last_error = None
                    break
                last_error = err
            except (urllib.error.URLError, TimeoutError, OSError) as err:
                last_error = err
            if attempt < DOWNLOAD_RETRIES:
                time.sleep(min(2 * attempt, 5))
        if last_error is not None:
            raise last_error
    actual_sha256 = sha256_file(partial)
    if expected_sha256 and actual_sha256.lower() != expected_sha256.lower():
        return {
            "status": "checksum_failed",
            "bytes": partial.stat().st_size,
            "sha256": actual_sha256,
        }
    partial.replace(destination)
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
            url = row.get("url", "")
            local_path = row.get("local_path", "")
            expected = row.get("sha256", "") or None
            if not source_id or not url or not local_path:
                failed += 1
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
                result = copy_or_download(url, destination, expected)
                if result["status"] == "ready":
                    ready += 1
                else:
                    failed += 1
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
        return {
            "status": "ready" if failed == 0 else "failed",
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


def sync_html_links(row: dict[str, str], target: Path) -> dict[str, object]:
    source_url = row.get("source_url") or row.get("download_url") or row.get("landing_url")
    include_pattern = row.get("include_pattern", "")
    if not source_url:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "html_source_url_required"}
    links = extract_html_links(source_url, include_pattern)
    if not links:
        return {"status": "failed", "bytes": 0, "sha256": "", "notes": "no_matching_links"}
    target.mkdir(parents=True, exist_ok=True)
    failures = []
    for link in links:
        destination = target / Path(urllib.parse.urlparse(link).path).name
        try:
            copy_or_download(link, destination, row.get("sha256", "") or None)
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


def sync_gcs_prefix(row: dict[str, str], target: Path) -> dict[str, object]:
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
            try:
                copy_or_download(media_url, destination, None)
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


def sync_external_row(row: dict[str, str], target: Path, env: dict[str, str] | None) -> dict[str, object]:
    method = (row.get("sync_method") or "direct").strip().lower()
    if method == "html_links":
        return sync_html_links(row, target)
    if method == "html_tree":
        return sync_html_tree(row, target)
    if method == "gcs_prefix":
        return sync_gcs_prefix(row, target)
    if method == "rsync":
        return sync_rsync(row, target, env)
    location = direct_source_location(row)
    if not location:
        return {"status": "requires_resource_bundle", "bytes": 0, "sha256": "", "notes": "direct_download_or_local_source_required"}
    destination = external_destination(target, location)
    result = copy_or_download(location, destination, row.get("sha256", "") or None)
    return {
        "status": result["status"],
        "bytes": result["bytes"],
        "sha256": result["sha256"],
        "notes": f"{method}_resource_synced",
    }


def sync_external_sources(
    plan: Path,
    science_store: Path,
    skip_downloads: bool,
    env: dict[str, str] | None = None,
) -> dict[str, object]:
    manifest = science_store / "external_sync_execution_manifest.tsv"
    rows = read_tsv(plan)
    manifest.parent.mkdir(parents=True, exist_ok=True)
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
            if marker and path_has_payload(marker):
                ready += 1
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
                result = sync_external_row(row, target, env)
                if result["status"] == "ready":
                    ready += 1
                elif result["status"] == "requires_resource_bundle":
                    needs_action += 1
                else:
                    failed += 1
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
        return {
            "status": status,
            "manifest": str(manifest),
            "ready": ready,
            "failed": failed,
            "needs_action": needs_action,
            "total": total,
        }


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


def write_science_resource_manifests(
    source_catalog: Path,
    source_sync_plan: Path,
    science_store: Path,
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
        for row in source_rows:
            source_id = row["source_id"] or "unknown_source"
            local_rel = row["local_path"]
            local_path = science_store / local_rel if local_rel else science_store / "__missing_local_path__"
            exists = path_has_payload(local_path)
            bytes_count = directory_size(local_path) if exists else 0
            digest = directory_digest(local_path) if exists else ""
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
                "file_count": str(file_count(local_path) if exists else 0),
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


def install_ready(payload: dict[str, object]) -> bool:
    downloads = payload.get("downloads")
    external_sources = payload.get("external_sources")
    resources = payload.get("resources")
    tool_environment = payload.get("tool_environment")
    source_manifests = payload.get("source_manifests")
    download_status = downloads.get("status") if isinstance(downloads, dict) else None
    external_status = external_sources.get("status") if isinstance(external_sources, dict) else None
    resource_status = resources.get("status") if isinstance(resources, dict) else None
    tool_status = tool_environment.get("status") if isinstance(tool_environment, dict) else None
    source_manifest_status = source_manifests.get("status") if isinstance(source_manifests, dict) else None
    return (
        payload.get("status") == "ready"
        and resource_status == "ready"
        and external_status == "ready"
        and tool_status == "ready"
        and download_status == "ready"
        and source_manifest_status == "ready"
    )


def installation_status(
    science_store: Path,
    completed: subprocess.CompletedProcess[str],
    download_result: dict[str, object],
    external_sync_result: dict[str, object],
    tool_environment_result: dict[str, object],
    source_manifest_result: dict[str, object],
) -> dict[str, object]:
    status_payload = load_json(science_store / "install_status.json")
    store_status = str(status_payload.get("status", "")).strip()
    download_status = str(download_result.get("status", "unknown"))
    external_status = str(external_sync_result.get("status", "unknown"))
    tool_status = str(tool_environment_result.get("status", "unknown"))
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
    elif download_status == "skipped":
        status = "needs_repair"
        summary = "本次未执行科学资源下载，科学仓库不能标记为完成。"
        next_action = "保持联网后重新运行安装/修复"
    elif tool_status != "ready":
        status = "needs_repair"
        summary = "Python 科学计算环境尚未准备完成。"
        next_action = "保持联网后重新运行安装/修复，或检查 Python 环境安装日志"
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
    python_env_dir = (
        Path(args.python_env_dir).resolve()
        if args.python_env_dir
        else default_python_env_dir(runtime_dir).resolve()
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
    external_sync_result = sync_external_sources(
        source_sync_plan,
        science_store,
        args.no_download,
        science_env,
    )
    source_manifest_result = write_science_resource_manifests(
        source_catalog,
        source_sync_plan,
        science_store,
    )
    tool_environment_result = ensure_python_tool_environment(
        python_env_dir,
        args.no_tool_install,
        python_backend,
        selected_wsl,
    )
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
    command_env = dict(science_env)
    tool_python = str(tool_environment_result.get("python", "")).strip()
    if tool_python:
        command_env["GENOLIGAND_PYTHON"] = tool_python
    completed = run_command(command, env=command_env)
    install_status = installation_status(
        science_store,
        completed,
        download_result,
        external_sync_result,
        tool_environment_result,
        source_manifest_result,
    )
    payload: dict[str, object] = {
        **install_status,
        "returncode": completed.returncode,
        "downloads": download_summary(download_result),
        "external_sources": source_sync_summary(
            source_sync_plan,
            science_store,
            external_sync_result,
        ),
        "source_manifests": source_manifest_result,
        "resources": resource_summary(science_store),
        "tool_environment": tool_environment_result,
        "runtime_backend": runtime_backend(),
        "access_url": f"http://{args.host}:{args.port}/",
    }
    payload["install_ready"] = install_ready(payload)
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
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    exit_code = 0 if payload["install_ready"] else 2
    if completed.returncode != 0:
        exit_code = completed.returncode
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
    env = dict(science_env)
    env["GENOLIGAND_SCIENCE_STORE"] = str(science_store)
    if tool_python:
        env["GENOLIGAND_PYTHON"] = tool_python
    print(
        json.dumps(
            {
                "status": "starting_webui",
                "summary": "WebUI 正在启动。",
                "access_url": f"http://{args.host}:{args.port}/",
            },
            ensure_ascii=False,
            indent=2,
        )
    )
    return subprocess.call(server_command, env=env)


if __name__ == "__main__":
    raise SystemExit(main())
