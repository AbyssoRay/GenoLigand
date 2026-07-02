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
    const created = await api("/api/user/projects", { method: "POST", body: "{}" });
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
  const saved = await api(`/api/user/projects/${state.project.project_id}/inputs`, {
    method: "POST",
    body: JSON.stringify(inputPayload),
  });
  if (saved.ok !== true) {
    state.project = saved.project || state.project;
    state.alert = saved.message || "输入未保存，请检查填写内容。";
    render();
    return false;
  }
  state.project = saved.project || (await api(`/api/user/projects/${state.project.project_id}`));
  state.run = null;
  state.runData = null;
  state.preflight = null;
  setStep("configure");
  return true;
}

async function openExistingProject(projectId) {
  state.alert = "";
  state.project = await api(`/api/user/projects/${projectId}`);
  state.preflight = null;
  state.run = null;
  state.runData = null;
  if (state.project.latest_run_id) {
    state.run = await api(`/api/user/runs/${state.project.latest_run_id}`);
    await loadRunData();
    setStep(state.run.state === "completed" ? "results" : "run");
    return;
  }
  setStep("configure");
}

async function runPreflight() {
  if (!state.project) {
    state.preflight = null;
    state.alert = "请先保存输入。";
    render();
    return;
  }
  state.preflight = await api(`/api/user/projects/${state.project.project_id}/preflight`, {
    method: "POST",
  });
  render();
}

async function refreshSetupProgress() {
  try {
    state.setupProgress = await api("/api/user/setup/progress");
  } catch {
    state.setupProgress = state.setupProgress || null;
  }
}

async function refreshRunProgress() {
  if (!state.run?.run_id) return;
  try {
    state.run = await api(`/api/user/runs/${state.run.run_id}`);
    await loadRunData();
  } catch (error) {
    state.alert = `无法刷新运行进度：${error.message}`;
  }
}

function startRunProgressPolling() {
  stopRunProgressPolling();
  runProgressTimer = window.setInterval(async () => {
    await refreshRunProgress();
    if (state.run?.state !== "running") {
      stopRunProgressPolling();
    }
    if (state.step === "run") render();
  }, 1000);
}

function stopRunProgressPolling() {
  if (runProgressTimer) {
    window.clearInterval(runProgressTimer);
    runProgressTimer = null;
  }
}

function startSetupProgressPolling() {
  stopSetupProgressPolling();
  setupProgressTimer = window.setInterval(async () => {
    await refreshSetupProgress();
    if (state.setupProgress && state.setupProgress.status !== "running") {
      stopSetupProgressPolling();
    }
    if (state.step === "check") render();
  }, 1000);
}

function stopSetupProgressPolling() {
  if (setupProgressTimer) {
    window.clearInterval(setupProgressTimer);
    setupProgressTimer = null;
  }
}

async function runSetupRepair() {
  state.alert = "";
  let keepPolling = false;
  state.setupRepair = { message: "正在运行安装/修复...", database_ready: false };
  state.setupProgress = {
    status: "running",
    stage: "安装/修复",
    current_resource: "科学资源",
    completed_steps: 0,
    total_steps: 1,
    failed_steps: 0,
    bytes_downloaded: 0,
    bytes_total: 0,
    percent: 0,
    message: "正在准备安装/修复",
  };
  render();
  startSetupProgressPolling();
  try {
    state.setupRepair = await api("/api/user/setup/run", { method: "POST" });
    state.setupProgress = state.setupRepair.progress || state.setupRepair.installer?.progress || state.setupProgress;
    [state.databases, state.diagnostics, state.setup, state.setupProgress] = await Promise.all([
      api("/api/user/databases"),
      api("/api/user/diagnostics"),
      api("/api/user/setup/status"),
      api("/api/user/setup/progress"),
    ]);
    if (state.project) {
      state.preflight = await api(`/api/user/projects/${state.project.project_id}/preflight`, {
        method: "POST",
      });
    }
    keepPolling = state.setupProgress?.status === "running";
  } catch (error) {
    state.alert = `修复动作失败：${error.message}`;
  } finally {
    if (!keepPolling) stopSetupProgressPolling();
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
  if (!canStartRun()) {
    state.alert = "当前运行正在进行中。";
    setStep("run");
    return;
  }
  state.alert = "";
  setStep("run");
  state.run = {
    state: "running",
    state_label: "启动中",
    current_stage: "输入归一化",
    stages: stageLabels.map((label, index) => ({
      label,
      status: index === 0 ? "running" : "waiting",
      status_label: index === 0 ? "进行中" : "等待",
      reason: "",
    })),
  };
  state.runData = null;
  render();
  try {
    const result = await api(`/api/user/projects/${state.project.project_id}/runs`, { method: "POST" });
    state.run = result.run;
    state.project = result.project || (await api(`/api/user/projects/${state.project.project_id}`));
    await loadRunData();
    if (state.run?.state === "running") {
      startRunProgressPolling();
    } else {
      stopRunProgressPolling();
    }
  } catch (error) {
    state.alert = `运行启动失败：${error.message}`;
    stopRunProgressPolling();
  }
  setStep("run");
}

async function loadRunData() {
  if (!state.run?.run_id) {
    state.runData = null;
    return;
  }
  const runId = state.run.run_id;
  const [candidates, detail, evidence, genome, structure, md, logs, artifacts] = await Promise.all([
    api(`/api/user/runs/${runId}/candidates`),
    api(`/api/user/runs/${runId}/candidate-detail`),
    api(`/api/user/runs/${runId}/evidence`),
    api(`/api/user/runs/${runId}/genome-context`),
    api(`/api/user/runs/${runId}/structure`),
    api(`/api/user/runs/${runId}/md-summary`),
    api(`/api/user/runs/${runId}/logs`),
    api(`/api/user/runs/${runId}/artifacts`),
  ]);
  state.runData = { candidates, detail, evidence, genome, structure, md, logs, artifacts };
}
