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
  ["dashboard_page", "01", ["本地工作台", "Dashboard"], ["项目、运行和资源总览", "Projects, runs, and resources"]],
  ["project_create_page", "02", ["项目管理", "Projects"], ["创建、打开、复制、删除、清理和继续运行", "Create, open, copy, delete, clean, and resume"]],
  ["input_wizard_project_basics_page", "03", ["输入向导", "Input Wizard"], ["面向科研工作者的分步配置", "Guided setup for research users"]],
  ["run_monitor_page", "04", ["运行监控", "Run Monitor"], ["队列、日志、资源和恢复状态", "Queue, logs, resources, and resume state"]],
  ["candidate_ranking_page", "05", ["候选排序", "Candidates"], ["真实计算结果进入后显示", "Shown after a computed run"]],
  ["database_registry_page", "06", ["数据库", "Databases"], ["路径、版本、索引和修复动作", "Paths, versions, indexes, and repairs"]],
  ["diagnostics_page", "07", ["环境诊断", "Diagnostics"], ["运行前检查与修复建议", "Pre-run checks and repair actions"]],
  ["github_sync_page", "08", ["GitHub 同步", "GitHub Sync"], ["本地状态、提交、推送和远端验证", "Local state, commit, push, and remote verification"]],
  ["report_export_page", "09", ["报告导出", "Exports"], ["报告、表格、清单和归档包", "Reports, tables, manifests, and bundles"]],
  ["settings_page", "10", ["设置", "Settings"], ["语言、工作区和服务连接", "Language, workspace, and service connection"]],
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
  input_wizard_database_profile_page: [["数据库配置", "Database Profile"], ["选择数据库 profile，检查路径、版本、hash、索引和空间。", "Select a database profile and inspect paths, versions, hashes, indexes, and storage."]],
  input_wizard_run_profile_page: [["运行模式", "Run Mode"], ["按计算成本、GPU、结构证据和 MD 摘要选择运行方式。", "Select a mode by cost, GPU need, structure evidence, and MD summary requirements."]],
  input_wizard_preflight_page: [["预检查", "Preflight"], ["启动前检查输入、数据库、权限、GPU、Nextflow、Git 和 GitHub。", "Check input, databases, permissions, GPU, Nextflow, Git, and GitHub before launch."]],
  input_wizard_review_start_page: [["确认并启动", "Review and Start"], ["确认配置后由 gls-server 提交本地工作流。", "Review configuration before gls-server submits the local workflow."]],
  run_monitor_page: [["运行监控", "Run Monitor"], ["查看阶段、命令、日志尾部、资源、输出目录、失败原因和恢复状态。", "Inspect stage, command, log tail, resources, output directory, failure reason, and resume state."]],
  candidate_ranking_page: [["候选排序", "Candidate Ranking"], ["真实计算完成后展示候选、分数和风险标记。", "Displays candidates, scores, and risk flags after a computed run."]],
  candidate_detail_page: [["候选详情", "Candidate Detail"], ["选择候选后展示来源、ORF、证据、风险和实验建议。", "Select a candidate to inspect source, ORF, evidence, risks, and experimental suggestions."]],
  genome_orf_context_page: [["Genome/ORF Context", "Genome/ORF Context"], ["显示候选来自 CDS、UTR、lncRNA、内含子、反义链等上下文。", "Shows whether candidates originate from CDS, UTR, lncRNA, intron, antisense, and related contexts."]],
  structure_viewer_page: [["结构与 Patch 查看", "Structure and Patch Viewer"], ["查看候选结构、binding patch、pose、关键残基和局部相互作用。", "Inspect candidate structure, binding patch, pose, key residues, and local interactions."]],
  md_summary_page: [["MD 摘要", "MD Summary"], ["查看 RMSD、RMSF、drift、occupancy、persistence 和分数分解。", "Inspect RMSD, RMSF, drift, occupancy, persistence, and score decomposition."]],
  evidence_ledger_page: [["证据链", "Evidence Ledger"], ["每个分数、结论和报告段落都映射到原始文件、manifest、schema、版本和 hash。", "Each score, conclusion, and report section maps to source files, manifests, schemas, versions, and hashes."]],
  database_registry_page: [["数据库管理", "Database Registry"], ["管理基因组、注释、转录本、BioLiP2、UniProt、ChEMBL/BindingDB 和结构缓存。", "Manage genome, annotation, transcript, BioLiP2, UniProt, ChEMBL/BindingDB, and structure cache."]],
  diagnostics_page: [["环境诊断", "Diagnostics"], ["覆盖 WSL2、Linux、Rust、gls-cli、gls-server、Nextflow、Java、Python worker、OpenMM、CUDA、数据库、Git 和 GitHub。", "Covers WSL2, Linux, Rust, gls-cli, gls-server, Nextflow, Java, Python workers, OpenMM, CUDA, databases, Git, and GitHub."]],
  settings_page: [["设置", "Settings"], ["正式版只提供暗色界面，可切换中文和 English。", "The production console is dark-only and supports Chinese and English."]],
  github_sync_page: [["GitHub 同步", "GitHub Sync"], ["查看分支、commit、远端、推送日志和远端验证结果。", "Inspect branch, commit, remote, push log, and remote verification."]],
  report_export_page: [["报告导出", "Report Export"], ["导出 HTML、Markdown、CSV/TSV、JSON、artifact bundle、manifest、版本清单、诊断报告和 GitHub push report。", "Export HTML, Markdown, CSV/TSV, JSON, artifact bundle, manifests, tool versions, diagnostics, and GitHub push report."]],
  error_state_page: [["错误状态", "Error State"], ["出现错误时显示原因、影响范围、修复动作和可恢复路径。", "Shows cause, impact, repair action, and recovery path when an error occurs."]],
  empty_state_page: [["空状态", "Empty State"], ["无项目、无运行或无结果时不显示虚构内容。", "No invented content is shown when projects, runs, or results are absent."]],
  loading_state_page: [["加载状态", "Loading State"], ["数据加载时保持页面稳定并显示当前请求。", "Keeps layout stable while data is loading and shows the active request."]],
};

const labels = {
  zh: {
    brandSubtitle: "本地科研工作台",
    backendNote: "本地 gls-server 提供服务",
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
    backendNote: "Served by local gls-server",
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
  "GET /api/databases",
  "GET /api/diagnostics",
  "GET /api/guides",
  "POST /api/github/push",
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
  databases: null,
  guides: null,
  github: null,
  exports: null,
  preflight: null,
  runLogs: [],
  runArtifacts: null,
  candidates: null,
  evidence: null,
  projectRuns: [],
  selectedCandidateId: null,
  inputDraft: { inputs: [], files: [] },
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
  const all = [...routes.map((item) => item[0]), ...wizardPages, ...extraPages];
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
    .map(([label, value, hint, type = "text"]) => {
      const field = fieldKeyFromLabel(label);
      return `<label><span>${pick(label)}</span><input type="${type}" value="${pick(value)}" aria-label="${pick(label)}" data-field="${field}"><small>${pick(hint)}</small></label>`;
    })
    .join("")}</div>`;
}

function textArea(label, value, hint) {
  const field = fieldKeyFromLabel(label);
  return `<label class="wide-field"><span>${pick(label)}</span><textarea rows="5" aria-label="${pick(label)}" data-field="${field}">${pick(value)}</textarea><small>${pick(hint)}</small></label>`;
}

function fileDrop(label, hint, acceptText) {
  const labelText = pick(label);
  const field = fieldKeyFromLabel(label);
  return `<label class="file-drop"><strong>${labelText}</strong><span>${pick(hint)}</span><small>${pick(acceptText)}</small><input type="file" aria-label="${labelText}" data-field="${field}"></label>`;
}

function optionTiles(items) {
  return `<div class="option-tiles">${items
    .map(([title, text, status]) => `<button class="option-tile ${status === "configured" ? "selected" : ""}" type="button"><strong>${pick(title)}</strong><span>${pick(text)}</span>${status ? statusPill(status) : ""}</button>`)
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
    "Create project": "create-project",
    "Create environment validation project template": "create-validation-project",
    "Select project": "select-project",
    "Select run": "select-run",
    "Save configuration": "save-configuration",
    "Open project": "open-project",
    "Copy project": "copy-project",
    "Delete project": "delete-project",
    "Clean project": "clean-project",
    "Resume last run": "resume-last-run",
    "Run diagnostics": "run-diagnostics",
    "Validate database profile": "validate-databases",
    "Build database indexes": "build-database-indexes",
    "Run preflight": "run-preflight",
    "Start workflow": "start-workflow",
    "Submit run": "submit-run",
    "Cancel run": "cancel-run",
    "Resume interrupted run": "resume-run",
    "Export candidate table": "export-report",
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
  return [
    card(
      [["系统状态", "System State"]],
      metricGrid([
        [["本地服务", "Local service"], ["gls-server", "gls-server"], "ready"],
        [["版本", "Version"], [`GenoLigand ${APP_VERSION}`, `GenoLigand ${APP_VERSION}`], "ready"],
        [["活动运行", "Active run"], [t("noActiveRun"), t("noActiveRun")], "idle"],
        [["项目工作区", "Project workspace"], [["等待选择", "Waiting for selection"], ["Waiting for selection", "Waiting for selection"]][appState.lang === "zh" ? 0 : 1], "waiting"],
      ]),
      `<a class="button primary" href="#/project_create_page">${pick(["创建或打开项目", "Create or open project"])}</a>`
    ),
    `<div class="dashboard-run-grid">
      ${card(
        [["正式工作流", "Production Workflow"]],
        dataTable(
          [[["步骤", "Step"], ["职责", "Owner"], ["状态", "State"], ["产物或动作", "Artifact or action"]].map((x) => x)][0],
          [
            [[["项目配置", "Project configuration"], "gls-server", statusPill("waiting"), ["保存输入、数据库、运行状态和报告路径", "Persist inputs, databases, run state, and report paths"]]],
            [[["科学执行", "Scientific execution"], "gls-cli", statusPill("waiting"), ["由本地命令启动 Nextflow 和科学引擎", "Starts Nextflow and scientific engines through local commands"]]],
            [[["结果浏览", "Result browsing"], "gls-server", statusPill("waiting"), ["候选、结构、MD 和证据链在真实产物生成后显示", "Candidates, structures, MD, and evidence appear after artifacts exist"]]],
            [[["代码同步", "Code sync"], "Git", statusPill("optional"), ["用户确认后执行本地提交和远端推送", "Runs local commit and remote push after user confirmation"]]],
          ].map((row) => row[0]),
          [["尚无运行记录。", "No run records yet."]]
        )
      )}
      ${card(
        [[t("validationProject"), t("validationProject")]],
        `<p>${t("validationProjectText")}</p>${buttonRow([[[ "创建环境验证项目模板", "Create environment validation project template" ], "", null], [[ "运行环境诊断", "Run diagnostics" ], "primary", "#/diagnostics_page"]])}`
      )}
    </div>`,
    card(
      [["入口能力", "Console Capabilities"]],
      optionTiles([
        [["项目生命周期", "Project lifecycle"], ["创建、打开、复制、删除、清理、继续上次运行和查看历史摘要。", "Create, open, copy, delete, clean, resume, and inspect run history."], "ready"],
        [["输入向导", "Input wizard"], ["覆盖小分子、多肽、蛋白质配体、已有复合物、自定义物种和搜索范围。", "Covers small molecules, peptides, protein ligands, existing complexes, custom species, and search scope."], "ready"],
        [["结果与证据", "Results and evidence"], ["排序表、候选详情、Genome/ORF、结构、MD 和 evidence ledger 使用真实产物。", "Ranking, details, Genome/ORF, structure, MD, and evidence ledger use computed artifacts."], "ready"],
      ])
    ),
  ].join("");
}

function projectRows() {
  return appState.projects.map((project) => [
    `<button type="button" data-action="select-project" data-project-id="${encodeURIComponent(project.project_id || "")}">${pick(["打开", "Open"])}</button>`,
    project.project_id || "",
    project.project_name || "",
    statusPill(project.project_status || "created"),
    project.latest_run_id || "",
    (project.report_paths || []).join(", "),
  ]);
}

function projectListCard() {
  return card(
    [["本地项目", "Local Projects"]],
    dataTable(
      [[["操作", "Action"], ["项目 ID", "Project ID"], ["名称", "Name"], ["状态", "State"], ["最近运行", "Latest run"], ["报告路径", "Report paths"]].map((x) => x)][0],
      projectRows(),
      [["尚无本地项目。点击创建项目后会写入本地项目 manifest。", "No local project yet. Creating a project writes a local project manifest."]]
    ) +
      (appState.selectedProject
        ? `<p class="mono">${pick(["当前项目", "Selected project"])}: ${appState.selectedProject.project_id}</p>`
        : "")
  );
}

function projectRunHistoryCard() {
  const rows = appState.projectRuns.map((run) => [
    `<button type="button" data-action="select-run" data-run-id="${encodeURIComponent(run.run_id || "")}">${pick(["查看", "View"])}</button>`,
    run.run_id || "",
    statusPill(run.state || "created"),
    run.current_stage || "",
    run.nextflow_run_id || "",
    run.resumable ? "true" : "false",
    run.output_dir || "",
  ]);
  return card(
    [["历史运行摘要", "Run History Summary"]],
    dataTable(
      [[["操作", "Action"], ["Run ID", "Run ID"], ["状态", "State"], ["当前阶段", "Current stage"], ["Nextflow run ID", "Nextflow run ID"], ["可恢复", "Resumable"], ["输出目录", "Output directory"]].map((x) => x)][0],
      rows,
      [["当前项目尚无运行记录。", "The selected project has no run records yet."]]
    ) +
      buttonRow([[[ "继续上次运行", "Resume last run" ], "", null], [[ "打开运行监控", "Open run monitor" ], "primary", "#/run_monitor_page"]])
  );
}

function projectPage() {
  return [
    projectListCard(),
    projectRunHistoryCard(),
    card(
      [["项目生命周期", "Project Lifecycle"]],
      formGrid([
        [["项目名称", "Project name"], ["", ""], ["用于本地目录和报告标题", "Used for local directory and report title"]],
        [["工作区路径", "Workspace path"], ["", ""], ["选择已有目录或创建新目录", "Choose an existing directory or create a new one"]],
        [["研究对象", "Research object"], ["", ""], ["简要描述输入配体、物种和目标范围", "Briefly describe ligand input, organism, and scope"]],
        [["远端仓库", "Remote repository"], ["origin", "origin"], ["仅在 GitHub 同步页执行推送", "Push is only triggered from GitHub Sync"]],
      ]) +
        buttonRow([
          [["创建项目", "Create project"], "primary", null],
          [["打开项目", "Open project"], "", null],
          [["复制项目", "Copy project"], "", null],
          [["删除项目", "Delete project"], "", null],
          [["清理项目", "Clean project"], "", null],
          [["继续上次运行", "Resume last run"], "", "#/run_monitor_page"],
        ])
    ),
    card(
      [["项目保存内容", "Persisted Project State"]],
      dataTable(
        [[["类别", "Category"], ["保存内容", "Saved state"], ["状态", "State"]].map((x) => x)][0],
        [
          [[["输入", "Inputs"], ["分子、物种、基因组、搜索范围和高级参数", "Molecule, species, genome, scope, and advanced parameters"], statusPill("waiting")]],
          [[["数据库", "Databases"], ["profile、路径、版本、hash、索引和缺失项", "profile, paths, versions, hashes, indexes, and missing items"], statusPill("waiting")]],
          [[["运行", "Runs"], ["created/preflight_passed/queued/running/failed/completed/cancelled/resumable/pushed_to_github", "created/preflight_passed/queued/running/failed/completed/cancelled/resumable/pushed_to_github"], statusPill("waiting")]],
          [[["结果", "Results"], ["产物索引、报告路径、导出包和证据链", "artifact index, report paths, bundles, and evidence ledger"], statusPill("waiting")]],
        ].map((row) => row[0]),
        [["尚未创建项目。", "No project has been created yet."]]
      )
    ),
  ].join("");
}

function wizardPage(id) {
  const body = {
    input_wizard_project_basics_page: [
      formGrid([
        [["项目名称", "Project name"], ["", ""], ["显示在运行、报告和 GitHub 同步记录中", "Shown in runs, reports, and GitHub sync records"]],
        [["负责人或团队", "Owner or team"], ["", ""], ["用于报告元数据", "Used for report metadata"]],
        [["工作区目录", "Workspace directory"], ["", ""], ["本地可写路径", "Local writable path"]],
        [["报告边界", "Report boundary"], ["优先级排序假设，不代表实验验证", "Prioritization hypothesis, not experimental validation"], ["自动写入导出报告", "Written to exported reports"]],
      ]),
      textArea([["研究问题", "Research question"]], [["", ""]], [["用普通科研语言说明研究对象和预期用途", "Describe the research object and intended use in research language"]]),
    ].join(""),
    input_wizard_molecule_type_page: optionTiles([
      [["小分子", "Small molecule"], ["SMILES、SDF、MOL2、PDB、名称或 CAS 辅助解析。", "SMILES, SDF, MOL2, PDB, name, or CAS-assisted resolution."], "configured"],
      [["多肽", "Peptide"], ["单字母序列、FASTA、长度、非法字符、非标准氨基酸和修饰信息。", "One-letter sequence, FASTA, length, invalid characters, non-standard residues, and modifications."], "ready"],
      [["蛋白质配体", "Protein ligand"], ["FASTA、PDB/mmCIF、预测结构和链选择。", "FASTA, PDB/mmCIF, predicted structure, and chain selection."], "ready"],
      [["已有复合物", "Existing complex"], ["晶体结构、冷冻电镜、预测复合物或对接复合物。", "Crystal, cryo-EM, predicted complex, or docked complex."], "ready"],
    ]),
    input_wizard_small_molecule_page: [
      textArea([["SMILES", "SMILES"]], [["", ""]], [["可直接粘贴；系统会检查格式、价态、三维结构和构象生成需求", "Paste directly; the system checks format, valence, 3D structure, and conformer needs"]]),
      fileDrop([["上传结构文件", "Upload structure file"]], [["拖入或选择 SDF/MOL2/PDB 文件", "Drop or choose SDF/MOL2/PDB file"]], [["支持本地文件；hash 会写入项目 manifest", "Local files are supported; hashes are written to the project manifest"]]),
      formGrid([
        [["化合物名称", "Compound name"], ["", ""], ["辅助解析，不替代结构检查", "Assisted resolution, not a substitute for structure checks"]],
        [["CAS 号", "CAS number"], ["", ""], ["可选字段", "Optional field"]],
      ]),
    ].join(""),
    input_wizard_peptide_page: [
      textArea([["多肽序列", "Peptide sequence"]], [["", ""]], [["粘贴单字母氨基酸序列；系统会检查非法字符、非标准氨基酸、长度和修饰", "Paste one-letter amino-acid sequence; the system checks invalid characters, non-standard residues, length, and modifications"]]),
      `<div class="chip-row">
        <button type="button" data-insert-peptide="IKVAV">IKVAV</button>
        <button type="button" data-insert-peptide="YIGSR">YIGSR</button>
        <button type="button" data-insert-peptide="RGD">RGD</button>
      </div>`,
      fileDrop([["上传 FASTA", "Upload FASTA"]], [["选择 .fa / .fasta 文件", "Choose .fa / .fasta file"]], [["文件名、bytes 和 hash 会记录到输入 manifest", "Filename, bytes, and hash are recorded in the input manifest"]]),
    ].join(""),
    input_wizard_protein_page: [
      textArea([["FASTA 序列", "FASTA sequence"]], [["", ""]], [["可作为蛋白质配体输入；结构缺口会在预检查中报告", "Can be used as protein-ligand input; structural gaps are reported during preflight"]]),
      fileDrop([["上传 PDB/mmCIF 或预测结构", "Upload PDB/mmCIF or predicted structure"]], [["支持链选择、残基编号和结构可读性检查", "Supports chain selection, residue numbering, and readability checks"]], [["PDB, CIF, mmCIF", "PDB, CIF, mmCIF"]]),
      formGrid([
        [["配体链 ID", "Ligand chain ID"], ["", ""], ["留空则在预检查中选择", "Leave empty to choose during preflight"]],
        [["残基编号范围", "Residue range"], ["", ""], ["可选", "Optional"]],
      ]),
    ].join(""),
    input_wizard_complex_page: [
      fileDrop([["上传 receptor-ligand complex", "Upload receptor-ligand complex"]], [["指定配体链、小分子 residue name 或结合区域", "Specify ligand chain, small-molecule residue name, or binding region"]], [["PDB, mmCIF, CIF", "PDB, mmCIF, CIF"]]),
      formGrid([
        [["配体链", "Ligand chain"], ["", ""], ["蛋白、多肽或核酸链", "Protein, peptide, or nucleic-acid chain"]],
        [["小分子 residue name", "Small-molecule residue name"], ["", ""], ["如结构中存在 HETATM ligand", "If the structure contains a HETATM ligand"]],
        [["结合区域", "Binding region"], ["", ""], ["可使用链和残基编号描述", "Use chain and residue numbering"]],
        [["结构来源", "Structure source"], ["", ""], ["晶体、冷冻电镜、预测或对接复合物", "Crystal, cryo-EM, predicted, or docked complex"]],
      ]),
    ].join(""),
    input_wizard_genome_page: [
      optionTiles([
        [["预设物种", "Preset species"], ["选择已配置的 genome/annotation/transcript profile。", "Choose a configured genome/annotation/transcript profile."], "configured"],
        [["自定义物种", "Custom species"], ["上传 genome FASTA、GTF/GFF3、transcript FASTA 和可选 VCF。", "Upload genome FASTA, GTF/GFF3, transcript FASTA, and optional VCF."], "ready"],
      ]),
      formGrid([
        [["物种", "Species"], ["Homo sapiens", "Homo sapiens"], ["可改为自定义物种名称", "Can be changed to a custom species name"]],
        [["基因组版本", "Genome version"], ["GRCh38", "GRCh38"], ["写入版本字段", "Written to version metadata"]],
      ]),
      fileDrop([["自定义文件", "Custom files"]], [["genome FASTA / GTF-GFF3 / transcript FASTA / VCF", "genome FASTA / GTF-GFF3 / transcript FASTA / VCF"]], [["系统检查路径、索引、hash 和版本状态", "The system checks paths, indexes, hashes, and versions"]]),
    ].join(""),
    input_wizard_search_scope_page: optionTiles([
      [["保守搜索", "Conservative search"], ["面向已知 CDS 和高可信注释。", "Known CDS and high-confidence annotation."], "configured"],
      [["扩展转录本搜索", "Expanded transcript search"], ["面向非经典转录本 ORF。", "Non-canonical transcript ORFs."], "ready"],
      [["全基因组发现搜索", "Whole-genome discovery search"], ["六框翻译候选空间，计算量最高。", "Six-frame translated candidate space, highest compute cost."], "ready"],
    ]),
    input_wizard_database_profile_page: databaseRegistry(),
    input_wizard_run_profile_page: optionTiles([
      [["快速环境自检", "Quick environment check"], ["低成本；验证服务、数据库路径、OpenMM/CUDA 和 Nextflow 可用性。", "Low cost; verifies service, database paths, OpenMM/CUDA, and Nextflow."], "configured"],
      [["标准靶点筛选", "Standard target screening"], ["生成候选排序和基础证据链。", "Produces candidate ranking and base evidence ledger."], "ready"],
      [["结构证据增强筛选", "Structure-enhanced screening"], ["增加结构、binding patch 和 docking evidence。", "Adds structure, binding patch, and docking evidence."], "ready"],
      [["物理中筛完整运行", "Physical mid-screen full run"], ["需要 GPU；增加 short-MD 摘要和稳定性评分。", "Requires GPU; adds short-MD summary and stability scoring."], "ready"],
    ]),
    input_wizard_preflight_page: preflightTable(),
    input_wizard_review_start_page: [
      card([["启动摘要", "Launch Summary"]], metricGrid([
        [["项目", "Project"], [["待保存", "Not saved"], ["Not saved", "Not saved"]][appState.lang === "zh" ? 0 : 1], "waiting"],
        [["输入", "Input"], [["待验证", "Pending validation"], ["Pending validation", "Pending validation"]][appState.lang === "zh" ? 0 : 1], "waiting"],
        [["运行模式", "Run mode"], [["快速环境自检", "Quick environment check"], ["Quick environment check", "Quick environment check"]][appState.lang === "zh" ? 0 : 1], "configured"],
        [["提交状态", "Submit state"], [["尚未提交", "Not submitted"], ["Not submitted", "Not submitted"]][appState.lang === "zh" ? 0 : 1], "idle"],
      ])),
      buttonRow([[[ "保存配置", "Save configuration" ], "", null], [[ "执行预检查", "Run preflight" ], "", null], [[ "启动任务", "Start workflow" ], "primary", null]]),
    ].join(""),
  }[id];
  return `<div class="wizard-layout">${wizardRail(id)}<section class="wizard-card">${body}</section></div>`;
}

function preflightTable() {
  const checks = appState.preflight?.checks || [];
  if (checks.length) {
    return `<div class="preflight-table">${dataTable(
      [[["检查项", "Check"], ["覆盖范围", "Coverage"], ["状态", "State"], ["实际结果", "Observed"], ["修复动作", "Repair action"]].map((x) => x)][0],
      checks.map((check) => [
        check.check_id || "",
        check.coverage || "",
        statusPill(check.status || "waiting"),
        check.observed || "",
        check.repair_action || "",
      ]),
      [["尚未执行预检查。", "Preflight has not run yet."]]
    )}</div>`;
  }
  return `<div class="preflight-table">${dataTable(
    [[["检查项", "Check"], ["覆盖范围", "Coverage"], ["状态", "State"], ["修复动作", "Repair action"]].map((x) => x)][0],
    [
      [[["分子格式", "Molecule format"], ["SMILES/SDF/MOL2/PDB、价态、三维结构、构象生成", "SMILES/SDF/MOL2/PDB, valence, 3D structure, conformers"], statusPill("waiting"), ["查看输入向导", "Open input wizard"]]],
      [[["序列合法性", "Sequence validity"], ["非法字符、非标准氨基酸、长度和修饰", "Invalid characters, non-standard residues, length, and modifications"], statusPill("waiting"), ["修正序列", "Edit sequence"]]],
      [[["结构文件", "Structure files"], ["链 ID、残基编号、缺失残基、可读性", "chain ID, residue numbering, missing residues, readability"], statusPill("waiting"), ["打开结构输入", "Open structure input"]]],
      [[["数据库", "Databases"], ["profile、路径、版本、hash、索引、缺失项", "profile, paths, versions, hashes, indexes, missing items"], statusPill("waiting"), ["进入数据库页", "Open databases"]]],
      [[["运行环境", "Runtime"], ["磁盘、写入权限、OpenMM、CUDA、Nextflow、Git remote、GitHub 权限", "disk, write permission, OpenMM, CUDA, Nextflow, Git remote, GitHub permission"], statusPill("waiting"), ["运行诊断", "Run diagnostics"]]],
    ].map((row) => row[0]),
    [["尚未执行预检查。", "Preflight has not run yet."]]
  )}</div>`;
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

function currentRunNotice() {
  return appState.selectedRun
    ? `run_id=${appState.selectedRun.run_id}`
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

function runMonitor() {
  const run = appState.selectedRun;
  const resource = run?.resource_status || {};
  const completedSteps = run?.completed_steps || [];
  const logTail = appState.runLogs.length ? appState.runLogs : run?.log_tail || [];
  const artifactItems = appState.runArtifacts?.items || run?.artifact_index?.items || [];
  return [
    card(
      [["运行状态机", "Run State Machine"]],
      optionTiles([
        [["created", "created"], ["项目和输入已保存，等待预检查。", "Project and input are saved, waiting for preflight."], "created"],
        [["preflight_passed", "preflight_passed"], ["预检查完成，可进入队列。", "Preflight completed and ready to queue."], "preflight_passed"],
        [["queued / running", "queued / running"], ["任务已排队或正在运行。", "Job is queued or running."], "running"],
        [["failed / cancelled / resumable", "failed / cancelled / resumable"], ["失败、取消或可恢复状态会显示原因和恢复入口。", "Failure, cancellation, or resumable state shows cause and recovery action."], "resumable"],
        [["completed / pushed_to_github", "completed / pushed_to_github"], ["完成后可浏览结果、导出报告或同步代码。", "After completion, browse results, export reports, or sync code."], "completed"],
      ])
    ),
    card(
      [["当前运行", "Current Run"]],
      dataTable(
        [[["字段", "Field"], ["值", "Value"]].map((x) => x)][0],
        run
          ? [
              [[["Run ID", "Run ID"], run.run_id]],
              [[["状态", "State"], run.state]],
              [[["当前阶段", "Current stage"], run.current_stage]],
              [[["已完成步骤", "Completed steps"], valueOrDash(completedSteps)]],
              [[["正在执行的命令", "Command"], run.current_command || "—"]],
              [["Nextflow run ID", run.nextflow_run_id || "—"]],
              [[["输出目录", "Output directory"], run.output_dir || "—"]],
              [[["失败原因", "Failure reason"], run.failure_reason || "—"]],
              [[["恢复状态", "Resume state"], run.resumable ? "resumable" : "not resumable"]],
            ].map((row) => row[0])
          : [
              [[["当前阶段", "Current stage"], [t("noActiveRun"), t("noActiveRun")]]],
              [[["正在执行的命令", "Command"], ["", ""]]],
              [["Nextflow run ID", ""]],
              [[["输出目录", "Output directory"], ["", ""]]],
              [[["失败原因", "Failure reason"], ["", ""]]],
              [[["恢复状态", "Resume state"], [t("idle"), t("idle")]]],
            ].map((row) => row[0]),
        [[t("noActiveRun"), t("noActiveRun")]]
      ) +
        buttonRow([[[ "提交任务", "Submit run" ], "primary", null], [[ "取消任务", "Cancel run" ], "", null], [[ "恢复中断任务", "Resume interrupted run" ], "", null]])
    ),
    card(
      [["资源监控", "Resource Monitor"]],
      dataTable(
        [["CPU", "GPU", "VRAM", ["内存", "Memory"], ["磁盘", "Disk"], ["日志尾部", "Log tail"]]],
        run
          ? [
              [
                resource.cpu || "—",
                resource.gpu || "—",
                resource.vram || "—",
                resource.ram || "—",
                resource.disk || "—",
                valueOrDash(logTail),
              ],
            ]
          : [],
        [["运行开始后显示资源占用。", "Resource usage appears after the run starts."]]
      )
    ),
    card(
      [["运行产物索引", "Run Artifact Index"]],
      dataTable(
        [[["产物类型", "Artifact type"], ["路径", "Path"], ["状态", "State"]].map((x) => x)][0],
        artifactItems.map((item) => [item.artifact_type || "", item.path || "", item.exists ? statusPill("ready") : statusPill("waiting")]),
        [["尚未创建运行产物。", "No run artifacts have been created yet."]]
      )
    ),
  ].join("");
}

function candidateRanking() {
  const payload = appState.candidates || {};
  const rows = candidateRows();
  const tableRows = rows.map((row) => candidateFieldKeys.map((key) => valueOrDash(rowValue(row, key))));
  const selector = rows.length
    ? `<div class="candidate-selector">${rows
        .slice(0, 24)
        .map((row) => {
          const key = candidateKey(row);
          const selected = key === candidateKey(selectedCandidate()) ? " selected" : "";
          return `<button class="candidate-chip${selected}" type="button" data-action="select-candidate" data-candidate-id="${encodeURIComponent(key)}">${escapeHtml(valueOrDash(rowValue(row, "rank")))} ${escapeHtml(key)}</button>`;
        })
        .join("")}</div>`
    : "";
  return card(
    [["候选排序表", "Candidate Ranking Table"]],
    `<p class="mono">${currentRunNotice()}</p>` +
      (payload.source_path ? `<p class="mono">source=${payload.source_path}</p>` : "") +
      dataTable(candidateHeaders, tableRows, payload.message || t("noCandidates")) +
      selector +
      buttonRow([[[ "打开候选详情", "Open candidate detail" ], "", "#/candidate_detail_page"], [[ "导出候选表", "Export candidate table" ], "primary", "#/report_export_page"]])
  );
}

function candidateDetail() {
  const candidate = selectedCandidate();
  if (candidate) {
    const evidenceRows = evidenceRowsForCandidate(candidate);
    const structureRows = artifactRows(["structure", "patch", "docking", "pose", ".pdb", ".cif", ".mmcif"]);
    const mdRows = artifactRows(["md_summary", "short_md", "molecular_dynamics", "rmsd", "rmsf", "drift", "occupancy", "trajectory", "hbond", "contact"]);
    return [
      card(
        [["候选详情", "Candidate Detail"]],
        `<p class="mono">${currentRunNotice()}</p>` +
          dataTable(
            [[["字段", "Field"], ["值", "Value"]].map((x) => x)][0],
            rowsFromFields(candidate, [
              [["candidate_id", "candidate_id"], ["candidate_id"]],
              [["gene_name", "gene_name"], ["gene_name", "gene"]],
              [["orf_id", "orf_id"], ["orf_id", "orf"]],
              [["source_artifact", "source_artifact"], ["source_artifact", "source_file", "artifact"]],
              [["candidate_source", "candidate_source"], ["candidate_source", "source", "origin"]],
              [["database_evidence", "database_evidence"], ["database_evidence", "database_source", "known_database_hit"]],
              [["risk_flags", "risk_flags"], ["risk_flags", "risk_flag"]],
              [["experimental_validation_guidance", "experimental_validation_guidance"], ["experimental_validation_guidance", "validation_guidance", "experimental_guidance"]],
            ]),
            [["当前候选行缺少可显示字段。", "The current candidate row has no displayable fields."]]
          )
      ),
      card(
        [["ORF 与基因组位置", "ORF and Genomic Location"]],
        dataTable(
          [[["字段", "Field"], ["值", "Value"]].map((x) => x)][0],
          rowsFromFields(candidate, [
            [["ORF sequence", "ORF sequence"], ["orf_sequence", "protein_sequence", "sequence", "translated_sequence"]],
            [["genomic_coordinates", "genomic_coordinates"], ["genomic_coordinates", "coordinates", "genome_location"]],
            [["reading_frame", "reading_frame"], ["reading_frame", "frame", "orf_frame"]],
            [["strand", "strand"], ["strand"]],
            [["flanking_region", "flanking_region"], ["flanking_region", "flank", "upstream_downstream_region"]],
            [["upstream_region", "upstream_region"], ["upstream_region", "upstream_sequence"]],
            [["downstream_region", "downstream_region"], ["downstream_region", "downstream_sequence"]],
          ]),
          [["候选行尚未包含 ORF 或基因组字段。", "The candidate row does not yet include ORF or genomic fields."]]
        )
      ),
      card(
        [["证据模块", "Evidence Modules"]],
        dataTable(
          [[["字段", "Field"], ["值", "Value"]].map((x) => x)][0],
          rowsFromFields(candidate, [
            [["fragment_hits", "fragment_hits"], ["fragment_hits", "fragment_evidence", "fragment_score"]],
            [["co_occurrence_evidence", "co_occurrence_evidence"], ["co_occurrence_evidence", "co_occurrence_score"]],
            [["pLM evidence", "pLM evidence"], ["plm_evidence", "plm_score"]],
            [["structure_evidence", "structure_evidence"], ["structure_evidence", "structure_score", "patch_evidence"]],
            [["docking_evidence", "docking_evidence"], ["docking_evidence", "docking_score", "docking_pose"]],
            [["MD evidence", "MD evidence"], ["md_evidence", "md_stability_score", "md_score_decomposition"]],
          ]).concat([[[ "linked_evidence_rows", "linked_evidence_rows" ], String(evidenceRows.length)]]),
          [["候选行尚未包含证据字段。", "The candidate row does not yet include evidence fields."]]
        )
      ),
      card(
        [["结构、Docking 与 MD 产物", "Structure, Docking, and MD Artifacts"]],
        dataTable(
          [[["类型", "Type"], ["路径", "Path"], ["状态", "State"]].map((x) => x)][0],
          [...structureRows, ...mdRows],
          [["当前候选没有关联结构或 MD 产物。", "The selected candidate has no linked structure or MD artifacts yet."]]
        )
      ),
      card(
        [["候选证据 Ledger 行", "Candidate Evidence Ledger Rows"]],
        detailEvidenceTable(evidenceRows, [["当前候选没有匹配的 evidence ledger 行。", "No evidence ledger row currently matches this candidate."]])
      ),
      buttonRow([[[ "返回候选排序", "Back to ranking" ], "", "#/candidate_ranking_page"]]),
    ].join("");
  }
  return [
    stateBlock("", [["未选择候选", "No Candidate Selected"]], [t("notSelected"), t("notSelected")], "#/candidate_ranking_page"),
    card(
      [["详情页字段", "Detail Sections"]],
      optionTiles([
        [["来源与 ORF 序列", "Source and ORF sequence"], ["来源文件、ORF 序列、阅读框和上下游区域。", "source file, ORF sequence, reading frame, and flanking regions."], "waiting"],
        [["证据模块", "Evidence modules"], ["片段命中、共现、结构、docking、MD、数据库证据。", "fragment hit, co-occurrence, structure, docking, MD, and database evidence."], "waiting"],
        [["风险与实验建议", "Risk and validation guidance"], ["显示风险提示和后续实验验证建议。", "Shows risk flags and follow-up experimental validation guidance."], "waiting"],
      ])
    ),
  ].join("");
}

function genomeContext() {
  const rows = candidateRows();
  if (rows.length) {
    return card(
      [["Genome/ORF Context Viewer", "Genome/ORF Context Viewer"]],
      `<p class="mono">${currentRunNotice()}</p>` +
        dataTable(
          [[["候选", "Candidate"], ["基因", "Gene"], ["ORF", "ORF"], ["坐标", "Coordinates"], ["上下文", "Context"], ["阅读框", "Frame"], ["变异", "Variant"]].map((x) => x)][0],
          rows.map((row) => [
            valueOrDash(candidateKey(row)),
            valueOrDash(rowValue(row, "gene_name")),
            valueOrDash(rowValue(row, "orf_id")),
            valueOrDash(rowValue(row, "genomic_coordinates")),
            valueOrDash(rowValue(row, "context_class")),
            valueOrDash(firstRowValue(row, ["reading_frame", "frame", "orf_frame", "strand"])),
            valueOrDash(firstRowValue(row, ["variant_id", "variant", "allele"])),
          ]),
          [["当前 run 尚未生成 Genome/ORF context 行。", "The selected run has not generated Genome/ORF context rows yet."]]
        )
    );
  }
  return card(
    [["Genome/ORF Context Viewer", "Genome/ORF Context Viewer"]],
    dataTable(
      [[["上下文类型", "Context type"], ["说明", "Description"], ["可显示字段", "Fields"]].map((x) => x)][0],
      [
        [["CDS", "known coding sequence", "gene, transcript, frame, coordinates"]],
        [["alternative reading frame", "non-primary ORF on annotated transcript", "frame, start, stop, overlap"]],
        [["UTR ORF", "5' or 3' UTR translated region", "UTR side, frame, length"]],
        [["lncRNA ORF", "translated ORF on non-coding transcript", "transcript, frame, coordinates"]],
        [["intron ORF", "candidate region inside intron", "intron index, strand, frame"]],
        [["antisense ORF", "opposite-strand translation", "strand, overlap, nearby gene"]],
        [["pseudogene ORF", "pseudogene-associated candidate", "pseudogene ID, frame"]],
        [["overlapping ORF", "overlaps another annotated feature", "overlap class, coordinates"]],
        [["variant-created ORF", "VCF-aware candidate", "variant ID, allele, frame"]],
      ],
      [["完成候选生成后显示 Genome/ORF context。", "Genome/ORF context appears after candidate generation."]]
    )
  );
}

function structureViewer() {
  const rows = artifactRows(["structure", "patch", "docking", "pose", ".pdb", ".cif", ".mmcif"]);
  const candidate = selectedCandidate();
  const evidenceRows = evidenceRowsForTokens(["structure", "patch", "docking", "pose", "residue"]);
  return [
    `<section class="panel"><div class="panel-header"><h2>${pick(["结构视图", "Structure View"])}</h2>${statusPill(rows.length ? "ready" : "idle")}</div>
      <div class="structure-stage" aria-label="${pick(["结构产物查看区域", "Structure artifact viewer"])}">
        <div class="pocket-cloud"></div><div class="ligand-stick"></div><div class="patch-ring"></div>
        <span>${rows.length ? pick(["已发现结构相关产物", "Structure-related artifacts detected"]) : pick(["等待真实结构产物", "Waiting for computed structure artifact"])}</span>
      </div>
      <p>${pick(["页面读取当前 run 的结构、patch、docking pose 和坐标产物索引。", "This page reads structure, patch, docking pose, and coordinate artifacts from the selected run index."])}</p>
    </section>`,
    card(
      [["结构与 Patch 产物", "Structure and Patch Artifacts"]],
      dataTable(
        [[["产物类型", "Artifact type"], ["路径", "Path"], ["状态", "State"]].map((x) => x)][0],
        rows,
        [["当前 run 尚未生成结构、patch 或 docking 产物。", "The selected run has not generated structure, patch, or docking artifacts yet."]]
      )
    ),
    card(
      [["结构与 Patch 字段", "Structure and Patch Fields"]],
      dataTable(
        [[["字段", "Field"], ["值", "Value"]].map((x) => x)][0],
        candidate
          ? rowsFromFields(candidate, [
              [["candidate_id", "candidate_id"], ["candidate_id"]],
              [["structure_score", "structure_score"], ["structure_score"]],
              [["structure_evidence", "structure_evidence"], ["structure_evidence"]],
              [["binding_patch", "binding_patch"], ["binding_patch", "patch", "patch_id"]],
              [["patch_evidence", "patch_evidence"], ["patch_evidence", "patch_descriptor_hash"]],
              [["key_residues", "key_residues"], ["key_residues", "interaction_residues", "residues"]],
              [["docking_score", "docking_score"], ["docking_score"]],
              [["docking_pose", "docking_pose"], ["docking_pose", "pose_file"]],
              [["ligand_position", "ligand_position"], ["ligand_position", "ligand_coordinates"]],
            ])
          : [],
        [["选择候选后显示结构、patch、pose 和相互作用字段。", "Select a candidate to show structure, patch, pose, and interaction fields."]]
      )
    ),
    card(
      [["结构证据 Ledger 行", "Structure Evidence Ledger Rows"]],
      detailEvidenceTable(evidenceRows, [["当前 run 尚未生成结构相关 evidence ledger 行。", "The selected run has no structure-related evidence ledger rows yet."]])
    ),
  ].join("");
}

function mdSummary() {
  const rows = artifactRows(["md_summary", "short_md", "molecular_dynamics", "rmsd", "rmsf", "drift", "occupancy", "trajectory", "hbond", "contact"]);
  const candidate = selectedCandidate();
  const evidenceRows = evidenceRowsForTokens(["md_stability", "md_score", "rmsd", "rmsf", "drift", "occupancy", "trajectory", "hbond", "contact"]);
  return [
    card(
      [["MD 摘要产物", "MD Summary Artifacts"]],
      dataTable(
        [[["产物类型", "Artifact type"], ["路径", "Path"], ["状态", "State"]].map((x) => x)][0],
        rows,
        [["当前 run 尚未生成 RMSD、RMSF、occupancy 或 score decomposition 产物。", "The selected run has not generated RMSD, RMSF, occupancy, or score decomposition artifacts yet."]]
      )
    ),
    card(
      [["MD 摘要字段", "MD Summary Fields"]],
      dataTable(
        [[["指标", "Metric"], ["值", "Value"]].map((x) => x)][0],
        candidate
          ? rowsFromFields(candidate, [
              [["RMSD", "RMSD"], ["rmsd", "md_rmsd", "rmsd_summary"]],
              [["RMSF", "RMSF"], ["rmsf", "md_rmsf", "rmsf_summary"]],
              [["ligand drift", "ligand drift"], ["ligand_drift", "drift"]],
              [["hydrogen bond occupancy", "hydrogen bond occupancy"], ["hydrogen_bond_occupancy", "hbond_occupancy"]],
              [["contact occupancy", "contact occupancy"], ["contact_occupancy"]],
              [["interaction persistence", "interaction persistence"], ["interaction_persistence"]],
              [["MD risk flags", "MD risk flags"], ["md_risk_flags", "risk_flags"]],
              [["MD score decomposition", "MD score decomposition"], ["md_score_decomposition", "score_decomposition"]],
              [["MD stability score", "MD stability score"], ["md_stability_score"]],
            ])
          : [],
        [["选择候选后显示 MD 摘要字段。", "Select a candidate to show MD summary fields."]]
      )
    ),
    card(
      [["MD 证据 Ledger 行", "MD Evidence Ledger Rows"]],
      detailEvidenceTable(evidenceRows, [["当前 run 尚未生成 MD 相关 evidence ledger 行。", "The selected run has no MD-related evidence ledger rows yet."]])
    ),
  ].join("");
}

function evidenceLedger() {
  const payload = appState.evidence || {};
  const rows = payload.evidence || [];
  const tableRows = rows.map((row) => evidenceFieldKeys.map((key) => valueOrDash(rowValue(row, key))));
  return card(
    [["Evidence Ledger", "Evidence Ledger"]],
    `<p class="mono">${currentRunNotice()}</p>` +
      (payload.source_path ? `<p class="mono">source=${payload.source_path}</p>` : "") +
      dataTable(evidenceHeaders, tableRows, payload.message || t("noLedger"))
  );
  return card(
    [["Evidence Ledger", "Evidence Ledger"]],
    dataTable(
      [[["报告项", "Report item"], ["原始文件", "Source file"], ["manifest", "manifest"], ["schema", "schema"], ["工具版本", "tool version"], ["artifact hash", "artifact hash"], ["验证状态", "validation state"], ["报告 section", "report section"]].map((x) => x)][0],
      [],
      [t("noLedger"), t("noLedger")]
    )
  );
}

function databaseRegistry() {
  const payload = appState.databases || {};
  const resources = payload.resources || [];
  const rows = resources.map((resource) => {
    if (typeof resource === "string") {
      return [
        resource,
        payload.profiles?.length ? `${payload.profiles.length} profile(s)` : pick(["尚未选择 profile", "No profile selected"]),
        "",
        "",
        statusPill("waiting"),
        "",
        pick(["选择路径并验证索引", "Select paths and validate indexes"]),
      ];
    }
    const state = resource.index_state === "indexed" || resource.index_state === "data_present_index_not_detected" ? "ready" : resource.required ? "waiting" : "optional";
    return [
      `${resource.resource || ""}${resource.required ? " *" : ""}`,
      valueOrDash(resource.path),
      valueOrDash(resource.version || resource.acquisition),
      valueOrDash(resource.sha256),
      statusPill(state, valueOrDash(resource.index_state)),
      `${valueOrDash(resource.storage_bytes)} bytes / ${valueOrDash(resource.file_count)} files`,
      valueOrDash(resource.repair_action),
    ];
  });
  if (rows.length) {
    const profileRows = (payload.profiles || []).map((profile) => [
      valueOrDash(profile.profile_id),
      valueOrDash(profile.path),
      statusPill(profile.status === "loaded" ? "ready" : "waiting", valueOrDash(profile.status)),
      valueOrDash(profile.source_count),
    ]);
    return [
      card(
        [["数据库 Profile", "Database Profile"]],
        dataTable(
          [[["profile", "profile"], ["配置路径", "Config path"], ["状态", "State"], ["source count", "source count"]].map((x) => x)][0],
          profileRows,
          payload.message || pick(["尚未选择数据库 profile。", "No database profile selected."])
        )
      ),
      card(
        [["数据库资源", "Database Resources"]],
        dataTable(
          [[["资源", "Resource"], ["路径", "Path"], ["版本", "Version"], ["hash", "hash"], ["索引状态", "Index state"], ["空间", "Storage"], ["修复动作", "Repair action"]].map((x) => x)][0],
          rows,
          payload.message || pick(["尚未选择数据库 profile。", "No database profile selected."])
        ) + buttonRow([[[ "验证数据库 profile", "Validate database profile" ], "", null], [[ "构建数据库索引", "Build database indexes" ], "primary", null]])
      ),
    ].join("");
  }
  return dataTable(
    [[["资源", "Resource"], ["路径", "Path"], ["版本", "Version"], ["hash", "hash"], ["索引状态", "Index state"], ["空间", "Storage"], ["修复动作", "Repair action"]].map((x) => x)][0],
    [
      [[["基因组", "Genome"], ["待选择 profile", "Select profile"], "", "", statusPill("waiting"), "", ["挂载或选择", "Mount or select"]]],
      [[["注释", "Annotation"], ["待选择 GTF/GFF3", "Select GTF/GFF3"], "", "", statusPill("waiting"), "", ["上传或索引", "Upload or index"]]],
      [[["转录本", "Transcripts"], ["待选择 FASTA", "Select FASTA"], "", "", statusPill("waiting"), "", ["上传或索引", "Upload or index"]]],
      [["BioLiP2", ["待配置数据根", "Configure data root"], "", "", statusPill("waiting"), "", ["解析 manifest", "Parse manifest"]]],
      [["UniProt", ["待配置 reviewed proteome", "Configure reviewed proteome"], "", "", statusPill("waiting"), "", ["构建序列索引", "Build sequence index"]]],
      [["ChEMBL/BindingDB", ["待配置 warehouse", "Configure warehouse"], "", "", statusPill("waiting"), "", ["验证表和映射", "Validate tables and mappings"]]],
      [[["结构缓存", "Structure cache"], ["待配置 RCSB/AFDB cache", "Configure RCSB/AFDB cache"], "", "", statusPill("waiting"), "", ["构建结构索引", "Build structure index"]]],
    ].map((row) => row[0]),
    [["尚未选择数据库 profile。", "No database profile selected."]]
  );
}

function diagnostics() {
  const checks = appState.diagnostics?.diagnostics || [];
  if (checks.length) {
    return [
      card(
        [["诊断覆盖", "Diagnostic Coverage"]],
        dataTable(
          [[["检查项", "Check"], ["说明", "Description"], ["状态", "State"], ["修复入口", "Repair entry"]].map((x) => x)][0],
          checks.map((check) => [
            check.check_id || "",
            check.description || check.check_id || "",
            statusPill(check.status || "waiting"),
            check.repair_action || "",
          ]),
          [["尚未运行诊断。", "Diagnostics have not run yet."]]
        ) + buttonRow([[[ "运行环境诊断", "Run diagnostics" ], "primary", null]])
      ),
    ].join("");
  }
  return [
    card(
      [["诊断覆盖", "Diagnostic Coverage"]],
      dataTable(
        [[["检查项", "Check"], ["说明", "Description"], ["状态", "State"], ["修复入口", "Repair entry"]].map((x) => x)][0],
        [
          [["WSL2 / Linux", ["运行后记录当前后端、发行版和 kernel", "Records active backend, distribution, and kernel after running"], statusPill("waiting"), ["查看运行环境", "Open runtime settings"]]],
          [["Rust binary", ["检查 gls-cli 和 gls-server", "Checks gls-cli and gls-server"], statusPill("waiting"), ["重新构建", "Rebuild"]]],
          [["Nextflow / Java", ["检查 workflow 调度入口", "Checks workflow scheduler"], statusPill("waiting"), ["打开运行模式", "Open run mode"]]],
          [["Python worker", ["只作为科学 primitive 执行边界", "Only scientific primitive execution boundary"], statusPill("waiting"), ["查看 worker schema", "Open worker schema"]]],
          [["OpenMM / CUDA / NVIDIA", ["检查平台、GPU、显存和驱动", "Checks platform, GPU, VRAM, and driver"], statusPill("waiting"), ["运行 GPU 诊断", "Run GPU diagnostics"]]],
          [[["数据库路径", "Database paths"], ["检查路径、hash、索引、空间和写权限", "Checks paths, hashes, indexes, space, and write permission"], statusPill("waiting"), ["打开数据库页", "Open databases"]]],
          [["Git / GitHub", ["检查 remote、权限、push 日志和远端 HEAD", "Checks remote, permission, push log, and remote HEAD"], statusPill("optional"), ["打开 GitHub 同步", "Open GitHub sync"]]],
          [[t("validationProject"), t("validationProject")], [t("validationProjectText"), t("validationProjectText")], statusPill("ready"), ["创建模板", "Create template"]],
        ].map((row) => row[0]),
        [["尚未运行诊断。", "Diagnostics have not run yet."]]
      )
    ),
  ].join("");
}

function githubSync() {
  const github = appState.github || {};
  const rows = [
    [["当前 branch", "Current branch"], valueOrDash(github.branch), github.branch ? statusPill("ready") : statusPill("waiting")],
    [["本地 commit hash", "Local commit hash"], valueOrDash(github.local_commit_hash), github.local_commit_hash ? statusPill("ready") : statusPill("waiting")],
    [["远端 branch", "Remote branch"], valueOrDash(github.remote_branch), github.remote_branch ? statusPill("ready") : statusPill("waiting")],
    [["push 状态", "Push state"], valueOrDash(github.push_status), statusPill(github.push_status === "completed" ? "completed" : "idle")],
    [["push 日志", "Push log"], valueOrDash(github.push_log), statusPill(github.push_log?.length ? "ready" : "idle")],
    [["远端验证", "Remote verification"], github.remote_head_matches_local_head === null || github.remote_head_matches_local_head === undefined ? "not checked" : String(github.remote_head_matches_local_head), statusPill(github.remote_head_matches_local_head ? "ready" : "idle")],
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
  return card(
    [["GitHub 同步", "GitHub Sync"]],
    dataTable(
      [[["字段", "Field"], ["值", "Value"], ["状态", "State"]].map((x) => x)][0],
      [
        [[["当前 branch", "Current branch"], ["", ""], statusPill("waiting")]],
        [[["本地 commit hash", "Local commit hash"], ["", ""], statusPill("waiting")]],
        [[["远端 branch", "Remote branch"], ["", ""], statusPill("waiting")]],
        [[["push 状态", "Push state"], ["", ""], statusPill("idle")]],
        [[["push 日志", "Push log"], ["", ""], statusPill("idle")]],
        [[["远端验证", "Remote verification"], ["", ""], statusPill("idle")]],
      ].map((row) => row[0]),
      [["尚未读取 Git 状态。", "Git state has not been read yet."]]
    ) +
      buttonRow([["git status", "", null], ["git add", "", null], ["git commit", "", null], ["git push", "primary", null], [["验证 remote HEAD", "Verify remote HEAD"], "", null]])
  );
}

function reportExport() {
  const payload = appState.exports || {};
  const exportRows = payload.files?.length
    ? payload.files.map((file) => [
        file.export_type || "",
        file.path || "",
        valueOrDash(file.bytes),
        file.exists ? statusPill("ready") : statusPill("warning"),
      ])
    : (payload.exports || [
        "html_report",
        "markdown_report",
        "candidate_csv_tsv",
        "machine_readable_json",
        "artifact_bundle",
        "run_manifest",
        "tool_version_manifest",
        "diagnostics_report",
        "github_push_report",
      ]).map((name) => [
        name,
        payload.export_dir || pick(["等待生成导出 manifest", "Waiting for export manifest"]),
        "",
        statusPill(payload.status || "waiting"),
      ]);
  return card(
    [["导出项目", "Export Targets"]],
    dataTable(
      [[["导出类型", "Export type"], ["输出位置", "Output location"], ["bytes", "bytes"], ["状态", "State"]].map((x) => x)][0],
      exportRows,
      [["尚未生成导出 manifest。", "No export manifest has been generated yet."]]
    ) + buttonRow([[[ "生成导出 manifest", "Export candidate table" ], "primary", null]])
  );
  return card(
    [["导出项目", "Export Targets"]],
    optionTiles([
      [["HTML report", "HTML report"], ["适合浏览器查看和分享。", "For browser viewing and sharing."], "ready"],
      [["Markdown report", "Markdown report"], ["适合版本控制和文本审阅。", "For version control and text review."], "ready"],
      [["CSV/TSV candidate table", "CSV/TSV candidate table"], ["候选排序和分数明细。", "Candidate ranking and score details."], "ready"],
      [["JSON machine-readable report", "JSON machine-readable report"], ["供脚本和下游系统读取。", "For scripts and downstream systems."], "ready"],
      [["artifact bundle", "artifact bundle"], ["打包产物、manifest 和 hash。", "Bundles artifacts, manifests, and hashes."], "ready"],
      [["run manifest", "run manifest"], ["记录命令、输入、资源和输出路径。", "Records commands, inputs, resources, and output paths."], "ready"],
      [["tool version manifest", "tool version manifest"], ["记录本地工具版本和 license。", "Records local tool versions and licenses."], "ready"],
      [["diagnostics report", "diagnostics report"], ["环境诊断和修复建议。", "Environment diagnostics and repair guidance."], "ready"],
      [["GitHub push report", "GitHub push report"], ["同步操作和远端验证记录。", "Sync operation and remote verification record."], "ready"],
    ])
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

function settings() {
  return [
    card(
      [["界面设置", "Interface Settings"]],
      dataTable(
        [[["设置", "Setting"], ["当前值", "Current value"], ["说明", "Description"]].map((x) => x)][0],
        [
          [[t("language"), appState.lang === "zh" ? "中文" : "English", ["右上角按钮可切换", "Use the top-right control to switch"]]],
          [[["界面配色", "Color scheme"], ["暗色", "Dark"], ["正式版固定暗色界面。", "The production console is dark-only."]]],
          [[["服务地址", "Service URL"], "http://127.0.0.1:19090/", ["本地 gls-server", "Local gls-server"]]],
          [[["版本", "Version"], APP_VERSION, ["正式版标识", "Production version"]]],
        ].map((row) => row[0]),
        [["设置尚未加载。", "Settings have not loaded yet."]]
      )
    ),
    card(
      [["安装指南", "Installation Guides"]],
      guideRegistry() || optionTiles([
        [["用户指南", "User guide"], ["启动服务、打开 WebUI、配置输入、运行、查看结果和导出报告。", "Start service, open WebUI, configure input, run, inspect results, and export reports."], "ready"],
        [["Windows + WSL2 安装指南", "Windows + WSL2 guide"], ["配置 WSL2、GPU/OpenMM、数据库挂载和 GitHub 同步。", "Configure WSL2, GPU/OpenMM, database mounts, and GitHub sync."], "ready"],
        [["Linux server 安装指南", "Linux server guide"], ["在 Linux 服务器上部署本地服务和浏览器入口。", "Deploy local service and browser entry on a Linux server."], "ready"],
      ])
    ),
  ].join("");
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
    database_registry_page: () => card([["数据库 Profile", "Database Profile"]], databaseRegistry()),
    diagnostics_page: diagnostics,
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
    .map(([id, index, title, subtitle]) => `<a class="nav-item ${id === activeId ? "active" : ""}" href="#/${id}"><span class="nav-icon">${index}</span><span class="nav-copy"><strong>${pick(title)}</strong><small>${pick(subtitle)}</small></span></a>`)
    .join("");
}

function renderRuntime() {
  const state = appState.selectedRun?.state || "idle";
  const steps = [
    [["项目", "Project"], appState.selectedProject ? "ready" : "idle"],
    [["预检查", "Preflight"], appState.selectedProject?.project_status === "preflight_passed" ? "ready" : "waiting"],
    [["运行", "Run"], state],
    [["结果", "Results"], state === "completed" ? "ready" : "waiting"],
    [["同步", "Sync"], state === "pushed_to_github" ? "pushed_to_github" : "optional"],
  ];
  document.querySelector("[data-runtime-timeline]").innerHTML = steps
    .map(([label, status], index) => `<div class="timeline-item"><span>0${index}</span><strong>${pick(label)}</strong>${statusPill(status)}</div>`)
    .join("");
  document.querySelector("[data-runtime-log]").textContent =
    appState.selectedRun
      ? [
          `run_id=${appState.selectedRun.run_id}`,
          `state=${appState.selectedRun.state}`,
          `stage=${appState.selectedRun.current_stage}`,
          `command=${appState.selectedRun.current_command || "not_started"}`,
          `output_dir=${appState.selectedRun.output_dir || "not_created"}`,
        ].join("\n")
      : appState.lang === "zh"
      ? "GenoLigand 0.9.1 本地工作台已就绪。\n当前浏览器会话没有绑定运行。\n请创建或打开项目，完成预检查后启动任务。"
      : "GenoLigand 0.9.1 local console is ready.\nNo active run is attached to this browser session.\nCreate or open a project, complete preflight, then start a workflow.";
}

function renderDetail(activeId) {
  const title = pageMeta[activeId]?.[0] || pageMeta.dashboard_page[0];
  document.querySelector("[data-detail]").innerHTML = `
    <h2>${pick(["页面详情", "Page Details"])}</h2>
    <div class="detail-grid">
      <div class="detail-card"><span class="detail-label">${pick(["当前页面", "Current page"])}</span><strong class="detail-value">${pick(title)}</strong>${statusPill("ready")}</div>
      <div class="detail-card"><span class="detail-label">${pick(["路由", "Route"])}</span><strong class="detail-value mono">${activeId}</strong>${statusPill("ready")}</div>
      <div class="detail-card"><span class="detail-label">${t("apiOwner")}</span><strong class="detail-value">gls-server</strong>${statusPill("ready")}</div>
      <div class="detail-card"><span class="detail-label">${t("scienceOwner")}</span><strong class="detail-value">gls-cli</strong>${statusPill("ready")}</div>
    </div>
    <h2>${pick(["接口覆盖", "API Surface"])}</h2>
    <ul class="api-list">${apiRoutes.slice(0, 10).map((item) => `<li>${item}</li>`).join("")}</ul>
    <h2>${pick(["本地文件", "Local Files"])}</h2>
    <ul class="artifact-tree">
      <li>projects/&lt;project-id&gt;/project.json</li>
      <li>projects/&lt;project-id&gt;/input_manifest.json</li>
      <li>projects/&lt;project-id&gt;/runs/&lt;run-id&gt;/run_manifest.json</li>
      <li>projects/&lt;project-id&gt;/reports/</li>
    </ul>
  `;
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
      ? "本地服务：127.0.0.1:19090 / 后端：gls-server / 状态：等待项目"
      : "Local service: 127.0.0.1:19090 / backend: gls-server / state: waiting for project";
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
  document.querySelector("[data-main]").innerHTML = `${actionBanner()}${renderPage(activeId)}`;
  renderDetail(activeId);
  renderRuntime();
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
    value: node.value || "",
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
  return {
    saved_at: new Date().toISOString(),
    route: routeFromLocation(),
    inputs: appState.inputDraft.inputs,
    files: appState.inputDraft.files,
    advanced_parameters_collapsed: true,
    field_schema: "genoligand.webui.input_manifest.v0.9.1",
  };
}

function queueInputDraftMerge() {
  mergeInputDraft().catch(() => {
    setMessage("warn", "输入暂存未完成，请保存配置时重试。", "Input capture did not finish; retry when saving configuration.");
  });
}

function setMessage(kind, zh, en) {
  appState.message = { kind, text: appState.lang === "zh" ? zh : en };
}

async function refreshProjects() {
  const payload = await apiRequest("/api/projects");
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
    appState.selectedCandidateId = null;
    return;
  }
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
}

async function refreshWorkspaceState() {
  try {
    appState.databases = await apiRequest("/api/databases");
  } catch (_error) {
    appState.databases = null;
  }
  try {
    appState.diagnostics = await apiRequest("/api/diagnostics");
  } catch (_error) {
    appState.diagnostics = null;
  }
  try {
    appState.guides = await apiRequest("/api/guides");
  } catch (_error) {
    appState.guides = null;
  }
  try {
    appState.github = await apiRequest("/api/github/status");
  } catch (_error) {
    appState.github = null;
  }
}

async function handleAction(action, dataset = {}) {
  try {
    if (action === "select-candidate") {
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
    } else if (action === "create-project" || action === "create-validation-project") {
      const path = action === "create-validation-project" ? "/api/projects/environment-validation" : "/api/projects";
      const payload = await apiRequest(path, { method: "POST" });
      appState.selectedProject = payload.project || null;
      appState.selectedRun = null;
      await refreshProjects();
      setMessage("", "项目已写入本地 manifest。", "Project manifest was written locally.");
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
      await refreshProjects();
      setMessage("warn", "项目已从本地 artifact 项目目录删除。", "Project was deleted from the local artifact project directory.");
    } else if (action === "clean-project") {
      const project = requireProject();
      await apiRequest(`/api/projects/${project.project_id}/cleanup`, { method: "POST" });
      setMessage("", "项目清理 manifest 已生成。", "Project cleanup manifest generated.");
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
      setMessage("", "配置已保存到本地项目 manifest。", "Configuration saved to the local project manifest.");
    } else if (action === "start-workflow" || action === "submit-run") {
      const project = requireProject();
      const payload = await apiRequest(`/api/projects/${project.project_id}/runs`, { method: "POST" });
      appState.selectedRun = payload.run || null;
      await refreshRunDetails(appState.selectedRun);
      await refreshProjects();
      await refreshProjectRuns();
      window.location.hash = "#/run_monitor_page";
      setMessage("", "运行已创建并排队，等待本地 gls-cli/Nextflow 执行。", "Run created and queued for local gls-cli/Nextflow execution.");
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
      appState.diagnostics = await apiRequest("/api/diagnostics/run", { method: "POST" });
      setMessage("", "诊断结果已从本地服务读取。", "Diagnostics were read from the local service.");
    } else if (action === "validate-databases") {
      await apiRequest("/api/databases/validate", { method: "POST" });
      await refreshWorkspaceState();
      setMessage("", "数据库 profile 验证已提交到本地服务。", "Database profile validation was submitted to the local service.");
    } else if (action === "build-database-indexes") {
      await apiRequest("/api/databases/build-index", { method: "POST" });
      await refreshWorkspaceState();
      setMessage("", "数据库索引构建动作已提交到本地服务。", "Database index build action was submitted to the local service.");
    } else if (action === "export-report") {
      appState.exports = await apiRequest("/api/reports/export", { method: "POST" });
      setMessage("", "导出 manifest 已生成。", "Export manifest generated.");
    } else if (action === "git-status") {
      appState.github = await apiRequest("/api/github/status");
      setMessage("", "Git 状态已读取。", "Git status loaded.");
    } else if (action === "git-add") {
      appState.github = await apiRequest("/api/github/add", { method: "POST" });
      setMessage("", "git add 已执行。", "git add completed.");
    } else if (action === "git-commit") {
      appState.github = await apiRequest("/api/github/commit", { method: "POST" });
      setMessage("", "git commit 已执行或返回当前状态。", "git commit completed or returned the current state.");
    } else if (action === "git-push") {
      appState.github = await apiRequest("/api/github/push", { method: "POST" });
      setMessage("", "git push 已执行并记录日志。", "git push completed and logged.");
    } else if (action === "git-verify") {
      appState.github = await apiRequest("/api/github/verify", { method: "POST" });
      setMessage("", "远端状态已读取。", "Remote state loaded.");
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
    const peptide = event.target.closest("[data-insert-peptide]");
    if (!peptide) {
      return;
    }
    const textAreaNode = document.querySelector("textarea");
    if (textAreaNode) {
      textAreaNode.value = peptide.dataset.insertPeptide;
      queueInputDraftMerge();
    }
  });
  document.addEventListener("input", (event) => {
    if (event.target.matches("input, textarea, select")) {
      queueInputDraftMerge();
    }
  });
  document.addEventListener("change", (event) => {
    if (event.target.matches("input, textarea, select")) {
      queueInputDraftMerge();
    }
  });
}

bindEvents();
render();
hydrateServerState();
