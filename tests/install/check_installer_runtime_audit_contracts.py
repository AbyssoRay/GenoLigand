from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
INSTALLER_PATH = ROOT / "scripts/setup/install_complete_scientific.py"


def load_installer_module():
    spec = importlib.util.spec_from_file_location("installer_runtime_audit", INSTALLER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class InstallerRuntimeAuditContracts(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.installer = load_installer_module()

    def test_install_lock_is_atomic_heartbeating_and_owner_aware(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            store = Path(tmp) / "science_store"
            with mock.patch.object(installer, "INSTALL_LOCK_HEARTBEAT_SECONDS", 0.02), mock.patch.object(
                installer, "INSTALL_LOCK_WAIT_SECONDS", 0.01
            ):
                first = installer.acquire_install_lock(store)
                first_path = getattr(first, "path", first)
                try:
                    initial = json.loads(first_path.read_text(encoding="utf-8"))
                    self.assertEqual(initial["pid"], os.getpid())
                    self.assertGreater(float(initial["started_at"]), 0)
                    self.assertGreater(float(initial["heartbeat_at"]), 0)

                    deadline = time.monotonic() + 1
                    heartbeat = float(initial["heartbeat_at"])
                    while time.monotonic() < deadline and heartbeat <= float(initial["heartbeat_at"]):
                        time.sleep(0.02)
                        heartbeat = float(json.loads(first_path.read_text(encoding="utf-8"))["heartbeat_at"])
                    self.assertGreater(heartbeat, float(initial["heartbeat_at"]))

                    acquired = threading.Event()
                    second_lock: list[object] = []

                    def acquire_second() -> None:
                        lock = installer.acquire_install_lock(store)
                        second_lock.append(lock)
                        acquired.set()

                    thread = threading.Thread(target=acquire_second, daemon=True)
                    thread.start()
                    time.sleep(0.08)
                    self.assertFalse(acquired.is_set(), "an active create-new lock must not be deleted by a contender")
                finally:
                    installer.release_install_lock(first)

                thread.join(timeout=2)
                self.assertTrue(acquired.is_set(), "the next installer must acquire after the owner releases")
                self.assertFalse(thread.is_alive())
                installer.release_install_lock(second_lock[0])

        active = {"owner_id": "owner", "pid": 321, "started_at": 100.0, "heartbeat_at": 1.0}
        with mock.patch.object(installer, "process_exists", return_value=True), mock.patch.object(
            installer, "process_start_time", return_value=100.0
        ):
            self.assertFalse(installer.install_lock_is_stale(active))
        with mock.patch.object(installer, "process_exists", return_value=False):
            self.assertTrue(installer.install_lock_is_stale(active))
        with mock.patch.object(installer, "process_exists", return_value=True), mock.patch.object(
            installer, "process_start_time", return_value=200.0
        ):
            self.assertTrue(installer.install_lock_is_stale(active), "a reused PID is not the recorded lock owner")

    def test_cpu_mode_does_not_require_or_install_cuda(self) -> None:
        installer = self.installer
        missing_cuda = {
            "openmm_ready": True,
            "version": "8.2.0",
            "platforms": ["Reference", "CPU"],
            "cuda_ready": False,
            "plugin_dir": "",
            "failure_reason": "openmm_platform_list_does_not_include_CUDA",
        }
        calls: list[list[str]] = []

        def fake_run(command, env=None):
            del env
            calls.append(list(command))
            return subprocess.CompletedProcess(command, 0, "", "")

        ready_import = {"ready": True, "version": "installed", "notes": "import_ok"}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            installer, "venv_python", return_value=Path(sys.executable)
        ), mock.patch.object(installer, "probe_python_import", return_value=ready_import), mock.patch.object(
            installer, "probe_local_openmm_cuda", return_value=missing_cuda
        ), mock.patch.object(installer, "run_command", side_effect=fake_run):
            cpu = installer.ensure_local_python_tool_environment(Path(tmp) / "cpu", False, require_cuda=False)
            self.assertEqual(cpu["status"], "ready")
            self.assertEqual(cpu["ready"], len(installer.PYTHON_TOOL_PACKAGES))
            self.assertEqual(cpu["total"], len(installer.PYTHON_TOOL_PACKAGES))
            self.assertFalse(any(installer.OPENMM_CUDA_PIP_PACKAGE in command for command in calls))

            calls.clear()
            gpu = installer.ensure_local_python_tool_environment(Path(tmp) / "gpu", False, require_cuda=True)
            self.assertEqual(gpu["status"], "needs_repair")
            self.assertEqual(gpu["total"], len(installer.PYTHON_TOOL_PACKAGES) + 1)
            self.assertTrue(any(installer.OPENMM_CUDA_PIP_PACKAGE in command for command in calls))

    def test_wsl_selection_probes_all_non_reserved_distributions(self) -> None:
        installer = self.installer
        names = ["Ubuntu-24.04", "Debian", "GenoLigandPhase08"]
        usable = {"Debian": True, "GenoLigandPhase08": True}
        probes: list[str] = []

        def capability_probe(name: str) -> bool:
            probes.append(name)
            return usable.get(name, False)

        with mock.patch.object(installer.os, "name", "nt"), mock.patch.object(
            installer, "list_wsl_distributions", return_value=names
        ), mock.patch.object(installer, "wsl_distribution_usable", side_effect=capability_probe, create=True):
            self.assertEqual(installer.selected_project_wsl_distro(None), "Debian")
            self.assertEqual(probes, ["Debian"])
            self.assertIsNone(installer.selected_project_wsl_distro("Ubuntu-24.04"))
            usable["Debian"] = False
            self.assertIsNone(installer.selected_project_wsl_distro("Debian"))

    def test_failed_cargo_build_never_returns_a_stale_binary(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            stale = root / "target" / "release" / installer.runtime_executable_name("gls-cli")
            stale.parent.mkdir(parents=True)
            stale.write_text("old binary", encoding="utf-8")
            failed = subprocess.CompletedProcess(["cargo", "build"], 101, "", "compile failed")
            with mock.patch.object(installer, "ensure_cargo_available", return_value="cargo"), mock.patch.object(
                installer, "run_command", return_value=failed
            ):
                with self.assertRaises(installer.BinaryBuildError):
                    installer.build_release_binary(root, "gls-cli", "gls-cli")
            self.assertEqual(stale.read_text(encoding="utf-8"), "old binary")

    def test_locked_runtime_publish_stops_only_the_recorded_instance_pid(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "source" / "gls-server.exe"
            source.parent.mkdir()
            source.write_text("new server", encoding="utf-8")
            runtime = root / "runtime"
            runtime.mkdir()
            record = runtime / "gls-server.instance.json"
            record.write_text(
                json.dumps(
                    {
                        "instance_id": "instance-under-test",
                        "binary": "gls-server",
                        "pid": 4242,
                        "started_at": 100.0,
                    }
                ),
                encoding="utf-8",
            )
            termination_command = ["taskkill", "/PID", "4242", "/T", "/F"]
            termination_result = subprocess.CompletedProcess(termination_command, 0, "stopped", "")
            original_copy = installer.shutil.copy2
            copy_attempts = 0

            def copy_once_locked(src, dst):
                nonlocal copy_attempts
                copy_attempts += 1
                if copy_attempts == 1:
                    raise PermissionError("runtime binary is locked")
                return original_copy(src, dst)

            with mock.patch.object(installer.os, "name", "nt"), mock.patch.object(
                installer, "process_exists", return_value=True
            ), mock.patch.object(installer, "process_start_time", return_value=100.0, create=True), mock.patch.object(
                installer, "terminate_runtime_pid", return_value=(termination_command, termination_result)
            ) as terminate, mock.patch.object(
                installer.subprocess,
                "run",
                side_effect=AssertionError("real process termination is forbidden in this contract test"),
            ), mock.patch.object(installer.shutil, "copy2", side_effect=copy_once_locked), mock.patch.object(
                installer.time, "sleep", return_value=None
            ):
                command, status = installer.publish_runtime_binary([str(source)], runtime, "gls-server")

            result = status["stopped_locked_process"]
            self.assertEqual(status["status"], "ready")
            self.assertEqual(Path(command[0]).read_text(encoding="utf-8"), "new server")
            self.assertEqual(result["instance_id"], "instance-under-test")
            terminate.assert_called_once_with(4242)
            self.assertIn("/PID", result["command"])
            self.assertIn("4242", result["command"])
            self.assertNotIn("/IM", result["command"])
            self.assertNotIn("pkill", result["command"])


if __name__ == "__main__":
    unittest.main(testRunner=unittest.TextTestRunner(stream=sys.stdout, verbosity=2))
