from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "python"))

from genoligand_workers.md_summary_worker import create_openmm_context  # noqa: E402


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def create_context(policy: str) -> dict[str, object]:
    import openmm

    system = openmm.System()
    system.addParticle(12.0)
    integrator = openmm.VerletIntegrator(0.001)
    context, name, properties, failures = create_openmm_context(
        openmm, system, integrator, policy
    )
    try:
        context.setPositions([[0.0, 0.0, 0.0]])
        context.getState(getEnergy=True)
        observed = {}
        platform = context.getPlatform()
        for key in platform.getPropertyNames():
            try:
                observed[key] = platform.getPropertyValue(context, key)
            except Exception:
                continue
        return {
            "policy": policy,
            "platform": name,
            "requested_properties": properties,
            "observed_properties": observed,
            "context_failures": failures,
        }
    finally:
        del context
        del integrator


def gpu_identity() -> dict[str, str]:
    completed = subprocess.run(
        [
            "nvidia-smi",
            "--query-gpu=name,driver_version,memory.total",
            "--format=csv,noheader,nounits",
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=True,
        timeout=30,
    )
    name, driver, memory = [item.strip() for item in completed.stdout.splitlines()[0].split(",")]
    return {"name": name, "driver": driver, "memory_mib": memory}


def main() -> int:
    disabled = create_context("disabled")
    preferred = create_context("preferred")
    required = create_context("required")
    require(disabled["platform"] in {"CPU", "Reference"}, "disabled policy used CUDA")
    require(preferred["platform"] == "CUDA", "preferred policy did not use available CUDA")
    require(required["platform"] == "CUDA", "required policy did not create a CUDA Context")
    payload = {
        "schema_version": "genoligand_openmm_platform_acceptance_v1",
        "gpu": gpu_identity(),
        "contexts": [disabled, preferred, required],
    }
    print(json.dumps(payload, ensure_ascii=False, sort_keys=True))
    print("openmm_platform_policies\tPASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
