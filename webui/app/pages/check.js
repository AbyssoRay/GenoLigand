function renderCheck() {
  page.className = "page";
  const groups = preflightGroups();
  const allReady = groups.every((group) => group.rows.every((row) => row.ready));
  const setupReady = state.setup?.database_ready === true;
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>预检查</h2>
            <p>确认当前输入、数据库和运行环境是否可以开始计算；未通过时会给出下一步处理动作。</p>
          </div>
          <span class="status-pill ${allReady ? "ready" : "blocked"}">${allReady ? "可以运行" : "需要处理"}</span>
        </div>
        <div class="actions">
          <button class="button secondary" data-action="check-now" ${state.project ? "" : "disabled"}>重新检查</button>
          <button class="button" data-action="start-run" ${allReady ? "" : "disabled"}>开始运行</button>
          <button class="button secondary" data-action="repair-store" ${setupReady ? "disabled" : ""}>运行安装/修复</button>
          <button class="button ghost" data-action="to-input">修改输入</button>
        </div>
      </section>
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>修复中心</h2>
            <p>这里汇总缺少什么、能否运行以及下一步如何处理。</p>
          </div>
          <span class="status-pill ${setupReady ? "ready" : "blocked"}">${setupReady ? "已就绪" : "需要准备"}</span>
        </div>
        ${setupRepairSummary()}
        ${installerProgressSummary()}
        ${installCatalogSummary()}
        ${installerDownloadSummary()}
        ${installerExternalSourceSummary()}
        ${installerSourceRecordSummary()}
        ${installerResourceSummary()}
        ${installerToolEnvironmentSummary()}
        ${missingResourceSummary()}
        ${diagnosticsSummary()}
        ${sourceStatusTable()}
      </section>
      ${groups.map(renderCheckGroup).join("")}
    </div>`;
  page.querySelector("[data-action='check-now']")?.addEventListener("click", runPreflight);
  page.querySelector("[data-action='start-run']")?.addEventListener("click", startRun);
  page.querySelector("[data-action='repair-store']")?.addEventListener("click", runSetupRepair);
  page.querySelector("[data-action='to-input']").addEventListener("click", () => setStep("input"));
}

function renderCheckGroup(group) {
  return `
    <section class="panel third">
      <div class="panel-header">
        <h2>${group.title}</h2>
        <span class="status-pill ${group.rows.every((row) => row.ready) ? "ready" : "blocked"}">${group.rows.every((row) => row.ready) ? "通过" : "待处理"}</span>
      </div>
      ${group.rows.map((row) => metricRow(row.label, row.ready ? row.readyText || "通过" : row.next)).join("")}
    </section>`;
}

function setupRepairSummary() {
  if (!state.setupRepair) {
    return `<div class="notice warn">尚未运行本机修复。缺项处理完成前，运行按钮会保持禁用。</div>`;
  }
  const ready = state.setupRepair.database_ready === true;
  return `
    <div class="notice ${ready ? "" : "warn"}">
      ${escapeHtml(state.setupRepair.message || (ready ? "科学仓库已就绪。" : "修复动作已执行，但仍有缺项。"))}
    </div>`;
}

function installerProgressSummary() {
  const progress = [state.setupProgress, state.setupRepair?.progress, state.setupRepair?.installer?.progress].find(
    (item) => item && item.status !== "idle",
  );
  if (!progress || progress.status === "idle") return "";
  const percent = boundedPercent(progress.percent);
  const failed = Number(progress.failed_steps || 0);
  const ok = progress.status === "ready";
  const paused = progress.status === "paused";
  const blocked = !paused && (failed > 0 || ["failed", "needs_repair"].includes(progress.status));
  const bytes = progress.bytes_total
    ? `${formatBytes(progress.bytes_downloaded || 0)} / ${formatBytes(progress.bytes_total || 0)}`
    : formatBytes(progress.bytes_downloaded || 0);
  return `
    <div class="progress-panel ${ok ? "ready" : blocked ? "blocked" : ""}">
      <div class="progress-head">
        <div>
          <strong>${escapeHtml(progress.stage || "安装/修复")}</strong>
          <span>${escapeHtml(progress.current_resource || "科学资源")}</span>
        </div>
        <span class="status-pill ${ok ? "ready" : blocked ? "blocked" : paused ? "waiting" : "loading"}">${paused ? "已暂停" : `${percent}%`}</span>
      </div>
      <div class="progress-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent}">
        <span style="width:${percent}%"></span>
      </div>
      <div class="progress-meta">
        <span>${escapeHtml(progress.message || "正在处理")}</span>
        <span>${Number(progress.completed_steps || 0)} / ${Number(progress.total_steps || 0)} 步</span>
        <span>${bytes}</span>
        ${failed ? `<span class="blocked-text">${failed} 项需要处理</span>` : ""}
      </div>
    </div>`;
}

function installCatalogSummary() {
  const direct = state.setupPlan?.direct_sources;
  const external = state.setupPlan?.external_sources;
  if (!direct && !external) return "";
  const directReady = direct?.ready === true;
  const externalReady = external?.ready === true;
  return `
    <div class="notice ${directReady && externalReady ? "" : "warn"}">
      <strong>安装准备：${direct?.source_count || 0} 个数据库来源，${external?.source_group_count || 0} 组大型资源</strong>
      <div class="compact-list">下一步：${escapeHtml((directReady && externalReady) ? "运行安装/修复" : "使用完整安装包重新运行安装/修复")}</div>
    </div>`;
}

function installerDownloadSummary() {
  const downloads = state.setupRepair?.installer?.downloads;
  if (!downloads) return "";
  const ok = downloads.completed === true;
  const message = downloads.message || (ok ? "科学资源下载步骤已完成。" : "科学资源下载状态需要检查。");
  const next = downloads.next_action || (ok ? "继续" : "重新运行安装/修复");
  return `
    <div class="notice ${ok ? "" : "warn"}">
      <strong>资源下载：${escapeHtml(message)}</strong>
      <div class="compact-list">下一步：${escapeHtml(next)}</div>
    </div>`;
}

function installerExternalSourceSummary() {
  const external = state.setupRepair?.installer?.external_sources;
  const items = external?.items || [];
  if (!external || !items.length) return "";
  return `
    <div class="notice ${external.needs_action ? "warn" : ""}">
      <strong>大型资源：${external.ready || 0} / ${external.total || items.length} 就绪</strong>
      <ul class="compact-list">
        ${items
          .map(
            (item) =>
              `<li>${escapeHtml(item.label || "科学资源")}：${escapeHtml(item.status || "需要同步")}；${escapeHtml(item.next_action || "运行安装/修复")}</li>`
          )
          .join("")}
      </ul>
    </div>`;
}

function installerSourceRecordSummary() {
  const records =
    state.setup?.source_records ||
    state.setupRepair?.source_records ||
    state.setupPlan?.source_records ||
    state.setupRepair?.installer?.source_records;
  if (!records || records.total === undefined) return "";
  const ok = records.completed === true;
  const next = records.next_action || (ok ? "继续" : "运行安装/修复");
  const sets = records.record_sets || [];
  return `
    <div class="notice ${ok ? "" : "warn"}">
      <strong>来源记录：${records.ready || 0} / ${records.total || 0} 就绪</strong>
      <div class="compact-list">
        完整性记录 ${records.integrity_ready || 0} / ${records.total || 0}，许可证记录 ${records.license_ready || 0} / ${records.total || 0}；下一步：${escapeHtml(next)}
      </div>
      ${
        sets.length
          ? `<ul class="compact-list">${sets
              .map(
                (item) =>
                  `<li>${escapeHtml(item.label || "记录")}：${item.ready || 0} / ${item.total || 0} ${item.completed ? "就绪" : "需要补齐"}</li>`
              )
              .join("")}</ul>`
          : ""
      }
    </div>`;
}

function installerResourceSummary() {
  const resources = state.setupRepair?.installer?.resources;
  const items = resources?.items || [];
  if (!resources || !items.length) return "";
  return `
    <div class="notice ${resources.needs_action ? "warn" : ""}">
      <strong>科学资源：${resources.ready || 0} / ${resources.total || items.length} 就绪</strong>
      <ul class="compact-list">
        ${items
          .map(
            (item) =>
              `<li>${escapeHtml(item.label || "科学资源")}：${escapeHtml(item.status || "需要检查")}；${escapeHtml(item.next_action || "运行安装/修复")}</li>`
          )
          .join("")}
      </ul>
    </div>`;
}

function installerToolEnvironmentSummary() {
  const tools = state.setupRepair?.installer?.tool_environment;
  if (!tools || tools.total === undefined) return "";
  const ok = tools.completed === true;
  const message = tools.message || (ok ? "科学计算环境已就绪" : "科学计算环境需要准备");
  const next = tools.next_action || (ok ? "继续" : "运行安装/修复");
  return `
    <div class="notice ${ok ? "" : "warn"}">
      <strong>计算环境：${tools.ready || 0} / ${tools.total || 0} 就绪</strong>
      <div class="compact-list">${escapeHtml(message)}；下一步：${escapeHtml(next)}</div>
    </div>`;
}

function missingResourceSummary() {
  const missing = state.setupRepair?.missing || state.setup?.missing || state.databases?.missing || [];
  if (!missing.length) {
    return `<div class="notice">数据库、索引和工具记录已满足当前运行要求。</div>`;
  }
  return `
    <div class="notice warn">
      <strong>还需要准备 ${missing.length} 项：</strong>
      <ul class="compact-list">
        ${missing
          .map(
            (item) =>
              `<li>${escapeHtml(item.label || "科学资源")}：${escapeHtml(item.next_action || "运行安装/修复")}</li>`
          )
          .join("")}
      </ul>
    </div>`;
}

function diagnosticsSummary() {
  const checks = state.diagnostics?.checks || [];
  if (!checks.length) return "";
  return `
    <div class="panel-grid">
      ${checks
        .map(
          (row) => `
            <section class="panel compact third">
              <h3>${escapeHtml(row.label)}</h3>
              <p>${escapeHtml(row.detail || (row.ready ? "通过" : row.next_action || "需要处理"))}</p>
              <span class="status-pill ${row.ready ? "ready" : "blocked"}">${row.ready ? "就绪" : "待处理"}</span>
            </section>`
        )
        .join("")}
    </div>`;
}

function sourceStatusTable() {
  const sources = state.databases?.sources || [];
  if (!sources.length) {
    return `<div class="empty">尚未读取到数据库状态。运行安装/修复后会显示每个资源的准备情况。</div>`;
  }
  return table(
    sources,
    [
      { key: "label", label: "资源" },
      { key: "category", label: "类别" },
      { key: "status", label: "状态" },
      { key: "next_action", label: "下一步" },
    ],
  );
}
