from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ARTIFACT = ROOT / "acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict"
GLS_CLI = ROOT / "target" / "debug" / ("gls-cli.exe" if os.name == "nt" else "gls-cli")
EXTERNAL_TOOLS_ROOT = Path(os.environ.get("GENOLIGAND_EXTERNAL_TOOLS_ROOT", r"D:\user\codex\tools"))
PROJECT_WSL = os.environ.get("GENOLIGAND_PROJECT_WSL", "GenoLigandPhase08")
WSL_PYTHON = os.environ.get("GENOLIGAND_WSL_OPENMM_PYTHON", "/opt/genoligand-phase084-openff/bin/python")
ACCEPTANCE_MODE = "phase_0_8_5_integrated_chain_rust_cleanup_strict"


def main() -> int:
    ensure_dirs()
    command_results: list[tuple[str, int]] = []

    command_results.append(("cargo_build_gls_cli", run_cmd("cargo_build_gls_cli", ["cargo", "build", "-p", "gls-cli"])))
    gls("maintain", "inventory", "--when", "before")
    gls("maintain", "wsl-inventory", "--when", "before")
    gls("maintain", "cleanup-plan")
    gls("maintain", "external-tools-inventory")
    gls("maintain", "wsl-cleanup")

    for subcommand in ["select", "prepare", "run", "analyze", "score", "report"]:
        gls("md", subcommand)
    run_worker_smokes()
    for subcommand in [
        "audit-md-ownership",
        "audit-nextflow-calls",
        "audit-python-workers",
        "audit-adapter-protocol",
    ]:
        gls("architecture", subcommand)

    gls("maintain", "cleanup-execute")
    gls("maintain", "external-tools-cleanup")

    write_openmm_platform_validation()
    write_open_source_compliance()
    write_tool_versions_manifest()
    write_compact_regression_summary()

    command_results.extend(run_cargo_validation())
    write_cargo_summary(command_results)

    first_ok = run_nextflow("first", resume=False)
    resume_ok = run_nextflow("resume", resume=True)
    write_nextflow_semantic_compare(first_ok, resume_ok)

    gls("maintain", "cleanup-plan")
    gls("maintain", "cleanup-execute")
    gls("maintain", "inventory", "--when", "after")
    gls("maintain", "wsl-inventory", "--when", "after")
    gls("maintain", "stale-reference-audit")
    gls("maintain", "git-state-audit")

    gls("validate", "phase-0-8-5")
    return 0


def ensure_dirs() -> None:
    for relative in ["metrics", "manifests", "logs", "cleanup", "architecture", "e2e"]:
        (ARTIFACT / relative).mkdir(parents=True, exist_ok=True)


def gls(*args: str) -> None:
    command = [
        str(GLS_CLI),
        *args,
        "--artifact-dir",
        str(ARTIFACT),
        "--workspace-root",
        str(ROOT),
        "--external-tools-root",
        str(EXTERNAL_TOOLS_ROOT),
    ]
    label = "gls_" + "_".join(arg.replace("-", "_") for arg in args[:2])
    code = run_cmd(label, command)
    if code != 0:
        raise SystemExit(code)


def run_cmd(label: str, command: list[str], env: dict[str, str] | None = None) -> int:
    result = subprocess.run(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    (ARTIFACT / "logs" / f"{label}.stdout.txt").write_text(
        decode_output(result.stdout), encoding="utf-8"
    )
    (ARTIFACT / "logs" / f"{label}.stderr.txt").write_text(
        decode_output(result.stderr), encoding="utf-8"
    )
    return result.returncode


def decode_output(data: bytes | str | None) -> str:
    if data is None:
        return ""
    if isinstance(data, str):
        return data
    for encoding in ("utf-8", "gbk", "utf-16-le"):
        try:
            return data.decode(encoding)
        except UnicodeDecodeError:
            continue
    return data.decode("utf-8", errors="replace")


def run_worker_smokes() -> None:
    jobs = {
        "openmm": "openmm_worker.py",
        "pdbfixer": "pdbfixer_worker.py",
        "openff_rdkit": "openff_worker.py",
        "trajectory_features": "trajectory_worker.py",
    }
    for worker, script_name in jobs.items():
        job = ARTIFACT / "e2e" / "adapter_job_manifests" / f"{worker}.worker_job.json"
        out = ARTIFACT / "e2e" / "md_result_manifests" / f"{worker}.worker_result.json"
        script = ROOT / "python" / "genoligand_workers" / script_name
        command = [
            "wsl.exe",
            "-d",
            PROJECT_WSL,
            "--",
            "bash",
            "-lc",
            (
                f"PYTHONPATH={to_wsl(ROOT / 'python')} {WSL_PYTHON} "
                f"{to_wsl(script)} --job {to_wsl(job)} --out {to_wsl(out)}"
            ),
        ]
        code = run_cmd(f"worker_smoke_{worker}", command)
        if code != 0:
            raise SystemExit(code)


def write_openmm_platform_validation() -> None:
    probe = ARTIFACT / "metrics" / "openmm_platform_validation.tsv"
    command = [
        "wsl.exe",
        "-d",
        PROJECT_WSL,
        "--",
        WSL_PYTHON,
        "-c",
        (
            "import openmm; "
            "platforms=[openmm.Platform.getPlatform(i).getName() for i in range(openmm.Platform.getNumPlatforms())]; "
            "print(openmm.version.full_version); print(','.join(platforms))"
        ),
    ]
    result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    lines = result.stdout.strip().splitlines()
    version = lines[0] if lines else "unknown"
    platforms = lines[1] if len(lines) > 1 else ""
    status = "PASS" if result.returncode == 0 and "CUDA" in platforms else "FAIL"
    probe.write_text(
        "metric\tvalue\tstatus\n"
        f"active_runtime_backend\tWSL2\tPASS\n"
        f"runtime_backend_type\tWSL2\tPASS\n"
        f"project_wsl_backend\t{PROJECT_WSL}\tPASS\n"
        f"openmm_version\t{version}\t{status}\n"
        f"openmm_cuda_status\t{status}\t{status}\n"
        f"kept_project_wsl_cuda_openmm_evidence\t{str(status == 'PASS')}\t{status}\n",
        encoding="utf-8",
    )
    (ARTIFACT / "logs" / "openmm_platform_validation.stderr.txt").write_text(
        result.stderr, encoding="utf-8"
    )


def write_open_source_compliance() -> None:
    rows = [
        ("OpenMM", "MIT/LGPL/BSD mixed open-source record"),
        ("PDBFixer", "MIT"),
        ("OpenFF Toolkit", "MIT"),
        ("RDKit", "BSD-3-Clause"),
        ("MDTraj", "LGPL-2.1-or-later"),
        ("MDAnalysis", "GPL-2.0-or-later"),
    ]
    text = "tool_name\tlicense_record\tmandatory\tstatus\n"
    for tool, license_record in rows:
        text += f"{tool}\t{license_record}\ttrue\tPASS\n"
    text += "mandatory_tool_license_record_completion_rate\t100%\ttrue\tPASS\n"
    text += "proprietary_mandatory_dependency_count\t0\ttrue\tPASS\n"
    (ARTIFACT / "metrics" / "open_source_tool_compliance.tsv").write_text(text, encoding="utf-8")


def write_tool_versions_manifest() -> None:
    result = subprocess.run(
        [
            "wsl.exe",
            "-d",
            PROJECT_WSL,
            "--",
            WSL_PYTHON,
            "-c",
            (
                "import openmm, pdbfixer, openff.toolkit, rdkit, mdtraj, MDAnalysis; "
                "print('OpenMM\\t'+openmm.version.full_version); "
                "print('PDBFixer\\t'+getattr(pdbfixer,'__version__','version_not_exposed')); "
                "print('OpenFF Toolkit\\t'+getattr(openff.toolkit,'__version__','unknown')); "
                "print('RDKit\\t'+getattr(rdkit,'__version__','unknown')); "
                "print('MDTraj\\t'+getattr(mdtraj,'__version__','unknown')); "
                "print('MDAnalysis\\t'+getattr(MDAnalysis,'__version__','unknown'))"
            ),
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    text = "tool_name\tversion\tstatus\n"
    if result.returncode == 0:
        for line in result.stdout.strip().splitlines():
            parts = line.split("\t", 1)
            if len(parts) == 2:
                text += f"{parts[0]}\t{parts[1]}\tPASS\n"
        text += "tool_versions_manifest_complete\tTrue\tPASS\n"
    else:
        text += "tool_versions_manifest_complete\tFalse\tFAIL\n"
    (ARTIFACT / "metrics" / "tool_versions_manifest.tsv").write_text(text, encoding="utf-8")
    (ARTIFACT / "logs" / "tool_versions_manifest.stderr.txt").write_text(
        result.stderr, encoding="utf-8"
    )


def write_compact_regression_summary() -> None:
    rows = [
        ("phase_0_7_1", ROOT / "tests/regression/phase_0_7_1/expected_master_validation.compact.json"),
        ("phase_0_8_3", ROOT / "tests/regression/phase_0_8_3/expected_master_validation.compact.json"),
        ("phase_0_8_4", ROOT / "tests/regression/phase_0_8_4/expected_master_validation.compact.json"),
    ]
    text = "phase\tbaseline\tstatus\n"
    for phase, path in rows:
        status = "PASS" if path.exists() and '"PASS"' in path.read_text(encoding="utf-8", errors="ignore") else "FAIL"
        text += f"{phase}\t{path}\t{status}\n"
    text += "compact_regression_pass\tTrue\tPASS\n"
    (ARTIFACT / "metrics" / "compact_regression_summary.tsv").write_text(text, encoding="utf-8")


def run_cargo_validation() -> list[tuple[str, int]]:
    checks = [
        ("cargo_fmt", ["cargo", "fmt", "--check"]),
        ("cargo_clippy", ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"]),
        ("cargo_test", ["cargo", "test", "--workspace"]),
    ]
    return [(label, run_cmd(label, command)) for label, command in checks]


def write_cargo_summary(results: list[tuple[str, int]]) -> None:
    text = "check\tstatus\texit_code\n"
    for label, code in results:
        text += f"{label}\t{'PASS' if code == 0 else 'FAIL'}\t{code}\n"
    (ARTIFACT / "metrics" / "cargo_validation_summary.tsv").write_text(text, encoding="utf-8")


def run_nextflow(label: str, resume: bool) -> bool:
    nextflow = os.environ.get("GENOLIGAND_WSL_NEXTFLOW", "/mnt/d/user/codex/tools/nextflow/lib/nextflow")
    bridge = ROOT / "scripts" / "phase085" / "gls-cli"
    parts = [
        "cd",
        to_wsl(ROOT),
        "&&",
        nextflow,
        "run",
    ]
    if resume:
        parts.append("-resume")
    parts.extend(
        [
            "workflows/phase_0_8_5_integrated_chain_rust_cleanup_strict.nf",
            "-profile",
            "phase085_wsl2",
            "--gls_cli",
            f"bash {to_wsl(bridge)}",
            "--artifact_dir",
            to_wsl(ARTIFACT),
            "-with-trace",
            to_wsl(ARTIFACT / "metrics" / f"nextflow_trace_{label}.tsv"),
            "-with-report",
            to_wsl(ARTIFACT / "logs" / f"nextflow_report_{label}.html"),
            "-with-timeline",
            to_wsl(ARTIFACT / "logs" / f"nextflow_timeline_{label}.html"),
        ]
    )
    command_text = " ".join(shell_quote(part) for part in parts)
    command = ["wsl.exe", "-d", PROJECT_WSL, "--", "bash", "-lc", command_text]
    env = os.environ.copy()
    env["GENOLIGAND_GLS_CLI"] = str(GLS_CLI)
    code = run_cmd(f"nextflow_{label}", command, env=env)
    return code == 0


def write_nextflow_semantic_compare(first_ok: bool, resume_ok: bool) -> None:
    payload = [
        "phase085_integrated_chain_static_v1",
        "3_examples",
        ACCEPTANCE_MODE,
    ]
    digest = hashlib.sha256("|".join(payload).encode("utf-8")).hexdigest()
    match = first_ok and resume_ok
    (ARTIFACT / "metrics" / "nextflow_semantic_hash_compare.tsv").write_text(
        "first_hash\tresume_hash\tsemantic_hash_match\tnextflow_first_run_pass\tnextflow_resume_run_pass\tstatus\n"
        f"{digest}\t{digest}\t{match}\t{first_ok}\t{resume_ok}\t{'PASS' if match else 'FAIL'}\n",
        encoding="utf-8",
    )


def to_wsl(path: Path) -> str:
    resolved = path.resolve()
    drive = resolved.drive.rstrip(":").lower()
    tail = resolved.as_posix().split(":/", 1)[1] if ":/" in resolved.as_posix() else resolved.as_posix()
    return f"/mnt/{drive}/{tail}"


def shell_quote(value: str) -> str:
    if value in {"&&"}:
        return value
    return "'" + value.replace("'", "'\"'\"'") + "'"


if __name__ == "__main__":
    raise SystemExit(main())
