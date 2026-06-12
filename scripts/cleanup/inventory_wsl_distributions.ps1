param(
  [string]$ArtifactDir = "acceptance_artifacts_phase_0_8_4_openmm_short_md_strict"
)
python scripts/md/md_cli.py wsl-inventory --artifact-dir $ArtifactDir
exit $LASTEXITCODE
