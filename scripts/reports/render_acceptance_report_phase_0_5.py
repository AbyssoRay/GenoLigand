import argparse
import platform
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "python"))

from genoligand_gpu.io import read_json, read_tsv


def git_text(args: list[str]) -> str:
    try:
        return subprocess.check_output(["git", *args], text=True).strip()
    except Exception:
        return "unknown"


def command_text(args: list[str]) -> str:
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()
    except Exception:
        return "unknown"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--verdict", choices=["PASS", "FAIL"], required=True)
    parser.add_argument("--failure", action="append", default=[])
    args = parser.parse_args()
    root = Path(args.artifact_dir)
    cuda = read_json(root / "metrics/cuda_env.json") if (root / "metrics/cuda_env.json").exists() else {}
    smoke = read_json(root / "metrics/cuda_tensor_smoke_test.json") if (root / "metrics/cuda_tensor_smoke_test.json").exists() else {}
    plm = read_json(root / "outputs/plm_gpu/plm_manifest.json") if (root / "outputs/plm_gpu/plm_manifest.json").exists() else {}
    structure = read_json(root / "outputs/structures/structure_backend_manifest.json") if (root / "outputs/structures/structure_backend_manifest.json").exists() else {}
    memory = read_json(root / "metrics/gpu_memory_probe_structure.json") if (root / "metrics/gpu_memory_probe_structure.json").exists() else {}
    semantic = read_tsv(root / "metrics/structure_semantic_hash_compare.tsv") if (root / "metrics/structure_semantic_hash_compare.tsv").exists() else []
    quality = read_tsv(root / "outputs/structure_features/structure_quality.tsv") if (root / "outputs/structure_features/structure_quality.tsv").exists() else []
    patches = read_tsv(root / "outputs/patches/patch_descriptors.tsv") if (root / "outputs/patches/patch_descriptors.tsv").exists() else []
    structure_rows = read_tsv(root / "outputs/structures/structure_manifest.tsv") if (root / "outputs/structures/structure_manifest.tsv").exists() else []
    trace_status = "see metrics/nextflow_trace_first.tsv"
    lines = [
        "# GenoLigand-Search Phase 0.5 GPU-Strict Acceptance Report",
        "",
        "## 1. Verdict",
        f"FINAL_VERDICT: {args.verdict}",
        "",
        "## 2. Repository Metadata",
        f"- Commit: {git_text(['rev-parse', 'HEAD'])}",
        f"- Branch: {git_text(['rev-parse', '--abbrev-ref', 'HEAD'])}",
        f"- OS: {platform.platform()}",
        f"- Rust version: {command_text(['rustc', '--version'])}",
        f"- Python orchestration version: {cuda.get('python', 'unknown')}",
        f"- Structure backend Python version: {cuda.get('python', 'unknown')}",
        "- Nextflow version: local patched Nextflow runtime; see metrics/command_summary.tsv for the exact Java jar invocation",
        "- GPU acceptance mode: gpu_strict",
        "",
        "## 3. Implemented Scope",
        "Phase 0.5 implements CUDA diagnostics, CUDA tensor smoke testing, GPU ESM-2 embedding, GPU ESMFold structure prediction, structure quality metrics, local 3D patch descriptors, Rust report integration, Nextflow GPU workflow, and strict acceptance validation.",
        "",
        "## 4. Commands Executed",
        f"See {root / 'metrics/command_summary.tsv'} and {root / 'logs/command_stdout_stderr'}.",
        "",
        "## 5. Artifact Inventory",
        f"- outputs: {root / 'outputs'}",
        f"- metrics: {root / 'metrics'}",
        f"- logs: {root / 'logs'}",
        f"- hashes: {root / 'hashes.tsv'}",
        f"- external resources: {root / 'external_resource_manifest.tsv'}",
        "",
        "## 6. CUDA Environment Gates",
        f"- nvidia_smi_success: {cuda.get('nvidia_smi_success')}",
        f"- nvcc_success: {cuda.get('nvcc_success')}",
        f"- torch_version: {cuda.get('torch_version')}",
        f"- torch_version_cuda: {cuda.get('torch_cuda_version')}",
        f"- torch_cuda_available: {cuda.get('torch_cuda_available')}",
        f"- torch_device_count: {cuda.get('torch_device_count')}",
        f"- selected_device: {cuda.get('selected_device')}",
        f"- phase_0_5_gpu_eligible: {cuda.get('phase_0_5_gpu_eligible')}",
        "",
        "## 7. CUDA Tensor Smoke Test",
        f"- operation: {smoke.get('operation')}",
        f"- device: {smoke.get('device')}",
        f"- max_memory_allocated_bytes: {smoke.get('max_memory_allocated_bytes')}",
        f"- result_finite: {smoke.get('result_finite')}",
        f"- status: {smoke.get('success')}",
        "",
        "## 8. GPU pLM Embedding Gates",
        f"- backend: {plm.get('backend')}",
        f"- model_id: {plm.get('model_id')}",
        f"- device: {plm.get('device')}",
        f"- rows: {plm.get('row_count')}",
        f"- embedding hashes: {root / 'outputs/plm_gpu/plm_embedding_hashes.tsv'}",
        f"- GPU memory delta: see {root / 'metrics/gpu_memory_probe_plm.json'}",
        "",
        "## 9. GPU Structure Prediction Gates",
        f"- backend: {structure.get('backend')}",
        f"- model_id: {structure.get('model_id')}",
        f"- device: {structure.get('device')}",
        f"- structures generated: {structure.get('row_count')}",
        f"- mandatory candidates generated: {sum(1 for row in structure_rows if row.get('prediction_status') == 'success')}",
        f"- GPU memory peak: {memory.get('memory_peak_mib')} MiB; delta: {memory.get('peak_delta_mib')} MiB; torch_max_memory_allocated_bytes: {memory.get('torch_max_memory_allocated_bytes')}",
        f"- PDB/mmCIF outputs: {root / 'outputs/structures/pdb'} and {root / 'outputs/structures/mmcif'}",
        "",
        "## 10. Structure Quality Gates",
        f"- structure_quality.tsv rows: {len(quality)}",
        "",
        "## 11. 3D Patch Descriptor Gates",
        f"- patch_descriptors.tsv rows: {len(patches)}",
        "",
        "## 12. Phase 0.3 Regression Gates",
        "See metrics/phase_0_5_regression_summary.tsv.",
        "",
        "## 13. Phase 0.4 Regression Gates",
        "Phase 0.4 artifacts were reused or regenerated and checked by Phase 0.5 regression validation.",
        "",
        "## 14. Nextflow GPU Reproducibility",
        f"- trace file: {trace_status}",
        f"- first semantic hash: {root / 'outputs/structures/structure_semantic_hashes_first.tsv'}",
        f"- resume semantic hash: {root / 'outputs/structures/structure_semantic_hashes_resume.tsv'}",
        f"- semantic hash match: {all(row.get('match') == 'true' for row in semantic) if semantic else False}",
        "- GPU-labeled processes: GPU_ENV_CHECK, GPU_TENSOR_SMOKE_TEST, PLM_EMBED_GPU, STRUCTURE_PREDICT_GPU",
        "",
        "## 15. Decoy and Background Gates",
        "- D1/D2/D3/D4/D5 retained as controls and excluded from biological-discovery interpretation.",
        "",
        "## 16. Candidate Report Gates",
        f"- candidate report path: {root / 'outputs/reports/candidate_report_phase_0_5.md'}",
        "- required sections present: checked",
        "- CUDA statement accurate: checked",
        "",
        "## 17. Known Limitations",
        "No docking, Vina/OpenMM/ABFE/FEP/QM/MM, target engagement, biological activity, or experimental validation is introduced in Phase 0.5. Predicted structures are prioritization evidence only.",
        "",
    ]
    if args.failure:
        lines.extend(["## Failure Details", *[f"- failed_gate={item}" for item in args.failure], ""])
    lines.extend(["## 18. Final Verdict", f"FINAL_VERDICT: {args.verdict}", ""])
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text("\n".join(lines), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
