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
    spec = importlib.util.spec_from_file_location("installer_runtime_contracts", INSTALLER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class InstallerRuntimeContracts(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.installer = load_installer_module()

    def test_digest_bound_worker_locks_checkout_with_lf(self) -> None:
        installer = self.installer
        paths = [
            "python/requirements-workers.lock.txt",
            "python/requirements-workers-sources.lock.tsv",
            *(f"python/{row['file']}" for row in installer.worker_lock_catalog().values()),
        ]
        completed = subprocess.run(
            ["git", "check-attr", "eol", "--", *paths],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        attributes = {
            line.rsplit(": ", 2)[0]: line.rsplit(": ", 2)[-1]
            for line in completed.stdout.splitlines()
            if line.strip()
        }
        self.assertEqual(attributes, {path: "lf" for path in paths})

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
            "cuda_context_ready": False,
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
        ), mock.patch.object(
            installer,
            "python_version_probe",
            return_value={"supported": True, "version": "3.12", "executable": sys.executable, "reason": "supported"},
        ), mock.patch.object(
            installer, "select_supported_local_python", return_value=Path(sys.executable)
        ), mock.patch.object(installer, "probe_python_import", return_value=ready_import), mock.patch.object(
            installer, "probe_local_openmm_cuda", return_value=missing_cuda
        ), mock.patch.object(installer, "install_locked_sources_local", return_value={"status": "ready", "reason": "fixture"}), mock.patch.object(
            installer, "run_command", side_effect=fake_run
        ):
            cpu = installer.ensure_local_python_tool_environment(Path(tmp) / "cpu", False, require_cuda=False)
            self.assertEqual(cpu["status"], "ready")
            self.assertEqual(cpu["ready"], len(installer.PYTHON_TOOL_PACKAGES))
            self.assertEqual(cpu["total"], len(installer.PYTHON_TOOL_PACKAGES))
            self.assertFalse(any("cuda12" in " ".join(command).lower() for command in calls))

            calls.clear()
            gpu = installer.ensure_local_python_tool_environment(Path(tmp) / "gpu", False, require_cuda=True)
            self.assertEqual(gpu["status"], "needs_repair")
            self.assertEqual(gpu["total"], len(installer.PYTHON_TOOL_PACKAGES) + 1)
            self.assertTrue(any("cuda12" in " ".join(command).lower() for command in calls))

    def test_cuda_profile_recovers_after_cpu_repair_and_stays_version_aligned(self) -> None:
        installer = self.installer
        state = {"version": installer.OPENMM_RUNTIME_VERSION, "cuda": True}
        force_repairs: list[Path] = []

        def import_probe(_python: Path, import_name: str, _version_expr: str) -> dict[str, object]:
            version = state["version"] if import_name == "openmm" else "installed"
            return {"ready": True, "version": version, "notes": "import_ok"}

        def cuda_probe(_python: Path) -> dict[str, object]:
            return {
                "openmm_ready": True,
                "version": state["version"],
                "platforms": ["Reference", "CPU", *( ["CUDA"] if state["cuda"] else [])],
                "cuda_ready": state["cuda"],
                "cuda_context_ready": state["cuda"],
                "cuda_context_platform": "CUDA" if state["cuda"] else "",
                "cuda_context_failure": "" if state["cuda"] else "CUDA_context_unavailable",
                "plugin_dir": "/plugins" if state["cuda"] else "",
                "default_plugin_dir": "/default-plugins",
                "loaded_plugins": ["libOpenMMCUDA.so"] if state["cuda"] else [],
                "plugin_load_failures": [],
                "failure_reason": "" if state["cuda"] else "CUDA_platform_missing",
            }

        def regular_install(_python: Path, _locks: list[Path]) -> dict[str, str]:
            state.update(version=installer.OPENMM_RUNTIME_VERSION, cuda=False)
            return {"status": "ready", "reason": "runtime_then_cuda_metadata_satisfied"}

        def force_cuda(_python: Path, lock: Path) -> dict[str, str]:
            force_repairs.append(lock)
            state.update(version=installer.OPENMM_CUDA_VERSION, cuda=True)
            return {"status": "ready", "reason": "hashed_cuda_lock_force_reinstalled"}

        locks = [Path("bootstrap.lock"), Path("runtime.lock"), Path("cuda.lock")]
        ready_install = {"status": "ready", "reason": "locked_sources_ready"}
        ready_version = {
            "supported": True,
            "version": "3.12",
            "executable": sys.executable,
            "reason": "supported",
        }
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            installer, "venv_python", return_value=Path(sys.executable)
        ), mock.patch.object(installer, "python_version_probe", return_value=ready_version), mock.patch.object(
            installer, "select_supported_local_python", return_value=Path(sys.executable)
        ), mock.patch.object(installer, "worker_platform_id", return_value="windows_x86_64"), mock.patch.object(
            installer, "worker_lock_paths", return_value=locks
        ), mock.patch.object(installer, "environment_lock_digest", side_effect=lambda _p, _v, cuda: f"lock-{cuda}"), mock.patch.object(
            installer, "worker_source_locks", return_value=[]
        ), mock.patch.object(installer, "probe_python_import", side_effect=import_probe), mock.patch.object(
            installer, "probe_local_openmm_cuda", side_effect=cuda_probe
        ), mock.patch.object(installer, "install_hashed_requirements_local", side_effect=regular_install), mock.patch.object(
            installer, "force_reinstall_hashed_cuda_lock_local", side_effect=force_cuda
        ), mock.patch.object(installer, "install_locked_sources_local", return_value=ready_install):
            env = Path(tmp) / "worker"
            first_gpu = installer.ensure_local_python_tool_environment(env, False, require_cuda=True)
            cpu = installer.ensure_local_python_tool_environment(env, False, require_cuda=False)
            second_gpu = installer.ensure_local_python_tool_environment(env, False, require_cuda=True)

        self.assertEqual(first_gpu["status"], "ready")
        self.assertEqual(cpu["status"], "ready")
        self.assertEqual(second_gpu["status"], "ready")
        self.assertTrue(state["cuda"])
        self.assertEqual(state["version"], installer.OPENMM_CUDA_VERSION)
        self.assertEqual(force_repairs, [locks[-1], locks[-1]])

    def test_worker_locks_keep_openmm_runtime_and_cuda_versions_equal(self) -> None:
        installer = self.installer
        for platform_id in ("windows_x86_64", "manylinux_2_34_x86_64"):
            locks = installer.worker_lock_paths(platform_id, "3.12", True)
            self.assertEqual(
                installer.locked_requirement_version(locks[1], "openmm"),
                installer.OPENMM_RUNTIME_VERSION,
            )
            self.assertEqual(
                installer.locked_requirement_version(locks[2], "openmm-cuda-12"),
                installer.OPENMM_RUNTIME_VERSION,
            )

    def test_openmmforcefields_dependency_is_pinned_and_import_gated(self) -> None:
        installer = self.installer
        spec = next(
            item for item in installer.PYTHON_TOOL_PACKAGES if item["component"] == "importlib_resources"
        )
        self.assertEqual(spec["package"], "importlib-resources==7.1.0")
        self.assertEqual(spec["import_name"], "importlib_resources")
        for platform_id in ("windows_x86_64", "manylinux_2_34_x86_64"):
            runtime_lock = installer.worker_lock_paths(platform_id, "3.12", False)[1]
            self.assertEqual(
                installer.locked_requirement_version(runtime_lock, "importlib-resources"),
                "7.1.0",
            )
        missing = {"ready": False, "version": "", "notes": "ModuleNotFoundError"}
        self.assertFalse(installer.locked_probe_ready(spec, missing))

    def test_locked_probe_normalizes_release_versions_but_rejects_other_releases(self) -> None:
        installer = self.installer
        ready = {"ready": True, "notes": "import_ok"}
        self.assertTrue(
            installer.locked_probe_ready(
                {"package": "rdkit==2025.9.6"},
                {**ready, "version": "2025.09.6"},
            )
        )
        self.assertTrue(
            installer.locked_probe_ready(
                {"package": "openmm==8.5.2"},
                {**ready, "version": "8.5.2.dev-36a30cb"},
            )
        )
        self.assertFalse(
            installer.locked_probe_ready(
                {"package": "openmm==8.5.2"},
                {**ready, "version": "8.5.3"},
            )
        )

    def test_cuda_probe_accepts_opencl_diagnostics_after_a_real_cuda_context(self) -> None:
        installer = self.installer
        probe = {
            "openmm_ready": True,
            "version": "8.5.2.dev-36a30cb",
            "platforms": ["Reference", "CPU", "CUDA"],
            "cuda_ready": True,
            "cuda_context_ready": True,
            "cuda_context_platform": "CUDA",
            "cuda_context_failure": "",
            "plugin_dir": "/plugins",
            "loaded_plugins": ["/plugins/libOpenMMCUDA.so"],
            "plugin_load_failures": [
                "libOpenMMOpenCL.so: libOpenCL.so.1: cannot open shared object file"
            ],
            "failure_reason": "",
        }
        self.assertTrue(installer.openmm_cuda_probe_ready(probe))
        self.assertIn("plugin_load_failures=libOpenMMOpenCL.so", installer.openmm_cuda_notes(probe, "fixture"))
        self.assertIn("openmm.Context", installer.OPENMM_CUDA_PROBE_CODE)
        self.assertIn("context.setPositions", installer.OPENMM_CUDA_PROBE_CODE)

    def test_cuda_probe_rejects_other_release_abi_and_context_failures(self) -> None:
        installer = self.installer
        ready = {
            "openmm_ready": True,
            "version": "8.5.2",
            "platforms": ["Reference", "CPU", "CUDA"],
            "cuda_ready": True,
            "cuda_context_ready": True,
            "cuda_context_platform": "CUDA",
            "cuda_context_failure": "",
            "plugin_dir": "/plugins",
            "loaded_plugins": ["/plugins/libOpenMMCUDA.so"],
            "plugin_load_failures": [],
            "failure_reason": "",
        }
        self.assertFalse(installer.openmm_cuda_probe_ready({**ready, "version": "8.5.3"}))
        self.assertFalse(
            installer.openmm_cuda_probe_ready(
                {
                    **ready,
                    "plugin_load_failures": ["libOpenMMCUDA.so: undefined symbol: PythonForceImpl"],
                }
            )
        )
        self.assertFalse(
            installer.openmm_cuda_probe_ready(
                {
                    **ready,
                    "cuda_context_ready": False,
                    "cuda_context_platform": "",
                    "cuda_context_failure": "CUDA_ERROR_UNKNOWN",
                }
            )
        )

    def test_wsl_locked_source_build_root_uses_direct_nonempty_argv(self) -> None:
        installer = self.installer
        commands: list[list[str]] = []

        def run(_distro: str, command: list[str]) -> subprocess.CompletedProcess[str]:
            commands.append(command)
            return subprocess.CompletedProcess(command, 0, "", "")

        with mock.patch.object(installer, "run_wsl_command", side_effect=run), mock.patch.object(
            installer, "worker_source_locks", return_value=[]
        ), mock.patch.object(installer.uuid, "uuid4", return_value=mock.Mock(hex="fixed")):
            result = installer.install_locked_sources_wsl(
                "GenoLigandTools",
                "/home/test/.venv/bin/python",
                "/home/test",
            )

        build_root = "/home/test/.local/state/genoligand/locked-source-build-fixed"
        self.assertEqual(result, {"status": "ready", "reason": "locked_source_wheels_installed"})
        self.assertEqual(commands, [["mkdir", "-p", "--", build_root], ["rm", "-rf", "--", build_root]])
        self.assertNotIn("bash", [part for command in commands for part in command])
        self.assertTrue(all(build_root in command for command in commands))

    def test_locked_source_git_commands_are_cross_backend_and_repeat_stable(self) -> None:
        installer = self.installer
        source = {
            "git_url": "https://github.com/example/source.git",
            "commit": "a" * 40,
            "version": "1.2.3",
        }
        local = installer.locked_source_checkout_commands("C:/Git/bin/git.exe", Path("C:/build/repo"), source)
        wsl = installer.locked_source_checkout_commands("git", "/build/repo", source)

        self.assertEqual([command[3:] for command in local], [command[3:] for command in wsl])
        self.assertEqual(local[1][3:], ["config", "core.autocrlf", "false"])
        self.assertLess(
            next(index for index, command in enumerate(local) if "core.autocrlf" in command),
            next(index for index, command in enumerate(local) if "checkout" in command),
        )
        local_archive = installer.locked_source_archive_command(
            "C:/Git/bin/git.exe", Path("C:/build/repo"), Path("C:/build/source.tar"), source["commit"]
        )
        wsl_archive = installer.locked_source_archive_command(
            "git", "/build/repo", "/build/source.tar", source["commit"]
        )
        self.assertEqual(local_archive[1:4], ["-c", "core.autocrlf=false", "-C"])
        self.assertEqual(wsl_archive[1:4], ["-c", "core.autocrlf=false", "-C"])
        self.assertEqual(
            wsl_archive,
            installer.locked_source_archive_command(
                "git", "/build/repo", "/build/source.tar", source["commit"]
            ),
        )

    def test_locked_source_fetch_retries_only_explicit_transient_network_failures(self) -> None:
        installer = self.installer
        fetch = ["git", "-C", "/repo", "fetch", "--depth", "1", "origin", "a" * 40]
        transient = subprocess.CompletedProcess(fetch, 1, "", "gnutls_handshake() failed")
        success = subprocess.CompletedProcess(fetch, 0, "", "")
        runner = mock.Mock(side_effect=[transient, transient, success])
        with mock.patch.object(installer.time, "sleep") as sleep:
            completed = installer.run_locked_source_checkout_command(runner, fetch)
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(runner.call_count, 3)
        self.assertEqual(sleep.call_args_list, [mock.call(1), mock.call(2)])

        deterministic = subprocess.CompletedProcess(fetch, 1, "", "repository not found")
        runner = mock.Mock(return_value=deterministic)
        installer.run_locked_source_checkout_command(runner, fetch)
        runner.assert_called_once_with(fetch)

        checkout = ["git", "-C", "/repo", "checkout", "--detach", "a" * 40]
        runner = mock.Mock(return_value=subprocess.CompletedProcess(checkout, 1, "", "RPC failed"))
        installer.run_locked_source_checkout_command(runner, checkout)
        runner.assert_called_once_with(checkout)

    def test_wsl_selection_probes_all_non_reserved_distributions(self) -> None:
        installer = self.installer
        names = ["Ubuntu-24.04", "Debian", "GenoLigandTools"]
        usable = {"Debian": True, "GenoLigandTools": True}
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

            with mock.patch.object(installer, "process_exists", return_value=True), mock.patch.object(
                installer, "process_start_time", return_value=100.0, create=True
            ), mock.patch.object(
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

    def test_dead_runtime_instance_record_is_removed_before_launch(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            runtime = Path(tmp)
            record = runtime / "gls-server.instance.json"
            record.write_text(
                json.dumps(
                    {
                        "instance_id": "dead-instance",
                        "binary": "gls-server",
                        "pid": 4242,
                        "started_at": 100.0,
                    }
                ),
                encoding="utf-8",
            )

            with mock.patch.object(installer, "process_identity_matches", return_value=False):
                result = installer.reconcile_runtime_instance_record(runtime, "gls-server")

            self.assertEqual(result["status"], "stale_instance_removed")
            self.assertEqual(result["pid"], 4242)
            self.assertFalse(record.exists())


if __name__ == "__main__":
    unittest.main(testRunner=unittest.TextTestRunner(stream=sys.stdout, verbosity=2))
