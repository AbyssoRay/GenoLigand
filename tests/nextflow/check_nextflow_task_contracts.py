from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / "workflows" / "main.nf"
CONFIG = ROOT / "nextflow.config"
PINNED_VERSION = ROOT / ".nextflow-version"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def process_block(source: str) -> str:
    match = re.search(
        r"process\s+GENOLIGAND_SCIENCE_PIPELINE\s*\{(?P<body>.*?)\n\}",
        source,
        re.DOTALL,
    )
    require(match is not None, "GENOLIGAND_SCIENCE_PIPELINE process must exist")
    return match.group("body")


def main() -> int:
    source = WORKFLOW.read_text(encoding="utf-8")
    config = CONFIG.read_text(encoding="utf-8")
    process = process_block(source)

    for name in ("input_manifest", "gls_cli", "workflow_config"):
        require(
            re.search(rf"(?m)^\s*path\s+{name}\s*$", process) is not None,
            f"{name} must be a task-local path input",
        )
        require(
            re.search(rf"(?m)^\s*val\s+{name}\s*$", process) is None,
            f"{name} must not be modeled as val",
        )
    require(
        re.search(r"(?m)^\s*val\s+science_store\s*$", process) is not None,
        "the large science store must be passed as an external value reference",
    )
    require(
        re.search(r"(?m)^\s*path\s+science_store\s*$", process) is None,
        "the large science store must not be staged or content-fingerprinted as a path input",
    )
    require(
        "cpus params.pipeline_cpus" in process
        and "memory params.pipeline_memory" in process,
        "pipeline resources must come from profile parameters",
    )
    require(
        "local_acceptance" in config
        and "params.pipeline_cpus = 4" in config
        and "params.pipeline_memory = '8 GB'" in config,
        "the local acceptance profile must request 4 CPU and 8 GB",
    )
    require(
        PINNED_VERSION.read_text(encoding="utf-8").strip() == "26.04.3",
        "Nextflow must have one pinned runtime version",
    )

    require(
        re.search(r'(?m)^\s*path\s+["\']run_output["\']', process) is not None,
        "the scientific run directory must be a declared path output",
    )
    require("publishDir params.outdir" in process, "run outputs must be published")
    require(
        re.search(r'--run-dir\s+["\']run_output["\']', process) is not None,
        "CLI output must be written inside the task work directory",
    )

    cli_call = process.find('"./${gls_cli}" pipeline run')
    state_read = process.find("run_output/run_state.txt")
    completed_check = process.find('!= "completed"')
    done_write = process.find("> pipeline_done.txt")
    require(cli_call >= 0, "workflow must invoke the staged CLI")
    require(
        "if " in process[max(0, cli_call - 80) : cli_call],
        "workflow must branch on the CLI exit status",
    )
    require(state_read > cli_call, "run state must be checked after CLI success")
    require(completed_check > state_read, "run state must equal completed")
    require(
        done_write > completed_check,
        "pipeline_done.txt must be written after the completed-state gate",
    )
    provenance_write = process.find("nextflow_provenance.tsv")
    require(
        provenance_write > completed_check and provenance_write < done_write,
        "Nextflow version and config digest provenance must be written before the done marker",
    )
    require(
        "workflow.nextflow.version" in source and "sha256sum" in process,
        "runtime version and the executed config digest must drive provenance",
    )

    print("nextflow_task_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
