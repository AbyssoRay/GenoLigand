function table(rows, columns) {
  const keys = columns.filter((column) => rows.some((row) => row[column.key] !== undefined));
  if (!keys.length) return `<div class="empty">当前输出没有可显示字段。</div>`;
  return `
    <div class="table-wrap">
      <table>
        <thead><tr>${keys.map((column) => `<th>${escapeHtml(column.label)}</th>`).join("")}</tr></thead>
        <tbody>
          ${rows.map((row) => `<tr>${keys.map((column) => `<td>${escapeHtml(formatCell(column.key, row[column.key]))}</td>`).join("")}</tr>`).join("")}
        </tbody>
      </table>
    </div>`;
}

function columnsFor(kind) {
  const common = [
    { key: "rank", label: "排序" },
    { key: "input_id", label: "输入编号" },
    { key: "record_id", label: "记录编号" },
    { key: "input_kind", label: "输入类型" },
    { key: "ranking_policy", label: "排序路线" },
    { key: "candidate_id", label: "候选编号" },
    { key: "target_id", label: "目标编号" },
    { key: "target_name", label: "目标名称" },
    { key: "candidate_scope_label", label: "候选范围" },
    { key: "organism", label: "物种" },
    { key: "final_score", label: "综合分数" },
    { key: "known_database_score", label: "已知库分数" },
    { key: "metagenome_score", label: "宏基因组分数" },
    { key: "structure_score", label: "结构分数" },
    { key: "docking_score", label: "对接分数" },
    { key: "md_score", label: "动力学分数" },
    { key: "md_support_label", label: "MD 支持" },
    { key: "risk_note", label: "风险提示" },
    { key: "evidence_summary", label: "证据摘要" },
    { key: "status_label", label: "状态" },
    { key: "result_source", label: "输出材料" },
  ];
  if (kind === "ranking") return common;
  if (kind === "candidate_detail") {
    return [
      { key: "input_id", label: "输入编号" },
      { key: "record_id", label: "记录编号" },
      { key: "input_kind", label: "输入类型" },
      { key: "ranking_policy", label: "排序路线" },
      { key: "candidate_id", label: "候选编号" },
      { key: "target_id", label: "目标编号" },
      { key: "target_name", label: "目标名称" },
      { key: "candidate_scope_label", label: "候选范围" },
      { key: "organism", label: "物种" },
      { key: "final_score", label: "综合分数" },
      { key: "known_database_score", label: "已知库分数" },
      { key: "metagenome_score", label: "宏基因组分数" },
      { key: "structure_score", label: "结构分数" },
      { key: "docking_score", label: "对接分数" },
      { key: "md_score", label: "动力学分数" },
      { key: "md_support_label", label: "MD 支持" },
      { key: "risk_note", label: "风险提示" },
      { key: "evidence_summary", label: "证据摘要" },
      { key: "status_label", label: "状态" },
      { key: "detail_source", label: "详情来源" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  if (kind === "evidence") {
    return [
      { key: "evidence_type", label: "证据类型" },
      { key: "source", label: "来源" },
      { key: "status", label: "状态" },
      { key: "summary", label: "说明" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  if (kind === "genome") {
    return [
      { key: "source", label: "来源" },
      { key: "target_id", label: "目标编号" },
      { key: "target_name", label: "目标名称" },
      { key: "organism", label: "物种" },
      { key: "target_class", label: "类别" },
      { key: "evidence", label: "证据" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  if (kind === "structure") {
    return [
      { key: "candidate_id", label: "候选编号" },
      { key: "target_id", label: "目标编号" },
      { key: "target_name", label: "目标名称" },
      { key: "structure_score", label: "结构分数" },
      { key: "docking_score", label: "对接分数" },
      { key: "summary", label: "结构证据" },
      { key: "status", label: "状态" },
      { key: "viewer_status", label: "查看器" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  if (kind === "md") {
    return [
      { key: "candidate_id", label: "候选编号" },
      { key: "target_id", label: "目标编号" },
      { key: "target_name", label: "目标名称" },
      { key: "md_score", label: "动力学分数" },
      { key: "rmsd", label: "RMSD" },
      { key: "rmsf", label: "RMSF" },
      { key: "ligand_drift", label: "配体漂移" },
      { key: "hydrogen_bond_occupancy", label: "氢键占有率" },
      { key: "contact_occupancy", label: "接触占有率" },
      { key: "interaction_persistence", label: "相互作用持续性" },
      { key: "summary", label: "摘要" },
      { key: "status", label: "状态" },
      { key: "trajectory_status", label: "轨迹数据" },
      { key: "result_source", label: "输出材料" },
    ];
  }
  return common;
}

function metricRow(label, value) {
  return `<div class="metric-row"><div class="metric-label">${escapeHtml(label)}</div><div>${escapeHtml(value)}</div></div>`;
}

function boundedPercent(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return 0;
  return Math.max(0, Math.min(100, Math.round(number)));
}

function formatBytes(value) {
  const bytes = Number(value || 0);
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size >= 10 || unit === 0 ? Math.round(size) : size.toFixed(1)} ${units[unit]}`;
}

function valueOrMissing(value, fallback = "未提供") {
  return value === undefined || value === null || value === "" ? fallback : value;
}

function runStateLabel(value) {
  return {
    not_started: "未开始",
    created: "已创建",
    running: "运行中",
    completed: "已完成",
    blocked: "已阻塞",
    failed: "失败",
    cancelled: "已取消",
    resumable: "可恢复",
  }[value] || value || "未开始";
}

function runStateClass(value) {
  if (value === "completed") return "ready";
  if (value === "running") return "loading";
  if (value === "failed") return "failed";
  if (value === "blocked") return "blocked";
  return "waiting";
}

function friendlyArtifact(name) {
  return {
    structure_patch: "结构与口袋摘要",
    md_summary: "动力学摘要",
    candidate_ranking: "候选排序",
    evidence: "证据链",
    research_report: "科研报告",
  }[name] || name;
}

function friendlyScope(value) {
  return {
    known_database: "已知蛋白数据库",
    known_human_protein: "已知蛋白数据库",
    known_nonhuman_protein: "已知非人源蛋白数据库",
    metagenome_potential: "宏基因组潜在序列",
    metagenome_potential_protein: "宏基因组潜在序列",
    structure_interaction: "结构索引证据",
  }[value] || value || "未提供";
}

function formatCell(key, value) {
  if (value === undefined || value === null || value === "") return "未提供";
  if (key === "candidate_scope") return friendlyScope(value);
  if (key === "input_kind") return friendlyInputKind(value);
  if (key === "ranking_policy") return friendlyRankingPolicy(value);
  if (key === "status") return friendlyStatus(value);
  return value;
}

function friendlyInputKind(value) {
  return {
    small_molecule: "小分子",
    peptide: "肽",
    protein_ligand: "蛋白-配体",
    complex: "复合物",
    genome_or_metagenome: "基因组/宏基因组",
  }[value] || value || "未提供";
}

function friendlyRankingPolicy(value) {
  if (!value) return "未提供";
  const version = String(value).match(/v(\d+)$/)?.[1];
  const suffix = version ? ` v${version}` : "";
  if (String(value).startsWith("small_molecule")) return `小分子路线${suffix}`;
  if (String(value).startsWith("peptide")) return `肽路线${suffix}`;
  if (String(value).startsWith("protein_ligand")) return `蛋白-配体路线${suffix}`;
  if (String(value).startsWith("complex")) return `复合物路线${suffix}`;
  if (String(value).startsWith("genome_or_metagenome")) return `基因组/宏基因组路线${suffix}`;
  if (String(value).startsWith("fallback")) return `通用路线${suffix}`;
  return value;
}

function rankingWeightSummary(row) {
  const weights = [
    ["已知库", row.known_database_weight],
    ["宏基因组", row.metagenome_weight],
    ["结构", row.structure_weight],
    ["对接", row.docking_weight],
    ["MD", row.md_weight],
  ]
    .map(([label, value]) => [label, Number(value)])
    .filter(([, value]) => Number.isFinite(value) && value > 0)
    .map(([label, value]) => `${label} ${Math.round(value * 100)}%`);
  return weights.length ? weights.join(" · ") : "未提供";
}

function friendlyStatus(value) {
  return {
    not_started: "未开始",
    created: "已创建",
    running: "运行中",
    completed: "已完成",
    blocked: "已阻塞",
    failed: "失败",
    cancelled: "已取消",
    resumable: "可恢复",
    computed_prediction: "计算候选",
    ready: "就绪",
    missing: "缺失",
    unavailable: "不可用",
  }[value] || value || "未提供";
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function escapeAttr(value) {
  return escapeHtml(value).replaceAll("'", "&#39;");
}
