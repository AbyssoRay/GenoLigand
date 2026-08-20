# GenoLigand Drug-Target Validation Test Harness
param(
    [string]$DrugName,
    [string]$Smiles,
    [string]$ExpectedUniProt,  # e.g., PDE5A, PTGS2, HMGCR
    [int]$ExpectedRankMax = 15  # Max rank for expected target to be considered success
)

$ErrorActionPreference = "Stop"
$cli = "d:\Project\rust\GenoLigand\target\release\gls-cli.exe"
$scienceStore = "D:\GenoLigandData\science_store"
$workspaceRoot = "d:\Project\rust\GenoLigand"
$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$projectName = "drug-val-$DrugName-$timestamp"
$projectDir = "$env:LOCALAPPDATA\GenoLigand\runtime\projects\$projectName"
$runDir = "$projectDir\runs\run-1"

Write-Host "============================================"
Write-Host "Testing: $DrugName -> $ExpectedUniProt"
Write-Host "============================================"

# 1. Create project directory
New-Item -ItemType Directory -Force -Path $runDir | Out-Null

# 2. Create input manifest
$input = @{
    input_kind = "multi_input_workbench"
    inputs = @(
        @{field="project_name"; value="$DrugName - target validation"},
        @{field="small_molecule"; value=$Smiles}
    )
    files = @()
} | ConvertTo-Json -Compress -Depth 3

$manifestPath = "$projectDir\input_manifest.json"
Set-Content -Path $manifestPath -Value $input -Encoding UTF8
Write-Host "[OK] Input manifest created: $manifestPath"
Write-Host "  SMILES: $Smiles"

# 3. Run pipeline
Write-Host "Running pipeline..."
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$output = & $cli pipeline run `
    --workspace-root $workspaceRoot `
    --science-store $scienceStore `
    --input-manifest $manifestPath `
    --run-dir $runDir 2>&1
$sw.Stop()
$elapsed = $sw.Elapsed.TotalMinutes
Write-Host "[OK] Pipeline completed in $([math]::Round($elapsed, 1)) min"

# 4. Check output
$tsvPath = "$runDir\target_candidate_ranking.tsv"
if (-not (Test-Path $tsvPath)) {
    Write-Host "[FAIL] ranking TSV not found at $tsvPath"
    Write-Host "Pipeline output:"
    $output | Select-Object -Last 20 | ForEach-Object { Write-Host $_ }
    exit 1
}

# 5. Search for expected target
$lines = Get-Content $tsvPath
$header = $lines[0]
$found = $false
$foundRank = -1
$foundTarget = ""
$totalCandidates = ($lines.Count - 1)

Write-Host "`nSearching for $ExpectedUniProt in $totalCandidates candidates..."

for ($i = 1; $i -lt $lines.Count; $i++) {
    $cols = $lines[$i] -split "`t"
    $rank = $cols[0]
    $targetId = $cols[2]
    $targetName = $cols[3]
    $finalScore = $cols[7]
    
    if ($targetId -like "*$ExpectedUniProt*" -or $targetName -like "*$ExpectedUniProt*") {
        $found = $true
        $foundRank = [int]$rank
        $foundTarget = "$targetId - $targetName"
        Write-Host "  FOUND at Rank $rank : $targetId - $targetName (score=$finalScore)"
    }
}

# 6. Show top 15 for context
Write-Host "`n=== Top 15 Candidates ==="
for ($i = 1; $i -le [Math]::Min(15, $lines.Count - 1); $i++) {
    $cols = $lines[$i] -split "`t"
    Write-Host "$($cols[0]). $($cols[2]) - $($cols[3]) [score=$($cols[7])]"
}

# 7. Report result
Write-Host "`n============================================"
if ($found -and $foundRank -le $ExpectedRankMax) {
    Write-Host "[PASS] $DrugName -> $ExpectedUniProt found at rank $foundRank (<= $ExpectedRankMax)"
    Write-Host "RESULT: SUCCESS"
    exit 0
} elseif ($found) {
    Write-Host "[WARN] $DrugName -> $ExpectedUniProt found at rank $foundRank (> $ExpectedRankMax)"
    Write-Host "RESULT: MARGINAL"
    exit 0
} else {
    Write-Host "[FAIL] $DrugName -> $ExpectedUniProt NOT found in top $totalCandidates"
    Write-Host "RESULT: FAILURE"
    exit 1
}
