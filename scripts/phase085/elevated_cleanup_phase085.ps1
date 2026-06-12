param(
    [string]$WorkspaceRoot = "D:\Project\rust\GenoLigand",
    [string]$ArtifactDir = "D:\Project\rust\GenoLigand\acceptance_artifacts_phase_0_8_5_integrated_chain_rust_cleanup_strict"
)

$ErrorActionPreference = "Continue"

$logDir = Join-Path $ArtifactDir "cleanup"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$logPath = Join-Path $logDir "elevated_acl_cleanup_attempt.tsv"

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

$targets = @(
    (Join-Path $WorkspaceRoot ".phase_0_6_tools\pytest_tmp"),
    (Join-Path $WorkspaceRoot "acceptance_artifacts_phase_0_4\pytest_cache"),
    (Join-Path $WorkspaceRoot "acceptance_artifacts_phase_0_4\pytest_tmp"),
    (Join-Path $WorkspaceRoot ".phase_0_6_tools"),
    (Join-Path $WorkspaceRoot "acceptance_artifacts_phase_0_4")
)

$lines = New-Object System.Collections.Generic.List[string]
$lines.Add("path`twas_present`tis_admin`toperation`texit_code`tresult`tmessage")

foreach ($target in $targets) {
    $presentBefore = Test-Path -LiteralPath $target
    if (-not $presentBefore) {
        $lines.Add("$target`tFalse`t$isAdmin`tnone`t0`tABSENT`tpath_not_present")
        continue
    }

    & takeown.exe /F $target /R /D Y | Out-Null
    $takeownExit = $LASTEXITCODE
    & icacls.exe $target /inheritance:e /T /C | Out-Null
    $inheritExit = $LASTEXITCODE
    & icacls.exe $target /grant:r "${env:USERNAME}:(OI)(CI)F" "Administrators:(OI)(CI)F" /T /C | Out-Null
    $grantExit = $LASTEXITCODE
    & attrib.exe -R -S -H $target /S /D | Out-Null
    $attribExit = $LASTEXITCODE

    $longTarget = "\\?\$target"
    & cmd.exe /c rd /s /q "$longTarget"
    $rdExit = $LASTEXITCODE

    if (Test-Path -LiteralPath $target) {
        $result = "FAILED"
        $message = "takeown=$takeownExit;inheritance=$inheritExit;grant=$grantExit;attrib=$attribExit;rd=$rdExit"
    } else {
        $result = "DELETED"
        $message = "deleted_after_acl_repair"
    }
    $lines.Add("$target`tTrue`t$isAdmin`tacl_repair_and_delete`t$rdExit`t$result`t$message")
}

$lines | Set-Content -LiteralPath $logPath -Encoding UTF8
Write-Host "phase085_elevated_cleanup_log=$logPath"
