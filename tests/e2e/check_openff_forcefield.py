from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path


EXPECTED_SHA256 = "1b24deb47970bae2d179a5b4e023d4a57c9c78614fe431f1670e3f75e0012c3a"


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_openff_forcefield.py PATH")
    path = Path(sys.argv[1]).resolve()
    observed = hashlib.sha256(path.read_bytes()).hexdigest()
    if observed != EXPECTED_SHA256:
        raise AssertionError(f"force-field digest mismatch: {observed}")
    from openff.toolkit.typing.engines.smirnoff import ForceField

    force_field = ForceField(str(path), load_plugins=True)
    handlers = sorted(force_field.registered_parameter_handlers)
    required = {"Bonds", "Angles", "ProperTorsions", "vdW", "Electrostatics"}
    if not required.issubset(handlers):
        raise AssertionError(f"force-field handlers missing: {sorted(required - set(handlers))}")
    print(
        json.dumps(
            {
                "schema_version": "genoligand_openff_forcefield_acceptance_v1",
                "path": str(path),
                "sha256": observed,
                "handlers": handlers,
            },
            sort_keys=True,
        )
    )
    print("openff_forcefield\tPASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
