function renderInput() {
  page.className = "page";
  const active = inputSpecs.find((item) => item.kind === state.selectedInput) || inputSpecs[0];
  const activeValidation = inputValidation(active);
  const saveDisabled = inputsReadyToSave() ? "" : "disabled";
  page.innerHTML = `
    <div class="panel-grid">
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>选择输入类型</h2>
            <p>可以填写一种或多种输入。只有用户填写的内容会进入计算。</p>
          </div>
          <span class="status-pill ${inputStatusClass()}" data-input-status>${inputStatusLabel()}</span>
        </div>
        <div class="input-grid">
          ${inputSpecs.map(renderInputTypeCard).join("")}
        </div>
      </section>
      <section class="panel full">
        <div class="panel-header">
          <div>
            <h2>${active.title}</h2>
            <p>${active.label}</p>
          </div>
          <span class="status-pill">${active.minimum}</span>
        </div>
        <div class="field">
          <label for="project_name">项目名称</label>
          <input id="project_name" data-form="project_name" value="${escapeAttr(state.form.project_name)}" placeholder="可选；不填时使用自动名称" />
        </div>
        <div class="field">
          <label for="${active.field}">${active.label}</label>
          <textarea id="${active.field}" data-form="${active.field}" placeholder="${escapeAttr(active.placeholder)}">${escapeHtml(state.form[active.field])}</textarea>
          <div class="field-help ${activeValidation.ready ? "ready-text" : activeValidation.value ? "blocked-text" : ""}" data-active-validation>${escapeHtml(activeValidation.message)}</div>
        </div>
        <div class="actions">
          <button class="button" data-action="save-input" data-save-button ${saveDisabled}>保存输入</button>
          <button class="button secondary" data-action="save-and-check" data-save-button ${saveDisabled}>保存并检查</button>
          <button class="button ghost" data-action="clear-input">清空当前输入</button>
        </div>
      </section>
      <section class="panel third">
        <h2>输入状态</h2>
        ${metricRow("填写类型", `${filledInputs().length} / ${inputSpecs.length}`)}
        ${metricRow("可用输入", `${usableInputs().length} / ${inputSpecs.length}`)}
        ${metricRow("候选显示", "运行完成前不显示候选")}
        ${metricRow("结果来源", "仅读取当前项目运行输出")}
      </section>
      <section class="panel third">
        <h2>下一步</h2>
        <p>${inputsReadyToSave() ? "保存后进入检查。" : "先完成当前输入检查。"}</p>
      </section>
    </div>`;
  bindInputEvents();
}

function renderInputTypeCard(spec) {
  const selected = state.selectedInput === spec.kind;
  const value = state.form[spec.field].trim();
  const filled = value.length > 0;
  const usable = inputValueUsable(spec.field, value);
  return `
    <button class="input-card ${selected ? "selected" : ""}" data-select-input="${spec.kind}">
      <h3>${spec.title}</h3>
      <p>${spec.minimum}</p>
      <span class="status-pill ${inputCardStatusClass(usable, filled)}" data-input-card-status="${spec.field}">${usable ? "可用" : filled ? "需检查" : "未填写"}</span>
    </button>`;
}

function bindInputEvents() {
  page.querySelectorAll("[data-select-input]").forEach((button) => {
    button.addEventListener("click", () => {
      collectForm();
      state.selectedInput = button.dataset.selectInput;
      renderInput();
    });
  });
  page.querySelectorAll("[data-form]").forEach((field) => {
    field.addEventListener("input", () => {
      collectForm();
      updateInputValidationUi();
    });
  });
  page.querySelector("[data-action='save-input']").addEventListener("click", saveInput);
  page.querySelector("[data-action='save-and-check']").addEventListener("click", async () => {
    const saved = await saveInput();
    if (saved) {
      await runPreflight();
      setStep("check");
    }
  });
  page.querySelector("[data-action='clear-input']").addEventListener("click", () => {
    const active = inputSpecs.find((item) => item.kind === state.selectedInput);
    state.form[active.field] = "";
    renderInput();
  });
}

function updateInputValidationUi() {
  const active = inputSpecs.find((item) => item.kind === state.selectedInput) || inputSpecs[0];
  const validation = inputValidation(active);
  const status = page.querySelector("[data-input-status]");
  if (status) {
    status.textContent = inputStatusLabel();
    status.className = `status-pill ${inputStatusClass()}`;
  }
  const activeHelp = page.querySelector("[data-active-validation]");
  if (activeHelp) {
    activeHelp.textContent = validation.message;
    activeHelp.className = `field-help ${validation.ready ? "ready-text" : validation.value ? "blocked-text" : ""}`;
  }
  page.querySelectorAll("[data-save-button]").forEach((button) => {
    button.disabled = !inputsReadyToSave();
  });
  inputSpecs.forEach((spec) => {
    const statusNode = page.querySelector(`[data-input-card-status="${spec.field}"]`);
    if (!statusNode) return;
    const value = (state.form[spec.field] || "").trim();
    const usable = inputValueUsable(spec.field, value);
    statusNode.textContent = usable ? "可用" : value ? "需检查" : "未填写";
    statusNode.className = `status-pill ${inputCardStatusClass(usable, Boolean(value))}`;
  });
}
