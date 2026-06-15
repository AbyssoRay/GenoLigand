const state = {
  step: "input",
  selectedInput: "small_molecule",
  project: null,
  run: null,
  health: null,
  databases: null,
  diagnostics: null,
  setup: null,
  setupPlan: null,
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
    placeholder: "粘贴 SMILES，或填写本机结构文件路径",
    minimum: "至少提供一个可解析的分子表示。",
  },
  {
    kind: "peptide",
    title: "多肽",
    field: "peptide",
    label: "短肽序列、修饰说明或结构文件",
    placeholder: "输入肽序列；修饰说明可另起一行填写",
    minimum: "至少提供肽序列或结构文件路径。",
  },
  {
    kind: "protein_ligand",
    title: "蛋白质配体",
    field: "protein_ligand",
    label: "FASTA、蛋白序列或结构文件",
    placeholder: "粘贴 FASTA、蛋白序列，或填写结构文件路径",
    minimum: "至少提供序列或结构文件路径。",
  },
  {
    kind: "complex",
    title: "复合物结构",
    field: "complex",
    label: "蛋白-配体复合物结构、链信息或结合区域",
    placeholder: "填写 PDB/mmCIF 文件路径，并注明链或配体",
    minimum: "至少提供结构文件路径或复合物描述。",
  },
  {
    kind: "genome_or_metagenome",
    title: "自定义基因组",
    field: "genome_or_metagenome",
    label: "基因组、宏基因组、转录组或蛋白序列文件",
    placeholder: "填写 FASTA/FASTQ 文件路径，或粘贴蛋白序列",
    minimum: "至少提供序列文件路径或序列文本。",
  },
];

const stepMeta = {
  input: ["输入研究对象", "填写任一种研究输入，空输入不会生成候选。"],
  configure: ["配置搜索范围", "选择本次计算要覆盖的证据类型和输出。"],
  check: ["检查运行条件", "输入、数据库、运行环境、存储和导出条件都通过后再运行。"],
  run: ["运行中心", "查看当前阶段、下一步动作、资源状态和日志。"],
  results: ["查看结果", "候选排序、详情、证据、结构和动力学结果只来自当前运行输出。"],
  export: ["导出", "整理报告、候选表和证据摘要。"],
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
    [state.health, state.databases, state.diagnostics, state.setup, state.setupPlan] = await Promise.all([
      api("/api/user/health"),
      api("/api/user/databases"),
      api("/api/user/diagnostics"),
      api("/api/user/setup/status"),
      api("/api/user/setup/install-plan"),
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
  const activeValidation = inputValidation(active);
  const saveDisabled = inputsReadyToSave() ? "" : "disabled";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>选择输入类型</h2>
            <p>可以填写一种或多种输入。只有用户填写的内容会进入计算。</p>
          </div>
          <span class="status-pill ${inputStatusClass()}" data-input-status>${inputStatusLabel()}</span>
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
          <div class="field-help ${activeValidation.ready ? "ready-text" : activeValidation.value ? "blocked-text" : ""}" data-active-validation>${escapeHtml(activeValidation.message)}</div>
        </div>
        <div class="actions">
          <button class="button" data-action="save-input" data-save-button ${saveDisabled}>保存输入</button>
          <button class="button secondary" data-action="save-and-check" data-save-button ${saveDisabled}>保存并检查</button>
          <button class="button ghost" data-action="clear-input">清空当前输入</button>
        </div>
      </section>
      <section class="panel third">
        <h2>输入状态</h2>
        ${metricRow("填写类型", `${filledInputs().length} / ${inputSpecs.length}`)}
        ${metricRow("可用输入", `${usableInputs().length} / ${inputSpecs.length}`)}
        ${metricRow("候选显示", "运行完成前不显示候选")}
        ${metricRow("结果来源", "仅读取当前项目运行输出")}
      </section>
      <section class="panel third">
        <h2>下一步</h2>
        <p>${inputsReadyToSave() ? "保存后进入配置和检查。" : "先完成当前输入检查。"}</p>
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
  const value = state.form[spec.field].trim();
  const filled = value.length > 0;
  const usable = inputValueUsable(spec.field, value);
  return `
    <button class="input-card ${selected ? "selected" : ""}" data-select-input="${spec.kind}">
      <h3>${spec.title}</h3>
      <p>${spec.minimum}</p>
      <span class="status-pill ${inputCardStatusClass(usable, filled)}" data-input-card-status="${spec.field}">${usable ? "可用" : filled ? "需检查" : "未填写"}</span>
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
    field.addEventListener("input", () => {
      collectForm();
      updateInputValidationUi();
    });
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

function updateInputValidationUi() {
  const active = inputSpecs.find((item) => item.kind === state.selectedInput) || inputSpecs[0];
  const validation = inputValidation(active);
  const status = page.querySelector("[data-input-status]");
  if (status) {
    status.textContent = inputStatusLabel();
    status.className = `status-pill ${inputStatusClass()}`;
  }
  const activeHelp = page.querySelector("[data-active-validation]");
  if (activeHelp) {
    activeHelp.textContent = validation.message;
    activeHelp.className = `field-help ${validation.ready ? "ready-text" : validation.value ? "blocked-text" : ""}`;
  }
  page.querySelectorAll("[data-save-button]").forEach((button) => {
    button.disabled = !inputsReadyToSave();
  });
  inputSpecs.forEach((spec) => {
    const statusNode = page.querySelector(`[data-input-card-status="${spec.field}"]`);
    if (!statusNode) return;
    const value = (state.form[spec.field] || "").trim();
    const usable = inputValueUsable(spec.field, value);
    statusNode.textContent = usable ? "可用" : value ? "需检查" : "未填写";
    statusNode.className = `status-pill ${inputCardStatusClass(usable, Boolean(value))}`;
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
          <span class="status-pill ${state.project ? "ready" : "waiting"}">${state.project ? "已保存输入" : "等待输入"}</span>
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
        ${installCatalogSummary()}
        ${installerDownloadSummary()}
        ${installerExternalSourceSummary()}
        ${installerResourceSummary()}
        ${missingResourceSummary()}
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

function installCatalogSummary() {
  const direct = state.setupPlan?.direct_sources;
  const external = state.setupPlan?.external_sources;
  if (!direct && !external) return "";
  const directReady = direct?.ready === true;
  const externalReady = external?.ready === true;
  return `
    <div class="notice ${directReady && externalReady ? "" : "warn"}">
      <strong>安装准备：${direct?.source_count || 0} 个数据库来源，${external?.source_group_count || 0} 组大型资源</strong>
      <div class="compact-list">下一步：${escapeHtml((directReady && externalReady) ? "运行安装/修复" : "使用完整安装包重新运行安装/修复")}</div>
    </div>`;
}

function installerDownloadSummary() {
  const downloads = state.setupRepair?.installer?.downloads;
  if (!downloads) return "";
  const ok = downloads.completed === true;
  const message = downloads.message || (ok ? "科学资源下载步骤已完成。" : "科学资源下载状态需要检查。");
  const next = downloads.next_action || (ok ? "继续" : "重新运行安装/修复");
  return `
    <div class="notice ${ok ? "" : "warn"}">
      <strong>资源下载：${escapeHtml(message)}</strong>
      <div class="compact-list">下一步：${escapeHtml(next)}</div>
    </div>`;
}

function installerExternalSourceSummary() {
  const external = state.setupRepair?.installer?.external_sources;
  const items = external?.items || [];
  if (!external || !items.length) return "";
  return `
    <div class="notice ${external.needs_action ? "warn" : ""}">
      <strong>大型资源：${external.ready || 0} / ${external.total || items.length} 就绪</strong>
      <ul class="compact-list">
        ${items
          .map(
            (item) =>
              `<li>${escapeHtml(item.label || "科学资源")}：${escapeHtml(item.status || "需要同步")}；${escapeHtml(item.next_action || "运行安装/修复")}</li>`
          )
          .join("")}
      </ul>
    </div>`;
}

function installerResourceSummary() {
  const resources = state.setupRepair?.installer?.resources;
  const items = resources?.items || [];
  if (!resources || !items.length) return "";
  return `
    <div class="notice ${resources.needs_action ? "warn" : ""}">
      <strong>科学资源：${resources.ready || 0} / ${resources.total || items.length} 就绪</strong>
      <ul class="compact-list">
        ${items
          .map(
            (item) =>
              `<li>${escapeHtml(item.label || "科学资源")}：${escapeHtml(item.status || "需要检查")}；${escapeHtml(item.next_action || "运行安装/修复")}</li>`
          )
          .join("")}
      </ul>
    </div>`;
}

function missingResourceSummary() {
  const missing = state.setupRepair?.missing || state.setup?.missing || state.databases?.missing || [];
  if (!missing.length) {
    return `<div class="notice">数据库、索引和工具记录已满足当前运行要求。</div>`;
  }
  return `
    <div class="notice warn">
      <strong>还需要准备 ${missing.length} 项：</strong>
      <ul class="compact-list">
        ${missing
          .map(
            (item) =>
              `<li>${escapeHtml(item.label || "科学资源")}：${escapeHtml(item.next_action || "运行安装/修复")}</li>`
          )
          .join("")}
      </ul>
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
    return `<div class="empty">尚未读取到数据库状态。运行安装/修复后会显示每个资源的准备情况。</div>`;
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
        ${metricRow("候选数量", candidateCountText())}
        ${metricRow("阻塞原因", state.run?.failure_summary || "无")}
      </section>
      <section class="panel">
        <h2>资源计划</h2>
        ${metricRow("候选量", candidateForecastText())}
        ${metricRow("时间等级", runtimeClassText())}
        ${metricRow("GPU 建议", gpuAdviceText())}
        ${metricRow("CPU 路径", cpuPathText())}
        ${metricRow("磁盘需求", storageNeedText())}
      </section>
      <section class="panel">
        <h2>当前资源</h2>
        ${resourceStatusRows()}
      </section>
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>恢复建议</h2>
            <p>如果运行被阻塞，先处理这里列出的下一步，再重新检查。</p>
          </div>
          <span class="status-pill ${blockedActionRows().length ? "blocked" : "ready"}">${blockedActionRows().length ? "需要处理" : "无需处理"}</span>
        </div>
        ${failureRecoveryRows()}
      </section>
      <section class="panel full">
        <h2>可导出结果</h2>
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
  const waiting = !completed && !active;
  return `
    <div class="stage">
      <span class="stage-index">${index + 1}</span>
      <span>${label}</span>
      <span class="status-pill ${completed ? "ready" : active ? "loading" : waiting ? "waiting" : "blocked"}">${completed ? "完成" : active ? "进行中" : "等待"}</span>
    </div>`;
}

function renderResults() {
  page.className = "page";
  const candidates = candidateRows();
  const candidateDetails = state.runData?.detail?.candidate_details || [];
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
      if (tab === "detail") body.innerHTML = candidateDetailSection(candidateDetails);
      if (tab === "evidence") body.innerHTML = resultSection("证据链", evidence, "evidence");
      if (tab === "genome") body.innerHTML = resultSection("基因组/ORF", genome, "genome");
      if (tab === "structure") body.innerHTML = structureSection(structure);
      if (tab === "md") body.innerHTML = mdSection(md);
    });
  });
}

function resultTracePanel() {
  const trace = state.run?.trace || {};
  const projectText = trace.project_label || state.project?.project_name || (state.project ? "当前项目" : "未创建");
  const runText = state.run ? `${trace.run_label || "当前运行"}：${runStateLabel(state.run.state)}` : "未运行";
  const databaseText = trace.database_status || "未检查";
  const outputText = trace.output_status
    ? `${trace.output_status}${Number.isFinite(trace.output_count) ? `（${trace.output_count} 项）` : ""}`
    : "暂无输出";
  const candidateText = trace.candidate_status || (candidateRows().length ? "已生成候选排序" : "暂无候选排序");
  const evidenceText = trace.evidence_status || (state.runData?.evidence?.has_output ? "已生成证据链" : "暂无证据链");
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>结果来源</h2>
          <p>结果只读取当前项目和当前运行的输出；没有输出时保持为空。</p>
        </div>
        <span class="status-pill ${state.run ? "ready" : "waiting"}">${state.run ? "已选择运行" : "未运行"}</span>
      </div>
      ${metricRow("当前项目", projectText)}
      ${metricRow("当前运行", runText)}
      ${metricRow("输入来源", trace.input_status || (state.project ? "已保存用户输入" : "未保存用户输入"))}
      ${metricRow("数据库来源", databaseText)}
      ${metricRow("输出材料", outputText)}
      ${metricRow("候选来源", candidateText)}
      ${metricRow("证据来源", evidenceText)}
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
        <span class="status-pill ${outputStatusClass(rows)}">${rows.length ? `${rows.length} 行` : "空"}</span>
      </div>
      ${rows.length ? table(rows, columnsFor(kind)) : `<div class="empty">${emptyText}</div>`}
    </section>`;
}

function candidateDetailSection(rows) {
  if (!rows.length) {
    return `<section class="panel full"><h2>候选详情</h2><div class="empty">当前没有候选详情输出。完成运行并生成候选详情后才会显示。</div></section>`;
  }
  const first = rows[0];
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>第一候选摘要</h2>
          <p>摘要来自当前运行的候选详情输出；下方表格显示全部候选详情。</p>
        </div>
        <span class="status-pill ready">候选编号 ${escapeHtml(valueOrMissing(first.candidate_id))}</span>
      </div>
      ${metricRow("目标名称", valueOrMissing(first.target_name))}
      ${metricRow("目标编号", valueOrMissing(first.target_id))}
      ${metricRow("候选范围", friendlyScope(first.candidate_scope))}
      ${metricRow("物种", valueOrMissing(first.organism))}
      ${metricRow("综合分数", valueOrMissing(first.final_score))}
      ${metricRow("证据摘要", valueOrMissing(first.evidence_summary))}
      ${metricRow("详情来源", valueOrMissing(first.detail_source, "当前运行输出"))}
    </section>
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>全部候选详情</h2>
          <p>每一行都来自当前运行生成的候选详情输出。</p>
        </div>
        <span class="status-pill ready">${rows.length} 行</span>
      </div>
      ${table(rows, columnsFor("candidate_detail"))}
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
        <span class="status-pill ${outputStatusClass(rows)}">${rows.length ? `${rows.length} 行` : "未就绪"}</span>
      </div>
      ${visualStatus([
        { label: "摘要表", value: rows.length ? "已生成" : "未生成", ready: rows.length > 0 },
        { label: "三维查看器", value: "未接入可用输出", ready: false, waiting: true },
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
        <span class="status-pill ${outputStatusClass(rows)}">${rows.length ? `${rows.length} 行` : "未就绪"}</span>
      </div>
      ${visualStatus([
        { label: "摘要表", value: rows.length ? "已生成" : "未生成", ready: rows.length > 0 },
        { label: "轨迹图", value: "未接入可用输出", ready: false, waiting: true },
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
              <strong class="${item.ready ? "ready-text" : item.waiting ? "waiting-text" : "blocked-text"}">${escapeHtml(item.value)}</strong>
            </div>`
        )
        .join("")}
    </div>`;
}

function renderExport() {
  page.className = "page";
  const artifacts = state.run?.outputs?.items || [];
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
          <span class="status-pill ${outputStatusClass(available)}">${available.length ? `${available.length} 项可用` : "暂无可导出项"}</span>
        </div>
      </section>
      ${exportCard("科研报告", "research_report", artifacts, runId)}
      ${exportCard("候选表", "candidate_ranking", artifacts, runId)}
      ${exportCard("证据链", "evidence", artifacts, runId)}
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
  if (!inputsReadyToSave()) {
    state.alert = "请先填写至少一个可识别的研究输入，并处理所有已填写字段的提示。";
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
        value: state.form.project_name.trim() || "未命名研究项目",
      },
      ...filledInputs().map((item) => ({ field: item.field, value: item.value })),
    ],
    files: [],
  };
  const saved = await api(`/api/projects/${state.project.project_id}/inputs`, {
    method: "POST",
    body: JSON.stringify(inputPayload),
  });
  if (saved.status !== "saved") {
    state.project = saved.project || state.project;
    state.alert = saved.message || "输入未保存，请检查填写内容。";
    render();
    return false;
  }
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
  const [candidates, detail, evidence, genome, structure, md, logs, artifacts] = await Promise.all([
    api(`/api/runs/${runId}/candidates`),
    api(`/api/runs/${runId}/candidate-detail`),
    api(`/api/runs/${runId}/evidence`),
    api(`/api/runs/${runId}/genome-context`),
    api(`/api/runs/${runId}/structure`),
    api(`/api/runs/${runId}/md-summary`),
    api(`/api/runs/${runId}/logs`),
    api(`/api/runs/${runId}/artifacts`),
  ]);
  state.runData = { candidates, detail, evidence, genome, structure, md, logs, artifacts };
}

function hasAnyInput() {
  return filledInputs().length > 0;
}

function hasUsableInput() {
  return usableInputs().length > 0;
}

function inputsReadyToSave() {
  const filled = filledInputs();
  return filled.length > 0 && filled.every((item) => inputValueUsable(item.field, item.value));
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

function usableInputs() {
  return filledInputs().filter((item) => inputValueUsable(item.field, item.value));
}

function inputStatusLabel() {
  if (inputsReadyToSave()) return "输入可保存";
  if (hasAnyInput()) return "需要检查输入";
  return "等待输入";
}

function inputStatusClass() {
  if (inputsReadyToSave()) return "ready";
  if (hasAnyInput()) return "blocked";
  return "waiting";
}

function inputCardStatusClass(usable, filled) {
  if (usable) return "ready";
  if (filled) return "blocked";
  return "waiting";
}

function outputStatusClass(rows) {
  return rows.length ? "ready" : "waiting";
}

function inputValidation(spec) {
  const value = (state.form[spec.field] || "").trim();
  if (!value) {
    return { ready: false, value: "", message: spec.minimum };
  }
  if (inputValueUsable(spec.field, value)) {
    return { ready: true, value, message: "该输入可进入检查。" };
  }
  return {
    ready: false,
    value,
    message: "请填写可识别的序列、结构文件路径、分子表示或登记名称。",
  };
}

function inputValueUsable(field, value) {
  const text = String(value || "").trim();
  if (!text) return false;
  if (field === "small_molecule") {
    return looksLikePathWithExt(text, ["sdf", "mol2", "mol", "pdb", "cif", "mmcif"]) || alnumCount(text) >= 1;
  }
  if (field === "peptide") {
    return looksLikePathWithExt(text, ["fasta", "fa", "pdb", "cif", "mmcif"]) || alnumCount(text) >= 2;
  }
  if (field === "protein_ligand") {
    return looksLikePathWithExt(text, ["fasta", "fa", "faa", "pdb", "cif", "mmcif"]) || looksLikeSequence(text, 5);
  }
  if (field === "complex") {
    return looksLikePathWithExt(text, ["pdb", "cif", "mmcif", "ent"]) || alnumCount(text) >= 5;
  }
  if (field === "genome_or_metagenome") {
    return (
      looksLikePathWithExt(text, ["fasta", "fa", "fna", "faa", "fastq", "fq", "gb", "gff", "gtf", "vcf"]) ||
      looksLikeSequence(text, 8)
    );
  }
  return alnumCount(text) >= 2;
}

function looksLikePathWithExt(value, extensions) {
  const lower = value.toLowerCase();
  return extensions.some((ext) => lower.endsWith(`.${ext}`) || lower.includes(`.${ext}\n`));
}

function looksLikeSequence(value, minLength) {
  const body = value.trim().startsWith(">")
    ? value
        .split(/\r?\n/)
        .slice(1)
        .join("")
    : value;
  return alnumCount(body) >= minLength;
}

function alnumCount(value) {
  return Array.from(String(value || "")).filter((char) => /[\p{Letter}\p{Number}]/u.test(char)).length;
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

function candidateCountText() {
  const count = candidateRows().length;
  if (count > 0) return `${count}`;
  if (state.run?.state === "completed") return "0";
  return "运行完成后计算";
}

function candidateForecastText() {
  if (!state.project) return "保存输入后评估";
  if (!state.preflight) return "预检查后评估";
  if (!canStartRun()) return "资源补齐后评估";
  return "由数据库命中和结构证据决定";
}

function runtimeClassText() {
  if (!state.project) return "未评估";
  const filled = usableInputs().length;
  const structureReady = sourceReady("structure");
  if (filled > 2 || structureReady) return "中到高";
  return "低到中";
}

function gpuAdviceText() {
  if (!sourceReady("structure")) return "结构和动力学工具未就绪时不启动相关计算";
  return "可用于对接和短动力学筛选";
}

function cpuPathText() {
  if (!state.project) return "等待输入";
  return "输入整理、特征计算和数据库比对可先执行";
}

function storageNeedText() {
  const available = state.setup?.database_ready === true;
  if (available) return "科学仓库已由安装流程维护";
  return "先运行安装/修复以准备数据库和索引";
}

function resourceStatusRows() {
  const rows = [
    ["研究输入", state.project ? "已保存" : "未保存"],
    ["科学仓库", sourceReady("known") || sourceReady("metagenome") ? "已读取状态" : "需要准备"],
    ["已知蛋白索引", sourceReady("known") ? "就绪" : "需要准备"],
    ["宏基因组索引", sourceReady("metagenome") ? "就绪" : "需要准备"],
    ["结构/动力学工具", sourceReady("structure") ? "就绪" : "需要准备"],
  ];
  return rows.map(([label, value]) => metricRow(label, value)).join("");
}

function blockedActionRows() {
  return preflightGroups()
    .flatMap((group) =>
      group.rows
        .filter((row) => !row.ready)
        .map((row) => ({
          group: group.title,
          label: row.label,
          next: row.next,
        }))
    );
}

function failureRecoveryRows() {
  const rows = blockedActionRows();
  if (!rows.length) {
    return `<div class="empty">当前没有阻塞项。可以开始运行，或等待当前运行完成后查看结果。</div>`;
  }
  return table(
    rows,
    [
      { key: "group", label: "分组" },
      { key: "label", label: "项目" },
      { key: "next", label: "下一步" },
    ],
  );
}

function candidateRows() {
  return state.runData?.candidates?.candidates || [];
}

function artifactRows() {
  const items = state.run?.outputs?.items || state.runData?.artifacts?.items || [];
  if (!items.length) return `<div class="empty">还没有运行输出。</div>`;
  return `
    <div class="table-wrap">
      <table>
        <thead><tr><th>输出</th><th>状态</th></tr></thead>
        <tbody>
          ${items.map((item) => `<tr><td>${friendlyArtifact(item.id || item.path)}</td><td><span class="status-pill ${item.exists ? "ready" : "waiting"}">${item.exists ? "已生成" : "未生成"}</span></td></tr>`).join("")}
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
    { key: "result_source", label: "输出材料" },
  ];
  if (kind === "ranking") return common;
  if (kind === "candidate_detail") {
    return [
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
      { key: "detail_source", label: "详情来源" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  if (kind === "evidence") {
    return [
      { key: "evidence_type", label: "证据类型" },
      { key: "source", label: "来源" },
      { key: "status", label: "状态" },
      { key: "summary", label: "说明" },
      { key: "result_source", label: "输出材料" },
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
      { key: "result_source", label: "输出材料" },
    ];
  }
  if (kind === "structure") {
    return [
      { key: "candidate_id", label: "候选编号" },
      { key: "target_id", label: "目标编号" },
      { key: "target_name", label: "目标名称" },
      { key: "structure_score", label: "结构分数" },
      { key: "docking_score", label: "对接分数" },
      { key: "summary", label: "结构证据" },
      { key: "status", label: "状态" },
      { key: "viewer_status", label: "查看器" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  if (kind === "md") {
    return [
      { key: "candidate_id", label: "候选编号" },
      { key: "target_id", label: "目标编号" },
      { key: "target_name", label: "目标名称" },
      { key: "md_score", label: "动力学分数" },
      { key: "rmsd", label: "RMSD" },
      { key: "rmsf", label: "RMSF" },
      { key: "ligand_drift", label: "配体漂移" },
      { key: "contact_occupancy", label: "接触占有率" },
      { key: "summary", label: "摘要" },
      { key: "status", label: "状态" },
      { key: "trajectory_status", label: "轨迹图" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  return common;
}

function metricRow(label, value) {
  return `<div class="metric-row"><div class="metric-label">${escapeHtml(label)}</div><div>${escapeHtml(value)}</div></div>`;
}

function valueOrMissing(value, fallback = "未提供") {
  return value === undefined || value === null || value === "" ? fallback : value;
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
  if (value === "running") return "loading";
  if (value === "failed") return "failed";
  if (value === "blocked") return "blocked";
  return "waiting";
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
    structure_patch: "结构与口袋摘要",
    md_summary: "动力学摘要",
    candidate_ranking: "候选排序",
    evidence: "证据链",
    research_report: "科研报告",
  }[name] || name;
}

function friendlyScope(value) {
  return {
    known_database: "已知蛋白数据库",
    known_human_protein: "已知蛋白数据库",
    metagenome_potential: "宏基因组潜在序列",
    metagenome_potential_protein: "宏基因组潜在序列",
    structure_interaction: "结构互作",
  }[value] || value || "未提供";
}

function formatCell(key, value) {
  if (value === undefined || value === null || value === "") return "未提供";
  if (key === "candidate_scope") return friendlyScope(value);
  if (key === "status") return friendlyStatus(value);
  return value;
}

function friendlyStatus(value) {
  return {
    not_started: "未开始",
    created: "已创建",
    running: "运行中",
    completed: "已完成",
    blocked: "已阻塞",
    failed: "失败",
    cancelled: "已取消",
    resumable: "可恢复",
    computed_prediction: "计算候选",
    ready: "就绪",
    missing: "缺失",
    unavailable: "不可用",
  }[value] || value || "未提供";
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
