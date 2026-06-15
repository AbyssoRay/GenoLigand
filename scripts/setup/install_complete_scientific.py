from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Prepare or validate the GenoLigand local science store."
    )
    parser.add_argument("--workspace-root", default=".")
    parser.add_argument("--science-store", required=True)
    parser.add_argument("--gls-cli", default="cargo run -p gls-cli --")
    args = parser.parse_args()

    command = args.gls_cli.split() + [
        "install",
        "prepare-science-store",
        "--workspace-root",
        str(Path(args.workspace_root)),
        "--science-store",
        str(Path(args.science_store)),
    ]
    completed = subprocess.run(command, check=False, text=True, capture_output=True)
    payload = {
        "command": command,
        "returncode": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
        "science_store": str(Path(args.science_store)),
    }
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
