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
  setupProgress: null,
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

let setupProgressTimer = null;

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
