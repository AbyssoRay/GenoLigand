function renderRun() {
  page.className = "page";
  const runState = state.run?.state || "not_started";
  const runLabel = state.run?.state_label || runStateLabel(runState);
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>运行中心</h2>
            <p>这里显示当前运行阶段、下一步动作和日志。</p>
          </div>
          <span class="status-pill ${runStateClass(runState)}">${escapeHtml(runLabel)}</span>
        </div>
        <div class="actions">
          <button class="button" data-action="start-run" ${canStartRun() ? "" : "disabled"}>开始运行</button>
          <button class="button secondary" data-action="cancel-run" ${runState === "running" && !state.runCancelPending ? "" : "disabled"}>${state.runCancelPending ? "正在取消" : "取消运行"}</button>
          <button class="button secondary" data-action="refresh-run">刷新</button>
          <button class="button ghost" data-action="to-results" ${canViewResults() ? "" : "disabled"}>查看结果</button>
        </div>
        ${renderRunProgress()}
      </section>
      <section class="panel">
        <h2>流程阶段</h2>
        <div class="stage-list">${runStageRows().map(renderStage).join("")}</div>
      </section>
      <section class="panel">
        <h2>运行摘要</h2>
        ${metricRow("当前状态", runLabel)}
        ${metricRow("下一步", nextActionText())}
        ${metricRow("候选数量", candidateCountText())}
        ${metricRow("阻塞原因", state.run?.failure_summary || "无")}
      </section>
      <section class="panel full">
        <details>
          <summary>运行事件</summary>
          <pre>${escapeHtml((state.runData?.logs?.logs || []).join("\n") || "暂无运行事件。")}</pre>
        </details>
      </section>
    </div>`;
  page.querySelector("[data-action='start-run']")?.addEventListener("click", startRun);
  page.querySelector("[data-action='cancel-run']")?.addEventListener("click", cancelRun);
  page.querySelector("[data-action='refresh-run']").addEventListener("click", refreshAll);
  page.querySelector("[data-action='to-results']")?.addEventListener("click", () => setStep("results"));
}

function runStageRows() {
  if (Array.isArray(state.run?.stages) && state.run.stages.length) return state.run.stages;
  return stageLabels.map((label) => ({ label, status: "waiting", status_label: "等待", reason: "" }));
}

function renderStage(stage, index) {
  const label = typeof stage === "string" ? stage : stage.label;
  const status = typeof stage === "string" ? "waiting" : stage.status || "waiting";
  const statusLabel = typeof stage === "string" ? "等待" : stage.status_label || stageStatusLabel(status);
  const reason = typeof stage === "string" ? "" : stage.reason || "";
  return `
    <div class="stage">
      <span class="stage-index">${index + 1}</span>
      <span class="stage-main">
        <span class="stage-label">${escapeHtml(label)}</span>
        ${reason ? `<small class="stage-reason">${escapeHtml(reason)}</small>` : ""}
      </span>
      <span class="status-pill ${stageStatusClass(stage)}">${escapeHtml(statusLabel)}</span>
    </div>`;
}

function renderRunProgress() {
  const stages = runStageRows();
  const percent = runProgressPercent(stages);
  const current = stages.find((stage) => stage.status === "running");
  const done = stages.filter((stage) => ["ready", "not_generated"].includes(stage.status)).length;
  const blocked = state.run?.state === "blocked" || state.run?.state === "failed";
  const ready = state.run?.state === "completed";
  const currentLabel = current?.label || (ready ? "运行完成" : state.run?.current_stage || "等待开始");
  return `
    <div class="progress-panel ${ready ? "ready" : blocked ? "blocked" : ""}">
      <div class="progress-head">
        <div>
          <strong>${escapeHtml(currentLabel)}</strong>
          <span>${escapeHtml(nextActionText())}</span>
        </div>
        <strong>${percent}%</strong>
      </div>
      <div class="progress-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent}">
        <span style="width: ${percent}%"></span>
      </div>
      <div class="progress-meta">
        <span>${done} / ${stages.length} 阶段完成</span>
        <span>${escapeHtml(state.run?.state_label || runStateLabel(state.run?.state || "not_started"))}</span>
      </div>
    </div>`;
}

function runProgressPercent(stages = runStageRows()) {
  if (!stages.length) return 0;
  if (state.run?.state === "completed") return 100;
  const completed = stages.filter((stage) => ["ready", "not_generated"].includes(stage.status)).length;
  const running = stages.some((stage) => stage.status === "running") ? 0.5 : 0;
  return Math.max(0, Math.min(100, Math.round(((completed + running) / stages.length) * 100)));
}

function stageStatusClass(stage) {
  if (stage.status === "ready") return "ready";
  if (stage.status === "running") return "loading";
  if (stage.status === "not_generated") return "waiting";
  if (stage.status === "blocked") return "blocked";
  return "waiting";
}

function stageStatusLabel(status) {
  if (status === "ready") return "完成";
  if (status === "running") return "进行中";
  if (status === "not_generated") return "未生成";
  if (status === "blocked") return "需处理";
  return "等待";
}
