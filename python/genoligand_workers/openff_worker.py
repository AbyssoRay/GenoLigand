from __future__ import annotations

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        import openff.toolkit
        import rdkit

        version = (
            f"OpenFF Toolkit {getattr(openff.toolkit, '__version__', 'unknown')}; "
            f"RDKit {getattr(rdkit, '__version__', 'unknown')}"
        )
        status = "PASS"
    except Exception as exc:  # pragma: no cover - environment-dependent smoke
        status = "FAIL"
        version = f"OpenFF/RDKit import failed: {exc}"
    write_result(args.out, job, status, version)
    return 0 if status == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
