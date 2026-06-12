from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", default="acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict")
    args = parser.parse_args()
    root = Path.cwd()
    gls_cli = root / "target" / "debug" / ("gls-cli.exe" if __import__("os").name == "nt" else "gls-cli")
    result = subprocess.run(
        [str(gls_cli), "validate", "phase-0-8-5", "--artifact-dir", args.artifact_dir],
        cwd=root,
        text=True,
    )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
