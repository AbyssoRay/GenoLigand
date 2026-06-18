function renderExport() {
  page.className = "page";
  const artifacts = state.run?.outputs?.items || [];
  const available = artifacts.filter((item) => item.exists);
  const runId = state.run?.run_id || "";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>导出材料</h2>
            <p>导出项只在当前运行生成相应输出后启用。</p>
          </div>
          <span class="status-pill ${outputStatusClass(available)}">${available.length ? `${available.length} 项可用` : "暂无可导出项"}</span>
        </div>
      </section>
      ${
        available.length
          ? available.map((item) => exportCard(friendlyArtifact(item.label || item.id), item.id, runId)).join("")
          : `<section class="panel full"><div class="empty">当前运行还没有可导出材料。完成运行并生成输出后才会显示下载项。</div></section>`
      }
    </div>`;
}

function exportCard(title, artifactId, runId) {
  const href = `/api/user/runs/${encodeURIComponent(runId)}/download/${encodeURIComponent(artifactId)}`;
  return `
    <section class="panel third">
      <h2>${title}</h2>
      <p>当前运行已生成。</p>
      <a class="button secondary" href="${href}">下载</a>
    </section>`;
}
