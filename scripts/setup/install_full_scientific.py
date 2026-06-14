from __future__ import annotations

import argparse
import base64
import json
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
from pathlib import Path


DEFAULT_WSL_DISTRO = "GenoLigandPhase08"
DEFAULT_WSL_PYTHON = "/opt/genoligand-phase084-openmm/bin/python"
DEFAULT_WSL_FULL_PYTHON = "$HOME/.genoligand/micromamba/envs/genoligand-full-scientific/bin/python"
WSL_USER_PATH_EXPORT = 'export PATH="$HOME/.local/bin:$PATH"'
WSL_MAMBA_ROOT = "$HOME/.genoligand/micromamba"
WSL_INDEX_ENV_NAME = "genoligand-index-tools"
WSL_FULL_ENV_NAME = "genoligand-full-scientific"
ANSI_ESCAPE_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")


TOOL_CHECKS = [
    ("java", ["java", "-version"], "Install OpenJDK 17+ or Temurin 17+."),
    ("nextflow", ["nextflow", "-version"], "Install Nextflow in the active Linux or WSL backend."),
    ("python_worker", [sys.executable, "--version"], "Use the current Python interpreter for schema-bound workers."),
    ("openmm", [sys.executable, "-c", "import openmm"], "Install OpenMM from conda-forge."),
    ("rdkit", [sys.executable, "-c", "import rdkit"], "Install RDKit from conda-forge."),
    ("openff", [sys.executable, "-c", "import openff.toolkit"], "Install OpenFF Toolkit from conda-forge."),
    ("mdtraj", [sys.executable, "-c", "import mdtraj"], "Install MDTraj from conda-forge."),
    ("mdanalysis", [sys.executable, "-c", "import MDAnalysis"], "Install MDAnalysis from conda-forge."),
    ("pdbfixer", [sys.executable, "-c", "import pdbfixer"], "Install PDBFixer from conda-forge."),
    ("cuda_openmm", [sys.executable, "-c", "import openmm; print(openmm.Platform.getPlatformByName('CUDA').getName())"], "Install a CUDA-capable OpenMM build and NVIDIA driver support."),
    ("diamond", ["diamond", "--version"], "Install DIAMOND or use MMseqs2/Foldseek for sequence/structure indexing."),
    ("mmseqs", ["mmseqs", "version"], "Install MMseqs2 if DIAMOND is not used."),
    ("foldseek", ["foldseek", "version"], "Install Foldseek for structure indexing."),
]


CONDA_PACKAGES = [
    "openmm",
    "rdkit",
    "openff-toolkit",
    "mdtraj",
    "mdanalysis",
    "pdbfixer",
    "diamond",
    "mmseqs2",
    "foldseek",
]

PYTHON_REPAIR_PACKAGES = {
    "openmm": "openmm",
    "rdkit": "rdkit",
    "openff": "openff-toolkit",
    "mdtraj": "mdtraj",
    "mdanalysis": "MDAnalysis",
    "pdbfixer": "pdbfixer",
}

INDEX_TOOL_IDS = {"diamond", "mmseqs", "foldseek"}


def decode_bytes(payload: bytes) -> str:
    if not payload:
        return ""
    if payload.startswith(b"\xff\xfe") or payload.count(b"\x00") > max(1, len(payload) // 8):
        try:
            return payload.decode("utf-16-le", errors="replace").lstrip("\ufeff")
        except UnicodeError:
            pass
    return payload.decode("utf-8", errors="replace")


def run(command: list[str], cwd: Path, timeout_seconds: int = 120) -> dict[str, str]:
    try:
        result = subprocess.run(command, cwd=cwd, capture_output=True, timeout=timeout_seconds)
        return {
            "ok": str(result.returncode == 0).lower(),
            "exit_code": str(result.returncode),
            "stdout": clean_multiline(decode_bytes(result.stdout)),
            "stderr": clean_multiline(decode_bytes(result.stderr)),
        }
    except Exception as exc:  # noqa: BLE001 - setup reports exact local failure.
        return {"ok": "false", "exit_code": "error", "stdout": "", "stderr": clean(str(exc))}


def clean(text: str) -> str:
    return " ".join(ANSI_ESCAPE_RE.sub(" ", str(text or "")).replace("\x00", " ").split())


def clean_multiline(text: str) -> str:
    lines = []
    normalized = ANSI_ESCAPE_RE.sub(" ", str(text or "")).replace("\x00", " ")
    for line in normalized.splitlines():
        cleaned = " ".join(line.split())
        if cleaned:
            lines.append(cleaned)
    return "\n".join(lines)


def command_available(name: str) -> bool:
    return shutil.which(name) is not None


def wsl_command(distro: str, command: list[str]) -> list[str]:
    return ["wsl.exe", "-d", distro, "--", *command]


def shell_join(command: list[str]) -> str:
    return " ".join(shlex.quote(part) for part in command)


def wsl_shell_command(distro: str, script: str) -> list[str]:
    encoded = base64.b64encode(script.encode("utf-8")).decode("ascii")
    return wsl_command(distro, ["sh", "-lc", f"printf %s {shlex.quote(encoded)} | base64 -d | sh"])


def wsl_path_command(distro: str, command: list[str]) -> list[str]:
    return wsl_shell_command(distro, f"{WSL_USER_PATH_EXPORT}; {shell_join(command)}")


def wsl_python_any_command(distro: str, primary_python: str, python_args: list[str]) -> list[str]:
    script = f"""
{WSL_USER_PATH_EXPORT}
for py in {shlex.quote(primary_python)} "{DEFAULT_WSL_FULL_PYTHON}"; do
  if [ -x "$py" ]; then
    "$py" {shell_join(python_args)} && exit 0
  fi
done
exit 1
""".strip()
    return wsl_shell_command(distro, script)


def wsl_available(distro: str, workspace: Path) -> bool:
    result = run(wsl_command(distro, ["true"]), workspace)
    return result["ok"] == "true"


def wsl_command_available(distro: str, name: str, workspace: Path) -> bool:
    result = run(wsl_shell_command(distro, f"{WSL_USER_PATH_EXPORT}; command -v {shlex.quote(name)}"), workspace)
    return result["ok"] == "true" and bool(result["stdout"].strip())


def detect_install_backend(runtime: str, workspace: Path, distro: str) -> tuple[str, list[str], str]:
    if runtime == "wsl":
        for backend in ["micromamba", "mamba", "conda"]:
            if wsl_command_available(distro, backend, workspace):
                args = wsl_command(distro, [backend, "install", "-y", "-c", "conda-forge", *CONDA_PACKAGES])
                return f"wsl:{backend}", args, " ".join(args)
        return "wsl:manual", [], f"Install micromamba/mamba/conda in {distro}, then install the conda-forge package set."
    if command_available("micromamba"):
        args = ["micromamba", "install", "-y", "-c", "conda-forge", *CONDA_PACKAGES]
        return "micromamba", args, " ".join(args)
    if command_available("mamba"):
        args = ["mamba", "install", "-y", "-c", "conda-forge", *CONDA_PACKAGES]
        return "mamba", args, " ".join(args)
    if command_available("conda"):
        args = ["conda", "install", "-y", "-c", "conda-forge", *CONDA_PACKAGES]
        return "conda", args, " ".join(args)
    if command_available("pip"):
        packages = ["openmm", "rdkit", "openff-toolkit", "mdtraj", "MDAnalysis"]
        args = [sys.executable, "-m", "pip", "install", *packages]
        return "pip", args, " ".join(args)
    return "manual", [], "Install micromamba/mamba/conda, then install the conda-forge package set."


def nextflow_repair_script() -> str:
    return """
set -eu
export PATH="$HOME/.local/bin:$PATH"
mkdir -p "$HOME/.local/bin"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
cd "$tmp_dir"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL https://get.nextflow.io | bash
else
  wget -qO- https://get.nextflow.io | bash
fi
chmod +x nextflow
mv nextflow "$HOME/.local/bin/nextflow"
"$HOME/.local/bin/nextflow" -version
""".strip()


def java_repair_script() -> str:
    return """
set -eu
export PATH="$HOME/.local/bin:$PATH"
if [ ! -s "$HOME/.sdkman/bin/sdkman-init.sh" ]; then
  curl -s https://get.sdkman.io | bash
fi
. "$HOME/.sdkman/bin/sdkman-init.sh"
sdk install java 17.0.10-tem || sdk install java
java -version
""".strip()


def micromamba_bootstrap_script() -> str:
    return """
set -eu
export PATH="$HOME/.local/bin:$PATH"
mkdir -p "$HOME/.local/bin" "$HOME/.genoligand/micromamba"
if ! command -v micromamba >/dev/null 2>&1; then
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT
  cd "$tmp_dir"
  curl -Ls https://micro.mamba.pm/api/micromamba/linux-64/latest | tar -xj bin/micromamba
  mv bin/micromamba "$HOME/.local/bin/micromamba"
  chmod +x "$HOME/.local/bin/micromamba"
fi
micromamba --version
""".strip()


def index_tools_repair_script() -> str:
    return f"""
set -eu
export PATH="$HOME/.local/bin:$PATH"
export MAMBA_ROOT_PREFIX="{WSL_MAMBA_ROOT}"
MAMBA="$(command -v micromamba)"
if "$MAMBA" env list | awk '{{print $1}}' | grep -qx "{WSL_INDEX_ENV_NAME}"; then
  "$MAMBA" install -y -n "{WSL_INDEX_ENV_NAME}" -c conda-forge -c bioconda diamond mmseqs2 foldseek
else
  "$MAMBA" create -y -n "{WSL_INDEX_ENV_NAME}" -c conda-forge -c bioconda diamond mmseqs2 foldseek
fi
mkdir -p "$HOME/.local/bin"
for tool in diamond mmseqs foldseek; do
  cat > "$HOME/.local/bin/$tool" <<SH
#!/bin/sh
export MAMBA_ROOT_PREFIX="\\${{MAMBA_ROOT_PREFIX:-\\$HOME/.genoligand/micromamba}}"
MICROMAMBA="\\${{MICROMAMBA:-\\$HOME/.local/bin/micromamba}}"
exec "\\$MICROMAMBA" run -n {WSL_INDEX_ENV_NAME} $tool "\\$@"
SH
  chmod +x "$HOME/.local/bin/$tool"
done
diamond --version
mmseqs version
foldseek version
""".strip()


def full_python_environment_repair_script(missing_packages: list[str]) -> str:
    package_args = " ".join(shlex.quote(package) for package in missing_packages)
    return f"""
set -eu
export PATH="$HOME/.local/bin:$PATH"
export MAMBA_ROOT_PREFIX="{WSL_MAMBA_ROOT}"
MAMBA="$(command -v micromamba)"
if "$MAMBA" env list | awk '{{print $1}}' | grep -qx "{WSL_FULL_ENV_NAME}"; then
  "$MAMBA" install -y -n "{WSL_FULL_ENV_NAME}" -c conda-forge {package_args}
else
  "$MAMBA" create -y -n "{WSL_FULL_ENV_NAME}" -c conda-forge python=3.12 {package_args}
fi
"$MAMBA" run -n "{WSL_FULL_ENV_NAME}" python -c "import openff.toolkit; import openmm; import rdkit; import mdtraj; import MDAnalysis; import pdbfixer"
""".strip()


def build_repair_steps(
    runtime: str,
    missing: list[str],
    workspace: Path,
    distro: str,
    wsl_python: str,
    micromamba_available: bool | None = None,
) -> tuple[str, list[dict[str, object]], str]:
    if not missing:
        return "not_needed", [], "No missing full scientific tools detected."
    steps: list[dict[str, object]] = []
    if runtime == "wsl":
        if "java" in missing:
            steps.append({
                "step_id": "install_java_with_sdkman",
                "tool_ids": ["java"],
                "reason": "Nextflow requires Java 17+; SDKMAN is the upstream-recommended non-root path.",
                "source": "https://docs.seqera.io/nextflow/install",
                "command": wsl_shell_command(distro, java_repair_script()),
            })
        if "nextflow" in missing:
            steps.append({
                "step_id": "install_nextflow_user_local",
                "tool_ids": ["nextflow"],
                "reason": "Install Nextflow into ~/.local/bin using the official self-install package.",
                "source": "https://docs.seqera.io/nextflow/install",
                "command": wsl_shell_command(distro, nextflow_repair_script()),
            })
        missing_python_packages = [
            package for tool_id, package in PYTHON_REPAIR_PACKAGES.items() if tool_id in missing
        ]
        needs_micromamba = bool(missing_python_packages or INDEX_TOOL_IDS.intersection(missing))
        has_micromamba = (
            micromamba_available
            if micromamba_available is not None
            else wsl_command_available(distro, "micromamba", workspace)
        )
        if needs_micromamba and not has_micromamba:
            steps.append({
                "step_id": "bootstrap_micromamba_user_local",
                "tool_ids": sorted(set([tool for tool in PYTHON_REPAIR_PACKAGES if tool in missing]) | INDEX_TOOL_IDS.intersection(missing)),
                "reason": "Install a user-local micromamba binary before provisioning conda-forge/bioconda scientific tools.",
                "source": "https://mamba.readthedocs.io/en/latest/installation/micromamba-installation.html",
                "command": wsl_shell_command(distro, micromamba_bootstrap_script()),
            })
        if missing_python_packages:
            steps.append({
                "step_id": "install_python_worker_conda_environment",
                "tool_ids": [tool_id for tool_id in PYTHON_REPAIR_PACKAGES if tool_id in missing],
                "reason": "Create or update the schema-bound full scientific worker environment with conda-forge packages.",
                "source": "https://docs.openforcefield.org/projects/toolkit/en/stable/",
                "command": wsl_shell_command(distro, full_python_environment_repair_script(sorted(set(PYTHON_REPAIR_PACKAGES.values())))),
            })
        if INDEX_TOOL_IDS.intersection(missing):
            steps.append({
                "step_id": "install_index_tools_with_micromamba",
                "tool_ids": sorted(INDEX_TOOL_IDS.intersection(missing)),
                "reason": "Provision DIAMOND, MMseqs2, and Foldseek from conda-forge/bioconda and expose user-local shims.",
                "source": "https://bioconda.github.io/",
                "command": wsl_shell_command(distro, index_tools_repair_script()),
            })
        if "cuda_openmm" in missing:
            steps.append({
                "step_id": "verify_cuda_openmm_after_python_repair",
                "tool_ids": ["cuda_openmm"],
                "reason": "CUDA/OpenMM depends on both the OpenMM package and NVIDIA WSL driver visibility; this step verifies the repaired environment.",
                "source": "OpenMM CUDA platform probe",
                "command": wsl_python_any_command(distro, wsl_python, ["-c", "import openmm; print(openmm.Platform.getPlatformByName('CUDA').getName())"]),
            })
        suggested = f"{sys.executable} {Path(__file__).as_posix()} --workspace {workspace.as_posix()} --mode full-scientific --target-runtime wsl --wsl-distro {distro} --wsl-python {wsl_python} --yes"
        return "wsl:stepwise_user_local", steps, suggested
    backend, install_args, install_command = detect_install_backend(runtime, workspace, distro)
    if install_args:
        steps.append({
            "step_id": f"install_with_{backend}",
            "tool_ids": missing,
            "reason": "Install missing full scientific packages with the detected host package manager.",
            "source": "local package manager",
            "command": install_args,
        })
    return backend, steps, install_command


def write_self_check_report(report_dir: Path, workspace: Path, distro: str, wsl_python: str) -> tuple[bool, dict[str, object]]:
    required_tool_ids = {
        "java",
        "nextflow",
        "python_worker",
        "openmm",
        "rdkit",
        "openff",
        "mdtraj",
        "mdanalysis",
        "pdbfixer",
        "cuda_openmm",
        "diamond",
        "mmseqs",
        "foldseek",
    }
    wsl_tool_ids = {tool_id for tool_id, _, _ in tool_checks_for_runtime("wsl", distro, wsl_python)}
    all_missing = sorted(required_tool_ids)
    _, repair_steps, suggested_command = build_repair_steps(
        "wsl",
        all_missing,
        workspace,
        distro,
        wsl_python,
        micromamba_available=False,
    )
    repair_step_ids = {str(step["step_id"]) for step in repair_steps}
    required_step_ids = {
        "install_java_with_sdkman",
        "install_nextflow_user_local",
        "bootstrap_micromamba_user_local",
        "install_python_worker_conda_environment",
        "install_index_tools_with_micromamba",
        "verify_cuda_openmm_after_python_repair",
    }
    rows = [
        [
            "required_runtime_tool_coverage",
            f"{len(required_tool_ids.intersection(wsl_tool_ids))}/{len(required_tool_ids)}",
            "ok" if required_tool_ids.issubset(wsl_tool_ids) else "issue",
        ],
        [
            "gpu_driver_probe_present",
            "nvidia_smi" in wsl_tool_ids,
            "ok" if "nvidia_smi" in wsl_tool_ids else "issue",
        ],
        [
            "repair_step_coverage",
            f"{len(required_step_ids.intersection(repair_step_ids))}/{len(required_step_ids)}",
            "ok" if required_step_ids.issubset(repair_step_ids) else "issue",
        ],
        [
            "repair_step_count",
            len(repair_steps),
            "ok" if len(repair_steps) >= len(required_step_ids) else "issue",
        ],
        [
            "single_command_execute_guard",
            "--yes" in suggested_command,
            "ok" if "--yes" in suggested_command else "issue",
        ],
        [
            "manifest_paths_declared",
            "phase09_complete_scientific_install_plan.tsv|phase09_complete_scientific_repair_plan.tsv|phase09_complete_scientific_post_repair.tsv",
            "ok",
        ],
        [
            "mojibake_guard_present",
            "utf-16-le|clean_multiline|ensure_ascii",
            "ok",
        ],
    ]
    ok = all(row[-1] == "ok" for row in rows)
    path = report_dir / "phase09_complete_scientific_installer_self_check.tsv"
    lines = ["check\tvalue\tstatus"]
    lines.extend("\t".join(clean(str(value)) for value in row) for row in rows)
    write_text(path, "\n".join(lines) + "\n")
    summary = {
        "status": "ready" if ok else "needs_attention",
        "self_check": True,
        "required_tool_ids": sorted(required_tool_ids),
        "wsl_tool_ids": sorted(wsl_tool_ids),
        "repair_step_ids": sorted(repair_step_ids),
        "suggested_command": suggested_command,
        "manifest": str(path),
    }
    write_text(
        report_dir / "phase09_complete_scientific_installer_self_check.json",
        json.dumps(summary, indent=2, ensure_ascii=True) + "\n",
    )
    return ok, summary


def execute_repair_steps(steps: list[dict[str, object]], workspace: Path) -> list[dict[str, object]]:
    results = []
    for step in steps:
        command = [str(part) for part in step["command"]]
        result = run(command, workspace, timeout_seconds=3600)
        results.append({
            "step_id": step["step_id"],
            "tool_ids": ",".join(step["tool_ids"]),
            "status": "ok" if result["ok"] == "true" else "failed",
            "exit_code": result["exit_code"],
            "stdout": result["stdout"][:1200],
            "stderr": result["stderr"][:1200],
            "command": " ".join(command),
        })
        if result["ok"] != "true":
            break
    return results


def collect_tool_rows(tool_checks: list[tuple[str, list[str], str]], workspace: Path) -> tuple[list[dict[str, str]], list[str]]:
    rows = []
    missing = []
    for tool_id, command, hint in tool_checks:
        result = run(command, workspace)
        ok = result["ok"] == "true"
        if not ok:
            missing.append(tool_id)
        rows.append({
            "tool_id": tool_id,
            "status": "ready" if ok else "missing",
            "verify_command": " ".join(command),
            "install_hint": hint,
            "stdout": result["stdout"][:600],
            "stderr": result["stderr"][:600],
        })
    return rows, missing


def write_tool_manifest(path: Path, rows: list[dict[str, str]]) -> None:
    lines = ["tool_id\tstatus\tverify_command\tinstall_hint\tstdout\tstderr"]
    for row in rows:
        lines.append("\t".join(clean(row[key]) for key in ["tool_id", "status", "verify_command", "install_hint", "stdout", "stderr"]))
    write_text(path, "\n".join(lines) + "\n")


def runtime_probe(workspace: Path) -> dict[str, object]:
    if platform.system().lower() == "windows":
        status = run(["wsl.exe", "--status"], workspace)
        distros = run(["wsl.exe", "-l", "-v"], workspace)
        return {
            "runtime_backend_type": "WSL2",
            "wsl_status_ok": status["ok"] == "true",
            "wsl_list_ok": distros["ok"] == "true",
            "wsl_status": clean_multiline(status["stdout"] or status["stderr"]),
            "wsl_distributions": clean_multiline(distros["stdout"] or distros["stderr"]),
        }
    uname = run(["uname", "-a"], workspace)
    return {
        "runtime_backend_type": "native_Linux",
        "native_linux_probe_ok": uname["ok"] == "true",
        "kernel": uname["stdout"] or uname["stderr"],
    }


def target_runtime(requested: str, workspace: Path, distro: str) -> str:
    if requested != "auto":
        return requested
    if platform.system().lower() == "windows" and command_available("wsl.exe") and wsl_available(distro, workspace):
        return "wsl"
    return "host"


def tool_checks_for_runtime(runtime: str, distro: str, wsl_python: str) -> list[tuple[str, list[str], str]]:
    if runtime == "wsl":
        py = wsl_python
        return [
            ("java", wsl_command(distro, ["java", "-version"]), "Install OpenJDK 17+ or Temurin 17+ in the project WSL backend."),
            ("nextflow", wsl_path_command(distro, ["nextflow", "-version"]), "Install Nextflow in the project WSL backend."),
            ("python_worker", wsl_python_any_command(distro, py, ["--version"]), "Use the project WSL Python worker interpreter."),
            ("openmm", wsl_python_any_command(distro, py, ["-c", "import openmm"]), "Install OpenMM from conda-forge in the project WSL Python environment."),
            ("rdkit", wsl_python_any_command(distro, py, ["-c", "import rdkit"]), "Install RDKit from conda-forge in the project WSL Python environment."),
            ("openff", wsl_python_any_command(distro, py, ["-c", "import openff.toolkit"]), "Install OpenFF Toolkit from conda-forge in the project WSL Python environment."),
            ("mdtraj", wsl_python_any_command(distro, py, ["-c", "import mdtraj"]), "Install MDTraj from conda-forge in the project WSL Python environment."),
            ("mdanalysis", wsl_python_any_command(distro, py, ["-c", "import MDAnalysis"]), "Install MDAnalysis from conda-forge in the project WSL Python environment."),
            ("pdbfixer", wsl_python_any_command(distro, py, ["-c", "import pdbfixer"]), "Install PDBFixer from conda-forge in the project WSL Python environment."),
            ("cuda_openmm", wsl_python_any_command(distro, py, ["-c", "import openmm; print(openmm.Platform.getPlatformByName('CUDA').getName())"]), "Install a CUDA-capable OpenMM build and NVIDIA WSL driver support."),
            ("nvidia_smi", wsl_command(distro, ["nvidia-smi", "--query-gpu=name,driver_version,memory.total", "--format=csv,noheader"]), "Install NVIDIA driver with WSL CUDA support."),
            ("diamond", wsl_path_command(distro, ["diamond", "--version"]), "Install DIAMOND or use MMseqs2/Foldseek for indexing."),
            ("mmseqs", wsl_path_command(distro, ["mmseqs", "version"]), "Install MMseqs2 if DIAMOND is not used."),
            ("foldseek", wsl_path_command(distro, ["foldseek", "version"]), "Install Foldseek for structure indexing."),
        ]
    return TOOL_CHECKS


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Detect and optionally repair GenoLigand full scientific dependencies.")
    parser.add_argument("--workspace", default=".")
    parser.add_argument("--report-dir", default="acceptance_artifacts_phase_0_9_1_production_webui/setup")
    parser.add_argument("--mode", default="full-scientific", choices=["demo-only", "full-scientific"])
    parser.add_argument("--target-runtime", default="auto", choices=["auto", "host", "wsl"])
    parser.add_argument("--wsl-distro", default=DEFAULT_WSL_DISTRO)
    parser.add_argument("--wsl-python", default=DEFAULT_WSL_PYTHON)
    parser.add_argument("--self-check", action="store_true", help="Validate installer coverage without probing, downloading, or installing tools.")
    parser.add_argument("--yes", action="store_true", help="Execute installation commands. Without this flag, only write a plan.")
    args = parser.parse_args()

    workspace = Path(args.workspace).resolve()
    report_dir = Path(args.report_dir).resolve()
    if args.self_check:
        ok, summary = write_self_check_report(report_dir, workspace, args.wsl_distro, args.wsl_python)
        print(json.dumps(summary, ensure_ascii=True))
        return 0 if ok else 2

    runtime = runtime_probe(workspace)
    selected_runtime = target_runtime(args.target_runtime, workspace, args.wsl_distro)
    tool_checks = tool_checks_for_runtime(selected_runtime, args.wsl_distro, args.wsl_python)
    rows, missing = collect_tool_rows(tool_checks, workspace)
    backend, repair_steps, install_command = build_repair_steps(
        selected_runtime,
        missing,
        workspace,
        args.wsl_distro,
        args.wsl_python,
    )

    execution_status = "planned"
    execution_results: list[dict[str, object]] = []
    missing_after_repair = missing
    if args.yes and missing:
        execution_status = "executed"
        execution_results = execute_repair_steps(repair_steps, workspace)
        rows, missing_after_repair = collect_tool_rows(tool_checks, workspace)

    manifest = report_dir / "phase09_complete_scientific_install_plan.tsv"
    write_tool_manifest(manifest, rows)
    if args.yes:
        write_tool_manifest(report_dir / "phase09_complete_scientific_post_repair.tsv", rows)

    repair_plan = report_dir / "phase09_complete_scientific_repair_plan.tsv"
    repair_lines = ["step_id\ttool_ids\treason\tsource\tcommand"]
    for step in repair_steps:
        command = " ".join(str(part) for part in step["command"])
        repair_lines.append("\t".join(clean(str(value)) for value in [
            step["step_id"],
            ",".join(step["tool_ids"]),
            step["reason"],
            step["source"],
            command,
        ]))
    write_text(repair_plan, "\n".join(repair_lines) + "\n")

    execution_manifest = report_dir / "phase09_complete_scientific_repair_execution.tsv"
    execution_lines = ["step_id\ttool_ids\tstatus\texit_code\tstdout\tstderr\tcommand"]
    for result in execution_results:
        execution_lines.append("\t".join(clean(str(result[key])) for key in ["step_id", "tool_ids", "status", "exit_code", "stdout", "stderr", "command"]))
    write_text(execution_manifest, "\n".join(execution_lines) + "\n")

    final_missing = missing_after_repair if args.yes else missing
    failed_steps = [result for result in execution_results if result.get("status") != "ok"]

    summary = {
        "status": "ready" if not final_missing else "needs_repair",
        "mode": args.mode,
        "execute_requested": args.yes,
        "execution_status": execution_status,
        "platform": platform.platform(),
        "runtime_probe": runtime,
        "selected_runtime": selected_runtime,
        "wsl_distro": args.wsl_distro if selected_runtime == "wsl" else "",
        "wsl_python": args.wsl_python if selected_runtime == "wsl" else "",
        "wsl_complete_scientific_python": DEFAULT_WSL_FULL_PYTHON if selected_runtime == "wsl" else "",
        "python": sys.executable,
        "install_backend": backend,
        "repair_step_count": len(repair_steps),
        "repair_steps": [
            {
                "step_id": str(step["step_id"]),
                "tool_ids": list(step["tool_ids"]),
                "reason": str(step["reason"]),
                "source": str(step["source"]),
                "command": " ".join(str(part) for part in step["command"]),
            }
            for step in repair_steps
        ],
        "single_install_command": f"{sys.executable} {Path(__file__).as_posix()} --workspace {workspace.as_posix()} --report-dir {report_dir.as_posix()} --mode full-scientific --target-runtime {selected_runtime} --wsl-distro {args.wsl_distro} --wsl-python {args.wsl_python} --yes",
        "suggested_package_command": install_command,
        "missing_tool_ids": missing,
        "post_repair_status": "checked" if args.yes else "not_run",
        "missing_after_repair": missing_after_repair if args.yes else None,
        "failed_repair_step_count": len(failed_steps),
        "manifest": str(manifest),
        "repair_plan": str(repair_plan),
        "repair_execution_manifest": str(execution_manifest),
        "execution_results": execution_results,
    }
    write_text(report_dir / "phase09_complete_scientific_install_summary.json", json.dumps(summary, indent=2))
    print(json.dumps(summary, ensure_ascii=True))
    return 0 if not final_missing or not args.yes else 2


if __name__ == "__main__":
    raise SystemExit(main())
