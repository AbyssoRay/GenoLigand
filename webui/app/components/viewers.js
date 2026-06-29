function renderStructureViewer(viewer) {
  if (!viewer?.ready) {
    return `
      <div class="viewer-empty">
        <strong>三维查看未就绪</strong>
        <span>${escapeHtml(viewer?.reason || "当前运行没有可显示的结构坐标输出。")}</span>
      </div>`;
  }
  return `
    <div class="viewer-card">
      <div class="viewer-head">
        <div>
          <strong>三维结构查看</strong>
          <span>${escapeHtml(viewer.source || "当前运行结构坐标输出")} · ${escapeHtml(viewer.artifact || "结构坐标")}</span>
        </div>
        <span class="status-pill ready">${Number(viewer.atom_count || 0)} atoms</span>
      </div>
      <canvas data-structure-viewer width="960" height="420" aria-label="三维结构查看器"></canvas>
      <div class="viewer-help">拖动旋转，滚轮缩放。坐标来自当前运行输出。</div>
    </div>`;
}

function hydrateStructureViewer() {
  const canvas = document.querySelector("[data-structure-viewer]");
  const viewer = state.runData?.structure?.viewer;
  if (!canvas || !viewer?.ready || !Array.isArray(viewer.atoms) || !viewer.atoms.length) return;
  drawStructure(canvas, viewer.atoms);
}

function drawStructure(canvas, atoms) {
  const ctx = canvas.getContext("2d");
  const coords = atoms
    .map((atom) => ({
      x: Number(atom.x),
      y: Number(atom.y),
      z: Number(atom.z),
      element: String(atom.element || "C").toUpperCase(),
    }))
    .filter((atom) => Number.isFinite(atom.x) && Number.isFinite(atom.y) && Number.isFinite(atom.z));
  if (!coords.length) return;
  let angleX = -0.45;
  let angleY = 0.62;
  let zoom = 1;
  let dragging = false;
  let last = null;
  const center = coords.reduce(
    (acc, atom) => ({ x: acc.x + atom.x / coords.length, y: acc.y + atom.y / coords.length, z: acc.z + atom.z / coords.length }),
    { x: 0, y: 0, z: 0 },
  );
  const palette = {
    H: "#dce7ea",
    C: "#44d7c7",
    N: "#78a8ff",
    O: "#ff8a8a",
    S: "#f5c46b",
    P: "#d79cff",
    CL: "#73d79c",
  };

  function project(atom) {
    const x0 = atom.x - center.x;
    const y0 = atom.y - center.y;
    const z0 = atom.z - center.z;
    const cosY = Math.cos(angleY);
    const sinY = Math.sin(angleY);
    const cosX = Math.cos(angleX);
    const sinX = Math.sin(angleX);
    const x1 = x0 * cosY - z0 * sinY;
    const z1 = x0 * sinY + z0 * cosY;
    const y1 = y0 * cosX - z1 * sinX;
    const z2 = y0 * sinX + z1 * cosX;
    const scale = Math.min(canvas.width, canvas.height) / 42 * zoom;
    return {
      x: canvas.width / 2 + x1 * scale,
      y: canvas.height / 2 + y1 * scale,
      z: z2,
      element: atom.element,
    };
  }

  function render() {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#061014";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    const projected = coords.map(project).sort((a, b) => a.z - b.z);
    for (const atom of projected) {
      const radius = Math.max(2.5, Math.min(7, 4.2 + atom.z * 0.025));
      ctx.beginPath();
      ctx.fillStyle = palette[atom.element] || palette.C;
      ctx.globalAlpha = 0.82;
      ctx.arc(atom.x, atom.y, radius, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalAlpha = 1;
    }
  }

  canvas.addEventListener("pointerdown", (event) => {
    dragging = true;
    last = { x: event.clientX, y: event.clientY };
    canvas.setPointerCapture(event.pointerId);
  });
  canvas.addEventListener("pointermove", (event) => {
    if (!dragging || !last) return;
    angleY += (event.clientX - last.x) * 0.01;
    angleX += (event.clientY - last.y) * 0.01;
    last = { x: event.clientX, y: event.clientY };
    render();
  });
  canvas.addEventListener("pointerup", () => {
    dragging = false;
    last = null;
  });
  canvas.addEventListener("wheel", (event) => {
    event.preventDefault();
    zoom = Math.max(0.35, Math.min(4, zoom + (event.deltaY < 0 ? 0.08 : -0.08)));
    render();
  }, { passive: false });
  render();
}

function renderMdCharts(charts) {
  const items = Array.isArray(charts) ? charts : [];
  if (!items.length) {
    return `
      <div class="viewer-empty">
        <strong>动力学图未就绪</strong>
        <span>当前运行没有真实 MD 摘要图形输出。</span>
      </div>`;
  }
  const max = Math.max(...items.map((item) => Number(item.value) || 0), 1);
  return `
    <div class="chart-grid">
      ${items
        .map((item) => {
          const value = Number(item.value) || 0;
          const width = Math.max(3, Math.min(100, (value / max) * 100));
          return `
            <div class="chart-row">
              <span>${escapeHtml(item.label)}</span>
              <div class="chart-bar"><i style="width:${width}%"></i></div>
              <strong>${escapeHtml(item.display_value || String(value))}</strong>
              <em>${escapeHtml(item.artifact || "当前运行输出")}</em>
            </div>`;
        })
        .join("")}
    </div>`;
}

function hydrateResultVisuals() {
  hydrateStructureViewer();
}
