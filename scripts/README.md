# GenoLigand Scripts Boundary

`scripts/` is for repository support tasks, not for the user-facing scientific workflow.

Allowed entrypoint groups:

- `scripts/setup/`: installer internals used by `install.ps1`, `install.sh`, and the WebUI repair action.
- `scripts/databases/`: database preparation, validation, indexing, migration, and repair.
- `scripts/validation/`: repository, artifact, and release contract checks.

The release-cleanliness entrypoint is
`tests/clean_release/check_clean_release.py`. Pass `--workspace-root`,
`--science-store`, and `--runtime-dir` to validate repository transients,
science-store partial/staging state, and stale runtime acceptance state in one
inspection gate.

Controlled wrapper groups:

- `scripts/md/`
- `scripts/structure/`
- `scripts/plm/`
- `scripts/similarity/`

Controlled wrapper groups may be called by GenoLigand-owned adapters or worker harnesses. They are not ordinary user entrypoints and must not be called directly by WebUI, `gls-server`, or Nextflow. Product flow enters through `gls-cli pipeline run`.
