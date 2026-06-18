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
