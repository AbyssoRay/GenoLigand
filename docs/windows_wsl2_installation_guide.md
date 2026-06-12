# GenoLigand 0.9.1 Windows + WSL2 Installation Guide

## 1. Windows 前置条件 / Windows Requirements

- Windows 11
- PowerShell
- Rust toolchain with `cargo`
- Python 3.11 or newer
- Git
- A WSL2 Linux backend for scientific engines
- NVIDIA driver with WSL CUDA support when physical mid-screen mode is required

Keep your default personal WSL distribution unchanged unless you explicitly decide to use it for GenoLigand. A dedicated project backend is preferred for scientific tools and database mounts.

## 2. WSL2 科学后端 / WSL2 Scientific Backend

The Linux backend should provide Java, Nextflow, Python worker environment, OpenMM, and CUDA device visibility when GPU-backed MD is selected.

Mount or configure paths for:

- genome FASTA
- GTF/GFF3 annotation
- transcript FASTA
- optional VCF
- BioLiP2
- UniProt
- ChEMBL/BindingDB warehouse
- RCSB/AFDB structure cache

Run Diagnostics in the WebUI after any WSL path or environment change.

## 3. 从 Windows 启动 WebUI / Start From Windows

```powershell
cd D:\Project\rust\GenoLigand
python scripts\phase09\start_phase_0_9_webui.py
```

Open:

```text
http://127.0.0.1:19090/
```

To reuse an existing binary:

```powershell
python scripts\phase09\start_phase_0_9_webui.py --no-build
```

To choose another local port:

```powershell
python scripts\phase09\start_phase_0_9_webui.py --port 19190
```

## 4. 配置数据库 / Configure Databases

Open Database Registry. Select or create a profile, then review path, release/version, hash, index status, missing item count, storage use, and repair action for every required source.

Use absolute Windows paths for Windows-hosted files and mounted Linux paths for WSL-hosted files. Keep the same profile for a run so evidence ledgers can map claims to exact database versions.

## 5. 运行诊断 / Run Diagnostics

Diagnostics covers WSL2, Linux kernel, Rust binary, `gls-cli`, `gls-server`, Nextflow, Java, Python worker, OpenMM, CUDA, NVIDIA device, database paths, write permission, disk space, Git remote, GitHub push permission, and environment validation project template status.

Resolve environment warnings before launching expensive workflows.

## 6. 运行环境验证项目模板 / Run The Environment Validation Project Template

Use the environment validation project template to verify installation and workflow plumbing. It stores validation configuration only and does not include precomputed candidates or biological conclusions.

After the template run creates manifests and artifact indexes, inspect Candidate Ranking, Candidate Detail, Genome/ORF Context, Structure and Patch Viewer, MD Summary, Evidence Ledger, Report Export, Diagnostics, and GitHub Sync.

## 7. 停止服务 / Stop The Service

Press `Ctrl+C` in the service terminal. If the process was started in a background window:

```powershell
Get-Process gls-server
Stop-Process -Name gls-server
```
