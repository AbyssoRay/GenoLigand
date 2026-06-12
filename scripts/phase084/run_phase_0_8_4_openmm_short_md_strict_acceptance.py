from __future__ import annotations

import argparse
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from genoligand_md.phase084 import PHASE084_DIRNAME, compare_semantic_hashes, path_arg, render_acceptance_report, run_acceptance, validate_phase084, write_tsv

WSL_PHASE084_PYTHON = os.environ.get("GENOLIGAND_PHASE084_WSL_PYTHON", "/opt/genoligand-phase084-openff/bin/python")


def nextflow_cmd() -> list[str]:
    local = ROOT / "DUMMY"
    candidate = Path("D:/user/codex/tools/nextflow/bin/nextflow.cmd")
    if candidate.exists():
        return [str(candidate)]
    found = shutil.which("nextflow")
    return [found or "nextflow"]


def run_nextflow(artifact_dir: Path, resume: bool) -> int:
    trace = artifact_dir / "metrics" / ("nextflow_trace_resume.tsv" if resume else "nextflow_trace_first.tsv")
    report = artifact_dir / "outputs/workflow" / ("nextflow_report_resume.html" if resume else "nextflow_report_first.html")
    timeline = artifact_dir / "outputs/workflow" / ("nextflow_timeline_resume.html" if resume else "nextflow_timeline_first.html")
    log = artifact_dir / "logs/nextflow" / ("nextflow_resume.log" if resume else "nextflow_first.log")
    log.parent.mkdir(parents=True, exist_ok=True)
    def wsl_path(path: Path) -> str:
        resolved = path.resolve()
        text = str(resolved).replace("\\", "/")
        if len(text) > 2 and text[1] == ":":
            return f"/mnt/{text[0].lower()}{text[2:]}"
        return text

    workflow = wsl_path(ROOT / "workflows/phase_0_8_4_openmm_short_md_strict.nf")
    root = wsl_path(ROOT)
    nf = "/mnt/d/user/codex/tools/nextflow/lib/nextflow"
    cmd = [
        nf,
        "run",
        workflow,
    ]
    if resume:
        cmd.append("-resume")
    cmd += [
        "-profile",
        "linux_cuda",
        "--artifact_dir",
        wsl_path(artifact_dir),
        "--python",
        WSL_PHASE084_PYTHON,
        "-with-trace",
        wsl_path(trace),
        "-with-report",
        wsl_path(report),
        "-with-timeline",
        wsl_path(timeline),
    ]
    shell = "cd " + shlex.quote(root) + " && " + " ".join(shlex.quote(part) for part in cmd)
    host_cmd = ["wsl.exe", "-d", "GenoLigandPhase08", "--", "bash", "-lc", shell]
    with log.open("w", encoding="utf-8", errors="replace") as handle:
        proc = subprocess.run(host_cmd, cwd=ROOT, text=True, stdout=handle, stderr=subprocess.STDOUT, check=False, timeout=7200)
    return int(proc.returncode)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=path_arg, default=ROOT / PHASE084_DIRNAME)
    parser.add_argument("--execute-cleanup", action="store_true")
    parser.add_argument("--skip-nextflow", action="store_true")
    args = parser.parse_args()
    args.artifact_dir.mkdir(parents=True, exist_ok=True)
    run_acceptance(args.artifact_dir, execute_cleanup=args.execute_cleanup)
    nf_rows = []
    for rel in [
        "outputs/workflow/phase084_semantic_hash_first.tsv",
        "outputs/workflow/phase084_semantic_hash_resume.tsv",
        "metrics/semantic_hash_compare.tsv",
    ]:
        path = args.artifact_dir / rel
        if path.exists():
            path.unlink()
    if not args.skip_nextflow:
        first = run_nextflow(args.artifact_dir, resume=False)
        resume = run_nextflow(args.artifact_dir, resume=True)
        nf_rows.append({"run": "first", "exit_code": first, "status": "PASS" if first == 0 else "FAIL"})
        nf_rows.append({"run": "resume", "exit_code": resume, "status": "PASS" if resume == 0 else "FAIL"})
    else:
        nf_rows.append({"run": "first", "exit_code": "not_run", "status": "FAIL"})
        nf_rows.append({"run": "resume", "exit_code": "not_run", "status": "FAIL"})
    write_tsv(args.artifact_dir / "metrics/nextflow_run_summary.tsv", nf_rows, ["run", "exit_code", "status"])
    compare_semantic_hashes(args.artifact_dir)
    validate_phase084(args.artifact_dir, strict=True)
    render_acceptance_report(args.artifact_dir)
    passed, _payload = validate_phase084(args.artifact_dir, strict=True)
    render_acceptance_report(args.artifact_dir)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
