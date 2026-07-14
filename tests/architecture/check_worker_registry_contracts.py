from __future__ import annotations

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKERS = ROOT / "python" / "genoligand_workers"


def main() -> int:
    rust_registry = (ROOT / "crates" / "gls-science" / "src" / "lib.rs").read_text(
        encoding="utf-8"
    )
    installer_registry = (ROOT / "scripts" / "setup" / "install_complete_scientific.py").read_text(
        encoding="utf-8"
    )
    entrypoints = rust_registry + "\n" + installer_registry
    orphaned = []
    for path in sorted(WORKERS.glob("*_worker.py")):
        module = path.stem
        if f"genoligand_workers.{module}" not in entrypoints and path.name not in entrypoints:
            orphaned.append(path.name)
    if orphaned:
        raise AssertionError(f"unregistered production worker modules: {','.join(orphaned)}")
    print("worker_registry_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
