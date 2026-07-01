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
  if (looksLikeConvertibleFreeText(field, text)) return true;
  if (field === "small_molecule") {
    return false;
  }
  if (field === "peptide") {
    return looksLikePathWithExt(text, ["fasta", "fa", "pdb", "cif", "mmcif"]) || looksLikePeptideSequence(text, 2);
  }
  if (field === "protein_ligand") {
    return looksLikePathWithExt(text, ["fasta", "fa", "faa", "pdb", "cif", "mmcif"]) || looksLikePeptideSequence(text, 5);
  }
  if (field === "complex") {
    return looksLikePathWithExt(text, ["pdb", "cif", "mmcif", "ent"]) || alnumCount(text) >= 5;
  }
  if (field === "genome_or_metagenome") {
    return (
      looksLikePathWithExt(text, ["fasta", "fa", "fna", "faa", "fastq", "fq", "gb", "gff", "gtf", "vcf"]) ||
      looksLikeNucleotideSequence(text, 8)
    );
  }
  return alnumCount(text) >= 2;
}

function looksLikeConvertibleFreeText(field, value) {
  return (
    looksLikePathWithExt(value, [
      "sdf",
      "mol2",
      "mol",
      "pdb",
      "cif",
      "mmcif",
      "fasta",
      "fa",
      "faa",
      "fna",
      "fastq",
      "fq",
      "gb",
      "gff",
      "gtf",
      "vcf",
    ]) ||
    looksLikePeptideSequence(value, 2) ||
    looksLikeInchi(value) ||
    looksLikeSmilesText(value) ||
    looksLikeNucleotideSequence(value, 8)
  );
}

function looksLikePathWithExt(value, extensions) {
  const lower = value.toLowerCase();
  return extensions.some((ext) => lower.endsWith(`.${ext}`) || lower.includes(`.${ext}\n`));
}

function sequenceBody(value) {
  const body = value.trim().startsWith(">")
    ? value
        .split(/\r?\n/)
        .slice(1)
        .join("")
    : value;
  return body.replace(/\s+/g, "");
}

function looksLikePeptideSequence(value, minLength) {
  const body = sequenceBody(value);
  return body.length >= minLength && /^[ACDEFGHIKLMNPQRSTVWY]+$/i.test(body);
}

function looksLikeNucleotideSequence(value, minLength) {
  const body = sequenceBody(value);
  return body.length >= minLength && /^[ACGTUNRYSWKMBDHV]+$/i.test(body);
}

function looksLikeInchi(value) {
  return value.trim().startsWith("InChI=");
}

function looksLikeSmilesText(value) {
  const text = value.trim();
  if (!text || /\s/.test(text)) return false;
  if (!/^[A-Za-z0-9[\]()=#@+\-\\/%.:]+$/.test(text)) return false;
  let sawAtom = false;
  for (let index = 0; index < text.length; ) {
    const ch = text[index];
    if (/[A-Z]/.test(ch)) {
      const two = text.slice(index, index + 2);
      if (/[a-z]/.test(text[index + 1] || "")) {
        if (validElementSymbol(two)) {
          sawAtom = true;
          index += 2;
          continue;
        }
        return false;
      }
      if (validElementSymbol(ch)) {
        sawAtom = true;
        index += 1;
        continue;
      }
      return false;
    }
    if (/[bcnops]/.test(ch)) {
      sawAtom = true;
      index += 1;
      continue;
    }
    if (/[a-z]/.test(ch)) return false;
    index += 1;
  }
  return sawAtom;
}

function validElementSymbol(symbol) {
  return ["B", "C", "N", "O", "P", "S", "F", "I", "H", "Cl", "Br", "Na", "K", "Li", "Mg", "Ca", "Zn", "Fe"].includes(symbol);
}

function alnumCount(value) {
  return Array.from(String(value || "")).filter((char) => /[\p{Letter}\p{Number}]/u.test(char)).length;
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
  return Boolean(state.project) && groups.every((group) => group.rows.every((row) => row.ready));
}

function canViewResults() {
  return state.run?.state === "completed";
}

function canOpenStep(step) {
  if (step === "input") return true;
  if (step === "configure" || step === "check" || step === "run") return Boolean(state.project);
  if (step === "results" || step === "export") return canViewResults();
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
    ["结构索引", sourceReady("structure_index") ? "就绪" : "需要准备"],
    ["结构/动力学工具", sourceReady("tools") ? "就绪" : "需要准备"],
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
  const available = items.filter((item) => item.exists);
  if (!available.length) return `<div class="empty">当前运行还没有可导出材料。完成运行并生成输出后才会显示下载项。</div>`;
  return `
    <div class="table-wrap">
      <table>
        <thead><tr><th>输出材料</th><th>状态</th></tr></thead>
        <tbody>
          ${available.map((item) => `<tr><td>${friendlyArtifact(item.label || item.id)}</td><td><span class="status-pill ready">可下载</span></td></tr>`).join("")}
        </tbody>
      </table>
    </div>`;
}
