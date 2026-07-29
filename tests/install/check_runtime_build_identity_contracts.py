from __future__ import annotations

import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
INSTALLER_PATH = ROOT / "scripts/setup/install_complete_scientific.py"


def load_installer_module():
    spec = importlib.util.spec_from_file_location("runtime_build_identity_installer", INSTALLER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RuntimeBuildIdentityContracts(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.installer = load_installer_module()

    def git(self, root: Path, *arguments: str) -> str:
        completed = subprocess.run(
            ["git", "-C", str(root), *arguments],
            check=True,
            capture_output=True,
            text=True,
        )
        return completed.stdout.strip()

    def workspace(self, parent: Path) -> Path:
        root = parent / "repo"
        (root / "src").mkdir(parents=True)
        (root / ".gitignore").write_text("target/\nwork/\n", encoding="utf-8")
        (root / "Cargo.toml").write_text(
            '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "1.2.3"\n',
            encoding="utf-8",
        )
        (root / "src/lib.rs").write_text("pub fn answer() -> u8 { 42 }\n", encoding="utf-8")
        self.git(root, "init", "--quiet")
        self.git(root, "config", "user.name", "Runtime Identity Test")
        self.git(root, "config", "user.email", "runtime-identity@example.invalid")
        self.git(root, "add", ".")
        self.git(root, "commit", "--quiet", "-m", "initial")
        return root

    def fake_build(self, root: Path, binary_name: str = "gls-cli") -> Path:
        installer = self.installer
        binary = root / "target/release" / installer.runtime_executable_name(binary_name)
        real_run_command = installer.run_command

        def run(command, env=None):
            if list(command)[:2] == ["cargo", "build"]:
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(b"fixture release binary")
                return subprocess.CompletedProcess(command, 0, "", "")
            return real_run_command(command, env)

        with mock.patch.object(installer, "ensure_cargo_available", return_value="cargo"), mock.patch.object(
            installer, "run_command", side_effect=run
        ):
            observed = installer.build_release_binary(root, binary_name, binary_name)
        self.assertEqual(observed, binary)
        return binary

    def test_repository_version_surfaces_match_workspace_release(self) -> None:
        version = self.installer.workspace_release_version(ROOT)
        self.assertEqual(version, "0.9.76")
        self.assertIn(f"Product Version: {version}", (ROOT / "EULA.txt").read_text(encoding="utf-8"))
        for relative in (
            "README.md",
            "docs/local_webui_user_guide.md",
            "docs/windows_wsl2_installation_guide.md",
            "docs/linux_server_installation_guide.md",
        ):
            text = (ROOT / relative).read_text(encoding="utf-8")
            self.assertIn(version, text, relative)
            self.assertIn(f"v{version}", text, relative)

    def test_modified_staged_and_untracked_sources_block_build_and_publish(self) -> None:
        installer = self.installer
        for state in ("modified", "staged", "untracked"):
            with self.subTest(state=state), tempfile.TemporaryDirectory() as tmp:
                root = self.workspace(Path(tmp))
                source = root / "target/release" / installer.runtime_executable_name("gls-cli")
                source.parent.mkdir(parents=True)
                source.write_bytes(b"old binary")
                if state == "untracked":
                    (root / "src/new.rs").write_text("pub fn new_source() {}\n", encoding="utf-8")
                else:
                    (root / "src/lib.rs").write_text("pub fn answer() -> u8 { 43 }\n", encoding="utf-8")
                    if state == "staged":
                        self.git(root, "add", "src/lib.rs")

                with self.assertRaisesRegex(
                    installer.BinaryBuildError, r"^runtime_source_tree_not_clean$"
                ), mock.patch.object(
                    installer,
                    "ensure_cargo_available",
                    side_effect=AssertionError("dirty source must be rejected before cargo"),
                ):
                    installer.build_release_binary(root, "gls-cli", "gls-cli")

                with self.assertRaisesRegex(
                    installer.BinaryBuildError, r"^runtime_source_tree_not_clean$"
                ), mock.patch.object(
                    installer,
                    "runtime_binary_smoke",
                    side_effect=AssertionError("dirty source must be rejected before binary smoke"),
                ):
                    installer.publish_runtime_binary(
                        [str(source)], root / "work/runtime", "gls-cli", root
                    )

    def test_ignored_outputs_allow_clean_build_and_matching_publish(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = self.workspace(Path(tmp))
            (root / "target/cache").mkdir(parents=True)
            (root / "target/cache/output.bin").write_bytes(b"ignored")
            (root / "work").mkdir()
            (root / "work/evidence.json").write_text("{}\n", encoding="utf-8")
            commit = self.git(root, "rev-parse", "HEAD")
            source = self.fake_build(root)
            smoke = {
                "status": "ready",
                "version": "1.2.3",
                "commit": commit,
                "build_id": f"1.2.3+{commit}",
                "notes": "fixture identity",
            }
            with mock.patch.object(installer, "runtime_binary_smoke", return_value=smoke) as identity:
                command, status = installer.publish_runtime_binary(
                    [str(source)], root / "work/runtime", "gls-cli", root
                )

            self.assertEqual(status["status"], "ready")
            self.assertTrue(status["published"])
            self.assertEqual(status["commit"], commit)
            self.assertEqual(status["expected_commit"], commit)
            self.assertEqual(status["build_id"], f"1.2.3+{commit}")
            self.assertEqual(status["expected_build_id"], f"1.2.3+{commit}")
            self.assertEqual(Path(command[0]).read_bytes(), source.read_bytes())
            self.assertEqual(identity.call_count, 2)

    def test_new_commit_rejects_binary_built_from_previous_commit(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = self.workspace(Path(tmp))
            old_commit = self.git(root, "rev-parse", "HEAD")
            source = self.fake_build(root)
            (root / "src/lib.rs").write_text("pub fn answer() -> u8 { 44 }\n", encoding="utf-8")
            self.git(root, "add", "src/lib.rs")
            self.git(root, "commit", "--quiet", "-m", "new source commit")
            new_commit = self.git(root, "rev-parse", "HEAD")
            self.assertNotEqual(new_commit, old_commit)

            old_identity = {
                "status": "failed",
                "version": "1.2.3",
                "commit": old_commit,
                "build_id": f"1.2.3+{old_commit}",
                "notes": "old fixture identity",
            }
            runtime = root / "work/runtime"
            with mock.patch.object(installer, "runtime_binary_smoke", return_value=old_identity) as identity:
                with self.assertRaisesRegex(
                    installer.BinaryBuildError, r"^runtime_binary_identity_mismatch$"
                ):
                    installer.publish_runtime_binary([str(source)], runtime, "gls-cli", root)

            self.assertEqual(identity.call_args.args[2], new_commit)
            self.assertFalse((runtime / "bin" / installer.runtime_executable_name("gls-cli")).exists())

    def test_archive_prebuilt_replaces_stale_runtime_without_git_metadata(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "release"
            runtime = Path(tmp) / "runtime"
            packaged = root / "bin" / installer.runtime_executable_name("gls-cli")
            target = runtime / "bin" / installer.runtime_executable_name("gls-cli")
            packaged.parent.mkdir(parents=True)
            target.parent.mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "1.2.3"\n',
                encoding="utf-8",
            )
            packaged.write_bytes(b"new release binary")
            target.write_bytes(b"stale installed binary")
            commit = "a" * 40
            smoke = {
                "status": "ready",
                "version": "1.2.3",
                "commit": commit,
                "build_id": f"1.2.3+{commit}",
                "notes": "packaged release identity",
            }

            with mock.patch.object(installer, "runtime_binary_smoke", return_value=smoke):
                command = installer.resolve_binary_command(
                    None, "GENOLIGAND_TEST_GLS_CLI", "gls-cli", "gls-cli", root
                )
                published, status = installer.publish_runtime_binary(
                    command, runtime, "gls-cli", root
                )

            self.assertEqual(command, [str(packaged)])
            self.assertEqual(published, [str(target)])
            self.assertEqual(target.read_bytes(), b"new release binary")
            self.assertEqual(status["commit"], commit)
            self.assertEqual(status["expected_commit"], commit)
            self.assertEqual(status["build_id"], f"1.2.3+{commit}")

    def test_runtime_manifest_v2_publishes_one_canonical_identity(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            runtime = Path(tmp)
            commit = "b" * 40
            build_id = f"1.2.3+{commit}"
            rows = []
            for binary in ("gls-cli", "gls-server"):
                path = runtime / installer.runtime_executable_name(binary)
                path.write_bytes(binary.encode("ascii"))
                rows.append(
                    {
                        "binary": binary,
                        "status": "ready",
                        "path": str(path),
                        "sha256": installer.sha256_file(path),
                        "version": "1.2.3",
                        "expected_version": "1.2.3",
                        "commit": commit,
                        "expected_commit": commit,
                        "build_id": build_id,
                        "expected_build_id": build_id,
                        "build_profile": "release",
                        "smoke": "ready",
                    }
                )
            wrapper = runtime / "python-worker"
            wrapper.write_text("worker", encoding="utf-8")

            manifest = installer.write_runtime_binary_manifest(runtime, rows, wrapper)

            self.assertEqual(manifest["schema_version"], "genoligand_runtime_binary_manifest_v2")
            self.assertEqual(manifest["status"], "ready")
            self.assertEqual(manifest["version"], "1.2.3")
            self.assertEqual(manifest["commit"], commit)
            self.assertEqual(manifest["build_id"], build_id)


if __name__ == "__main__":
    unittest.main()
