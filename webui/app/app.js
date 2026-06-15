const state = {
  step: "input",
  selectedInput: "small_molecule",
  project: null,
  run: null,
  health: null,
  databases: null,
  diagnostics: null,
  setup: null,
  setupRepair: null,
  preflight: null,
  runData: null,
  form: {
    project_name: "",
    small_molecule: "",
    peptide: "",
    protein_ligand: "",
    complex: "",
    genome_or_metagenome: "",
  },
  alert: "",
};

const page = document.querySelector("[data-page]");
const alerts = document.querySelector("[data-alerts]");
const inputSpecs = [
  {
    kind: "small_molecule",
    title: "小分子",
    field: "small_molecule",
    label: "SMILES、SDF、MOL2、化合物名称或登记号",
    placeholder: "例如：粘贴 SMILES，或填写本机结构文件路径",
    minimum: "至少提供一个可解析的分子表示。",
  },
  {
    kind: "peptide",
    title: "多肽",
    field: "peptide",
    label: "短肽序列、修饰说明或结构文件",
    placeholder: "例如：输入肽序列，修饰可另起一行说明",
    minimum: "至少提供肽序列或结构文件路径。",
  },
  {
    kind: "protein_ligand",
    title: "蛋白质配体",
    field: "protein_ligand",
    label: "FASTA、蛋白序列或结构文件",
    placeholder: "例如：粘贴 FASTA 或填写结构文件路径",
    minimum: "至少提供序列或结构文件路径。",
  },
  {
    kind: "complex",
    title: "复合物结构",
    field: "complex",
    label: "蛋白-配体复合物结构、链信息或结合区域",
    placeholder: "例如：填写 PDB/mmCIF 文件路径，并注明链或配体",
    minimum: "至少提供结构文件路径或复合物描述。",
  },
  {
    kind: "genome_or_metagenome",
    title: "自定义基因组",
    field: "genome_or_metagenome",
    label: "基因组、宏基因组、转录组或蛋白序列文件",
    placeholder: "例如：填写 FASTA/FASTQ 文件路径或粘贴蛋白序列",
    minimum: "至少提供序列文件路径或序列文本。",
  },
];

const stepMeta = {
  input: ["输入研究对象", "填写任一种研究输入，空输入不会生成候选。"],
  configure: ["配置搜索范围", "选择本次计算要覆盖的证据类型和输出。"],
  check: ["检查运行条件", "输入、数据库、运行环境、存储和导出条件都通过后再运行。"],
  run: ["运行中心", "查看当前阶段、下一步动作、资源状态和日志。"],
  results: ["查看结果", "候选排序、详情、证据、结构和动力学结果只来自当前运行输出。"],
  export: ["导出", "整理报告、候选表和可复现材料。"],
};

const stageLabels = [
  "输入归一化",
  "特征计算",
  "已知数据库比对",
  "宏基因组潜在序列比对",
  "结构与口袋评估",
  "对接与短动力学筛选",
  "联合排序",
  "证据链与报告",
];

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...options,
  });
  const text = await response.text();
  const value = text ? JSON.parse(text) : {};
  if (!response.ok) {
    throw new Error(value.error || `请求失败：${response.status}`);
  }
  return value;
}

function setStep(step) {
  state.step = step;
  document.querySelectorAll("[data-step]").forEach((button) => {
    button.classList.toggle("active", button.dataset.step === step);
  });
  render();
}

async function boot() {
  document.querySelectorAll("[data-step]").forEach((button) => {
    button.addEventListener("click", () => setStep(button.dataset.step));
  });
  document.querySelector("[data-refresh]").addEventListener("click", refreshAll);
  await refreshAll();
}

async function refreshAll() {
  try {
    state.alert = "";
    [state.health, state.databases, state.diagnostics, state.setup] = await Promise.all([
      api("/api/user/health"),
      api("/api/user/databases"),
      api("/api/user/diagnostics"),
      api("/api/user/setup/status"),
    ]);
    if (state.run?.run_id) {
      state.run = await api(`/api/runs/${state.run.run_id}`);
      await loadRunData();
    }
  } catch (error) {
    state.alert = `无法连接本机服务：${error.message}`;
  }
  render();
}

function render() {
  const [title, subtitle] = stepMeta[state.step];
  document.querySelector("[data-title]").textContent = title;
  document.querySelector("[data-subtitle]").textContent = subtitle;
  renderShellStatus();
  renderAlert();
  if (state.step === "input") renderInput();
  if (state.step === "configure") renderConfigure();
  if (state.step === "check") renderCheck();
  if (state.step === "run") renderRun();
  if (state.step === "results") renderResults();
  if (state.step === "export") renderExport();
}

function renderShellStatus() {
  const health = document.querySelector("[data-health]");
  const ready = state.health?.status === "ready";
  health.textContent = ready ? "服务就绪" : "服务未连接";
  health.className = `status-pill ${ready ? "ready" : "blocked"}`;
  document.querySelector("[data-project-pill]").textContent = state.project
    ? state.project.project_name || "当前项目"
    : "空白工作台";
}

function renderAlert() {
  alerts.innerHTML = state.alert
    ? `<div class="notice bad">${escapeHtml(state.alert)}</div>`
    : "";
}

function renderInput() {
  page.className = "page";
  const active = inputSpecs.find((item) => item.kind === state.selectedInput) || inputSpecs[0];
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>选择输入类型</h2>
            <p>可以填写一种或多种输入。只有用户填写的内容会进入计算。</p>
          </div>
          <span class="status-pill ${hasAnyInput() ? "ready" : "blocked"}">${hasAnyInput() ? "已有输入" : "等待输入"}</span>
        </div>
        <div class="input-grid">
          ${inputSpecs.map(renderInputTypeCard).join("")}
        </div>
      </section>
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>${active.title}</h2>
            <p>${active.label}</p>
          </div>
          <span class="status-pill">${active.minimum}</span>
        </div>
        <div class="field">
          <label for="project_name">项目名称</label>
          <input id="project_name" data-form="project_name" value="${escapeAttr(state.form.project_name)}" placeholder="可选；不填时使用自动名称" />
        </div>
        <div class="field">
          <label for="${active.field}">${active.label}</label>
          <textarea id="${active.field}" data-form="${active.field}" placeholder="${escapeAttr(active.placeholder)}">${escapeHtml(state.form[active.field])}</textarea>
        </div>
        <div class="actions">
          <button class="button" data-action="save-input">保存输入</button>
          <button class="button secondary" data-action="save-and-check">保存并检查</button>
          <button class="button ghost" data-action="clear-input">清空当前输入</button>
        </div>
      </section>
      <section class="panel third">
        <h2>输入状态</h2>
        ${metricRow("填写类型", `${filledInputs().length} / ${inputSpecs.length}`)}
        ${metricRow("候选显示", "运行完成前不显示候选")}
        ${metricRow("结果来源", "仅读取当前项目运行输出")}
      </section>
      <section class="panel third">
        <h2>下一步</h2>
        <p>${hasAnyInput() ? "保存后进入配置和检查。" : "先填写任一种输入。"}</p>
      </section>
      <section class="panel third">
        <h2>数据库状态</h2>
        ${storeStatusRows()}
      </section>
      <section class="panel full">
        <h2>科学资源</h2>
        ${sourceStatusTable()}
      </section>
    </div>`;
  bindInputEvents();
}

function renderInputTypeCard(spec) {
  const selected = state.selectedInput === spec.kind;
  const filled = state.form[spec.field].trim().length > 0;
  return `
    <button class="input-card ${selected ? "selected" : ""}" data-select-input="${spec.kind}">
      <h3>${spec.title}</h3>
      <p>${spec.minimum}</p>
      <span class="status-pill ${filled ? "ready" : "blocked"}">${filled ? "已填写" : "未填写"}</span>
    </button>`;
}

function bindInputEvents() {
  page.querySelectorAll("[data-select-input]").forEach((button) => {
    button.addEventListener("click", () => {
      collectForm();
      state.selectedInput = button.dataset.selectInput;
      renderInput();
    });
  });
  page.querySelectorAll("[data-form]").forEach((field) => {
    field.addEventListener("input", collectForm);
  });
  page.querySelector("[data-action='save-input']").addEventListener("click", saveInput);
  page.querySelector("[data-action='save-and-check']").addEventListener("click", async () => {
    const saved = await saveInput();
    if (saved) {
      await runPreflight();
      setStep("check");
    }
  });
  page.querySelector("[data-action='clear-input']").addEventListener("click", () => {
    const active = inputSpecs.find((item) => item.kind === state.selectedInput);
    state.form[active.field] = "";
    renderInput();
  });
}

function renderConfigure() {
  page.className = "page";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>搜索范围</h2>
            <p>数据库和索引由安装流程维护。普通运行不需要配置路径。</p>
          </div>
          <span class="status-pill ${state.project ? "ready" : "blocked"}">${state.project ? "已保存输入" : "等待输入"}</span>
        </div>
        <div class="panel-grid">
          ${scopeCard("已知蛋白数据库", "ChEMBL、BindingDB、UniProt、结构互作证据。", "known")}
          ${scopeCard("宏基因组潜在序列", "公共和自定义序列空间中的潜在靶点。", "metagenome")}
          ${scopeCard("结构与口袋证据", "结构、口袋、界面、对接和短动力学摘要。", "structure")}
        </div>
        <div class="actions">
          <button class="button" data-action="to-check" ${state.project ? "" : "disabled"}>进入检查</button>
          <button class="button ghost" data-action="to-input">返回输入</button>
        </div>
      </section>
      <section class="panel">
        <h2>运行边界</h2>
        ${metricRow("空输入", "不生成候选")}
        ${metricRow("数据库缺失", "阻塞运行并给出修复动作")}
        ${metricRow("结果解释", "计算预测，需要后续实验验证")}
      </section>
      <section class="panel">
        <h2>资源提示</h2>
        ${metricRow("GPU", "OpenMM/CUDA 可用时用于动力学筛选")}
        ${metricRow("CPU", "可完成输入归一化和数据库比对")}
        ${metricRow("磁盘", "完整科学仓库由安装流程准备")}
      </section>
    </div>`;
  page.querySelector("[data-action='to-check']")?.addEventListener("click", async () => {
    await runPreflight();
    setStep("check");
  });
  page.querySelector("[data-action='to-input']").addEventListener("click", () => setStep("input"));
}

function scopeCard(title, body, key) {
  const ready = sourceReady(key);
  return `
    <section class="panel compact third">
      <h3>${title}</h3>
      <p>${body}</p>
      <span class="status-pill ${ready ? "ready" : "blocked"}">${ready ? "就绪" : "需要准备"}</span>
    </section>`;
}

function renderCheck() {
  page.className = "page";
  const groups = preflightGroups();
  const allReady = groups.every((group) => group.rows.every((row) => row.ready));
  const setupReady = state.setup?.database_ready === true;
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>预检查</h2>
            <p>通过后即可开始运行；未通过时不会生成候选。</p>
          </div>
          <span class="status-pill ${allReady ? "ready" : "blocked"}">${allReady ? "可以运行" : "需要处理"}</span>
        </div>
        <div class="actions">
          <button class="button secondary" data-action="check-now" ${state.project ? "" : "disabled"}>重新检查</button>
          <button class="button" data-action="start-run" ${allReady ? "" : "disabled"}>开始运行</button>
          <button class="button secondary" data-action="repair-store" ${setupReady ? "disabled" : ""}>运行安装/修复</button>
          <button class="button ghost" data-action="to-input">修改输入</button>
        </div>
      </section>
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>安装与修复</h2>
            <p>本机缺少数据库、索引或工具记录时，可以先运行修复动作。</p>
          </div>
          <span class="status-pill ${setupReady ? "ready" : "blocked"}">${setupReady ? "已就绪" : "需要准备"}</span>
        </div>
        ${setupRepairSummary()}
        ${diagnosticsSummary()}
        ${sourceStatusTable()}
      </section>
      ${groups.map(renderCheckGroup).join("")}
    </div>`;
  page.querySelector("[data-action='check-now']")?.addEventListener("click", runPreflight);
  page.querySelector("[data-action='start-run']")?.addEventListener("click", startRun);
  page.querySelector("[data-action='repair-store']")?.addEventListener("click", runSetupRepair);
  page.querySelector("[data-action='to-input']").addEventListener("click", () => setStep("input"));
}

function renderCheckGroup(group) {
  return `
    <section class="panel third">
      <div class="panel-header">
        <h2>${group.title}</h2>
        <span class="status-pill ${group.rows.every((row) => row.ready) ? "ready" : "blocked"}">${group.rows.every((row) => row.ready) ? "通过" : "待处理"}</span>
      </div>
      ${group.rows.map((row) => metricRow(row.label, row.ready ? "通过" : row.next)).join("")}
    </section>`;
}

function setupRepairSummary() {
  if (!state.setupRepair) {
    return `<div class="notice warn">尚未运行本机修复。缺项处理完成前，运行按钮会保持禁用。</div>`;
  }
  const ready = state.setupRepair.database_ready === true;
  return `
    <div class="notice ${ready ? "" : "warn"}">
      ${escapeHtml(state.setupRepair.message || (ready ? "科学仓库已就绪。" : "修复动作已执行，但仍有缺项。"))}
    </div>`;
}

function diagnosticsSummary() {
  const checks = state.diagnostics?.checks || [];
  if (!checks.length) return "";
  return `
    <div class="panel-grid">
      ${checks
        .map(
          (row) => `
            <section class="panel compact third">
              <h3>${escapeHtml(row.label)}</h3>
              <p>${escapeHtml(row.ready ? "通过" : row.next_action || "需要处理")}</p>
              <span class="status-pill ${row.ready ? "ready" : "blocked"}">${row.ready ? "就绪" : "待处理"}</span>
            </section>`
        )
        .join("")}
    </div>`;
}

function sourceStatusTable() {
  const sources = state.databases?.sources || [];
  if (!sources.length) {
    return `<div class="empty">尚未生成科学资源清单。运行安装/修复后会显示每个数据库源的状态。</div>`;
  }
  return table(
    sources,
    [
      { key: "label", label: "资源" },
      { key: "category", label: "类别" },
      { key: "status", label: "状态" },
      { key: "next_action", label: "下一步" },
    ],
  );
}

function renderRun() {
  page.className = "page";
  const runState = state.run?.state || "not_started";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>运行中心</h2>
            <p>这里显示当前运行阶段、下一步动作、资源状态和日志。</p>
          </div>
          <span class="status-pill ${runStateClass(runState)}">${runStateLabel(runState)}</span>
        </div>
        <div class="actions">
          <button class="button" data-action="start-run" ${canStartRun() ? "" : "disabled"}>开始运行</button>
          <button class="button secondary" data-action="refresh-run">刷新</button>
          <button class="button ghost" data-action="to-results" ${state.run ? "" : "disabled"}>查看结果</button>
        </div>
      </section>
      <section class="panel">
        <h2>流程阶段</h2>
        <div class="stage-list">${stageLabels.map(renderStage).join("")}</div>
      </section>
      <section class="panel">
        <h2>运行摘要</h2>
        ${metricRow("当前状态", runStateLabel(runState))}
        ${metricRow("下一步", nextActionText())}
        ${metricRow("候选数量", `${candidateRows().length}`)}
        ${metricRow("阻塞原因", state.run?.failure_summary || "无")}
      </section>
      <section class="panel full">
        <h2>输出文件</h2>
        ${artifactRows()}
      </section>
      <section class="panel full">
        <details>
          <summary>运行事件</summary>
          <pre>${escapeHtml((state.runData?.logs?.logs || []).join("\n") || "暂无运行事件。")}</pre>
        </details>
      </section>
    </div>`;
  page.querySelector("[data-action='start-run']")?.addEventListener("click", startRun);
  page.querySelector("[data-action='refresh-run']").addEventListener("click", refreshAll);
  page.querySelector("[data-action='to-results']")?.addEventListener("click", () => setStep("results"));
}

function renderStage(label, index) {
  const runState = state.run?.state;
  const completed = runState === "completed";
  const active = runState === "running" && index === 0;
  return `
    <div class="stage">
      <span class="stage-index">${index + 1}</span>
      <span>${label}</span>
      <span class="status-pill ${completed ? "ready" : active ? "loading" : "blocked"}">${completed ? "完成" : active ? "进行中" : "等待"}</span>
    </div>`;
}

function renderResults() {
  page.className = "page";
  const candidates = candidateRows();
  const evidence = state.runData?.evidence?.evidence || [];
  const genome = state.runData?.genome?.genome_context || [];
  const structure = state.runData?.structure?.structure_rows || [];
  const md = state.runData?.md?.md_rows || [];
  page.innerHTML = `
    ${resultTracePanel()}
    <div class="tabs">
      <button class="tab active" data-result-tab="ranking">候选排序</button>
      <button class="tab" data-result-tab="detail">候选详情</button>
      <button class="tab" data-result-tab="evidence">证据链</button>
      <button class="tab" data-result-tab="genome">基因组/ORF</button>
      <button class="tab" data-result-tab="structure">结构/口袋</button>
      <button class="tab" data-result-tab="md">动力学摘要</button>
    </div>
    <div data-result-body>
      ${resultSection("候选排序", candidates, "ranking")}
    </div>`;
  page.querySelectorAll("[data-result-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      page.querySelectorAll(".tab").forEach((tab) => tab.classList.remove("active"));
      button.classList.add("active");
      const tab = button.dataset.resultTab;
      const body = page.querySelector("[data-result-body]");
      if (tab === "ranking") body.innerHTML = resultSection("候选排序", candidates, "ranking");
      if (tab === "detail") body.innerHTML = candidateDetailSection(candidates);
      if (tab === "evidence") body.innerHTML = resultSection("证据链", evidence, "evidence");
      if (tab === "genome") body.innerHTML = resultSection("基因组/ORF", genome, "genome");
      if (tab === "structure") body.innerHTML = structureSection(structure);
      if (tab === "md") body.innerHTML = mdSection(md);
    });
  });
}

function resultTracePanel() {
  const projectText = state.project?.project_name || (state.project ? "当前项目" : "未创建");
  const runText = state.run ? runStateLabel(state.run.state) : "未运行";
  const databaseReady = state.setup?.database_ready === true || preflightGroups()
    .find((group) => group.title === "数据库和索引")
    ?.rows.every((row) => row.ready);
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>结果来源</h2>
          <p>结果只读取当前项目的运行输出；没有输出时保持为空。</p>
        </div>
        <span class="status-pill ${state.run ? "ready" : "blocked"}">${state.run ? "已选择运行" : "未运行"}</span>
      </div>
      ${metricRow("当前项目", projectText)}
      ${metricRow("当前运行", runText)}
      ${metricRow("数据库状态", databaseReady ? "就绪" : "未就绪")}
      ${metricRow("候选来源", candidateRows().length ? "候选排序输出" : "暂无候选输出")}
      ${metricRow("证据来源", state.runData?.evidence?.has_output ? "证据链输出" : "暂无证据输出")}
    </section>`;
}

function resultSection(title, rows, kind) {
  const emptyText = {
    ranking: "当前没有候选。完成输入、检查和运行后才会显示候选排序。",
    evidence: "当前没有证据链输出。",
    genome: "当前没有宏基因组或 ORF 匹配输出。",
  }[kind] || "当前没有输出。";
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>${title}</h2>
          <p>${rows.length ? "数据来自当前运行输出。" : emptyText}</p>
        </div>
        <span class="status-pill ${rows.length ? "ready" : "blocked"}">${rows.length ? `${rows.length} 行` : "空"}</span>
      </div>
      ${rows.length ? table(rows, columnsFor(kind)) : `<div class="empty">${emptyText}</div>`}
    </section>`;
}

function candidateDetailSection(rows) {
  if (!rows.length) {
    return `<section class="panel full"><h2>候选详情</h2><div class="empty">当前没有候选详情。</div></section>`;
  }
  const first = rows[0];
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>${escapeHtml(first.target_name || first.target_id || "候选详情")}</h2>
          <p>默认显示排序第一的候选。详细字段来自当前运行输出。</p>
        </div>
        <span class="status-pill ready">候选编号 ${escapeHtml(first.candidate_id || "未提供")}</span>
      </div>
      ${metricRow("目标名称", first.target_name || "未提供")}
      ${metricRow("目标编号", first.target_id || "未提供")}
      ${metricRow("候选范围", friendlyScope(first.candidate_scope))}
      ${metricRow("物种", first.organism || "未提供")}
      ${metricRow("综合分数", first.final_score || "未提供")}
      ${metricRow("证据摘要", first.evidence_summary || "未提供")}
    </section>`;
}

function structureSection(rows) {
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>结构/口袋</h2>
          <p>${rows.length ? "结构和口袋摘要来自当前运行输出。" : "当前运行没有结构或口袋摘要。"}</p>
        </div>
        <span class="status-pill ${rows.length ? "ready" : "blocked"}">${rows.length ? `${rows.length} 行` : "未就绪"}</span>
      </div>
      ${visualStatus([
        { label: "摘要表", value: rows.length ? "已生成" : "未生成", ready: rows.length > 0 },
        { label: "三维查看器", value: "未接入可用输出", ready: false },
        { label: "数据来源", value: rows.length ? "当前运行输出" : "等待运行输出", ready: rows.length > 0 },
      ])}
      ${rows.length ? table(rows, columnsFor("structure")) : `<div class="empty">未生成结构查看数据或口袋摘要。</div>`}
    </section>`;
}

function mdSection(rows) {
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>动力学摘要</h2>
          <p>${rows.length ? "动力学摘要来自当前运行输出。" : "当前运行没有轨迹摘要。"}</p>
        </div>
        <span class="status-pill ${rows.length ? "ready" : "blocked"}">${rows.length ? `${rows.length} 行` : "未就绪"}</span>
      </div>
      ${visualStatus([
        { label: "摘要表", value: rows.length ? "已生成" : "未生成", ready: rows.length > 0 },
        { label: "轨迹图", value: "未接入可用输出", ready: false },
        { label: "数据来源", value: rows.length ? "当前运行输出" : "等待运行输出", ready: rows.length > 0 },
      ])}
      ${rows.length ? table(rows, columnsFor("md")) : `<div class="empty">未生成 RMSD、RMSF、接触或配体漂移摘要。</div>`}
    </section>`;
}

function visualStatus(items) {
  return `
    <div class="visual-status">
      ${items
        .map(
          (item) => `
            <div class="visual-status-item">
              <span>${escapeHtml(item.label)}</span>
              <strong class="${item.ready ? "ready-text" : "blocked-text"}">${escapeHtml(item.value)}</strong>
            </div>`
        )
        .join("")}
    </div>`;
}

function renderExport() {
  page.className = "page";
  const artifacts = state.run?.artifact_index?.items || [];
  const available = artifacts.filter((item) => item.exists);
  const runId = state.run?.run_id || "";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>导出材料</h2>
            <p>导出项只在当前运行生成相应输出后启用。</p>
          </div>
          <span class="status-pill ${available.length ? "ready" : "blocked"}">${available.length ? `${available.length} 项可用` : "暂无可导出项"}</span>
        </div>
      </section>
      ${exportCard("科研报告", "research_report", artifacts, runId)}
      ${exportCard("候选表", "candidate_ranking", artifacts, runId)}
      ${exportCard("证据链", "evidence", artifacts, runId)}
      ${exportCard("复现材料", "normalized_input", artifacts, runId)}
      ${exportCard("结构与口袋摘要", "structure_patch", artifacts, runId)}
      ${exportCard("动力学摘要", "md_summary", artifacts, runId)}
    </div>`;
}

function exportCard(title, artifactId, artifacts, runId) {
  const exists = Boolean(runId) && artifacts.some((item) => item.id === artifactId && item.exists);
  const href = `/api/runs/${encodeURIComponent(runId)}/download/${encodeURIComponent(artifactId)}`;
  return `
    <section class="panel third">
      <h2>${title}</h2>
      <p>${exists ? "当前运行已生成。" : "当前运行未生成。"}</p>
      ${exists ? `<a class="button secondary" href="${href}">下载</a>` : `<button class="button disabled" disabled>不可用</button>`}
    </section>`;
}

function collectForm() {
  page.querySelectorAll("[data-form]").forEach((field) => {
    state.form[field.dataset.form] = field.value;
  });
}

async function saveInput() {
  collectForm();
  if (!hasAnyInput()) {
    state.alert = "请先填写至少一种研究输入。";
    render();
    return false;
  }
  state.alert = "";
  if (!state.project) {
    const created = await api("/api/projects", { method: "POST", body: "{}" });
    state.project = created.project;
  }
  const inputPayload = {
    input_kind: "multi_input_workbench",
    inputs: [
      {
        field: "project_name",
        value: state.form.project_name.trim() || "GenoLigand research project",
      },
      ...filledInputs().map((item) => ({ field: item.field, value: item.value })),
    ],
    files: [],
  };
  await api(`/api/projects/${state.project.project_id}/inputs`, {
    method: "POST",
    body: JSON.stringify(inputPayload),
  });
  state.project = await api(`/api/projects/${state.project.project_id}`);
  setStep("configure");
  return true;
}

async function runPreflight() {
  if (!state.project) {
    state.preflight = null;
    state.alert = "请先保存输入。";
    render();
    return;
  }
  state.preflight = await api(`/api/projects/${state.project.project_id}/preflight`, {
    method: "POST",
  });
  render();
}

async function runSetupRepair() {
  state.alert = "";
  state.setupRepair = { message: "正在运行安装/修复...", database_ready: false };
  render();
  try {
    state.setupRepair = await api("/api/user/setup/run", { method: "POST" });
    [state.databases, state.diagnostics, state.setup] = await Promise.all([
      api("/api/user/databases"),
      api("/api/user/diagnostics"),
      api("/api/user/setup/status"),
    ]);
    if (state.project) {
      state.preflight = await api(`/api/projects/${state.project.project_id}/preflight`, {
        method: "POST",
      });
    }
  } catch (error) {
    state.alert = `修复动作失败：${error.message}`;
  }
  render();
}

async function startRun() {
  if (!state.project) {
    const saved = await saveInput();
    if (!saved) return;
  }
  if (!state.preflight) {
    await runPreflight();
  }
  const groups = preflightGroups();
  const allReady = groups.every((group) => group.rows.every((row) => row.ready));
  if (!allReady) {
    state.alert = "预检查未通过，运行已阻止。";
    setStep("check");
    return;
  }
  state.alert = "";
  const result = await api(`/api/projects/${state.project.project_id}/runs`, { method: "POST" });
  state.run = result.run;
  state.project = await api(`/api/projects/${state.project.project_id}`);
  await loadRunData();
  setStep("run");
}

async function loadRunData() {
  if (!state.run?.run_id) {
    state.runData = null;
    return;
  }
  const runId = state.run.run_id;
  const [candidates, evidence, genome, structure, md, logs, artifacts] = await Promise.all([
    api(`/api/runs/${runId}/candidates`),
    api(`/api/runs/${runId}/evidence`),
    api(`/api/runs/${runId}/genome-context`),
    api(`/api/runs/${runId}/structure`),
    api(`/api/runs/${runId}/md-summary`),
    api(`/api/runs/${runId}/logs`),
    api(`/api/runs/${runId}/artifacts`),
  ]);
  state.runData = { candidates, evidence, genome, structure, md, logs, artifacts };
}

function hasAnyInput() {
  return filledInputs().length > 0;
}

function filledInputs() {
  return inputSpecs
    .map((spec) => ({
      kind: spec.kind,
      field: spec.field,
      value: (state.form[spec.field] || "").trim(),
    }))
    .filter((item) => item.value);
}

function sourceReady(key) {
  const required = state.databases?.required || [];
  if (key === "known") return required.some((row) => row.id === "known_database" && row.ready);
  if (key === "metagenome") return required.some((row) => row.id === "metagenome" && row.ready);
  if (key === "structure") return required.some((row) => row.id === "tools" && row.ready);
  return false;
}

function storeStatusRows() {
  const required = state.databases?.required || [];
  if (!required.length) return `<div class="empty">尚未读取到科学仓库状态。</div>`;
  return required
    .map((row) =>
      metricRow(row.label || friendlyRequirement(row.id), row.ready ? "就绪" : "需要安装或修复")
    )
    .join("");
}

function preflightGroups() {
  const byId = new Map((state.preflight?.checks || []).map((row) => [row.id, row]));
  const inputReady = state.project && byId.get("input")?.ready;
  return [
    {
      title: "输入",
      rows: [
        {
          label: "研究输入",
          ready: Boolean(inputReady),
          next: state.project ? "保存有效输入" : "先保存输入",
        },
      ],
    },
    {
      title: "数据库和索引",
      rows: [
        checkRow(byId, "science_store", "科学仓库", "运行安装/修复"),
        checkRow(byId, "known_database", "已知蛋白索引", "准备数据库索引"),
        checkRow(byId, "metagenome", "宏基因组索引", "准备序列索引"),
      ],
    },
    {
      title: "运行环境",
      rows: [checkRow(byId, "tools", "计算工具", "准备运行环境")],
    },
    {
      title: "存储",
      rows: [
        {
          label: "项目空间",
          ready: Boolean(state.project),
          next: "保存输入后创建",
        },
      ],
    },
    {
      title: "导出",
      rows: [
        {
          label: "报告输出",
          ready: Boolean(state.project),
          next: "完成运行后可导出",
        },
      ],
    },
  ];
}

function checkRow(byId, id, label, next) {
  const row = byId.get(id);
  return { label, ready: Boolean(row?.ready), next };
}

function canStartRun() {
  const groups = preflightGroups();
  return Boolean(state.project) && groups.every((group) => group.rows.every((row) => row.ready));
}

function nextActionText() {
  if (!state.project) return "填写并保存输入";
  if (!state.preflight) return "执行预检查";
  if (!canStartRun()) return "处理预检查阻塞项";
  if (!state.run) return "开始运行";
  if (state.run.state === "completed") return "查看结果或导出";
  if (state.run.state === "blocked") return "查看阻塞原因";
  if (state.run.state === "failed") return "查看日志并修复";
  return "等待当前阶段完成";
}

function candidateRows() {
  return state.runData?.candidates?.candidates || [];
}

function artifactRows() {
  const items = state.run?.artifact_index?.items || state.runData?.artifacts?.items || [];
  if (!items.length) return `<div class="empty">还没有运行输出。</div>`;
  return `
    <div class="table-wrap">
      <table>
        <thead><tr><th>输出</th><th>状态</th></tr></thead>
        <tbody>
          ${items.map((item) => `<tr><td>${friendlyArtifact(item.id || item.path)}</td><td><span class="status-pill ${item.exists ? "ready" : "blocked"}">${item.exists ? "已生成" : "未生成"}</span></td></tr>`).join("")}
        </tbody>
      </table>
    </div>`;
}

function table(rows, columns) {
  const keys = columns.filter((column) => rows.some((row) => row[column.key] !== undefined));
  if (!keys.length) return `<div class="empty">当前输出没有可显示字段。</div>`;
  return `
    <div class="table-wrap">
      <table>
        <thead><tr>${keys.map((column) => `<th>${escapeHtml(column.label)}</th>`).join("")}</tr></thead>
        <tbody>
          ${rows.map((row) => `<tr>${keys.map((column) => `<td>${escapeHtml(formatCell(column.key, row[column.key]))}</td>`).join("")}</tr>`).join("")}
        </tbody>
      </table>
    </div>`;
}

function columnsFor(kind) {
  const common = [
    { key: "rank", label: "排序" },
    { key: "candidate_id", label: "候选编号" },
    { key: "target_id", label: "目标编号" },
    { key: "target_name", label: "目标名称" },
    { key: "candidate_scope", label: "候选范围" },
    { key: "organism", label: "物种" },
    { key: "final_score", label: "综合分数" },
    { key: "known_database_score", label: "已知库分数" },
    { key: "metagenome_score", label: "宏基因组分数" },
    { key: "structure_score", label: "结构分数" },
    { key: "docking_score", label: "对接分数" },
    { key: "md_score", label: "动力学分数" },
    { key: "evidence_summary", label: "证据摘要" },
    { key: "status", label: "状态" },
  ];
  if (kind === "ranking") return common;
  if (kind === "evidence") {
    return [
      { key: "claim_id", label: "证据编号" },
      { key: "owner", label: "来源" },
      { key: "status", label: "状态" },
      { key: "notes", label: "说明" },
    ];
  }
  if (kind === "genome") {
    return [
      { key: "source", label: "来源" },
      { key: "target_id", label: "目标编号" },
      { key: "target_name", label: "目标名称" },
      { key: "organism", label: "物种" },
      { key: "target_class", label: "类别" },
      { key: "evidence", label: "证据" },
    ];
  }
  if (kind === "structure") {
    return [
      { key: "candidate_id", label: "候选编号" },
      { key: "target_id", label: "目标编号" },
      { key: "structure_score", label: "结构分数" },
      { key: "docking_score", label: "对接分数" },
      { key: "evidence_summary", label: "结构证据" },
      { key: "status", label: "状态" },
    ];
  }
  if (kind === "md") {
    return [
      { key: "candidate_id", label: "候选编号" },
      { key: "target_id", label: "目标编号" },
      { key: "md_score", label: "动力学分数" },
      { key: "rmsd", label: "RMSD" },
      { key: "contact_occupancy", label: "接触占有率" },
      { key: "status", label: "状态" },
    ];
  }
  return common;
}

function metricRow(label, value) {
  return `<div class="metric-row"><div class="metric-label">${escapeHtml(label)}</div><div>${escapeHtml(value)}</div></div>`;
}

function runStateLabel(value) {
  return {
    not_started: "未开始",
    created: "已创建",
    running: "运行中",
    completed: "已完成",
    blocked: "已阻塞",
    failed: "失败",
    cancelled: "已取消",
    resumable: "可恢复",
  }[value] || value || "未开始";
}

function runStateClass(value) {
  if (value === "completed") return "ready";
  if (value === "failed") return "failed";
  return "blocked";
}

function friendlyRequirement(item) {
  return {
    science_store: "科学仓库",
    known_database: "已知蛋白索引",
    metagenome: "宏基因组索引",
    tools: "计算工具",
  }[item] || item;
}

function friendlyArtifact(name) {
  return {
    run_summary: "运行摘要",
    run_table: "运行表格",
    source_trace: "来源追溯",
    normalized_input: "输入归一化记录",
    features: "特征计算记录",
    known_matches: "已知数据库命中",
    metagenome_matches: "宏基因组潜在命中",
    structure_patch: "结构与口袋摘要",
    docking: "对接摘要",
    md_summary: "动力学摘要",
    candidate_ranking: "候选排序",
    candidate_detail: "候选详情",
    score_breakdown: "分数拆解",
    evidence: "证据链",
    research_report: "科研报告",
  }[name] || name;
}

function friendlyScope(value) {
  return {
    known_database: "已知数据库",
    metagenome_potential: "宏基因组潜在序列",
    structure_interaction: "结构互作",
  }[value] || value || "未提供";
}

function formatCell(key, value) {
  if (value === undefined || value === null || value === "") return "未提供";
  if (key === "candidate_scope") return friendlyScope(value);
  if (key === "status") return runStateLabel(value);
  return value;
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function escapeAttr(value) {
  return escapeHtml(value).replaceAll("'", "&#39;");
}

boot().catch((error) => {
  page.className = "page";
  page.innerHTML = `<section class="panel full"><h2>服务连接失败</h2><div class="notice bad">${escapeHtml(error.message)}</div></section>`;
});
