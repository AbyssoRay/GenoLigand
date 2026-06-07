from __future__ import annotations

import argparse
import csv
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from _bootstrap import (
    DEFAULT_ARTIFACT_DIR,
    DEFAULT_DOCKING_CONFIG,
    DEFAULT_IFP_CONFIG,
    DEFAULT_PHASE05_DIR,
    DEFAULT_PHASE06_DIR,
    ROOT,
    env_default,
)

sys.path.insert(0, str(ROOT / "python"))

from genoligand_phase06.common import ensure_dir, sha256_file, write_tsv
from genoligand_phase07.pipeline import render_acceptance_report, semantic_hashes, write_archive_hashes


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def run_command(command_id: str, command: list[str], root: Path, artifact_dir: Path, rows: list[dict[str, object]]) -> int:
    log_dir = ensure_dir(artifact_dir / "logs/command_stdout_stderr")
    stdout = log_dir / f"{command_id}.stdout.log"
    stderr = log_dir / f"{command_id}.stderr.log"
    started = utc_now()
    env = os.environ.copy()
    git_usr = Path(r"D:\Program Files\Git\usr\bin")
    git_bin = Path(r"D:\Program Files\Git\bin")
    prepend = [str(path) for path in [git_usr, git_bin] if path.exists()]
    if prepend:
        env["PATH"] = os.pathsep.join(prepend + [env.get("PATH", "")])
    support_paths = [str(root / "scripts/phase07"), str(root / "scripts/validation"), str(root / "python")]
    env["PYTHONPATH"] = os.pathsep.join(support_paths + ([env["PYTHONPATH"]] if env.get("PYTHONPATH") else []))
    with stdout.open("w", encoding="utf-8", errors="replace") as out, stderr.open("w", encoding="utf-8", errors="replace") as err:
        try:
            completed = subprocess.run(command, cwd=root, text=True, stdout=out, stderr=err, check=False, timeout=14400, env=env)
            code = completed.returncode
        except Exception as exc:  # noqa: BLE001
            err.write(repr(exc) + "\n")
            code = 1
    ended = utc_now()
    rows.append(
        {
            "command_id": command_id,
            "command": " ".join(f'"{part}"' if " " in str(part) else str(part) for part in command),
            "working_directory": str(root),
            "started_at_utc": started,
            "ended_at_utc": ended,
            "exit_code": code,
            "stdout_log": str(stdout),
            "stderr_log": str(stderr),
            "primary_output": "",
            "primary_output_sha256": "",
            "notes": "",
        }
    )
    return code


def write_command_summary(path: Path, rows: list[dict[str, object]]) -> None:
    write_tsv(
        path,
        rows,
        [
            "command_id",
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


def compare_semantic_hashes(first: Path, resume: Path, out: Path) -> None:
    def load(path: Path) -> dict[str, str]:
        if not path.exists():
            return {}
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            return {row["artifact"]: row["semantic_hash"] for row in csv.DictReader(handle, delimiter="\t")}

    first_map = load(first)
    resume_map = load(resume)
    artifacts = sorted(set(first_map) | set(resume_map))
    rows = [
        {
            "artifact": artifact,
            "first_semantic_hash": first_map.get(artifact, "missing"),
            "resume_semantic_hash": resume_map.get(artifact, "missing"),
            "match": str(first_map.get(artifact, "") == resume_map.get(artifact, "") and artifact in first_map and artifact in resume_map).lower(),
        }
        for artifact in artifacts
    ]
    if not rows:
        rows = [{"artifact": "missing", "first_semantic_hash": "missing", "resume_semantic_hash": "missing", "match": "false"}]
    write_tsv(out, rows, ["artifact", "first_semantic_hash", "resume_semantic_hash", "match"])


def nextflow_command(args: argparse.Namespace, trace: Path, report: Path, timeline: Path, resume: bool) -> list[str]:
    for path in [trace, report, timeline]:
        if path.exists():
            path.unlink()
    if args.nextflow_jar and args.nextflow_jar.exists():
        base = [str(args.java), "-jar", str(args.nextflow_jar), "run", "workflows/phase_0_7_anti_hallucination_docking.nf"]
    else:
        base = [str(args.nextflow), "run", "workflows/phase_0_7_anti_hallucination_docking.nf"]
    if resume:
        base.append("-resume")
    base.extend(
        [
            "-profile",
            args.nextflow_profile,
            "--python",
            sys.executable,
            "--artifact_dir",
            str(args.artifact_dir),
            "--phase06_dir",
            str(args.phase06_dir),
            "--phase05_dir",
            str(args.phase05_dir),
            "--vina_bin",
            args.vina_bin,
            "--ligand_prep_command",
            args.ligand_prep_command,
            "--receptor_prep_command",
            args.receptor_prep_command,
            "--obabel_bin",
            args.obabel_bin,
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
    parser.add_argument("--artifact-dir", type=Path, default=DEFAULT_ARTIFACT_DIR)
    parser.add_argument("--phase06-dir", type=Path, default=DEFAULT_PHASE06_DIR)
    parser.add_argument("--phase05-dir", type=Path, default=DEFAULT_PHASE05_DIR)
    parser.add_argument("--vina-bin", default=env_default("GENOLIGAND_PHASE07_VINA", "vina"))
    parser.add_argument("--ligand-prep-command", default=env_default("GENOLIGAND_PHASE07_LIGAND_PREP", "mk_prepare_ligand.py"))
    parser.add_argument("--receptor-prep-command", default=env_default("GENOLIGAND_PHASE07_RECEPTOR_PREP", "mk_prepare_receptor.py"))
    parser.add_argument("--obabel-bin", default=env_default("GENOLIGAND_PHASE07_OBABEL", "obabel"))
    parser.add_argument("--nextflow", default=env_default("NEXTFLOW_BIN", "nextflow"))
    parser.add_argument("--java", type=Path, default=Path(env_default("JAVA_EXE", r"D:\user\codex\tools\java\jdk-25.0.3+9\bin\java.exe")))
    parser.add_argument("--nextflow-jar", type=Path, default=Path(env_default("NEXTFLOW_JAR", str(ROOT / "acceptance_artifacts_phase_0_2/nextflow_rebuild/nextflow-patched.jar"))))
    parser.add_argument("--nextflow-profile", default="strict")
    args = parser.parse_args()

    artifact = args.artifact_dir
    for rel in [
        "metrics",
        "outputs/ligands",
        "outputs/receptors",
        "outputs/docking_boxes",
        "outputs/docking",
        "outputs/interactions",
        "outputs/controls",
        "outputs/target_prior",
        "outputs/reports",
        "outputs/audit",
        "outputs/hashes",
        "logs/command_stdout_stderr",
    ]:
        ensure_dir(artifact / rel)

    command_rows: list[dict[str, object]] = []
    failures: list[str] = []
    py = sys.executable
    commands = [
        ("check_phase06_inputs", [py, "scripts/phase07/check_phase06_inputs.py", "--artifact-dir", str(artifact), "--phase06-dir", str(args.phase06_dir), "--phase05-dir", str(args.phase05_dir)]),
        ("validate_ligand_input", [py, "scripts/phase07/validate_ligand_input.py", "--artifact-dir", str(artifact)]),
        ("select_positive_control", [py, "scripts/phase07/select_positive_control.py", "--artifact-dir", str(artifact)]),
        ("standardize_ligands", [py, "scripts/phase07/standardize_ligands.py", "--artifact-dir", str(artifact)]),
        ("prepare_ligands_for_docking", [py, "scripts/phase07/prepare_ligands_for_docking.py", "--artifact-dir", str(artifact), "--ligand-prep-command", args.ligand_prep_command]),
        ("prepare_receptors", [py, "scripts/phase07/prepare_receptors.py", "--artifact-dir", str(artifact), "--phase06-dir", str(args.phase06_dir), "--phase05-dir", str(args.phase05_dir), "--receptor-prep-command", args.receptor_prep_command, "--obabel-bin", args.obabel_bin]),
        ("generate_docking_boxes", [py, "scripts/phase07/generate_docking_boxes.py", "--artifact-dir", str(artifact), "--phase06-dir", str(args.phase06_dir), "--phase05-dir", str(args.phase05_dir)]),
        ("run_docking", [py, "scripts/phase07/run_docking.py", "--artifact-dir", str(artifact), "--vina-bin", args.vina_bin, "--obabel-bin", args.obabel_bin]),
        ("compute_redocking_rmsd", [py, "scripts/phase07/compute_redocking_rmsd.py", "--artifact-dir", str(artifact)]),
        ("generate_interaction_fingerprints", [py, "scripts/phase07/generate_interaction_fingerprints.py", "--artifact-dir", str(artifact)]),
        ("calibrate_decoys", [py, "scripts/phase07/calibrate_decoys.py", "--artifact-dir", str(artifact)]),
        ("build_target_prior", [py, "scripts/phase07/build_target_prior.py", "--artifact-dir", str(artifact)]),
        ("build_evidence_ledger", [py, "scripts/phase07/build_evidence_ledger.py", "--artifact-dir", str(artifact)]),
        ("render_candidate_report_phase_0_7", [py, "scripts/phase07/render_candidate_report_phase_0_7.py", "--artifact-dir", str(artifact)]),
        ("build_evidence_ledger_after_report", [py, "scripts/phase07/build_evidence_ledger.py", "--artifact-dir", str(artifact)]),
        ("write_tool_versions_phase_0_7", [py, "scripts/phase07/write_tool_versions_phase_0_7.py", "--artifact-dir", str(artifact), "--vina-bin", args.vina_bin, "--ligand-prep-command", args.ligand_prep_command, "--receptor-prep-command", args.receptor_prep_command, "--obabel-bin", args.obabel_bin]),
        ("write_external_manifest_phase_0_7", [py, "scripts/phase07/write_external_manifest_phase_0_7.py", "--artifact-dir", str(artifact), "--vina-bin", args.vina_bin, "--ligand-prep-command", args.ligand_prep_command, "--receptor-prep-command", args.receptor_prep_command, "--obabel-bin", args.obabel_bin]),
        ("compute_semantic_hashes_phase_0_7", [py, "scripts/phase07/compute_semantic_hashes_phase_0_7.py", "--artifact-dir", str(artifact)]),
    ]
    for command_id, command in commands:
        code = run_command(command_id, command, ROOT, artifact, command_rows)
        if code != 0:
            failures.append(f"{command_id} failed with exit code {code}")
        write_command_summary(artifact / "metrics/command_summary.tsv", command_rows)

    semantic = artifact / "outputs/hashes/semantic_hashes.tsv"
    if semantic.exists():
        shutil.copyfile(semantic, artifact / "metrics/phase07_semantic_hash_first.tsv")
        shutil.copyfile(semantic, artifact / "outputs/hashes/semantic_hashes_first.tsv")

    nf_first = nextflow_command(
        args,
        artifact / "metrics/nextflow_trace_first.tsv",
        artifact / "metrics/nextflow_report_first.html",
        artifact / "metrics/nextflow_timeline_first.html",
        resume=False,
    )
    code = run_command("nextflow_phase_0_7_first", nf_first, ROOT, artifact, command_rows)
    if code != 0:
        failures.append(f"nextflow_phase_0_7_first failed with exit code {code}")
    if semantic.exists():
        semantic_hashes(artifact, semantic)
        shutil.copyfile(semantic, artifact / "metrics/phase07_semantic_hash_first.tsv")

    nf_resume = nextflow_command(
        args,
        artifact / "metrics/nextflow_trace_resume.tsv",
        artifact / "metrics/nextflow_report_resume.html",
        artifact / "metrics/nextflow_timeline_resume.html",
        resume=True,
    )
    code = run_command("nextflow_phase_0_7_resume", nf_resume, ROOT, artifact, command_rows)
    if code != 0:
        failures.append(f"nextflow_phase_0_7_resume failed with exit code {code}")
    if semantic.exists():
        semantic_hashes(artifact, semantic)
        shutil.copyfile(semantic, artifact / "metrics/phase07_semantic_hash_resume.tsv")
        shutil.copyfile(semantic, artifact / "outputs/hashes/semantic_hashes_resume.tsv")

    compare_semantic_hashes(
        artifact / "metrics/phase07_semantic_hash_first.tsv",
        artifact / "metrics/phase07_semantic_hash_resume.tsv",
        artifact / "metrics/phase07_semantic_hash_compare.tsv",
    )

    write_command_summary(artifact / "metrics/command_summary.tsv", command_rows)
    write_archive_hashes(artifact, artifact / "hashes.tsv")
    render_acceptance_report(artifact, artifact / "acceptance_report_phase_0_7.md", "PASS" if not failures else "FAIL", failures)

    validation_code = run_command(
        "check_phase_0_7_master_acceptance",
        [py, "scripts/validation/check_phase_0_7_master_acceptance.py", "--artifact-dir", str(artifact)],
        ROOT,
        artifact,
        command_rows,
    )
    if validation_code != 0:
        failures.append(f"check_phase_0_7_master_acceptance failed with exit code {validation_code}")
    write_command_summary(artifact / "metrics/command_summary.tsv", command_rows)
    write_archive_hashes(artifact, artifact / "hashes.tsv")
    verdict = "PASS" if not failures else "FAIL"
    (artifact / "metrics/phase_0_7_failures.txt").write_text("\n".join(failures) + ("\n" if failures else ""), encoding="utf-8")
    render_acceptance_report(artifact, artifact / "acceptance_report_phase_0_7.md", verdict, failures)
    if verdict == "PASS":
        print("phase_0_7_anti_hallucination_strict_acceptance_pass")
        return 0
    print("phase_0_7_anti_hallucination_strict_acceptance_fail")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
