async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...options,
  });
  const text = await response.text();
  const value = text ? JSON.parse(text) : {};
  if (!response.ok) {
    throw new Error(value.error || `请求失败：${response.status}`);
  }
  return value;
}

async function apiText(path, options = {}) {
  const response = await fetch(path, options);
  const text = await response.text();
  if (!response.ok) {
    let value = {};
    try {
      value = text ? JSON.parse(text) : {};
    } catch {
      value = {};
    }
    const error = new Error(value.error || `请求失败：${response.status}`);
    error.status = response.status;
    throw error;
  }
  return text;
}

async function downloadFile(path, filename) {
  const response = await fetch(path);
  if (!response.ok) {
    const text = await response.text();
    let value = {};
    try {
      value = text ? JSON.parse(text) : {};
    } catch {
      value = {};
    }
    throw new Error(value.error || `下载失败：${response.status}`);
  }
  const objectUrl = URL.createObjectURL(await response.blob());
  const link = document.createElement("a");
  link.href = objectUrl;
  link.download = filename;
  link.hidden = true;
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
}
