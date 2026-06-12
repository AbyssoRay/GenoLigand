from __future__ import annotations

from genoligand_workers.worker_io import parse_worker_args, read_job, write_result


def main() -> int:
    args = parse_worker_args()
    job = read_job(args.job)
    try:
        import openmm

        platforms = [
            openmm.Platform.getPlatform(index).getName()
            for index in range(openmm.Platform.getNumPlatforms())
        ]
        status = "PASS" if "CUDA" in platforms else "FAIL"
        version = f"OpenMM {openmm.version.full_version}; platforms={','.join(platforms)}"
    except Exception as exc:  # pragma: no cover - environment-dependent smoke
        status = "FAIL"
        version = f"OpenMM import failed: {exc}"
    write_result(args.out, job, status, version)
    return 0 if status == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
