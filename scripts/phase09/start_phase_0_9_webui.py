from __future__ import annotations

import argparse
import subprocess
import sys
import time
import urllib.request
import webbrowser
import os
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_ARTIFACT = ROOT / "acceptance_artifacts_phase_0_9_1_production_webui"


def main() -> int:
    parser = argparse.ArgumentParser(description="Start the GenoLigand 0.9.1 production local WebUI.")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=19090)
    parser.add_argument("--artifact-dir", default=str(DEFAULT_ARTIFACT))
    parser.add_argument("--no-build", action="store_true", help="Skip cargo build and use the existing gls-server binary.")
    parser.add_argument("--no-open", action="store_true", help="Do not open the browser automatically.")
    args = parser.parse_args()

    if not args.no_build:
        run(["cargo", "build", "-p", "gls-server"])

    server = ROOT / "target" / "debug" / ("gls-server.exe" if sys.platform.startswith("win") else "gls-server")
    if not server.exists():
        raise SystemExit(f"missing_gls_server_binary:{server}")

    runtime_dir = ROOT / "outputs" / "phase_0_9_1" / "runtime"
    runtime_dir.mkdir(parents=True, exist_ok=True)
    stdout = (runtime_dir / "gls-server.stdout.log").open("w", encoding="utf-8")
    stderr = (runtime_dir / "gls-server.stderr.log").open("w", encoding="utf-8")
    command = [
        str(server),
        "--host",
        args.host,
        "--port",
        str(args.port),
        "--artifact-dir",
        str(Path(args.artifact_dir).resolve()),
        "--workspace-root",
        str(ROOT),
    ]
    env = dict(os.environ)
    if "Path" in env and "PATH" in env:
        env.pop("PATH", None)
    process = subprocess.Popen(command, cwd=ROOT, stdout=stdout, stderr=stderr, env=env)
    url = f"http://{args.host}:{args.port}/"
    try:
        wait_for_health(args.host, args.port, process)
        print(f"GenoLigand 0.9.1 WebUI is running: {url}", flush=True)
        print("Press Ctrl+C to stop the local server.", flush=True)
        if not args.no_open:
            webbrowser.open(url)
        process.wait()
    except KeyboardInterrupt:
        print("\nStopping GenoLigand WebUI...", flush=True)
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
    finally:
        stdout.close()
        stderr.close()
    return 0


def run(command: list[str]) -> None:
    result = subprocess.run(command, cwd=ROOT)
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def wait_for_health(host: str, port: int, process: subprocess.Popen) -> None:
    url = f"http://{host}:{port}/api/health"
    deadline = time.time() + 20
    last_error = ""
    while time.time() < deadline:
        if process.poll() is not None:
            raise SystemExit(f"gls_server_exited:{process.returncode}")
        try:
            with urllib.request.urlopen(url, timeout=2) as response:
                body = response.read().decode("utf-8", errors="replace")
                if response.status == 200 and '"status":"ready"' in body:
                    return
        except Exception as err:  # noqa: BLE001
            last_error = str(err)
            time.sleep(0.25)
    raise SystemExit(f"health_check_timeout:{last_error}")


if __name__ == "__main__":
    raise SystemExit(main())
