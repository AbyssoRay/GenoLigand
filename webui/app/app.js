function setStep(step) {
  if (!canOpenStep(step)) return;
  if (state.inputSavePending && step !== state.step) {
    invalidateInputInspection();
  }
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
  state.alert = "";
  const requests = [
    ["health", "服务状态", () => api("/api/user/health")],
    ["inputCapabilities", "输入能力", loadInputCapabilities],
    ["databases", "数据库状态", () => api("/api/user/databases")],
    ["diagnostics", "诊断状态", () => api("/api/user/diagnostics")],
    ["setup", "安装状态", () => api("/api/user/setup/status")],
    ["setupPlan", "安装计划", () => api("/api/user/setup/install-plan")],
    ["setupProgress", "安装进度", () => api("/api/user/setup/progress")],
    ["workspaceSummary", "工作台摘要", () => api("/api/user/workspace-summary")],
    ["projects", "项目列表", () => api("/api/user/projects")],
  ];
  const settled = await Promise.allSettled(requests.map(([, , load]) => load()));
  const failures = [];
  settled.forEach((result, index) => {
    const [key, label] = requests[index];
    if (result.status === "fulfilled") {
      if (key === "projects") state.projects = result.value.projects || [];
      else state[key] = result.value;
      return;
    }
    failures.push({ key, label, error: result.reason });
    if (key === "health") state.health = null;
  });
  if (state.run?.run_id) {
    try {
      state.run = await api(`/api/user/runs/${state.run.run_id}`);
      await loadRunData();
      if (state.run?.state === "running") {
        startRunProgressPolling();
      } else {
        stopRunProgressPolling();
      }
    } catch (error) {
      failures.push({ key: "run", label: "当前运行", error });
    }
  }
  const healthFailure = failures.find((failure) => failure.key === "health");
  if (healthFailure) {
    state.alert = `无法连接本机服务：${healthFailure.error?.message || "请求失败"}`;
  } else if (failures.length) {
    state.alert = `部分信息未刷新：${failures.map((failure) => failure.label).join("、")}`;
  }
  render();
}

function render() {
  const [title, subtitle] = stepMeta[state.step];
  document.querySelector("[data-title]").textContent = title;
  document.querySelector("[data-subtitle]").textContent = subtitle;
  updateStepNavigation();
  renderShellStatus();
  renderAlert();
  if (state.step === "project") renderProject();
  if (state.step === "input") renderInput();
  if (state.step === "check") renderCheck();
  if (state.step === "run") renderRun();
  if (state.step === "results") renderResults();
  if (state.step === "report") renderReport();
}

function renderShellStatus() {
  const health = document.querySelector("[data-health]");
  const ready = state.health?.status === "ready";
  const version = state.health?.version || "";
  const build = state.health?.build_id?.slice(0, 8) || "";
  health.textContent = ready
    ? ["服务就绪", version, build].filter(Boolean).join(" · ")
    : "服务未连接";
  health.className = `status-pill ${ready ? "ready" : "blocked"}`;
  document.querySelector("[data-project-pill]").textContent = state.project
    ? state.project.project_name || "当前项目"
    : state.projects.length
      ? `已有 ${state.projects.length} 个项目`
      : "空白工作台";
}

function renderAlert() {
  alerts.innerHTML = state.alert
    ? `<div class="notice bad">${escapeHtml(state.alert)}</div>`
    : "";
}

boot().catch((error) => {
  page.className = "page";
  page.innerHTML = `<section class="panel full"><h2>服务连接失败</h2><div class="notice bad">${escapeHtml(error.message)}</div></section>`;
});
