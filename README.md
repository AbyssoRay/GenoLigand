# GenoLigand Local WebUI

GenoLigand is a local, single-user scientific workflow app for ligand-aware target discovery research. The current user-facing entry point is the Phase 0.9 WebUI beta. It runs on your own machine, serves a browser interface from `gls-server`, and keeps the scientific chain behind Rust-owned `gls-cli` commands.

GenoLigand is a method-validation and prioritization tool. It does not claim experimental binding, biological activity, mechanism, or clinical relevance.

## What You Can Do

- Create or open a local GenoLigand project.
- Enter small-molecule, peptide, protein-ligand, or existing-complex inputs.
- Select organism/genome, database profile, search scope, and run profile.
- Run preflight checks before starting a job.
- Monitor a run with queue state, timeline, logs, and artifact status.
- Review ranked candidates, structure/patch summaries, MD summaries, and evidence rows.
- Export reports and inspect validation artifacts.
- Push validated project state to GitHub when you explicitly choose to do so.

## Requirements

Install these before running the local WebUI:

- Windows 11 with PowerShell, or Linux.
- Rust toolchain with `cargo` in PATH.
- Python 3.11 or newer in PATH.
- For strict scientific validation on this machine: WSL2 project backend `GenoLigandPhase08` with CUDA/OpenMM evidence.

This repository already contains the Phase 0.9 acceptance artifact used by the local WebUI:

```text
acceptance_artifacts_phase_0_9_webui_local_beta_strict/
```

## Start The WebUI

From the repository root:

```powershell
cd D:\Project\rust\GenoLigand
python scripts\phase09\start_phase_0_9_webui.py
```

The script does four things:

1. Builds `gls-server`.
2. Starts the local server on `127.0.0.1:19090`.
3. Checks `/api/health`.
4. Opens your default browser.

If the browser does not open automatically, open:

```text
http://127.0.0.1:19090/
```

Keep the terminal open while using the WebUI. Press `Ctrl+C` in that terminal to stop the local server.

## Start Without Rebuilding

If `target\debug\gls-server.exe` already exists:

```powershell
python scripts\phase09\start_phase_0_9_webui.py --no-build
```

Use a different port if `19090` is busy:

```powershell
python scripts\phase09\start_phase_0_9_webui.py --port 19190
```

Then open:

```text
http://127.0.0.1:19190/
```

## WebUI Walkthrough

### 1. Dashboard

Open the Dashboard first. Confirm these status blocks are visible:

- `gls-server`
- `Rust ownership`
- `OpenMM CUDA`
- `Phase 0.9 regression`

The Dashboard also shows Active Run, Task Queue, Agent / Run Timeline, Runtime Logs, Run Details, API Contract, and Artifact Tree.

### 2. Project

Open **Project** to create a workspace. Fill or review:

- Project name
- Species
- Remote
- Claim boundary

The default claim boundary is method-validation-only.

### 3. Input Wizard

Open **Input Wizard** and go through each step:

- Project Basics
- Molecule Type
- Small Molecule
- Peptide
- Protein Ligand
- Existing Complex
- Species and Genome
- Search Scope
- Database Profile
- Run Profile
- Preflight Check
- Review and Start

Advanced scientific parameters should stay explicit and visible through preflight, not hidden in final reports.

### 4. Run Monitor

Open **Run Monitor** to check:

- Queue state
- Stage owner
- Logs
- Resume state
- Artifact path

The scientific workflow must enter through `gls-cli`; the WebUI must not call Python scientific workers directly.

### 5. Candidates And Evidence

Use these pages to inspect outputs:

- **Candidates** for ranked hypotheses.
- **Candidate Detail** for score decomposition.
- **Genome and ORF Context** for genomic evidence.
- **Structure and Patch Viewer** for structure/patch evidence.
- **Short-MD Summary** for MD stability summaries.
- **Evidence Ledger** for claim-to-evidence mapping.

Treat all candidates as prioritization hypotheses until independently validated.

### 6. Databases And Diagnostics

Use **Databases** to inspect protected database and warehouse manifests.

Use **Diagnostics** to check:

- WSL2 backend
- CUDA/OpenMM
- Rust ownership
- Nextflow
- Database profile
- GitHub remote

### 7. Export And GitHub Sync

Open **Report Export** to locate generated reports and machine-readable validation files.

Open **GitHub Sync** only when you intend to push the current repository state. Remote push should remain an explicit user action.

## Verify The Local Server

Health endpoint:

```powershell
(Invoke-WebRequest -Uri http://127.0.0.1:19090/api/health -UseBasicParsing).Content
```

Expected response:

```json
{"status":"PASS","service":"gls-server","acceptance_mode":"phase_0_9_webui_local_beta_strict"}
```

Master validation file:

```powershell
Get-Content acceptance_artifacts_phase_0_9_webui_local_beta_strict\metrics\master_validation_phase_0_9.json -First 8
```

Expected values:

```text
final_verdict: PASS
master_validation_p0_all_pass: true
failed_gate_count: 0
```

## Run Full Phase 0.9 Acceptance

Use this when you need to regenerate screenshots, API checks, WebSocket checks, Nextflow first/resume evidence, and the acceptance report:

```powershell
python scripts\phase09\run_phase_0_9_webui_local_beta_strict_acceptance.py
```

Important outputs:

```text
acceptance_artifacts_phase_0_9_webui_local_beta_strict\acceptance_report_phase_0_9.md
acceptance_artifacts_phase_0_9_webui_local_beta_strict\metrics\master_validation_phase_0_9.json
acceptance_artifacts_phase_0_9_webui_local_beta_strict\ui\screenshots\final\
acceptance_artifacts_phase_0_9_webui_local_beta_strict\server\api_contract_validation.tsv
acceptance_artifacts_phase_0_9_webui_local_beta_strict\server\websocket_validation.tsv
```

## Developer Checks

Before committing changes:

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
python -m compileall scripts\phase09 scripts\validation\check_phase_0_9_master.py
```

If Windows denies access to Cargo or Git lock files, rerun the same command from an elevated shell. Do not treat that permission failure as a code regression until it reproduces with adequate permissions.

## Stop The WebUI

If you started with the Python launcher, press `Ctrl+C`.

If a server is still running, find and stop it:

```powershell
Get-Process gls-server
Stop-Process -Name gls-server
```

## Clean Local Runtime Files

These files are local runtime state and can be removed when the server is stopped:

```powershell
Remove-Item -Recurse -Force outputs\phase_0_9 -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force .nextflow,work -ErrorAction SilentlyContinue
Remove-Item -Force .nextflow.log* -ErrorAction SilentlyContinue
```

Do not delete:

```text
acceptance_artifacts_phase_0_9_webui_local_beta_strict\
configs\
crates\
scripts\
webui\
workflows\
```

These are source, configuration, or required validation artifacts.

## Troubleshooting

### Port 19090 Is Busy

Start on a different port:

```powershell
python scripts\phase09\start_phase_0_9_webui.py --port 19190
```

### Browser Did Not Open

Open the URL manually:

```text
http://127.0.0.1:19090/
```

### Health Check Fails

Check whether the server process exists:

```powershell
Get-Process gls-server
```

Then inspect runtime logs:

```powershell
Get-Content outputs\phase_0_9\runtime\gls-server.stderr.log -Tail 80
```

### Nextflow Fails On Windows With `Unknown signal: HUP`

Run Nextflow inside the project WSL2 backend. The Phase 0.9 acceptance runner already does this by using `GenoLigandPhase08` and the D-drive Nextflow distribution.

### UI Looks Broken Or Text Overlaps

Regenerate the Phase 0.9 screenshots and visual audit:

```powershell
python scripts\phase09\run_phase_0_9_webui_local_beta_strict_acceptance.py --skip-nextflow
```

Then inspect:

```text
acceptance_artifacts_phase_0_9_webui_local_beta_strict\ui\screenshots\final\
acceptance_artifacts_phase_0_9_webui_local_beta_strict\ui\visual_audit\final_visual_audit_report.md
```

## License

GenoLigand is distributed under the project EULA in `EULA.txt`.
