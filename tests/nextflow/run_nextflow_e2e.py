from __future__ import annotations

import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / "workflows" / "main.nf"
CONFIG = ROOT / "nextflow.config"
FIXTURE_BUILDER = ROOT / "tests" / "e2e" / "prepare_cli_pipeline_fixture.py"
PINNED_VERSION = (ROOT / ".nextflow-version").read_text(encoding="utf-8").strip()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def prepare_fixture(root: Path, mode: str) -> None:
    subprocess.run(
        [sys.executable, str(FIXTURE_BUILDER), "--root", str(root), "--mode", mode],
        check=True,
    )


def validate_real_cli(path: Path) -> Path:
    require(path.is_file(), f"real gls-cli does not exist: {path}")
    require(os.access(path, os.X_OK), f"real gls-cli is not executable: {path}")
    with path.open("rb") as handle:
        require(
            handle.read(4) == b"\x7fELF",
            f"real Nextflow E2E requires a native Linux gls-cli ELF: {path}",
        )
    version = subprocess.run(
        [str(path), "--version"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    require(version.returncode == 0, f"real gls-cli version probe failed: {path}")
    require("gls-cli " in version.stdout, f"unexpected gls-cli version output: {version.stdout}")
    return path.resolve()


def resolve_real_cli() -> Path:
    configured = os.environ.get("GENOLIGAND_E2E_GLS_CLI") or os.environ.get(
        "GENOLIGAND_GLS_CLI"
    )
    if configured:
        return validate_real_cli(Path(configured).expanduser().resolve())
    for candidate in (
        ROOT / "target" / "debug" / "gls-cli",
        ROOT / "target" / "release" / "gls-cli",
    ):
        if candidate.is_file():
            return validate_real_cli(candidate)
    raise AssertionError(
        "set GENOLIGAND_E2E_GLS_CLI to the final native Linux gls-cli binary"
    )


def resolve_worker_python() -> Path:
    configured = (
        os.environ.get("GENOLIGAND_E2E_PYTHON")
        or os.environ.get("GENOLIGAND_PYTHON")
        or sys.executable
    )
    executable = shutil.which(configured) or configured
    # Keep virtual-environment launchers intact: resolving the symlink would
    # replace the venv interpreter with its base Python and lose site-packages.
    path = Path(os.path.abspath(os.path.expanduser(executable)))
    require(path.is_file(), f"worker Python does not exist: {path}")
    require(os.access(path, os.X_OK), f"worker Python is not executable: {path}")
    probe = subprocess.run(
        [str(path), "-c", "import jsonschema; import rdkit"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    require(
        probe.returncode == 0,
        f"worker Python cannot import RDKit/jsonschema: {path}\n{probe.stderr}",
    )
    return path


def create_workflow_gate_cli(path: Path) -> None:
    path.write_text(
        """#!/usr/bin/env bash
set -euo pipefail
run_dir=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--run-dir" ]; then
    run_dir="$2"
    shift 2
  else
    shift
  fi
done
[ -n "$run_dir" ]
mkdir -p "$run_dir"
case "${GENOLIGAND_E2E_MODE:-}" in
  missing_state)
    ;;
  cli_failure)
    exit 17
    ;;
  *)
    printf "workflow gate fixture cannot execute scientific state %s\\n" "${GENOLIGAND_E2E_MODE:-unset}" >&2
    exit 97
    ;;
esac
""",
        encoding="utf-8",
    )
    path.chmod(0o755)


def diagnostics(completed: subprocess.CompletedProcess[str], workdir: Path) -> str:
    parts = [completed.stdout, completed.stderr]
    for name in (".command.out", ".command.err", ".command.log"):
        for path in workdir.rglob(name):
            parts.append(path.read_text(encoding="utf-8", errors="replace"))
    return "\n".join(parts)


def run_case(
    nextflow: str,
    fixture_root: Path,
    mode: str,
    cli: Path,
    worker_python: Path,
    expect_success: bool,
    gate_mode: str | None = None,
) -> tuple[Path, Path, subprocess.CompletedProcess[str]]:
    case = fixture_root / mode
    outdir = case / "published"
    workdir = case / "work"
    case.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["GENOLIGAND_PYTHON"] = str(worker_python)
    env["NXF_ANSI_LOG"] = "false"
    if gate_mode is None:
        env.pop("GENOLIGAND_E2E_MODE", None)
    else:
        env["GENOLIGAND_E2E_MODE"] = gate_mode
    command = [
        nextflow,
        "-c",
        str(CONFIG),
        "run",
        str(WORKFLOW),
        "-profile",
        "local_acceptance",
        "-ansi-log",
        "false",
        "-work-dir",
        str(workdir),
        "--input_manifest",
        str(case / "input_manifest.json"),
        "--science_store",
        str(case / "science_store"),
        "--gls_cli",
        str(cli),
        "--outdir",
        str(outdir),
    ]
    completed = subprocess.run(
        command,
        cwd=case,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
        timeout=180,
    )
    if expect_success:
        require(
            completed.returncode == 0,
            f"completed workflow failed:\n{diagnostics(completed, workdir)}",
        )
    else:
        require(completed.returncode != 0, f"{mode} workflow must fail")
        require(
            not list(outdir.rglob("pipeline_done.txt"))
            and not list(workdir.rglob("pipeline_done.txt")),
            f"{mode} workflow must not produce a done marker",
        )
    return outdir, workdir, completed


def main() -> int:
    require(os.name != "nt", "run this real Nextflow E2E inside the project WSL environment")
    nextflow = shutil.which("nextflow")
    require(nextflow is not None, "nextflow executable is required")
    version_output = subprocess.run(
        [nextflow, "-version"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=True,
    ).stdout
    version_match = re.search(r"version\s+(\S+)", version_output)
    require(version_match is not None, "Nextflow version output is unreadable")
    require(
        version_match.group(1) == PINNED_VERSION,
        f"Nextflow {version_match.group(1)} does not match {PINNED_VERSION}",
    )
    real_cli = resolve_real_cli()
    worker_python = resolve_worker_python()
    print(f"nextflow_e2e_worker_python\t{worker_python}", flush=True)

    with tempfile.TemporaryDirectory(prefix="genoligand-nextflow-e2e-") as temporary:
        fixture_root = Path(temporary)

        completed_case = fixture_root / "completed"
        prepare_fixture(completed_case, "ready")
        marker = completed_case / "science_store" / "science-store-must-not-be-staged.txt"
        marker.write_text("external read-only store\n", encoding="utf-8")
        outdir, workdir, completed = run_case(
            nextflow, fixture_root, "completed", real_cli, worker_python, True
        )
        run_output = outdir / "run_output"
        require((outdir / "pipeline_done.txt").is_file(), "completed run needs a done marker")
        require(
            (run_output / "run_state.txt").read_text(encoding="utf-8") == "completed\n",
            "real gls-cli must publish completed run_state",
        )
        require(
            "pipeline_completed" in diagnostics(completed, workdir),
            "completed path must execute the real gls-cli pipeline",
        )
        provenance = (run_output / "nextflow_provenance.tsv").read_text(encoding="utf-8")
        config_sha256 = hashlib.sha256(CONFIG.read_bytes()).hexdigest()
        require(PINNED_VERSION in provenance, "runtime Nextflow version must enter provenance")
        require(config_sha256 in provenance, "executed config digest must enter provenance")
        require("\t4\t8 GB" in provenance, "acceptance resources must enter provenance")
        require(
            not list(workdir.rglob(marker.name)),
            "science store content must not be staged into task work",
        )

        blocked_case = fixture_root / "blocked"
        prepare_fixture(blocked_case, "blocked")
        _, blocked_workdir, blocked = run_case(
            nextflow, fixture_root, "blocked", real_cli, worker_python, False
        )
        blocked_diagnostics = diagnostics(blocked, blocked_workdir)
        require("pipeline_blocked" in blocked_diagnostics, "blocked path must execute real gls-cli")
        require("exit code 2" in blocked_diagnostics, "blocked gls-cli exit code must remain 2")
        require(
            any(
                path.read_text(encoding="utf-8") == "blocked\n"
                for path in blocked_workdir.rglob("run_state.txt")
            ),
            "blocked real gls-cli state must remain in the task work directory",
        )

        gate_cli = fixture_root / "workflow-gate-cli"
        create_workflow_gate_cli(gate_cli)
        for mode in ("missing_state", "cli_failure"):
            prepare_fixture(fixture_root / mode, "blocked")
            _, gate_workdir, gate = run_case(
                nextflow,
                fixture_root,
                mode,
                gate_cli,
                worker_python,
                False,
                gate_mode=mode,
            )
            gate_diagnostics = diagnostics(gate, gate_workdir)
            if mode == "missing_state":
                require(
                    "GenoLigand run state is missing" in gate_diagnostics,
                    "missing-state workflow gate must reject the run",
                )
            else:
                require(
                    "exit code 17" in gate_diagnostics,
                    "CLI-failure workflow gate must preserve the failing exit code",
                )

    print(
        f"nextflow_real_e2e\tPASS\treal_cli={real_cli}\tworker_python={worker_python}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
