import json
import os
import platform
import subprocess
import sys


def choose_device(requested: str) -> str:
    if requested == "cpu":
        return "cpu"
    if requested == "cuda":
        import torch

        if not torch.cuda.is_available():
            raise RuntimeError("CUDA requested but torch.cuda.is_available() is false")
        return "cuda"
    if requested == "auto":
        import torch

        return "cuda" if torch.cuda.is_available() else "cpu"
    raise ValueError(f"Unknown device request: {requested}")


def _run(cmd: list[str]) -> str:
    try:
        return subprocess.check_output(cmd, stderr=subprocess.STDOUT, text=True, timeout=20)
    except Exception as exc:  # pragma: no cover - diagnostic path
        return f"ERROR: {type(exc).__name__}: {exc}"


def cuda_diagnostics() -> dict:
    info = {
        "python": sys.version,
        "platform": platform.platform(),
        "cuda_visible_devices": os.environ.get("CUDA_VISIBLE_DEVICES"),
        "nvidia_smi": _run(["nvidia-smi"]),
        "nvcc_version": _run(["nvcc", "--version"]),
    }
    try:
        import torch

        info["torch_version"] = torch.__version__
        info["torch_cuda_version"] = torch.version.cuda
        info["torch_cuda_available"] = torch.cuda.is_available()
        info["torch_device_count"] = torch.cuda.device_count()
        if torch.cuda.is_available():
            info["torch_device_name_0"] = torch.cuda.get_device_name(0)
            info["torch_capability_0"] = torch.cuda.get_device_capability(0)
    except Exception as exc:
        info["torch_import_error"] = repr(exc)
    return info


def print_cuda_diagnostics() -> None:
    print(json.dumps(cuda_diagnostics(), indent=2, ensure_ascii=False))
