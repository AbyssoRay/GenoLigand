function collectForm() {
  page.querySelectorAll("[data-form]").forEach((field) => {
    state.form[field.dataset.form] = field.value;
  });
}

function inputPayloadFromForm() {
  return {
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
}

function inputPayloadKey(payload) {
  return JSON.stringify(payload);
}

function clearInputInspectionTimer() {
  if (inputInspectionTimer !== null) {
    window.clearTimeout(inputInspectionTimer);
    inputInspectionTimer = null;
  }
}

function invalidateInputInspection() {
  clearInputInspectionTimer();
  state.inputInspectionGeneration += 1;
  state.inputSaveGeneration += 1;
  state.inputSavePending = false;
  state.inputInspection = null;
  state.inputInspectionPayload = null;
  state.inputInspectionPayloadKey = "";
  state.inputInspectionPending = false;
  state.inputInspectionError = "";
}

function inputSaveIsCurrent(saveGeneration, inspectionGeneration, projectId, payload, payloadKey) {
  return (
    saveGeneration === state.inputSaveGeneration &&
    inspectionGeneration === state.inputInspectionGeneration &&
    state.project?.project_id === projectId &&
    state.inputInspectionPayload === payload &&
    state.inputInspectionPayloadKey === payloadKey &&
    inputPayloadKey(inputPayloadFromForm()) === payloadKey
  );
}

function finishInputSave(saveGeneration) {
  if (saveGeneration === state.inputSaveGeneration) {
    state.inputSavePending = false;
  }
}

function discardStaleInputSave(saveGeneration) {
  const currentSave = saveGeneration === state.inputSaveGeneration;
  finishInputSave(saveGeneration);
  if (currentSave) render();
  return false;
}

function scheduleInputInspection() {
  invalidateInputInspection();
  if (!filledInputs().length) return;
  const generation = state.inputInspectionGeneration;
  state.inputInspectionPending = true;
  inputInspectionTimer = window.setTimeout(() => {
    inputInspectionTimer = null;
    void inspectInputs(generation);
  }, 300);
}

async function inspectInputs(generation = null) {
  if (generation === null) {
    invalidateInputInspection();
    generation = state.inputInspectionGeneration;
  }
  const payload = inputPayloadFromForm();
  const payloadKey = inputPayloadKey(payload);
  if (!filledInputs().length) {
    if (generation === state.inputInspectionGeneration) {
      state.inputInspectionPending = false;
      render();
    }
    return null;
  }
  if (generation !== state.inputInspectionGeneration) return null;
  state.inputInspectionPending = true;
  state.inputInspectionError = "";
  render();
  try {
    const inspection = await api("/api/user/inputs/inspect", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    if (
      generation !== state.inputInspectionGeneration ||
      payloadKey !== inputPayloadKey(inputPayloadFromForm())
    ) {
      return null;
    }
    state.inputInspection = inspection;
    state.inputInspectionPayload = payload;
    state.inputInspectionPayloadKey = payloadKey;
    return inspection;
  } catch (error) {
    if (
      generation !== state.inputInspectionGeneration ||
      payloadKey !== inputPayloadKey(inputPayloadFromForm())
    ) {
      return null;
    }
    state.inputInspection = null;
    state.inputInspectionPayload = null;
    state.inputInspectionPayloadKey = "";
    state.inputInspectionError = error.message || "输入检查失败";
    return null;
  } finally {
    if (
      generation === state.inputInspectionGeneration &&
      payloadKey === inputPayloadKey(inputPayloadFromForm())
    ) {
      state.inputInspectionPending = false;
      render();
    }
  }
}

async function saveInput() {
  collectForm();
  if (!state.project) {
    state.alert = "请先新建或打开研究项目。";
    render();
    return false;
  }
  const currentPayload = inputPayloadFromForm();
  if (
    !inputsReadyToSave() ||
    state.inputInspectionPayloadKey !== inputPayloadKey(currentPayload)
  ) {
    state.alert = "请先完成当前输入的服务端检查。";
    render();
    return false;
  }
  const inputPayload = state.inputInspectionPayload;
  const inputPayloadKeyValue = state.inputInspectionPayloadKey;
  const inspectionGeneration = state.inputInspectionGeneration;
  const projectId = state.project.project_id;
  const saveGeneration = state.inputSaveGeneration + 1;
  state.inputSaveGeneration = saveGeneration;
  state.inputSavePending = true;
  state.alert = "";
  render();
  try {
    const saved = await api(`/api/user/projects/${projectId}/inputs`, {
      method: "POST",
      body: JSON.stringify(inputPayload),
    });
    if (!inputSaveIsCurrent(saveGeneration, inspectionGeneration, projectId, inputPayload, inputPayloadKeyValue)) {
      return discardStaleInputSave(saveGeneration);
    }
    if (saved.ok !== true) {
      finishInputSave(saveGeneration);
      state.project = saved.project || state.project;
      state.alert = saved.message || "输入未保存，请检查填写内容。";
      render();
      return false;
    }
    let savedProject = saved.project;
    if (!savedProject) {
      savedProject = await api(`/api/user/projects/${projectId}`);
    }
    if (!inputSaveIsCurrent(saveGeneration, inspectionGeneration, projectId, inputPayload, inputPayloadKeyValue)) {
      return discardStaleInputSave(saveGeneration);
    }
    state.project = savedProject;
    state.run = null;
    state.runData = null;
    state.preflight = null;
    await refreshProjects();
    if (!inputSaveIsCurrent(saveGeneration, inspectionGeneration, projectId, inputPayload, inputPayloadKeyValue)) {
      return discardStaleInputSave(saveGeneration);
    }
    finishInputSave(saveGeneration);
    setStep("check");
    return true;
  } catch (error) {
    finishInputSave(saveGeneration);
    if (saveGeneration === state.inputSaveGeneration) {
      state.alert = `输入保存失败：${error.message}`;
      render();
    }
    return false;
  }
}

async function createNewProject() {
  state.alert = "";
  invalidateInputInspection();
  const created = await api("/api/user/projects", { method: "POST", body: "{}" });
  state.project = created.project;
  state.run = null;
  state.runData = null;
  state.preflight = null;
  resetInputForm();
  await refreshProjects();
  setStep("input");
}

async function openExistingProject(projectId) {
  state.alert = "";
  resetInputForm();
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
  setStep("input");
}

async function deleteExistingProject(projectId) {
  const project = (state.projects || []).find((item) => item.project_id === projectId) || state.project;
  const name = project?.project_name || "当前项目";
  if (!window.confirm(`删除“${name}”？项目运行输出也会删除。`)) return;
  if (state.project?.project_id === projectId) invalidateInputInspection();
  const deleted = await api(`/api/user/projects/${projectId}`, { method: "DELETE" });
  if (deleted.status === "failed") {
    state.alert = deleted.error || "项目删除失败。";
    render();
    return;
  }
  if (state.project?.project_id === projectId) {
    state.project = null;
    state.run = null;
    state.runData = null;
    state.preflight = null;
    resetInputForm();
  }
  await refreshProjects();
  setStep("project");
}

async function refreshProjects() {
  const projectList = await api("/api/user/projects");
  state.projects = projectList.projects || [];
}

function resetInputForm() {
  invalidateInputInspection();
  state.form = {
    project_name: "",
    small_molecule: "",
    peptide: "",
    protein_ligand: "",
    complex: "",
    genome_or_metagenome: "",
  };
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
  const candidates = await api(`/api/user/runs/${runId}/candidates`);
  const detail = await api(`/api/user/runs/${runId}/candidate-detail`);
  const evidence = await api(`/api/user/runs/${runId}/evidence`);
  const genome = await api(`/api/user/runs/${runId}/genome-context`);
  const structure = await api(`/api/user/runs/${runId}/structure`);
  const md = await api(`/api/user/runs/${runId}/md-summary`);
  const logs = await api(`/api/user/runs/${runId}/logs`);
  const artifacts = await api(`/api/user/runs/${runId}/artifacts`);
  state.runData = { candidates, detail, evidence, genome, structure, md, logs, artifacts };
}
