# GenoLigand 0.9.1 Windows + WSL2 Installation Guide

## 1. Windows Requirements

- Windows 11
- PowerShell
- Rust toolchain with `cargo`
- Python 3.11 or newer
- Git
- A WSL2 Linux backend for scientific engines

## 2. WSL2 Runtime

Use the project WSL2 backend configured for GenoLigand scientific tools. Keep the default personal WSL distribution unchanged unless you explicitly decide to use it for this project.

The Linux backend should provide:

- Java and Nextflow
- Python worker environment
- OpenMM
- CUDA and NVIDIA device access when physical mid-screen mode is selected
- Database mounts or local paths for genome, annotation, transcript, BioLiP2, UniProt, ChEMBL/BindingDB, and structure cache

## 3. Start The WebUI From Windows

```powershell
cd D:\Project\rust\GenoLigand
python scripts\phase09\start_phase_0_9_webui.py
```

Open:

```text
http://127.0.0.1:19090/
```

## 4. Configure Databases

Open Databases in the WebUI and select or create a database profile. Check path, version, hash, index state, missing items, storage use, and repair action.

## 5. Run Diagnostics

Open Diagnostics and check WSL2, Rust binary, gls-cli, gls-server, Nextflow, Java, Python worker, OpenMM, CUDA, NVIDIA device, database paths, write permission, disk space, Git remote, and GitHub permission.

## 6. Run The Sample Project Template

Use the sample project template only to verify installation and workflow plumbing. It does not contain precomputed candidates or biological conclusions.
