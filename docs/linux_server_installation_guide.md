# GenoLigand 0.9.1 Linux Server Installation Guide

## 1. Server Requirements

- Linux host
- Rust toolchain with `cargo`
- Python 3.11 or newer
- Java and Nextflow
- Git
- Optional NVIDIA GPU with CUDA/OpenMM for physical mid-screen mode

## 2. Build And Start

From the repository root:

```bash
cargo build -p gls-server
python scripts/phase09/start_phase_0_9_webui.py --host 127.0.0.1 --port 19090
```

For remote access, run behind an SSH tunnel or a trusted internal reverse proxy. GenoLigand 0.9.1 is designed as a local single-user research console.

## 3. Mount Data

Configure local or mounted paths for:

- genome FASTA
- GTF/GFF3 annotation
- transcript FASTA
- optional VCF
- BioLiP2
- UniProt
- ChEMBL/BindingDB warehouse
- RCSB/AFDB structure cache

## 4. Open The WebUI

Open:

```text
http://127.0.0.1:19090/
```

## 5. Workflow

Create a project, complete the Input Wizard, run Diagnostics, execute Preflight, start the selected run mode, monitor execution, inspect results, export reports, and sync code only when desired.
