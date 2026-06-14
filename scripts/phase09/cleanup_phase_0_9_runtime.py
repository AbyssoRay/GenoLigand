from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_PORTS = [19190, 19191, 19192, 19193, 19194]


def main() -> int:
    parser = argparse.ArgumentParser(description="Plan or execute Phase 0.9 transient runtime cleanup.")
    parser.add_argument("--workspace-root", default=str(ROOT))
    parser.add_argument("--manifest", default="")
    parser.add_argument("--ports", default=",".join(str(port) for port in DEFAULT_PORTS))
    parser.add_argument("--execute", action="store_true", help="Stop allowlisted local gls-server PIDs and remove the runtime directory.")
    args = parser.parse_args()

    workspace = Path(args.workspace_root).resolve()
    runtime_dir = workspace / "outputs" / "phase_0_9_1" / "runtime"
    manifest = Path(args.manifest).resolve() if args.manifest else workspace / "acceptance_artifacts_phase_0_9_1_production_webui" / "cleanup" / "phase09_runtime_cleanup_manifest.tsv"
    ports = parse_ports(args.ports)
    runtime_safe = is_relative_to(runtime_dir.resolve() if runtime_dir.exists() else runtime_dir, workspace)
    if not runtime_safe:
        raise SystemExit(f"refusing_runtime_path_outside_workspace:{runtime_dir}")

    listeners = listening_pids(ports)
    rows: list[list[str]] = [
        ["cleanup_mode", "execute" if args.execute else "dry_run", "ok"],
        ["workspace_root", str(workspace), "ok"],
        ["runtime_dir", str(runtime_dir), "ok" if runtime_safe else "issue"],
        ["runtime_dir_exists", str(runtime_dir.exists()), "ok"],
        ["allowlisted_ports", ",".join(str(port) for port in ports), "ok"],
        ["allowlisted_listener_pids", json.dumps(listeners, sort_keys=True), "ok"],
        ["execute_requested", str(args.execute), "ok"],
    ]

    stopped: list[int] = []
    removed_runtime = False
    if args.execute:
        for pid in sorted({pid for pid in listeners.values() if pid > 0}):
            stop_process(pid)
            stopped.append(pid)
        if runtime_dir.exists():
            shutil.rmtree(runtime_dir)
            removed_runtime = True

    rows.extend(
        [
            ["stopped_pid_count", str(len(stopped)), "ok"],
            ["stopped_pids", ",".join(str(pid) for pid in stopped), "ok"],
            ["runtime_dir_removed", str(removed_runtime), "ok" if (not args.execute or removed_runtime or not runtime_dir.exists()) else "issue"],
        ]
    )
    write_tsv(manifest, rows)
    print(f"phase09_runtime_cleanup_manifest={manifest}")
    print(f"mode={'execute' if args.execute else 'dry_run'}")
    print(f"listener_pids={json.dumps(listeners, sort_keys=True)}")
    return 0


def parse_ports(text: str) -> list[int]:
    ports: list[int] = []
    for raw in text.split(","):
        raw = raw.strip()
        if not raw:
            continue
        port = int(raw)
        if port < 1 or port > 65535:
            raise SystemExit(f"invalid_port:{port}")
        ports.append(port)
    return ports


def is_relative_to(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
        return True
    except ValueError:
        return False


def listening_pids(ports: list[int]) -> dict[int, int]:
    if not sys.platform.startswith("win"):
        return {}
    port_list = ",".join(str(port) for port in ports)
    command = [
        "powershell",
        "-NoProfile",
        "-Command",
        (
            f"Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort {port_list} -ErrorAction SilentlyContinue | "
            "Where-Object { $_.State -eq 'Listen' -and $_.OwningProcess -ne 0 } | "
            "Select-Object LocalPort,OwningProcess | ConvertTo-Json -Compress"
        ),
    ]
    result = subprocess.run(command, text=True, capture_output=True)
    if result.returncode != 0 or not result.stdout.strip():
        return {}
    try:
        parsed = json.loads(result.stdout)
    except json.JSONDecodeError:
        return {}
    if isinstance(parsed, dict):
        parsed = [parsed]
    listeners: dict[int, int] = {}
    for item in parsed if isinstance(parsed, list) else []:
        try:
            listeners[int(item.get("LocalPort"))] = int(item.get("OwningProcess"))
        except (TypeError, ValueError, AttributeError):
            continue
    return listeners


def stop_process(pid: int) -> None:
    if sys.platform.startswith("win"):
        subprocess.run(["powershell", "-NoProfile", "-Command", f"Stop-Process -Id {pid} -Force"], check=False)


def write_tsv(path: Path, rows: list[list[str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = "metric\tvalue\tstatus\n" + "\n".join("\t".join(cell for cell in row) for row in rows) + "\n"
    path.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
