function renderResults() {
  page.className = "page";
  const candidates = candidateRows();
  const candidateDetails = state.runData?.detail?.candidate_details || [];
  const evidence = state.runData?.evidence?.evidence || [];
  const genome = state.runData?.genome?.genome_context || [];
  const structure = state.runData?.structure?.structure_rows || [];
  const md = state.runData?.md?.md_rows || [];
  page.innerHTML = `
    ${resultTracePanel()}
    <div class="tabs">
      <button class="tab active" data-result-tab="ranking">候选排序</button>
      <button class="tab" data-result-tab="detail">候选详情</button>
      <button class="tab" data-result-tab="evidence">证据链</button>
      <button class="tab" data-result-tab="genome">基因组/ORF</button>
      <button class="tab" data-result-tab="structure">结构/口袋</button>
      <button class="tab" data-result-tab="md">动力学摘要</button>
    </div>
    <div data-result-body>
      ${resultSection("候选排序", candidates, "ranking")}
    </div>`;
  page.querySelectorAll("[data-result-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      page.querySelectorAll(".tab").forEach((tab) => tab.classList.remove("active"));
      button.classList.add("active");
      const tab = button.dataset.resultTab;
      const body = page.querySelector("[data-result-body]");
      if (tab === "ranking") body.innerHTML = resultSection("候选排序", candidates, "ranking");
      if (tab === "detail") body.innerHTML = candidateDetailSection(candidateDetails);
      if (tab === "evidence") body.innerHTML = resultSection("证据链", evidence, "evidence");
      if (tab === "genome") body.innerHTML = resultSection("基因组/ORF", genome, "genome");
      if (tab === "structure") body.innerHTML = structureSection(structure);
      if (tab === "md") body.innerHTML = mdSection(md);
      hydrateResultVisuals();
    });
  });
}

function resultTracePanel() {
  const trace = state.run?.trace || {};
  const completed = state.run?.state === "completed";
  const projectText = trace.project_label || state.project?.project_name || (state.project ? "当前项目" : "未创建");
  const runText = state.run ? `${trace.run_label || "当前运行"}：${state.run.state_label || runStateLabel(state.run.state)}` : "未运行";
  const databaseText = trace.database_status || "未检查";
  const outputText = trace.output_status
    ? `${trace.output_status}${Number.isFinite(trace.output_count) ? `（${trace.output_count} 项）` : ""}`
    : "暂无输出";
  const candidateText = trace.candidate_status || (candidateRows().length ? "已生成候选排序" : "暂无候选排序");
  const evidenceText = trace.evidence_status || (state.runData?.evidence?.has_output ? "已生成证据链" : "暂无证据链");
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>结果来源</h2>
          <p>结果只读取当前项目和当前运行的输出；没有输出时保持为空。</p>
        </div>
        <span class="status-pill ${completed ? "ready" : "waiting"}">${completed ? "运行已完成" : "等待运行完成"}</span>
      </div>
      ${metricRow("当前项目", projectText)}
      ${metricRow("当前运行", runText)}
      ${metricRow("输入来源", trace.input_status || (state.project ? "已保存用户输入" : "未保存用户输入"))}
      ${metricRow("数据库来源", databaseText)}
      ${metricRow("输出材料", outputText)}
      ${metricRow("候选来源", candidateText)}
      ${metricRow("证据来源", evidenceText)}
    </section>`;
}

function resultSection(title, rows, kind) {
  const emptyText = {
    ranking: "当前没有候选。完成输入、检查和运行后才会显示候选排序。",
    evidence: "当前没有证据链输出。",
    genome: "当前没有宏基因组或 ORF 匹配输出。",
  }[kind] || "当前没有输出。";
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>${title}</h2>
          <p>${rows.length ? "数据来自当前运行输出。" : emptyText}</p>
        </div>
        <span class="status-pill ${outputStatusClass(rows)}">${rows.length ? `${rows.length} 行` : "空"}</span>
      </div>
      ${rows.length ? table(rows, columnsFor(kind)) : `<div class="empty">${emptyText}</div>`}
    </section>`;
}

function candidateDetailSection(rows) {
  if (!rows.length) {
    return `<section class="panel full"><h2>候选详情</h2><div class="empty">当前没有候选详情输出。完成运行并生成候选详情后才会显示。</div></section>`;
  }
  const first = rows[0];
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>第一候选摘要</h2>
          <p>摘要来自当前运行的候选详情输出；下方表格显示全部候选详情。</p>
        </div>
        <span class="status-pill ready">候选编号 ${escapeHtml(valueOrMissing(first.candidate_id))}</span>
      </div>
      ${metricRow("目标名称", valueOrMissing(first.target_name))}
      ${metricRow("目标编号", valueOrMissing(first.target_id))}
      ${metricRow("候选范围", friendlyScope(first.candidate_scope))}
      ${metricRow("物种", valueOrMissing(first.organism))}
      ${metricRow("综合分数", valueOrMissing(first.final_score))}
      ${metricRow("证据摘要", valueOrMissing(first.evidence_summary))}
      ${metricRow("详情来源", valueOrMissing(first.detail_source, "当前运行输出"))}
    </section>
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>全部候选详情</h2>
          <p>每一行都来自当前运行生成的候选详情输出。</p>
        </div>
        <span class="status-pill ready">${rows.length} 行</span>
      </div>
      ${table(rows, columnsFor("candidate_detail"))}
    </section>`;
}

function structureSection(rows) {
  const viewer = state.runData?.structure?.viewer;
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>结构/口袋</h2>
          <p>${rows.length ? "当前显示结构和口袋摘要；三维查看只在当前运行提供坐标时启用。" : "当前运行没有结构或口袋摘要；三维查看未就绪。"}</p>
        </div>
        <span class="status-pill ${outputStatusClass(rows)}">${rows.length ? `${rows.length} 行` : "未就绪"}</span>
      </div>
      ${visualStatus([
        { label: "摘要表", value: rows.length ? "已生成" : "未生成", ready: rows.length > 0 },
        { label: "三维查看器", value: viewer?.ready ? "已连接当前运行坐标" : "未就绪", ready: viewer?.ready === true, waiting: viewer?.ready !== true },
        { label: "数据来源", value: rows.length ? "当前运行输出" : "等待运行输出", ready: rows.length > 0 },
      ])}
      ${renderStructureViewer(viewer)}
      ${rows.length ? table(rows, columnsFor("structure")) : `<div class="empty">未生成结构查看数据或口袋摘要。</div>`}
    </section>`;
}

function mdSection(rows) {
  const readiness = state.runData?.md?.readiness || {};
  const charts = readiness.ready === true ? state.runData?.md?.charts || [] : [];
  const chartReady = readiness.chart_ready === true && charts.length > 0;
  const emptyText = readiness.reason || "未生成 RMSD、RMSF、接触、氢键或配体漂移摘要。";
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>动力学摘要</h2>
          <p>${rows.length ? "当前显示动力学表格和图形摘要；全部来自当前运行轨迹摘要或分数拆解。" : emptyText}</p>
        </div>
        <span class="status-pill ${outputStatusClass(rows)}">${rows.length ? `${rows.length} 行` : "未就绪"}</span>
      </div>
      ${visualStatus([
        { label: "摘要表", value: rows.length ? "已生成" : "未生成", ready: rows.length > 0 },
        { label: "轨迹图", value: chartReady ? "已生成图形摘要" : "轨迹图未就绪", ready: chartReady, waiting: !chartReady },
        { label: "数据来源", value: rows.length ? "当前运行输出" : emptyText, ready: rows.length > 0, waiting: !rows.length },
      ])}
      ${renderMdCharts(charts)}
      ${rows.length ? table(rows, columnsFor("md")) : `<div class="empty">${escapeHtml(emptyText)}</div>`}
    </section>`;
}

function visualStatus(items) {
  return `
    <div class="visual-status">
      ${items
        .map(
          (item) => `
            <div class="visual-status-item">
              <span>${escapeHtml(item.label)}</span>
              <strong class="${item.ready ? "ready-text" : item.waiting ? "waiting-text" : "blocked-text"}">${escapeHtml(item.value)}</strong>
            </div>`
        )
        .join("")}
    </div>`;
}
