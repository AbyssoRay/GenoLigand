const state = {
  route: "workflow",
  project: null,
  run: null,
  health: null,
  databases: null,
};

const $ = (selector) => document.querySelector(selector);
const page = $("[data-page]");

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...options,
  });
  return response.json();
}

function setRoute(route) {
  state.route = route;
  document.querySelectorAll(".nav").forEach((button) => {
    button.classList.toggle("active", button.dataset.route === route);
  });
  render();
}

async function boot() {
  document.querySelectorAll(".nav").forEach((button) => {
    button.addEventListener("click", () => setRoute(button.dataset.route));
  });
  state.health = await api("/api/health");
  state.databases = await api("/api/databases");
  const projects = await api("/api/projects");
  state.project = projects.projects?.[0] || null;
  if (state.project?.latest_run_id) {
    state.run = await api(`/api/runs/${state.project.latest_run_id}`);
  }
  render();
}

function render() {
  $("[data-health]").textContent = state.health?.status || "未连接";
  $("[data-health]").className = `pill ${state.health?.status === "ready" ? "ready" : "blocked"}`;
  const title = {
    workflow: "科研主流程",
    results: "计算结果",
    system: "系统状态",
  }[state.route];
  $("[data-title]").textContent = title;
  $("[data-subtitle]").textContent = "输入由用户提供；数据库、索引、工具和运行环境由安装流程准备。";
  if (state.route === "workflow") renderWorkflow();
  if (state.route === "results") renderResults();
  if (state.route === "system") renderSystem();
}

function renderWorkflow() {
  page.innerHTML = `
    <div class="grid">
      <section class="card wide">
        <h2>统一流程</h2>
        <p>输入归一化 -> 特征计算 -> 已知数据库比对 -> 宏基因组潜在序列比对 -> 结构/口袋/对接/动力学证据 -> 联合靶点排序 -> 证据报告。</p>
        <div class="actions">
          <button class="button" data-create>创建项目</button>
          <button class="button secondary" data-preflight ${state.project ? "" : "disabled"}>预检查</button>
          <button class="button" data-run ${state.project ? "" : "disabled"}>开始计算</button>
        </div>
      </section>
      ${inputCard("small_molecule", "小分子", "SMILES / SDF / MOL2 / 名称 / 登记号", "smiles")}
      ${inputCard("peptide", "多肽", "短肽序列 / 修饰说明 / 结构文件", "peptide")}
      ${inputCard("protein_ligand", "蛋白质配体", "FASTA / 蛋白序列 / 结构文件", "protein")}
      ${inputCard("complex", "复合物", "蛋白-配体结构 / 链 / 结合区域", "complex")}
      ${inputCard("genome_or_metagenome", "自定义基因组/宏基因组", "基因组 / 宏基因组 / 转录组 / 蛋白序列文件", "genome")}
      <section class="card wide">
        <h2>当前项目</h2>
        <pre>${escapeHtml(JSON.stringify(state.project || {status:"waiting_for_project"}, null, 2))}</pre>
      </section>
    </div>`;
  page.querySelector("[data-create]").addEventListener("click", createProject);
  page.querySelector("[data-preflight]")?.addEventListener("click", runPreflight);
  page.querySelector("[data-run]")?.addEventListener("click", startRun);
  page.querySelectorAll("[data-save-input]").forEach((button) => {
    button.addEventListener("click", () => saveInput(button.dataset.kind, button.dataset.field));
  });
}

function inputCard(kind, title, help, field) {
  return `
    <section class="card">
      <h3>${title}</h3>
      <p>${help}</p>
      <textarea data-input="${field}" placeholder="填写研究输入；空输入不会生成候选。"></textarea>
      <div class="actions">
        <button class="button secondary" data-save-input data-kind="${kind}" data-field="${field}" ${state.project ? "" : "disabled"}>保存到项目</button>
      </div>
    </section>`;
}

async function createProject() {
  const created = await api("/api/projects", { method: "POST", body: "{}" });
  state.project = created.project;
  render();
}

async function saveInput(kind, field) {
  const value = page.querySelector(`[data-input="${field}"]`).value.trim();
  const manifest = {
    field_schema: "genoligand.webui.input_manifest.current",
    input_kind: kind,
    inputs: [
      { field: "project_name", value: "GenoLigand research project" },
      { field, value },
    ],
    files: [],
  };
  await api(`/api/projects/${state.project.project_id}/inputs`, {
    method: "POST",
    body: JSON.stringify(manifest),
  });
  state.project = await api(`/api/projects/${state.project.project_id}`);
  render();
}

async function runPreflight() {
  const result = await api(`/api/projects/${state.project.project_id}/preflight`, { method: "POST" });
  alert(JSON.stringify(result, null, 2));
}

async function startRun() {
  const result = await api(`/api/projects/${state.project.project_id}/runs`, { method: "POST" });
  state.run = result.run;
  state.project = await api(`/api/projects/${state.project.project_id}`);
  setRoute("results");
}

async function renderResults() {
  if (!state.run && state.project?.latest_run_id) {
    state.run = await api(`/api/runs/${state.project.latest_run_id}`);
  }
  if (!state.run) {
    page.innerHTML = `<section class="card"><h2>暂无运行</h2><p>没有运行记录时不会显示候选。</p></section>`;
    return;
  }
  const [candidates, evidence, structure, md, logs] = await Promise.all([
    api(`/api/runs/${state.run.run_id}/candidates`),
    api(`/api/runs/${state.run.run_id}/evidence`),
    api(`/api/runs/${state.run.run_id}/structure`),
    api(`/api/runs/${state.run.run_id}/md-summary`),
    api(`/api/runs/${state.run.run_id}/logs`),
  ]);
  page.innerHTML = `
    <div class="grid">
      <section class="card wide">
        <h2>运行状态</h2>
        <p>状态：<span class="pill ${state.run.state === "completed" ? "ready" : "blocked"}">${state.run.state}</span></p>
        <p>阻塞原因：${escapeHtml(state.run.failure_reason || "无")}</p>
      </section>
      ${tableCard("候选排序", candidates.candidates || [])}
      ${tableCard("证据链", evidence.evidence || [])}
      ${tableCard("结构/口袋", structure.structure_rows || [])}
      ${tableCard("动力学摘要", md.md_rows || [])}
      <section class="card wide"><h2>日志</h2><pre>${escapeHtml((logs.logs || []).join("\n"))}</pre></section>
    </div>`;
}

function tableCard(title, rows) {
  const headers = Array.from(new Set(rows.flatMap((row) => Object.keys(row)))).slice(0, 14);
  const body = rows.map((row) => `<tr>${headers.map((key) => `<td>${escapeHtml(row[key] || "")}</td>`).join("")}</tr>`).join("");
  return `<section class="card wide"><h2>${title}</h2>${rows.length ? `<table><thead><tr>${headers.map((key) => `<th>${escapeHtml(key)}</th>`).join("")}</tr></thead><tbody>${body}</tbody></table>` : "<p>暂无计算结果。</p>"}</section>`;
}

function renderSystem() {
  const required = state.databases?.required || [];
  page.innerHTML = `
    <div class="grid">
      <section class="card">
        <h2>科学仓库</h2>
        <p>${escapeHtml(state.databases?.science_store || "")}</p>
        <table><tbody>${required.map((row) => `<tr><td>${escapeHtml(row.item)}</td><td><span class="pill ${row.ready ? "ready" : "blocked"}">${row.ready ? "ready" : "missing"}</span></td></tr>`).join("")}</tbody></table>
      </section>
      <section class="card">
        <h2>安装命令</h2>
        <pre>cargo run -p gls-cli -- install prepare-science-store --science-store ${escapeHtml(state.databases?.science_store || "science_store")}</pre>
      </section>
    </div>`;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

boot().catch((error) => {
  page.innerHTML = `<section class="card"><h2>服务连接失败</h2><pre>${escapeHtml(error.stack || error.message)}</pre></section>`;
});
