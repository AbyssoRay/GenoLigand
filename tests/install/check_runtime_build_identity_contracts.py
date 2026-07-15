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
        (root / ".gitignore").write_text("target/\nwork/\n.codex/\n", encoding="utf-8")
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
            (root / ".codex").mkdir()
            (root / ".codex/session.json").write_text("{}\n", encoding="utf-8")

            commit = self.git(root, "rev-parse", "HEAD")
            source = self.fake_build(root)
            smoke = {
                "status": "ready",
                "version": "1.2.3",
                "commit": commit,
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


if __name__ == "__main__":
    unittest.main()
