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
  try {
    state.alert = "";
    let projectList;
    [
      state.health,
      state.databases,
      state.diagnostics,
      state.setup,
      state.setupPlan,
      state.setupProgress,
      state.workspaceSummary,
      projectList,
    ] = await Promise.all([
      api("/api/user/health"),
      api("/api/user/databases"),
      api("/api/user/diagnostics"),
      api("/api/user/setup/status"),
      api("/api/user/setup/install-plan"),
      api("/api/user/setup/progress"),
      api("/api/user/workspace-summary"),
      api("/api/user/projects"),
    ]);
    state.projects = projectList.projects || [];
    if (state.run?.run_id) {
      state.run = await api(`/api/user/runs/${state.run.run_id}`);
      await loadRunData();
      if (state.run?.state === "running") {
        startRunProgressPolling();
      } else {
        stopRunProgressPolling();
      }
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
  health.textContent = ready ? "服务就绪" : "服务未连接";
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
