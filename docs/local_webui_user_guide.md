# GenoLigand 0.9.1 Local WebUI User Guide

## 1. Start The Local Service

From the repository root:

```powershell
python scripts\phase09\start_phase_0_9_webui.py
```

Open:

```text
http://127.0.0.1:19090/
```

The browser connects to the local `gls-server`. Scientific workflow execution is submitted through `gls-cli`.

## 2. Create Or Open A Project

Use the Project page to create, open, copy, delete, clean, or resume a project. A project stores input manifests, database profile, run state, artifact index, and report paths.

## 3. Configure Inputs

Use the Input Wizard:

1. Project Basics
2. Molecule Type
3. Small Molecule, Peptide, Protein Ligand, or Existing Complex
4. Species and Genome
5. Search Scope
6. Database Profile
7. Run Profile
8. Preflight
9. Review and Start

Advanced scientific parameters stay folded by default for ordinary use.

## 4. Run And Monitor

Submit the workflow after preflight. The Run Monitor shows current stage, command, Nextflow run ID, log tail, resource usage, output directory, failure reason, and resume state.

Supported run states are `created`, `preflight_passed`, `queued`, `running`, `failed`, `completed`, `cancelled`, `resumable`, and `pushed_to_github`.

## 5. View Results

Computed outputs appear in Candidates, Candidate Detail, Genome/ORF Context, Structure and Patch Viewer, MD Summary, and Evidence Ledger. Empty pages do not contain invented candidate rows.

## 6. Export Reports

Use Report Export to generate HTML, Markdown, CSV/TSV, JSON, artifact bundle, run manifest, tool version manifest, diagnostics report, and GitHub push report.

## 7. Diagnostics And GitHub Sync

Run Diagnostics before expensive workflows. Use GitHub Sync only when you intend to record and push the current project state.
