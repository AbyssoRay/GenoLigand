from __future__ import annotations

import argparse
import hashlib
import http.server
import importlib.util
import json
import os
import shutil
import socket
import stat
import subprocess
import sys
import threading
import time
import uuid
from contextlib import contextmanager
from pathlib import Path
from types import SimpleNamespace
from typing import Any, Iterator
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "scripts/setup/install_complete_scientific.py"
WORK_ROOT = ROOT / "work/installer-resilience"
DEFAULT_EVIDENCE = WORK_ROOT / "acceptance.json"
SIMULATED_SOURCE_BYTES = 79_301_981_114
SCHEMA_VERSION = "genoligand_installer_resilience_acceptance_v1"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def load_installer():
    name = f"installer_resilience_{uuid.uuid4().hex}"
    spec = importlib.util.spec_from_file_location(name, INSTALLER)
    if spec is None or spec.loader is None:
        raise RuntimeError("installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def identity_digest(payload: dict[str, Any]) -> str:
    identity = {
        key: payload[key]
        for key in ("url", "etag", "last_modified", "content_length", "accept_ranges")
    }
    return hashlib.sha256(
        json.dumps(identity, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()


@contextmanager
def resource_server(
    content: bytes,
    *,
    etag: str,
    interrupt_at: int | None = None,
) -> Iterator[tuple[str, dict[str, Any]]]:
    state: dict[str, Any] = {"requests": [], "breakpoint_bytes": 0, "interrupted": False}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_HEAD(self) -> None:  # noqa: N802
            self.send_response(200)
            self.send_header("Content-Length", str(len(content)))
            self.send_header("ETag", etag)
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()

        def do_GET(self) -> None:  # noqa: N802
            range_header = self.headers.get("Range", "")
            if_range = self.headers.get("If-Range", "")
            start = int(range_header.removeprefix("bytes=").removesuffix("-")) if range_header else 0
            if range_header and if_range and if_range != etag:
                start = 0
                range_header = ""
            body = content[start:]
            self.send_response(206 if range_header else 200)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("ETag", etag)
            self.send_header("Accept-Ranges", "bytes")
            if range_header:
                self.send_header("Content-Range", f"bytes {start}-{len(content) - 1}/{len(content)}")
            self.end_headers()
            state["requests"].append(
                {
                    "range": self.headers.get("Range", ""),
                    "if_range": if_range,
                    "response_status": 206 if range_header else 200,
                    "response_start": start,
                }
            )
            if interrupt_at is not None and not range_header and not state["interrupted"]:
                state["interrupted"] = True
                state["breakpoint_bytes"] = interrupt_at
                self.wfile.write(body[:interrupt_at])
                self.wfile.flush()
                try:
                    self.connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                self.connection.close()
                return
            self.wfile.write(body)

        def log_message(self, _format: str, *args: object) -> None:
            return

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/resource.bin", state
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def transient_download_residue(installer, destination: Path) -> dict[str, Any]:
    partial = destination.with_suffix(destination.suffix + ".part")
    quarantines = sorted(partial.parent.glob(f"{partial.name}.quarantine-*"))
    return {
        "part_files": int(partial.exists()),
        "partial_identity_files": int(installer.partial_identity_path(partial).exists()),
        "quarantine_files": len(quarantines),
    }


def interrupted_resume_case(installer, root: Path) -> dict[str, Any]:
    content = (b"0123456789abcdef" * 400_000)[:6_000_000]
    expected_sha256 = hashlib.sha256(content).hexdigest()
    cutoff = 2_250_000
    destination = root / "interrupted/resource.bin"
    with resource_server(content, etag='"interrupt-v1"', interrupt_at=cutoff) as (url, server):
        result = installer.copy_or_download(url, destination, expected_sha256)
    requests = server["requests"]
    residue = transient_download_residue(installer, destination)
    require(result.get("status") == "ready", "interrupted transfer did not become ready")
    require(len(requests) >= 2, "interrupted transfer did not issue a resume request")
    require(requests[0]["range"] == "", "first transfer unexpectedly used Range")
    require(requests[1]["range"] == f"bytes={cutoff}-", "resume offset differs from breakpoint")
    require(requests[1]["if_range"] == '"interrupt-v1"', "resume omitted the trusted If-Range")
    require(destination.stat().st_size == len(content), "resumed payload length differs")
    require(sha256_file(destination) == expected_sha256, "resumed payload digest differs")
    require(not any(residue.values()), "resumed transfer left transient files")
    return {
        "status": "passed",
        "first_breakpoint_bytes": server["breakpoint_bytes"],
        "resume_offset_bytes": requests[1]["response_start"],
        "requests": requests,
        "final_bytes": destination.stat().st_size,
        "final_sha256": sha256_file(destination),
        "expected_sha256": expected_sha256,
        "transient_residue": residue,
        "published_identity_present": installer.destination_identity_path(destination).is_file(),
    }


def changed_identity_case(installer, root: Path, changed_field: str) -> dict[str, Any]:
    content = f"publisher-object-after-{changed_field}-change".encode("ascii")
    expected_sha256 = hashlib.sha256(content).hexdigest()
    destination = root / changed_field / "resource.bin"
    partial = destination.with_suffix(destination.suffix + ".part")
    partial.parent.mkdir(parents=True, exist_ok=True)
    stale_prefix = b"stale-partial-prefix"
    partial.write_bytes(stale_prefix)
    etag = '"publisher-v2"'
    with resource_server(content, etag=etag) as (url, server):
        current = installer.remote_resource_identity(url)
        stale = dict(current)
        if changed_field == "etag":
            stale["etag"] = '"publisher-v1"'
        elif changed_field == "content_length":
            stale["content_length"] = int(current["content_length"]) + 17
        else:
            raise ValueError(f"unsupported identity field: {changed_field}")
        stale["identity_digest"] = identity_digest(stale)
        stale["trusted"] = True
        installer.write_partial_identity(partial, stale)
        result = installer.copy_or_download(url, destination, expected_sha256)
    requests = server["requests"]
    residue = transient_download_residue(installer, destination)
    require(result.get("status") == "ready", f"{changed_field} change did not become ready")
    require(requests and requests[0]["range"] == "", f"{changed_field} change reused stale partial")
    require(destination.read_bytes() == content, f"{changed_field} change produced appended content")
    require(not any(residue.values()), f"{changed_field} change left transient files")
    return {
        "status": "passed",
        "changed_field": changed_field,
        "stale_partial_bytes": len(stale_prefix),
        "stored_identity_digest": stale["identity_digest"],
        "current_identity_digest": current["identity_digest"],
        "first_get": requests[0],
        "full_restart": requests[0]["range"] == "",
        "final_bytes": destination.stat().st_size,
        "final_sha256": sha256_file(destination),
        "expected_sha256": expected_sha256,
        "transient_residue": residue,
    }


@contextmanager
def simulated_large_metadata(
    installer,
    source: Path,
    token: str,
) -> Iterator[dict[str, int]]:
    source_key = os.path.normcase(os.path.abspath(os.fspath(source)))
    original_stat = Path.stat
    calls = {"sha256": 0}

    def fake_stat(path: Path, *args: Any, **kwargs: Any):
        key = os.path.normcase(os.path.abspath(os.fspath(path)))
        if key != source_key:
            return original_stat(path, *args, **kwargs)
        return SimpleNamespace(
            st_mode=stat.S_IFREG | 0o644,
            st_size=SIMULATED_SOURCE_BYTES,
            st_mtime_ns=1_700_000_000_000_000_000,
            st_ctime_ns=1_700_000_000_000_000_000,
            st_dev=79,
            st_ino=301_981_114,
        )

    def fake_hash(_path: Path, _progress=None) -> str:
        calls["sha256"] += 1
        return "b" * 64

    with mock.patch.object(Path, "stat", fake_stat), mock.patch.object(
        installer, "file_change_token", return_value=token
    ), mock.patch.object(installer, "sha256_file_with_progress", side_effect=fake_hash):
        yield calls


def fingerprint_worker(source: Path, cache_path: Path, token: str, result_path: Path) -> int:
    installer = load_installer()
    physical_bytes = source.stat().st_size
    cache = installer.load_resource_summary_cache(cache_path)
    with simulated_large_metadata(installer, source, token) as calls:
        first = installer.resource_payload_summary(source, cache=cache)
        second = installer.resource_payload_summary(source, cache=cache)
    installer.write_json_atomic(
        result_path,
        {
            "hash_calls": calls["sha256"],
            "summaries_equal": first == second,
            "summary_bytes": int(first.get("bytes", 0)),
            "physical_bytes": physical_bytes,
            "fresh_process": True,
        },
    )
    return 0


def run_fingerprint_process(source: Path, cache: Path, token: str, result: Path) -> dict[str, Any]:
    command = [
        sys.executable,
        str(Path(__file__).resolve()),
        "--fingerprint-worker",
        "--source",
        str(source),
        "--cache",
        str(cache),
        "--token",
        token,
        "--worker-result",
        str(result),
    ]
    completed = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", check=False)
    require(
        completed.returncode == 0,
        f"fingerprint subprocess failed: {(completed.stderr or completed.stdout).strip()[:500]}",
    )
    return json.loads(result.read_text(encoding="utf-8"))


def fingerprint_cache_case(installer, root: Path) -> dict[str, Any]:
    source = root / "fingerprint/mgy_clusters.fa.gz"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_bytes(b"x")
    cache_path = source.parent / "source_fingerprint_cache.json"
    cache: dict[str, Any] = {}
    with simulated_large_metadata(installer, source, "fixture-token-v1") as seeded_calls:
        installer.seed_resource_file_fingerprint(cache, source, "a" * 64)
    require(seeded_calls["sha256"] == 0, "fixed digest seed unexpectedly hashed the source")
    installer.save_resource_summary_cache(cache_path, cache)

    reloaded = installer.load_resource_summary_cache(cache_path)
    with simulated_large_metadata(installer, source, "fixture-token-v1") as reload_calls:
        reloaded_summary = installer.resource_payload_summary(source, cache=reloaded)
    unchanged = run_fingerprint_process(
        source, cache_path, "fixture-token-v1", source.parent / "unchanged-process.json"
    )
    changed = run_fingerprint_process(
        source, cache_path, "fixture-token-v2", source.parent / "changed-process.json"
    )
    require(reload_calls["sha256"] == 0, "reloaded persistent cache rehashed unchanged metadata")
    require(unchanged["hash_calls"] == 0, "fresh process rehashed unchanged metadata")
    require(changed["hash_calls"] <= 1, "changed source hashed more than once in one invocation")
    require(reloaded_summary.get("bytes") == SIMULATED_SOURCE_BYTES, "simulated source size was lost")
    return {
        "status": "passed",
        "simulated_bytes": SIMULATED_SOURCE_BYTES,
        "physical_bytes": source.stat().st_size,
        "seed_sha256": "a" * 64,
        "seed_hash_calls": seeded_calls["sha256"],
        "reload_hash_calls": reload_calls["sha256"],
        "fresh_process_unchanged": unchanged,
        "fresh_process_changed": changed,
        "cache_schema_version": json.loads(cache_path.read_text(encoding="utf-8"))["schema_version"],
    }


def manifest_interruption_case(installer, root: Path) -> dict[str, Any]:
    manifest_root = root / "manifests"
    manifest_root.mkdir(parents=True, exist_ok=True)
    results: dict[str, Any] = {}
    for category, name in (
        ("download", "download_execution_manifest.tsv"),
        ("external", "external_sync_execution_manifest.tsv"),
        ("source", "source_file_manifest.tsv"),
    ):
        target = manifest_root / name
        last_good = f"kind\tstatus\n{category}\tready\n".encode("utf-8")
        target.write_bytes(last_good)
        before_sha256 = sha256_file(target)
        try:
            with installer.atomic_text_writer(target) as handle:
                handle.write(f"kind\tstatus\n{category}\tpartial-new-state\n")
                raise RuntimeError(f"injected_{category}_manifest_interrupt")
        except RuntimeError as err:
            require(str(err) == f"injected_{category}_manifest_interrupt", "unexpected injected error")
        attempt_path = target.with_name(f"{target.name}.attempt.json")
        attempt = json.loads(attempt_path.read_text(encoding="utf-8"))
        temporary = sorted(target.parent.glob(f".{target.name}.*.tmp"))
        require(target.read_bytes() == last_good, f"{category} last-good bytes changed")
        require(attempt.get("status") == "failed", f"{category} attempt diagnostic missing")
        require(
            f"injected_{category}_manifest_interrupt" in str(attempt.get("error", "")),
            f"{category} attempt diagnostic lost the cause",
        )
        require(not temporary, f"{category} atomic temporary file remained")
        results[category] = {
            "status": "passed",
            "manifest": name,
            "last_good_bytes": len(last_good),
            "before_sha256": before_sha256,
            "after_sha256": sha256_file(target),
            "last_good_unchanged": True,
            "attempt_status": attempt["status"],
            "attempt_error": attempt["error"],
            "temporary_files": len(temporary),
        }
    return {"status": "passed", "manifests": results}


def safe_remove_run(run_dir: Path) -> bool:
    runs_root = (WORK_ROOT / "runs").resolve()
    resolved = run_dir.resolve()
    require(resolved.parent == runs_root, f"unsafe work cleanup path: {resolved}")
    if resolved.exists():
        shutil.rmtree(resolved)
    return not resolved.exists()


def run_acceptance(evidence_path: Path) -> int:
    installer = load_installer()
    run_dir = WORK_ROOT / "runs" / f"run-{uuid.uuid4().hex}"
    run_dir.mkdir(parents=True, exist_ok=False)
    evidence: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "generated_at_unix": int(time.time()),
        "installer": str(INSTALLER),
        "installer_sha256": sha256_file(INSTALLER),
        "status": "running",
        "acceptance": {"GL025": "running", "GL028": "running"},
        "cases": {},
    }
    exit_code = 0
    try:
        evidence["cases"] = {
            "interrupted_resume": interrupted_resume_case(installer, run_dir),
            "changed_identity": {
                "status": "passed",
                "etag": changed_identity_case(installer, run_dir / "identity", "etag"),
                "content_length": changed_identity_case(
                    installer, run_dir / "identity", "content_length"
                ),
            },
            "fingerprint_cache": fingerprint_cache_case(installer, run_dir),
            "manifest_interruption": manifest_interruption_case(installer, run_dir),
        }
        evidence["status"] = "passed"
        evidence["acceptance"] = {"GL025": "passed", "GL028": "passed"}
    except Exception as err:
        evidence["status"] = "failed"
        evidence["acceptance"] = {"GL025": "failed", "GL028": "failed"}
        evidence["error"] = f"{type(err).__name__}: {err}"
        exit_code = 1
    try:
        evidence["work_cleanup"] = {"run_directory_removed": safe_remove_run(run_dir)}
    except Exception as err:
        evidence["status"] = "failed"
        evidence["work_cleanup"] = {"run_directory_removed": False, "error": str(err)}
        exit_code = 1
    installer.write_json_atomic(evidence_path.resolve(), evidence)
    print(
        json.dumps(
            {
                "status": evidence["status"],
                "evidence": str(evidence_path.resolve()),
                "GL025": evidence["acceptance"]["GL025"],
                "GL028": evidence["acceptance"]["GL028"],
            },
            ensure_ascii=False,
        )
    )
    return exit_code


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run installer resilience acceptance checks")
    parser.add_argument("--self-test", action="store_true", help="run GL025/GL028 acceptance checks")
    parser.add_argument("--evidence", type=Path, default=DEFAULT_EVIDENCE)
    parser.add_argument("--fingerprint-worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--source", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--cache", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--token", help=argparse.SUPPRESS)
    parser.add_argument("--worker-result", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.fingerprint_worker:
        for name in ("source", "cache", "token", "worker_result"):
            if getattr(args, name) is None:
                parser.error(f"--{name.replace('_', '-')} is required for fingerprint worker")
    elif not args.self_test:
        parser.error("--self-test is required")
    return args


def main() -> int:
    args = parse_args()
    if args.fingerprint_worker:
        return fingerprint_worker(args.source, args.cache, args.token, args.worker_result)
    return run_acceptance(args.evidence)


if __name__ == "__main__":
    raise SystemExit(main())
