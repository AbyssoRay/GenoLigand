from __future__ import annotations

import ctypes
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORK_ROOT = (ROOT / "work" / "large-input-memory").resolve()
TARGET_BYTES = int(os.environ.get("GENOLIGAND_LARGE_INPUT_BYTES", str(1 << 30)))
MAX_PEAK_BYTES = int(os.environ.get("GENOLIGAND_LARGE_INPUT_MAX_RSS", str(256 << 20)))


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def cli_path() -> Path:
    explicit = os.environ.get("GENOLIGAND_GLS_CLI", "").strip()
    candidates = [
        Path(explicit) if explicit else None,
        ROOT / "target" / "release" / ("gls-cli.exe" if os.name == "nt" else "gls-cli"),
        ROOT / "target" / "debug" / ("gls-cli.exe" if os.name == "nt" else "gls-cli"),
    ]
    for candidate in candidates:
        if candidate is not None and candidate.is_file():
            return candidate.resolve()
    raise AssertionError("built gls-cli is required")


def create_sequence_file(path: Path, file_format: str) -> tuple[int, int]:
    amino_acids = b"ACDEFGHIKLMNPQRSTVWY" * 205
    nucleotides = b"ACGT" * 1024
    quality = b"I" * len(nucleotides)
    records = 0
    with path.open("wb", buffering=1024 * 1024) as handle:
        while handle.tell() < TARGET_BYTES:
            if file_format == "fasta":
                handle.write(f">record_{records}\n".encode("ascii"))
                handle.write(amino_acids)
                handle.write(b"\n")
            elif file_format == "fastq":
                handle.write(f"@record_{records}\n".encode("ascii"))
                handle.write(nucleotides)
                handle.write(b"\n+\n")
                handle.write(quality)
                handle.write(b"\n")
            elif file_format == "gbk":
                handle.write(f"LOCUS record_{records}\nORIGIN\n1 ".encode("ascii"))
                handle.write(nucleotides)
                handle.write(b"\n//\n")
            else:
                raise AssertionError(f"unsupported format: {file_format}")
            records += 1
    return records, path.stat().st_size


def windows_peak_working_set(process: subprocess.Popen[str]) -> int:
    from ctypes import wintypes

    class ProcessMemoryCounters(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL
    psapi.GetProcessMemoryInfo.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(ProcessMemoryCounters),
        wintypes.DWORD,
    ]
    psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
    handle = kernel32.OpenProcess(0x0410, False, process.pid)
    require(bool(handle), "OpenProcess failed")
    peak = 0
    try:
        while process.poll() is None:
            counters = ProcessMemoryCounters()
            counters.cb = ctypes.sizeof(counters)
            require(
                bool(psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb)),
                "GetProcessMemoryInfo failed",
            )
            peak = max(peak, int(counters.PeakWorkingSetSize))
            time.sleep(0.02)
    finally:
        kernel32.CloseHandle(handle)
    return peak


def linux_peak_working_set(process: subprocess.Popen[str]) -> int:
    peak = 0
    status = Path(f"/proc/{process.pid}/status")
    while process.poll() is None:
        try:
            for line in status.read_text(encoding="utf-8").splitlines():
                if line.startswith("VmHWM:"):
                    peak = max(peak, int(line.split()[1]) * 1024)
                    break
        except FileNotFoundError:
            break
        time.sleep(0.02)
    return peak


def run_case(file_format: str, field: str) -> dict[str, int | str]:
    case_root = WORK_ROOT / file_format
    case_root.mkdir(parents=True)
    try:
        source = case_root / f"large.{file_format}"
        expected_records, input_bytes = create_sequence_file(source, file_format)
        require(input_bytes >= 1 << 30, "acceptance input is smaller than one GiB")
        manifest = case_root / "input.json"
        manifest.write_text(
            json.dumps(
                {
                    "inputs": [
                        {"field": field, "value": str(source)}
                    ],
                    "files": [],
                }
            )
            + "\n",
            encoding="utf-8",
        )
        run_dir = case_root / "run"
        process = subprocess.Popen(
            [
                str(cli_path()),
                "input",
                "normalize",
                "--workspace-root",
                str(ROOT),
                "--science-store",
                str(case_root / "unused-store"),
                "--input-manifest",
                str(manifest),
                "--run-dir",
                str(run_dir),
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        peak = (
            windows_peak_working_set(process)
            if os.name == "nt"
            else linux_peak_working_set(process)
        )
        stdout, stderr = process.communicate(timeout=30)
        require(process.returncode == 0, f"normalization failed: {stdout}\n{stderr}")
        with (run_dir / "input_normalized_manifest.tsv").open("rb") as handle:
            observed_records = (
                sum(
                    chunk.count(b"\n")
                    for chunk in iter(lambda: handle.read(1 << 20), b"")
                )
                - 1
            )
        require(observed_records == expected_records, "normalized record count changed")
        require(peak <= MAX_PEAK_BYTES, f"peak RSS {peak} exceeds {MAX_PEAK_BYTES}")
        return {
            "format": file_format,
            "input_bytes": input_bytes,
            "records": observed_records,
            "peak_rss_bytes": peak,
        }
    finally:
        if case_root.exists():
            shutil.rmtree(case_root)


def main() -> int:
    require(WORK_ROOT == (ROOT / "work" / "large-input-memory").resolve(), "unsafe work root")
    if WORK_ROOT.exists():
        shutil.rmtree(WORK_ROOT)
    WORK_ROOT.mkdir(parents=True)
    try:
        cases = [
            run_case("fasta", "protein_ligand"),
            run_case("fastq", "genome_or_metagenome"),
            run_case("gbk", "genome_or_metagenome"),
        ]
        print(
            json.dumps(
                {
                    "schema_version": "genoligand_large_input_memory_acceptance_v1",
                    "limit_bytes": MAX_PEAK_BYTES,
                    "cases": cases,
                },
                sort_keys=True,
            )
        )
        print("large_input_memory\tPASS")
        return 0
    finally:
        if WORK_ROOT.exists():
            shutil.rmtree(WORK_ROOT)


if __name__ == "__main__":
    raise SystemExit(main())
