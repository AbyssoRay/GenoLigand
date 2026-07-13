from __future__ import annotations

import sys
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WEB = ROOT / "webui" / "app"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    required = [
        "state/app-state.js",
        "api/client.js",
        "pages/project.js",
        "pages/input.js",
        "pages/check.js",
        "pages/run.js",
        "pages/results.js",
        "pages/report.js",
        "core/actions.js",
        "core/selectors.js",
        "components/common.js",
        "components/viewers.js",
        "styles/tokens.css",
        "styles/base.css",
        "styles/layout.css",
        "styles/components.css",
        "styles/pages.css",
    ]
    for rel in required:
        require((WEB / rel).is_file(), f"missing split web asset: {rel}")

    index = read(WEB / "index.html")
    require(
        'rel="icon" href="/favicon.svg" type="image/svg+xml"' in index,
        "index.html must declare a first-party favicon",
    )
    for rel in required[:12]:
        require(f'src="/{rel}"' in index, f"index.html does not load {rel}")
    for label in ["项目", "输入", "检查", "运行", "结果", "报告"]:
        require(f">{label}</button>" in index, f"workflow navigation must use Chinese label: {label}")
    require('data-step="configure"' not in index, "configure step must not be exposed in navigation")
    require("pages/configure.js" not in index, "configure page script must not be loaded")
    require('data-step="export"' not in index, "export step must be replaced by report")
    require("pages/export.js" not in index, "export page script must be replaced by report page")
    for step in ["input", "check", "run", "results", "report"]:
        require(
            f'data-step="{step}" class="step disabled" disabled' in index,
            f"initial step navigation must disable {step}",
        )

    user_text = "\n".join(
        read(path)
        for path in [WEB / "index.html", *[WEB / rel for rel in required[:12]]]
    )
    forbidden = ["GitHub", "commit", "remote", "schema", "manifest", "hash", "raw JSON", "gate", "route"]
    for term in forbidden:
        require(term not in user_text, f"user web assets expose developer term: {term}")

    common = read(WEB / "components/common.js")
    require(
        'known_nonhuman_protein: "已知非人源蛋白数据库"' in common,
        "known_nonhuman_protein must render as a user-readable scope label",
    )
    require('{ key: "md_support_label", label: "MD 支持" }' in common, "ranking must show MD support")
    require('{ key: "risk_note", label: "风险提示" }' in common, "ranking must show risk notes")

    project_page = read(WEB / "pages/project.js")
    input_page = read(WEB / "pages/input.js")
    run_page = read(WEB / "pages/run.js")
    check_page = read(WEB / "pages/check.js")
    actions = read(WEB / "core/actions.js")
    selectors = read(WEB / "core/selectors.js")
    app_state = read(WEB / "state/app-state.js")
    app = read(WEB / "app.js")
    require(
        "/api/user/inputs/inspect" in actions,
        "WebUI must call server inspection",
    )
    require(
        "looksLikeSmilesText" not in selectors,
        "client SMILES heuristic must be removed",
    )
    require(
        "looksLikeNucleotideSequence" not in selectors,
        "client sequence heuristic must be removed",
    )
    require(
        "inputInspection" in app_state,
        "authoritative inspection state is required",
    )
    for removed_heuristic in [
        "inputValueUsable",
        "looksLikeConvertibleFreeText",
        "looksLikePathWithExt",
        "looksLikePeptideSequence",
        "validElementSymbol",
    ]:
        require(
            removed_heuristic not in selectors,
            f"client scientific heuristic must be removed: {removed_heuristic}",
        )
    require(
        actions.count("inputPayloadFromForm()") >= 3,
        "inspection and save must share inputPayloadFromForm",
    )
    require(
        "state.inputInspectionPayload" in actions,
        "save must reuse the payload accepted by inspection",
    )
    require(
        "inputInspectionGeneration" in actions + app_state,
        "input inspection must reject stale asynchronous responses",
    )
    require(
        'if (step === "input") return Boolean(state.project)' in selectors,
        "input step must require an existing project",
    )
    require(
        "state.inputSavePending" in input_page and "formDisabled" in input_page,
        "input controls must be locked while save is pending",
    )
    require(
        "state.inputSavePending && step !== state.step" in app
        and "invalidateInputInspection();" in app,
        "leaving the input step must invalidate a pending save",
    )
    require("function renderProject" in project_page, "project view must be the dedicated project manager")
    require('data-action="new-project"' in project_page, "project view must expose new project action")
    require("data-open-project" in project_page, "project view must open historical projects")
    require("data-delete-project" in project_page, "project view must delete projects")
    require("继续已有项目" not in input_page, "input view must not duplicate project history")
    require("renderConfigure" not in actions + selectors + run_page, "configure view must be removed from active UI code")
    require('"configure"' not in selectors, "workflow selectors must not allow configure step")
    require('"report"' in selectors and '"export"' not in selectors, "workflow selectors must use report step")
    report_page = read(WEB / "pages/report.js")
    require("function renderReport" in report_page, "report view must replace the old export view")
    require("报告材料" in report_page, "report view must use report wording")
    require("当前运行还没有可下载报告材料" in report_page, "report view must keep empty downloads honest")
    require(
        "runStageRows().map(renderStage).join(\"\")" in run_page,
        "run view must render backend stage readiness instead of fixed completed labels",
    )
    require(
        "renderRunProgress()" in run_page and 'role="progressbar"' in run_page,
        "run view must render a visible overall progress bar",
    )
    require(
        "function runProgressPercent" in run_page and "stage.status" in run_page,
        "run view must compute progress from backend stage statuses",
    )
    require(
        "stage.status === \"not_generated\"" in run_page,
        "run view must show not-generated stages for missing current-run artifacts",
    )
    require(
        'state.run?.state !== "running"' in selectors,
        "start-run control must be disabled while a run is already running",
    )
    require(
        'data-action="start-run" ${canStartRun() ? "" : "disabled"}' in check_page,
        "check view start-run button must share the central run-start predicate",
    )
    require(
        "if (!canStartRun())" in actions,
        "startRun action must reject duplicate start attempts before posting to the server",
    )
    load_run_data = actions[actions.index("async function loadRunData()") :]
    require("Promise.all([" not in load_run_data, "run data loading must avoid concurrent browser fetch bursts")
    require("/candidate-detail" in load_run_data and "/md-summary" in load_run_data, "run data loading must still read all result endpoints")
    require(
        'class="stage-main"' in run_page and 'class="stage-reason"' in run_page,
        "run view must keep long stage reasons in the text column",
    )
    require(
        "const completed = runState === \"completed\"" not in run_page,
        "run view must not mark every stage complete solely from run state",
    )
    for removed in ["资源计划", "当前资源", "恢复建议", "可导出结果"]:
        require(removed not in run_page, f"run view must not render {removed}")
    results_page = read(WEB / "pages/results.js")
    require("cuda_monitoring" in results_page, "MD results must read CUDA production monitoring summary")
    require("mdRuntimeStatus" in results_page, "MD results must render protocol/state/CUDA runtime status")
    require("OpenMM 平台" in results_page, "MD results must show the OpenMM platform")
    require("CUDA 监测" in results_page, "MD results must show CUDA monitoring evidence")
    require('cuda["production_" + "sam" + "ple_count"]' in results_page, "MD results must show production GPU sample count")
    require("paginationSummary" in results_page, "result pages must render pagination totals")
    require("paginationControls" in results_page, "result pages must render pagination controls")
    require("filters_active" in results_page, "result pages must show active filter state")
    require('data-page-action="next"' in results_page, "result pages must expose next-page control")
    require('data-page-action="prev"' in results_page, "result pages must expose previous-page control")
    require("loadResultPage" in results_page, "result page controls must load paginated API data")
    require("URLSearchParams" in results_page, "result page controls must pass page/filter query parameters")
    require("data-result-filter" in results_page, "result pages must expose a filter input")
    components = read(WEB / "styles/components.css")
    require(".stage-main" in components, "stage text column must have a stable width")
    require(".stage-reason" in components, "stage reasons must wrap as normal text")

    inspection_probe_source = (
        """
const timerCallbacks = new Map();
let nextTimerId = 1;
const syntheticFormFields = [];
const syntheticInputHandlers = [];
const inertActionNode = { addEventListener: () => {} };
const pageNode = {
  querySelectorAll: (selector) => selector === "[data-form]" ? syntheticFormFields : [],
  querySelector: () => inertActionNode
};
global.document = {
  querySelector: () => pageNode,
  querySelectorAll: () => []
};
global.window = {
  setTimeout: (callback) => {
    const id = nextTimerId++;
    timerCallbacks.set(id, callback);
    return id;
  },
  clearTimeout: (id) => timerCallbacks.delete(id),
  setInterval,
  clearInterval,
  confirm: () => true
};
"""
            + read(WEB / "state/app-state.js")
            + read(WEB / "core/selectors.js")
            + read(WEB / "core/actions.js")
            + read(WEB / "pages/input.js")
            + """
const requests = [];
const apiCalls = [];
const saveRequests = [];
const visitedSteps = [];
function render() {}
function setStep(step) { visitedSteps.push(step); }
function api(path, options = {}) {
  apiCalls.push({ path, options });
  if (path === "/api/user/inputs/inspect") {
    return new Promise((resolve, reject) => {
      requests.push({ path, options, resolve, reject });
    });
  }
  if (path.startsWith("/api/user/projects/") && path.endsWith("/inputs")) {
    return new Promise((resolve, reject) => {
      saveRequests.push({ path, options, resolve, reject });
    });
  }
  return Promise.resolve({ projects: [] });
}
function assert(condition, message) {
  if (!condition) throw new Error(message);
}
function clearScientificForm() {
  for (const spec of inputSpecs) state.form[spec.field] = "";
  state.form.project_name = "";
  invalidateInputInspection();
}

(async () => {
  clearScientificForm();
  const emptySave = await saveInput();
  assert(!emptySave, "empty form must not save");
  assert(!apiCalls.some((call) => call.path === "/api/user/projects"), "empty form must not create a project");

  state.form.project_name = "Race fixture";
  state.form.small_molecule = "CCO";
  const firstPayload = inputPayloadFromForm();
  assert(firstPayload.files.length === 0, "payload must contain files: []");
  assert(firstPayload.inputs[1].value === "CCO", "payload must include filled scientific input");

  const oldSuccess = inspectInputs();
  state.form.small_molecule = "CCCCCCCC";
  const latestSuccess = inspectInputs();
  assert(state.inputInspection === null, "new input must immediately invalidate old inspection");
  assert(state.inputInspectionPending, "latest request must remain pending");
  requests[1].resolve({ usable: true, summary: "latest", errors: [], inputs: [] });
  await latestSuccess;
  const acceptedPayload = state.inputInspectionPayload;
  assert(acceptedPayload.inputs[1].value === "CCCCCCCC", "inspection must retain its exact payload");
  assert(inputsReadyToSave(), "latest usable inspection must enable save");
  requests[0].resolve({ usable: true, summary: "stale", errors: [], inputs: [] });
  await oldSuccess;
  assert(state.inputInspection.summary === "latest", "old success must not replace latest result");
  assert(state.inputInspectionPayload === acceptedPayload, "old success must not replace accepted payload");

  state.form.small_molecule = "first-failure";
  const oldFailure = inspectInputs();
  state.form.small_molecule = "latest-value";
  const latestAfterFailure = inspectInputs();
  requests[2].reject(new Error("stale failure"));
  await oldFailure;
  assert(state.inputInspectionPending, "old failure must not clear latest pending state");
  assert(state.inputInspectionError === "", "old failure must not replace latest error state");
  requests[3].resolve({ usable: true, summary: "latest after failure", errors: [], inputs: [] });
  await latestAfterFailure;
  assert(state.inputInspection.summary === "latest after failure", "latest result must survive old failure");

  state.project = null;
  const createCallsBeforeInspectedSave = apiCalls.filter((call) => call.path === "/api/user/projects").length;
  const noProjectSave = await saveInput();
  assert(!noProjectSave, "input save must require an existing project");
  assert(
    apiCalls.filter((call) => call.path === "/api/user/projects").length === createCallsBeforeInspectedSave,
    "input save must not create a project"
  );

  state.project = { project_id: "project-race" };
  const acceptedRacePayload = inputPayloadFromForm();
  state.inputInspection = { usable: true, summary: "save race", errors: [], inputs: [] };
  state.inputInspectionPayload = acceptedRacePayload;
  state.inputInspectionPayloadKey = inputPayloadKey(acceptedRacePayload);
  state.inputInspectionPending = false;
  state.inputInspectionError = "";
  const saveRace = saveInput();
  await Promise.resolve();
  assert(saveRequests.length === 1, "valid inspected input must start one save request");
  state.form.project_name = "edited after post";
  scheduleInputInspection();
  saveRequests[0].resolve({ ok: true, project: { project_id: "project-race" } });
  const staleSaveResult = await saveRace;
  assert(!staleSaveResult, "save response for changed form must be ignored");
  assert(!visitedSteps.includes("check"), "stale save response must not enter check step");

  state.form.small_molecule = "changed-after-inspection";
  assert(!inputsReadyToSave(), "payload mismatch must disable save before debounce runs");
  const projectCallsBeforeStaleSave = apiCalls.filter((call) => call.path === "/api/user/projects").length;
  const staleSave = await saveInput();
  assert(!staleSave, "stale inspection must not save");
  assert(
    apiCalls.filter((call) => call.path === "/api/user/projects").length === projectCallsBeforeStaleSave,
    "stale inspection must not create a project"
  );
  scheduleInputInspection();
  assert(state.inputInspection === null, "form change must invalidate inspection immediately");
  assert(state.inputInspectionPending, "non-empty form must be pending during debounce");

  clearScientificForm();
  const requestCount = requests.length;
  scheduleInputInspection();
  assert(!state.inputInspectionPending, "empty form must not remain pending");
  assert(!inputsReadyToSave(), "empty form must not become saveable");
  assert(requests.length === requestCount, "empty form must not inspect");

  state.form.small_molecule = "CCO";
  scheduleInputInspection();
  assert(timerCallbacks.size === 1, "non-empty input must schedule one debounce callback");
  const debounceCallback = Array.from(timerCallbacks.values())[0];
  timerCallbacks.clear();
  debounceCallback();
  await Promise.resolve();
  assert(requests.length === requestCount + 1, "debounce callback must start server inspection");
  requests[requestCount].resolve({ usable: true, summary: "debounced", errors: [], inputs: [] });
  await new Promise((resolve) => setImmediate(resolve));
  assert(state.inputInspection?.summary === "debounced", "debounced inspection result must become current");

  state.form.small_molecule = "unusable";
  const unusable = inspectInputs();
  requests[requestCount + 1].resolve({ usable: false, message: "rejected", errors: ["bad input"], inputs: [] });
  await unusable;
  assert(!inputsReadyToSave(), "unusable inspection must disable save");

  clearScientificForm();
  state.form.small_molecule = "CCO";
  state.form.peptide = "Z:/missing/peptide.fasta";
  const mixedPayload = inputPayloadFromForm();
  state.inputInspection = {
    usable: false,
    message: "one input failed",
    errors: ["input_2:record_1:input_file_not_found"],
    inputs: [
      { original_field: "small_molecule", usable: true, message: null },
      { original_field: "peptide", usable: false, message: "找不到输入文件，请检查文件路径。" }
    ]
  };
  state.inputInspectionPayload = mixedPayload;
  state.inputInspectionPayloadKey = inputPayloadKey(mixedPayload);
  assert(inputCardStatus("small_molecule", true).ready, "valid mixed-input card must remain usable");
  assert(!inputCardStatus("peptide", true).ready, "failed mixed-input card must remain blocked");
  assert(inputInspectionMessage("small_molecule") === "服务端检查通过。", "valid field must show its own result");
  assert(
    inputInspectionMessage("peptide") === "找不到输入文件，请检查文件路径。",
    "failed field must show its own server message"
  );

  clearScientificForm();
  const syntheticField = {
    dataset: { form: "small_molecule" },
    value: "CCN",
    addEventListener: (event, handler) => {
      if (event === "input") syntheticInputHandlers.push(handler);
    }
  };
  syntheticFormFields.push(syntheticField);
  bindInputEvents();
  assert(syntheticInputHandlers.length === 1, "input page must bind one synthetic input handler");
  syntheticInputHandlers[0]();
  assert(state.form.small_molecule === "CCN", "input event must collect the current field value");
  assert(state.inputInspectionPending, "input event must immediately enter pending state");
  assert(timerCallbacks.size === 1, "input event must schedule one debounce timer");
  console.log(JSON.stringify({ passed: true, requestCount: requests.length }));
})().catch((error) => {
  console.error(error.stack || error.message);
  process.exitCode = 1;
});
"""
    )
    inspection_probe = subprocess.run(
        ["node", "-"],
        input=inspection_probe_source,
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    inspection_checks = json.loads(inspection_probe.stdout)
    require(inspection_checks["passed"], "authoritative inspection probe must pass")

    mojibake_terms = [
        chr(0x9428),
        chr(0x7ECC),
        chr(0x7039),
        chr(0x6748),
        chr(0x93C1),
        chr(0x93C8),
        chr(0x95C7),
        chr(0x6DC7),
        chr(0x59AB) + chr(0x20AC),
        chr(0x5A34),
    ]
    for term in mojibake_terms:
        require(term not in user_text, f"user web assets contain mojibake text: {term}")

    require("示例项目" not in user_text, "first-run UI must not seed example projects")
    require("components/viewers.js" in index, "index.html must load the structure and MD visual components")
    print("static_webui_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
