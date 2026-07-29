function collectForm() {
  page.querySelectorAll("[data-form]").forEach((field) => {
    state.form[field.dataset.form] = field.value;
  });
}

async function loadInputCapabilities() {
  const capabilities = await api("/api/user/inputs/capabilities");
  const specs = capabilities?.input_types;
  const valid =
    Array.isArray(specs) &&
    specs.length > 0 &&
    specs.every(
      (spec) =>
        spec &&
        typeof spec.kind === "string" &&
        typeof spec.field === "string" &&
        typeof spec.title === "string" &&
        typeof spec.label === "string" &&
        typeof spec.placeholder === "string" &&
        typeof spec.minimum === "string" &&
        spec.text?.supported === true &&
        spec.file?.supported === true &&
        Array.isArray(spec.file.extensions) &&
        spec.file.extensions.length > 0 &&
        Number.isInteger(spec.constraints?.minimum_records) &&
        spec.constraints.minimum_records > 0,
    );
  if (!valid) throw new Error("输入能力描述不可用。");
  state.inputCapabilities = capabilities;
  inputSpecs.splice(0, inputSpecs.length, ...specs);
  if (!inputSpecs.some((spec) => spec.kind === state.selectedInput)) {
    state.selectedInput = inputSpecs[0].kind;
  }
  inputSpecs.forEach((spec) => {
    if (!(spec.field in state.form)) state.form[spec.field] = "";
  });
  return capabilities;
}

function emptyInputForm() {
  const form = { project_name: "" };
  inputSpecs.forEach((spec) => {
    form[spec.field] = "";
  });
  return form;
}

function restoreInputForm(project, savedInputs) {
  invalidateInputInspection();
  const form = emptyInputForm();
  const fields = new Set(["project_name", ...inputSpecs.map((spec) => spec.field)]);
  (savedInputs?.inputs || []).forEach((row) => {
    if (fields.has(row?.field)) form[row.field] = String(row.value ?? "");
  });
  if (!form.project_name.trim()) {
    form.project_name = project?.project_name || "";
  }
  state.form = form;
  state.savedInputs = savedInputs;
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

function resetResultViewState() {
  state.resultTab = "ranking";
  state.resultQueries = {};
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
    state.savedInputs = inputPayload;
    resetResultViewState();
    state.run = null;
    state.runData = null;
    state.preflight = null;
    let projectRefreshFailed = false;
    try {
      await refreshProjects();
    } catch {
      projectRefreshFailed = true;
    }
    if (!inputSaveIsCurrent(saveGeneration, inspectionGeneration, projectId, inputPayload, inputPayloadKeyValue)) {
      return discardStaleInputSave(saveGeneration);
    }
    finishInputSave(saveGeneration);
    if (projectRefreshFailed) {
      state.alert = "输入已保存，项目列表暂未刷新。";
    }
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
  projectOpenGeneration += 1;
  cancelResultRequest();
  resetResultViewState();
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
  const generation = projectOpenGeneration + 1;
  projectOpenGeneration = generation;
  state.alert = "";
  cancelResultRequest();
  resetResultViewState();
  stopRunProgressPolling();
  resetInputForm();
  state.preflight = null;
  state.run = null;
  state.runData = null;
  const [project, savedInputs] = await Promise.all([
    api(`/api/user/projects/${projectId}`),
    api(`/api/user/projects/${projectId}/inputs`),
  ]);
  if (generation !== projectOpenGeneration) return false;
  state.project = project;
  restoreInputForm(project, savedInputs);
  if (state.project.latest_run_id) {
    state.run = await api(`/api/user/runs/${state.project.latest_run_id}`);
    if (generation !== projectOpenGeneration) return false;
    await loadRunData();
    if (generation !== projectOpenGeneration) return false;
    setStep(state.run.state === "completed" ? "results" : "run");
    return true;
  }
  setStep("input");
  return true;
}

async function deleteExistingProject(projectId) {
  const project = (state.projects || []).find((item) => item.project_id === projectId) || state.project;
  const name = project?.project_name || "当前项目";
  if (!window.confirm(`删除“${name}”？项目运行输出也会删除。`)) return;
  if (state.project?.project_id === projectId) {
    projectOpenGeneration += 1;
    cancelResultRequest();
    resetResultViewState();
    invalidateInputInspection();
  }
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

async function refreshCurrentProject() {
  const projectId = state.project?.project_id;
  if (!projectId) return null;
  const [project, projectList] = await Promise.all([
    api(`/api/user/projects/${projectId}`),
    api("/api/user/projects"),
  ]);
  if (state.project?.project_id !== projectId) return null;
  state.project = project;
  state.projects = projectList.projects || [];
  return project;
}

function resetInputForm() {
  invalidateInputInspection();
  state.form = emptyInputForm();
  state.savedInputs = null;
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

function refreshSetupProgress() {
  if (setupProgressRequestPromise) return setupProgressRequestPromise;
  let request;
  request = (async () => {
    try {
      const setup = await api("/api/user/setup/status");
      const progress = await api("/api/user/setup/progress");
      state.setup = setup;
      state.setupProgress = progress;
    } catch {
      state.setupProgress = state.setupProgress || null;
    } finally {
      if (setupProgressRequestPromise === request) setupProgressRequestPromise = null;
    }
  })();
  setupProgressRequestPromise = request;
  return request;
}

function refreshRunProgress() {
  if (runProgressRequestPromise) return runProgressRequestPromise;
  if (!state.run?.run_id) return Promise.resolve(null);
  const runId = state.run.run_id;
  let request;
  request = (async () => {
    try {
      const run = await api(`/api/user/runs/${runId}`);
      if (state.run?.run_id !== runId) return null;
      state.run = run;
      if (run.state === "running") {
        await loadRunData();
      } else {
        await Promise.all([loadRunData(), refreshCurrentProject()]);
      }
      return run;
    } catch (error) {
      state.alert = `无法刷新运行进度：${error.message}`;
      return null;
    } finally {
      if (runProgressRequestPromise === request) runProgressRequestPromise = null;
    }
  })();
  runProgressRequestPromise = request;
  return request;
}

function startRunProgressPolling() {
  stopRunProgressPolling();
  const generation = runProgressPollingGeneration;
  const schedule = () => {
    if (generation !== runProgressPollingGeneration) return;
    runProgressTimer = window.setTimeout(async () => {
      runProgressTimer = null;
      await refreshRunProgress();
      if (generation !== runProgressPollingGeneration) return;
      if (state.step === "run") render();
      if (state.run?.state === "running") schedule();
      else stopRunProgressPolling();
    }, 1000);
  };
  schedule();
}

function stopRunProgressPolling() {
  runProgressPollingGeneration += 1;
  if (runProgressTimer !== null) {
    window.clearTimeout(runProgressTimer);
    runProgressTimer = null;
  }
}

function startSetupProgressPolling() {
  stopSetupProgressPolling();
  const generation = setupProgressPollingGeneration;
  const schedule = () => {
    if (generation !== setupProgressPollingGeneration) return;
    setupProgressTimer = window.setTimeout(async () => {
      setupProgressTimer = null;
      await refreshSetupProgress();
      if (generation !== setupProgressPollingGeneration) return;
      if (state.step === "check") render();
      if (state.setupProgress?.status === "running") schedule();
      else stopSetupProgressPolling();
    }, 1000);
  };
  schedule();
}

function stopSetupProgressPolling() {
  setupProgressPollingGeneration += 1;
  if (setupProgressTimer !== null) {
    window.clearTimeout(setupProgressTimer);
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
  cancelResultRequest();
  resetResultViewState();
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

async function cancelRun() {
  const runId = state.run?.run_id;
  if (!runId || state.run?.state !== "running" || state.runCancelPending) return;
  state.runCancelPending = true;
  state.alert = "";
  render();
  try {
    const result = await api(`/api/user/runs/${runId}/cancel`, { method: "POST" });
    if (state.run?.run_id !== runId) return;
    state.run = await api(`/api/user/runs/${runId}`);
    await Promise.all([loadRunData(), refreshCurrentProject()]);
    if (result.status === "failed" || result.process_stopped === false) {
      state.alert = result.error || "运行取消失败，请重试。";
    }
  } catch (error) {
    state.alert = `运行取消失败：${error.message}`;
  } finally {
    state.runCancelPending = false;
    if (state.run?.state !== "running") stopRunProgressPolling();
    render();
  }
}

function resultRequestAborted(error) {
  return error?.name === "AbortError";
}

function cancelResultRequest() {
  const controller = resultRequestController;
  resultRequestController = null;
  resultRequestPromise = null;
  if (controller && !controller.signal.aborted) controller.abort();
}

function runResultRequest(task, { replace = false } = {}) {
  if (resultRequestPromise && !replace) return resultRequestPromise;
  if (replace) cancelResultRequest();
  const controller = new AbortController();
  const request = Promise.resolve()
    .then(() => task(controller.signal))
    .catch((error) => {
      if (resultRequestAborted(error)) return null;
      throw error;
    });
  const tracked = request.finally(() => {
    if (resultRequestPromise === tracked) {
      resultRequestPromise = null;
      resultRequestController = null;
    }
  });
  resultRequestController = controller;
  resultRequestPromise = tracked;
  return tracked;
}

function parseTsvRows(text, headers) {
  const lines = String(text || "")
    .replace(/^\uFEFF/, "")
    .split(/\r?\n/)
    .filter((line) => line.trim());
  if (lines.length < 2) return [];
  const keys = lines[0].split("\t").map((label) => headers[label] || label);
  return lines.slice(1).map((line) => {
    const values = line.split("\t");
    return keys.reduce((row, key, index) => {
      row[key] = values[index] || "";
      return row;
    }, {});
  });
}

async function loadOptionalArtifactRows(runId, artifactId, headers, signal) {
  try {
    const text = await apiText(
      `/api/user/runs/${encodeURIComponent(runId)}/download/${encodeURIComponent(artifactId)}`,
      { signal },
    );
    return parseTsvRows(text, headers);
  } catch (error) {
    if (resultRequestAborted(error)) throw error;
    if (error.status === 404) return [];
    throw error;
  }
}

async function loadRunData() {
  if (!state.run?.run_id) {
    cancelResultRequest();
    state.runData = null;
    return Promise.resolve(null);
  }
  const runId = state.run.run_id;
  return runResultRequest(async (signal) => {
    const candidates = await api(`/api/user/runs/${runId}/candidates`, { signal });
    const detail = await api(`/api/user/runs/${runId}/candidate-detail`, { signal });
    const evidence = await api(`/api/user/runs/${runId}/evidence`, { signal });
    const genome = await api(`/api/user/runs/${runId}/genome-context`, { signal });
    const structure = await api(`/api/user/runs/${runId}/structure`, { signal });
    const md = await api(`/api/user/runs/${runId}/md-summary`, { signal });
    const logs = await api(`/api/user/runs/${runId}/logs`, { signal });
    const artifacts = await api(`/api/user/runs/${runId}/artifacts`, { signal });
    let onsite = [];
    let shortlist = [];
    let docking = [];
    if (state.run?.state === "completed") {
      [onsite, shortlist, docking] = await Promise.all([
        loadOptionalArtifactRows(
          runId,
          "onsite_search",
          {
            输入编号: "input_id",
            来源: "source",
            目标编号: "target_id",
            目标名称: "target_name",
            物种: "organism",
            目标类型: "target_class",
            匹配分数: "match_score",
            匹配特征: "matched_features",
            证据: "evidence",
          },
          signal,
        ),
        loadOptionalArtifactRows(
          runId,
          "candidate_shortlist",
          {
            短名单排序: "shortlist_rank",
            候选编号: "candidate_id",
            输入编号: "input_id",
            记录编号: "record_id",
            目标编号: "target_id",
            粗排分数: "preliminary_score",
            入选原因: "shortlist_reason",
            坐标状态: "coordinate_status",
          },
          signal,
        ),
        loadOptionalArtifactRows(
          runId,
          "docking_summary",
          {
            候选编号: "candidate_id",
            对接状态: "docking_status",
            对接分数: "docking_score",
            对接引擎: "docking_engine",
            说明: "notes",
          },
          signal,
        ),
      ]);
    }
    if (signal.aborted || state.run?.run_id !== runId) return null;
    state.runData = {
      candidates,
      detail,
      evidence,
      genome,
      onsite: { rows: onsite },
      shortlist: { rows: shortlist },
      docking: { rows: docking },
      structure,
      md,
      logs,
      artifacts,
    };
    return state.runData;
  });
}
