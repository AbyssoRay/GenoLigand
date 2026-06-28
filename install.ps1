[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Args
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Installer = Join-Path $Root "scripts\setup\install_complete_scientific.py"

function Resolve-PythonCommand {
    if ($env:GENOLIGAND_PYTHON) {
        return @($env:GENOLIGAND_PYTHON)
    }
    $candidates = @(
        @("py", "-3"),
        @("python"),
        @("python3")
    )
    foreach ($candidate in $candidates) {
        $command = Get-Command $candidate[0] -ErrorAction SilentlyContinue
        if ($command) {
            return $candidate
        }
    }
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($winget) {
        winget install --id Python.Python.3.12 --exact --source winget --accept-package-agreements --accept-source-agreements
        $machinePath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
        $userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
        $env:Path = "$machinePath;$userPath"
        foreach ($candidate in $candidates) {
            $command = Get-Command $candidate[0] -ErrorAction SilentlyContinue
            if ($command) {
                return $candidate
            }
        }
    }
    throw "Python was not found and winget could not install Python automatically. Install Python 3 and rerun install.ps1."
}

$PythonCommand = Resolve-PythonCommand
$PythonArgs = if ($PythonCommand.Count -gt 1) { $PythonCommand[1..($PythonCommand.Count - 1)] } else { @() }
& $PythonCommand[0] @PythonArgs $Installer --workspace-root $Root --start-webui @Args
exit $LASTEXITCODE
