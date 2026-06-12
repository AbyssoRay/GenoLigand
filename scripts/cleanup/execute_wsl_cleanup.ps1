param(
  [string]$ArtifactDir = "acceptance_artifacts_phase_0_8_4_openmm_short_md_strict",
  [switch]$Execute
)
$argsList = @("wsl-cleanup", "--artifact-dir", $ArtifactDir)
if ($Execute) { $argsList += "--execute" }
python scripts/md/md_cli.py @argsList
exit $LASTEXITCODE
