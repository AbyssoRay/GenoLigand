function renderProject() {
  page.className = "page";
  const projects = state.projects || [];
  const current = state.project;
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>项目</h2>
            <p>新建研究项目，或进入已有项目继续运行和查看结果。</p>
          </div>
          <span class="status-pill ${current ? "ready" : projects.length ? "waiting" : "waiting"}">${projectCountLabel(projects)}</span>
        </div>
        <div class="actions">
          <button class="button" data-action="new-project">新建项目</button>
          <button class="button secondary" data-refresh-projects>刷新项目</button>
          <button class="button ghost" data-delete-current-project ${current ? "" : "disabled"}>删除当前项目</button>
        </div>
      </section>
      ${currentProjectSection(current)}
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>历史项目</h2>
            <p>${projects.length ? "选择项目进入输入、运行或结果。" : "当前没有已保存的历史项目。"}</p>
          </div>
          <span class="status-pill ${projects.length ? "ready" : "waiting"}">${projects.length} 个项目</span>
        </div>
        ${
          projects.length
            ? `<div class="input-grid">${projects.map(renderProjectCard).join("")}</div>`
            : `<div class="empty">保存有效输入后，项目会出现在这里。</div>`
        }
      </section>
    </div>`;
  bindProjectEvents();
}

function currentProjectSection(project) {
  if (!project) {
    return `
      <section class="panel full">
        <h2>当前项目</h2>
        <div class="empty">当前未进入项目。可以新建项目，或从历史项目进入。</div>
      </section>`;
  }
  return `
    <section class="panel full">
      <div class="panel-header">
        <div>
          <h2>当前项目</h2>
          <p>${escapeHtml(project.project_name || "未命名研究项目")}</p>
        </div>
        <span class="status-pill ${project.latest_run_id ? "ready" : "waiting"}">${escapeHtml(project.project_state || "等待输入")}</span>
      </div>
      ${metricRow("项目名称", project.project_name || "未命名研究项目")}
      ${metricRow("运行状态", project.project_state || "等待输入")}
      ${metricRow("最近运行", project.latest_run_id ? "已有运行记录" : "暂无运行记录")}
      <div class="actions">
        <button class="button" data-action="to-input">进入输入</button>
        <button class="button secondary" data-action="to-run" ${project.latest_run_id ? "" : "disabled"}>进入运行</button>
        <button class="button secondary" data-action="to-results" ${canViewResults() ? "" : "disabled"}>查看结果</button>
      </div>
    </section>`;
}

function renderProjectCard(project) {
  const projectId = project.project_id || "";
  const selected = state.project?.project_id === projectId;
  const hasRun = Boolean(project.latest_run_id);
  return `
    <section class="input-card ${selected ? "selected" : ""}">
      <h3>${escapeHtml(project.project_name || "未命名研究项目")}</h3>
      <p>${hasRun ? "可继续上次运行或查看结果" : "可继续编辑输入"}</p>
      <span class="status-pill ${hasRun ? "ready" : "waiting"}">${escapeHtml(project.project_state || "等待输入")}</span>
      <div class="actions">
        <button class="button secondary" data-open-project="${escapeAttr(projectId)}">进入</button>
        <button class="button ghost" data-delete-project="${escapeAttr(projectId)}">删除</button>
      </div>
    </section>`;
}

function projectCountLabel(projects) {
  if (state.project) return "已进入项目";
  return projects.length ? `${projects.length} 个历史项目` : "空白工作台";
}

function bindProjectEvents() {
  page.querySelector("[data-action='new-project']")?.addEventListener("click", createNewProject);
  page.querySelector("[data-refresh-projects]")?.addEventListener("click", refreshAll);
  page.querySelector("[data-delete-current-project]")?.addEventListener("click", () => {
    if (state.project?.project_id) deleteExistingProject(state.project.project_id);
  });
  page.querySelector("[data-action='to-input']")?.addEventListener("click", () => setStep("input"));
  page.querySelector("[data-action='to-run']")?.addEventListener("click", () => setStep("run"));
  page.querySelector("[data-action='to-results']")?.addEventListener("click", () => setStep("results"));
  page.querySelectorAll("[data-open-project]").forEach((button) => {
    button.addEventListener("click", () => openExistingProject(button.dataset.openProject));
  });
  page.querySelectorAll("[data-delete-project]").forEach((button) => {
    button.addEventListener("click", () => deleteExistingProject(button.dataset.deleteProject));
  });
}
