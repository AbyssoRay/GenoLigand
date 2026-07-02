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
          ? available.map((item) => reportCard(friendlyArtifact(item.label || item.id), item.id, runId)).join("")
          : `<section class="panel full"><div class="empty">当前运行还没有可下载报告材料。完成运行并生成输出后才会显示下载项。</div></section>`
      }
    </div>`;
}

function reportCard(title, artifactId, runId) {
  const href = `/api/user/runs/${encodeURIComponent(runId)}/download/${encodeURIComponent(artifactId)}`;
  return `
    <section class="panel third">
      <h2>${title}</h2>
      <p>当前运行已生成。</p>
      <a class="button secondary" href="${href}">下载</a>
    </section>`;
}
