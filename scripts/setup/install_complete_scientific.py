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
        return [
            {key: (value or "").strip() for key, value in row.items()}
            for row in csv.DictReader(handle, delimiter="\t")
        ]


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


def source_sync_summary(plan: Path, science_store: Path) -> dict[str, object]:
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
    items: list[dict[str, object]] = []
    for row in rows:
        target_rel = row.get("ready_marker") or row.get("target_rel_path") or ""
        target = science_store / target_rel if target_rel else None
        ready = bool(target and target.exists())
        items.append(
            {
                "label": row.get("source_label") or row.get("source_id") or "科学资源",
                "status": "就绪" if ready else "需要同步",
                "ready": ready,
                "next_action": row.get("user_next_action") or "运行安装/修复以同步科学资源",
            }
        )
    ready_count = sum(1 for item in items if item["ready"])
    total = len(items)
    needs_action = total - ready_count
    return {
        "status": "ready" if total and needs_action == 0 else "needs_sync",
        "ready": ready_count,
        "total": total,
        "needs_action": needs_action,
        "message": "大型科学资源同步状态已检查。",
        "next_action": "继续" if needs_action == 0 else "保持联网和足够磁盘空间后运行安装/修复",
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


def installation_status(
    science_store: Path,
    completed: subprocess.CompletedProcess[str],
    download_result: dict[str, object],
) -> dict[str, object]:
    status_payload = load_json(science_store / "install_status.json")
    store_status = str(status_payload.get("status", "")).strip()
    download_status = str(download_result.get("status", "unknown"))
    missing = status_payload.get("missing_components", [])
    missing_count = len(missing) if isinstance(missing, list) else 0
    if completed.returncode != 0:
        status = "failed"
        summary = "安装/修复未完成。"
        next_action = "检查安装环境后重新运行安装/修复"
    elif store_status == "ready":
        status = "ready"
        summary = "科学仓库已就绪。"
        next_action = "打开 WebUI 并输入研究对象"
    elif download_status == "not_configured":
        status = "needs_repair"
        summary = "安装包未包含科学资源下载清单，科学仓库尚未准备完成。"
        next_action = "使用包含科学资源清单的完整安装包重新运行安装/修复"
    elif download_status == "failed":
        status = "needs_repair"
        summary = "部分科学资源下载或校验失败，科学仓库尚未准备完成。"
        next_action = "检查网络、磁盘空间后重新运行安装/修复"
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
    parser.add_argument("--no-download", action="store_true")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default="19090")
    parser.add_argument("--start-webui", action="store_true")
    parser.add_argument("--debug-output", action="store_true")
    args = parser.parse_args()

    workspace_root = Path(args.workspace_root).resolve()
    science_store = Path(args.science_store).resolve()
    runtime_dir = Path(args.runtime_dir).resolve()
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
    install_status = installation_status(science_store, completed, download_result)
    payload: dict[str, object] = {
        **install_status,
        "returncode": completed.returncode,
        "downloads": download_summary(download_result),
        "external_sources": source_sync_summary(source_sync_plan, science_store),
        "resources": resource_summary(science_store),
        "runtime_backend": runtime_backend(),
        "access_url": f"http://{args.host}:{args.port}/",
    }
    if args.debug_output:
        payload["debug"] = {
            "command": command,
            "stdout": completed.stdout.strip(),
            "stderr": completed.stderr.strip(),
            "workspace_root": str(workspace_root),
            "science_store": str(science_store),
            "runtime_dir": str(runtime_dir),
            "download_manifest": download_result.get("manifest", ""),
            "source_catalog": download_result.get("catalog", ""),
            "source_sync_plan": str(source_sync_plan),
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
