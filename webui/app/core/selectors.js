function hasAnyInput() {
  return filledInputs().length > 0;
}

function inputsReadyToSave() {
  const filled = filledInputs();
  return (
    filled.length > 0 &&
    !state.inputInspectionPending &&
    !state.inputSavePending &&
    !state.inputInspectionError &&
    inputInspectionIsCurrent() &&
    state.inputInspection?.usable === true
  );
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
  return filledInputs().filter((item) => inputFieldInspection(item.field).ready);
}

function inputInspectionIsCurrent() {
  return (
    Boolean(state.inputInspectionPayloadKey) &&
    state.inputInspectionPayloadKey === inputPayloadKey(inputPayloadFromForm())
  );
}

function inputStatusLabel() {
  if (state.inputSavePending) return "保存中";
  if (state.inputInspectionPending) return "检查中";
  if (inputsReadyToSave()) return "输入可保存";
  if (hasAnyInput() && (state.inputInspectionError || inputInspectionIsCurrent())) return "检查未通过";
  if (hasAnyInput()) return "等待检查";
  return "等待输入";
}

function inputStatusClass() {
  if (state.inputSavePending) return "waiting";
  if (state.inputInspectionPending) return "waiting";
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
  return {
    ready: inputFieldInspection(spec.field).ready,
    value,
    message: inputInspectionMessage(spec.field),
  };
}

function inputInspectionMessage(field) {
  if (state.inputInspectionPending) return "检查中";
  if (state.inputSavePending) return "正在保存当前检查结果。";
  if (state.inputInspectionError) return state.inputInspectionError;
  if (!inputInspectionIsCurrent()) return "等待检查";
  return inputFieldInspection(field).message;
}

function inputInspectionRowsForField(field) {
  if (!inputInspectionIsCurrent()) return [];
  return (state.inputInspection?.inputs || []).filter((row) => row.original_field === field);
}

function inputFieldInspection(field) {
  const rows = inputInspectionRowsForField(field);
  if (rows.length) {
    const failure = rows.find((row) => row.usable !== true);
    if (failure) {
      return {
        ready: false,
        message: failure.message || failure.error || "输入未通过检查。",
      };
    }
    return { ready: true, message: "服务端检查通过。" };
  }
  if (state.inputInspection?.usable === true) {
    return { ready: true, message: "服务端检查通过。" };
  }
  return {
    ready: false,
    message:
      state.inputInspection?.message ||
      state.inputInspection?.errors?.[0] ||
      state.inputInspection?.summary ||
      "输入未通过检查。",
  };
}

function inputCardStatus(field, filled) {
  if (!filled) return { ready: false, label: "未填写" };
  if (state.inputInspectionPending) return { ready: false, label: "检查中" };
  if (state.inputSavePending) return { ready: false, label: "保存中" };
  if (inputFieldInspection(field).ready) return { ready: true, label: "可用" };
  if (state.inputInspectionError || inputInspectionIsCurrent()) {
    return { ready: false, label: "未通过" };
  }
  return { ready: false, label: "待检查" };
}

function sourceReady(key) {
  const required = state.databases?.required || [];
  if (key === "known") return required.some((row) => row.id === "known_database" && row.ready);
  if (key === "metagenome") return required.some((row) => row.id === "metagenome" && row.ready);
  if (key === "structure_index") return required.some((row) => row.id === "structure_index" && row.ready);
  if (key === "tools") return required.some((row) => row.id === "tools" && row.ready);
  if (key === "structure") return sourceReady("structure_index") && sourceReady("tools");
  return false;
}

function preflightGroups() {
  const byId = new Map((state.preflight?.checks || []).map((row) => [row.id, row]));
  const inputRow = byId.get("input");
  const inputReady = state.project && inputRow?.ready;
  return [
    {
      title: "输入",
      rows: [
        {
          label: "研究输入",
          ready: Boolean(inputReady),
          readyText: inputRow?.detail || "通过",
          next: inputRow?.detail || (state.project ? "保存有效输入" : "先保存输入"),
        },
      ],
    },
    {
      title: "数据库和索引",
      rows: [
        checkRow(byId, "science_store", "科学仓库", "运行安装/修复"),
        checkRow(byId, "known_database", "已知蛋白索引", "准备数据库索引"),
        checkRow(byId, "metagenome", "宏基因组索引", "准备序列索引"),
        checkRow(byId, "structure_index", "结构索引", "准备结构索引"),
      ],
    },
    {
      title: "运行环境",
      rows: [
        checkRow(byId, "tools", "计算工具", "准备运行环境"),
        checkRow(byId, "python_worker", "Python worker", "使用正式启动入口或运行安装/修复"),
      ],
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
  return { label, ready: Boolean(row?.ready), readyText: row?.detail || "通过", next };
}

function canStartRun() {
  const groups = preflightGroups();
  return (
    Boolean(state.project) &&
    state.run?.state !== "running" &&
    groups.every((group) => group.rows.every((row) => row.ready))
  );
}

function canViewResults() {
  return state.run?.state === "completed";
}

function canOpenStep(step) {
  if (step === "project") return true;
  if (step === "input") return Boolean(state.project);
  if (step === "check" || step === "run") return Boolean(state.project);
  if (step === "results" || step === "report") return canViewResults();
  return false;
}

function updateStepNavigation() {
  document.querySelectorAll("[data-step]").forEach((button) => {
    const enabled = canOpenStep(button.dataset.step);
    button.disabled = !enabled;
    button.classList.toggle("disabled", !enabled);
    button.classList.toggle("active", button.dataset.step === state.step);
  });
}

function nextActionText() {
  if (!state.project) return "填写并保存输入";
  if (state.run?.state === "running") return "等待当前阶段完成";
  if (state.run?.state === "completed") return "查看结果或报告";
  if (state.run?.state === "blocked") return "查看阻塞原因";
  if (state.run?.state === "failed") return "查看日志并修复";
  if (state.run?.state === "cancelled") return "检查输入后重新运行";
  if (state.run?.state === "resumable") return "恢复运行或重新开始";
  if (!state.preflight) return "执行预检查";
  if (!canStartRun()) return "处理预检查阻塞项";
  if (!state.run) return "开始运行";
  return "开始运行";
}

function candidateCountText() {
  const count = state.runData?.candidates?.pagination?.total_rows;
  if (Number.isFinite(count)) return `${count}`;
  const rowCount = state.runData?.candidates?.row_count;
  if (Number.isFinite(rowCount)) return `${rowCount}`;
  const traceCount = state.run?.trace?.candidate_count;
  if (Number.isFinite(traceCount)) return `${traceCount}`;
  if (state.run?.state === "completed") return "0";
  return "运行完成后计算";
}

function candidateRows() {
  return state.runData?.candidates?.candidates || [];
}
