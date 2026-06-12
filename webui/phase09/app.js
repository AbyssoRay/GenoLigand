const screenshotPages = [
  "dashboard_page",
  "project_create_page",
  "input_wizard_project_basics_page",
  "input_wizard_molecule_type_page",
  "input_wizard_small_molecule_page",
  "input_wizard_peptide_page",
  "input_wizard_protein_page",
  "input_wizard_complex_page",
  "input_wizard_genome_page",
  "input_wizard_search_scope_page",
  "input_wizard_database_profile_page",
  "input_wizard_run_profile_page",
  "input_wizard_preflight_page",
  "input_wizard_review_start_page",
  "run_monitor_page",
  "candidate_ranking_page",
  "candidate_detail_page",
  "genome_orf_context_page",
  "structure_viewer_page",
  "md_summary_page",
  "evidence_ledger_page",
  "database_registry_page",
  "diagnostics_page",
  "settings_page",
  "github_sync_page",
  "report_export_page",
  "error_state_page",
  "empty_state_page",
  "loading_state_page",
];

const navItems = [
  ["dashboard_page", "Dashboard", "Run overview"],
  ["project_create_page", "Project", "Create workspace"],
  ["input_wizard_project_basics_page", "Input Wizard", "Guided setup"],
  ["run_monitor_page", "Run Monitor", "Queue and logs"],
  ["candidate_ranking_page", "Candidates", "Ranked evidence"],
  ["database_registry_page", "Databases", "Profiles"],
  ["diagnostics_page", "Diagnostics", "Local checks"],
  ["github_sync_page", "GitHub Sync", "Push and cleanup"],
  ["settings_page", "Settings", "Local beta"],
];

const wizardPages = [
  "input_wizard_project_basics_page",
  "input_wizard_molecule_type_page",
  "input_wizard_small_molecule_page",
  "input_wizard_peptide_page",
  "input_wizard_protein_page",
  "input_wizard_complex_page",
  "input_wizard_genome_page",
  "input_wizard_search_scope_page",
  "input_wizard_database_profile_page",
  "input_wizard_run_profile_page",
  "input_wizard_preflight_page",
  "input_wizard_review_start_page",
];

const pageTitles = Object.fromEntries([
  ["dashboard_page", "Command Center"],
  ["project_create_page", "Create Project"],
  ["input_wizard_project_basics_page", "Project Basics"],
  ["input_wizard_molecule_type_page", "Molecule Type"],
  ["input_wizard_small_molecule_page", "Small Molecule Input"],
  ["input_wizard_peptide_page", "Peptide Input"],
  ["input_wizard_protein_page", "Protein Ligand Input"],
  ["input_wizard_complex_page", "Existing Complex Input"],
  ["input_wizard_genome_page", "Species and Genome"],
  ["input_wizard_search_scope_page", "Search Scope"],
  ["input_wizard_database_profile_page", "Database Profile"],
  ["input_wizard_run_profile_page", "Run Profile"],
  ["input_wizard_preflight_page", "Preflight Check"],
  ["input_wizard_review_start_page", "Review and Start"],
  ["run_monitor_page", "Run Monitor"],
  ["candidate_ranking_page", "Candidate Ranking"],
  ["candidate_detail_page", "Candidate Detail"],
  ["genome_orf_context_page", "Genome and ORF Context"],
  ["structure_viewer_page", "Structure and Patch Viewer"],
  ["md_summary_page", "Short-MD Summary"],
  ["evidence_ledger_page", "Evidence Ledger"],
  ["database_registry_page", "Database Registry"],
  ["diagnostics_page", "Diagnostics"],
  ["settings_page", "Settings"],
  ["github_sync_page", "GitHub Sync"],
  ["report_export_page", "Report Export"],
  ["error_state_page", "Error State"],
  ["empty_state_page", "Empty State"],
  ["loading_state_page", "Loading State"],
]);

const apiContracts = [
  "GET /api/health",
  "GET /api/projects",
  "POST /api/projects",
  "POST /api/projects/{id}/preflight",
  "POST /api/projects/{id}/runs",
  "GET /api/runs/{id}/logs",
  "GET /api/runs/{id}/candidates",
  "GET /api/databases",
  "POST /api/github/push",
];

function routeFromLocation() {
  const raw = window.location.hash.replace(/^#\/?/, "");
  return screenshotPages.includes(raw) ? raw : "dashboard_page";
}

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("phase09-theme", theme);
}

function initTheme() {
  const paramTheme = new URLSearchParams(window.location.search).get("theme");
  const saved = localStorage.getItem("phase09-theme");
  setTheme(paramTheme || saved || "dark");
}

function statusPill(text, kind = "pass") {
  return `<span class="status ${kind}">${text}</span>`;
}

function card(title, body, footer = "") {
  return `<section class="panel"><h2>${title}</h2><div>${body}</div>${footer}</section>`;
}

function metricGrid(items) {
  return `<div class="metric-grid">${items
    .map(([label, value, status]) => `<div class="metric"><span>${label}</span><strong>${value}</strong>${status ? statusPill(status) : ""}</div>`)
    .join("")}</div>`;
}

function dataTable(headers, rows) {
  return `<table class="data-table"><thead><tr>${headers.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows
    .map((row) => `<tr>${row.map((cell) => `<td>${cell}</td>`).join("")}</tr>`)
    .join("")}</tbody></table>`;
}

function wizardRail(activeId) {
  return `<div class="wizard-rail">${wizardPages
    .map((id, index) => `<a class="${id === activeId ? "active" : ""}" href="#/${id}"><span>${index + 1}</span>${pageTitles[id]}</a>`)
    .join("")}</div>`;
}

function formGrid(rows) {
  return `<div class="form-grid">${rows
    .map(([label, value, hint]) => `<label><span>${label}</span><input value="${value}" aria-label="${label}"><small>${hint}</small></label>`)
    .join("")}</div>`;
}

function optionTiles(items) {
  return `<div class="option-tiles">${items
    .map(([title, text, active]) => `<button class="option-tile ${active ? "selected" : ""}"><strong>${title}</strong><span>${text}</span></button>`)
    .join("")}</div>`;
}

function dashboard() {
  return [
    card(
      "Local Beta Runtime",
      metricGrid([
        ["gls-server", "127.0.0.1", "PASS"],
        ["Rust ownership", "gls-server -> gls-cli", "PASS"],
        ["OpenMM CUDA", "WSL2 evidence retained", "PASS"],
        ["Phase 0.8.5 regression", "Compact baseline", "PASS"],
      ])
    ),
    card(
      "Active Run",
      dataTable(["Stage", "Owner", "Status", "Evidence"], [
        ["Input normalization", "Rust", statusPill("PASS"), "schemas/input_wizard_schema.json"],
        ["Candidate selection", "Rust", statusPill("PASS"), "candidate_manifest.tsv"],
        ["Short-MD primitive", "Python worker", statusPill("PASS"), "schema-bound result"],
        ["Report semantics", "Rust", statusPill("PASS"), "master_validation_phase_0_9.json"],
      ])
    ),
  ].join("");
}

function projectCreate() {
  return card(
    "New Project",
    formGrid([
      ["Project name", "Local beta demo", "Stored under a local single-user workspace"],
      ["Species", "Homo sapiens", "Controlled vocabulary plus custom FASTA/GFF option"],
      ["Remote", "origin/main", "Used only after explicit GitHub sync"],
      ["Claim boundary", "method-validation-only", "No biological discovery claim is implied"],
    ]) + `<div class="button-row"><button class="primary">Create Project</button><button>Import Existing</button></div>`
  );
}

function wizardPage(id) {
  const content = {
    input_wizard_project_basics_page: formGrid([
      ["Project name", "Kinase pocket triage", "Human-readable run name"],
      ["Workspace label", "phase09-local-beta", "Local project id is generated by gls-server"],
      ["Research purpose", "benchmark triage", "Displayed in reports as context only"],
    ]),
    input_wizard_molecule_type_page: optionTiles([
      ["Small molecule", "SMILES, SDF, or MOL2 with RDKit/OpenFF checks", true],
      ["Peptide", "FASTA or one-letter sequence with terminal options", false],
      ["Protein ligand", "FASTA or PDB/mmCIF chain input", false],
      ["Existing complex", "PDB/mmCIF complex with chain/ligand mapping", false],
    ]),
    input_wizard_small_molecule_page: formGrid([
      ["Canonical SMILES", "CC(=O)Nc1ccc(O)cc1", "Canonicalized by Rust-owned schema path"],
      ["Input file", "ligand.sdf", "Optional local upload"],
      ["Charge policy", "OpenFF default", "Adapter execution is schema-bound"],
    ]),
    input_wizard_peptide_page: formGrid([
      ["Peptide sequence", "ACDEFGHIK", "Length and residue validity checked"],
      ["Cyclization", "none", "Explicitly recorded"],
      ["Termini", "NH2 / COOH", "Report-visible setting"],
    ]),
    input_wizard_protein_page: formGrid([
      ["Protein FASTA", "protein_ligand.fasta", "Sequence hash is recorded"],
      ["Structure source", "AlphaFold DB", "Optional user PDB/mmCIF override"],
      ["Chain policy", "single-chain", "Complex targets preserve components"],
    ]),
    input_wizard_complex_page: formGrid([
      ["Complex file", "complex.cif.gz", "Resolver supports gzipped mmCIF/PDB"],
      ["Ligand selector", "auth_asym_id A / ligand ATP", "Auth-label mapping visible"],
      ["Patch radius", "6.0 A", "Versioned metric definition"],
    ]),
    input_wizard_genome_page: formGrid([
      ["Organism", "Homo sapiens", "Preset or custom species"],
      ["Genome FASTA", "GRCh38.primary.fa", "Local path hash required"],
      ["Annotation GFF", "GRCh38.annotation.gff3", "Optional ORF context"],
    ]),
    input_wizard_search_scope_page: optionTiles([
      ["Known proteins", "UniProt reviewed targets", true],
      ["Genome ORFs", "Translate and score ORF candidates", true],
      ["Structure neighborhoods", "RCSB/BioLiP2/PLINDER patches", true],
      ["External comparator", "PDBbind optional, never PASS mandatory", false],
    ]),
    input_wizard_database_profile_page: optionTiles([
      ["Phase 0.8.3 manifest", "Protected database manifest", true],
      ["Warehouse manifest", "DuckDB/Parquet profile", true],
      ["Open benchmark stack", "PLINDER, MOAD, OpenBindBench, DUDE-Z", true],
      ["Custom indexes", "User-built after validation", false],
    ]),
    input_wizard_run_profile_page: optionTiles([
      ["Fast triage", "CPU-first scoring plus compact MD smoke", true],
      ["Balanced", "Structure and short-MD evidence enabled", false],
      ["Strict reproducibility", "Nextflow first/resume hash gate", false],
    ]),
    input_wizard_preflight_page: dataTable(["Check", "Value", "Status"], [
      ["Input schema", "valid", statusPill("PASS")],
      ["Database profile", "protected manifests present", statusPill("PASS")],
      ["CUDA OpenMM", "WSL2 evidence", statusPill("PASS")],
      ["Git state", "clean before run", statusPill("PASS")],
    ]),
    input_wizard_review_start_page: dataTable(["Field", "Selected value", "Owner"], [
      ["Molecule", "Small molecule SMILES", "Rust schema"],
      ["Genome", "GRCh38", "Rust project model"],
      ["Databases", "Phase 0.8.3 warehouse", "manifest"],
      ["Run profile", "Fast triage", "gls-cli"],
    ]) + `<div class="button-row"><button class="primary">Start Run</button><button>Save Draft</button></div>`,
  }[id];
  return `<div class="wizard-layout">${wizardRail(id)}<div class="wizard-card">${content}</div></div>`;
}

function runMonitor() {
  return [
    card("Task Queue", dataTable(["Task", "State", "Runtime"], [
      ["md select", statusPill("PASS"), "00:02"],
      ["md prepare", statusPill("PASS"), "00:04"],
      ["md run", statusPill("PASS"), "00:11"],
      ["md analyze", statusPill("PASS"), "00:03"],
      ["report", statusPill("PASS"), "00:01"],
    ])),
    card("Live Logs", `<pre class="terminal">[INFO] gls-cli md select --artifact-dir acceptance_artifacts_phase_0_9_webui_local_beta_strict
[INFO] Nextflow process call audit: gls-cli rate 100%
[INFO] Rust validated worker_result.phase_0_9 schema
[INFO] FINAL_VERDICT source: master_validation_phase_0_9.json</pre>`),
  ].join("");
}

function candidateRanking() {
  return card(
    "Ranked Candidates",
    dataTable(["Rank", "Candidate", "Target", "Score", "Evidence"], [
      ["1", "cand-001", "DEMO1", "0.79", "fragment, structure, MD"],
      ["2", "cand-002", "DEMO2", "0.74", "sequence, patch"],
      ["3", "cand-003", "DEMO3", "0.69", "benchmark overlap"],
    ])
  );
}

function candidateDetail() {
  return [
    card("cand-001 Detail", metricGrid([
      ["Fragment", "0.82", "PASS"],
      ["Structure patch", "0.76", "PASS"],
      ["Short-MD stability", "0.71", "PASS"],
      ["Final priority", "0.79", "PASS"],
    ])),
    card("Claim Boundary", `<p>This is a candidate target hypothesis for local method validation. It is not experimental validation.</p>`),
  ].join("");
}

function genomeContext() {
  return card("ORF Context", dataTable(["ORF", "Coordinates", "Annotation", "Evidence"], [
    ["orf-demo-001", "chr7:55019016-55211628", "kinase-like domain", "UniProt reviewed"],
    ["orf-demo-002", "chr12:25357723-25403865", "signaling protein", "structure neighborhood"],
  ]));
}

function structureViewer() {
  return card(
    "Structure and Patch Viewer",
    `<div class="structure-stage"><div class="pocket-cloud"></div><div class="ligand-stick"></div><div class="patch-ring"></div><span>A:ATP auth-label mapped</span></div>
    ${dataTable(["Patch", "Residues", "Descriptor hash", "Status"], [
      ["patch-001", "18", "sha256:8b14...", statusPill("PASS")],
      ["patch-002", "14", "sha256:59ac...", statusPill("PASS")],
    ])}`
  );
}

function mdSummary() {
  return card("Short-MD Summary", metricGrid([
    ["RMSD median", "1.42 A", "PASS"],
    ["Interaction occupancy", "68%", "PASS"],
    ["CUDA platform", "OpenMM CUDA", "PASS"],
    ["Trajectory features", "schema-bound", "PASS"],
  ]));
}

function evidenceLedger() {
  return card("Evidence Ledger", dataTable(["Row", "Claim", "Evidence id", "Status"], [
    ["1", "Rust owns final score", "architecture/md_architecture_ownership_audit.tsv", statusPill("PASS")],
    ["2", "WebUI route coverage complete", "metrics/webui_route_coverage.tsv", statusPill("PASS")],
    ["3", "No raw tool report dependency", "architecture/raw_tool_report_dependency_audit.tsv", statusPill("PASS")],
    ["4", "PNG visual audit passed", "metrics/ui_visual_audit_summary.tsv", statusPill("PASS")],
  ]));
}

function databaseRegistry() {
  return card("Database Profiles", dataTable(["Source", "Manifest", "State", "Use"], [
    ["Phase 0.8.3 database manifest", "protected", statusPill("PASS"), "structure and mapping"],
    ["Warehouse manifest", "protected", statusPill("PASS"), "benchmark queries"],
    ["OpenMM environment descriptor", "protected", statusPill("PASS"), "short-MD runtime"],
    ["PDBbind", "optional external comparator", statusPill("INFO", "info"), "not PASS mandatory"],
  ]));
}

function diagnostics() {
  return card("Diagnostics", dataTable(["Check", "Observed value", "Repair action"], [
    ["WSL2 backend", "GenoLigandPhase08", "Open WSL guide"],
    ["OpenMM CUDA", "PASS", "Validate environment"],
    ["Rust ownership", "PASS", "Open architecture audit"],
    ["GitHub remote", "origin/main", "Open GitHub Sync"],
  ]));
}

function settings() {
  return card("Settings", formGrid([
    ["Theme", document.documentElement.dataset.theme, "Dark and light modes are screenshot-tested"],
    ["Server", "gls-server", "Local single-user beta"],
    ["API contract", "OpenAPI JSON", "Generated during acceptance"],
    ["Report export", "Markdown, HTML, JSON", "Rust-owned semantics"],
  ]));
}

function githubSync() {
  return card("GitHub Sync", dataTable(["Step", "Value", "Status"], [
    ["Local commit", "prepared", statusPill("PASS")],
    ["Push", "origin/main", statusPill("PASS")],
    ["Remote HEAD", "matches local HEAD", statusPill("PASS")],
    ["Post-push cleanup", "transient files removed", statusPill("PASS")],
  ]));
}

function reportExport() {
  return card("Report Export", dataTable(["Format", "File", "Status"], [
    ["Markdown", "acceptance_report_phase_0_9.md", statusPill("PASS")],
    ["HTML", "phase_0_9_local_beta_report.html", statusPill("PASS")],
    ["JSON", "master_validation_phase_0_9.json", statusPill("PASS")],
    ["Bundle", "artifact_manifest.tsv", statusPill("PASS")],
  ]));
}

function errorState() {
  return card("Recoverable Error", `<div class="state-block error"><strong>Preflight gate blocked</strong><p>Database profile hash mismatch. Open diagnostics to rebuild or select a protected manifest.</p><button class="primary">Open Diagnostics</button></div>`);
}

function emptyState() {
  return card("No Runs Yet", `<div class="state-block"><strong>Start from the Input Wizard</strong><p>Create a project, choose a molecule type, validate databases, then launch a local run.</p><button class="primary">Open Wizard</button></div>`);
}

function loadingState() {
  return card("Loading", `<div class="loading-stack"><span class="skeleton"></span><span class="skeleton"></span><span class="skeleton short"></span></div>`);
}

function pageBody(id) {
  if (wizardPages.includes(id)) return wizardPage(id);
  return {
    dashboard_page: dashboard,
    project_create_page: projectCreate,
    run_monitor_page: runMonitor,
    candidate_ranking_page: candidateRanking,
    candidate_detail_page: candidateDetail,
    genome_orf_context_page: genomeContext,
    structure_viewer_page: structureViewer,
    md_summary_page: mdSummary,
    evidence_ledger_page: evidenceLedger,
    database_registry_page: databaseRegistry,
    diagnostics_page: diagnostics,
    settings_page: settings,
    github_sync_page: githubSync,
    report_export_page: reportExport,
    error_state_page: errorState,
    empty_state_page: emptyState,
    loading_state_page: loadingState,
  }[id]();
}

function renderNavigation(activeId) {
  const nav = document.querySelector("[data-nav]");
  nav.innerHTML = navItems
    .map(([id, title, subtitle]) => `<a class="nav-item ${id === activeId ? "active" : ""}" href="#/${id}"><span>${title}</span><small>${subtitle}</small></a>`)
    .join("");
}

function renderDetail(activeId) {
  document.querySelector("[data-detail]").innerHTML = `
    <h2>Run Details</h2>
    ${metricGrid([
      ["Current page", pageTitles[activeId], "PASS"],
      ["Route id", activeId, "PASS"],
      ["API owner", "gls-server", "PASS"],
      ["Scientific owner", "gls-cli", "PASS"],
    ])}
    <h3>API Contract</h3>
    <ul class="compact-list">${apiContracts.map((item) => `<li>${item}</li>`).join("")}</ul>
    <h3>Artifact Tree</h3>
    <ul class="artifact-tree">
      <li>metrics/master_validation_phase_0_9.json</li>
      <li>server/api_schema.openapi.json</li>
      <li>ui/screenshots/final/${activeId}.png</li>
      <li>webui/playwright_final_summary/summary.tsv</li>
    </ul>`;
}

function render() {
  const activeId = routeFromLocation();
  document.querySelector("[data-page-title]").textContent = pageTitles[activeId];
  document.querySelector("[data-main]").innerHTML = pageBody(activeId);
  renderNavigation(activeId);
  renderDetail(activeId);
}

document.addEventListener("DOMContentLoaded", () => {
  initTheme();
  document.querySelector("[data-theme-toggle]").addEventListener("click", () => {
    setTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark");
  });
  window.addEventListener("hashchange", render);
  render();
});

window.PHASE09_ROUTES = screenshotPages;
