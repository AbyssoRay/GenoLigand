from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_phase06.common import ensure_dir, sha256_file, write_tsv
from genoligand_phase07.audit_fix import PHASE07_DIRNAME, PHASE071_DIRNAME, render_acceptance_report, validate_phase071


DEFAULT_PYTHON = r"D:\user\codex\tools\genoligand-phase07\python312-embed\python.exe"
DEFAULT_VINA = r"D:\user\codex\tools\genoligand-phase07\vina_1.2.7_win.exe"
DEFAULT_LIGAND_PREP = r"D:\user\codex\tools\genoligand-phase07\python312-embed\Scripts\mk_prepare_ligand.exe"
DEFAULT_RECEPTOR_PREP = r"D:\user\codex\tools\genoligand-phase07\python312-embed\Scripts\mk_prepare_receptor.exe"
DEFAULT_OBABEL = r"D:\user\codex\tools\genoligand-phase07\python312-embed\Scripts\obabel.exe"
DEFAULT_JAVA = r"D:\user\codex\tools\java\jdk-25.0.3+9\bin\java.exe"
DEFAULT_NEXTFLOW_JAR = ROOT / "acceptance_artifacts_phase_0_2/nextflow_rebuild/nextflow-patched.jar"


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def quote(command: list[str]) -> str:
    return " ".join(f'"{part}"' if " " in str(part) else str(part) for part in command)


def path_arg(value: str) -> Path:
    return Path(value).resolve()


def command_env() -> dict[str, str]:
    env = os.environ.copy()
    support_paths = [str(ROOT / "scripts/audit"), str(ROOT / "scripts/phase07"), str(ROOT / "scripts/validation"), str(ROOT / "python")]
    env["PYTHONPATH"] = os.pathsep.join(support_paths + ([env["PYTHONPATH"]] if env.get("PYTHONPATH") else []))
    git_usr = Path(r"D:\Program Files\Git\usr\bin")
    git_bin = Path(r"D:\Program Files\Git\bin")
    prepend = [str(path) for path in [git_usr, git_bin] if path.exists()]
    if prepend:
        env["PATH"] = os.pathsep.join(prepend + [env.get("PATH", "")])
    return env


def run_command(command_id: str, phase: str, purpose: str, command: list[str], artifact_dir: Path, rows: list[dict[str, object]], primary_output: Path | None = None, timeout: int = 14400, allow_fail: bool = False) -> int:
    log_dir = ensure_dir(artifact_dir / "logs/command_stdout_stderr")
    stdout = log_dir / f"{command_id}.stdout.log"
    stderr = log_dir / f"{command_id}.stderr.log"
    started = utc_now()
    with stdout.open("w", encoding="utf-8", errors="replace") as out, stderr.open("w", encoding="utf-8", errors="replace") as err:
        try:
            completed = subprocess.run(command, cwd=ROOT, text=True, stdout=out, stderr=err, check=False, timeout=timeout, env=command_env())
            code = completed.returncode
        except Exception as exc:  # noqa: BLE001
            err.write(repr(exc) + "\n")
            code = 1
    ended = utc_now()
    output_path = str(primary_output) if primary_output else ""
    output_hash = sha256_file(primary_output) if primary_output and primary_output.exists() and primary_output.is_file() else ""
    rows.append(
        {
            "command_id": command_id,
            "phase": phase,
            "purpose": purpose,
            "command": quote(command),
            "working_directory": str(ROOT),
            "started_at_utc": started,
            "ended_at_utc": ended,
            "exit_code": code,
            "stdout_log": str(stdout),
            "stderr_log": str(stderr),
            "primary_output": output_path,
            "primary_output_sha256": output_hash,
            "notes": "allowed_fail_for_final_fail_report" if allow_fail and code != 0 else "",
        }
    )
    write_command_summary(artifact_dir / "metrics/command_summary.tsv", rows)
    return code


def write_command_summary(path: Path, rows: list[dict[str, object]]) -> None:
    write_tsv(
        path,
        rows,
        [
            "command_id",
            "phase",
            "purpose",
            "command",
            "working_directory",
            "started_at_utc",
            "ended_at_utc",
            "exit_code",
            "stdout_log",
            "stderr_log",
            "primary_output",
            "primary_output_sha256",
            "notes",
        ],
    )


def nextflow_command(args: argparse.Namespace, trace: Path, report: Path, timeline: Path, resume: bool) -> list[str]:
    for path in [trace, report, timeline]:
        if path.exists():
            path.unlink()
    if args.nextflow_jar.exists():
        base = [args.java, "-jar", str(args.nextflow_jar), "run", "workflows/phase_0_7_anti_hallucination_strict.nf"]
    else:
        base = [args.nextflow, "run", "workflows/phase_0_7_anti_hallucination_strict.nf"]
    if resume:
        base.append("-resume")
    base.extend(
        [
            "-profile",
            "control_only",
            "--artifact_dir",
            str(args.artifact_dir),
            "--phase07_dir",
            str(args.phase07_dir),
            "--python",
            args.python,
            "-with-trace",
            str(trace),
            "-with-report",
            str(report),
            "-with-timeline",
            str(timeline),
        ]
    )
    return base


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE071_DIRNAME)
    parser.add_argument("--phase07-dir", type=path_arg, default=ROOT / PHASE07_DIRNAME)
    parser.add_argument("--python", default=os.environ.get("GENOLIGAND_PHASE07_PYTHON", DEFAULT_PYTHON if Path(DEFAULT_PYTHON).exists() else sys.executable))
    parser.add_argument("--vina-bin", default=os.environ.get("GENOLIGAND_PHASE07_VINA", DEFAULT_VINA))
    parser.add_argument("--ligand-prep-command", default=os.environ.get("GENOLIGAND_PHASE07_LIGAND_PREP", DEFAULT_LIGAND_PREP))
    parser.add_argument("--receptor-prep-command", default=os.environ.get("GENOLIGAND_PHASE07_RECEPTOR_PREP", DEFAULT_RECEPTOR_PREP))
    parser.add_argument("--obabel-bin", default=os.environ.get("GENOLIGAND_PHASE07_OBABEL", DEFAULT_OBABEL))
    parser.add_argument("--java", default=os.environ.get("JAVA_EXE", DEFAULT_JAVA))
    parser.add_argument("--nextflow-jar", type=path_arg, default=DEFAULT_NEXTFLOW_JAR)
    parser.add_argument("--nextflow", default=os.environ.get("NEXTFLOW_BIN", "nextflow"))
    args = parser.parse_args()

    artifact = args.artifact_dir
    if artifact.exists():
        shutil.rmtree(artifact)
    for rel in [
        "inputs",
        "outputs/reports",
        "outputs/audit",
        "outputs/docking_audit",
        "outputs/workflow",
        "outputs/versions",
        "metrics",
        "logs/command_stdout_stderr",
    ]:
        ensure_dir(artifact / rel)

    py = args.python
    rows: list[dict[str, object]] = []
    failures: list[str] = []

    commands: list[tuple[str, str, str, list[str], Path | None]] = [
        ("inventory_phase07_artifacts", "0.7.1", "inventory existing Phase 0.7 artifacts", [py, "scripts/audit/inventory_phase07_artifacts.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)], artifact / "inputs/phase_0_7_artifact_manifest.tsv"),
        (
            "extract_vina_version",
            "0.7.1",
            "extract real tool versions",
            [
                py,
                "scripts/audit/extract_vina_version.py",
                "--artifact-dir",
                str(artifact),
                "--python",
                py,
                "--vina-bin",
                args.vina_bin,
                "--ligand-prep-command",
                args.ligand_prep_command,
                "--receptor-prep-command",
                args.receptor_prep_command,
                "--obabel-bin",
                args.obabel_bin,
                "--java",
                args.java,
                "--nextflow-jar",
                str(args.nextflow_jar),
                "--nextflow",
                args.nextflow,
            ],
            artifact / "outputs/versions/tool_versions.tsv",
        ),
        ("write_audit_source_manifests", "0.7.1", "write official audit source manifests", [py, "scripts/audit/write_audit_source_manifests.py", "--artifact-dir", str(artifact)], artifact / "audit_source_manifest.tsv"),
        ("audit_docking_artifacts", "0.7.1", "audit docking jobs poses scores and redocking", [py, "scripts/audit/audit_docking_artifacts_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)], artifact / "outputs/docking_audit/docking_job_audit.tsv"),
        ("expand_decoy_superiority", "0.7.1", "expand decoy superiority warnings and confidence downgrade", [py, "scripts/audit/expand_decoy_superiority_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)], artifact / "outputs/docking_audit/decoy_superiority_expanded.tsv"),
    ]
    for command_id, phase, purpose, command, primary in commands:
        code = run_command(command_id, phase, purpose, command, artifact, rows, primary_output=primary)
        if code != 0:
            failures.append(f"{command_id} failed with exit code {code}")

    first_trace = artifact / "outputs/workflow/nextflow_trace_first.tsv"
    resume_trace = artifact / "outputs/workflow/nextflow_trace_resume.tsv"
    first_cmd = nextflow_command(args, first_trace, artifact / "outputs/workflow/nextflow_report_first.html", artifact / "outputs/workflow/nextflow_timeline_first.html", resume=False)
    code = run_command("nextflow_phase_0_7_1_first", "0.7.1", "run first Nextflow audit workflow", first_cmd, artifact, rows, primary_output=first_trace)
    if code != 0:
        failures.append(f"nextflow first failed with exit code {code}")
    code = run_command(
        "semantic_hash_first",
        "0.7.1",
        "compute first semantic hashes",
        [py, "scripts/audit/semantic_hashes_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir), "--out", str(artifact / "outputs/workflow/phase07_semantic_hash_first.tsv")],
        artifact,
        rows,
        primary_output=artifact / "outputs/workflow/phase07_semantic_hash_first.tsv",
    )
    if code != 0:
        failures.append(f"semantic hash first failed with exit code {code}")

    resume_cmd = nextflow_command(args, resume_trace, artifact / "outputs/workflow/nextflow_report_resume.html", artifact / "outputs/workflow/nextflow_timeline_resume.html", resume=True)
    code = run_command("nextflow_phase_0_7_1_resume", "0.7.1", "run resume Nextflow audit workflow", resume_cmd, artifact, rows, primary_output=resume_trace)
    if code != 0:
        failures.append(f"nextflow resume failed with exit code {code}")
    for command_id, purpose, command, primary in [
        (
            "semantic_hash_resume",
            "compute resume semantic hashes",
            [py, "scripts/audit/semantic_hashes_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir), "--out", str(artifact / "outputs/workflow/phase07_semantic_hash_resume.tsv")],
            artifact / "outputs/workflow/phase07_semantic_hash_resume.tsv",
        ),
        (
            "compare_phase07_semantic_hashes",
            "compare first and resume semantic hashes",
            [py, "scripts/audit/compare_phase07_semantic_hashes.py", "--artifact-dir", str(artifact)],
            artifact / "outputs/workflow/phase07_semantic_hash_compare.tsv",
        ),
        (
            "check_nextflow_trace_phase07",
            "validate Nextflow first and resume traces",
            [py, "scripts/audit/check_nextflow_trace_phase07.py", "--artifact-dir", str(artifact)],
            artifact / "outputs/workflow/nextflow_trace_audit.tsv",
        ),
        (
            "hash_phase07_artifacts_after",
            "hash Phase 0.7 artifacts after audit",
            [py, "scripts/audit/hash_phase07_artifacts_after.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)],
            artifact / "inputs/phase_0_7_artifact_hashes_after.tsv",
        ),
    ]:
        code = run_command(command_id, "0.7.1", purpose, command, artifact, rows, primary_output=primary)
        if code != 0:
            failures.append(f"{command_id} failed with exit code {code}")

    validate_phase071(artifact, args.phase07_dir, include_report=False)
    for command_id, purpose, command, primary in [
        (
            "build_evidence_ledger_phase_0_7_1",
            "map report claims to evidence rows",
            [py, "scripts/audit/build_evidence_ledger_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)],
            artifact / "outputs/audit/evidence_ledger_expanded.tsv",
        ),
        (
            "render_candidate_report_phase_0_7_1",
            "render control-only candidate audit report",
            [py, "scripts/audit/render_candidate_report_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)],
            artifact / "outputs/reports/candidate_report_phase_0_7_1.md",
        ),
        (
            "render_acceptance_report_phase_0_7_1_draft",
            "render draft acceptance report from validation summary",
            [py, "scripts/audit/render_acceptance_report_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)],
            artifact / "acceptance_report_phase_0_7_1.md",
        ),
        (
            "report_claim_lint_phase_0_7_1",
            "lint report claims",
            [py, "scripts/audit/report_claim_lint_phase_0_7_1.py", "--artifact-dir", str(artifact)],
            artifact / "outputs/audit/report_claim_lint.tsv",
        ),
        (
            "no_placeholder_artifacts_phase_0_7_1",
            "audit required outputs for placeholders",
            [py, "scripts/audit/no_placeholder_artifacts_phase_0_7_1.py", "--artifact-dir", str(artifact)],
            artifact / "outputs/audit/no_placeholder_artifacts.tsv",
        ),
        (
            "write_hashes_phase_0_7_1",
            "hash Phase 0.7.1 acceptance archive",
            [py, "scripts/audit/write_hashes_phase_0_7_1.py", "--artifact-dir", str(artifact)],
            artifact / "hashes.tsv",
        ),
    ]:
        code = run_command(command_id, "0.7.1", purpose, command, artifact, rows, primary_output=primary)
        if code != 0:
            failures.append(f"{command_id} failed with exit code {code}")

    first_validation = run_command(
        "check_phase_0_7_1_audit_fix_strict",
        "0.7.1",
        "run master validation script",
        [py, "scripts/validation/check_phase_0_7_1_audit_fix_strict.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir), "--strict"],
        artifact,
        rows,
        primary_output=artifact / "metrics/master_validation_summary.json",
        allow_fail=True,
    )
    if first_validation != 0:
        failures.append(f"master validation failed with exit code {first_validation}")

    # Regenerate evidence/report from the mechanical master summary, then validate once more.
    for command_id, purpose, command, primary in [
        (
            "build_evidence_ledger_phase_0_7_1_final",
            "map final report claims to final evidence rows",
            [py, "scripts/audit/build_evidence_ledger_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)],
            artifact / "outputs/audit/evidence_ledger_expanded.tsv",
        ),
        (
            "render_acceptance_report_phase_0_7_1_final",
            "render final acceptance report from master validation summary",
            [py, "scripts/audit/render_acceptance_report_phase_0_7_1.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir)],
            artifact / "acceptance_report_phase_0_7_1.md",
        ),
        (
            "report_claim_lint_phase_0_7_1_final",
            "lint final report claims",
            [py, "scripts/audit/report_claim_lint_phase_0_7_1.py", "--artifact-dir", str(artifact)],
            artifact / "outputs/audit/report_claim_lint.tsv",
        ),
        (
            "no_placeholder_artifacts_phase_0_7_1_final",
            "audit final required outputs for placeholders",
            [py, "scripts/audit/no_placeholder_artifacts_phase_0_7_1.py", "--artifact-dir", str(artifact)],
            artifact / "outputs/audit/no_placeholder_artifacts.tsv",
        ),
        (
            "write_hashes_phase_0_7_1_final",
            "hash final Phase 0.7.1 acceptance archive",
            [py, "scripts/audit/write_hashes_phase_0_7_1.py", "--artifact-dir", str(artifact)],
            artifact / "hashes.tsv",
        ),
    ]:
        code = run_command(command_id, "0.7.1", purpose, command, artifact, rows, primary_output=primary)
        if code != 0:
            failures.append(f"{command_id} failed with exit code {code}")

    final_validation = run_command(
        "check_phase_0_7_1_audit_fix_strict_final",
        "0.7.1",
        "run final master validation script",
        [py, "scripts/validation/check_phase_0_7_1_audit_fix_strict.py", "--artifact-dir", str(artifact), "--phase07-dir", str(args.phase07_dir), "--strict"],
        artifact,
        rows,
        primary_output=artifact / "metrics/master_validation_summary.json",
        allow_fail=True,
    )
    if final_validation != 0:
        failures.append(f"final master validation failed with exit code {final_validation}")

    render_acceptance_report(artifact, args.phase07_dir)
    run_command(
        "write_hashes_phase_0_7_1_post_final_report",
        "0.7.1",
        "hash archive after final report render",
        [py, "scripts/audit/write_hashes_phase_0_7_1.py", "--artifact-dir", str(artifact)],
        artifact,
        rows,
        primary_output=artifact / "hashes.tsv",
    )

    if final_validation == 0 and not failures:
        print("phase_0_7_1_audit_fix_strict_acceptance_pass")
        return 0
    print("phase_0_7_1_audit_fix_strict_acceptance_fail")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
