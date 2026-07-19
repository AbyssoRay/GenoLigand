const state = {
  step: "project",
  selectedInput: "small_molecule",
  projects: [],
  workspaceSummary: null,
  project: null,
  run: null,
  health: null,
  databases: null,
  diagnostics: null,
  setup: null,
  setupPlan: null,
  setupRepair: null,
  setupProgress: null,
  preflight: null,
  runData: null,
  resultTab: "ranking",
  resultQueries: {},
  inputCapabilities: null,
  savedInputs: null,
  form: {
    project_name: "",
    small_molecule: "",
    peptide: "",
    protein_ligand: "",
    complex: "",
    genome_or_metagenome: "",
  },
  inputInspection: null,
  inputInspectionPayload: null,
  inputInspectionPayloadKey: "",
  inputInspectionPending: false,
  inputInspectionError: "",
  inputInspectionGeneration: 0,
  inputSavePending: false,
  inputSaveGeneration: 0,
  alert: "",
};

let setupProgressTimer = null;
let runProgressTimer = null;
let inputInspectionTimer = null;
let setupProgressRequestPromise = null;
let runProgressRequestPromise = null;
let resultRequestController = null;
let resultRequestPromise = null;
let projectOpenGeneration = 0;
let setupProgressPollingGeneration = 0;
let runProgressPollingGeneration = 0;

const page = document.querySelector("[data-page]");
const alerts = document.querySelector("[data-alerts]");
const inputSpecs = [
  { kind: "small_molecule", field: "small_molecule" },
  { kind: "peptide", field: "peptide" },
  { kind: "protein_ligand", field: "protein_ligand" },
  { kind: "complex", field: "complex" },
  { kind: "genome_or_metagenome", field: "genome_or_metagenome" },
];

const stepMeta = {
  project: ["项目", "新建研究项目，或进入已有项目继续运行和查看结果。"],
  input: ["输入研究对象", "填写任一种研究输入，空输入不会生成候选。"],
  check: ["检查运行条件", "输入、数据库、运行环境、存储和导出条件都通过后再运行。"],
  run: ["运行中心", "查看当前阶段、下一步动作和日志。"],
  results: ["查看结果", "候选排序、详情、证据、结构和动力学结果只来自当前运行输出。"],
  report: ["报告", "整理报告、候选表和证据摘要。"],
};

const stageLabels = [
  "输入归一化",
  "输入结构准备",
  "结构特征编码",
  "本地序列比对",
  "数据库精确与相似性检索",
  "宏基因组潜在序列比对",
  "结构候选检索",
  "候选粗排与短名单",
  "对接与短动力学精筛",
  "分路线联合排序",
  "证据链整理",
  "报告生成",
];
