import json

from geno_plm.backends.fixture_mock import embed_sequence
from geno_plm.cli import main
from geno_plm.device import choose_device
from geno_plm.io import read_tsv


def test_fixture_mock_is_deterministic_and_finite():
    first = embed_sequence("cand1", "MPEPTIDE", "fixture_mock_64")
    second = embed_sequence("cand1", "MPEPTIDE", "fixture_mock_64")
    assert first == second
    assert len(first) == 64
    assert all(-1.0 <= value <= 1.0 for value in first)


def test_device_selector_cpu_is_explicit():
    assert choose_device("cpu") == "cpu"


def test_cli_embed_and_validate_fixture_mock(tmp_path):
    fasta = tmp_path / "candidates.fa"
    out = tmp_path / "plm"
    meta = out / "embedding_metadata.json"
    fasta.write_text(">cand1|context=K1|orf_len=8|source=test|phase=0.4\nMPEPTIDE\n>cand2|context=K2|orf_len=6|source=test|phase=0.4\nAAAAAA\n", encoding="utf-8")
    assert (
        main(
            [
                "embed",
                "--input-fasta",
                str(fasta),
                "--output-dir",
                str(out),
                "--backend",
                "fixture_mock",
                "--model-id",
                "fixture_mock_64",
                "--device",
                "cpu",
                "--metadata",
                str(meta),
            ]
        )
        == 0
    )
    manifest = out / "embedding_manifest.tsv"
    assert main(["validate", "--manifest", str(manifest)]) == 0
    rows = read_tsv(manifest)
    assert len(rows) == 2
    assert all(row["plm_status"] == "success" for row in rows)
    metadata = json.loads(meta.read_text(encoding="utf-8"))
    assert metadata["backend"] == "fixture_mock"
