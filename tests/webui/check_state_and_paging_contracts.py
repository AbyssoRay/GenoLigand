from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WEB = ROOT / "webui" / "app"


def read(relative: str) -> str:
    return (WEB / relative).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    state_source = read("state/app-state.js")
    actions_source = read("core/actions.js")
    project_source = read("pages/project.js")
    results_source = read("pages/results.js")
    check_source = read("pages/check.js")
    app_source = read("app.js")

    require(
        "/api/user/inputs/capabilities" in actions_source + app_source,
        "input descriptions must be loaded from the server capability endpoint",
    )
    require(
        "/inputs`" in actions_source and "restoreInputForm" in actions_source,
        "opening a project must reload and restore its saved inputs",
    )
    require(
        "projectOpenGeneration" in state_source + actions_source,
        "project switching must reject stale project/input responses",
    )
    require(
        '"draft"' in project_source and '"input_ready"' in project_source,
        "project cards must distinguish draft and input_ready",
    )
    require(
        "setupProgressForDisplay" in check_source,
        "setup progress rendering must reconcile with current setup status",
    )
    require(
        "AbortController" in actions_source + results_source,
        "result requests must own an AbortController",
    )
    require(
        "resultRequestPromise" in state_source + actions_source + results_source,
        "result reads must share one in-flight promise",
    )
    require(
        "setInterval" not in actions_source,
        "polling must schedule the next cycle only after the current cycle settles",
    )

    probe_source = f"""
const deferredRequests = [];
const timers = new Map();
let nextTimerId = 1;
const inertNode = {{
  addEventListener: () => {{}},
  classList: {{ toggle: () => {{}} }},
  querySelector: () => null,
  querySelectorAll: () => [],
}};
global.document = {{
  querySelector: () => inertNode,
  querySelectorAll: () => [],
}};
global.window = {{
  setTimeout: (callback) => {{
    const id = nextTimerId++;
    timers.set(id, callback);
    return id;
  }},
  clearTimeout: (id) => timers.delete(id),
  confirm: () => true,
}};
global.AbortController = AbortController;

{state_source}
{read("core/selectors.js")}
{actions_source}
{project_source}
{results_source}
{check_source}

function render() {{}}
function setStep(step) {{ state.step = step; }}
function api(path, options = {{}}) {{
  let resolve;
  let reject;
  const promise = new Promise((onResolve, onReject) => {{
    resolve = onResolve;
    reject = onReject;
  }});
  const request = {{ path, options, resolve, reject, promise }};
  deferredRequests.push(request);
  if (options.signal) {{
    options.signal.addEventListener("abort", () => {{
      const error = new Error("aborted");
      error.name = "AbortError";
      reject(error);
    }}, {{ once: true }});
  }}
  return promise;
}}
function request(path, occurrence = 0) {{
  const matches = deferredRequests.filter((item) => item.path === path);
  if (!matches[occurrence]) throw new Error(`missing request ${{path}} #${{occurrence}}`);
  return matches[occurrence];
}}
function assert(condition, message) {{
  if (!condition) throw new Error(message);
}}
async function tick() {{
  await Promise.resolve();
  await Promise.resolve();
}}

(async () => {{
  const capabilities = loadInputCapabilities();
  request("/api/user/inputs/capabilities").resolve({{
    input_types: [{{
      kind: "small_molecule",
      field: "small_molecule",
      title: "服务端小分子",
      label: "服务端支持名称、CAS、SMILES 和文件",
      placeholder: "服务端占位",
      minimum: "服务端最低要求",
    }}],
  }});
  await capabilities;
  assert(inputSpecs.length === 1, "capabilities must replace local descriptions");
  assert(inputSpecs[0].title === "服务端小分子", "server title must drive the input card");

  const openA = openExistingProject("project-a");
  await tick();
  const openB = openExistingProject("project-b");
  await tick();
  request("/api/user/projects/project-b").resolve({{
    project_id: "project-b",
    project_name: "项目 B",
    project_status: "input_ready",
    latest_run_id: null,
  }});
  request("/api/user/projects/project-b/inputs").resolve({{
    inputs: [
      {{ field: "project_name", value: "项目 B" }},
      {{ field: "small_molecule", value: "CCC" }},
    ],
    files: [],
  }});
  await openB;
  assert(state.project.project_id === "project-b", "project B must become active");
  assert(state.form.project_name === "项目 B", "project B name must be restored");
  assert(state.form.small_molecule === "CCC", "project B input must be restored");

  request("/api/user/projects/project-a").resolve({{
    project_id: "project-a",
    project_name: "项目 A",
    project_status: "input_ready",
    latest_run_id: null,
  }});
  request("/api/user/projects/project-a/inputs").resolve({{
    inputs: [
      {{ field: "project_name", value: "项目 A" }},
      {{ field: "small_molecule", value: "CCO" }},
    ],
    files: [],
  }});
  await openA;
  assert(state.project.project_id === "project-b", "late project A must not replace project B");
  assert(state.form.small_molecule === "CCC", "late project A input must not replace project B");

  assert(projectStateMeta({{ project_status: "draft" }}).label === "草稿", "draft label must be explicit");
  assert(
    projectStateMeta({{ project_status: "input_ready" }}).label === "输入已就绪",
    "input_ready label must be explicit",
  );

  state.setup = {{ database_ready: false }};
  state.setupProgress = {{ status: "ready", percent: 100, message: "旧结论" }};
  assert(setupProgressForDisplay() === null, "stale ready progress must not contradict setup status");
  state.setup = {{ database_ready: true }};
  assert(setupProgressForDisplay() === state.setupProgress, "matching ready progress must remain visible");

  state.run = {{ run_id: "run-1", state: "completed" }};
  state.runData = {{}};
  const firstRead = loadRunData();
  const duplicateRead = loadRunData();
  await tick();
  const candidateReads = deferredRequests.filter(
    (item) => item.path === "/api/user/runs/run-1/candidates",
  );
  assert(candidateReads.length === 1, "duplicate refreshes must share one result request");
  cancelResultRequest();
  await Promise.all([firstRead, duplicateRead]);
  assert(candidateReads[0].options.signal.aborted, "cancel must abort the active result request");

  const oldPage = loadResultPage("ranking", 1, "old");
  await tick();
  const oldRequest = deferredRequests.find((item) => item.path.includes("filter=old"));
  const newPage = loadResultPage("ranking", 1, "new");
  await tick();
  const newRequest = deferredRequests.find((item) => item.path.includes("filter=new"));
  assert(oldRequest.options.signal.aborted, "new paging intent must abort the old page request");
  newRequest.resolve({{
    candidates: [{{ candidate_id: "new-result" }}],
    pagination: {{ page: 1, page_size: 500, returned_rows: 1, total_rows: 1 }},
  }});
  await Promise.all([oldPage, newPage]);
  assert(
    state.runData.candidates.candidates[0].candidate_id === "new-result",
    "only the latest page response may update state",
  );

  const originalLoadRunData = loadRunData;
  loadRunData = async () => null;
  const refreshOne = refreshRunProgress();
  const refreshTwo = refreshRunProgress();
  await tick();
  const runRefreshes = deferredRequests.filter((item) => item.path === "/api/user/runs/run-1");
  assert(runRefreshes.length === 1, "run polling must share one in-flight refresh");
  runRefreshes[0].resolve({{ run_id: "run-1", state: "completed" }});
  await Promise.all([refreshOne, refreshTwo]);
  loadRunData = originalLoadRunData;

  const setupOne = refreshSetupProgress();
  const setupTwo = refreshSetupProgress();
  await tick();
  const setupStatuses = deferredRequests.filter((item) => item.path === "/api/user/setup/status");
  assert(setupStatuses.length === 1, "setup polling must share one status refresh");
  setupStatuses[0].resolve({{ database_ready: false }});
  await tick();
  const setupProgresses = deferredRequests.filter((item) => item.path === "/api/user/setup/progress");
  assert(setupProgresses.length === 1, "setup polling must share one progress refresh");
  setupProgresses[0].resolve({{ status: "needs_repair", percent: 20 }});
  await Promise.all([setupOne, setupTwo]);

  console.log(JSON.stringify({{
    passed: true,
    activeProject: state.project.project_id,
    candidateReads: candidateReads.length,
    runRefreshes: runRefreshes.length,
    setupRefreshes: setupProgresses.length,
  }}));
}})().catch((error) => {{
  console.error(error.stack || error.message);
  process.exitCode = 1;
}});
"""

    probe = subprocess.run(
        ["node", "-"],
        input=probe_source,
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    result = json.loads(probe.stdout)
    require(result["passed"], "state and paging browser-side probe must pass")
    print("state_and_paging_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
