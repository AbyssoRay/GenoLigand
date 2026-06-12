# GenoLigand 0.9.1 Linux Server Installation Guide

## 1. 服务器前置条件 / Server Requirements

- Linux host
- Rust toolchain with `cargo`
- Python 3.11 or newer
- Java and Nextflow
- Git
- Optional NVIDIA GPU with CUDA/OpenMM for physical mid-screen mode

GenoLigand 0.9.1 is a local single-user research console. For remote use, expose it through SSH tunneling or a trusted internal reverse proxy.

## 2. 构建和启动 / Build And Start

```bash
cd /path/to/GenoLigand
cargo build -p gls-server
python scripts/phase09/start_phase_0_9_webui.py --host 127.0.0.1 --port 19090
```

Open locally:

```text
http://127.0.0.1:19090/
```

For SSH forwarding from a workstation:

```bash
ssh -L 19090:127.0.0.1:19090 user@linux-host
```

Then open `http://127.0.0.1:19090/` on the workstation.

## 3. 数据路径 / Data Paths

Configure mounted or local paths for genome FASTA, GTF/GFF3 annotation, transcript FASTA, optional VCF, BioLiP2, UniProt, ChEMBL/BindingDB warehouse, and RCSB/AFDB structure cache.

Keep database profiles stable during a run. The evidence ledger and report export rely on exact manifest paths, versions, hashes, and tool versions.

## 4. GPU 和 OpenMM / GPU And OpenMM

For physical mid-screen mode, confirm that the Linux host sees the NVIDIA device and that OpenMM can use the intended CUDA platform. The WebUI Diagnostics page records OpenMM, CUDA, NVIDIA device, Python worker, and write-permission status.

If no GPU is available, choose a run profile that does not require GPU-backed MD.

## 5. 工作流 / Workflow

1. Create or open a project.
2. Complete the Input Wizard.
3. Select database profile and search scope.
4. Run Diagnostics and Preflight.
5. Start the selected run mode.
6. Monitor current stage, command, Nextflow run ID, logs, resources, output directory, and resume state.
7. Review Candidate Ranking, Candidate Detail, Genome/ORF Context, Structure and Patch Viewer, MD Summary, and Evidence Ledger.
8. Export reports and machine-readable artifacts.
9. Use GitHub Sync when the repository state should be committed and pushed.

## 6. Sample Project 模板 / Sample Project Template

The sample project template verifies service startup, database wiring, GPU/OpenMM discovery, Nextflow resume behavior, WebUI flow, inputs, preflight, run creation, monitoring, result browsing, export generation, and diagnostics.

It contains no precomputed candidates and no biological conclusions.
