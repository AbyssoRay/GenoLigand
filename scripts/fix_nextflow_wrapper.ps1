$ErrorActionPreference = "Stop"

$root = "D:\user\codex\tools\nextflow"
$bin = Join-Path $root "bin"
$lib = Join-Path $root "lib"
$scriptOld = Join-Path $bin "nextflow"
$scriptNew = Join-Path $lib "nextflow"

New-Item -ItemType Directory -Path $bin -Force | Out-Null
New-Item -ItemType Directory -Path $lib -Force | Out-Null

if (Test-Path -LiteralPath $scriptOld) {
    Move-Item -LiteralPath $scriptOld -Destination $scriptNew -Force
}

if (-not (Test-Path -LiteralPath $scriptNew)) {
    throw "Nextflow script not found: $scriptNew"
}

$cmdPath = Join-Path $bin "nextflow.cmd"
$shPath = Join-Path $lib "nextflow-wrapper.sh"

$shContent = @'
#!/usr/bin/env bash
set -e
export JAVA_HOME=/d/user/codex/tools/java/jdk-25.0.3+9
export NXF_JAVA_HOME=/d/user/codex/tools/java/jdk-25.0.3+9
export NXF_HOME=/d/user/codex/tools/nextflow/home
export PATH="/d/user/codex/tools/java/jdk-25.0.3+9/bin:/usr/bin:/bin:/mingw64/bin:$PATH"
exec /d/user/codex/tools/nextflow/lib/nextflow "$@"
'@

$cmdContent = @"
@echo off
"D:\Program Files\Git\bin\bash.exe" "D:/user/codex/tools/nextflow/lib/nextflow-wrapper.sh" %*
exit /b %ERRORLEVEL%
"@

[System.IO.File]::WriteAllText($shPath, $shContent.Replace("`r`n", "`n"), [System.Text.Encoding]::ASCII)
Set-Content -LiteralPath $cmdPath -Value $cmdContent -Encoding ASCII
Get-Content -LiteralPath $cmdPath
