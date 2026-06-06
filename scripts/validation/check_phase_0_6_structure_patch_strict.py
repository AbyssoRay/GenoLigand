from __future__ import annotations

from pathlib import Path

from phase06_validation import artifact_parser, check_master


def main() -> int:
    args = artifact_parser().parse_args()
    check_master(Path(args.artifact_dir))
    print("phase_0_6_structure_patch_strict_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
