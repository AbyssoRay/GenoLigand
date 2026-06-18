function renderConfigure() {
  page.className = "page";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>搜索范围</h2>
            <p>数据库和索引由安装流程维护。普通运行不需要配置路径。</p>
          </div>
          <span class="status-pill ${state.project ? "ready" : "waiting"}">${state.project ? "已保存输入" : "等待输入"}</span>
        </div>
        <div class="panel-grid">
          ${scopeCard("已知蛋白数据库", "ChEMBL、BindingDB、UniProt、结构互作证据。", "known")}
          ${scopeCard("宏基因组潜在序列", "公共和自定义序列空间中的潜在靶点。", "metagenome")}
          ${scopeCard("结构与口袋证据", "结构、口袋、界面、对接和短动力学摘要。", "structure")}
        </div>
        <div class="actions">
          <button class="button" data-action="to-check" ${state.project ? "" : "disabled"}>进入检查</button>
          <button class="button ghost" data-action="to-input">返回输入</button>
        </div>
      </section>
      <section class="panel">
        <h2>运行边界</h2>
        ${metricRow("空输入", "不生成候选")}
        ${metricRow("数据库缺失", "阻塞运行并给出修复动作")}
        ${metricRow("结果解释", "计算预测，需要后续实验验证")}
      </section>
      <section class="panel">
        <h2>资源提示</h2>
        ${metricRow("GPU", "OpenMM/CUDA 可用时用于动力学筛选")}
        ${metricRow("CPU", "可完成输入归一化和数据库比对")}
        ${metricRow("磁盘", "完整科学仓库由安装流程准备")}
      </section>
    </div>`;
  page.querySelector("[data-action='to-check']")?.addEventListener("click", async () => {
    await runPreflight();
    setStep("check");
  });
  page.querySelector("[data-action='to-input']").addEventListener("click", () => setStep("input"));
}

function scopeCard(title, body, key) {
  const ready = sourceReady(key);
  return `
    <section class="panel compact third">
      <h3>${title}</h3>
      <p>${body}</p>
      <span class="status-pill ${ready ? "ready" : "blocked"}">${ready ? "就绪" : "需要准备"}</span>
    </section>`;
}
