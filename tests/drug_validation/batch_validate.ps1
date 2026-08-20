# GenoLigand Rapid Drug Validation (Stages 1-3 only)
# Each test: ~1 second
param()

$cli = "d:\Project\rust\GenoLigand\target\release\gls-cli.exe"
$store = "D:\GenoLigandData\science_store"
$ws = "d:\Project\rust\GenoLigand"

$tests = @(
    @{Name="Aspirin-fast"; SMILES="CC(=O)Oc1ccccc1C(=O)O"; Target="PTGS2"; UniProt="P35354"},
    @{Name="Captopril"; SMILES="CC(CS)C(=O)N1CCCC1C(=O)O"; Target="ACE"; UniProt="P12821"},
    @{Name="Isoproterenol"; SMILES="CC(C)NCC(O)c1ccc(O)c(O)c1"; Target="ADRB1"; UniProt="P08588"},
    @{Name="Salbutamol"; SMILES="CC(C)(C)NCC(O)c1cc(O)cc(O)c1"; Target="ADRB2"; UniProt="P07550"},
    @{Name="Dopamine"; SMILES="NCCc1ccc(O)c(O)c1"; Target="DRD2"; UniProt="P14416"},
    @{Name="Diphenhydramine"; SMILES="CN1CCN(C(c2ccccc2)c2ccccc2)CC1"; Target="HRH1"; UniProt="P35367"},
    @{Name="SalicylicAcid"; SMILES="O=C(O)c1ccc(O)cc1"; Target="PTGS1"; UniProt="P23219"},
    @{Name="ACE-inhibitor"; SMILES="CC(NP(=O)(O)O)C(=O)C1CCCC1C(=O)O"; Target="ACE"; UniProt="P12821"},
    @{Name="Statin"; SMILES="O=C(O)C[C@H](O)C[C@H](O)CCc1c(Cl)cc(Cl)cc1OCc1ccccc1"; Target="HMGCR"; UniProt="P04035"},
    @{Name="Caffeine"; SMILES="CN1C(=O)N(C)c2ncn(C)c2C1=O"; Target="ADORA1"; UniProt="P30542"},
    @{Name="Acetaminophen"; SMILES="CC(=O)Nc1ccc(O)cc1"; Target="PTGS1"; UniProt="P23219"},
    @{Name="Ibuprofen"; SMILES="CC(C)Cc1ccc(C(C)C(=O)O)cc1"; Target="PTGS2"; UniProt="P35354"}
)

$count = 0
$success = 0
$fail = 0

foreach ($t in $tests) {
    $count++
    $ts = Get-Date -Format "yyyyMMdd_HHmmss"
    $pn = "dvq-$($t.Name)-$ts"
    $pd = "$env:LOCALAPPDATA\GenoLigand\runtime\projects\$pn"
    $rd = "$pd\runs\run-1"
    New-Item -ItemType Dir -Force -Path $rd | Out-Null
    
    $in = @{
        input_kind = "multi_input_workbench"
        inputs = @(
            @{field="project_name"; value="$($t.Name)->$($t.Target)"},
            @{field="small_molecule"; value=$t.SMILES}
        )
        files = @()
    } | ConvertTo-Json -Compress -Depth 3
    Set-Content "$pd\input_manifest.json" -Value $in -Encoding UTF8
    
    Write-Host -NoNewline "[$count] $($t.Name) -> $($t.Target) ... "
    
    & $cli input normalize --workspace-root $ws --science-store $store --input-manifest "$pd\input_manifest.json" --run-dir $rd 2>&1 | Out-Null
    & $cli features compute --workspace-root $ws --science-store $store --input-manifest "$pd\input_manifest.json" --run-dir $rd 2>&1 | Out-Null
    & $cli database match-known --workspace-root $ws --science-store $store --input-manifest "$pd\input_manifest.json" --run-dir $rd 2>&1 | Out-Null
    
    $kdm = "$rd\known_database_matches.tsv"
    if (Test-Path $kdm) {
        $kl = Get-Content $kdm
        $found = $false
        $bestScore = 0
        $bestTarget = ""
        for ($i = 1; $i -lt $kl.Count; $i++) {
            $row = $kl[$i]
            if ($row -match "target_id=$($t.UniProt)") {
                # Extract match_score from the row
                $cols = $row -split "`t"
                if ($cols.Count -ge 7) {
                    try {
                        $ms = [double]$cols[6]
                        if ($ms -gt $bestScore) { $bestScore = $ms; $bestTarget = $cols[3] }
                        $found = $true
                    } catch {}
                }
            }
        }
        # Also check if the target_name contains the keyword
        if (-not $found) {
            for ($i = 1; $i -lt $kl.Count; $i++) {
                $row = $kl[$i]
                if ($row -match $t.Target -or $row -match $t.UniProt) {
                    $found = $true
                    break
                }
            }
        }
        
        if ($found) {
            Write-Host "PASS (score=$bestScore matches=$($kl.Count-1))" -ForegroundColor Green
            $success++
        } else {
            Write-Host "FAIL (target not found in $($kl.Count-1) matches)" -ForegroundColor Red
            $fail++
        }
    } else {
        Write-Host "FAIL (no known_database_matches.tsv)" -ForegroundColor Red
        $fail++
    }
}

Write-Host "`n=== RESULTS ===" -ForegroundColor Cyan
Write-Host "Total: $count | Success: $success | Fail: $fail"
