const APP_VERSION = "0.9.1";
const LANG_KEY = "genoligand-ui-language";

function readSavedLanguage() {
  try {
    return localStorage.getItem(LANG_KEY) || "zh";
  } catch (_error) {
    return "zh";
  }
}

function saveLanguage(lang) {
  try {
    localStorage.setItem(LANG_KEY, lang);
  } catch (_error) {
    // Storage can be disabled in embedded browser contexts; keep the in-memory language state.
  }
}

const routes = [
  ["workflow", "01", ["工作流", "Workflow"], ["项目、输入、检查和运行", "Projects, inputs, checks, and runs"], "dashboard_page"],
  ["results", "02", ["结果", "Results"], ["候选、结构、MD 和证据", "Candidates, structure, MD, and evidence"], "candidate_ranking_page"],
  ["system", "03", ["系统", "System"], ["环境、数据库、导出和设置", "Environment, databases, export, and settings"], "diagnostics_page"],
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

const extraPages = [
  "candidate_detail_page",
  "genome_orf_context_page",
  "structure_viewer_page",
  "md_summary_page",
  "evidence_ledger_page",
  "error_state_page",
  "empty_state_page",
  "loading_state_page",
];

const navGroups = {
  workflow: [
    "dashboard_page",
    "project_create_page",
    "run_monitor_page",
    "database_registry_page",
    ...wizardPages,
  ],
  results: [
    "candidate_ranking_page",
    "candidate_detail_page",
    "genome_orf_context_page",
    "structure_viewer_page",
    "md_summary_page",
    "evidence_ledger_page",
    "report_export_page",
  ],
  system: [
    "diagnostics_page",
    "setup_wizard_page",
    "settings_page",
    "github_sync_page",
    "error_state_page",
    "empty_state_page",
    "loading_state_page",
  ],
};

const pageMeta = {
  dashboard_page: [["本地工作台", "Dashboard"], ["通过图形界面管理本地 GenoLigand 项目、运行和产物。", "Manage local GenoLigand projects, runs, and artifacts through the browser."]],
  project_create_page: [["项目管理", "Project Management"], ["系统保存项目输入、数据库配置、运行状态、产物索引和报告路径。", "The system persists inputs, database profiles, run state, artifact indexes, and report paths."]],
  input_wizard_project_basics_page: [["项目基本信息", "Project Basics"], ["先定义研究项目、工作区和报告边界。", "Define project identity, workspace, and reporting boundary first."]],
  input_wizard_molecule_type_page: [["分子类型", "Molecule Type"], ["选择小分子、多肽、蛋白质配体或已有复合物。", "Choose small molecule, peptide, protein ligand, or existing complex."]],
  input_wizard_small_molecule_page: [["小分子输入", "Small Molecule Input"], ["粘贴 SMILES、上传结构文件，或填写名称和 CAS 号辅助解析。", "Paste SMILES, upload structure files, or provide name and CAS for assisted resolution."]],
  input_wizard_peptide_page: [["多肽输入", "Peptide Input"], ["粘贴单字母氨基酸序列或上传 FASTA，并记录修饰信息。", "Paste one-letter amino-acid sequence or upload FASTA, with modification notes."]],
  input_wizard_protein_page: [["蛋白质配体输入", "Protein Ligand Input"], ["粘贴 FASTA、上传 PDB/mmCIF 或选择已有预测结构链。", "Paste FASTA, upload PDB/mmCIF, or select a chain from an existing predicted structure."]],
  input_wizard_complex_page: [["已有复合物输入", "Existing Complex Input"], ["上传 receptor-ligand complex 并指定配体或结合区域。", "Upload a receptor-ligand complex and specify ligand or binding region."]],
  input_wizard_genome_page: [["物种与基因组", "Species and Genome"], ["选择预设物种或上传自定义基因组、注释、转录本和 VCF。", "Choose a preset species or upload custom genome, annotation, transcript, and VCF files."]],
  input_wizard_search_scope_page: [["搜索范围", "Search Scope"], ["按科研语言选择保守、扩展转录本或全基因组发现空间。", "Choose conservative, expanded transcript, or whole-genome discovery scope in research terms."]],
  input_wizard_database_profile_page: [["数据库配置", "Database Profile"], ["选择数据库、检查索引状态、空间占用和修复动作。", "Choose databases and inspect index state, storage, and repair actions."]],
  input_wizard_run_profile_page: [["运行模式", "Run Mode"], ["按计算成本、GPU、结构证据和 MD 摘要选择运行方式。", "Select a mode by cost, GPU need, structure evidence, and MD summary requirements."]],
  input_wizard_preflight_page: [["预检查", "Preflight"], ["启动前检查输入、数据库、运行环境、GPU、存储和导出位置。", "Check input, databases, runtime, GPU, storage, and export location before launch."]],
  input_wizard_review_start_page: [["确认并启动", "Review and Start"], ["确认配置后提交本地工作流。", "Review configuration before submitting the local workflow."]],
  run_monitor_page: [["运行监控", "Run Monitor"], ["查看阶段、资源、输出、失败原因和恢复状态；详细日志默认折叠。", "Inspect stage, resources, outputs, failure reason, and recovery; detailed logs stay collapsed."]],
  candidate_ranking_page: [["候选排序", "Candidate Ranking"], ["真实计算完成后展示候选、分数和风险标记。", "Displays candidates, scores, and risk flags after a computed run."]],
  candidate_detail_page: [["候选详情", "Candidate Detail"], ["选择候选后展示来源、ORF、证据、风险和实验建议。", "Select a candidate to inspect source, ORF, evidence, risks, and experimental suggestions."]],
  genome_orf_context_page: [["Genome/ORF Context", "Genome/ORF Context"], ["显示候选来自 CDS、UTR、lncRNA、内含子、反义链等上下文。", "Shows whether candidates originate from CDS, UTR, lncRNA, intron, antisense, and related contexts."]],
  structure_viewer_page: [["结构与 Patch 查看", "Structure and Patch Viewer"], ["查看候选结构、binding patch、pose、关键残基和局部相互作用。", "Inspect candidate structure, binding patch, pose, key residues, and local interactions."]],
  md_summary_page: [["MD 摘要", "MD Summary"], ["查看 RMSD、RMSF、drift、occupancy、persistence 和分数分解。", "Inspect RMSD, RMSF, drift, occupancy, persistence, and score decomposition."]],
  evidence_ledger_page: [["证据链", "Evidence Ledger"], ["按输入、基因组、结构、Docking、MD、数据库和报告分组查看证据。", "Review evidence grouped by input, genome, structure, docking, MD, database, and report."]],
  database_registry_page: [["数据库管理", "Database Registry"], ["管理基因组、注释、转录本、BioLiP2、UniProt、ChEMBL/BindingDB 和结构缓存。", "Manage genome, annotation, transcript, BioLiP2, UniProt, ChEMBL/BindingDB, and structure cache."]],
  diagnostics_page: [["环境诊断", "Diagnostics"], ["基础诊断给出状态、影响和修复动作；高级详情默认关闭。", "Basic diagnostics show state, impact, and repair actions; advanced details stay collapsed."]],
  setup_wizard_page: [["安装向导", "Setup Wizard"], ["首次运行时选择演示或完整科学配置，检查 WSL2/GPU，修复依赖，设置数据目录，下载演示数据并验证清洁流程。", "Choose demo or complete scientific setup, check WSL2/GPU, repair dependencies, set data roots, download demo data, and validate a clean example workflow on first launch."]],
  settings_page: [["设置", "Settings"], ["正式版只提供暗色界面，可切换中文和 English。", "The production console is dark-only and supports Chinese and English."]],
  github_sync_page: [["GitHub 同步", "GitHub Sync"], ["查看分支、commit、远端、推送日志和远端验证结果。", "Inspect branch, commit, remote, push log, and remote verification."]],
  report_export_page: [["报告导出", "Report Export"], ["按研究报告、候选表、复现包、支持包和项目包组织导出。", "Export research reports, candidate tables, reproducibility packages, support bundles, and project packages."]],
  error_state_page: [["错误状态", "Error State"], ["出现错误时显示原因、影响范围、修复动作和可恢复路径。", "Shows cause, impact, repair action, and recovery path when an error occurs."]],
  empty_state_page: [["空状态", "Empty State"], ["无项目、无运行或无结果时不显示虚构内容。", "No invented content is shown when projects, runs, or results are absent."]],
  loading_state_page: [["加载状态", "Loading State"], ["数据加载时保持页面稳定并显示当前请求。", "Keeps layout stable while data is loading and shows the active request."]],
};

const labels = {
  zh: {
    brandSubtitle: "本地科研工作台",
    backendNote: "本地科研工作台已就绪",
    versionBadge: "0.9.1 正式版",
    startSetup: "开始配置",
    export: "导出",
    ready: "就绪",
    idle: "空闲",
    configured: "已配置",
    required: "必填",
    optional: "可选",
    waiting: "待检查",
    warning: "需注意",
    missing: "缺失",
    created: "已创建",
    preflight_passed: "预检查已通过",
    queued: "排队中",
    running: "运行中",
    failed: "失败",
    completed: "已完成",
    cancelled: "已取消",
    resumable: "可恢复",
    pushed_to_github: "已推送",
    noActiveRun: "当前没有绑定运行。创建或打开项目后可提交任务。",
    noCandidates: "尚无真实计算候选。完成运行后这里会显示排序表。",
    notSelected: "尚未选择候选。请先在候选排序表中选择一行。",
    noLedger: "尚无证据链条目。报告生成后会显示映射关系。",
    validationProject: "环境验证项目模板",
    validationProjectText: "用于验证安装、数据库、GPU/OpenMM、Nextflow 恢复、WebUI 流程、输入、预检查、运行监控、结果浏览、报告导出和诊断。模板只保存验证配置，不包含预计算候选或生物学结论。",
    apiOwner: "服务接口",
    scienceOwner: "科学执行",
    storageOwner: "本地存储",
    language: "语言",
  },
  en: {
    brandSubtitle: "Local Research Console",
    backendNote: "Local research workspace ready",
    versionBadge: "0.9.1 Production",
    startSetup: "Start Setup",
    export: "Export",
    ready: "Ready",
    idle: "Idle",
    configured: "Configured",
    required: "Required",
    optional: "Optional",
    waiting: "Waiting",
    warning: "Needs attention",
    missing: "Missing",
    created: "Created",
    preflight_passed: "Preflight passed",
    queued: "Queued",
    running: "Running",
    failed: "Failed",
    completed: "Completed",
    cancelled: "Cancelled",
    resumable: "Resumable",
    pushed_to_github: "Pushed to GitHub",
    noActiveRun: "No active run is attached. Create or open a project to submit a workflow.",
    noCandidates: "No computed candidates yet. Ranking appears after a completed run.",
    notSelected: "No candidate is selected. Select a row from the ranking table first.",
    noLedger: "No evidence ledger rows yet. Mappings appear after report generation.",
    validationProject: "Environment validation project template",
    validationProjectText: "Use it to verify installation, databases, GPU/OpenMM, Nextflow resume, WebUI flow, inputs, preflight, run monitoring, result browsing, exports, and diagnostics. The template stores validation configuration only and contains no precomputed candidates or biological conclusions.",
    apiOwner: "Service API",
    scienceOwner: "Scientific execution",
    storageOwner: "Local storage",
    language: "Language",
  },
};

const statusClass = {
  ready: "ready",
  idle: "info",
  configured: "ready",
  required: "warn",
  optional: "info",
  waiting: "warn",
  warning: "warn",
  missing: "failed",
  created: "info",
  preflight_passed: "ready",
  queued: "info",
  running: "running",
  failed: "failed",
  completed: "ready",
  cancelled: "warn",
  resumable: "warn",
  pushed_to_github: "pushed",
};

const apiRoutes = [
  "GET /api/health",
  "GET /api/version",
  "GET /api/user/health",
  "GET /api/user/projects",
  "GET /api/user/runs",
  "GET /api/user/databases",
  "GET /api/user/diagnostics",
  "POST /api/user/diagnostics/run",
  "POST /api/user/diagnostics/action",
  "GET /api/user/setup/status",
  "POST /api/user/setup/run",
  "GET /api/user/setup/install-plan",
  "POST /api/user/setup/install",
  "POST /api/user/setup/validate-sample-project",
  "GET /api/user/storage/policy",
  "POST /api/user/projects/run-example",
  "POST /api/user/projects/clear-examples",
  "POST /api/user/projects/import-package",
  "POST /api/user/reports/export",
  "GET /api/debug/diagnostics",
  "GET /api/debug/runtime",
  "GET /api/debug/routes",
  "GET /api/projects",
  "POST /api/projects",
  "POST /api/projects/{id}/copy",
  "DELETE /api/projects/{id}",
  "POST /api/projects/{id}/cleanup",
  "POST /api/projects/{id}/preflight",
  "POST /api/projects/{id}/runs",
  "POST /api/runs/{id}/cancel",
  "POST /api/runs/{id}/resume",
  "GET /api/runs/{id}/logs",
  "GET /api/runs/{id}/candidates",
  "GET /api/runs/{id}/evidence",
  "GET /api/runs/{id}/genome-context",
  "GET /api/runs/{id}/structure",
  "GET /api/runs/{id}/md-summary",
  "GET /api/databases",
  "GET /api/diagnostics",
  "GET /api/guides",
];

const candidateHeaders = [
  ["rank", "rank"],
  ["candidate ID", "candidate ID"],
  ["gene name", "gene name"],
  ["ORF ID", "ORF ID"],
  ["基因组坐标", "genomic coordinates"],
  ["上下文分类", "context class"],
  ["fragment score", "fragment score"],
  ["co-occurrence score", "co-occurrence score"],
  ["pLM score", "pLM score"],
  ["structure score", "structure score"],
  ["docking score", "docking score"],
  ["MD stability score", "MD stability score"],
  ["final priority score", "final priority score"],
  ["risk flags", "risk flags"],
];

const candidateFieldKeys = [
  "rank",
  "candidate_id",
  "gene_name",
  "orf_id",
  "genomic_coordinates",
  "context_class",
  "fragment_score",
  "co_occurrence_score",
  "plm_score",
  "structure_score",
  "docking_score",
  "md_stability_score",
  "final_priority_score",
  "risk_flags",
];

const evidenceHeaders = [
  ["报告项", "Report item"],
  ["原始文件", "Source file"],
  ["manifest", "manifest"],
  ["schema", "schema"],
  ["工具版本", "tool version"],
  ["artifact hash", "artifact hash"],
  ["验证状态", "validation state"],
  ["报告 section", "report section"],
];

const evidenceFieldKeys = [
  "report_item",
  "source_file",
  "manifest",
  "schema",
  "tool_version",
  "artifact_hash",
  "validation_state",
  "report_section",
];

let appState = {
  lang: readSavedLanguage(),
  server: { state: "idle", version: APP_VERSION },
  projects: [],
  selectedProject: null,
  selectedRun: null,
  message: null,
  diagnostics: null,
  diagnosticAction: null,
  setup: null,
  setupInstall: null,
  setupSampleValidation: null,
  storage: null,
  debugRoutes: null,
  developerMode: false,
  databases: null,
  databaseAction: null,
  guides: null,
  github: null,
  exports: null,
  preflight: null,
  runLogs: [],
  runArtifacts: null,
  candidates: null,
  evidence: null,
  genomeContext: null,
  structureArtifacts: null,
  mdSummary: null,
  projectRuns: [],
  selectedCandidateId: null,
  inputDraft: { inputs: [], files: [] },
  fieldValidation: {},
  candidateTable: {
    query: "",
    risk: "all",
    sort: "priority_desc",
    page: 1,
    pageSize: 10,
    visible: { gene: true, priority: true, risk: true, md: true, evidence: true },
  },
  liveStream: {
    status: "disconnected",
    runId: null,
    state: "idle",
    stage: "",
    resource: null,
    messages: [],
    lastEvent: "",
  },
  liveSocket: null,
};

function pick(value) {
  if (Array.isArray(value) && value.length === 1 && Array.isArray(value[0])) {
    return pick(value[0]);
  }
  if (Array.isArray(value)) {
    return appState.lang === "zh" ? value[0] : value[1];
  }
  return value;
}

function fieldKeyFromLabel(label) {
  const raw = Array.isArray(label) ? label[1] || label[0] : label;
  return String(raw || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

function t(key) {
  return labels[appState.lang][key] || labels.zh[key] || key;
}

function routeFromLocation() {
  const raw = window.location.hash.replace(/^#\/?/, "");
  if (!raw) {
    return "setup_wizard_page";
  }
  const all = Object.keys(pageMeta);
  return all.includes(raw) ? raw : "dashboard_page";
}

function statusPill(key, text) {
  const cls = statusClass[key] || "info";
  return `<span class="status ${cls}">${text || t(key)}</span>`;
}

function setStatusNode(node, key) {
  if (!node) {
    return;
  }
  node.className = `status ${statusClass[key] || "info"}`;
  node.textContent = t(key).toUpperCase();
}

function card(title, body, footer = "") {
  return `<section class="panel"><div class="panel-header"><h2>${pick(title)}</h2>${footer}</div><div>${body}</div></section>`;
}

function metricGrid(items) {
  return `<div class="metric-grid">${items
    .map(([label, value, status]) => `<div class="metric"><span>${pick(label)}</span><strong>${pick(value)}</strong>${status ? statusPill(status) : ""}</div>`)
    .join("")}</div>`;
}

function dataTable(headers, rows, emptyMessage) {
  const head = headers.map((h) => `<th>${pick(h)}</th>`).join("");
  const body = rows.length
    ? rows.map((row) => `<tr>${row.map((cell) => `<td>${pick(cell)}</td>`).join("")}</tr>`).join("")
    : `<tr><td colspan="${headers.length}" class="empty-cell">${pick(emptyMessage)}</td></tr>`;
  return `<div class="data-table-wrap"><table class="data-table"><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div>`;
}

function formGrid(rows) {
  return `<div class="form-grid">${rows
    .map(([label, value, hint, type = "text", explicitField = ""]) => {
      const field = explicitField || fieldKeyFromLabel(label);
      return `<label><span>${pick(label)}</span><input type="${type}" value="${pick(value)}" aria-label="${pick(label)}" data-field="${field}"><small>${pick(hint)}</small></label>`;
    })
    .join("")}</div>`;
}

function textArea(label, value, hint, explicitField = "") {
  const field = explicitField || fieldKeyFromLabel(label);
  return `<label class="wide-field"><span>${pick(label)}</span><textarea rows="5" aria-label="${pick(label)}" data-field="${field}">${pick(value)}</textarea><small>${pick(hint)}</small></label>`;
}

function fileDrop(label, hint, acceptText, explicitField = "") {
  const labelText = pick(label);
  const field = explicitField || fieldKeyFromLabel(label);
  return `<label class="file-drop"><strong>${labelText}</strong><span>${pick(hint)}</span><small>${pick(acceptText)}</small><input type="file" aria-label="${labelText}" data-field="${field}"></label>`;
}

function selectedChoiceValue(field) {
  const entries = appState.inputDraft.inputs.filter((entry) => entry.field === field);
  return entries.length ? entries[entries.length - 1].value : "";
}

function optionTiles(items, choiceField = "") {
  const selectedChoice = choiceField ? selectedChoiceValue(choiceField) : "";
  return `<div class="option-tiles">${items
    .map(([title, text, status, explicitValue]) => {
      const value = explicitValue || fieldKeyFromLabel(title);
      const isSelected = choiceField
        ? selectedChoice
          ? selectedChoice === value
          : status === "configured"
        : status === "configured";
      const choiceAttrs = choiceField
        ? `data-choice-field="${choiceField}" data-choice-label="${escapeHtml(pick(title))}" data-choice-value="${escapeHtml(value)}" aria-pressed="${isSelected}"`
        : "";
      return `<button class="option-tile ${isSelected ? "selected" : ""}" type="button" ${choiceAttrs}><strong>${pick(title)}</strong><span>${pick(text)}</span>${status ? statusPill(status) : ""}</button>`;
    })
    .join("")}</div>`;
}

function buttonRow(items) {
  return `<div class="button-row">${items
    .map(([label, variant, href]) => {
      const action = inferAction(label);
      return href
        ? `<a class="button ${variant || ""}" href="${href}">${pick(label)}</a>`
        : `<button class="${variant || ""}" type="button" ${action ? `data-action="${action}"` : ""}>${pick(label)}</button>`;
    })
    .join("")}</div>`;
}

function inferAction(label) {
  const key = Array.isArray(label) ? label[1] : label;
  const actions = {
    "New project": "create-project",
    "Create project": "create-project",
    "Run example": "run-example",
    "Clear examples": "clear-examples",
    "Create environment validation project template": "create-validation-project",
    "Select project": "select-project",
    "Select run": "select-run",
    "Save configuration": "save-configuration",
    "Open project": "open-project",
    "Import Project Package": "import-project-package",
    "Copy project": "copy-project",
    "Delete project": "delete-project",
    "Clean project": "clean-project",
    "Resume last run": "resume-last-run",
    "Check environment": "run-diagnostics",
    "Open Developer Mode": "open-developer-mode",
    "Close Developer Mode": "close-developer-mode",
    "Run setup check": "run-setup-check",
    "Prepare full scientific install": "prepare-full-install",
    "Prepare complete install": "prepare-full-install",
    "Validate sample project": "validate-sample-project",
    "Validate clean workflow": "validate-sample-project",
    "Install missing tools": "prepare-full-install",
    "Run diagnostics": "run-diagnostics",
    "Validate database profile": "validate-databases",
    "Build database indexes": "build-database-indexes",
    "Run preflight": "run-preflight",
    "Run CPU mode": "run-cpu-mode",
    "Open cleanup": "open-cleanup",
    "Start workflow": "start-workflow",
    "Submit run": "submit-run",
    "Cancel run": "cancel-run",
    "Resume interrupted run": "resume-run",
    "Generate export files": "export-report",
    "Export candidate table": "export-report",
    "Pin key columns": "candidate-pin-columns",
    "Open output folder": "open-output-folder",
    "git status": "git-status",
    "git add": "git-add",
    "git commit": "git-commit",
    "git push": "git-push",
    "Verify remote HEAD": "git-verify",
  };
  return actions[key] || "";
}

function wizardRail(activeId) {
  return `<div class="wizard-rail">${wizardPages
    .map((id, index) => `<a class="${id === activeId ? "active" : ""}" href="#/${id}"><span>${index + 1}</span>${pick(pageMeta[id][0])}</a>`)
    .join("")}</div>`;
}

function stateBlock(kind, title, text, actionHref) {
  return `<div class="state-block ${kind || ""}"><h3>${pick(title)}</h3><p>${pick(text)}</p>${actionHref ? buttonRow([[[ "继续配置", "Continue setup" ], "primary", actionHref]]) : ""}</div>`;
}

function dashboard() {
  const recentProjects = appState.projects.slice(0, 5).map((project) => [
    project.project_name || project.project_id || "",
    statusPill(project.project_status || "created"),
    project.latest_run_id ? pick(["已有运行", "Run available"]) : pick(["尚无运行", "No run yet"]),
  ]);
  const recentResults = appState.projectRuns.slice(0, 5).map((run) => [
    friendlyRunName(run),
    statusPill(run.state || "created"),
    run.current_stage || "",
    run.state === "completed" ? pick(["可查看结果", "Results available"]) : pick(["等待完成", "Waiting"]),
  ]);
  return [
    card(
      [["开始", "Start"]],
      buttonRow([
        [["新建项目", "New project"], "primary", null],
        [["打开项目", "Open project"], "", null],
        [["运行示例", "Run example"], "", null],
        [["清空示例", "Clear examples"], "danger", null],
        [["安装向导", "Setup Wizard"], "", "#/setup_wizard_page"],
        [["检查环境", "Check environment"], "", "#/diagnostics_page"],
      ])
    ),
    `<div class="dashboard-run-grid">
      ${card(
        [["最近项目", "Recent Projects"]],
        dataTable(
          [[["项目", "Project"], ["状态", "State"], ["最近运行", "Latest run"]].map((x) => x)][0],
          recentProjects,
          [["尚无本地项目。", "No local projects yet."]]
        )
      )}
      ${card(
        [["最近结果", "Recent Results"]],
        dataTable(
          [[["运行", "Run"], ["状态", "State"], ["阶段", "Stage"], ["输出目录", "Output"]].map((x) => x)][0],
          recentResults,
          [["完成运行后这里显示结果入口。", "Completed runs appear here."]]
        )
      )}
    </div>`,
    card(
      [["关键环境状态", "Key Environment Status"]],
      metricGrid([
        [["本地工作台", "Local workspace"], ["就绪", "Ready"], "ready"],
        [["版本", "Version"], [`GenoLigand ${APP_VERSION}`, `GenoLigand ${APP_VERSION}`], "ready"],
        [["数据库", "Databases"], ["待检查", "Not checked"], "waiting"],
        [["GPU/OpenMM", "GPU/OpenMM"], ["待检查", "Not checked"], "waiting"],
      ])
    ),
  ].join("");
}







function valueOrDash(value) {
  if (value === null || value === undefined || value === "") {
    return "—";
  }
  if (Array.isArray(value)) {
    return value.length ? value.join(", ") : "—";
  }
  if (typeof value === "object") {
    return Object.entries(value)
      .map(([key, entry]) => `${key}: ${entry ?? "—"}`)
      .join("; ");
  }
  return String(value);
}

function rowValue(row, key) {
  if (!row || typeof row !== "object") {
    return "";
  }
  return row[key] ?? row[key.replace(/_/g, " ")] ?? row[key.replace(/_/g, "-")] ?? "";
}

function artifactItems() {
  return appState.runArtifacts?.items || appState.selectedRun?.artifact_index?.items || [];
}

function artifactMatches(item, tokens) {
  const text = `${item.artifact_type || ""} ${item.path || ""}`.toLowerCase();
  return tokens.some((token) => text.includes(token));
}

function artifactRows(tokens) {
  return artifactItems()
    .filter((item) => artifactMatches(item, tokens))
    .map((item) => [item.artifact_type || "", item.path || "", item.exists ? statusPill("ready") : statusPill("waiting")]);
}

function artifactObjectRows(items) {
  return (items || []).map((item) => [
    item.artifact_type || "",
    item.path || "",
    item.exists ? statusPill("ready") : statusPill("waiting"),
  ]);
}

function currentRunNotice() {
  return appState.selectedRun
    ? `${pick(["Current run", "Current run"])}: ${friendlyRunName(appState.selectedRun)}`
    : pick(["当前浏览器会话没有绑定运行。", "No run is attached to this browser session."]);
}

function candidateRows() {
  return appState.candidates?.candidates || [];
}

function candidateKey(row) {
  return (
    rowValue(row, "candidate_id") ||
    rowValue(row, "orf_id") ||
    rowValue(row, "gene_name") ||
    rowValue(row, "rank") ||
    ""
  );
}

function selectedCandidate() {
  const rows = candidateRows();
  if (!rows.length) {
    return null;
  }
  const wanted = appState.selectedCandidateId;
  return rows.find((row) => candidateKey(row) === wanted) || rows[0];
}

function firstRowValue(row, keys) {
  for (const key of keys) {
    const value = rowValue(row, key);
    if (value !== null && value !== undefined && value !== "") {
      return value;
    }
  }
  return "";
}

function rowContains(row, tokens) {
  const text = Object.values(row || {}).join(" ").toLowerCase();
  return tokens.some((token) => token && text.includes(String(token).toLowerCase()));
}

function evidenceRowsForCandidate(row) {
  const rows = appState.evidence?.evidence || [];
  const tokens = [candidateKey(row), rowValue(row, "orf_id"), rowValue(row, "gene_name")].filter(Boolean);
  if (!tokens.length) {
    return [];
  }
  return rows.filter((entry) => rowContains(entry, tokens));
}

function evidenceRowsForTokens(tokens) {
  return (appState.evidence?.evidence || []).filter((entry) => rowContains(entry, tokens));
}

function rowsFromFields(row, fields) {
  return fields.map(([label, keys]) => [label, valueOrDash(firstRowValue(row, keys))]);
}

function detailEvidenceTable(rows, emptyMessage) {
  return dataTable(
    evidenceHeaders,
    rows.map((row) => evidenceFieldKeys.map((key) => valueOrDash(rowValue(row, key)))),
    emptyMessage
  );
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}








function friendlyDatabaseRepairAction(action) {
  const raw = String(action || "").toLowerCase();
  if (raw.includes("validate")) {
    return pick(["验证数据源", "Validate source"]);
  }
  if (raw.includes("index")) {
    return pick(["构建索引", "Build index"]);
  }
  if (raw.includes("optional")) {
    return pick(["可选数据源", "Optional source"]);
  }
  if (raw.includes("configure")) {
    return pick(["配置数据位置", "Configure data location"]);
  }
  return action ? valueOrDash(action) : pick(["检查数据源", "Check source"]);
}



function githubSync() {
  const github = appState.github || {};
  const remoteChecked =
    github.remote_head_matches_local_head !== null && github.remote_head_matches_local_head !== undefined;
  const remoteVerification = remoteChecked
    ? `${github.remote_head_matches_local_head} (${github.remote_validation_status || "checked"})`
    : github.remote_validation_status || "not checked";
  const rows = [
    [["Git remote", "Git remote"], valueOrDash(github.remote), github.remote ? statusPill("ready") : statusPill("waiting")],
    [["当前 branch", "Current branch"], valueOrDash(github.branch), github.branch ? statusPill("ready") : statusPill("waiting")],
    [["本地 commit hash", "Local commit hash"], valueOrDash(github.local_commit_hash), github.local_commit_hash ? statusPill("ready") : statusPill("waiting")],
    [["本地 HEAD", "Local HEAD"], valueOrDash(github.local_head_hash), github.local_head_hash ? statusPill("ready") : statusPill("waiting")],
    [["远端 branch", "Remote branch"], valueOrDash(github.remote_branch), github.remote_branch ? statusPill("ready") : statusPill("waiting")],
    [["远端 HEAD", "Remote HEAD"], valueOrDash(github.remote_head_hash), github.remote_head_hash ? statusPill("ready") : statusPill("idle")],
    [["push 状态", "Push state"], valueOrDash(github.push_status), statusPill(github.push_status === "completed" ? "completed" : "idle")],
    [["push 日志路径", "Push log path"], valueOrDash(github.push_log_path), github.push_log_path ? statusPill("ready") : statusPill("idle")],
    [["push 日志", "Push log"], valueOrDash(github.push_log), statusPill(github.push_log?.length ? "ready" : "idle")],
    [["远端验证日志", "Remote verification log"], valueOrDash(github.remote_verify_log_path), github.remote_verify_log_path ? statusPill("ready") : statusPill("idle")],
    [["远端验证", "Remote verification"], remoteVerification, statusPill(github.remote_head_matches_local_head ? "ready" : "idle")],
    [["工作区摘要", "Working tree summary"], valueOrDash(github.working_tree_log), statusPill(github.working_tree_log?.length ? "warning" : "ready")],
  ];
  return card(
    [["GitHub 同步", "GitHub Sync"]],
    dataTable(
      [[["字段", "Field"], ["值", "Value"], ["状态", "State"]].map((x) => x)][0],
      rows,
      [["尚未读取 Git 状态。", "Git state has not been read yet."]]
    ) +
      buttonRow([["git status", "", null], ["git add", "", null], ["git commit", "", null], ["git push", "primary", null], [["验证 remote HEAD", "Verify remote HEAD"], "", null]])
  );
}


function guideRegistry() {
  const guides = appState.guides?.guides || [];
  return dataTable(
    [[["指南", "Guide"], ["路径", "Path"], ["bytes", "bytes"], ["覆盖项", "Coverage"], ["状态", "State"]].map((x) => x)][0],
    guides.map((guide) => [
      [guide.title_zh || guide.guide_id || "", guide.title_en || guide.guide_id || ""],
      guide.link ? `<a href="${guide.link}" target="_blank" rel="noopener">${guide.path || guide.link}</a>` : valueOrDash(guide.path),
      valueOrDash(guide.bytes),
      valueOrDash(guide.coverage),
      statusPill(guide.exists ? "ready" : "missing", guide.status || (guide.exists ? t("ready") : t("missing"))),
    ]),
    [["尚未读取指南清单。", "Guide manifest has not loaded yet."]]
  );
}


function errorState() {
  return stateBlock(
    "error",
    ["运行遇到错误", "Run Error"],
    ["错误原因会来自 gls-server 或工作流产物；页面会显示影响范围、修复动作和可恢复路径。", "The cause comes from gls-server or workflow artifacts; this page shows impact, repair action, and recovery path."],
    "#/diagnostics_page"
  );
}

function emptyState() {
  return stateBlock("", ["当前没有项目或结果", "No Project Or Result"], [t("noActiveRun"), t("noActiveRun")], "#/project_create_page");
}

function loadingState() {
  return `<div class="loading-stack"><div class="skeleton"></div><div class="skeleton short"></div><div class="skeleton"></div><p>${pick(["正在读取本地服务数据。", "Reading local service data."])}</p></div>`;
}

const pipelineSteps = [
  ["project", ["Project", "Project"], "project_create_page"],
  ["input", ["Input", "Input"], "input_wizard_project_basics_page"],
  ["database", ["Database", "Database"], "database_registry_page"],
  ["scope", ["Scope", "Scope"], "input_wizard_search_scope_page"],
  ["preflight", ["Preflight", "Preflight"], "input_wizard_preflight_page"],
  ["run", ["Run", "Run"], "run_monitor_page"],
  ["results", ["Results", "Results"], "candidate_ranking_page"],
  ["evidence", ["Evidence", "Evidence"], "evidence_ledger_page"],
  ["export", ["Export", "Export"], "report_export_page"],
];

const pagePipelineStep = {
  dashboard_page: "project",
  project_create_page: "project",
  input_wizard_project_basics_page: "input",
  input_wizard_molecule_type_page: "input",
  input_wizard_small_molecule_page: "input",
  input_wizard_peptide_page: "input",
  input_wizard_protein_page: "input",
  input_wizard_complex_page: "input",
  input_wizard_genome_page: "input",
  input_wizard_search_scope_page: "scope",
  input_wizard_database_profile_page: "database",
  database_registry_page: "database",
  input_wizard_run_profile_page: "scope",
  input_wizard_preflight_page: "preflight",
  input_wizard_review_start_page: "run",
  run_monitor_page: "run",
  candidate_ranking_page: "results",
  candidate_detail_page: "results",
  genome_orf_context_page: "results",
  structure_viewer_page: "results",
  md_summary_page: "results",
  evidence_ledger_page: "evidence",
  report_export_page: "export",
  diagnostics_page: "preflight",
  setup_wizard_page: "project",
  settings_page: "project",
};

function userText(value, fallback = "—") {
  const rendered = pick(value);
  if (rendered === null || rendered === undefined || rendered === "") {
    return fallback;
  }
  const scrubbed = String(rendered)
    .replace(/\bmanifest\b/gi, appState.lang === "zh" ? "记录" : "record")
    .replace(/\bschema\b/gi, appState.lang === "zh" ? "格式" : "format")
    .replace(/\bhash\b/gi, appState.lang === "zh" ? "校验值" : "checksum")
    .replace(/\bcommit\b/gi, appState.lang === "zh" ? "版本记录" : "version record")
    .replace(/\bremote\b/gi, appState.lang === "zh" ? "同步端" : "sync target");
  return escapeHtml(scrubbed);
}

function advancedDetails(title, body, intro = "") {
  return `<details class="advanced-panel"><summary>${pick(title)}</summary>${intro ? `<p>${pick(intro)}</p>` : ""}<div class="advanced-body">${body}</div></details>`;
}



function friendlyRunName(run) {
  if (!run) {
    return pick(["尚未选择运行", "No run selected"]);
  }
  return run.display_name || run.project_name || run.current_stage || run.state || pick(["本地运行", "Local run"]);
}

function friendlyArtifactType(type) {
  const text = String(type || "").toLowerCase();
  if (text.includes("run_manifest") || text.includes("manifest")) return pick(["运行记录", "Run record"]);
  if (text.includes("launch_plan")) return pick(["运行计划", "Run plan"]);
  if (text.includes("resource_status")) return pick(["资源状态", "Resource status"]);
  if (text.includes("artifact_index")) return pick(["产物清单", "Artifact list"]);
  if (text.includes("run_artifact")) return pick(["运行产物", "Run artifact"]);
  if (text.includes("report")) return pick(["报告", "Report"]);
  if (text.includes("candidate")) return pick(["候选表", "Candidate table"]);
  if (text.includes("evidence")) return pick(["证据", "Evidence"]);
  if (text.includes("structure") || text.includes("patch") || text.includes("pose")) return pick(["结构", "Structure"]);
  if (text.includes("md") || text.includes("trajectory") || text.includes("rms")) return pick(["MD 摘要", "MD summary"]);
  if (text.includes("log")) return pick(["日志", "Log"]);
  return type || pick(["产物", "Artifact"]);
}

function friendlyState(value) {
  const state = String(value || "waiting");
  const map = {
    created: ["已创建", "Created"],
    preflight_passed: ["预检查通过", "Preflight passed"],
    queued: ["排队中", "Queued"],
    running: ["运行中", "Running"],
    failed: ["失败", "Failed"],
    cancelled: ["已取消", "Cancelled"],
    completed: ["已完成", "Completed"],
    resumable: ["可恢复", "Resumable"],
    waiting: ["等待", "Waiting"],
    ready: ["就绪", "Ready"],
  };
  return pick(map[state] || [state, state]);
}

function projectRows() {
  return appState.projects.map((project) => [
    `<button type="button" data-action="select-project" data-project-id="${encodeURIComponent(project.project_id || "")}">${pick(["打开", "Open"])}</button>`,
    userText(project.project_name || project.project_id),
    statusPill(project.project_status || "created", friendlyState(project.project_status || "created")),
    friendlyProjectMode(project),
    project.latest_run_id ? pick(["有结果入口", "Result available"]) : pick(["尚未运行", "Not run yet"]),
    (project.report_paths || []).length ? pick(["有报告", "Report available"]) : pick(["未生成", "Not generated"]),
  ]);
}

function friendlyProjectMode(project) {
  if (project?.read_only_review_mode) {
    return pick(["只读审阅", "Read-only review"]);
  }
  if (project?.example_project || project?.collaboration_mode === "demo_example") {
    return pick(["演示示例", "Demo example"]);
  }
  return pick(["可编辑项目", "Editable project"]);
}

function projectListCard() {
  return card(
    [["本地项目", "Local Projects"]],
    dataTable(
      [[["操作", "Action"], ["项目", "Project"], ["状态", "State"], ["模式", "Mode"], ["最近结果", "Latest result"], ["报告", "Report"]].map((x) => x)][0],
      projectRows(),
      [["尚无本地项目。", "No local project yet."]]
    ) +
      advancedDetails(
        [["高级详情", "Advanced details"]],
        dataTable(
          [["project_id", "latest_run_id", "report_paths"]],
          appState.projects.map((project) => [
            valueOrDash(project.project_id),
            valueOrDash(project.latest_run_id),
            valueOrDash(project.report_paths),
          ]),
          [["无项目详情。", "No project details."]]
        )
      )
  );
}

function projectRunHistoryCard() {
  const rows = appState.projectRuns.map((run) => [
    `<button type="button" data-action="select-run" data-run-id="${encodeURIComponent(run.run_id || "")}">${pick(["查看", "View"])}</button>`,
    friendlyRunName(run),
    statusPill(run.state || "created", friendlyState(run.state || "created")),
    run.current_stage || pick(["等待开始", "Waiting to start"]),
    run.resumable ? pick(["可以恢复", "Can resume"]) : pick(["正常", "Normal"]),
  ]);
  return card(
    [["运行摘要", "Run Summary"]],
    dataTable(
      [[["操作", "Action"], ["运行", "Run"], ["状态", "State"], ["当前阶段", "Current stage"], ["恢复", "Recovery"]].map((x) => x)][0],
      rows,
      [["当前项目尚无运行记录。", "The selected project has no run records yet."]]
    ) +
      buttonRow([[[ "继续上次运行", "Resume last run" ], "primary", null], [[ "打开输出目录", "Open output folder" ], "", "#/report_export_page"]]) +
      advancedDetails(
        [["Run identifiers", "Run identifiers"]],
        dataTable(
          [["run_id", "nextflow_run_id", "output_dir"]],
          appState.projectRuns.map((run) => [valueOrDash(run.run_id), valueOrDash(run.nextflow_run_id), valueOrDash(run.output_dir)]),
          [["尚无运行详情。", "No run details."]]
        )
      )
  );
}

function projectPage() {
  return [
    projectListCard(),
    projectRunHistoryCard(),
    card(
      [["项目操作", "Project Actions"]],
      formGrid([
        [["项目名称", "Project name"], ["", ""], ["用于本地项目和报告标题", "Used for the local project and report title"]],
        [["工作区位置", "Workspace location"], ["", ""], ["选择已有目录或创建新目录", "Choose an existing directory or create a new one"]],
        [["研究对象", "Research object"], ["", ""], ["简要描述输入材料、物种和搜索范围", "Briefly describe input material, organism, and scope"]],
      ]) +
        buttonRow([
          [["新建项目", "New project"], "primary", null],
          [["打开项目", "Open project"], "", null],
          [["Import Project Package", "Import Project Package"], "", null],
          [["Project Package", "Project Package"], "", "#/report_export_page"],
          [["Backup", "Backup"], "", null],
          [["Reproducibility Package", "Reproducibility Package"], "", "#/report_export_page"],
          [["清理项目", "Clean project"], "danger", null],
        ])
    ),
    card(
      [["项目包含内容", "Project Contents"]],
      dataTable(
        [[["类别", "Category"], ["用户可见内容", "User-facing content"], ["状态", "State"]].map((x) => x)][0],
        [
          [["输入", "Inputs"], ["分子、序列、结构、基因组和搜索范围", "Molecules, sequences, structures, genome, and scope"], statusPill("waiting")],
          [["数据库", "Databases"], ["数据版本、索引状态、空间和修复动作", "Data versions, index state, storage, and repair actions"], statusPill("waiting")],
          [["运行", "Runs"], ["阶段、资源、恢复状态和结果入口", "Stages, resources, recovery state, and result entry"], statusPill("waiting")],
          [["结果", "Results"], ["候选排序、结构、MD、证据和导出包", "Candidates, structure, MD, evidence, and export packages"], statusPill("waiting")],
        ],
        [["尚未创建项目。", "No project has been created yet."]]
      )
    ),
  ].join("");
}





function databaseRegistry() {
  const payload = appState.databases || {};
  const resources = payload.resources || [];
  const rows = resources.length
    ? resources.map((resource) => {
        const state = typeof resource === "string" ? "waiting" : resource.index_state === "indexed" || resource.index_state === "data_present_index_not_detected" ? "ready" : resource.required ? "waiting" : "optional";
        return [
          typeof resource === "string" ? resource : `${resource.resource || ""}${resource.required ? " *" : ""}`,
          statusPill(state, typeof resource === "string" ? pick(["待配置", "Needs setup"]) : valueOrDash(resource.index_state)),
          typeof resource === "string" ? pick(["选择数据位置", "Select data location"]) : friendlyDatabaseRepairAction(resource.repair_action),
          typeof resource === "string" ? "—" : `${valueOrDash(resource.storage_bytes)} bytes`,
        ];
      })
    : [
        [["Genome", "Genome"], statusPill("waiting"), ["选择 genome profile", "Select genome profile"], "—"],
        [["Annotation", "Annotation"], statusPill("waiting"), ["上传或选择 GTF/GFF3", "Upload or select GTF/GFF3"], "—"],
        ["BioLiP2", statusPill("waiting"), ["配置数据根目录", "Configure data root"], "—"],
        ["UniProt", statusPill("waiting"), ["构建序列索引", "Build sequence index"], "—"],
        ["ChEMBL/BindingDB", statusPill("waiting"), ["验证 warehouse", "Validate warehouse"], "—"],
      ].map((row) => [row[0], row[1], row[2], ["Waiting for check", "Waiting for check"], row[3], ["Run diagnostics", "Run diagnostics"]]);
  return [
    card(
      [["数据库设置", "Database Setup"]],
      dataTable(
        [[["资源", "Resource"], ["状态", "State"], ["下一步", "Next step"], ["空间", "Storage"]].map((x) => x)][0],
        rows,
        [["尚未选择数据库 profile。", "No database profile selected."]]
      ) +
        buttonRow([[[ "验证数据库", "Validate databases" ], "", null], [[ "构建索引", "Build indexes" ], "primary", null]])
    ),
    advancedDetails(
      [["Advanced database details", "Advanced database details"]],
      dataTable(
        [["resource", "path", "version", "sha256", "file_count"]],
        resources
          .filter((resource) => typeof resource !== "string")
          .map((resource) => [valueOrDash(resource.resource), valueOrDash(resource.path), valueOrDash(resource.version || resource.acquisition), valueOrDash(resource.sha256), valueOrDash(resource.file_count)]),
        [["无数据库详情。", "No database details."]]
      )
    ),
  ].join("");
}

function friendlyDiagnosticRow(check) {
  if (check.group && check.impact) {
    return [
      [check.group, check.group],
      check.status || "waiting",
      [check.impact, check.impact],
      [check.reason || "", check.reason || ""],
      [check.repair_action || "Run diagnostics", check.repair_action || "Run diagnostics"],
      [check.recheck_action || "Run diagnostics", check.recheck_action || "Run diagnostics"],
    ];
  }
  const raw = `${check.check_id || ""} ${check.description || ""}`.toLowerCase();
  if (raw.includes("wsl") || raw.includes("linux") || raw.includes("windows")) {
    return [["WSL2 / Linux", "WSL2 / Linux"], check.status || "waiting", ["确认本地科学运行后端是否可用", "Confirms local scientific backend availability"], ["打开 Runtime", "Open Runtime"]];
  }
  if (raw.includes("openmm") || raw.includes("cuda") || raw.includes("gpu") || raw.includes("nvidia")) {
    return [["OpenMM / CUDA", "OpenMM / CUDA"], check.status || "waiting", ["完整物理中筛需要 GPU 证据；可切换 CPU 模式", "Full physical screening needs GPU evidence; CPU mode is available"], ["Repair GPU", "Repair GPU"]];
  }
  if (raw.includes("java") || raw.includes("nextflow")) {
    return [["Java / Nextflow", "Java / Nextflow"], check.status || "waiting", ["工作流调度异常会阻止运行", "Workflow scheduler problems block runs"], ["修复运行环境", "Repair runtime"]];
  }
  if (raw.includes("rust") || raw.includes("gls")) {
    return [["本地服务", "Local service"], check.status || "waiting", ["本地服务不可用会阻止项目和运行操作", "Local service problems block project and run actions"], ["重新构建服务", "Rebuild service"]];
  }
  if (raw.includes("python") || raw.includes("worker")) {
    return [["Python worker", "Python worker"], check.status || "waiting", ["科学计算 worker 不可用会阻止结构和 MD 步骤", "Scientific workers are required for structure and MD steps"], ["检查 worker", "Check worker"]];
  }
  if (raw.includes("database") || raw.includes("artifact") || raw.includes("data")) {
    return [["数据库", "Databases"], check.status || "waiting", ["缺少索引会阻止或限制搜索", "Missing indexes block or limit search"], ["Open database setup", "Open database setup"]];
  }
  if (raw.includes("storage") || raw.includes("disk") || raw.includes("write")) {
    return [["存储", "Storage"], check.status || "waiting", ["空间不足或不可写会导致运行失败", "Low or unwritable storage can fail a run"], ["Open cleanup", "Open cleanup"]];
  }
  return [["环境检查", "Environment check"], check.status || "waiting", ["需要确认后再启动运行", "Confirm before starting a run"], ["Run diagnostics", "Run diagnostics"]];
}

function diagnosticActionId(check, fallback = "") {
  if (check?.repair_action_id) {
    return check.repair_action_id;
  }
  const text = `${check?.group || ""} ${check?.check_id || ""} ${check?.repair_action || ""} ${fallback}`.toLowerCase();
  if (text.includes("gpu") || text.includes("cuda") || text.includes("openmm")) return "repair-gpu";
  if (text.includes("worker") || text.includes("python")) return "repair-worker";
  if (text.includes("java") || text.includes("nextflow") || text.includes("runtime") || text.includes("wsl")) return "repair-runtime";
  if (text.includes("database") || text.includes("index")) return "open-database-setup";
  if (text.includes("storage") || text.includes("cleanup") || text.includes("cache")) return "clean-cache";
  if (text.includes("export")) return "open-export";
  return "run-diagnostics";
}

function diagnosticActionLabel(actionId, fallback = "") {
  const labelsByAction = {
    "repair-runtime": ["Repair runtime", "Repair runtime"],
    "repair-worker": ["Repair worker", "Repair worker"],
    "repair-gpu": ["Repair GPU", "Repair GPU"],
    "open-database-setup": ["Open database setup", "Open database setup"],
    "clean-cache": ["Clean cache", "Clean cache"],
    "run-cpu-mode": ["Run CPU mode", "Run CPU mode"],
    "open-export": ["Open export", "Open export"],
    "run-diagnostics": ["Run diagnostics", "Run diagnostics"],
  };
  return labelsByAction[actionId] || [fallback || "Run diagnostics", fallback || "Run diagnostics"];
}

function diagnosticFixButton(check, fallback = "") {
  const actionId = diagnosticActionId(check, fallback);
  const label = diagnosticActionLabel(actionId, fallback);
  return `<button type="button" data-action="${escapeHtml(actionId)}" data-diagnostic-action="${escapeHtml(actionId)}">${pick(label)}</button>`;
}

function diagnosticRecheckButton() {
  return `<button type="button" data-action="run-diagnostics" data-diagnostic-action="run-diagnostics">${pick(["Recheck", "Recheck"])}</button>`;
}

function diagnostics() {
  const checks = appState.diagnostics?.diagnostics || [];
  const basicRows = checks.length
    ? checks.map((check) => {
        const row = friendlyDiagnosticRow(check);
        if (row.length >= 6) {
          const [label, state, impact, reason, action, recheck] = row;
          return [label, statusPill(state), impact, reason, diagnosticFixButton(check, pick(action)), diagnosticRecheckButton(check, pick(recheck))];
        }
        const [label, state, impact, action] = row;
        return [label, statusPill(state), impact, ["Waiting for check", "Waiting for check"], diagnosticFixButton(check, pick(action)), diagnosticRecheckButton()];
      })
    : [
        ["WSL2 / Linux", statusPill("waiting"), ["确认本地科学运行后端是否可用", "Confirms local scientific backend availability"], ["打开 Runtime", "Open Runtime"]],
        ["Java / Nextflow", statusPill("waiting"), ["工作流调度不可用会阻止运行", "Workflow scheduler problems block runs"], ["修复运行环境", "Repair runtime"]],
        ["Python worker", statusPill("waiting"), ["科学计算 worker 不可用会阻止结构和 MD 步骤", "Scientific workers are required for structure and MD steps"], ["检查 worker", "Check worker"]],
        ["OpenMM / CUDA", statusPill("waiting"), ["完整物理中筛需要 GPU 证据；可切 CPU 模式", "Full physical screening needs GPU evidence; CPU mode is available"], ["Repair GPU", "Repair GPU"]],
        [["数据库", "Databases"], statusPill("waiting"), ["缺少索引会阻止或限制搜索", "Missing indexes block or limit search"], ["Open database setup", "Open database setup"]],
        [["存储", "Storage"], statusPill("waiting"), ["空间不足会导致运行失败", "Low storage can fail a run"], ["Open cleanup", "Open cleanup"]],
      ];
  return [
    card(
      [["基础诊断", "Basic Diagnostics"]],
      dataTable(
        [[["检查", "Check"], ["状态", "State"], ["影响", "Impact"], ["原因", "Reason"], ["修复动作", "Fix"], ["复查", "Recheck"]].map((x) => x)][0],
        basicRows,
        [["尚未运行诊断。", "Diagnostics have not run yet."]]
      ) + buttonRow([[[ "运行环境诊断", "Run diagnostics" ], "primary", null], [[ "打开数据库设置", "Open database setup" ], "", "#/database_registry_page"], [[ "打开清理", "Open cleanup" ], "", "#/settings_page"]])
    ),
    advancedDetails(
      [["Advanced diagnostics", "Advanced diagnostics"]],
      dataTable(
        [["check_id", "command", "stdout/stderr", "raw_state"]],
        checks.map((check) => [valueOrDash(check.check_id), valueOrDash(check.command), valueOrDash(check.stdout || check.stderr), valueOrDash(check.status)]),
        [["无高级诊断详情。", "No advanced diagnostics details."]]
      )
    ),
  ].join("");
}





function renderPage(id) {
  const map = {
    dashboard_page: dashboard,
    project_create_page: projectPage,
    run_monitor_page: runMonitor,
    candidate_ranking_page: candidateRanking,
    candidate_detail_page: candidateDetail,
    genome_orf_context_page: genomeContext,
    structure_viewer_page: structureViewer,
    md_summary_page: mdSummary,
    evidence_ledger_page: evidenceLedger,
    database_registry_page: databaseRegistry,
    diagnostics_page: diagnostics,
    setup_wizard_page: setupWizard,
    settings_page: settings,
    github_sync_page: githubSync,
    report_export_page: reportExport,
    error_state_page: errorState,
    empty_state_page: emptyState,
    loading_state_page: loadingState,
  };
  if (wizardPages.includes(id)) {
    return wizardPage(id);
  }
  return (map[id] || dashboard)();
}

function renderNav(activeId) {
  document.querySelector("[data-nav]").innerHTML = routes
    .map(([group, index, title, subtitle, target]) => {
      const active = (navGroups[group] || []).includes(activeId);
      return `<a class="nav-item ${active ? "active" : ""}" href="#/${target}"><span class="nav-icon">${index}</span><span class="nav-copy"><strong>${pick(title)}</strong><small>${pick(subtitle)}</small></span></a>`;
    })
    .join("");
}





function setupToolRows() {
  const tools = appState.setup?.tools || [];
  return tools.length
    ? tools.map((tool) => [
        tool.name || tool.tool_id || "",
        humanSetupMode(tool.mode || ""),
        statusPill(tool.status || (tool.ready ? "ready" : "warning")),
        tool.impact || "",
        [tool.repair_action, tool.install_hint, tool.verify_command].filter(Boolean).join(" | "),
      ])
    : [
        ["WSL2 / Linux backend", humanSetupMode("demo_and_full"), statusPill("waiting"), ["本地科学后端尚未检查。", "Local scientific backend has not been checked yet."], ["运行 setup check", "Run setup check"]],
        ["Java / Nextflow", humanSetupMode("demo_and_full"), statusPill("waiting"), ["工作流调度器状态未知。", "Workflow scheduler state is unknown."], ["运行 setup check", "Run setup check"]],
        ["OpenMM / CUDA", humanSetupMode("full_scientific"), statusPill("waiting"), ["完整物理中筛状态未知。", "Full physical screening state is unknown."], ["运行 setup check", "Run setup check"]],
      ];
}

function storagePolicyRows() {
  const storage = appState.storage || {};
  const policies = storage.policies || [];
  return policies.length
    ? policies.map((policy) => [policy.policy || "", policy.impact || "", policy.recovery || ""])
    : [
        [["运行清理", "Run cleanup"], ["导出报告后删除选定的运行中间产物。", "Deletes selected run intermediates after report export."], ["输入仍可用时，可从保留的项目包重新运行。", "Re-run from preserved project package when inputs remain available."]],
        [["索引清理", "Index cleanup"], ["只在存储审查后删除可重建索引。", "Removes rebuildable indexes only after storage review."], ["从数据库设置重新构建。", "Rebuild from database setup."]],
        [["轨迹压缩", "Trajectory compression"], ["保留摘要指标并压缩大型轨迹。", "Keeps summary metrics and compresses large trajectories."], ["需要原始轨迹时使用完整产物归档。", "Use full artifact archive when raw trajectory is required."]],
      ];
}

function humanPolicyValue(value) {
  const raw = String(value || "").trim();
  const normalized = raw.toLowerCase();
  const labels = {
    compress_after_export: ["导出后压缩轨迹", "Compress trajectories after export"],
    manual_review_required: ["删除前人工复核", "Manual review before deletion"],
    full_scientific: ["完整科学配置", "Complete scientific setup"],
    demo_only: ["演示验证", "Demo validation"],
  };
  if (labels[normalized]) {
    return pick(labels[normalized]);
  }
  return raw.replaceAll("_", " ");
}

function humanSetupMode(value) {
  const raw = String(value || "").trim();
  const normalized = raw.toLowerCase();
  const labels = {
    demo_and_full: ["演示与完整模式", "Demo and full"],
    full_scientific: ["完整科学配置", "Complete scientific setup"],
    demo_only: ["演示验证", "Demo validation"],
    "demo-only": ["演示验证", "Demo validation"],
    "full scientific": ["完整科学配置", "Complete scientific setup"],
    "complete scientific setup": ["完整科学配置", "Complete scientific setup"],
  };
  if (labels[normalized]) {
    return pick(labels[normalized]);
  }
  return raw.replaceAll("_", " ");
}

function setupModeRows() {
  const setup = appState.setup || {};
  const modes = setup.setup_modes || [];
  return modes.length
    ? modes.map((mode) => [
        humanSetupMode(mode.mode || ""),
        statusPill(mode.status || "waiting"),
        mode.download_size || "",
        mode.disk_required || "",
        mode.index_time || "",
        mode.version_policy || "",
        mode.resume || "",
        mode.retry || "",
      ])
    : [
        [humanSetupMode("demo-only"), statusPill("ready"), ["最小下载", "minimal"], ["低于 2 GB", "under 2 GB"], ["分钟级", "minutes"], ["本地服务和紧凑回归数据", "local service and compact regression data"], ["不需要", "not needed"], ["重新运行 setup check", "rerun setup check"]],
        [humanSetupMode("full scientific"), statusPill("warning"), ["下载前确认", "confirm before download"], ["取决于数据库和轨迹", "database and trajectory dependent"], ["可能需要数小时", "hours possible"], ["记录工具和数据库版本", "record tool and database versions"], ["需要断点续传", "resume required"], ["重试失败来源", "retry failed source"]],
      ];
}

function storageQuotaRows() {
  const storage = appState.storage || {};
  const protectedItems = storage.protected_items || [];
  return [
    [["缓存上限", "Cache limit"], `${storage.cache_limit_gb || 50} GB`, ["删除前先审查临时文件。", "Temporary files are reviewed before deletion."]],
    [["配额预警", "Quota warning"], `${storage.quota_warning_gb || 45} GB`, ["接近上限前提醒。", "Warn before cache reaches the limit."]],
    [["轨迹保留", "Trajectory retention"], humanPolicyValue(storage.trajectory_retention || "compress_after_export"), ["完整物理中筛前决定原始轨迹保留策略。", "Raw trajectory retention is decided before full physical screening."]],
    [["受保护项目", "Protected items"], protectedItems.length ? protectedItems.join(", ") : pick(["当前项目包、报告、紧凑回归基线", "current project package, reports, compact regressions"]), ["清理流程不会删除这些内容。", "These are not removed by cleanup."]],
  ];
}

function setupWizardStepRows() {
  const setup = appState.setup || {};
  const fullReady = Boolean(setup.full_scientific_ready);
  const demoReady = setup.demo_only_ready !== false;
  const readyTools = setup.ready_tool_count ?? 0;
  const requiredTools = setup.required_tool_count ?? 0;
  return [
    [["1. 选择模式", "1. Choose mode"], demoReady ? statusPill("ready") : statusPill("warning"), ["Demo 可立即验证用户流程；完整科学配置会继续检查工具、GPU 和数据库。", "Demo validates the user flow immediately; complete scientific setup continues through tools, GPU, and databases."], ["Run setup check", "Run setup check"]],
    [["2. WSL2 / GPU 检测", "2. WSL2 / GPU check"], fullReady ? statusPill("ready") : statusPill("warning"), ["确认 Linux 后端、NVIDIA 驱动和 CUDA/OpenMM 平台证据。", "Confirm Linux backend, NVIDIA driver, and CUDA/OpenMM platform evidence."], ["Repair GPU", "Repair GPU"]],
    [["3. 依赖修复", "3. Dependency repair"], readyTools >= requiredTools && requiredTools > 0 ? statusPill("ready") : statusPill("warning"), [`${readyTools}/${requiredTools} tools ready`, `${readyTools}/${requiredTools} tools ready`], ["Run setup check", "Run setup check"]],
    [["4. 数据目录", "4. Data roots"], statusPill(appState.databases?.status || "waiting"), ["设置数据库、索引和外部数据根目录。", "Set database, index, and external data roots."], ["Open database setup", "Open database setup"]],
    [["5. Demo database", "5. Demo database"], demoReady ? statusPill("ready") : statusPill("waiting"), ["只在用户点击 Run example 后创建演示流程和结果。", "Demo workflow and results are created only after the user clicks Run example."], ["Run example", "Run example"]],
    [["6. 示例项目验证", "6. Sample project validation"], statusPill(appState.setupSampleValidation?.status || "waiting"), ["从新建项目、输入、preflight、运行、结果到 Project Package 导出，并在结束后清理临时项目。", "Runs project creation, input, preflight, run, results, and Project Package export, then removes the temporary project."], ["Validate clean workflow", "Validate clean workflow"]],
  ];
}

function setupWizard() {
  const setup = appState.setup || {};
  const install = appState.setupInstall || {};
  return [
    card(
      [["首次运行安装向导", "First-launch Setup Wizard"]],
      dataTable(
        [[["步骤", "Step"], ["状态", "State"], ["说明", "Description"], ["下一步", "Next action"]].map((x) => x)][0],
        setupWizardStepRows(),
        [["Setup status has not loaded.", "Setup status has not loaded."]]
      ) +
        buttonRow([
          [["Run setup check", "Run setup check"], "primary", null],
          [["Prepare complete install", "Prepare complete install"], "", null],
          [["Validate clean workflow", "Validate clean workflow"], "", null],
          [["Clear examples", "Clear examples"], "danger", null],
          [["Check environment", "Check environment"], "", null],
          [["Open database setup", "Open database setup"], "", "#/database_registry_page"],
        ])
    ),
    card(
      [["Demo / Complete scientific setup", "Demo / Complete scientific setup"]],
      dataTable(
        [[["模式", "Mode"], ["状态", "State"], ["下载量", "Download"], ["磁盘", "Disk"], ["索引时间", "Index time"], ["版本记录", "Version record"], ["断点续传", "Resume"], ["失败重试", "Retry"]].map((x) => x)][0],
        setupModeRows(),
        [["Setup modes have not loaded.", "Setup modes have not loaded."]]
      ) +
        metricGrid([
          [["Ready tools", "Ready tools"], `${setup.ready_tool_count ?? 0}/${setup.required_tool_count ?? 0}`, setup.status || "waiting"],
          [["Single command", "Single command"], setup.single_command || "target\\debug\\gls-server.exe --host 127.0.0.1 --port 19090", "info"],
          [["Complete install command", "Complete install command"], install.command || setup.full_scientific_install_command || "Run Prepare complete install", install.command ? "ready" : "waiting"],
          [["Install plan record", "Install plan record"], install.manifest ? "Created" : setup.install_report_path ? "Setup check record" : "Run setup check", install.manifest || setup.install_report_path ? "ready" : "waiting"],
          [["Install report", "Install report"], setup.report || setup.install_report_path || pick(["运行 setup check 后生成", "Run setup check to generate"]), setup.report ? "ready" : "waiting"],
          [["Clean workflow validation", "Clean workflow validation"], appState.setupSampleValidation?.status || pick(["尚未运行", "Not run"]), appState.setupSampleValidation?.status || "waiting"],
          [["Clean workflow report", "Clean workflow report"], appState.setupSampleValidation?.summary_tsv || pick(["运行 clean workflow validation 后生成", "Run clean workflow validation to generate"]), appState.setupSampleValidation?.summary_tsv ? "ready" : "waiting"],
        ])
    ),
    card(
      [["Scientific tool repair queue", "Scientific tool repair queue"]],
      dataTable(
        [[["工具", "Tool"], ["适用模式", "Mode"], ["状态", "State"], ["影响", "Impact"], ["修复", "Repair"]].map((x) => x)][0],
        setupToolRows(),
        [["Tool status has not loaded.", "Tool status has not loaded."]]
      )
    ),
  ].join("");
}

function settings() {
  const storage = appState.storage || {};
  const debugRoutes = appState.developerMode ? [
    ...(appState.debugRoutes?.debug_routes || []),
    ...(appState.debugRoutes?.user_routes || []),
  ] : [];
  const dailyRows = [
    [["Language", "Language"], appState.lang === "zh" ? "中文" : "English", statusPill("ready")],
    [["Theme", "Theme"], ["Dark only", "Dark only"], statusPill("ready")],
    [["Default start page", "Default start page"], ["Workflow", "Workflow"], statusPill("ready")],
    [["Run log density", "Run log density"], ["Summary first; raw logs collapsed", "Summary first; raw logs collapsed"], statusPill("ready")],
    [["Evidence density", "Evidence density"], ["Summary cards first; technical details collapsed", "Summary cards first; technical details collapsed"], statusPill("ready")],
    [["Cache limit", "Cache limit"], `${storage.cache_limit_gb || 50} GB`, statusPill(storage.status || "waiting")],
  ];
  return [
    card(
      [["Settings", "Settings"]],
      dataTable(
        [[["Preference", "Preference"], ["Value", "Value"], ["State", "State"]].map((x) => x)][0],
        dailyRows,
        [["Settings have not loaded.", "Settings have not loaded."]]
      ) +
        buttonRow([
          [["Open setup wizard", "Open setup wizard"], "primary", "#/setup_wizard_page"],
          [["Check environment", "Check environment"], "", "#/diagnostics_page"],
        ])
    ),
    card(
      [["Storage preferences", "Storage preferences"]],
      dataTable(
        [[["Item", "Item"], ["Policy", "Policy"], ["Impact", "Impact"]].map((x) => x)][0],
        storageQuotaRows(),
        [["Storage policy has not loaded.", "Storage policy has not loaded."]]
      )
    ),
    advancedDetails(
      [["Advanced / Developer Mode", "Advanced / Developer Mode"]],
      `<div class="warning-box">${pick(["开发者区域包含 schema、manifest、hash、route、gate、raw JSON、commit 和 remote 等复现审计信息；普通科研流程不需要打开。", "Developer mode contains schema, manifest, hash, route, gate, raw JSON, commit, and remote information for reproducibility audits. Ordinary research workflows do not need it."])}</div>` +
        (appState.developerMode
          ? dataTable(
              [["API", "Surface"]],
              debugRoutes.map((route) => [route, route.startsWith("GET /api/user") || route.startsWith("POST /api/user") ? "user" : "debug"]),
              [["Developer routes have not loaded.", "Developer routes have not loaded."]]
            ) +
            buttonRow([[[ "Close Developer Mode", "Close Developer Mode" ], "", null], [[ "Open Git/GitHub sync", "Open Git/GitHub sync" ], "", "#/github_sync_page"]]) +
            guideRegistry()
          : `<p>${pick(["默认不会读取开发者接口、版本同步或原始审计详情。需要排错或复现审计时再打开。", "Developer APIs, version sync, and raw audit details are not loaded by default. Open this only for troubleshooting or reproducibility review."])}</p>` +
            buttonRow([[[ "Open Developer Mode", "Open Developer Mode" ], "", null]]))
    ),
  ].join("");
}

function validationBadge(state, text) {
  const status = state === "pass" ? "ready" : state === "fail" ? "failed" : state === "warn" ? "warning" : "waiting";
  return statusPill(status, pick(text));
}


function normalizeFieldText(value) {
  return String(value || "").toLowerCase();
}

function inputValuesMatching(tokens) {
  const lowered = tokens.map(normalizeFieldText);
  return appState.inputDraft.inputs
    .filter((entry) => {
      const field = normalizeFieldText(entry.field);
      const label = normalizeFieldText(entry.label);
      return lowered.some((token) => field.includes(token) || label.includes(token));
    })
    .map((entry) => String(entry.value || "").trim())
    .filter(Boolean);
}

function filesMatching(tokens, extensions = []) {
  const loweredTokens = tokens.map(normalizeFieldText);
  const loweredExtensions = extensions.map(normalizeFieldText);
  return appState.inputDraft.files.filter((entry) => {
    const field = normalizeFieldText(entry.field);
    const label = normalizeFieldText(entry.label);
    const name = normalizeFieldText(entry.name);
    const tokenOk = !loweredTokens.length || loweredTokens.some((token) => field.includes(token) || label.includes(token) || name.includes(token));
    const extOk = !loweredExtensions.length || loweredExtensions.some((ext) => name.endsWith(ext));
    return tokenOk && extOk;
  });
}

function validSmilesClient(value) {
  const text = String(value || "").trim();
  if (!text) {
    return false;
  }
  if (/\s/.test(text) || /[^A-Za-z0-9@+\-\[\]\(\)=#$\\/%.:]/.test(text)) {
    return false;
  }
  return /[A-Zcnops]/.test(text);
}

function sequenceStats(values) {
  const joined = values.join("").replace(/^>.*$/gm, "").replace(/\s+/g, "").toUpperCase();
  if (!joined) {
    return { length: 0, invalid: 0 };
  }
  const invalid = (joined.match(/[^ACDEFGHIKLMNPQRSTVWY]/g) || []).length;
  return { length: joined.length, invalid };
}

function fileStatus(tokens, extensions) {
  const matching = filesMatching(tokens, extensions);
  const related = filesMatching(tokens);
  if (matching.length) {
    return { state: "pass", fix: ["File type recognized.", "File type recognized."] };
  }
  if (related.length) {
    return { state: "fail", fix: ["Use a supported file extension.", "Use a supported file extension."] };
  }
  return { state: "waiting", fix: ["Add a file to check.", "Add a file to check."] };
}

function computedFieldValidation(label, fallbackState, fallbackFix) {
  const name = normalizeFieldText(Array.isArray(label) ? label[1] || label[0] : label);
  if (name.includes("smiles")) {
    const values = inputValuesMatching(["smiles"]);
    if (!values.length) {
      return { state: "waiting", fix: ["Paste SMILES to check.", "Paste SMILES to check."] };
    }
    return values.some(validSmilesClient)
      ? { state: "pass", fix: ["Format looks usable; worker will check chemistry.", "Format looks usable; worker will check chemistry."] }
      : { state: "fail", fix: ["Remove spaces or invalid SMILES characters.", "Remove spaces or invalid SMILES characters."] };
  }
  if (name.includes("structure file")) {
    return fileStatus(["structure", "complex", "upload"], [".sdf", ".mol2", ".pdb", ".cif", ".mmcif"]);
  }
  if (name.includes("fasta") || name.includes("peptide sequence") || name.includes("protein sequence")) {
    const values = inputValuesMatching(["fasta", "peptide_sequence", "protein_sequence"]);
    const files = filesMatching(["fasta", "peptide", "protein"], [".fa", ".fasta", ".faa", ".fna"]);
    if (files.length) {
      return { state: "pass", fix: ["FASTA file type recognized.", "FASTA file type recognized."] };
    }
    const stats = sequenceStats(values);
    if (!stats.length) {
      return { state: "waiting", fix: ["Paste sequence or add FASTA.", "Paste sequence or add FASTA."] };
    }
    if (stats.invalid) {
      return { state: "fail", fix: ["Remove non-standard residues or record modifications.", "Remove non-standard residues or record modifications."] };
    }
    return stats.length >= 3
      ? { state: "pass", fix: ["Sequence characters and length look usable.", "Sequence characters and length look usable."] }
      : { state: "warn", fix: ["Sequence is very short; confirm it is intended.", "Sequence is very short; confirm it is intended."] };
  }
  if (name.includes("chain id")) {
    const values = inputValuesMatching(["chain_id", "ligand_chain", "chain"]);
    if (!values.length) {
      return { state: "waiting", fix: ["Enter chain after loading a structure.", "Enter chain after loading a structure."] };
    }
    return values.every((value) => /^[A-Za-z0-9_.-]{1,8}$/.test(value))
      ? { state: "pass", fix: ["Chain label format is usable.", "Chain label format is usable."] }
      : { state: "fail", fix: ["Use a short chain label such as A or L.", "Use a short chain label such as A or L."] };
  }
  if (name.includes("ligand residue")) {
    const values = inputValuesMatching(["residue_name", "ligand_residue"]);
    if (!values.length) {
      return { state: "waiting", fix: ["Enter residue name after structure load.", "Enter residue name after structure load."] };
    }
    return values.every((value) => /^[A-Za-z0-9]{1,6}$/.test(value))
      ? { state: "pass", fix: ["Residue name format is usable.", "Residue name format is usable."] }
      : { state: "fail", fix: ["Use the ligand residue name from the structure.", "Use the ligand residue name from the structure."] };
  }
  if (name.includes("gtf") || name.includes("gff3")) {
    return fileStatus(["gtf", "gff", "annotation"], [".gtf", ".gff", ".gff3"]);
  }
  if (name.includes("vcf")) {
    const files = filesMatching(["vcf", "variant"], [".vcf", ".vcf.gz"]);
    return files.length
      ? { state: "pass", fix: ["Variant file type recognized.", "Variant file type recognized."] }
      : { state: "warn", fix: ["Optional; add VCF only when variants are needed.", "Optional; add VCF only when variants are needed."] };
  }
  if (name.includes("database")) {
    return selectedChoiceValue("database_profile")
      ? { state: "pass", fix: ["Database profile selected.", "Database profile selected."] }
      : { state: fallbackState, fix: fallbackFix };
  }
  if (name.includes("search scope")) {
    return selectedChoiceValue("search_scope")
      ? { state: "pass", fix: ["Search scope selected.", "Search scope selected."] }
      : { state: fallbackState, fix: fallbackFix };
  }
  if (name.includes("run profile")) {
    return selectedChoiceValue("run_profile")
      ? { state: "pass", fix: ["Run mode selected.", "Run mode selected."] }
      : { state: fallbackState, fix: fallbackFix };
  }
  return { state: fallbackState, fix: fallbackFix };
}

function fieldValidationRows(rows) {
  return `<div class="field-validation-grid">${rows
    .map(([label, state, fix]) => {
      const validation = computedFieldValidation(label, state, fix);
      return `<div class="field-status ${validation.state}">
        <span>${pick(label)}</span>
        ${validationBadge(validation.state, validation.state === "pass" ? ["PASS", "PASS"] : validation.state === "warn" ? ["WARN", "WARN"] : validation.state === "fail" ? ["FAIL", "FAIL"] : ["Pending", "Pending"])}
        <small>${pick(validation.fix)}</small>
      </div>`;
    })
    .join("")}</div>`;
}

function inputMaterialCard(title, desc, href, state = "waiting") {
  return `<a class="material-card" href="#/${href}">
    <strong>${pick(title)}</strong>
    <span>${pick(desc)}</span>
    ${statusPill(state)}
  </a>`;
}

function structurePreview(kind = "small") {
  return `<div class="structure-preview ${kind}">
    <div class="preview-gridline"></div>
    <div class="preview-molecule"></div>
    <div class="preview-chain"></div>
    <span>${pick(["结构预览在输入通过后显示", "Structure preview appears after input passes"])}</span>
  </div>`;
}


function runForecastModel() {
  const scope = selectedChoiceValue("search_scope") || "conservative_search";
  const profile = selectedChoiceValue("run_profile") || "demo";
  const scopeMap = {
    conservative_search: {
      candidates: ["Low", "Low"],
      time: ["Minutes", "Minutes"],
      disk: ["Under 2 GB", "Under 2 GB"],
      scenario: ["Known annotation check", "Known annotation check"],
    },
    expanded_transcript_search: {
      candidates: ["Medium", "Medium"],
      time: ["Minutes to hours", "Minutes to hours"],
      disk: ["2-20 GB", "2-20 GB"],
      scenario: ["Expanded transcript evidence", "Expanded transcript evidence"],
    },
    whole_genome_discovery_search: {
      candidates: ["Large", "Large"],
      time: ["Hours or longer", "Hours or longer"],
      disk: ["20 GB or more", "20 GB or more"],
      scenario: ["Discovery search", "Discovery search"],
    },
  };
  const profileMap = {
    demo: {
      gpu: ["Not required", "Not required"],
      cpu: ["Default", "Default"],
      outputs: ["Demo report", "Demo report"],
      needsConfirmation: false,
    },
    standard: {
      gpu: ["Optional", "Optional"],
      cpu: ["Available", "Available"],
      outputs: ["Ranking and evidence cards", "Ranking and evidence cards"],
      needsConfirmation: false,
    },
    structure: {
      gpu: ["Helpful for docking and MD checks", "Helpful for docking and MD checks"],
      cpu: ["Available with longer runtime", "Available with longer runtime"],
      outputs: ["Structure, patch, docking evidence", "Structure, patch, docking evidence"],
      needsConfirmation: false,
    },
    physical: {
      gpu: ["CUDA OpenMM recommended", "CUDA OpenMM recommended"],
      cpu: ["Fallback for smaller runs", "Fallback for smaller runs"],
      outputs: ["Ranking, structure, docking, MD, report", "Ranking, structure, docking, MD, report"],
      needsConfirmation: true,
    },
    quick_environment_check: {
      gpu: ["Not required", "Not required"],
      cpu: ["Default", "Default"],
      outputs: ["Environment check", "Environment check"],
      needsConfirmation: false,
    },
  };
  return {
    scope,
    profile,
    ...(scopeMap[scope] || scopeMap.conservative_search),
    ...(profileMap[profile] || profileMap.demo),
  };
}

function runForecastCards() {
  const forecast = runForecastModel();
  return `<div class="forecast-grid">
    <div class="forecast-card"><span>${pick(["Estimated candidates", "Estimated candidates"])}</span><strong>${pick(forecast.candidates)}</strong><small>${pick(["Updates with search scope", "Updates with search scope"])}</small></div>
    <div class="forecast-card"><span>${pick(["Time class", "Time class"])}</span><strong>${pick(forecast.time)}</strong><small>${pick(["Full physical screen is highest", "Full physical screen is highest"])}</small></div>
    <div class="forecast-card"><span>${pick(["GPU guidance", "GPU guidance"])}</span><strong>${pick(forecast.gpu)}</strong><small>${pick(["Checked again in preflight", "Checked again in preflight"])}</small></div>
    <div class="forecast-card"><span>${pick(["CPU fallback", "CPU fallback"])}</span><strong>${pick(forecast.cpu)}</strong><small>${pick(["Use for smaller or diagnostic runs", "Use for smaller or diagnostic runs"])}</small></div>
    <div class="forecast-card"><span>${pick(["Storage need", "Storage need"])}</span><strong>${pick(forecast.disk)}</strong><small>${pick(["Trajectory retention changes this", "Trajectory retention changes this"])}</small></div>
    <div class="forecast-card"><span>${pick(["Main outputs", "Main outputs"])}</span><strong>${pick(forecast.outputs)}</strong><small>${pick(forecast.scenario)}</small></div>
  </div>`;
}

function forecastConfirmed() {
  const visible = document.querySelector('[data-field="forecast_confirmed"]');
  if (visible?.type === "checkbox") {
    return visible.checked;
  }
  return appState.inputDraft.inputs.some((entry) => entry.field === "forecast_confirmed" && String(entry.value) === "true");
}

function requiresForecastConfirmation() {
  return runForecastModel().needsConfirmation && !forecastConfirmed();
}

function forecastConfirmationPanel() {
  const forecast = runForecastModel();
  const checked = forecastConfirmed() ? "checked" : "";
  if (!forecast.needsConfirmation) {
    return fieldValidationRows([
      [["Forecast confirmation", "Forecast confirmation"], "pass", ["Only required for full physical screen.", "Only required for full physical screen."]],
    ]);
  }
  return `<label class="forecast-confirmation">
    <input type="checkbox" data-field="forecast_confirmed" aria-label="Confirm full physical forecast" ${checked}>
    <span>${pick(["Confirm full physical forecast", "Confirm full physical forecast"])}</span>
    <small>${pick(["I reviewed estimated candidates, time, GPU, CPU fallback, storage, outputs, and fit for this run.", "I reviewed estimated candidates, time, GPU, CPU fallback, storage, outputs, and fit for this run."])}</small>
  </label>`;
}

function resultTabs(active = "overview") {
  const tabs = [
    ["overview", ["Overview", "Overview"], "candidate_detail_page"],
    ["genome", ["Genome", "Genome"], "genome_orf_context_page"],
    ["structure", ["Structure", "Structure"], "structure_viewer_page"],
    ["docking", ["Docking", "Docking"], "structure_viewer_page"],
    ["md", ["MD", "MD"], "md_summary_page"],
    ["evidence", ["Evidence", "Evidence"], "evidence_ledger_page"],
    ["export", ["Export", "Export"], "report_export_page"],
  ];
  return `<div class="result-tabs">${tabs
    .map(([key, label, href]) => `<a class="${key === active ? "active" : ""}" href="#/${href}">${pick(label)}</a>`)
    .join("")}</div>`;
}

function genomeTrackGraphic(rows = []) {
  const markers = [
    ["upstream", 4, 14, "optional", 58],
    ["ORF span", 14, 62, "ready", 112],
    ["CDS", 18, 38, "ready", 88],
    ["UTR", 40, 52, "waiting", 88],
    ["intron", 54, 66, "optional", 150],
    ["antisense", 20, 58, "warning", 198],
    ["pseudogene", 68, 82, "optional", 150],
    ["variant", 84, 90, "failed", 58],
    ["downstream", 90, 98, "optional", 58],
  ];
  return `<div class="genome-track">
    <div class="track-axis"><span>5'</span><span>3'</span></div>
    ${markers.map(([label, left, width, state, top]) => `<div class="track-segment ${state}" style="left:${left}%;width:${width - left}%;top:${top}px">${label}</div>`).join("")}
    <p>${rows.length ? pick(["已读取候选基因组位置、链方向和变异位置", "Candidate genomic positions, strand, and variant positions loaded"]) : pick(["完成候选生成后显示 upstream、ORF span、CDS、UTR、intron、antisense、pseudogene、variant 和 downstream", "After candidate generation, shows upstream, ORF span, CDS, UTR, intron, antisense, pseudogene, variant, and downstream"])}</p>
  </div>`;
}

function miniLineChart(title, points, state = "waiting", meta = "") {
  const polyline = points.map(([x, y]) => `${x},${y}`).join(" ");
  return `<div class="mini-chart">
    <div class="mini-chart-head"><strong>${pick(title)}</strong>${statusPill(state)}</div>
    <svg viewBox="0 0 100 42" role="img" aria-label="${escapeHtml(pick(title))}">
      <polyline points="${polyline}" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"></polyline>
    </svg>
    ${meta ? `<small class="mini-chart-meta">${escapeHtml(meta)}</small>` : ""}
  </div>`;
}

function wizardPage(id) {
  const materialGrid = `<div class="material-grid">
    ${inputMaterialCard(["化合物名称", "Compound name"], ["可从名称开始，后续补结构", "Start from a name, then add structure"], "input_wizard_small_molecule_page", "waiting")}
    ${inputMaterialCard(["结构文件", "Structure file"], ["SDF / MOL2 / PDB / mmCIF", "SDF / MOL2 / PDB / mmCIF"], "input_wizard_complex_page", "waiting")}
    ${inputMaterialCard(["多肽序列", "Peptide sequence"], ["一字母序列或 FASTA", "One-letter sequence or FASTA"], "input_wizard_peptide_page", "waiting")}
    ${inputMaterialCard(["蛋白序列", "Protein sequence"], ["FASTA 或预测结构", "FASTA or predicted structure"], "input_wizard_protein_page", "waiting")}
    ${inputMaterialCard(["复合物结构", "Complex structure"], ["已有 receptor-ligand complex", "Existing receptor-ligand complex"], "input_wizard_complex_page", "waiting")}
    ${inputMaterialCard(["自定义基因组", "Custom genome"], ["FASTA / GTF / GFF3 / VCF", "FASTA / GTF / GFF3 / VCF"], "input_wizard_genome_page", "waiting")}
  </div>`;
  const bodies = {
    input_wizard_project_basics_page: [
      card([["你手里有什么材料？", "What material do you have?"]], materialGrid),
      card(
        [["项目基础信息", "Project basics"]],
        formGrid([
          [["项目名称", "Project name"], ["", ""], ["用于本地项目和报告标题", "Used for the local project and report title"]],
          [["研究问题", "Research question"], ["", ""], ["用普通科研语言说明目标", "State the goal in research language"]],
          [["输出位置", "Output location"], ["", ""], ["本地可写目录", "Local writable directory"]],
        ]) +
          fieldValidationRows([
            [["项目名称", "Project name"], "warn", ["建议填写，便于区分运行", "Recommended for separating runs"]],
            [["输出位置", "Output location"], "waiting", ["选择后检查写入权限", "Writable check after selection"]],
          ])
      ),
    ].join(""),
    input_wizard_molecule_type_page: card([["选择输入材料", "Choose input material"]], materialGrid),
    input_wizard_small_molecule_page: [
      card(
        [["小分子输入", "Small-molecule input"]],
        formGrid([
          [["化合物名称", "Compound name"], ["", ""], ["可选，不能替代结构检查", "Optional, not a substitute for structure checks"]],
          [["SMILES", "SMILES"], ["", ""], ["粘贴后检查格式、价态和三维构象需求", "Checks format, valence, and 3D conformer needs"]],
          [["CAS", "CAS"], ["", ""], ["可选辅助字段", "Optional helper field"]],
        ]) +
          fileDrop([["上传结构文件", "Upload structure file"]], [["拖入或选择 SDF/MOL2/PDB", "Drop or choose SDF/MOL2/PDB"]], [["上传后显示结构预览", "Preview appears after upload"]]) +
          fieldValidationRows([
            [["SMILES", "SMILES"], "waiting", ["粘贴后即时检查", "Validated after paste"]],
            [["结构文件", "Structure file"], "waiting", ["上传后检查可读性", "Readability check after upload"]],
          ]) +
          structurePreview("small")
      ),
    ].join(""),
    input_wizard_peptide_page: card(
      [["多肽输入", "Peptide input"]],
      textArea([["多肽序列", "Peptide sequence"]], [["", ""]], [["粘贴一字母氨基酸序列", "Paste one-letter amino-acid sequence"]]) +
        `<div class="chip-row"><button type="button" data-insert-peptide="IKVAV">IKVAV</button><button type="button" data-insert-peptide="YIGSR">YIGSR</button><button type="button" data-insert-peptide="RGD">RGD</button></div>` +
        fileDrop([["上传 FASTA", "Upload FASTA"]], [["选择 .fa / .fasta", "Choose .fa / .fasta"]], [["上传后检查长度和非法字符", "Checks length and invalid characters"]]) +
        fieldValidationRows([
          [["FASTA", "FASTA"], "waiting", ["上传后检查格式", "Format check after upload"]],
          [["非标准残基", "Non-standard residues"], "waiting", ["发现后给出修复建议", "Fix suggestion when detected"]],
        ])
    ),
    input_wizard_protein_page: card(
      [["蛋白输入", "Protein input"]],
      textArea([["FASTA 序列", "FASTA sequence"]], [["", ""]], [["可作为蛋白配体或受体输入", "Use as protein ligand or receptor input"]]) +
        fileDrop([["上传 PDB/mmCIF 或预测结构", "Upload PDB/mmCIF or predicted structure"]], [["支持 chain 和 residue 检查", "Supports chain and residue checks"]], [["PDB / CIF / mmCIF", "PDB / CIF / mmCIF"]]) +
        formGrid([
          [["Chain ID", "Chain ID"], ["", ""], ["留空则在预检查中选择", "Leave empty to choose during preflight"]],
          [["Residue range", "Residue range"], ["", ""], ["可选", "Optional"]],
        ]) +
        fieldValidationRows([
          [["FASTA", "FASTA"], "waiting", ["粘贴后检查字符和长度", "Checks characters and length"]],
          [["Chain ID", "Chain ID"], "waiting", ["读取结构后检查", "Checked after structure load"]],
        ]) +
        structurePreview("protein")
    ),
    input_wizard_complex_page: card(
      [["复合物结构", "Complex structure"]],
      fileDrop([["上传 receptor-ligand complex", "Upload receptor-ligand complex"]], [["指定 ligand chain、residue 或结合区", "Specify ligand chain, residue, or binding region"]], [["PDB / mmCIF / CIF", "PDB / mmCIF / CIF"]]) +
        formGrid([
          [["Ligand chain", "Ligand chain"], ["", ""], ["蛋白、多肽或核酸链", "Protein, peptide, or nucleic-acid chain"]],
          [["Ligand residue", "Ligand residue"], ["", ""], ["如结构中存在 HETATM ligand", "If a HETATM ligand exists"], "text", "residue_name"],
          [["Binding region", "Binding region"], ["", ""], ["用 chain 和 residue 编号描述", "Use chain and residue numbering"]],
        ]) +
        fieldValidationRows([
          [["结构文件", "Structure file"], "waiting", ["上传后检查可读性", "Readability check after upload"]],
          [["Ligand residue", "Ligand residue"], "waiting", ["读取结构后检查", "Checked after structure load"]],
        ]) +
        structurePreview("complex")
    ),
    input_wizard_genome_page: card(
      [["基因组输入", "Genome input"]],
      formGrid([
        [["物种", "Species"], ["Homo sapiens", "Homo sapiens"], ["可改为自定义物种", "Can be changed"]],
        [["基因组版本", "Genome version"], ["GRCh38", "GRCh38"], ["用于报告和筛选范围", "Used in report and scope"]],
      ]) +
        fileDrop([["自定义文件", "Custom files"]], [["genome FASTA / GTF / GFF3 / transcript FASTA / VCF", "genome FASTA / GTF / GFF3 / transcript FASTA / VCF"]], [["上传后检查格式和索引需求", "Checks format and index needs"]]) +
        fieldValidationRows([
          [["FASTA", "FASTA"], "waiting", ["上传后检查", "Checked after upload"]],
          [["GTF/GFF3", "GTF/GFF3"], "waiting", ["上传后检查", "Checked after upload"]],
          [["VCF", "VCF"], "waiting", ["可选", "Optional"]],
        ])
    ),
    input_wizard_search_scope_page: [
      card(
        [["搜索范围", "Search scope"]],
        optionTiles([
          [["保守搜索", "Conservative search"], ["已知 CDS 和高可信注释，成本最低。", "Known CDS and high-confidence annotation, lowest cost."], "configured"],
          [["扩展转录本搜索", "Expanded transcript search"], ["覆盖非经典 ORF，成本中等。", "Covers non-canonical ORFs, medium cost."], "ready"],
          [["全基因组发现搜索", "Whole-genome discovery search"], ["六框翻译候选空间，成本最高。", "Six-frame translated space, highest cost."], "ready"],
        ], "search_scope") +
          fieldValidationRows([
            [["Search scope", "Search scope"], "waiting", ["选择搜索范围后显示成本和覆盖影响。", "Select a scope to show cost and coverage impact."]],
          ])
      ),
      card([["资源预测", "Resource forecast"]], runForecastCards()),
    ].join(""),
    input_wizard_database_profile_page: [
      databaseRegistry(),
      card(
        [["数据库字段检查", "Database field checks"]],
        fieldValidationRows([
          [["Database profile", "Database profile"], "waiting", ["选择数据库 profile 后检查索引和空间。", "Choose a database profile to check indexes and storage."]],
        ])
      ),
    ].join(""),
    input_wizard_run_profile_page: [
      card(
        [["运行模式", "Run mode"]],
        optionTiles([
          [["Demo-only", "Demo-only"], ["最小数据验证完整流程。", "Minimal data validates the flow."], "configured", "demo"],
          [["标准筛选", "Standard screening"], ["候选排序和基础证据链。", "Candidate ranking and base evidence."], "ready", "standard"],
          [["结构增强", "Structure-enhanced"], ["增加结构、patch 和 docking 证据。", "Adds structure, patch, and docking evidence."], "ready", "structure"],
          [["完整物理中筛", "Full physical screen"], ["需要确认 GPU、时间和磁盘 forecast。", "Requires GPU, time, and storage forecast confirmation."], "warning", "physical"],
        ], "run_profile") +
          fieldValidationRows([
            [["Run profile", "Run profile"], "waiting", ["选择运行模式后确认时间、GPU 和输出。", "Select a run mode to confirm time, GPU, and outputs."]],
          ])
      ),
      card([["运行前确认", "Run forecast confirmation"]], runForecastCards() + fieldValidationRows([
        [["GPU 建议", "GPU guidance"], "warn", ["完整物理中筛前确认", "Confirm before full physical screen"]],
        [["CPU fallback", "CPU fallback"], "pass", ["可用于较小运行", "Available for smaller runs"]],
        [["磁盘空间", "Storage"], "waiting", ["预检查时确认", "Confirmed during preflight"]],
      ])),
      card([["Forecast confirmation", "Forecast confirmation"]], forecastConfirmationPanel()),
    ].join(""),
    input_wizard_preflight_page: preflightTable(),
    input_wizard_review_start_page: [
      card([["启动摘要", "Launch Summary"]], metricGrid([
        [["项目", "Project"], appState.selectedProject ? appState.selectedProject.project_name || appState.selectedProject.project_id : ["未选择", "Not selected"], appState.selectedProject ? "ready" : "waiting"],
        [["输入", "Input"], ["等待验证", "Pending validation"], "waiting"],
        [["运行模式", "Run mode"], ["Demo-only 或标准筛选", "Demo-only or standard screening"], "configured"],
        [["提交状态", "Submit state"], ["尚未提交", "Not submitted"], "idle"],
      ]) + runForecastCards() + forecastConfirmationPanel()),
      buttonRow([[[ "保存配置", "Save configuration" ], "", null], [[ "执行预检查", "Run preflight" ], "", null], [[ "启动任务", "Start workflow" ], "primary", null]]),
    ].join(""),
  };
  const body = bodies[id] || materialGrid;
  return `<div class="wizard-layout">${wizardRail(id)}<section class="wizard-card">${body}</section></div>`;
}



function candidateDetail() {
  const candidate = selectedCandidate();
  if (!candidate) {
    return [
      resultTabs("overview"),
      stateBlock("", [["未选择候选", "No Candidate Selected"]], [t("notSelected"), t("notSelected")], "#/candidate_ranking_page"),
      card([["详情页结构", "Detail structure"]], optionTiles([
        [["Overview", "Overview"], ["候选摘要、风险和下一步实验建议。", "Summary, risks, and follow-up guidance."], "waiting"],
        [["Genome / Structure / Docking / MD", "Genome / Structure / Docking / MD"], ["按层级阅读证据。", "Layered evidence reading."], "waiting"],
        [["Evidence / Export", "Evidence / Export"], ["查看证据卡并导出报告。", "Review evidence cards and export reports."], "waiting"],
      ])),
    ].join("");
  }
  const evidenceRows = evidenceRowsForCandidate(candidate);
  const detailJumpRows = [
    [["Overview", "Overview"], ["候选摘要、风险和下一步实验建议。", "Candidate summary, risk, and follow-up guidance."], `<a class="button" href="#/candidate_detail_page">${pick(["打开", "Open"])}</a>`],
    [["Genome", "Genome"], ["ORF、基因组上下文和变异位置。", "ORF, genomic context, and variant position."], `<a class="button" href="#/genome_orf_context_page">${pick(["打开", "Open"])}</a>`],
    [["Structure", "Structure"], ["结构、patch 和相互作用注释。", "Structure, patch, and interaction annotations."], `<a class="button" href="#/structure_viewer_page">${pick(["打开", "Open"])}</a>`],
    [["Docking", "Docking"], ["对接姿势、结构证据和可视化入口。", "Docking pose, structure evidence, and viewer entry."], `<a class="button" href="#/structure_viewer_page">${pick(["打开", "Open"])}</a>`],
    [["MD", "MD"], ["RMSD、RMSF、接触、氢键和漂移摘要。", "RMSD, RMSF, contacts, H-bonds, and drift summary."], `<a class="button" href="#/md_summary_page">${pick(["打开", "Open"])}</a>`],
    [["Evidence", "Evidence"], ["证据卡、可信度和 why this matters。", "Evidence cards, confidence, and why this matters."], `<a class="button" href="#/evidence_ledger_page">${pick(["打开", "Open"])}</a>`],
    [["Export", "Export"], ["报告、候选表和复现包导出。", "Report, candidate table, and reproducibility package export."], `<a class="button" href="#/report_export_page">${pick(["打开", "Open"])}</a>`],
  ];
  return [
    resultTabs("overview"),
    card([["阅读路径", "Reading path"]], dataTable(
      [[["层级", "Layer"], ["内容", "Content"], ["入口", "Entry"]].map((x) => x)][0],
      detailJumpRows,
      [["尚未选择候选。", "No candidate selected."]]
    )),
    card([["候选概览", "Candidate Overview"]], metricGrid([
      [["Candidate", "Candidate"], valueOrDash(candidateKey(candidate)), "ready"],
      [["Priority", "Priority"], valueOrDash(firstRowValue(candidate, ["final_priority_score", "priority_score", "score"])), "waiting"],
      [["Risk", "Risk"], valueOrDash(rowValue(candidate, "risk_flags")), rowValue(candidate, "risk_flags") ? "warning" : "ready"],
      [["Evidence", "Evidence"], String(evidenceRows.length), evidenceRows.length ? "ready" : "waiting"],
    ])),
    card(
      [["关键指标", "Key metrics"]],
      dataTable(
        [[["指标", "Metric"], ["值", "Value"]].map((x) => x)][0],
        rowsFromFields(candidate, [
          [["gene", "gene"], ["gene_name", "gene"]],
          [["ORF", "ORF"], ["orf_id", "orf"]],
          [["genome context", "genome context"], ["context_class", "genomic_coordinates"]],
          [["structure score", "structure score"], ["structure_score"]],
          [["docking score", "docking score"], ["docking_score"]],
          [["MD stability", "MD stability"], ["md_stability_score"]],
          [["validation guidance", "validation guidance"], ["experimental_validation_guidance", "validation_guidance"]],
        ]),
        [["当前候选缺少可展示字段。", "The current candidate row has no displayable fields."]]
      )
    ),
    card([["相关产物", "Artifacts"]], dataTable(
      [[["类别", "Type"], ["状态", "State"], ["入口", "Entry"]].map((x) => x)][0],
      [
        [["Overview", "Overview"], statusPill("ready"), `<a class="button" href="#/candidate_detail_page">${pick(["打开", "Open"])}</a>`],
        [["Genome/ORF", "Genome/ORF"], statusPill("waiting"), `<a class="button" href="#/genome_orf_context_page">${pick(["打开", "Open"])}</a>`],
        [["Structure/Patch", "Structure/Patch"], statusPill("waiting"), `<a class="button" href="#/structure_viewer_page">${pick(["打开", "Open"])}</a>`],
        [["Docking", "Docking"], statusPill("waiting"), `<a class="button" href="#/structure_viewer_page">${pick(["打开", "Open"])}</a>`],
        [["MD", "MD"], statusPill("waiting"), `<a class="button" href="#/md_summary_page">${pick(["打开", "Open"])}</a>`],
        [["Evidence", "Evidence"], statusPill(evidenceRows.length ? "ready" : "waiting"), `<a class="button" href="#/evidence_ledger_page">${pick(["打开", "Open"])}</a>`],
        [["Export", "Export"], statusPill("ready"), `<a class="button" href="#/report_export_page">${pick(["打开", "Open"])}</a>`],
      ],
      [["暂无产物。", "No artifacts yet."]]
    )),
  ].join("");
}

function genomeContext() {
  const payload = appState.genomeContext || {};
  const rows = payload.genome_context?.length ? payload.genome_context : candidateRows();
  return [
    resultTabs("genome"),
    card([["Genome/ORF 轨道", "Genome/ORF Track"]], genomeTrackGraphic(rows)),
    card(
      [["候选位置", "Candidate positions"]],
      dataTable(
        [[["候选", "Candidate"], ["Gene", "Gene"], ["ORF", "ORF"], ["Coordinates", "Coordinates"], ["Strand / Frame", "Strand / Frame"], ["Variant", "Variant"]].map((x) => x)][0],
        rows.map((row) => [
          valueOrDash(candidateKey(row)),
          valueOrDash(rowValue(row, "gene_name")),
          valueOrDash(rowValue(row, "orf_id")),
          valueOrDash(rowValue(row, "genomic_coordinates")),
          valueOrDash(firstRowValue(row, ["strand", "reading_frame", "frame", "orf_frame"])),
          valueOrDash(firstRowValue(row, ["variant_id", "variant", "allele"])),
        ]),
        payload.message || [["完成候选生成后显示 Genome/ORF context。", "Genome/ORF context appears after candidate generation."]]
      )
    ),
  ].join("");
}



function userEvidenceTable(rows, emptyMessage) {
  return dataTable(
    [[["证据", "Evidence"], ["重要性", "Importance"], ["可信度", "Confidence"], ["状态", "State"], ["Why this matters", "Why this matters"]].map((x) => x)][0],
    (rows || []).map((row) => [
      valueOrDash(rowValue(row, "report_item") || rowValue(row, "evidence_type") || rowValue(row, "source")),
      valueOrDash(rowValue(row, "importance") || rowValue(row, "priority") || pick(["支持排序解释", "Supports ranking interpretation"])),
      valueOrDash(rowValue(row, "confidence") || rowValue(row, "confidence_level") || pick(["待确认", "Pending"])),
      statusPill(rowValue(row, "validation_state") || rowValue(row, "status") || "waiting"),
      valueOrDash(rowValue(row, "why_this_matters") || rowValue(row, "interpretation") || pick(["帮助判断该候选是否值得后续实验。", "Helps decide whether the candidate merits follow-up."])),
    ]),
    emptyMessage
  );
}

function preflightTable() {
  const checks = appState.preflight?.checks || [];
  const findState = (tokens) => {
    const hit = checks.find((check) => {
      const text = `${check.check_id || ""} ${check.coverage || ""} ${check.description || ""} ${check.repair_action || ""}`.toLowerCase();
      return tokens.some((token) => text.includes(token));
    });
    return hit?.status || "waiting";
  };
  const rows = [
    [["Input", "Input"], findState(["input", "smiles", "fasta", "chain", "ligand"]), ["SMILES、结构、FASTA、chain 和 ligand residue 需要通过", "SMILES, structure, FASTA, chain, and ligand residue need to pass"], buttonRow([[[ "Fix input", "Fix input" ], "", "#/input_wizard_small_molecule_page"]])],
    [["Database", "Database"], findState(["database", "index", "profile", "data"]), ["数据库 profile、索引和空间需要可用", "Database profile, indexes, and storage need to be available"], buttonRow([[[ "Open database setup", "Open database setup" ], "", "#/database_registry_page"]])],
    [["Runtime", "Runtime"], findState(["runtime", "java", "nextflow", "worker", "wsl", "linux"]), ["WSL/Linux、Java、Nextflow 和 worker 需要可用", "WSL/Linux, Java, Nextflow, and worker need to be available"], buttonRow([[[ "Run diagnostics", "Run diagnostics" ], "", "#/diagnostics_page"]])],
    [["GPU", "GPU"], findState(["gpu", "cuda", "openmm", "nvidia"]), ["OpenMM/CUDA 异常时可切 CPU mode", "Switch to CPU mode if OpenMM/CUDA is unavailable"], buttonRow([[[ "Repair GPU", "Repair GPU" ], "", "#/diagnostics_page"], [[ "Run CPU mode", "Run CPU mode" ], "", null]])],
    [["Storage", "Storage"], findState(["storage", "disk", "space", "write"]), ["空间不足时先清理或压缩轨迹", "Clean or compress trajectories when storage is low"], buttonRow([[[ "Open cleanup", "Open cleanup" ], "", null]])],
    [["Export", "Export"], findState(["export", "report", "package"]), ["导出位置需要可写", "Export location must be writable"], buttonRow([[[ "Open export", "Open export" ], "", "#/report_export_page"]])],
  ];
  return [
    card([["预检查分组", "Preflight groups"]], `<div class="preflight-table">${dataTable(
      [[["分组", "Group"], ["状态", "State"], ["影响", "Impact"], ["下一步", "Next step"]].map((x) => x)][0],
      rows.map(([label, state, impact, action]) => [label, statusPill(state), impact, action]),
      [["尚未执行预检查。", "Preflight has not run yet."]]
    )}</div>`),
    card([["运行资源预测", "Run resource forecast"]], runForecastCards()),
    advancedDetails(
      [["Advanced preflight details", "Advanced preflight details"]],
      dataTable(
        [["check_id", "coverage", "observed", "repair_action", "status"]],
        checks.map((check) => [valueOrDash(check.check_id), valueOrDash(check.coverage), valueOrDash(check.observed || check.description), valueOrDash(check.repair_action), valueOrDash(check.status)]),
        [["尚无高级预检查详情。", "No advanced preflight details."]]
      )
    ),
  ].join("");
}


function compactPathLabel(value) {
  const text = String(value || "");
  if (!text) {
    return pick(["Not available", "Not available"]);
  }
  const normalized = text.replaceAll("\\", "/");
  const parts = normalized.split("/").filter(Boolean);
  return parts.length > 2 ? `.../${parts.slice(-2).join("/")}` : normalized;
}

function sourceModeLabel(mode) {
  if (mode === "atomic_coordinates") {
    return ["Atomic coordinates", "Atomic coordinates"];
  }
  if (mode === "evidence_schematic") {
    return ["Evidence schematic", "Evidence schematic"];
  }
  return ["No coordinate file", "No coordinate file"];
}

function sourceModeStatus(mode) {
  if (mode === "atomic_coordinates") return "ready";
  if (mode === "evidence_schematic") return "warning";
  return "waiting";
}

function provenanceGrid(rows) {
  return `<div class="provenance-grid">${rows
    .map(([label, value, state]) => `<div class="provenance-item"><span>${pick(label)}</span><strong>${escapeHtml(pick(value))}</strong>${state ? statusPill(state) : ""}</div>`)
    .join("")}</div>`;
}

function structureSourceRows(payload, viewer) {
  const mode = viewer.coordinate_mode || "not_available";
  const artifacts = asArray(payload.artifacts);
  const evidence = asArray(payload.evidence);
  const candidate = selectedCandidate();
  return [
    [["Coordinate evidence", "Coordinate evidence"], sourceModeLabel(mode), sourceModeStatus(mode)],
    [["3D viewer", "3D viewer"], viewer.structure_url ? "NGL runtime viewer" : "Static evidence schematic", viewer.structure_url ? "ready" : "warning"],
    [["Candidate", "Candidate"], candidateKey(candidate) || "Run-level summary", candidate ? "ready" : "waiting"],
    [["Run table", "Run table"], compactPathLabel(payload.source_path), payload.source_path ? "ready" : "waiting"],
    [["Artifact links", "Artifact links"], String(asArray(viewer.artifact_links).length || artifacts.length), artifacts.length || asArray(viewer.artifact_links).length ? "ready" : "waiting"],
    [["Evidence rows", "Evidence rows"], String(evidence.length), evidence.length ? "ready" : "waiting"],
  ];
}

function mdProvenanceRows(payload, candidate, artifactRowsList, evidenceRowsList) {
  const mdRows = asArray(payload.md_rows);
  const selected = candidate || mdRows[0] || null;
  const artifactCount = artifactRowsList.length;
  const evidenceCount = evidenceRowsList.length;
  const timeBasis = mdTimeBasis(payload, selected);
  return [
    [["Data source", "Data source"], payload.artifact_backed ? "Run artifacts" : "No MD artifacts", payload.artifact_backed ? "ready" : "waiting"],
    [["Candidate", "Candidate"], candidateKey(selected) || "Run-level summary", selected ? "ready" : "waiting"],
    [["Time basis", "Time basis"], timeBasis, payload.artifact_backed ? "ready" : "waiting"],
    [["Units", "Units"], "RMSD/RMSF/drift as reported; occupancy fraction; score unitless", payload.artifact_backed ? "ready" : "waiting"],
    [["Source table", "Source table"], compactPathLabel(payload.source_path), payload.source_path ? "ready" : "waiting"],
    [["Artifact links", "Artifact links"], String(artifactCount), artifactCount ? "ready" : "waiting"],
    [["Evidence rows", "Evidence rows"], String(evidenceCount), evidenceCount ? "ready" : "waiting"],
  ];
}

function mdTimeBasis(payload, candidate) {
  return (
    firstRowValue(candidate, ["time_step", "timestep", "time_ps", "duration_ns", "frames"]) ||
    (payload.artifact_backed ? "summary table" : "not generated")
  );
}

function mdChartMeta(payload, candidate, artifactRowsList, unit) {
  const source = payload.artifact_backed ? compactPathLabel(payload.source_path || "run artifacts") : "no MD artifact for the selected run";
  const candidateLabel = candidateKey(candidate) || "Run-level summary";
  return [
    `Data source: ${source}`,
    `candidate: ${candidateLabel}`,
    `time basis: ${mdTimeBasis(payload, candidate)}`,
    `artifact links: ${artifactRowsList.length}`,
    `unit: ${unit}`,
  ].join("; ");
}

function mdSummary() {
  const payload = appState.mdSummary || {};
  const rows = payload.artifacts?.length ? artifactObjectRows(payload.artifacts) : artifactRows(["md_summary", "short_md", "molecular_dynamics", "rmsd", "rmsf", "drift", "occupancy", "trajectory", "hbond", "contact"]);
  const candidate = selectedCandidate() || (payload.md_rows || [])[0] || null;
  const evidenceRows = payload.evidence?.length ? payload.evidence : evidenceRowsForTokens(["md_stability", "md_score", "rmsd", "rmsf", "drift", "occupancy", "trajectory", "hbond", "contact"]);
  return [
    resultTabs("md"),
    card([["MD data provenance", "MD data provenance"]], provenanceGrid(mdProvenanceRows(payload, candidate, rows, evidenceRows))),
    card([["MD 图形摘要", "MD graphical summary"]], `<div class="chart-grid">
      ${miniLineChart(["RMSD", "RMSD"], [[4, 28], [18, 24], [34, 20], [52, 24], [70, 18], [94, 16]], payload.artifact_backed ? "ready" : "waiting", mdChartMeta(payload, candidate, rows, "reported RMSD"))}
      ${miniLineChart(["RMSF", "RMSF"], [[4, 30], [16, 22], [30, 32], [44, 18], [64, 26], [94, 20]], payload.artifact_backed ? "ready" : "waiting", mdChartMeta(payload, candidate, rows, "reported RMSF"))}
      ${miniLineChart(["Contact occupancy", "Contact occupancy"], [[4, 34], [18, 30], [34, 22], [52, 20], [70, 16], [94, 13]], payload.artifact_backed ? "ready" : "waiting", mdChartMeta(payload, candidate, rows, "fraction"))}
      ${miniLineChart(["H-bond occupancy", "H-bond occupancy"], [[4, 32], [16, 27], [30, 24], [48, 19], [70, 22], [94, 17]], payload.artifact_backed ? "ready" : "waiting", mdChartMeta(payload, candidate, rows, "fraction"))}
      ${miniLineChart(["Ligand drift", "Ligand drift"], [[4, 18], [20, 20], [38, 23], [56, 26], [74, 25], [94, 28]], payload.artifact_backed ? "warning" : "waiting", mdChartMeta(payload, candidate, rows, "reported drift"))}
    </div>`),
    card(
      [["MD 关键指标", "MD key metrics"]],
      dataTable(
        [[["指标", "Metric"], ["值", "Value"]].map((x) => x)][0],
        candidate
          ? rowsFromFields(candidate, [
              [["RMSD", "RMSD"], ["rmsd", "md_rmsd", "rmsd_summary"]],
              [["RMSF", "RMSF"], ["rmsf", "md_rmsf", "rmsf_summary"]],
              [["ligand drift", "ligand drift"], ["ligand_drift", "drift"]],
              [["hydrogen bonds", "hydrogen bonds"], ["hydrogen_bond_occupancy", "hbond_occupancy"]],
              [["contacts", "contacts"], ["contact_occupancy"]],
              [["score decomposition", "score decomposition"], ["md_score_decomposition", "score_decomposition"]],
            ])
          : [],
        [["选择候选后显示 MD 指标。", "Select a candidate to show MD metrics."]]
      )
    ),
    card([["MD 产物", "MD artifacts"]], dataTable(
      [[["类别", "Type"], ["状态", "State"], ["入口", "Entry"]].map((x) => x)][0],
      rows.map((row) => [friendlyArtifactType(row[0]), row[2], pick(["高级详情中查看路径", "Path in advanced details"])]),
      payload.message || [["当前运行尚未生成 MD 产物。", "The selected run has not generated MD artifacts yet."]]
    ) + advancedDetails([["Artifact details", "Artifact details"]], dataTable([["artifact_type", "path", "state"]], rows, [["无路径详情。", "No path details."]]))),
    card([["MD 证据", "MD evidence"]], userEvidenceTable(evidenceRows, [["当前运行尚未生成 MD 相关证据。", "No MD-related evidence yet."]])),
  ].join("");
}

function asArray(value) {
  return Array.isArray(value) ? value : [];
}

function viewerCountRows(viewer) {
  return [
    [["Chains", "Chains"], String(asArray(viewer.chains).length), asArray(viewer.chains).length ? "ready" : "waiting"],
    [["Residues", "Residues"], String(asArray(viewer.residues).length), asArray(viewer.residues).length ? "ready" : "waiting"],
    [["Patch residues", "Patch residues"], String(asArray(viewer.patch_residues).length), asArray(viewer.patch_residues).length ? "ready" : "waiting"],
    [["Ligand atoms", "Ligand atoms"], String(asArray(viewer.ligand_atoms).length), asArray(viewer.ligand_atoms).length ? "ready" : "waiting"],
    [["Interactions", "Interactions"], String(asArray(viewer.interaction_annotations).length), asArray(viewer.interaction_annotations).length ? "ready" : "waiting"],
  ];
}

function structureViewerGraphic(viewer) {
  if (viewer.structure_url) {
    return `<div class="structure-stage ngl-viewer-shell waiting" data-ngl-viewer data-structure-url="${escapeHtml(viewer.structure_url)}" data-structure-format="${escapeHtml(viewer.structure_format || "")}">
      <div class="ngl-stage" data-ngl-stage></div>
      <div class="ngl-status" data-ngl-status>${pick(["正在准备结构查看器", "Preparing structure viewer"])}</div>
      <div class="ngl-fallback">${evidenceSchematicGraphic(viewer)}</div>
    </div>`;
  }
  return evidenceSchematicGraphic(viewer);
}

function evidenceSchematicGraphic(viewer) {
  const chains = asArray(viewer.chains).slice(0, 4);
  const patches = asArray(viewer.patch_residues).slice(0, 28);
  const ligands = asArray(viewer.ligand_atoms).slice(0, 6);
  const interactions = asArray(viewer.interaction_annotations).slice(0, 6);
  const ready = viewer.viewer_ready ? "ready" : "waiting";
  const chainNodes = (chains.length ? chains : [{ chain_id: "target", label: "Target chain" }])
    .map((chain, index) => {
      const top = 26 + index * 14;
      return `<div class="viewer-chain" style="top:${top}%"><span>${escapeHtml(chain.chain_id || `chain_${index + 1}`)}</span><small>${escapeHtml(chain.label || chain.role || "target")}</small></div>`;
    })
    .join("");
  const patchNodes = (patches.length ? patches : [{ residue_label: "Patch appears after structure evidence" }])
    .map((patch, index) => {
      const left = 24 + ((index * 13) % 52);
      const top = 34 + ((index * 17) % 36);
      return `<span class="viewer-patch-dot" style="left:${left}%;top:${top}%" title="${escapeHtml(patch.residue_label || patch.patch_residue_id || "patch residue")}"></span>`;
    })
    .join("");
  const ligandNodes = (ligands.length ? ligands : [{ ligand_label: "Ligand" }])
    .map((ligand, index) => {
      const left = 55 + index * 5;
      const top = 48 + (index % 2) * 8;
      return `<span class="viewer-ligand-node" style="left:${left}%;top:${top}%">${escapeHtml(ligand.ligand_label || ligand.atom_id || "Ligand")}</span>`;
    })
    .join("");
  const interactionNodes = interactions
    .map((interaction) => `<li>${escapeHtml(interaction.residue_label || "contact")} <span>${escapeHtml(interaction.annotation || "contact")}</span></li>`)
    .join("");
  return `<div class="structure-stage viewer-stage ${ready}" aria-label="${pick(["Structure and patch viewer", "Structure and patch viewer"])}">
    <div class="viewer-grid"></div>
    ${chainNodes}
    <div class="viewer-pocket"></div>
    ${patchNodes}
    ${ligandNodes}
    <div class="viewer-mode">${escapeHtml(viewer.coordinate_mode || "not_available")}</div>
    <ul class="viewer-interactions">${interactionNodes || `<li>${pick(["Interactions appear after evidence load.", "Interactions appear after evidence load."])}</li>`}</ul>
  </div>`;
}

function structureViewerDetails(viewer) {
  const rows = [
    ["viewer_ready", viewer.viewer_ready ? "true" : "false"],
    ["viewer_engine", viewer.viewer_engine || "evidence_schematic"],
    ["coordinate_mode", viewer.coordinate_mode || "not_available"],
    ["chains", String(asArray(viewer.chains).length)],
    ["residues", String(asArray(viewer.residues).length)],
    ["patch_residues", String(asArray(viewer.patch_residues).length)],
    ["ligand_atoms", String(asArray(viewer.ligand_atoms).length)],
    ["interaction_annotations", String(asArray(viewer.interaction_annotations).length)],
    ["artifact_links", String(asArray(viewer.artifact_links).length)],
    ["source_note", viewer.source_note || ""],
  ];
  return dataTable([["field", "value"]], rows, [["No viewer data.", "No viewer data."]]);
}

function structureViewer() {
  const payload = appState.structureArtifacts || {};
  const viewer = payload.viewer || {};
  const rows = payload.artifacts?.length ? artifactObjectRows(payload.artifacts) : artifactRows(["structure", "patch", "docking", "pose", ".pdb", ".cif", ".mmcif"]);
  const evidenceRows = payload.evidence?.length ? payload.evidence : evidenceRowsForTokens(["structure", "patch", "docking", "pose", "residue"]);
  const viewerState = viewer.viewer_ready ? "ready" : rows.length ? "warning" : "waiting";
  return [
    resultTabs("structure"),
    card([["Structure data provenance", "Structure data provenance"]], provenanceGrid(structureSourceRows(payload, viewer))),
    `<section class="panel"><div class="panel-header"><h2>${pick(["Structure/Patch Viewer", "Structure/Patch Viewer"])}</h2>${statusPill(viewerState)}</div>
      ${structureViewerGraphic(viewer)}
      ${metricGrid(viewerCountRows(viewer))}
      <p class="viewer-note">${escapeHtml(viewer.viewer_ready ? "Structure and patch evidence is loaded for this run." : payload.message || "Structure and patch evidence appears after a run generates structure artifacts.")}</p>
      ${advancedDetails([["Viewer data details", "Viewer data details"]], structureViewerDetails(viewer))}
    </section>`,
    card([["Structure and patch artifacts", "Structure and patch artifacts"]], dataTable(
      [[["Type", "Type"], ["State", "State"], ["Entry", "Entry"]].map((x) => x)][0],
      rows.map((row) => [friendlyArtifactType(row[0]), row[2], pick(["Open details for file location.", "Open details for file location."])]),
      payload.message || [["The selected run has not generated structure, patch, or docking artifacts yet.", "The selected run has not generated structure, patch, or docking artifacts yet."]]
    ) + advancedDetails([["Artifact details", "Artifact details"]], dataTable([["artifact_type", "path", "state"]], rows, [["No path details.", "No path details."]]))),
    card([["Structure evidence", "Structure evidence"]], userEvidenceTable(evidenceRows, [["No structure-related evidence yet.", "No structure-related evidence yet."]])),
  ].join("");
}

let nglLoadPromise = null;

function loadNglLibrary() {
  if (window.NGL?.Stage) {
    return Promise.resolve(window.NGL);
  }
  if (!nglLoadPromise) {
    nglLoadPromise = new Promise((resolve, reject) => {
      const existing = document.querySelector("script[data-ngl-loader]");
      if (existing) {
        existing.addEventListener("load", () => resolve(window.NGL), { once: true });
        existing.addEventListener("error", () => reject(new Error("ngl_load_failed")), { once: true });
        return;
      }
      const script = document.createElement("script");
      script.src = "https://cdn.jsdelivr.net/npm/ngl@2.0.0-dev.39/dist/ngl.js";
      script.async = true;
      script.dataset.nglLoader = "true";
      script.addEventListener("load", () => resolve(window.NGL), { once: true });
      script.addEventListener("error", () => reject(new Error("ngl_load_failed")), { once: true });
      document.head.appendChild(script);
    });
  }
  return nglLoadPromise.then((library) => {
    if (!library?.Stage) {
      throw new Error("ngl_stage_unavailable");
    }
    return library;
  });
}

function setNglStatus(host, status, message) {
  host.classList.remove("waiting", "ready", "failed");
  host.classList.add(status);
  const statusNode = host.querySelector("[data-ngl-status]");
  if (statusNode) {
    statusNode.textContent = message;
  }
}

function hydrateStructureViewer(activeId) {
  if (activeId !== "structure_viewer_page") {
    return;
  }
  const host = document.querySelector("[data-ngl-viewer]");
  if (!host || host.dataset.hydrated === "true") {
    return;
  }
  const stageNode = host.querySelector("[data-ngl-stage]");
  const url = host.dataset.structureUrl;
  if (!stageNode || !url) {
    return;
  }
  host.dataset.hydrated = "true";
  setNglStatus(host, "waiting", pick(["正在加载结构坐标", "Loading structure coordinates"]));
  loadNglLibrary()
    .then((NGL) => {
      const stage = new NGL.Stage(stageNode, { backgroundColor: "#071014" });
      window.addEventListener("resize", () => stage.handleResize(), { passive: true });
      const extension = host.dataset.structureFormat || undefined;
      return stage.loadFile(url, extension ? { ext: extension } : {}).then((component) => {
        component.addRepresentation("cartoon", { color: "chainname", opacity: 0.86 });
        component.addRepresentation("licorice", { sele: "hetero and not water", multipleBond: "symmetric" });
        component.addRepresentation("surface", { sele: "protein", opacity: 0.18, color: "hydrophobicity" });
        component.autoView();
        setNglStatus(host, "ready", pick(["结构坐标已加载", "Structure coordinates loaded"]));
      });
    })
    .catch(() => {
      setNglStatus(host, "failed", pick(["3D 查看器暂不可用，已显示证据示意图", "3D viewer unavailable; showing evidence schematic"]));
    });
}

function actionBanner() {
  if (!appState.message) {
    return "";
  }
  return `<div class="state-block ${appState.message.kind || ""}"><h3>${pick(["操作状态", "Action State"])}</h3><p>${appState.message.text}</p></div>`;
}

function applyStaticText() {
  document.querySelectorAll("[data-i18n]").forEach((node) => {
    node.textContent = t(node.dataset.i18n);
  });
  document.documentElement.lang = appState.lang === "zh" ? "zh-CN" : "en";
  document.querySelector("[data-server-state]").textContent = t(appState.server.state || "ready").toUpperCase();
  document.querySelector("[data-top-project]").textContent = `GenoLigand ${APP_VERSION}`;
  document.querySelector("[data-top-run]").textContent =
    appState.lang === "zh"
      ? "本地工作台已就绪 / 等待项目"
      : "Local workspace ready / waiting for project";
}

function render() {
  const activeId = routeFromLocation();
  renderNav(activeId);
  applyStaticText();
  const [title, desc] = pageMeta[activeId] || pageMeta.dashboard_page;
  document.querySelector("[data-page-kicker]").textContent = `GenoLigand ${APP_VERSION}`;
  document.querySelector("[data-page-title]").textContent = pick(title);
  document.querySelector("[data-page-description]").textContent = pick(desc);
  setStatusNode(document.querySelector("[data-page-status]"), activeId === "run_monitor_page" ? "idle" : "ready");
  document.querySelector("[data-main]").innerHTML = `${actionBanner()}${workflowContextPanel(activeId)}${renderPage(activeId)}`;
  renderDetail(activeId);
  renderRuntime();
  hydrateStructureViewer(activeId);
}

async function apiRequest(path, options = {}) {
  const response = await fetch(path, {
    method: options.method || "GET",
    cache: "no-store",
    headers: { "Content-Type": "application/json" },
    body: options.body,
  });
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || `http_${response.status}`);
  }
  return payload;
}

function inputEntryKey(entry) {
  return [entry.route, entry.field || entry.label, entry.control_type || entry.type || ""].join("::");
}

async function fileSha256(file) {
  if (!window.crypto?.subtle || !file?.arrayBuffer) {
    return "";
  }
  const digest = await window.crypto.subtle.digest("SHA-256", await file.arrayBuffer());
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

async function fileBase64(file) {
  if (!file?.arrayBuffer) {
    return "";
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  const chunkSize = 0x8000;
  for (let index = 0; index < bytes.length; index += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize));
  }
  return btoa(binary);
}

async function captureVisibleInputs() {
  const activeRoute = routeFromLocation();
  const fieldNodes = Array.from(document.querySelectorAll("input, textarea, select")).filter((node) => node.type !== "file");
  const fileNodes = Array.from(document.querySelectorAll('input[type="file"]'));
  const inputs = fieldNodes.map((node) => ({
    route: activeRoute,
    field: node.dataset.field || fieldKeyFromLabel(node.getAttribute("aria-label") || node.name || node.id || ""),
    label: node.getAttribute("aria-label") || node.closest("label")?.querySelector("span")?.textContent || node.name || node.id || "",
    value: node.type === "checkbox" ? String(node.checked) : node.value || "",
    control_type: node.tagName.toLowerCase(),
  }));
  const files = (
    await Promise.all(
      fileNodes.map(async (node) =>
        Promise.all(
          Array.from(node.files || []).map(async (file) => ({
            route: activeRoute,
            field: node.dataset.field || fieldKeyFromLabel(node.getAttribute("aria-label") || ""),
            label: node.getAttribute("aria-label") || "",
            name: file.name,
            size: file.size,
            type: file.type || "application/octet-stream",
            last_modified: file.lastModified,
            sha256: await fileSha256(file),
            content_base64: await fileBase64(file),
          }))
        )
      )
    )
  ).flat();
  return { inputs, files };
}

async function mergeInputDraft(snapshot) {
  const currentSnapshot = snapshot || (await captureVisibleInputs());
  const visibleInputKeys = new Set(currentSnapshot.inputs.map(inputEntryKey));
  const visibleFileKeys = new Set(currentSnapshot.files.map(inputEntryKey));
  appState.inputDraft.inputs = appState.inputDraft.inputs.filter((entry) => !visibleInputKeys.has(inputEntryKey(entry))).concat(currentSnapshot.inputs);
  appState.inputDraft.files = appState.inputDraft.files.filter((entry) => !visibleFileKeys.has(inputEntryKey(entry))).concat(currentSnapshot.files);
}

async function collectInputManifest() {
  await mergeInputDraft();
  ensureDefaultChoices();
  return {
    saved_at: new Date().toISOString(),
    route: routeFromLocation(),
    inputs: appState.inputDraft.inputs,
    files: appState.inputDraft.files,
    advanced_parameters_collapsed: true,
    field_schema: "genoligand.webui.input_manifest.v0.9.1",
  };
}

function ensureDefaultChoices() {
  const defaults = [
    ["input_wizard_molecule_type_page", "molecule_type", "Molecule Type", "small_molecule"],
    ["input_wizard_genome_page", "species_mode", "Species mode", "preset_species"],
    ["input_wizard_search_scope_page", "search_scope", "Search scope", "conservative_search"],
    ["input_wizard_run_profile_page", "run_profile", "Run profile", "quick_environment_check"],
  ];
  for (const [route, field, label, value] of defaults) {
    const hasChoice = appState.inputDraft.inputs.some((entry) => entry.field === field && entry.control_type === "choice");
    if (!hasChoice) {
      appState.inputDraft.inputs.push({ route, field, label, value, control_type: "choice" });
    }
  }
}


function queueInputDraftMerge(renderAfter = false) {
  mergeInputDraft()
    .then(() => {
      if (renderAfter) {
        render();
      }
    })
    .catch(() => {
      setMessage("warn", "Input capture did not finish; retry when saving configuration.", "Input capture did not finish; retry when saving configuration.");
    });
}

function storeChoiceSelection(node) {
  const field = node.dataset.choiceField || "";
  const value = node.dataset.choiceValue || "";
  if (!field || !value) {
    return;
  }
  const route = routeFromLocation();
  const label = node.dataset.choiceLabel || value;
  const entry = {
    route,
    field,
    label,
    value,
    control_type: "choice",
  };
  appState.inputDraft.inputs = appState.inputDraft.inputs
    .filter((existing) => !(existing.route === route && existing.field === field && existing.control_type === "choice"))
    .concat(entry);
  setMessage("", "选择已写入当前输入配置。", "Selection was written to the current input configuration.");
}

function setMessage(kind, zh, en) {
  appState.message = { kind, text: appState.lang === "zh" ? zh : en };
}

async function refreshProjects() {
  const payload = await apiRequest("/api/user/projects");
  appState.projects = payload.projects || [];
  if (!appState.selectedProject && appState.projects.length) {
    appState.selectedProject = appState.projects[0];
  }
  if (appState.selectedProject) {
    const fresh = appState.projects.find((project) => project.project_id === appState.selectedProject.project_id);
    if (fresh) {
      appState.selectedProject = fresh;
    }
  }
  if (appState.selectedProject) {
    try {
      const preflight = await apiRequest(`/api/projects/${appState.selectedProject.project_id}/preflight`);
      appState.preflight = preflight.preflight || appState.preflight;
    } catch (_error) {
      // Project lists remain usable even if an older project has no preflight artifact yet.
    }
    await refreshProjectRuns();
  } else {
    appState.projectRuns = [];
  }
}

async function refreshProjectRuns() {
  if (!appState.selectedProject?.project_id) {
    appState.projectRuns = [];
    return;
  }
  try {
    const runs = await apiRequest(`/api/projects/${appState.selectedProject.project_id}/runs`);
    appState.projectRuns = runs.runs || [];
    if (!appState.selectedRun && appState.selectedProject.latest_run_id) {
      const latest = appState.projectRuns.find((run) => run.run_id === appState.selectedProject.latest_run_id);
      if (latest) {
        appState.selectedRun = latest;
        await refreshRunDetails(latest);
      }
    }
  } catch (_error) {
    appState.projectRuns = [];
  }
}

function requireProject() {
  if (!appState.selectedProject) {
    throw new Error(appState.lang === "zh" ? "请先创建或打开项目。" : "Create or open a project first.");
  }
  return appState.selectedProject;
}

function requireRun() {
  if (!appState.selectedRun) {
    throw new Error(appState.lang === "zh" ? "当前没有可操作的运行。" : "No run is selected.");
  }
  return appState.selectedRun;
}

async function refreshRunDetails(run = appState.selectedRun) {
  if (!run?.run_id) {
    appState.runLogs = [];
    appState.runArtifacts = null;
    appState.candidates = null;
    appState.evidence = null;
    appState.genomeContext = null;
    appState.structureArtifacts = null;
    appState.mdSummary = null;
    appState.selectedCandidateId = null;
    connectRunStream(null);
    return;
  }
  connectRunStream(run.run_id);
  try {
    const logs = await apiRequest(`/api/runs/${run.run_id}/logs`);
    appState.runLogs = logs.logs || [];
  } catch (_error) {
    appState.runLogs = [];
  }
  try {
    appState.runArtifacts = await apiRequest(`/api/runs/${run.run_id}/artifacts`);
  } catch (_error) {
    appState.runArtifacts = null;
  }
  try {
    appState.candidates = await apiRequest(`/api/runs/${run.run_id}/candidates`);
    const rows = appState.candidates?.candidates || [];
    if (!rows.some((row) => candidateKey(row) === appState.selectedCandidateId)) {
      appState.selectedCandidateId = rows.length ? candidateKey(rows[0]) : null;
    }
  } catch (_error) {
    appState.candidates = null;
    appState.selectedCandidateId = null;
  }
  try {
    appState.evidence = await apiRequest(`/api/runs/${run.run_id}/evidence`);
  } catch (_error) {
    appState.evidence = null;
  }
  try {
    appState.genomeContext = await apiRequest(`/api/runs/${run.run_id}/genome-context`);
  } catch (_error) {
    appState.genomeContext = null;
  }
  try {
    appState.structureArtifacts = await apiRequest(`/api/runs/${run.run_id}/structure`);
  } catch (_error) {
    appState.structureArtifacts = null;
  }
  try {
    appState.mdSummary = await apiRequest(`/api/runs/${run.run_id}/md-summary`);
  } catch (_error) {
    appState.mdSummary = null;
  }
}

async function refreshWorkspaceState() {
  try {
    appState.databases = await apiRequest("/api/user/databases");
  } catch (_error) {
    appState.databases = null;
  }
  try {
    appState.diagnostics = await apiRequest("/api/user/diagnostics");
  } catch (_error) {
    appState.diagnostics = null;
  }
  try {
    appState.setup = await apiRequest("/api/user/setup/status");
  } catch (_error) {
    appState.setup = null;
  }
  try {
    appState.storage = await apiRequest("/api/user/storage/policy");
  } catch (_error) {
    appState.storage = null;
  }
}

async function recordDiagnosticAction(action) {
  const payload = await apiRequest("/api/user/diagnostics/action", {
    method: "POST",
    body: JSON.stringify({ action }),
  });
  appState.diagnosticAction = payload;
  return payload;
}

async function handleAction(action, dataset = {}) {
  try {
    if (action === "candidate-prev-page") {
      appState.candidateTable.page = Math.max(1, appState.candidateTable.page - 1);
      setMessage("", "候选表已翻到上一页。", "Candidate table moved to the previous page.");
    } else if (action === "candidate-next-page") {
      appState.candidateTable.page += 1;
      setMessage("", "候选表已翻到下一页。", "Candidate table moved to the next page.");
    } else if (action === "candidate-clear-filter") {
      appState.candidateTable.query = "";
      appState.candidateTable.risk = "all";
      appState.candidateTable.page = 1;
      setMessage("", "候选表筛选已清空。", "Candidate table filters were cleared.");
    } else if (action === "candidate-pin-columns") {
      appState.candidateTable.visible = { gene: true, priority: true, risk: true, md: true, evidence: true };
      appState.candidateTable.page = 1;
      setMessage("", "关键列已固定显示。", "Key columns are visible.");
    } else if (action === "open-output-folder") {
      const exportDir = appState.exports?.export_dir || "";
      setMessage("", exportDir ? `导出目录：${exportDir}` : "请先生成导出文件。", exportDir ? `Export folder: ${exportDir}` : "Generate export files first.");
    } else if (action === "reduce-candidates") {
      setMessage("warn", "建议缩小搜索范围或提高筛选阈值后重新预检查。", "Reduce the search scope or raise screening thresholds, then rerun preflight.");
      window.location.hash = "#/input_wizard_search_scope_page";
    } else if (["repair-runtime", "repair-worker", "repair-gpu", "open-database-setup", "clean-cache", "open-export"].includes(action)) {
      const payload = await recordDiagnosticAction(action);
      if (payload.next_route?.startsWith("#/")) {
        window.location.hash = payload.next_route;
      }
      await refreshWorkspaceState();
      setMessage(action === "clean-cache" ? "warn" : "", payload.effect || "Diagnostics action recorded.", payload.effect || "Diagnostics action recorded.");
    } else if (action === "run-cpu-mode") {
      await recordDiagnosticAction("run-cpu-mode");
      setMessage("warn", "可切换到 CPU 模式；完整物理中筛前仍需确认时间和磁盘预估。", "CPU mode is available; confirm time and storage estimate before a full physical screen.");
      window.location.hash = "#/input_wizard_run_profile_page";
    } else if (action === "open-developer-mode") {
      const [debugRoutes, guides] = await Promise.all([
        apiRequest("/api/debug/routes"),
        apiRequest("/api/guides"),
      ]);
      appState.debugRoutes = debugRoutes;
      appState.guides = guides;
      appState.developerMode = true;
      setMessage("warn", "Developer Mode 已打开。这里显示复现审计和排错信息，普通科研流程不需要使用。", "Developer Mode is open. It shows reproducibility and troubleshooting details that ordinary research workflows do not need.");
      render();
    } else if (action === "close-developer-mode") {
      appState.developerMode = false;
      appState.debugRoutes = null;
      appState.guides = null;
      setMessage("", "Developer Mode 已关闭。", "Developer Mode closed.");
      render();
    } else if (action === "reduce-concurrency") {
      setMessage("warn", "建议减少并发或缩小候选集后重新启动运行。", "Reduce concurrency or candidate count before restarting the run.");
      window.location.hash = "#/input_wizard_run_profile_page";
    } else if (action === "open-cleanup") {
      setMessage("warn", "删除前请检查影响范围、复现风险和恢复可能。", "Review impact, reproducibility risk, and recovery before deletion.");
      window.location.hash = "#/settings_page";
    } else if (action === "select-candidate") {
      appState.selectedCandidateId = dataset.candidateId ? decodeURIComponent(dataset.candidateId) : null;
      setMessage("", "候选已选中。", "Candidate selected.");
    } else if (action === "select-project") {
      const projectId = dataset.projectId ? decodeURIComponent(dataset.projectId) : "";
      const project = appState.projects.find((entry) => entry.project_id === projectId);
      if (!project) {
        throw new Error(appState.lang === "zh" ? "项目未找到。" : "Project was not found.");
      }
      appState.selectedProject = project;
      appState.selectedRun = null;
      appState.runLogs = [];
      appState.runArtifacts = null;
      appState.candidates = null;
      appState.evidence = null;
      appState.genomeContext = null;
      appState.structureArtifacts = null;
      appState.mdSummary = null;
      await refreshProjectRuns();
      setMessage("", "项目已打开。", "Project opened.");
    } else if (action === "select-run") {
      const runId = dataset.runId ? decodeURIComponent(dataset.runId) : "";
      const run = appState.projectRuns.find((entry) => entry.run_id === runId);
      if (!run) {
        throw new Error(appState.lang === "zh" ? "运行记录未找到。" : "Run record was not found.");
      }
      appState.selectedRun = run;
      await refreshRunDetails(run);
      window.location.hash = "#/run_monitor_page";
      setMessage("", "运行记录已打开。", "Run record opened.");
    } else if (action === "run-example") {
      const payload = await apiRequest("/api/user/projects/run-example", { method: "POST" });
      appState.selectedProject = payload.project || null;
      appState.selectedRun = payload.run_result?.run || null;
      await refreshProjects();
      if (appState.selectedProject?.latest_run_id) {
        await refreshProjectRuns();
        appState.selectedRun = appState.projectRuns.find((entry) => entry.run_id === appState.selectedProject.latest_run_id) || appState.selectedRun;
      }
      await refreshRunDetails(appState.selectedRun);
      appState.exports = payload.run_result?.exports || appState.exports;
      window.location.hash = "#/run_monitor_page";
      setMessage("", "Demo example generated. Results are ready to review.", "Demo example generated. Results are ready to review.");
    } else if (action === "clear-examples") {
      const payload = await apiRequest("/api/user/projects/clear-examples", { method: "POST" });
      appState.selectedProject = null;
      appState.selectedRun = null;
      appState.runLogs = [];
      appState.runArtifacts = null;
      appState.candidates = null;
      appState.evidence = null;
      appState.genomeContext = null;
      appState.structureArtifacts = null;
      appState.mdSummary = null;
      await refreshProjects();
      setMessage("warn", `Cleared ${payload.deleted_project_count || 0} example projects.`, `Cleared ${payload.deleted_project_count || 0} example projects.`);
    } else if (action === "create-project" || action === "create-validation-project") {
      const path = action === "create-validation-project" ? "/api/projects/environment-validation" : "/api/projects";
      const payload = await apiRequest(path, { method: "POST" });
      appState.selectedProject = payload.project || null;
      appState.selectedRun = null;
      appState.genomeContext = null;
      appState.structureArtifacts = null;
      appState.mdSummary = null;
      await refreshProjects();
      setMessage("", "项目已保存到本地。", "Project saved locally.");
    } else if (action === "import-project-package") {
      const payload = await apiRequest("/api/user/projects/import-package", { method: "POST" });
      if (payload.status !== "imported") {
        throw new Error(appState.lang === "zh" ? "Project Package import did not complete." : "Project Package import did not complete.");
      }
      appState.selectedProject = payload.project || null;
      appState.selectedRun = null;
      appState.genomeContext = null;
      appState.structureArtifacts = null;
      appState.mdSummary = null;
      await refreshProjects();
      setMessage("", "Project Package imported.", "Project Package imported.");
    } else if (action === "open-project") {
      await refreshProjects();
      setMessage("", "已刷新本地项目列表。", "Local project list refreshed.");
    } else if (action === "copy-project") {
      const project = requireProject();
      const payload = await apiRequest(`/api/projects/${project.project_id}/copy`, { method: "POST" });
      appState.selectedProject = payload.project || appState.selectedProject;
      await refreshProjects();
      setMessage("", "项目副本已创建。", "Project copy created.");
    } else if (action === "delete-project") {
      const project = requireProject();
      await apiRequest(`/api/projects/${project.project_id}`, { method: "DELETE" });
      appState.selectedProject = null;
      appState.selectedRun = null;
      appState.genomeContext = null;
      appState.structureArtifacts = null;
      appState.mdSummary = null;
      await refreshProjects();
      setMessage("warn", "项目已从本地工作区删除。", "Project was deleted from the local workspace.");
    } else if (action === "clean-project") {
      const project = requireProject();
      await apiRequest(`/api/projects/${project.project_id}/cleanup`, { method: "POST" });
      setMessage("", "项目清理计划已生成。", "Project cleanup plan generated.");
    } else if (action === "run-preflight") {
      const project = requireProject();
      const payload = await apiRequest(`/api/projects/${project.project_id}/preflight`, { method: "POST" });
      appState.preflight = payload.preflight || null;
      await refreshProjects();
      if (payload.status === "preflight_passed") {
        setMessage("", "预检查已写入项目状态。", "Preflight state was written to the project.");
      } else {
        setMessage("warn", "预检查已完成，存在需要处理的项目。", "Preflight completed with items that need attention.");
      }
    } else if (action === "save-configuration") {
      const project = appState.selectedProject || (await apiRequest("/api/projects", { method: "POST" })).project;
      appState.selectedProject = project;
      const manifest = await collectInputManifest();
      await apiRequest(`/api/projects/${project.project_id}/inputs`, {
        method: "POST",
        body: JSON.stringify(manifest),
      });
      await refreshProjects();
      setMessage("", "配置已保存到本地项目。", "Configuration saved to the local project.");
    } else if (action === "start-workflow" || action === "submit-run") {
      await mergeInputDraft();
      if (requiresForecastConfirmation()) {
        window.location.hash = "#/input_wizard_run_profile_page";
        setMessage("warn", "Review the full physical resource estimate before launch.", "Review the full physical resource estimate before launch.");
        return;
      }
      const project = requireProject();
      const payload = await apiRequest(`/api/projects/${project.project_id}/runs`, { method: "POST" });
      appState.selectedRun = payload.run || null;
      await refreshRunDetails(appState.selectedRun);
      await refreshProjects();
      await refreshProjectRuns();
      window.location.hash = "#/run_monitor_page";
      setMessage("", "运行已创建并排队，等待本地科学后端执行。", "Run created and queued for the local scientific backend.");
    } else if (action === "cancel-run") {
      const run = requireRun();
      const payload = await apiRequest(`/api/runs/${run.run_id}/cancel`, { method: "POST" });
      appState.selectedRun = payload.run || appState.selectedRun;
      await refreshRunDetails(appState.selectedRun);
      setMessage("warn", "运行已取消。", "Run cancelled.");
    } else if (action === "resume-run" || action === "resume-last-run") {
      if (!appState.selectedRun && appState.selectedProject?.latest_run_id) {
        await refreshProjectRuns();
        appState.selectedRun = appState.projectRuns.find((entry) => entry.run_id === appState.selectedProject.latest_run_id) || null;
      }
      const run = requireRun();
      const payload = await apiRequest(`/api/runs/${run.run_id}/resume`, { method: "POST" });
      appState.selectedRun = payload.run || appState.selectedRun;
      await refreshRunDetails(appState.selectedRun);
      setMessage("", "运行已标记为可恢复。", "Run marked as resumable.");
    } else if (action === "run-diagnostics") {
      appState.diagnostics = await apiRequest("/api/user/diagnostics/run", { method: "POST" });
      setMessage("", "诊断结果已从本地服务读取。", "Diagnostics were read from the local service.");
    } else if (action === "run-setup-check") {
      appState.setup = await apiRequest("/api/user/setup/run", { method: "POST" });
      await refreshWorkspaceState();
      setMessage("", "环境检查报告已生成。", "Environment check report generated.");
    } else if (action === "prepare-full-install") {
      appState.setupInstall = await apiRequest("/api/user/setup/install", {
        method: "POST",
        body: JSON.stringify({ execute: false }),
      });
      appState.setup = await apiRequest("/api/user/setup/status");
      setMessage("", "Complete install plan is ready.", "Complete install plan is ready.");
    } else if (action === "validate-sample-project") {
      appState.setupSampleValidation = await apiRequest("/api/user/setup/validate-sample-project", { method: "POST" });
      await refreshProjects();
      const ok = appState.setupSampleValidation?.pass === true || appState.setupSampleValidation?.status === "ready";
      setMessage(
        ok ? "" : "warn",
        ok ? "Clean workflow validation completed and temporary project was removed." : "Clean workflow validation needs attention.",
        ok ? "Clean workflow validation completed and temporary project was removed." : "Clean workflow validation needs attention."
      );
    } else if (action === "validate-databases") {
      appState.databaseAction = await apiRequest("/api/databases/validate", { method: "POST" });
      await refreshWorkspaceState();
      setMessage("", "数据库配置检查已完成。", "Database setup check completed.");
    } else if (action === "build-database-indexes") {
      appState.databaseAction = await apiRequest("/api/databases/build-index", { method: "POST" });
      await refreshWorkspaceState();
      setMessage("", "数据库索引计划已生成。", "Database index plan generated.");
    } else if (action === "export-report") {
      appState.exports = await apiRequest("/api/user/reports/export", {
        method: "POST",
        body: JSON.stringify({
          project_id: appState.selectedProject?.project_id || null,
          run_id: appState.selectedRun?.run_id || appState.selectedProject?.latest_run_id || null,
          export_type: dataset.exportType || "all",
        }),
      });
      setMessage("", "导出文件已生成。", "Export files generated.");
    } else if (action === "git-status") {
      appState.github = await apiRequest("/api/github/status");
      setMessage("", "版本同步状态已读取。", "Version sync status loaded.");
    } else if (action === "git-add") {
      appState.github = await apiRequest("/api/github/add", { method: "POST" });
      setMessage("", "本地变更已加入版本准备区。", "Local changes staged for version preparation.");
    } else if (action === "git-commit") {
      appState.github = await apiRequest("/api/github/commit", { method: "POST" });
      setMessage("", "本地版本记录已准备或返回当前状态。", "Local version record prepared or current state returned.");
    } else if (action === "git-push") {
      appState.github = await apiRequest("/api/github/push", { method: "POST" });
      setMessage("", "远端同步已执行并记录结果。", "Remote sync completed and logged.");
    } else if (action === "git-verify") {
      appState.github = await apiRequest("/api/github/verify", { method: "POST" });
      setMessage("", "同步端状态已读取。", "Sync target state loaded.");
    }
  } catch (error) {
    setMessage("error", error.message, error.message);
  }
  render();
}

async function hydrateServerState() {
  try {
    const response = await fetch("/api/health", { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`health ${response.status}`);
    }
    const payload = await response.json();
    appState.server.state = payload.status === "ready" ? "ready" : "idle";
    const versionResponse = await fetch("/api/version", { cache: "no-store" });
    if (versionResponse.ok) {
      const version = await versionResponse.json();
      appState.server.version = version.version || APP_VERSION;
    }
    await refreshWorkspaceState();
    await refreshProjects();
  } catch (error) {
    appState.server.state = "failed";
  }
  render();
}

function numericCandidateValue(row, keys) {
  const raw = firstRowValue(row, keys);
  const value = Number.parseFloat(String(raw || "").replace(/[^0-9.+-eE]/g, ""));
  return Number.isFinite(value) ? value : Number.NEGATIVE_INFINITY;
}

function candidateRiskText(row) {
  return valueOrDash(rowValue(row, "risk_flags") || rowValue(row, "risk") || rowValue(row, "warning"));
}

function candidateRiskBucket(row) {
  const risk = candidateRiskText(row).toLowerCase();
  if (!risk || risk === "—" || risk === "鈥?" || risk === "none" || risk === "clear" || risk === "low") {
    return "clear";
  }
  if (risk.includes("high") || risk.includes("severe") || risk.includes("fail")) {
    return "high";
  }
  if (risk.includes("warn") || risk.includes("medium") || risk.includes("review")) {
    return "flagged";
  }
  return "flagged";
}

function candidateEvidenceCount(row) {
  return evidenceRowsForCandidate(row).length;
}

function candidateVisibleColumns() {
  const state = appState.candidateTable;
  return [
    {
      key: "rank",
      label: [["排序", "Rank"], ["Rank", "Rank"]][appState.lang === "zh" ? 0 : 1],
      fixed: true,
      value: (row) => escapeHtml(valueOrDash(rowValue(row, "rank"))),
    },
    {
      key: "candidate",
      label: [["候选", "Candidate"], ["Candidate", "Candidate"]][appState.lang === "zh" ? 0 : 1],
      fixed: true,
      value: (row) => {
        const key = candidateKey(row);
        return `<button type="button" data-action="select-candidate" data-candidate-id="${encodeURIComponent(key)}">${escapeHtml(valueOrDash(key))}</button>`;
      },
    },
    {
      key: "gene",
      label: [["基因", "Gene"], ["Gene", "Gene"]][appState.lang === "zh" ? 0 : 1],
      visible: state.visible.gene,
      value: (row) => escapeHtml(valueOrDash(rowValue(row, "gene_name") || rowValue(row, "gene"))),
    },
    {
      key: "priority",
      label: [["优先级", "Priority"], ["Priority", "Priority"]][appState.lang === "zh" ? 0 : 1],
      visible: state.visible.priority,
      value: (row) => escapeHtml(valueOrDash(firstRowValue(row, ["final_priority_score", "priority_score", "score"]))),
    },
    {
      key: "risk",
      label: [["风险", "Risk"], ["Risk", "Risk"]][appState.lang === "zh" ? 0 : 1],
      visible: state.visible.risk,
      value: (row) => escapeHtml(candidateRiskText(row)),
    },
    {
      key: "md",
      label: [["MD 支持", "MD support"], ["MD support", "MD support"]][appState.lang === "zh" ? 0 : 1],
      visible: state.visible.md,
      value: (row) => escapeHtml(valueOrDash(rowValue(row, "md_stability_score"))),
    },
    {
      key: "evidence",
      label: [["证据", "Evidence"], ["Evidence", "Evidence"]][appState.lang === "zh" ? 0 : 1],
      visible: state.visible.evidence,
      value: (row) => String(candidateEvidenceCount(row)),
    },
  ].filter((column) => column.fixed || column.visible);
}

function candidateFilteredRows() {
  const rows = candidateRows().slice();
  const state = appState.candidateTable;
  const query = state.query.trim().toLowerCase();
  const filtered = rows.filter((row) => {
    const riskBucket = candidateRiskBucket(row);
    const riskMatches =
      state.risk === "all" ||
      (state.risk === "clear" && riskBucket === "clear") ||
      (state.risk === "flagged" && riskBucket !== "clear") ||
      (state.risk === "high" && riskBucket === "high");
    const queryMatches = !query || Object.values(row || {}).join(" ").toLowerCase().includes(query);
    return riskMatches && queryMatches;
  });
  const sorters = {
    priority_desc: (a, b) => numericCandidateValue(b, ["final_priority_score", "priority_score", "score"]) - numericCandidateValue(a, ["final_priority_score", "priority_score", "score"]),
    md_desc: (a, b) => numericCandidateValue(b, ["md_stability_score"]) - numericCandidateValue(a, ["md_stability_score"]),
    rank_asc: (a, b) => numericCandidateValue(a, ["rank"]) - numericCandidateValue(b, ["rank"]),
    evidence_desc: (a, b) => candidateEvidenceCount(b) - candidateEvidenceCount(a),
    risk_first: (a, b) => (candidateRiskBucket(a) === "clear" ? 1 : 0) - (candidateRiskBucket(b) === "clear" ? 1 : 0),
  };
  filtered.sort(sorters[state.sort] || sorters.priority_desc);
  return filtered;
}

function candidateTableControls(totalRows, filteredRows) {
  const state = appState.candidateTable;
  const columnControls = ["gene", "priority", "risk", "md", "evidence"]
    .map((key) => {
      const labels = {
        gene: ["基因", "Gene"],
        priority: ["优先级", "Priority"],
        risk: ["风险", "Risk"],
        md: ["MD", "MD"],
        evidence: ["证据", "Evidence"],
      };
      return `<label class="check-control"><input type="checkbox" data-candidate-column="${key}" ${state.visible[key] ? "checked" : ""}>${pick(labels[key])}</label>`;
    })
    .join("");
  return `<div class="candidate-controls">
    <label><span>${pick(["搜索", "Search"])}</span><input type="search" data-candidate-query value="${escapeHtml(state.query)}" aria-label="${pick(["输入候选、基因或分数", "Candidate, gene, or score"])}"></label>
    <label><span>${pick(["风险筛选", "Risk filter"])}</span><select data-candidate-risk>
      <option value="all" ${state.risk === "all" ? "selected" : ""}>${pick(["全部", "All"])}</option>
      <option value="flagged" ${state.risk === "flagged" ? "selected" : ""}>${pick(["有风险标记", "Flagged"])}</option>
      <option value="high" ${state.risk === "high" ? "selected" : ""}>${pick(["高风险优先", "High risk"])}</option>
      <option value="clear" ${state.risk === "clear" ? "selected" : ""}>${pick(["无明显风险", "Clear"])}</option>
    </select></label>
    <label><span>${pick(["排序", "Sort"])}</span><select data-candidate-sort>
      <option value="priority_desc" ${state.sort === "priority_desc" ? "selected" : ""}>${pick(["优先级最高", "Priority high to low"])}</option>
      <option value="md_desc" ${state.sort === "md_desc" ? "selected" : ""}>${pick(["MD 支持最高", "MD support high to low"])}</option>
      <option value="evidence_desc" ${state.sort === "evidence_desc" ? "selected" : ""}>${pick(["证据最多", "Most evidence"])}</option>
      <option value="risk_first" ${state.sort === "risk_first" ? "selected" : ""}>${pick(["风险标记优先", "Flagged first"])}</option>
      <option value="rank_asc" ${state.sort === "rank_asc" ? "selected" : ""}>${pick(["原始排序", "Original rank"])}</option>
    </select></label>
    <label><span>${pick(["每页", "Rows"])}</span><select data-candidate-page-size>
      ${[10, 25, 50].map((size) => `<option value="${size}" ${state.pageSize === size ? "selected" : ""}>${size}</option>`).join("")}
    </select></label>
    <div class="column-controls"><span>${pick(["列", "Columns"])}</span>${columnControls}</div>
    <div class="table-result-count">${filteredRows} / ${totalRows} ${pick(["个候选", "candidates"])}</div>
  </div>`;
}

function candidateTableMarkup(filteredRows) {
  const state = appState.candidateTable;
  const totalPages = Math.max(1, Math.ceil(filteredRows.length / state.pageSize));
  if (state.page > totalPages) {
    state.page = totalPages;
  }
  const start = (state.page - 1) * state.pageSize;
  const pageRows = filteredRows.slice(start, start + state.pageSize);
  const columns = candidateVisibleColumns();
  const header = columns.map((column) => `<th class="${column.fixed ? "sticky-col" : ""}">${pick(column.label)}</th>`).join("");
  const body = pageRows.length
    ? pageRows
        .map((row) => `<tr>${columns.map((column) => `<td class="${column.fixed ? "sticky-col" : ""}">${column.value(row)}</td>`).join("")}</tr>`)
        .join("")
    : `<tr><td colspan="${columns.length}" class="empty-cell">${pick(["当前筛选没有匹配候选。", "No candidates match the current filters."])}</td></tr>`;
  const pager = `<div class="table-pager">
    <button type="button" data-action="candidate-prev-page" ${state.page <= 1 ? "disabled" : ""}>${pick(["上一页", "Previous"])}</button>
    <span>${state.page} / ${totalPages}</span>
    <button type="button" data-action="candidate-next-page" ${state.page >= totalPages ? "disabled" : ""}>${pick(["下一页", "Next"])}</button>
    <button type="button" data-action="candidate-clear-filter">${pick(["清空筛选", "Clear filters"])}</button>
  </div>`;
  return `<div class="data-table-wrap candidate-table-wrap"><table class="data-table candidate-table"><thead><tr>${header}</tr></thead><tbody>${body}</tbody></table></div>${pager}`;
}

function candidateRanking() {
  const payload = appState.candidates || {};
  const rows = candidateRows();
  const filtered = candidateFilteredRows();
  const selected = selectedCandidate();
  const riskCount = rows.filter((row) => candidateRiskBucket(row) !== "clear").length;
  const mdCount = rows.filter((row) => rowValue(row, "md_stability_score")).length;
  const selector = rows.length
    ? `<div class="candidate-selector">${filtered
        .slice(0, 24)
        .map((row) => {
          const key = candidateKey(row);
          const selectedClass = key === candidateKey(selected) ? " selected" : "";
          return `<button class="candidate-chip${selectedClass}" type="button" data-action="select-candidate" data-candidate-id="${encodeURIComponent(key)}">${escapeHtml(valueOrDash(rowValue(row, "rank")))} ${escapeHtml(key)}</button>`;
        })
        .join("")}</div>`
    : "";
  return [
    card(
      [["候选摘要", "Candidate Summary"]],
      metricGrid([
        [["Top candidates", "Top candidates"], String(rows.length || 0), rows.length ? "ready" : "waiting"],
        [["风险分布", "Risk distribution"], rows.length ? `${riskCount} ${pick(["有标记", "flagged"])}` : pick(["等待结果", "Waiting"]), rows.length ? (riskCount ? "warning" : "ready") : "waiting"],
        [["证据完整性", "Evidence completeness"], appState.evidence?.evidence?.length ? `${appState.evidence.evidence.length} ${pick(["条证据", "rows"])}` : pick(["等待证据", "Waiting"]), appState.evidence?.evidence?.length ? "ready" : "waiting"],
        [["MD 支持", "MD support"], rows.length ? `${mdCount} / ${rows.length}` : pick(["等待结果", "Waiting"]), mdCount ? "ready" : "waiting"],
        [["运行模式", "Run mode"], appState.selectedRun?.run_profile || pick(["未选择", "Not selected"]), appState.selectedRun ? "ready" : "waiting"],
      ])
    ),
    card(
      [["候选排序", "Candidate Ranking"]],
      candidateTableControls(rows.length, filtered.length) +
        buttonRow([[[ "固定关键列", "Pin key columns" ], "", null], [[ "导出候选表", "Export candidate table" ], "", "#/report_export_page"]]) +
        candidateTableMarkup(filtered) +
        selector +
        advancedDetails(
          [["Full candidate table", "Full candidate table"]],
          (payload.source_path ? `<p>source_path=${escapeHtml(payload.source_path)}</p>` : "") +
            dataTable(candidateHeaders, rows.map((row) => candidateFieldKeys.map((key) => valueOrDash(rowValue(row, key)))), payload.message || t("noCandidates"))
        )
    ),
  ].join("");
}

function liveStatusKey() {
  if (appState.liveStream.status === "connected" || appState.liveStream.status === "updated") {
    return "ready";
  }
  if (appState.liveStream.status === "connecting") {
    return "running";
  }
  if (appState.liveStream.status === "refreshing") {
    return "warning";
  }
  return "waiting";
}

function appendLiveMessage(message) {
  const text = String(message || "").trim();
  if (!text) {
    return;
  }
  appState.liveStream.messages = appState.liveStream.messages.concat(text).slice(-24);
  appState.liveStream.lastEvent = new Date().toLocaleTimeString();
}

function applyLiveStreamMessage(raw) {
  let payload = null;
  try {
    payload = JSON.parse(raw);
  } catch (_error) {
    appendLiveMessage(raw);
    return;
  }
  if (payload.run && (!appState.selectedRun || payload.run.run_id === appState.selectedRun.run_id)) {
    appState.selectedRun = Object.assign({}, appState.selectedRun || {}, payload.run);
    appState.liveStream.state = payload.run.state || appState.liveStream.state;
    appState.liveStream.stage = payload.run.current_stage || appState.liveStream.stage;
    if (Array.isArray(payload.run.log_tail)) {
      appState.runLogs = payload.run.log_tail;
      appState.liveStream.messages = payload.run.log_tail.slice(-24);
    }
  }
  if (payload.resource) {
    appState.liveStream.resource = payload.resource;
  }
  if (Array.isArray(payload.messages)) {
    appState.runLogs = payload.messages;
    appState.liveStream.messages = payload.messages.slice(-24);
  }
  if (payload.message) {
    appendLiveMessage(payload.message);
  } else if (payload.type) {
    appendLiveMessage(`${payload.type}: ${payload.stage || payload.state || "updated"}`);
  }
}

function connectRunStream(runId) {
  if (appState.liveStream.runId === runId && appState.liveSocket && appState.liveSocket.readyState <= 1) {
    return;
  }
  if (appState.liveSocket) {
    try {
      appState.liveSocket.close();
    } catch (_error) {
      // The socket may already be closed by the local service.
    }
    appState.liveSocket = null;
  }
  appState.liveStream.runId = runId;
  appState.liveStream.messages = [];
  if (!runId || !("WebSocket" in window)) {
    appState.liveStream.status = runId ? "refreshing" : "disconnected";
    return;
  }
  const protocol = window.location.protocol === "https:" ? "wss" : "ws";
  const socket = new WebSocket(`${protocol}://${window.location.host}/ws/runs/${encodeURIComponent(runId)}`);
  appState.liveSocket = socket;
  appState.liveStream.status = "connecting";
  socket.addEventListener("open", () => {
    appState.liveStream.status = "connected";
    if (routeFromLocation() === "run_monitor_page") {
      render();
    }
  });
  socket.addEventListener("message", (event) => {
    applyLiveStreamMessage(event.data);
    appState.liveStream.status = "updated";
    if (routeFromLocation() === "run_monitor_page") {
      render();
    }
  });
  socket.addEventListener("close", () => {
    if (appState.liveSocket === socket) {
      appState.liveStream.status = appState.liveStream.messages.length ? "updated" : "refreshing";
      appState.liveSocket = null;
      if (routeFromLocation() === "run_monitor_page") {
        render();
      }
    }
  });
  socket.addEventListener("error", () => {
    appState.liveStream.status = "refreshing";
    appendLiveMessage(pick(["实时连接不可用，已保留刷新模式。", "Live connection unavailable; refresh mode remains available."]));
    if (routeFromLocation() === "run_monitor_page") {
      render();
    }
  });
}

function liveRunPanel(run, resource) {
  const live = appState.liveStream;
  const liveResource = live.resource || resource || {};
  const liveMessages = live.messages.length ? live.messages : appState.runLogs;
  return card(
    [["实时更新", "Live Updates"]],
    dataTable(
      [[["项目", "Item"], ["当前值", "Current value"]].map((x) => x)][0],
      [
        [["连接", "Connection"], statusPill(liveStatusKey(), live.status || "waiting")],
        [["运行状态", "Run state"], run ? statusPill(run.state || live.state || "created", friendlyState(run.state || live.state || "created")) : statusPill("waiting")],
        [["当前阶段", "Current stage"], run?.current_stage || live.stage || pick(["等待运行", "Waiting for run"])],
        [["最近更新", "Last update"], live.lastEvent || pick(["等待", "Waiting"])],
        ["CPU", liveResource.cpu || pick(["等待运行", "Waiting for run"])],
        ["GPU", liveResource.gpu || pick(["等待检测", "Waiting for check"])],
        ["VRAM", liveResource.vram || pick(["等待检测", "Waiting for check"])],
        [["磁盘", "Disk"], liveResource.disk || pick(["等待检测", "Waiting for check"])],
      ],
      [["尚无实时更新。", "No live updates yet."]]
    ) +
      advancedDetails(
        [["Advanced live messages", "Advanced live messages"]],
        `<pre class="log-block">${escapeHtml(valueOrDash(liveMessages))}</pre>`
      )
  );
}

function resourceRecoveryRows(resource = {}) {
  return [
    [
      "CPU",
      resource.cpu || pick(["等待运行", "Waiting for run"]),
      pick(["正常时无需操作；若接近限制，减少候选数量。", "No action when normal; reduce candidates near the limit."]),
      `<button type="button" data-action="reduce-candidates">${pick(["减少候选", "Reduce candidates"])}</button>`,
    ],
    [
      "GPU",
      resource.gpu || pick(["等待检测", "Waiting for check"]),
      pick(["GPU 异常时可切 CPU 模式或减少候选。", "Switch to CPU mode or reduce candidates when GPU is abnormal."]),
      `<button type="button" data-action="run-cpu-mode">${pick(["Run CPU mode", "Run CPU mode"])}</button>`,
    ],
    [
      "VRAM",
      resource.vram || pick(["等待检测", "Waiting for check"]),
      pick(["显存接近限制时减少并发或缩小候选集。", "Reduce concurrency or candidate count when VRAM is near the limit."]),
      `<button type="button" data-action="reduce-concurrency">${pick(["减少并发", "Reduce concurrency"])}</button>`,
    ],
    [
      pick(["磁盘", "Disk"]),
      resource.disk || pick(["等待检测", "Waiting for check"]),
      pick(["空间不足时清理临时文件、压缩轨迹或导出后删除中间产物。", "Clean temporary files, compress trajectories, or delete intermediates after export when disk is low."]),
      `<button type="button" data-action="open-cleanup">${pick(["打开清理", "Open cleanup"])}</button>`,
    ],
  ];
}

function resourceRecoveryPanel(resource = {}) {
  return card(
    [["资源建议", "Resource guidance"]],
    dataTable(
      [[["资源", "Resource"], ["状态", "State"], ["建议", "Guidance"], ["操作", "Action"]].map((x) => x)][0],
      resourceRecoveryRows(resource),
      [["等待资源状态。", "Waiting for resource status."]]
    )
  );
}

function friendlyCompletedSteps(steps) {
  const labels = {
    project_loaded: ["项目已读取", "Project loaded"],
    input_manifest_loaded: ["输入已读取", "Input loaded"],
    preflight_checked: ["预检查完成", "Preflight checked"],
    run_manifest_written: ["运行记录已写入", "Run record written"],
    production_launch_1_completed: ["科学运行步骤 1 完成", "Scientific run step 1 completed"],
    production_launch_2_completed: ["科学运行步骤 2 完成", "Scientific run step 2 completed"],
  };
  const list = Array.isArray(steps) ? steps : String(steps || "").split(",");
  const friendly = list
    .map((step) => String(step || "").trim())
    .filter(Boolean)
    .map((step) => pick(labels[step] || [step.replaceAll("_", " "), step.replaceAll("_", " ")]));
  return friendly.length ? friendly.join(", ") : "—";
}

function runMonitor() {
  const run = appState.selectedRun;
  const resource = run?.resource_status || {};
  const completedSteps = run?.completed_steps || [];
  const logTail = appState.runLogs.length ? appState.runLogs : run?.log_tail || [];
  const artifactItems = appState.runArtifacts?.items || run?.artifact_index?.items || [];
  return [
    card([["Pipeline", "Pipeline"]], pipelineStepper("run_monitor_page")),
    card([["资源预测", "Resource forecast"]], runForecastCards()),
    liveRunPanel(run, resource),
    resourceRecoveryPanel(resource),
    card(
      [["当前运行", "Current Run"]],
      dataTable(
        [[["项目项", "Item"], ["当前值", "Current value"]].map((x) => x)][0],
        run
          ? [
              [["运行", "Run"], friendlyRunName(run)],
              [["状态", "State"], statusPill(run.state || "created", friendlyState(run.state || "created"))],
              [["当前阶段", "Current stage"], run.current_stage || pick(["等待开始", "Waiting to start"])],
              [["已完成步骤", "Completed steps"], friendlyCompletedSteps(completedSteps)],
              [["失败原因", "Failure reason"], run.failure_reason || pick(["无", "None"])],
              [["恢复状态", "Resume state"], run.resumable ? pick(["可恢复", "Resumable"]) : pick(["正常", "Normal"])],
            ]
          : [
              [["运行", "Run"], pick(["尚未选择运行", "No run selected"])],
              [["状态", "State"], statusPill("waiting")],
              [["当前阶段", "Current stage"], pick(["等待提交", "Waiting to submit"])],
              [["已完成步骤", "Completed steps"], "—"],
              [["失败原因", "Failure reason"], "—"],
              [["恢复状态", "Resume state"], pick(["从项目页开始", "Start from Projects"])],
            ],
        [[t("noActiveRun"), t("noActiveRun")]]
      ) +
        buttonRow([[[ "提交运行", "Submit run" ], "primary", null], [[ "取消运行", "Cancel run" ], "danger", null], [[ "恢复中断运行", "Resume interrupted run" ], "", null], [[ "查看结果", "View results" ], "", "#/candidate_ranking_page"]]) +
        advancedDetails(
          [["Advanced run details", "Advanced run details"]],
          dataTable(
            [["run_id", "nextflow_run_id", "command", "output_dir"]],
            run ? [[valueOrDash(run.run_id), valueOrDash(run.nextflow_run_id), valueOrDash(run.current_command), valueOrDash(run.output_dir)]] : [],
            [["无运行详情。", "No run details."]]
          )
        )
    ),
    card(
      [["产物树和日志", "Artifact tree and logs"]],
      dataTable(
        [[["类别", "Type"], ["状态", "State"], ["入口", "Entry"]].map((x) => x)][0],
        artifactItems.map((item) => [friendlyArtifactType(item.artifact_type), item.exists ? statusPill("ready") : statusPill("waiting"), item.exists ? pick(["可打开", "Available"]) : pick(["等待生成", "Pending"])]),
        [["尚未创建运行产物。", "No run artifacts have been created yet."]]
      ) +
        advancedDetails(
          [["Advanced logs and artifact tree", "Advanced logs and artifact tree"]],
          `<pre class="log-block">${escapeHtml(valueOrDash(logTail))}</pre>` +
            dataTable(
              [["artifact_type", "path", "exists"]],
              artifactItems.map((item) => [valueOrDash(item.artifact_type), valueOrDash(item.path), valueOrDash(item.exists)]),
              [["无产物路径。", "No artifact paths."]]
            )
        )
    ),
  ].join("");
}

function exportDefinitions() {
  return [
    ["research_report", ["研究报告", "Research report"], ["摘要、输入、数据库版本、候选排序、关键图、结构/MD 证据、限制和建议", "Summary, inputs, database versions, ranking, figures, structure/MD evidence, limitations, and suggestions"]],
    ["candidate_table", ["候选表", "Candidate table"], ["用于排序、筛选和后续实验设计", "For ranking, filtering, and follow-up experimental design"]],
    ["reproducibility_package", ["复现包", "Reproducibility package"], ["参数、版本、输入和可复现说明", "Parameters, versions, inputs, and reproducibility notes"]],
    ["full_artifact_archive", ["完整产物归档", "Full artifact archive"], ["完整运行产物索引，体积可能较大", "Full run artifact index; may be large"]],
    ["support_bundle", ["支持包", "Support bundle"], ["用于排错、重检和支持沟通，不复制大型数据库", "Troubleshooting, recheck, and support records without large database copies"]],
    ["project_package", ["Project Package", "Project Package"], ["用于只读审阅、演示模式、移动或重新打开项目", "For read-only review, demo mode, moving, or reopening a project"]],
    ["backup", ["Backup", "Backup"], ["项目级备份清单，不复制大型数据库", "Project-level backup list without copying large databases"]],
  ];
}

function exportFileForKey(files, key) {
  const aliases = {
    research_report: ["research_report", "html_report", "markdown_report"],
    candidate_table: ["candidate_table", "candidate_table_tsv", "candidate_table_csv"],
    reproducibility_package: ["reproducibility_package"],
    full_artifact_archive: ["full_artifact_archive", "artifact_bundle"],
    support_bundle: ["support_bundle", "diagnostics_report"],
    project_package: ["project_package"],
    backup: ["backup"],
  }[key] || [key];
  return (files || []).find((file) => aliases.some((alias) => String(file.export_type || "").includes(alias)));
}

function reportExport() {
  const payload = appState.exports || {};
  const rows = exportDefinitions().map(([key, label, purpose]) => {
    const found = exportFileForKey(payload.files || [], key);
    return [
      pick(label),
      pick(purpose),
      found?.exists ? statusPill("ready") : statusPill(payload.status || "waiting"),
      found ? valueOrDash(found.bytes) : "—",
      `<button type="button" data-action="export-report" data-export-type="${key}">${pick(["生成", "Generate"])}</button>`,
    ];
  });
  return card(
    [["导出", "Export"]],
    dataTable(
      [[["用途", "Use"], ["内容", "Contents"], ["状态", "State"], ["大小", "Size"], ["操作", "Action"]].map((x) => x)][0],
      rows,
      [["尚未生成导出文件。", "No export files have been generated yet."]]
    ) +
      buttonRow([[[ "生成导出文件", "Generate export files" ], "primary", null], [[ "打开输出目录", "Open output folder" ], "", null]]) +
      advancedDetails(
        [["Advanced export details", "Advanced export details"]],
        dataTable(
          [["export_type", "path", "bytes", "exists"]],
          (payload.files || []).map((file) => [valueOrDash(file.export_type), valueOrDash(file.path), valueOrDash(file.bytes), valueOrDash(file.exists)]),
          [["暂无导出详情。", "No export details."]]
        )
      )
  );
}

function evidenceGroupForRow(row) {
  const text = `${rowValue(row, "report_section")} ${rowValue(row, "report_item")} ${rowValue(row, "source_file")}`.toLowerCase();
  if (text.includes("input")) return "Input";
  if (text.includes("genome") || text.includes("orf") || text.includes("variant") || text.includes("cds")) return "Genome";
  if (text.includes("structure") || text.includes("patch") || text.includes("pose")) return "Structure";
  if (text.includes("dock") || text.includes("vina")) return "Docking";
  if (text.includes("md") || text.includes("rmsd") || text.includes("trajectory") || text.includes("drift")) return "MD";
  if (text.includes("database") || text.includes("chembl") || text.includes("bindingdb") || text.includes("uniprot")) return "Database";
  return "Report";
}

function evidenceImportance(row) {
  const text = `${rowValue(row, "report_item")} ${rowValue(row, "report_section")}`.toLowerCase();
  if (text.includes("md") || text.includes("score") || text.includes("ranking")) {
    return pick(["高", "High"]);
  }
  if (text.includes("database") || text.includes("input")) {
    return pick(["中", "Medium"]);
  }
  return pick(["参考", "Context"]);
}

function evidenceConfidence(row) {
  const state = String(rowValue(row, "validation_state") || rowValue(row, "status") || "").toLowerCase();
  if (state.includes("validated") || state.includes("rust") || state.includes("pass")) {
    return pick(["已由 GenoLigand 校验", "Validated by GenoLigand"]);
  }
  if (state.includes("warning") || state.includes("partial")) {
    return pick(["部分校验", "Partially checked"]);
  }
  return pick(["待校验", "Pending"]);
}

function evidenceStatusKey(row) {
  const state = String(rowValue(row, "validation_state") || rowValue(row, "status") || "").toLowerCase();
  if (state.includes("validated") || state.includes("rust") || state.includes("pass")) return "ready";
  if (state.includes("warn") || state.includes("partial")) return "warning";
  if (state.includes("fail") || state.includes("error")) return "failed";
  return "waiting";
}

function evidenceStatusLabel(row) {
  const key = evidenceStatusKey(row);
  if (key === "ready") return pick(["已校验", "Validated"]);
  if (key === "warning") return pick(["需复核", "Review"]);
  if (key === "failed") return pick(["未通过", "Failed"]);
  return pick(["待校验", "Pending"]);
}

function evidenceSourceLabel(row) {
  const source = String(rowValue(row, "source_file") || rowValue(row, "report_section") || "").toLowerCase();
  if (source.includes("score")) return pick(["分数分解", "Score decomposition"]);
  if (source.includes("md")) return pick(["短 MD 摘要", "Short-MD summary"]);
  if (source.includes("candidate")) return pick(["候选排序", "Candidate ranking"]);
  if (source.includes("structure") || source.includes("patch")) return pick(["结构证据", "Structure evidence"]);
  if (source.includes("evidence")) return pick(["证据账本", "Evidence ledger"]);
  return pick(["运行产物", "Run artifact"]);
}

function evidenceWhy(row, group) {
  const text = `${rowValue(row, "report_item")} ${rowValue(row, "report_section")}`.toLowerCase();
  if (group === "MD" || text.includes("md")) {
    return pick(["说明候选在短程物理筛选中的稳定性支持。", "Shows whether short-MD supports the candidate stability."]);
  }
  if (text.includes("score")) {
    return pick(["解释候选排序分数来自哪些计算证据。", "Explains which computed evidence supports the ranking score."]);
  }
  if (group === "Structure") {
    return pick(["帮助判断结构位点和相互作用是否支持该候选。", "Helps judge whether structural contacts support the candidate."]);
  }
  if (group === "Database") {
    return pick(["说明数据库证据是否足以支撑本轮解释。", "Shows whether database evidence supports this interpretation."]);
  }
  return pick(["帮助判断该候选是否值得后续实验。", "Helps decide whether the candidate merits follow-up."]);
}

function evidenceUserRows(rows, group) {
  return dataTable(
    [[["证据", "Evidence"], ["重要性", "Importance"], ["可信度", "Confidence"], ["来源", "Source"], ["状态", "State"], ["Why this matters", "Why this matters"]].map((x) => x)][0],
    rows.slice(0, 3).map((row) => [
      valueOrDash(rowValue(row, "report_item")).replaceAll("_", " "),
      evidenceImportance(row),
      evidenceConfidence(row),
      evidenceSourceLabel(row),
      statusPill(evidenceStatusKey(row), evidenceStatusLabel(row)),
      evidenceWhy(row, group),
    ]),
    [["该分组暂无证据。", "No evidence in this group yet."]]
  );
}

function evidenceInternalDetails(rows) {
  return dataTable(
    [["report_item", "source_file", "manifest", "schema", "tool_version", "artifact_hash", "validation_state", "report_section"]],
    rows.map((row) => evidenceFieldKeys.map((key) => valueOrDash(rowValue(row, key)))),
    [["无内部证据字段。", "No internal evidence fields."]]
  );
}

function evidenceLedger() {
  const payload = appState.evidence || {};
  const rows = payload.evidence || [];
  const groups = ["Input", "Genome", "Structure", "Docking", "MD", "Database", "Report"];
  const cards = groups
    .map((group) => {
      const groupRows = rows.filter((row) => evidenceGroupForRow(row) === group);
      return `<article class="evidence-card">
        <div class="evidence-card-head"><h3>${group}</h3>${statusPill(groupRows.length ? "ready" : "waiting", groupRows.length ? pick(["有证据", "Evidence"]) : pick(["等待", "Pending"]))}</div>
        <p>${groupRows.length ? pick(["关键证据已连接到当前运行。", "Key evidence is linked to the current run."]) : pick(["完成运行后显示该组证据。", "Appears after a run completes."])}</p>
        ${evidenceUserRows(groupRows, group)}
        ${advancedDetails([["高级详情", "Advanced details"]], evidenceInternalDetails(groupRows))}
      </article>`;
    })
    .join("");
  return card(
    [["证据卡", "Evidence Cards"]],
    `<div class="evidence-grid">${cards}</div>` +
      advancedDetails(
        [["高级详情", "Advanced details"]],
        (payload.source_path ? `<p>source_path=${escapeHtml(payload.source_path)}</p>` : "") + evidenceInternalDetails(rows)
      )
  );
}

function workflowFacts() {
  const projectReady = Boolean(appState.selectedProject?.project_id);
  const inputReady = projectReady && (appState.inputDraft.inputs.length > 0 || appState.inputDraft.files.length > 0 || appState.selectedProject?.project_status !== "created");
  const resources = appState.databases?.resources || [];
  const databaseReady = resources.some((resource) => typeof resource !== "string" && ["indexed", "data_present_index_not_detected"].includes(resource.index_state));
  const scopeReady = appState.inputDraft.inputs.some((entry) => ["search_scope", "run_profile", "database_profile"].includes(entry.field));
  const preflightReady = appState.selectedProject?.project_status === "preflight_passed" || Boolean(appState.selectedRun);
  const runReady = Boolean(appState.selectedRun);
  const resultsReady = candidateRows().length > 0;
  const evidenceReady = (appState.evidence?.evidence || []).length > 0;
  const exportReady = (appState.exports?.files || []).some((file) => file.exists);
  return {
    project: projectReady,
    input: inputReady,
    database: databaseReady,
    scope: scopeReady,
    preflight: preflightReady,
    run: runReady,
    results: resultsReady,
    evidence: evidenceReady,
    export: exportReady,
  };
}

function workflowStepStatus(key, currentKey, facts) {
  if (facts[key]) {
    return "done";
  }
  if (key === currentKey) {
    return "active";
  }
  return "todo";
}

function pipelineStepper(activeId) {
  const current = pagePipelineStep[activeId] || "project";
  const facts = workflowFacts();
  return `<div class="pipeline-strip">${pipelineSteps
    .map(([key, title, href], index) => {
      const state = workflowStepStatus(key, current, facts);
      return `<a class="pipeline-step ${state}" href="#/${href}"><span>${index + 1}</span><strong>${pick(title)}</strong></a>`;
    })
    .join("")}</div>`;
}

function workflowBlockerFor(key, facts) {
  if (!facts.project) return [["未打开项目", "No project selected"], "project_create_page"];
  if (key === "input" && !facts.input) return [["还没有可运行输入", "No runnable input yet"], "input_wizard_small_molecule_page"];
  if (key === "database" && !facts.database) return [["数据库尚未检查", "Database not checked"], "database_registry_page"];
  if (key === "scope" && !facts.scope) return [["搜索范围尚未确认", "Search scope not confirmed"], "input_wizard_search_scope_page"];
  if (key === "preflight" && !facts.preflight) return [["预检查尚未通过", "Preflight not passed"], "input_wizard_preflight_page"];
  if (key === "run" && !facts.run) return [["尚未提交运行", "No run submitted"], "run_monitor_page"];
  if (key === "results" && !facts.results) return [["结果尚未生成", "Results not generated"], "run_monitor_page"];
  if (key === "evidence" && !facts.evidence) return [["证据尚未生成", "Evidence not generated"], "evidence_ledger_page"];
  if (key === "export" && !facts.export) return [["尚未生成导出文件", "Exports not generated"], "report_export_page"];
  return [["无阻塞", "No blocker"], null];
}

function nextWorkflowStep(currentKey, facts) {
  const currentIndex = Math.max(0, pipelineSteps.findIndex(([key]) => key === currentKey));
  const firstIncomplete = pipelineSteps.find(([key]) => !facts[key]);
  if (firstIncomplete) {
    return firstIncomplete;
  }
  return pipelineSteps[Math.min(currentIndex + 1, pipelineSteps.length - 1)] || pipelineSteps[pipelineSteps.length - 1];
}

function activeStepSummary(activeId) {
  const key = pagePipelineStep[activeId] || "project";
  const facts = workflowFacts();
  const current = pipelineSteps.find(([step]) => step === key) || pipelineSteps[0];
  const next = nextWorkflowStep(key, facts);
  const [blocker, blockerHref] = workflowBlockerFor(key, facts);
  const complete = facts[key];
  return {
    current,
    next,
    completion: complete ? ["已完成", "Complete"] : ["待完成", "Pending"],
    blocker,
    blockerHref,
    resume: appState.selectedRun ? ["回到当前运行", "Resume current run"] : ["从项目开始", "Start from project"],
  };
}

function workflowContextPanel(activeId) {
  if (activeId === "dashboard_page") {
    return "";
  }
  const summary = activeStepSummary(activeId);
  const blockerOk = summary.blocker[1] === "No blocker";
  const blockerAction = summary.blockerHref
    ? `<a class="button subtle" href="#/${summary.blockerHref}">${pick(["修复阻塞", "Fix blocker"])}</a>`
    : `<span class="muted">${pick(["无需修复", "No fix needed"])}</span>`;
  const resumeHref = appState.selectedRun ? "run_monitor_page" : "project_create_page";
  return `<section class="workflow-context">
    ${pipelineStepper(activeId)}
    <div class="workflow-context-grid">
      <div><span>${pick(["当前步骤", "Current step"])}</span><strong>${pick(summary.current[1])}</strong>${statusPill(summary.completion[1] === "Complete" ? "ready" : "waiting")}</div>
      <div><span>${pick(["完成状态", "Completion"])}</span><strong>${pick(summary.completion)}</strong></div>
      <div><span>${pick(["阻塞原因", "Blocker"])}</span><strong>${pick(summary.blocker)}</strong>${statusPill(blockerOk ? "ready" : "warning")}</div>
      <div><span>${pick(["恢复入口", "Resume entry"])}</span><strong>${pick(summary.resume)}</strong></div>
    </div>
      <div><span>${pick(["修复入口", "Fix blocker"])}</span>${blockerAction}</div>
      <div><span>${pick(["下一步入口", "Next step action"])}</span><a class="button primary" href="#/${summary.next[2]}">${pick(summary.next[1])}</a></div>
      <div><span>${pick(["恢复操作", "Resume action"])}</span><a class="button" href="#/${resumeHref}">${pick(summary.resume)}</a></div>
    ${buttonRow([[summary.next[1], "primary", `#/${summary.next[2]}`], [["查看运行", "Open run"], "", "#/run_monitor_page"], [["打开输出目录", "Open output folder"], "", "#/report_export_page"]])}
  </section>`;
}

function renderRuntime() {
  const facts = workflowFacts();
  const activeId = routeFromLocation();
  const currentKey = pagePipelineStep[activeId] || "project";
  document.querySelector("[data-runtime-timeline]").innerHTML = pipelineSteps
    .map(([key, label], index) => {
      const state = facts[key] ? "ready" : key === currentKey ? "running" : "waiting";
      return `<div class="timeline-item"><span>${String(index + 1).padStart(2, "0")}</span><strong>${pick(label)}</strong>${statusPill(state, friendlyState(state))}</div>`;
    })
    .join("");
  document.querySelector("[data-runtime-log]").textContent = appState.selectedRun
    ? [
        `${pick(["当前运行", "Current run"])}: ${friendlyRunName(appState.selectedRun)}`,
        `${pick(["状态", "State"])}: ${friendlyState(appState.selectedRun.state)}`,
        `${pick(["阶段", "Stage"])}: ${appState.selectedRun.current_stage || pick(["等待开始", "Waiting to start"])}`,
        `${pick(["下一步", "Next"])}: ${appState.selectedRun.state === "completed" ? pick(["查看结果或导出", "View results or export"]) : pick(["查看运行监控", "Open Run Monitor"])}`,
      ].join("\n")
    : appState.lang === "zh"
    ? "GenoLigand 0.9.1 本地工作台已就绪。\n当前浏览器会话没有绑定运行。\n请创建或打开项目，完成预检查后启动任务。"
    : "GenoLigand 0.9.1 local console is ready.\nNo active run is attached to this browser session.\nCreate or open a project, complete preflight, then start a workflow.";
}

function renderDetail(activeId) {
  const title = pageMeta[activeId]?.[0] || pageMeta.dashboard_page[0];
  if (activeId === "dashboard_page") {
    document.querySelector("[data-detail]").innerHTML = `
      <h2>${pick(["工作台", "Dashboard"])}</h2>
      <div class="detail-grid">
        <div class="detail-card"><span class="detail-label">${pick(["项目", "Projects"])}</span><strong class="detail-value">${appState.projects.length}</strong>${statusPill(appState.projects.length ? "ready" : "waiting")}</div>
        <div class="detail-card"><span class="detail-label">${pick(["最近结果", "Recent results"])}</span><strong class="detail-value">${appState.projectRuns.length}</strong>${statusPill(appState.projectRuns.length ? "ready" : "waiting")}</div>
        <div class="detail-card"><span class="detail-label">${pick(["本地工作台", "Local workspace"])}</span><strong class="detail-value">${pick(["就绪", "Ready"])}</strong>${statusPill("ready")}</div>
        <div class="detail-card"><span class="detail-label">${pick(["界面", "Interface"])}</span><strong class="detail-value">${pick(["暗色", "Dark"])}</strong>${statusPill("ready")}</div>
      </div>
    `;
    return;
  }
  const summary = activeStepSummary(activeId);
  const blockerOk = summary.blocker[1] === "No blocker";
  document.querySelector("[data-detail]").innerHTML = `
    <h2>${pick(["流水线", "Pipeline"])}</h2>
    ${pipelineStepper(activeId)}
    <h2>${pick(["当前步骤", "Current Step"])}</h2>
    <div class="detail-grid">
      <div class="detail-card"><span class="detail-label">${pick(["页面", "Page"])}</span><strong class="detail-value">${pick(title)}</strong>${statusPill("ready")}</div>
      <div class="detail-card"><span class="detail-label">${pick(["完成状态", "Completion"])}</span><strong class="detail-value">${pick(summary.completion)}</strong>${statusPill(summary.completion[1] === "Complete" ? "ready" : "waiting")}</div>
      <div class="detail-card"><span class="detail-label">${pick(["阻塞原因", "Blocker"])}</span><strong class="detail-value">${pick(summary.blocker)}</strong>${statusPill(blockerOk ? "ready" : "warning")}</div>
      <div class="detail-card"><span class="detail-label">${pick(["恢复入口", "Resume entry"])}</span><strong class="detail-value">${pick(summary.resume)}</strong><a class="button" href="#/run_monitor_page">${pick(["打开", "Open"])}</a></div>
    </div>
    <h2>${pick(["下一步", "Next Step"])}</h2>
    ${buttonRow([[summary.next[1], "primary", `#/${summary.next[2]}`], [["打开输出目录", "Open output folder"], "", "#/report_export_page"]])}
    ${advancedDetails(
      [["高级详情", "Advanced details"]],
      `<ul class="artifact-tree"><li>${escapeHtml(pick(["页面", "Page"]))}: ${escapeHtml(pick(title))}</li><li>${escapeHtml(pick(["流水线步骤", "Pipeline step"]))}: ${escapeHtml(pick(summary.current[1]))}</li></ul>`
    )}
  `;
}

function bindEvents() {
  window.addEventListener("hashchange", render);
  document.addEventListener("click", (event) => {
    const langToggle = event.target.closest("[data-lang-toggle]");
    if (langToggle) {
      appState.lang = appState.lang === "zh" ? "en" : "zh";
      saveLanguage(appState.lang);
      render();
      return;
    }
    const actionButton = event.target.closest("[data-action]");
    if (actionButton) {
      handleAction(actionButton.dataset.action, actionButton.dataset);
      return;
    }
    const choiceButton = event.target.closest("[data-choice-field]");
    if (choiceButton) {
      storeChoiceSelection(choiceButton);
      render();
      return;
    }
    const peptide = event.target.closest("[data-insert-peptide]");
    if (!peptide) {
      return;
    }
    const textAreaNode = document.querySelector("textarea");
    if (textAreaNode) {
      textAreaNode.value = peptide.dataset.insertPeptide;
      queueInputDraftMerge(true);
    }
  });
  document.addEventListener("input", (event) => {
    if (event.target.matches("[data-candidate-query]")) {
      appState.candidateTable.query = event.target.value || "";
      appState.candidateTable.page = 1;
      render();
      return;
    }
    if (event.target.matches("input, textarea, select")) {
      queueInputDraftMerge(true);
    }
  });
  document.addEventListener("change", (event) => {
    if (event.target.matches("[data-candidate-risk]")) {
      appState.candidateTable.risk = event.target.value || "all";
      appState.candidateTable.page = 1;
      render();
      return;
    }
    if (event.target.matches("[data-candidate-sort]")) {
      appState.candidateTable.sort = event.target.value || "priority_desc";
      appState.candidateTable.page = 1;
      render();
      return;
    }
    if (event.target.matches("[data-candidate-page-size]")) {
      appState.candidateTable.pageSize = Number(event.target.value) || 10;
      appState.candidateTable.page = 1;
      render();
      return;
    }
    if (event.target.matches("[data-candidate-column]")) {
      const key = event.target.dataset.candidateColumn;
      if (key) {
        appState.candidateTable.visible[key] = event.target.checked;
        render();
        return;
      }
    }
    if (event.target.matches("input, textarea, select")) {
      queueInputDraftMerge(true);
    }
  });
}

bindEvents();
render();
hydrateServerState();
