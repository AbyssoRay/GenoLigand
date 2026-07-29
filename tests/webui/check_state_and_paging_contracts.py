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
    run_source = read("pages/run.js")
    common_source = read("components/common.js")
    app_source = read("app.js")

    require(
        "/api/user/inputs/capabilities" in actions_source + app_source,
        "input descriptions must be loaded from the server capability endpoint",
    )
    require(
        "Promise.allSettled" in app_source and "healthFailure" in app_source,
        "startup refreshes must preserve successful health state when an auxiliary request fails",
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
  innerHTML: "",
  className: "",
  addEventListener: () => {{}},
  classList: {{ toggle: () => {{}} }},
  querySelector: () => inertNode,
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
{common_source}
{run_source}

function render() {{}}
function setStep(step) {{ state.step = step; }}
async function refreshAll() {{}}
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
      text: {{ supported: true, forms: ["smiles"] }},
      file: {{ supported: true, extensions: ["sdf"] }},
      constraints: {{ minimum_records: 1 }},
    }}],
  }});
  await capabilities;
  assert(inputSpecs.length === 1, "capabilities must replace local descriptions");
  assert(inputSpecs[0].title === "服务端小分子", "server title must drive the input card");

  const openA = openExistingProject("project-a");
  await tick();
  state.resultTab = "md";
  state.resultQueries = {{ ranking: {{ page: 3, filter: "project-a-only" }} }};
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
  assert(state.resultTab === "ranking", "opening another project must reset the result tab");
  assert(
    Object.keys(state.resultQueries).length === 0,
    "opening another project must clear result paging and filters",
  );

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
  await tick();
  request("/api/user/projects/project-b", 1).resolve({{
    project_id: "project-b",
    project_name: "项目 B",
    project_status: "completed",
    latest_run_id: "run-1",
  }});
  request("/api/user/projects").resolve({{
    projects: [{{ project_id: "project-b", project_status: "completed", latest_run_id: "run-1" }}],
  }});
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

  const readyChecks = [
    {{ id: "input", ready: true }},
    {{ id: "science_store", ready: true }},
    {{ id: "known_database", ready: true }},
    {{ id: "metagenome", ready: true }},
    {{ id: "structure_index", ready: true }},
    {{ id: "tools", ready: true }},
    {{ id: "python_worker", ready: true }},
  ];
  state.project = {{ project_id: "project-state-priority" }};
  state.preflight = {{ checks: readyChecks }};
  state.run = {{ run_id: "run-state-priority", state: "running" }};
  assert(
    nextActionText() === "等待当前阶段完成",
    "running state must take priority over run-start eligibility",
  );
  state.preflight = null;
  state.run = {{ run_id: "run-completed", state: "completed" }};
  assert(
    nextActionText() === "查看结果或报告",
    "completed state must take priority over a missing in-memory preflight",
  );

  state.runData = {{
    candidates: {{
      candidates: Array.from({{ length: 500 }}, (_, index) => ({{ candidate_id: `candidate-${{index + 1}}` }})),
      pagination: {{ page: 1, page_size: 500, returned_rows: 500, total_rows: 1200 }},
    }},
  }};
  assert(candidateCountText() === "1200", "run summary must use the candidate pagination total");

  const pageTwoSummary = candidateDetailSection(
    [{{ candidate_id: "candidate-501", target_name: "page two target" }}],
    {{ page: 2, page_size: 500, returned_rows: 1, total_rows: 501 }},
  );
  assert(
    pageTwoSummary.includes("本页首个候选摘要") && !pageTwoSummary.includes("第一候选摘要"),
    "paged candidate detail must label the current page's first row accurately",
  );

  state.project = {{ project_id: "project-cancel" }};
  state.preflight = {{ checks: readyChecks }};
  state.run = {{ run_id: "run-cancel", state: "running", stages: [] }};
  inertNode.innerHTML = "";
  renderRun();
  const cancelTag = inertNode.innerHTML.match(/<button[^>]*data-action="cancel-run"[^>]*>/)?.[0] || "";
  assert(cancelTag && !cancelTag.includes("disabled"), "running view must show an enabled cancel action");
  const originalCancelLoadRunData = loadRunData;
  const originalRefreshCurrentProject = refreshCurrentProject;
  loadRunData = async () => null;
  refreshCurrentProject = async () => null;
  const cancelledRun = cancelRun();
  await tick();
  const cancelRequest = request("/api/user/runs/run-cancel/cancel");
  assert(cancelRequest.options.method === "POST", "run cancel action must use POST");
  cancelRequest.resolve({{ status: "cancelled", process_stopped: true }});
  await tick();
  request("/api/user/runs/run-cancel").resolve({{ run_id: "run-cancel", state: "cancelled" }});
  await cancelledRun;
  loadRunData = originalCancelLoadRunData;
  refreshCurrentProject = originalRefreshCurrentProject;

  state.project = {{ project_id: "project-run-switch" }};
  state.preflight = {{ checks: readyChecks }};
  state.run = {{ run_id: "run-old", state: "completed" }};
  state.resultTab = "md";
  state.resultQueries = {{ md: {{ page: 2, filter: "old-run-only" }} }};
  const switchedRun = startRun();
  await tick();
  request("/api/user/projects/project-run-switch/runs").resolve({{
    run: {{ state: "failed", state_label: "失败" }},
    project: {{ project_id: "project-run-switch", project_status: "failed" }},
  }});
  await switchedRun;
  assert(state.resultTab === "ranking", "starting another run must reset the result tab");
  assert(
    Object.keys(state.resultQueries).length === 0,
    "starting another run must clear result paging and filters",
  );

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
        check=False,
    )
    require(probe.returncode == 0, f"state and paging probe failed: {probe.stderr}")
    require(probe.stdout.strip(), f"state and paging probe produced no result: {probe.stderr}")
    result = json.loads(probe.stdout)
    require(result["passed"], "state and paging browser-side probe must pass")
    print("state_and_paging_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
