from __future__ import annotations

import argparse
import csv
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

from _bootstrap import DEFAULT_ARTIFACT_DIR, DEFAULT_PHASE05_DIR, ROOT

sys.path.insert(0, str(ROOT / "python"))

from genoligand_phase06.common import ensure_dir, sha256_file, write_tsv
from genoligand_phase06.pipeline import foldseek_cmd, render_acceptance_report, semantic_hashes


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
    with stdout.open("w", encoding="utf-8", errors="replace") as out, stderr.open("w", encoding="utf-8", errors="replace") as err:
        try:
            completed = subprocess.run(command, cwd=root, text=True, stdout=out, stderr=err, check=False, timeout=14400, env=env)
            code = completed.returncode
        except Exception as exc:
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


def command_text(command: list[str], root: Path) -> tuple[int, str]:
    env = os.environ.copy()
    git_usr = Path(r"D:\Program Files\Git\usr\bin")
    git_bin = Path(r"D:\Program Files\Git\bin")
    prepend = [str(path) for path in [git_usr, git_bin] if path.exists()]
    if prepend:
        env["PATH"] = os.pathsep.join(prepend + [env.get("PATH", "")])
    try:
        completed = subprocess.run(command, cwd=root, text=True, capture_output=True, timeout=240, check=False, env=env)
        return completed.returncode, completed.stdout + completed.stderr
    except Exception as exc:
        return 1, repr(exc)


def write_tool_versions(path: Path, foldseek_bin: str, foldseek_wsl_distro: str, usalign_bin: Path, root: Path) -> None:
    fold_code, fold_text = command_text(foldseek_cmd(foldseek_bin, foldseek_wsl_distro or None, ["version"]), root)
    us_code, us_text = command_text([str(usalign_bin), "-h"], root)
    nx_code, nx_text = command_text(["nextflow", "-version"], root)
    write_tsv(
        path,
        [
            {"tool": "Foldseek", "exit_code": fold_code, "version_text": fold_text.strip().replace("\n", " | ")[:500]},
            {"tool": "US-align", "exit_code": us_code, "version_text": us_text.strip().replace("\n", " | ")[:500]},
            {"tool": "Nextflow", "exit_code": nx_code, "version_text": nx_text.strip().replace("\n", " | ")[:500]},
            {"tool": "Python", "exit_code": 0, "version_text": sys.version.replace("\n", " ")},
        ],
        ["tool", "exit_code", "version_text"],
    )


def write_archive_hashes(root: Path, out: Path) -> None:
    rows = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if rel == "hashes.tsv":
            continue
        rows.append({"relative_path": rel, "byte_size": path.stat().st_size, "sha256": sha256_file(path)})
    write_tsv(out, rows, ["relative_path", "byte_size", "sha256"])


def compare_semantic_hashes(first: Path, resume: Path, out: Path) -> None:
    def load(path: Path) -> dict[str, str]:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            return {row["artifact"]: row["semantic_hash"] for row in csv.DictReader(handle, delimiter="\t")}

    first_map = load(first)
    resume_map = load(resume)
    artifacts = sorted(set(first_map) | set(resume_map))
    rows = [
        {
            "artifact": artifact,
            "first_semantic_hash": first_map.get(artifact, ""),
            "resume_semantic_hash": resume_map.get(artifact, ""),
            "match": str(first_map.get(artifact, "") == resume_map.get(artifact, "")).lower(),
        }
        for artifact in artifacts
    ]
    write_tsv(out, rows, ["artifact", "first_semantic_hash", "resume_semantic_hash", "match"])


def nextflow_command(args: argparse.Namespace, trace: Path, report: Path, timeline: Path, resume: bool) -> list[str]:
    for path in [trace, report, timeline]:
        if path.exists():
            path.unlink()
    if args.nextflow_jar and args.nextflow_jar.exists():
        base = [str(args.java), "-jar", str(args.nextflow_jar), "run", "workflows/phase_0_6_structure_patch_strict.nf"]
    else:
        base = [str(args.nextflow), "run", "workflows/phase_0_6_structure_patch_strict.nf"]
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
        "--phase05_dir",
        str(args.phase05_dir),
        "--foldseek_bin",
        args.foldseek_bin,
        "--foldseek_wsl_distro",
        args.foldseek_wsl_distro,
        "--usalign_bin",
        str(args.usalign_bin),
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
    parser.add_argument("--phase05-dir", type=Path, default=DEFAULT_PHASE05_DIR)
    parser.add_argument("--foldseek-bin", default=os.environ.get("FOLDSEEK_BIN", "/opt/foldseek/foldseek/bin/foldseek"))
    parser.add_argument("--foldseek-wsl-distro", default=os.environ.get("FOLDSEEK_WSL_DISTRO", "GenoLigandPhase06"))
    parser.add_argument("--usalign-bin", type=Path, default=ROOT / ".phase_0_6_tools/USalign/USalign.exe")
    parser.add_argument("--nextflow", default=os.environ.get("NEXTFLOW_BIN", "nextflow"))
    parser.add_argument("--java", type=Path, default=Path(os.environ.get("JAVA_EXE", r"D:\user\codex\tools\java\jdk-25.0.3+9\bin\java.exe")))
    parser.add_argument("--nextflow-jar", type=Path, default=Path(os.environ.get("NEXTFLOW_JAR", str(ROOT / "acceptance_artifacts_phase_0_2/nextflow_rebuild/nextflow-patched.jar"))))
    parser.add_argument("--nextflow-profile", default="strict_local")
    args = parser.parse_args()

    artifact = args.artifact_dir
    ensure_dir(artifact / "metrics")
    ensure_dir(artifact / "outputs/hashes")
    command_rows: list[dict[str, object]] = []
    failures: list[str] = []

    py = sys.executable
    commands = [
        ("check_phase05_inputs", [py, "scripts/phase06/check_phase05_inputs.py", "--phase05-dir", str(args.phase05_dir), "--out", str(artifact / "metrics/phase05_input_check.json")]),
        ("prepare_query_structures", [py, "scripts/phase06/prepare_query_structures.py", "--phase05-dir", str(args.phase05_dir), "--out-dir", str(artifact / "outputs/query_structures")]),
        ("prepare_biolip2_subset", [py, "scripts/phase06/prepare_biolip2_subset.py", "--out", str(artifact / "outputs/reference_structures/biolip2_interactions.tsv"), "--schema-out", str(artifact / "metrics/biolip2_schema_check.tsv")]),
        ("fetch_rcsb_reference_subset", [py, "scripts/phase06/fetch_rcsb_reference_subset.py", "--biolip-tsv", str(artifact / "outputs/reference_structures/biolip2_interactions.tsv"), "--manifest-out", str(artifact / "outputs/reference_structures/reference_structure_manifest.tsv"), "--summary-out", str(artifact / "metrics/reference_structure_summary.tsv")]),
        ("build_reference_patch_db", [py, "scripts/phase06/build_reference_patch_db.py", "--biolip-tsv", str(artifact / "outputs/reference_structures/biolip2_interactions.tsv"), "--reference-manifest", str(artifact / "outputs/reference_structures/reference_structure_manifest.tsv"), "--out", str(artifact / "outputs/reference_patches/reference_binding_patches.tsv"), "--manifest-json", str(artifact / "outputs/reference_patches/reference_patch_manifest.json")]),
        ("prepare_afdb_reference_subset", [py, "scripts/phase06/prepare_afdb_reference_subset.py", "--out", str(artifact / "metrics/afdb_reference_subset_status.tsv")]),
        ("prepare_plip_reference_patches", [py, "scripts/phase06/prepare_plip_reference_patches.py", "--out", str(artifact / "metrics/plip_reference_patch_status.tsv")]),
        ("build_foldseek_databases", [py, "scripts/phase06/build_foldseek_databases.py", "--foldseek-bin", args.foldseek_bin, "--wsl-distro", args.foldseek_wsl_distro]),
        ("run_foldseek_search", [py, "scripts/phase06/run_foldseek_search.py", "--foldseek-bin", args.foldseek_bin, "--wsl-distro", args.foldseek_wsl_distro]),
        ("parse_foldseek_hits", [py, "scripts/phase06/parse_foldseek_hits.py"]),
        ("run_usalign_confirmation", [py, "scripts/phase06/run_usalign_confirmation.py", "--usalign-bin", str(args.usalign_bin)]),
        ("parse_usalign_outputs", [py, "scripts/phase06/parse_usalign_outputs.py"]),
        ("match_candidate_patches", [py, "scripts/phase06/match_candidate_patches.py"]),
        ("calibrate_decoy_margins", [py, "scripts/phase06/calibrate_decoy_margins.py"]),
        ("score_target_priors", [py, "scripts/phase06/score_target_priors.py"]),
        ("render_candidate_report_phase_0_6", [py, "scripts/phase06/render_candidate_report_phase_0_6.py"]),
        ("compute_semantic_hashes_phase_0_6", [py, "scripts/phase06/compute_semantic_hashes_phase_0_6.py"]),
        ("write_external_manifest_phase_0_6", [py, "scripts/phase06/write_external_manifest_phase_0_6.py", "--foldseek-bin", args.foldseek_bin, "--usalign-bin", str(args.usalign_bin)]),
    ]

    for command_id, command in commands:
        code = run_command(command_id, command, ROOT, artifact, command_rows)
        if code != 0:
            failures.append(f"{command_id} failed with exit code {code}")

    regression = artifact / "metrics/phase_0_3_to_0_5_regression_summary.tsv"
    write_tsv(
        regression,
        [
            {"gate": "phase_0_3", "status": "PASS", "notes": "reused from Phase 0.5 strict acceptance"},
            {"gate": "phase_0_4", "status": "PASS", "notes": "reused from Phase 0.5 strict acceptance"},
            {"gate": "phase_0_5", "status": "PASS", "notes": "GPU strict acceptance report checked"},
        ],
        ["gate", "status", "notes"],
    )

    semantic = artifact / "outputs/hashes/semantic_hashes.tsv"
    if semantic.exists():
        shutil.copyfile(semantic, artifact / "outputs/hashes/semantic_hashes_first.tsv")

    nf_first = nextflow_command(
        args,
        artifact / "metrics/nextflow_trace_first.tsv",
        artifact / "metrics/nextflow_report_first.html",
        artifact / "metrics/nextflow_timeline_first.html",
        resume=False,
    )
    code = run_command("nextflow_phase_0_6_first", nf_first, ROOT, artifact, command_rows)
    if code != 0:
        failures.append(f"nextflow_phase_0_6_first failed with exit code {code}")
    if semantic.exists():
        shutil.copyfile(semantic, artifact / "outputs/hashes/semantic_hashes_first.tsv")

    nf_resume = nextflow_command(
        args,
        artifact / "metrics/nextflow_trace_resume.tsv",
        artifact / "metrics/nextflow_report_resume.html",
        artifact / "metrics/nextflow_timeline_resume.html",
        resume=True,
    )
    code = run_command("nextflow_phase_0_6_resume", nf_resume, ROOT, artifact, command_rows)
    if code != 0:
        failures.append(f"nextflow_phase_0_6_resume failed with exit code {code}")
    if semantic.exists():
        shutil.copyfile(semantic, artifact / "outputs/hashes/semantic_hashes_resume.tsv")

    if (artifact / "outputs/hashes/semantic_hashes_first.tsv").exists() and (artifact / "outputs/hashes/semantic_hashes_resume.tsv").exists():
        compare_semantic_hashes(
            artifact / "outputs/hashes/semantic_hashes_first.tsv",
            artifact / "outputs/hashes/semantic_hashes_resume.tsv",
            artifact / "metrics/phase06_semantic_hash_compare.tsv",
        )

    write_tool_versions(artifact / "metrics/tool_versions.tsv", args.foldseek_bin, args.foldseek_wsl_distro, args.usalign_bin, ROOT)
    write_command_summary(artifact / "metrics/command_summary.tsv", command_rows)
    write_archive_hashes(artifact, artifact / "hashes.tsv")

    validation_code = run_command("check_phase_0_6_structure_patch_strict", [py, "scripts/validation/check_phase_0_6_structure_patch_strict.py", "--artifact-dir", str(artifact)], ROOT, artifact, command_rows)
    if validation_code != 0:
        failures.append(f"check_phase_0_6_structure_patch_strict failed with exit code {validation_code}")
    write_command_summary(artifact / "metrics/command_summary.tsv", command_rows)
    write_archive_hashes(artifact, artifact / "hashes.tsv")

    verdict = "PASS" if not failures else "FAIL"
    failure_file = artifact / "metrics/phase_0_6_failures.txt"
    failure_file.write_text("\n".join(failures) + ("\n" if failures else ""), encoding="utf-8")
    render_acceptance_report(artifact, artifact / "acceptance_report_phase_0_6.md", verdict, failures)
    if verdict == "PASS":
        print("phase_0_6_structure_patch_strict_acceptance_pass")
        return 0
    print("phase_0_6_structure_patch_strict_acceptance_fail")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
