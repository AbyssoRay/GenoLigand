from __future__ import annotations

import importlib.util
import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "scripts/setup/install_complete_scientific.py"


def load_installer():
    spec = importlib.util.spec_from_file_location("atomic_text_publish_installer", INSTALLER)
    if spec is None or spec.loader is None:
        raise RuntimeError("installer module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class AtomicTextPublishTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.installer = load_installer()

    @staticmethod
    def windows_share_error(code: int) -> PermissionError:
        err = PermissionError(13, f"injected Windows sharing error {code}")
        err.winerror = code
        return err

    def retryable(self, err: OSError) -> bool:
        return getattr(err, "winerror", None) in self.installer.WINDOWS_ATOMIC_REPLACE_RETRY_CODES

    def test_text_publish_retries_winerror_5_with_same_temporary_then_succeeds(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "download_execution_manifest.tsv"
            real_replace = installer.os.replace
            sources: list[Path] = []

            def replace_after_conflicts(source: Path, destination: Path) -> None:
                sources.append(Path(source))
                if len(sources) <= len(installer.ATOMIC_REPLACE_RETRY_DELAYS):
                    raise self.windows_share_error(5)
                real_replace(source, destination)

            with mock.patch.object(installer.os, "replace", side_effect=replace_after_conflicts), mock.patch.object(
                installer.time,
                "sleep",
            ) as sleeper, mock.patch.object(
                installer,
                "windows_atomic_replace_retryable",
                side_effect=self.retryable,
            ):
                with installer.atomic_text_writer(target) as handle:
                    handle.write("source_id\tstatus\nrcsb\tready\n")

            self.assertEqual(len(sources), len(installer.ATOMIC_REPLACE_RETRY_DELAYS) + 1)
            self.assertEqual(len(set(sources)), 1)
            self.assertEqual(
                [call.args[0] for call in sleeper.call_args_list],
                list(installer.ATOMIC_REPLACE_RETRY_DELAYS),
            )
            self.assertEqual(target.read_text(encoding="utf-8"), "source_id\tstatus\nrcsb\tready\n")
            self.assertFalse(Path(f"{target}.attempt.json").exists())
            self.assertFalse(list(target.parent.glob(f".{target.name}.*.tmp")))

    def test_text_publish_persistent_winerror_32_keeps_last_good_and_attempt(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "external_sync_execution_manifest.tsv"
            target.write_text("source_id\tstatus\nlast-good\tready\n", encoding="utf-8")
            last_good = target.read_bytes()
            error = self.windows_share_error(32)

            with mock.patch.object(installer.os, "replace", side_effect=error) as replace, mock.patch.object(
                installer.time,
                "sleep",
            ) as sleeper, mock.patch.object(
                installer,
                "windows_atomic_replace_retryable",
                side_effect=self.retryable,
            ):
                with self.assertRaises(PermissionError) as raised:
                    with installer.atomic_text_writer(target) as handle:
                        handle.write("source_id\tstatus\nnew\tready\n")

            self.assertIs(raised.exception, error)
            self.assertEqual(replace.call_count, len(installer.ATOMIC_REPLACE_RETRY_DELAYS) + 1)
            self.assertEqual(sleeper.call_count, len(installer.ATOMIC_REPLACE_RETRY_DELAYS))
            self.assertEqual(target.read_bytes(), last_good)
            attempt = json.loads(Path(f"{target}.attempt.json").read_text(encoding="utf-8"))
            self.assertEqual(attempt["status"], "failed")
            self.assertEqual(attempt["manifest"], target.name)
            self.assertEqual(attempt["winerror"], 32)
            self.assertEqual(attempt["replace_attempts"], replace.call_count)
            self.assertFalse(list(target.parent.glob(f".{target.name}.*.tmp")))

    def test_text_publish_does_not_retry_non_sharing_error(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "source_file_manifest.tsv"
            target.write_text("last-good\n", encoding="utf-8")
            error = OSError(22, "injected invalid argument")
            error.winerror = 87

            with mock.patch.object(installer.os, "replace", side_effect=error) as replace, mock.patch.object(
                installer.time,
                "sleep",
            ) as sleeper:
                with self.assertRaises(OSError) as raised:
                    with installer.atomic_text_writer(target) as handle:
                        handle.write("new\n")

            self.assertIs(raised.exception, error)
            self.assertEqual(replace.call_count, 1)
            sleeper.assert_not_called()
            self.assertEqual(target.read_text(encoding="utf-8"), "last-good\n")

    def test_concurrent_same_target_text_publish_produces_one_complete_tsv(self) -> None:
        installer = self.installer
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "foldseek_target_databases.tsv"
            real_replace = installer.os.replace
            workers = 8
            barrier = threading.Barrier(workers)
            guard = threading.Lock()
            active = 0
            maximum_active = 0
            sources: list[Path] = []
            failures: list[BaseException] = []
            payloads = {
                sequence: "database_id\tstatus\tpayload\n"
                f"rcsb_{sequence}\tready\t{'x' * (sequence + 257)}\n"
                for sequence in range(workers)
            }

            def observed_replace(source: Path, destination: Path) -> None:
                nonlocal active, maximum_active
                with guard:
                    active += 1
                    maximum_active = max(maximum_active, active)
                    sources.append(Path(source))
                threading.Event().wait(0.005)
                try:
                    real_replace(source, destination)
                finally:
                    with guard:
                        active -= 1

            def writer(sequence: int) -> None:
                try:
                    barrier.wait()
                    with installer.atomic_text_writer(target) as handle:
                        handle.write(payloads[sequence])
                except BaseException as err:
                    failures.append(err)

            with mock.patch.object(installer.os, "replace", side_effect=observed_replace):
                threads = [threading.Thread(target=writer, args=(sequence,)) for sequence in range(workers)]
                for thread in threads:
                    thread.start()
                for thread in threads:
                    thread.join(timeout=5)

            self.assertFalse(failures)
            self.assertTrue(all(not thread.is_alive() for thread in threads))
            self.assertEqual(maximum_active, 1)
            self.assertEqual(len(sources), workers)
            self.assertEqual(len(set(sources)), workers)
            self.assertIn(target.read_text(encoding="utf-8"), payloads.values())
            self.assertFalse(list(target.parent.glob(f".{target.name}.*.tmp")))


if __name__ == "__main__":
    unittest.main()
