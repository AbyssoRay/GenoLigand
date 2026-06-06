from __future__ import annotations

import json
import os
import platform
import re
import subprocess
import sys
import time
from pathlib import Path

from .io import write_json


def run_text(command: list[str]) -> tuple[bool, str]:
    try:
        completed = subprocess.run(command, text=True, capture_output=True, check=False)
    except Exception as exc:  # pragma: no cover - platform specific
        return False, repr(exc)
    return completed.returncode == 0, (completed.stdout + completed.stderr).strip()


def parse_nvidia_smi(raw: str) -> dict[str, str]:
    driver = ""
    cuda_umd = ""
    match = re.search(r"NVIDIA-SMI\s+([^\s]+).*?CUDA UMD Version:\s*([0-9.]+)", raw, re.S)
    if match:
        driver = match.group(1)
        cuda_umd = match.group(2)
    return {"nvidia_driver_version": driver, "cuda_umd_version_from_nvidia_smi": cuda_umd}


def collect_cuda_env(nvcc: str, selected_device: str = "cuda:0") -> dict[str, object]:
    nvidia_ok, nvidia_raw = run_text(["nvidia-smi"])
    nvcc_ok, nvcc_raw = run_text([nvcc, "--version"])
    payload: dict[str, object] = {
        "python": sys.version,
        "python_executable": sys.executable,
        "platform": platform.platform(),
        "cuda_visible_devices": os.environ.get("CUDA_VISIBLE_DEVICES"),
        "selected_device": selected_device,
        "nvidia_smi_success": nvidia_ok,
        "nvidia_smi_raw": nvidia_raw,
        "nvcc_success": nvcc_ok,
        "nvcc_version_raw": nvcc_raw,
    }
    payload.update(parse_nvidia_smi(nvidia_raw))
    try:
        import torch

        devices = []
        for index in range(torch.cuda.device_count() if torch.cuda.is_available() else 0):
            props = torch.cuda.get_device_properties(index)
            devices.append(
                {
                    "index": index,
                    "name": props.name,
                    "total_memory_bytes": props.total_memory,
                    "capability": f"{props.major}.{props.minor}",
                }
            )
        payload.update(
            {
                "torch_import_success": True,
                "torch_version": torch.__version__,
                "torch_version_contains_cpu": "+cpu" in torch.__version__,
                "torch_cuda_version": torch.version.cuda,
                "torch_cuda_available": torch.cuda.is_available(),
                "torch_device_count": torch.cuda.device_count(),
                "torch_devices": devices,
            }
        )
    except Exception as exc:
        payload.update(
            {
                "torch_import_success": False,
                "torch_import_error": repr(exc),
                "torch_version": "",
                "torch_version_contains_cpu": True,
                "torch_cuda_version": None,
                "torch_cuda_available": False,
                "torch_device_count": 0,
                "torch_devices": [],
            }
        )
    payload["phase_0_5_gpu_eligible"] = bool(
        payload["nvidia_smi_success"]
        and payload["nvcc_success"]
        and payload["torch_import_success"]
        and not payload["torch_version_contains_cpu"]
        and payload["torch_cuda_version"]
        and payload["torch_cuda_available"]
        and int(payload["torch_device_count"]) >= 1
    )
    return payload


def tensor_smoke(device: str = "cuda:0", size: int = 4096) -> dict[str, object]:
    import torch

    if not torch.cuda.is_available():
        raise SystemExit("cuda_unavailable")
    torch.cuda.set_device(device)
    torch.cuda.empty_cache()
    allocated_before = torch.cuda.memory_allocated()
    started = time.perf_counter()
    a = torch.randn((size, size), device=device, dtype=torch.float16)
    b = torch.randn((size, size), device=device, dtype=torch.float16)
    c = a @ b
    torch.cuda.synchronize()
    elapsed = time.perf_counter() - started
    finite = bool(torch.isfinite(c).all().item())
    return {
        "device": device,
        "operation": f"float16_matmul_{size}",
        "success": finite,
        "elapsed_seconds": elapsed,
        "allocated_before_bytes": allocated_before,
        "allocated_after_bytes": torch.cuda.memory_allocated(),
        "max_memory_allocated_bytes": torch.cuda.max_memory_allocated(),
        "result_finite": finite,
    }


def write_or_fail(payload: dict[str, object], out: Path, strict: bool = False) -> int:
    write_json(out, payload)
    if strict and not payload.get("phase_0_5_gpu_eligible", False):
        return 1
    return 0


def nvidia_memory_mib() -> int:
    ok, raw = run_text(
        [
            "nvidia-smi",
            "--query-gpu=memory.used",
            "--format=csv,noheader,nounits",
        ]
    )
    if not ok:
        return 0
    first = raw.splitlines()[0].strip()
    try:
        return int(first)
    except ValueError:
        return 0


def memory_probe_payload(process_name: str, before: int, peak: int, after: int, torch_peak: int) -> dict[str, object]:
    delta = max(0, peak - before)
    return {
        "process_name": process_name,
        "device_index": 0,
        "memory_before_mib": before,
        "memory_peak_mib": peak,
        "memory_after_mib": after,
        "peak_delta_mib": delta,
        "gpu_utilization_peak_percent": 0,
        "sample_count": 3,
        "torch_max_memory_allocated_bytes": torch_peak,
        "success": delta >= 100 or torch_peak > 0,
    }
