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
            <p>这里显示当前运行阶段、下一步动作、资源状态和日志。</p>
          </div>
          <span class="status-pill ${runStateClass(runState)}">${escapeHtml(runLabel)}</span>
        </div>
        <div class="actions">
          <button class="button" data-action="start-run" ${canStartRun() ? "" : "disabled"}>开始运行</button>
          <button class="button secondary" data-action="refresh-run">刷新</button>
          <button class="button ghost" data-action="to-results" ${canViewResults() ? "" : "disabled"}>查看结果</button>
        </div>
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
      <section class="panel">
        <h2>资源计划</h2>
        ${metricRow("候选量", candidateForecastText())}
        ${metricRow("时间等级", runtimeClassText())}
        ${metricRow("GPU 建议", gpuAdviceText())}
        ${metricRow("CPU 路径", cpuPathText())}
        ${metricRow("磁盘需求", storageNeedText())}
      </section>
      <section class="panel">
        <h2>当前资源</h2>
        ${resourceStatusRows()}
      </section>
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>恢复建议</h2>
            <p>如果运行被阻塞，先处理这里列出的下一步，再重新检查。</p>
          </div>
          <span class="status-pill ${blockedActionRows().length ? "blocked" : "ready"}">${blockedActionRows().length ? "需要处理" : "无需处理"}</span>
        </div>
        ${failureRecoveryRows()}
      </section>
      <section class="panel full">
        <h2>可导出结果</h2>
        ${artifactRows()}
      </section>
      <section class="panel full">
        <details>
          <summary>运行事件</summary>
          <pre>${escapeHtml((state.runData?.logs?.logs || []).join("\n") || "暂无运行事件。")}</pre>
        </details>
      </section>
    </div>`;
  page.querySelector("[data-action='start-run']")?.addEventListener("click", startRun);
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
      <span>${escapeHtml(label)}</span>
      <span class="status-pill ${stageStatusClass(stage)}">${escapeHtml(statusLabel)}</span>
      ${reason ? `<small>${escapeHtml(reason)}</small>` : ""}
    </div>`;
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
