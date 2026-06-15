from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
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


def default_source_catalog(workspace_root: Path, science_store: Path) -> Path:
    store_catalog = science_store / "source_catalog.tsv"
    if store_catalog.exists():
        return store_catalog
    repo_catalog = workspace_root / "configs" / "source_catalog.tsv"
    if repo_catalog.exists():
        return repo_catalog
    return store_catalog


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
        request = urllib.request.Request(url)
        existing = partial.stat().st_size if partial.exists() else 0
        if existing:
            request.add_header("Range", f"bytes={existing}-")
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                mode = "ab" if existing and response.status == 206 else "wb"
                with partial.open(mode) as handle:
                    shutil.copyfileobj(response, handle)
        except urllib.error.HTTPError as err:
            if existing and err.code == 416:
                pass
            else:
                raise
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


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Prepare or validate the GenoLigand local science store."
    )
    parser.add_argument("--workspace-root", default=str(repo_root()))
    parser.add_argument("--science-store", default=str(default_science_store()))
    parser.add_argument("--runtime-dir", default=str(default_runtime_dir()))
    parser.add_argument("--gls-cli")
    parser.add_argument("--gls-server")
    parser.add_argument("--source-catalog")
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default="19090")
    parser.add_argument("--start-webui", action="store_true")
    args = parser.parse_args()

    workspace_root = Path(args.workspace_root).resolve()
    science_store = Path(args.science_store).resolve()
    runtime_dir = Path(args.runtime_dir).resolve()
    source_catalog = Path(args.source_catalog).resolve() if args.source_catalog else default_source_catalog(workspace_root, science_store)
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
        "downloads": download_result,
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
