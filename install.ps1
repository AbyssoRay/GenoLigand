[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Args
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Python = if ($env:GENOLIGAND_PYTHON) { $env:GENOLIGAND_PYTHON } else { "python" }
$Installer = Join-Path $Root "scripts\setup\install_complete_scientific.py"

& $Python $Installer --workspace-root $Root --start-webui @Args
exit $LASTEXITCODE
