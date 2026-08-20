$cli = "d:\Project\rust\GenoLigand\target\release\gls-cli.exe"
$store = "D:\GenoLigandData\science_store"
$ws = "d:\Project\rust\GenoLigand"
$ErrorActionPreference = "Continue"

$tests = @(
    @{n="01-Captopril";s="CC(CS)C(=O)N1CCCC1C(=O)O";t="P12821";k="ACE"},
    @{n="02-Aspirin";s="CC(=O)Oc1ccccc1C(=O)O";t="P35354";k="PTGS2"},
    @{n="03-Isoproterenol";s="CC(C)NCC(O)c1ccc(O)c(O)c1";t="P08588";k="ADRB1"},
    @{n="04-Salbutamol";s="CC(C)(C)NCC(O)c1cc(O)cc(O)c1";t="P07550";k="ADRB2"},
    @{n="05-Dopamine";s="NCCc1ccc(O)c(O)c1";t="P14416";k="DRD2"},
    @{n="06-Diphenhydramine";s="CN1CCN(C(c2ccccc2)c2ccccc2)CC1";t="P35367";k="HRH1"},
    @{n="07-GABA";s="NCCCC(=O)O";t="P31644";k="GABA"},
    @{n="08-Serotonin";s="NCCc1c[nH]c2ccc(O)cc12";t="P08908";k="HTR1A"},
    @{n="09-Melatonin";s="COc1ccc2[nH]cc(CCNC(C)=O)c2c1";t="P48039";k="Melatonin"},
    @{n="10-Tryptamine";s="NCCc1c[nH]c2ccccc12";t="P28223";k="HTR2A"},
    @{n="11-Epinephrine";s="CNCC(O)c1ccc(O)c(O)c1";t="P07550";k="ADRB2"},
    @{n="12-Acetaminophen";s="CC(=O)Nc1ccc(O)cc1";t="P23219";k="PTGS1"},
    @{n="13-Nicotine";s="CN1CCC[C@H]1c1cccnc1";t="P36544";k="CHRNA7"},
    @{n="14-Tyramine";s="NCCc1ccc(O)cc1";t="Q96RJ0";k="TAAR"}
)

$consec = 0
$need = 10
$total = 0

Write-Host "=== GenoLigand Drug-Target Validation ===" -ForegroundColor Cyan
Write-Host "Target: $need consecutive successes`n"

foreach ($t in $tests) {
    $total++
    $ts = Get-Date -Format "HHmmss"
    $pd = "$env:LOCALAPPDATA\GenoLigand\runtime\projects\dvbat-$($t.n)-$ts"
    $rd = "$pd\runs\run-1"
    New-Item -ItemType Directory -Force -Path $rd | Out-Null
    
    $in = '{"input_kind":"multi_input_workbench","inputs":[{"field":"project_name","value":"' + $t.n + '"},{"field":"small_molecule","value":"' + $t.s + '"}],"files":[]}'
    Set-Content -Path "$pd\input_manifest.json" -Value $in -Encoding UTF8
    
    & $cli input normalize --workspace-root $ws --science-store $store --input-manifest "$pd\input_manifest.json" --run-dir $rd 2>&1 | Out-Null
    & $cli features compute --workspace-root $ws --science-store $store --input-manifest "$pd\input_manifest.json" --run-dir $rd 2>&1 | Out-Null
    & $cli database match-known --workspace-root $ws --science-store $store --input-manifest "$pd\input_manifest.json" --run-dir $rd 2>&1 | Out-Null
    
    $kdm = "$rd\known_database_matches.tsv"
    $found = $false
    $score = 0
    $tid = ""
    $cnt = 0
    
    if (Test-Path $kdm) {
        $kl = Get-Content $kdm
        $cnt = $kl.Count - 1
        if ($cnt -gt 0) {
            foreach ($l in $kl[1..($kl.Count-1)]) {
                if ($l -match $t.t -or $l -match $t.k) {
                    $found = $true
                }
            }
        }
    }
    
    if ($found) {
        $consec++
        Write-Host "[$total] PASS #$consec : $($t.n) -> $($t.t) (matches=$cnt)" -ForegroundColor Green
        if ($consec -ge $need) {
            Write-Host "`n*** TARGET REACHED: $consec consecutive successes! ***" -ForegroundColor Cyan
            break
        }
    } else {
        Write-Host "[$total] FAIL : $($t.n) -> $($t.t) NOT in $cnt matches (reset from $consec)" -ForegroundColor Red
        $consec = 0
    }
}

Write-Host "`nFinal: $consec consecutive / $total total tests"
