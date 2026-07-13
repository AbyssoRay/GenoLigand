from __future__ import annotations

import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from geno_plm.cli import MANIFEST_FIELDS, main
from geno_plm.io import read_npy_float32, read_tsv, sha256_file, write_npy_float32, write_tsv


REPO_ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = REPO_ROOT / "scripts" / "validation" / "check_plm_embedding_manifest.py"


class PlmEmbeddingFilenameTests(unittest.TestCase):
    def test_cleaned_header_collisions_get_distinct_content_digests(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plm_names_") as tmp:
            root = Path(tmp)
            fasta = root / "candidates.fasta"
            output = root / "output"
            metadata = root / "metadata.json"
            fasta.write_text(">A/B\nAAAA\n>A?B\nCCCC\n", encoding="utf-8")

            def fake_embed(records, _config):
                rows = [
                    {
                        "candidate_id": record["candidate_id"],
                        "vector": [float(index), float(index + 1)],
                        "sequence_length_effective": len(record["sequence"]),
                        "truncated": "false",
                    }
                    for index, record in enumerate(records, start=1)
                ]
                return rows, {
                    "model_source": "test_fixture",
                    "device_resolved": "cpu",
                    "dtype": "float32",
                    "repr_layer": 1,
                    "software_versions": {},
                }

            with patch("geno_plm.backends.esm_fair.embed_records", side_effect=fake_embed):
                main(
                    [
                        "embed",
                        "--input-fasta",
                        str(fasta),
                        "--output-dir",
                        str(output),
                        "--backend",
                        "esm2_fair",
                        "--model-id",
                        "test-model",
                        "--metadata",
                        str(metadata),
                    ]
                )

            rows = read_tsv(output / "embedding_manifest.tsv")
            paths = [Path(row["embedding_file"]) for row in rows]

            self.assertEqual(len(paths), 2)
            self.assertEqual(len(set(paths)), 2)
            for row, path in zip(rows, paths, strict=True):
                candidate_digest = hashlib.sha256(row["candidate_id"].encode("utf-8")).hexdigest()
                self.assertIn(candidate_digest, path.name)
                self.assertEqual(sha256_file(path), row["embedding_sha256"])
            self.assertEqual(read_npy_float32(paths[0]), [1.0, 2.0])
            self.assertEqual(read_npy_float32(paths[1]), [2.0, 3.0])


class PlmEmbeddingValidatorTests(unittest.TestCase):
    def write_manifest(self, root: Path, rows: list[dict[str, str]]) -> Path:
        manifest = root / "embedding_manifest.tsv"
        write_tsv(manifest, rows, MANIFEST_FIELDS)
        return manifest

    def valid_row(self, candidate_id: str, vector_path: Path) -> dict[str, str]:
        return {
            "candidate_id": candidate_id,
            "model_id": "test-model",
            "backend": "esm2_fair",
            "device_resolved": "cpu",
            "embedding_dim": "2",
            "embedding_file": str(vector_path),
            "embedding_sha256": sha256_file(vector_path),
            "embedding_l2_norm": "2.236068",
            "plm_status": "success",
        }

    def run_validator(
        self, manifest: Path, cwd: Path = REPO_ROOT
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        python_path = str(REPO_ROOT / "python")
        env["PYTHONPATH"] = os.pathsep.join(
            part for part in [python_path, env.get("PYTHONPATH", "")] if part
        )
        return subprocess.run(
            [sys.executable, str(VALIDATOR), str(manifest)],
            cwd=cwd,
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )

    def test_validator_recomputes_sha256_and_rejects_tampering(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plm_tamper_") as tmp:
            root = Path(tmp)
            vector = root / "candidate.mean.npy"
            write_npy_float32(vector, [1.0, 2.0])
            manifest = self.write_manifest(root, [self.valid_row("candidate-1", vector)])
            write_npy_float32(vector, [8.0, 9.0])

            result = self.run_validator(manifest)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("embedding_sha256_mismatch:candidate-1", result.stderr)

    def test_validator_accepts_distinct_untampered_embeddings(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plm_valid_") as tmp:
            root = Path(tmp)
            first = root / "first.mean.npy"
            second = root / "second.mean.npy"
            write_npy_float32(first, [1.0, 2.0])
            write_npy_float32(second, [3.0, 4.0])
            manifest = self.write_manifest(
                root,
                [self.valid_row("candidate-1", first), self.valid_row("candidate-2", second)],
            )

            result = self.run_validator(manifest)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("plm_manifest_ok\trows=2", result.stdout)

    def test_validator_rejects_multiple_rows_referencing_same_embedding(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plm_reference_") as tmp:
            root = Path(tmp)
            vector = root / "shared.mean.npy"
            write_npy_float32(vector, [1.0, 2.0])
            manifest = self.write_manifest(
                root,
                [self.valid_row("candidate-1", vector), self.valid_row("candidate-2", vector)],
            )

            result = self.run_validator(manifest)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("embedding_reference_collision:candidate-2:candidate-1", result.stderr)

    def test_validator_resolves_relative_embedding_from_manifest_directory(self) -> None:
        with tempfile.TemporaryDirectory(prefix="genoligand_plm_relative_") as tmp:
            root = Path(tmp)
            vectors = root / "vectors"
            vectors.mkdir()
            vector = vectors / "candidate.mean.npy"
            write_npy_float32(vector, [1.0, 2.0])
            row = self.valid_row("candidate-1", vector)
            row["embedding_file"] = "vectors/candidate.mean.npy"
            manifest = self.write_manifest(root, [row])

            result = self.run_validator(manifest, cwd=REPO_ROOT)

            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
