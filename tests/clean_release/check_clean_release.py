from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import sys
import tempfile
from pathlib import Path
from typing import Iterable


ROOT = Path(__file__).resolve().parents[2]
FORBIDDEN_WORKSPACE_ROOTS = (
    "outputs",
    ".genoligand",
    "acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict",
)
WORKSPACE_GENERATED_DIRS = {"target", "work"}
SKIPPED_WORKSPACE_TREES = {".git", ".codex", ".superpowers"}
PATH_FIELDS = {
    "canonical",
    "db_path",
    "index_target",
    "input_path",
    "local_path",
    "manifest",
    "output_path",
    "path",
    "ready_marker",
    "source_path",
    "target_rel_path",
}
MANIFEST_NAME_MARKERS = (
    "catalog",
    "manifest",
    "readiness",
    "requirements",
    "scope",
    "status",
)
ACCEPTANCE_MARKERS = ("acceptance", "fixture", "sample", "demo", "test", "验收", "测试", "示例")
DRIVE_MARKER = re.compile(r"^[A-Za-z][:\uf03a]")
HEX_SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")


Issue = tuple[str, Path]


def is_within(path: Path, root: Path) -> bool:
    try:
        path.resolve(strict=False).relative_to(root.resolve(strict=False))
        return True
    except ValueError:
        return False


def add_issue(issues: set[Issue], category: str, path: Path) -> None:
    issues.add((category, path.resolve(strict=False)))


def workspace_issues(workspace_root: Path) -> set[Issue]:
    issues: set[Issue] = set()
    for name in FORBIDDEN_WORKSPACE_ROOTS:
        path = workspace_root / name
        if path.exists():
            add_issue(issues, "workspace_generated_root", path)

    for current, directories, files in os.walk(workspace_root):
        current_path = Path(current)
        relative = current_path.relative_to(workspace_root)
        if any(part in SKIPPED_WORKSPACE_TREES for part in relative.parts):
            directories[:] = []
            continue

        for directory in list(directories):
            path = current_path / directory
            if directory in WORKSPACE_GENERATED_DIRS:
                add_issue(issues, "workspace_generated_root", path)
                directories.remove(directory)
                continue
            if directory == "__pycache__":
                add_issue(issues, "workspace_python_cache", path)
                directories.remove(directory)
                continue
            if directory.endswith(".tmp"):
                add_issue(issues, "workspace_temporary_path", path)
            if directory.casefold() == "run":
                parts = path.relative_to(workspace_root).parts[:-1]
                if any(DRIVE_MARKER.match(part) for part in parts):
                    add_issue(issues, "workspace_encoded_absolute_run", path)

        for name in files:
            path = current_path / name
            if name.startswith(".nextflow.log"):
                add_issue(issues, "workspace_nextflow_log", path)
            elif name.endswith(".tmp"):
                add_issue(issues, "workspace_temporary_path", path)
    return issues


def manifest_document(path: Path) -> bool:
    name = path.name.casefold()
    return (
        path.suffix.casefold() in {".json", ".tsv"}
        and (
            path.parent.name == "manifests"
            or any(marker in name for marker in MANIFEST_NAME_MARKERS)
            or name.endswith("_target_databases.tsv")
        )
    )


def referenced_value(science_store: Path, value: str) -> Path | None:
    value = value.strip().strip('"')
    if not value or value.startswith(("http://", "https://", "s3://", "gs://")):
        return None
    candidate = Path(value)
    if not candidate.is_absolute():
        candidate = science_store / candidate
    candidate = candidate.resolve(strict=False)
    return candidate if is_within(candidate, science_store) and candidate != science_store.resolve() else None


def path_field(name: str) -> bool:
    normalized = name.casefold()
    return normalized in PATH_FIELDS or normalized.endswith(("_path", "_file", "_dir"))


def tsv_references(path: Path, science_store: Path) -> Iterable[Path]:
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                for key, raw in row.items():
                    if key and raw and path_field(key):
                        for value in raw.split(";"):
                            candidate = referenced_value(science_store, value)
                            if candidate is not None:
                                yield candidate
    except (OSError, UnicodeError, csv.Error):
        return


def json_references(value: object, science_store: Path, key: str = "") -> Iterable[Path]:
    if isinstance(value, dict):
        for child_key, child in value.items():
            yield from json_references(child, science_store, str(child_key))
    elif isinstance(value, list):
        for child in value:
            yield from json_references(child, science_store, key)
    elif isinstance(value, str) and path_field(key):
        candidate = referenced_value(science_store, value)
        if candidate is not None:
            yield candidate


def collect_references(
    workspace_root: Path, science_store: Path, store_paths: Iterable[Path]
) -> set[Path]:
    documents = {
        workspace_root / "configs" / "source_catalog.tsv",
        workspace_root / "configs" / "source_sync_plan.tsv",
    }
    documents.update(path for path in store_paths if path.is_file() and manifest_document(path))
    references: set[Path] = set()
    for document in documents:
        try:
            if not document.is_file() or document.stat().st_size > 16 * 1024 * 1024:
                continue
        except OSError:
            continue
        if document.suffix.casefold() == ".tsv":
            references.update(tsv_references(document, science_store))
        else:
            try:
                payload = json.loads(document.read_text(encoding="utf-8-sig"))
            except (OSError, UnicodeError, json.JSONDecodeError):
                continue
            references.update(json_references(payload, science_store))
    return references


def referenced_destination(path: Path, references: set[Path]) -> bool:
    resolved = path.resolve(strict=False)
    return any(resolved == reference or is_within(resolved, reference) for reference in references)


def valid_partial_identity(partial: Path, references: set[Path]) -> bool:
    identity_path = partial.with_name(f"{partial.name}.identity.json")
    try:
        payload = json.loads(identity_path.read_text(encoding="utf-8-sig"))
        content_length = int(payload.get("content_length", 0))
        partial_size = partial.stat().st_size
    except (OSError, UnicodeError, ValueError, TypeError, json.JSONDecodeError):
        return False
    identity_payload = {
        "url": str(payload.get("url", "")),
        "etag": str(payload.get("etag", "")),
        "last_modified": str(payload.get("last_modified", "")),
        "content_length": content_length,
        "accept_ranges": str(payload.get("accept_ranges", "")),
    }
    expected_digest = hashlib.sha256(
        json.dumps(identity_payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    digest = str(payload.get("identity_digest", "")).casefold()
    trusted = payload.get("trusted") is True
    has_validator = bool(str(payload.get("etag", "")).strip() or str(payload.get("last_modified", "")).strip())
    destination = partial.with_name(partial.name[: -len(".part")])
    return (
        trusted
        and digest == expected_digest
        and bool(HEX_SHA256.fullmatch(digest))
        and identity_payload["url"].lower().startswith(("http://", "https://"))
        and content_length > 0
        and 0 < partial_size < content_length
        and has_validator
        and referenced_destination(destination, references)
    )


def stale_store_name(name: str) -> str | None:
    normalized = name.casefold()
    if ".quarantine-" in normalized:
        return "science_store_quarantine"
    if ".rebuild-" in normalized or normalized in {".rebuild", "rebuild"}:
        return "science_store_rebuild"
    if ".previous-" in normalized or normalized in {".previous", "previous"}:
        return "science_store_previous"
    if (
        normalized in {".staging", "staging"}
        or normalized.startswith((".staging-", "staging-"))
        or normalized.endswith(".staging")
    ):
        return "science_store_staging"
    return None


def science_store_issues(workspace_root: Path, science_store: Path) -> set[Issue]:
    issues: set[Issue] = set()
    if not science_store.is_dir():
        add_issue(issues, "science_store_missing", science_store)
        return issues

    store_paths: list[Path] = []
    for current, directories, files in os.walk(science_store):
        current_path = Path(current)
        store_paths.extend(current_path / name for name in directories)
        store_paths.extend(current_path / name for name in files)
    references = collect_references(workspace_root, science_store, store_paths)

    for path in store_paths:
        name = path.name
        if name.endswith(".part"):
            if not valid_partial_identity(path, references):
                add_issue(issues, "science_store_partial_without_identity", path)
            continue
        if name.endswith(".part.identity.json"):
            partial = path.with_name(name[: -len(".identity.json")])
            if not partial.is_file() or not valid_partial_identity(partial, references):
                add_issue(issues, "science_store_orphan_partial_identity", path)
            continue
        category = stale_store_name(name)
        if category and path.resolve(strict=False) not in references:
            add_issue(issues, category, path)
    return issues


def windows_process_alive(pid: int) -> bool:
    import ctypes
    from ctypes import wintypes

    process_query_limited_information = 0x1000
    still_active = 259
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel32.GetExitCodeProcess.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL

    handle = kernel32.OpenProcess(process_query_limited_information, False, pid)
    if not handle:
        return ctypes.get_last_error() == 5
    try:
        exit_code = wintypes.DWORD()
        return bool(kernel32.GetExitCodeProcess(handle, ctypes.byref(exit_code))) and exit_code.value == still_active
    finally:
        kernel32.CloseHandle(handle)


def process_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        return windows_process_alive(pid)
    try:
        os.kill(pid, 0)
        return True
    except PermissionError:
        return True
    except (OSError, OverflowError):
        return False


def runtime_pid(path: Path) -> int:
    try:
        if path.name.endswith(".instance.json"):
            return int(json.loads(path.read_text(encoding="utf-8-sig")).get("pid", 0))
        return int(path.read_text(encoding="utf-8-sig").strip())
    except (OSError, UnicodeError, ValueError, TypeError, json.JSONDecodeError):
        return 0


def acceptance_project(project: Path) -> bool:
    normalized = project.name.casefold()
    if re.fullmatch(r"project-\d{10,}", normalized):
        return True
    labels = [normalized]
    for name in ("project_name.txt", "acceptance.marker", ".acceptance"):
        path = project / name
        if path.is_file():
            try:
                labels.append(path.read_text(encoding="utf-8-sig").casefold())
            except (OSError, UnicodeError):
                pass
    return any(marker in label for marker in ACCEPTANCE_MARKERS for label in labels)


def runtime_issues(runtime_dir: Path) -> set[Issue]:
    issues: set[Issue] = set()
    if not runtime_dir.is_dir():
        add_issue(issues, "runtime_missing", runtime_dir)
        return issues
    for path in runtime_dir.glob("*.pid"):
        if not process_alive(runtime_pid(path)):
            add_issue(issues, "runtime_dead_pid", path)
    for path in runtime_dir.glob("*.instance.json"):
        if not process_alive(runtime_pid(path)):
            add_issue(issues, "runtime_dead_pid", path)
    projects = runtime_dir / "projects"
    if projects.is_dir():
        for project in projects.iterdir():
            if project.is_dir() and acceptance_project(project):
                add_issue(issues, "runtime_old_acceptance_project", project)
    return issues


def collect_issues(
    workspace_root: Path, science_store: Path | None, runtime_dir: Path | None
) -> list[Issue]:
    issues = workspace_issues(workspace_root)
    if science_store is not None:
        issues.update(science_store_issues(workspace_root, science_store))
    if runtime_dir is not None:
        issues.update(runtime_issues(runtime_dir))
    return sorted(issues, key=lambda issue: (issue[0], str(issue[1]).casefold()))


def write_valid_identity(partial: Path) -> None:
    identity = {
        "url": "https://example.invalid/source.dat",
        "etag": '"fixture"',
        "last_modified": "",
        "content_length": 10,
        "accept_ranges": "bytes",
    }
    identity["identity_digest"] = hashlib.sha256(
        json.dumps(identity, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    identity["trusted"] = True
    partial.with_name(f"{partial.name}.identity.json").write_text(
        json.dumps(identity),
        encoding="utf-8",
    )


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="genoligand-clean-release-") as temporary:
        root = Path(temporary)
        workspace = root / "workspace"
        store = root / "science_store"
        runtime = root / "runtime"
        (workspace / "configs").mkdir(parents=True)
        (store / "downloads/source").mkdir(parents=True)
        (store / "indexes/mmseqs/source").mkdir(parents=True)
        (store / "indexes/foldseek/source").mkdir(parents=True)
        (store / "indexes/forcefields").mkdir(parents=True)
        (runtime / "projects/project-123e4567-e89b-12d3-a456-426614174000").mkdir(parents=True)
        (workspace / "configs/source_catalog.tsv").write_text(
            "source_id\tlocal_path\nfixture\tdownloads/source/source.dat\n",
            encoding="utf-8",
        )
        partial = store / "downloads/source/source.dat.part"
        partial.write_bytes(b"partial")
        write_valid_identity(partial)
        (store / "indexes/mmseqs/source/target.dbtype").write_bytes(b"db")
        (store / "indexes/foldseek/source/target.dbtype").write_bytes(b"db")
        (store / "indexes/forcefields/openff-2.2.1.offxml").write_text("forcefield", encoding="utf-8")
        (runtime / "gls-server.pid").write_text(str(os.getpid()), encoding="utf-8")
        if collect_issues(workspace, store, runtime):
            raise AssertionError("clean fixture produced cleanup candidates")

        (workspace / "target").mkdir()
        (workspace / "pkg/work").mkdir(parents=True)
        (workspace / "pkg/__pycache__").mkdir(parents=True)
        (workspace / "C\uf03a/Users/fixture/run").mkdir(parents=True)
        (workspace / ".nextflow.log").write_text("log", encoding="utf-8")
        (workspace / "leftover.tmp").write_text("tmp", encoding="utf-8")
        (store / "downloads/source/orphan.dat.part").write_bytes(b"partial")
        (store / "downloads/source/lost.dat.part.identity.json").write_text("{}", encoding="utf-8")
        (store / "indexes/mmseqs/source/target.rebuild-old").mkdir()
        (store / "indexes/foldseek/source/target.previous-old").mkdir()
        (store / "downloads/source/.staging-old").mkdir()
        (store / "downloads/source/file.part.quarantine-old").write_bytes(b"bad")
        (runtime / "acceptance-server.pid").write_text("99999999", encoding="utf-8")
        (runtime / "projects/project-1782886189301").mkdir()
        categories = {category for category, _ in collect_issues(workspace, store, runtime)}
        expected = {
            "workspace_generated_root",
            "workspace_python_cache",
            "workspace_encoded_absolute_run",
            "workspace_nextflow_log",
            "workspace_temporary_path",
            "science_store_partial_without_identity",
            "science_store_orphan_partial_identity",
            "science_store_quarantine",
            "science_store_rebuild",
            "science_store_previous",
            "science_store_staging",
            "runtime_dead_pid",
            "runtime_old_acceptance_project",
        }
        missing = expected - categories
        if missing:
            raise AssertionError(f"cleanup self-test categories missing: {sorted(missing)}")
    print("clean_release_self_test\tPASS")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate GenoLigand release cleanup state.")
    parser.add_argument("--workspace-root", type=Path, default=ROOT)
    parser.add_argument("--science-store", type=Path)
    parser.add_argument("--runtime-dir", type=Path)
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        return self_test()
    workspace_root = args.workspace_root.resolve()
    science_store = args.science_store.resolve() if args.science_store else None
    runtime_dir = args.runtime_dir.resolve() if args.runtime_dir else None
    if not workspace_root.is_dir():
        print(f"workspace_missing\t{workspace_root}", file=sys.stderr)
        return 2
    issues = collect_issues(workspace_root, science_store, runtime_dir)
    if issues:
        for category, path in issues:
            print(f"{category}\t{path}", file=sys.stderr)
        print(f"clean_release_contracts\tFAIL\tissues={len(issues)}", file=sys.stderr)
        return 1
    print("clean_release_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
