function renderResults() {
  page.className = "page";
  const activeTab = state.resultTab || "ranking";
  page.innerHTML = `
    ${resultTracePanel()}
    <div class="tabs">
      ${["ranking", "detail", "evidence", "genome", "structure", "md"]
        .map((tab) => `<button class="tab ${activeTab === tab ? "active" : ""}" data-result-tab="${tab}">${resultTabLabel(tab)}</button>`)
        .join("")}
    </div>
    <div data-result-body>
      ${renderResultTab(activeTab)}
    </div>`;
  page.querySelectorAll("[data-result-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      state.resultTab = button.dataset.resultTab;
      renderResults();
    });
  });
  hydrateResultPagination();
  hydrateResultVisuals();
}

function resultTabLabel(tab) {
  return {
    ranking: "候选排序",
    detail: "候选详情",
    evidence: "证据链",
    genome: "基因组/ORF",
    structure: "结构/口袋",
    md: "动力学摘要",
  }[tab] || tab;
}

function renderResultTab(tab) {
  if (tab === "ranking") return resultSection("候选排序", candidateRows(), "ranking", state.runData?.candidates?.pagination);
  if (tab === "detail") return candidateDetailSection(state.runData?.detail?.candidate_details || [], state.runData?.detail?.pagination);
  if (tab === "evidence") return resultSection("证据链", state.runData?.evidence?.evidence || [], "evidence", state.runData?.evidence?.pagination);
  if (tab === "genome") return resultSection("基因组/ORF", state.runData?.genome?.genome_context || [], "genome", state.runData?.genome?.pagination);
  if (tab === "structure") return structureSection(state.runData?.structure?.structure_rows || [], state.runData?.structure?.pagination);
  if (tab === "md") return mdSection(state.runData?.md?.md_rows || [], state.runData?.md?.pagination);
  return resultSection("候选排序", candidateRows(), "ranking", state.runData?.candidates?.pagination);
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

function resultSection(title, rows, kind, pagination) {
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
      ${paginationSummary(pagination, rows)}
      ${paginationControls(pagination, kind)}
      ${rows.length ? table(rows, columnsFor(kind)) : `<div class="empty">${emptyText}</div>`}
    </section>`;
}

function candidateDetailSection(rows, pagination) {
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
      ${metricRow("输入编号", valueOrMissing(first.input_id))}
      ${metricRow("记录编号", valueOrMissing(first.record_id))}
      ${metricRow("输入类型", friendlyInputKind(first.input_kind))}
      ${metricRow("排序路线", friendlyRankingPolicy(first.ranking_policy))}
      ${metricRow("路线权重", rankingWeightSummary(first))}
      ${metricRow("目标名称", valueOrMissing(first.target_name))}
      ${metricRow("目标编号", valueOrMissing(first.target_id))}
      ${metricRow("候选范围", valueOrMissing(first.candidate_scope_label, friendlyScope(first.candidate_scope)))}
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
      ${paginationSummary(pagination, rows)}
      ${paginationControls(pagination, "detail")}
      ${table(rows, columnsFor("candidate_detail"))}
    </section>`;
}

function structureSection(rows, pagination) {
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
      ${paginationSummary(pagination, rows)}
      ${paginationControls(pagination, "structure")}
      ${renderStructureViewer(viewer)}
      ${rows.length ? table(rows, columnsFor("structure")) : `<div class="empty">未生成结构查看数据或口袋摘要。</div>`}
    </section>`;
}

function mdSection(rows, pagination) {
  const readiness = state.runData?.md?.readiness || {};
  const protocol = state.runData?.md?.protocol || {};
  const stateTrace = state.runData?.md?.state_trace || {};
  const cuda = state.runData?.md?.cuda_monitoring || {};
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
      ${mdRuntimeStatus(protocol, stateTrace, cuda)}
      ${paginationSummary(pagination, rows)}
      ${paginationControls(pagination, "md")}
      ${renderMdCharts(charts)}
      ${rows.length ? table(rows, columnsFor("md")) : `<div class="empty">${escapeHtml(emptyText)}</div>`}
    </section>`;
}

function paginationSummary(pagination, rows) {
  const meta = pagination || {};
  const total = Number.isFinite(meta.total_rows) ? meta.total_rows : rows.length;
  const returned = Number.isFinite(meta.returned_rows) ? meta.returned_rows : rows.length;
  const page = Number.isFinite(meta.page) ? meta.page : 1;
  const pageSize = Number.isFinite(meta.page_size) ? meta.page_size : returned;
  const filterText = meta.filters_active ? "过滤已启用" : "未启用过滤";
  return `
    <div class="pagination-summary">
      <span>总数 ${escapeHtml(total)}</span>
      <span>当前页 ${escapeHtml(page)}</span>
      <span>本页 ${escapeHtml(returned)} / ${escapeHtml(pageSize)}</span>
      <span>${filterText}</span>
    </div>`;
}

function paginationControls(pagination, kind) {
  const meta = pagination || {};
  const page = Number.isFinite(meta.page) ? meta.page : 1;
  const filter = resultFilterValue(kind, meta);
  return `
    <div class="pagination-controls">
      <button class="button secondary" data-page-action="prev" data-result-kind="${kind}" data-page-target="${Math.max(1, page - 1)}" ${meta.has_previous_page ? "" : "disabled"}>上一页</button>
      <button class="button secondary" data-page-action="next" data-result-kind="${kind}" data-page-target="${page + 1}" ${meta.has_next_page ? "" : "disabled"}>下一页</button>
      <label class="pagination-filter">
        <span>过滤</span>
        <input data-result-filter="${kind}" value="${escapeAttr(filter)}" placeholder="目标、来源或状态" />
      </label>
      <button class="button secondary" data-filter-action="apply" data-result-kind="${kind}">应用</button>
    </div>`;
}

function resultFilterValue(kind, pagination) {
  const saved = state.resultQueries?.[kind]?.filter;
  if (saved !== undefined) return saved;
  return Array.isArray(pagination?.active_filters) ? pagination.active_filters[0] || "" : "";
}

function hydrateResultPagination() {
  page.querySelectorAll("[data-page-action]").forEach((button) => {
    button.addEventListener("click", async () => {
      const kind = button.dataset.resultKind;
      const targetPage = Number(button.dataset.pageTarget || 1);
      const filter = page.querySelector(`[data-result-filter="${kind}"]`)?.value || "";
      await loadResultPage(kind, targetPage, filter);
    });
  });
  page.querySelectorAll("[data-filter-action='apply']").forEach((button) => {
    button.addEventListener("click", async () => {
      const kind = button.dataset.resultKind;
      const filter = page.querySelector(`[data-result-filter="${kind}"]`)?.value || "";
      await loadResultPage(kind, 1, filter);
    });
  });
  page.querySelectorAll("[data-result-filter]").forEach((input) => {
    input.addEventListener("keydown", async (event) => {
      if (event.key !== "Enter") return;
      await loadResultPage(input.dataset.resultFilter, 1, input.value || "");
    });
  });
}

async function loadResultPage(kind, pageNumber, filterText) {
  if (!state.run?.run_id) return;
  const runId = state.run.run_id;
  const config = resultEndpointConfig(kind);
  if (!config) return;
  const pageNumberSafe = Math.max(1, Number(pageNumber) || 1);
  const currentPagination = state.runData?.[config.key]?.pagination || {};
  const pageSize = Number.isFinite(currentPagination.page_size) ? currentPagination.page_size : 500;
  const filter = String(filterText || "").trim();
  const params = new URLSearchParams();
  params.set("page", String(pageNumberSafe));
  params.set("page_size", String(pageSize));
  if (filter) params.set("filter", filter);
  state.resultQueries = state.resultQueries || {};
  state.resultQueries[kind] = { page: pageNumberSafe, filter };
  state.resultTab = kind;
  try {
    const result = await runResultRequest(
      async (signal) => {
        const value = await api(`/api/user/runs/${runId}/${config.endpoint}?${params.toString()}`, { signal });
        if (signal.aborted || state.run?.run_id !== runId) return null;
        state.runData = state.runData || {};
        state.runData[config.key] = value;
        return value;
      },
      { replace: true },
    );
    if (result) render();
  } catch (error) {
    state.alert = `结果分页加载失败：${error.message}`;
    render();
  }
}

function resultEndpointConfig(kind) {
  return {
    ranking: { key: "candidates", endpoint: "candidates" },
    detail: { key: "detail", endpoint: "candidate-detail" },
    evidence: { key: "evidence", endpoint: "evidence" },
    genome: { key: "genome", endpoint: "genome-context" },
    structure: { key: "structure", endpoint: "structure" },
    md: { key: "md", endpoint: "md-summary" },
  }[kind];
}

function mdRuntimeStatus(protocol, stateTrace, cuda) {
  const platform = protocol.ready ? valueOrMissing(protocol.platform) : valueOrMissing(cuda.platform, "未生成");
  const protocolScope = protocol.candidate_count > 1 ? `${protocol.candidate_count} 个候选：` : "";
  const stateScope = stateTrace.candidate_count > 1 ? `${stateTrace.candidate_count} 个候选，共` : "";
  const cudaScope = cuda.candidate_count > 1 ? `${cuda.candidate_count} 个候选，` : "";
  const cudaReady = cuda.ready === true;
  const cudaCount = cuda["production_" + "sam" + "ple_count"];
  const cudaPeak = cuda["production_max_utilization_gpu_percent"];
  const cudaText = cuda.ready
    ? `${cudaScope}生产阶段 ${valueOrMissing(cudaCount, "0")} 次采样，峰值 ${valueOrMissing(cudaPeak, "0")} %`
    : cuda.reason || cuda.failure_reason || "当前运行没有 CUDA 生产阶段采样证据";
  return visualStatus([
    { label: "OpenMM 平台", value: `${protocolScope}${platform}`, ready: protocol.ready === true },
    {
      label: "MD 协议",
      value: protocol.ready ? `${protocolScope}${valueOrMissing(protocol.production_time, "未记录")} production，${valueOrMissing(protocol.time_step, "未记录")} 步长` : protocol.reason || "未生成",
      ready: protocol.ready === true,
      waiting: protocol.ready !== true,
    },
    {
      label: "状态序列",
      value: stateTrace.ready ? `${stateScope}${valueOrMissing(stateTrace.row_count, "0")} 行，最长 ${valueOrMissing(stateTrace.last_time_ps, "0")} ps` : stateTrace.reason || "未生成",
      ready: stateTrace.ready === true,
      waiting: stateTrace.ready !== true,
    },
    {
      label: "CUDA 监测",
      value: cudaText,
      ready: cudaReady,
      waiting: !cudaReady,
    },
  ]);
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
