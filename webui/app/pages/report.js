function renderReport() {
  page.className = "page";
  const outputs = state.run?.outputs?.items || [];
  const available = outputs.filter((item) => item.exists);
  const runId = state.run?.run_id || "";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>报告材料</h2>
            <p>报告、候选表和证据摘要在当前运行生成相应输出后启用下载。</p>
          </div>
          <span class="status-pill ${outputStatusClass(available)}">${available.length ? `${available.length} 项可下载` : "暂无可下载项"}</span>
        </div>
      </section>
      ${
        available.length
          ? available.map((item) => reportCard(friendlyArtifact(item.label || item.id), item.id)).join("")
          : `<section class="panel full"><div class="empty">当前运行还没有可下载报告材料。完成运行并生成输出后才会显示下载项。</div></section>`
      }
    </div>`;
  page.querySelectorAll("[data-download-artifact]").forEach((button) => {
    button.addEventListener("click", async () => {
      const artifactId = button.dataset.downloadArtifact;
      button.disabled = true;
      button.textContent = "正在下载";
      try {
        await downloadFile(
          `/api/user/runs/${encodeURIComponent(runId)}/download/${encodeURIComponent(artifactId)}`,
          artifactDownloadName(runId, artifactId),
        );
      } catch (error) {
        state.alert = `下载失败：${error.message}`;
        render();
        return;
      }
      button.disabled = false;
      button.textContent = "下载";
    });
  });
}

function reportCard(title, artifactId) {
  return `
    <section class="panel third">
      <h2>${title}</h2>
      <p>当前运行已生成。</p>
      <button class="button secondary" data-download-artifact="${escapeAttr(artifactId)}">下载</button>
    </section>`;
}

function artifactDownloadName(runId, artifactId) {
  const runName = String(runId || "run").replace(/[^a-zA-Z0-9._-]+/g, "-");
  const names = {
    query_structure: "query-structure.tsv",
    onsite_search: "onsite-search.tsv",
    candidate_shortlist: "candidate-shortlist.tsv",
    docking_summary: "docking-summary.tsv",
    structure_patch: "structure-pocket-summary.tsv",
    md_summary: "md-summary.tsv",
    md_protocol: "md-protocol.tsv",
    md_state: "md-state.tsv",
    gpu_telemetry: "gpu-telemetry.tsv",
    candidate_ranking: "candidate-ranking.tsv",
    evidence: "evidence.tsv",
    research_report: "research-report.md",
  };
  return `${runName}-${names[artifactId] || `${artifactId}.tsv`}`;
}
