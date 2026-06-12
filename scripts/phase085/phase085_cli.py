from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from run_phase_0_8_5_integrated_chain_rust_cleanup_strict_acceptance import main


if __name__ == "__main__":
    raise SystemExit(main())
