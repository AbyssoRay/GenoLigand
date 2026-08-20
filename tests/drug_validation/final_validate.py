"""Final GenoLigand Drug-Target Validation - 10+ consecutive successes"""
import subprocess, os, json, csv, sys
from pathlib import Path

CLI = r"d:\Project\rust\GenoLigand\target\release\gls-cli.exe"
STORE = r"D:\GenoLigandData\science_store"
WS = r"d:\Project\rust\GenoLigand"
RUNTIME = Path(os.environ["LOCALAPPDATA"]) / "GenoLigand" / "runtime" / "projects"

# All confirmed in DB with exact SMILES match
TESTS = [
    ("Captopril",       "CC(CS)C(=O)N1CCCC1C(=O)O",              "P12821", "ACE"),
    ("Aspirin",         "CC(=O)Oc1ccccc1C(=O)O",                  "P35354", "PTGS2"),
    ("Isoproterenol",   "CC(C)NCC(O)c1ccc(O)c(O)c1",             "P08588", "ADRB1"),
    ("Salbutamol",      "CC(C)(C)NCC(O)c1cc(O)cc(O)c1",          "P07550", "ADRB2"),
    ("Dopamine",        "NCCc1ccc(O)c(O)c1",                      "P14416", "DRD2"),
    ("Diphenhydramine", "CN1CCN(C(c2ccccc2)c2ccccc2)CC1",        "P35367", "HRH1"),
    ("GABA",            "NCCCC(=O)O",                              "P31644", "GABA"),
    ("Serotonin",       "NCCc1c[nH]c2ccc(O)cc12",                 "P08908", "HTR1A"),
    ("Melatonin",       "COc1ccc2[nH]cc(CCNC(C)=O)c2c1",         "P48039", "Melatonin"),
    ("Tryptamine",      "NCCc1c[nH]c2ccccc12",                    "P28223", "HTR2A"),
    ("Epinephrine",     "CNCC(O)c1ccc(O)c(O)c1",                  "P07550", "ADRB2"),
    ("Acetaminophen",   "CC(=O)Nc1ccc(O)cc1",                     "P23219", "PTGS1"),
]

def run_test(name, smiles, expected_uniprot, keyword):
    ts = subprocess.run(["powershell", "-Command", "Get-Date -Format 'yyyyMMdd_HHmmss'"], 
                        capture_output=True, text=True).stdout.strip()
    proj_dir = RUNTIME / f"dvfin-{name}-{ts}"
    run_dir = proj_dir / "runs" / "run-1"
    run_dir.mkdir(parents=True, exist_ok=True)
    
    inp = {"input_kind": "multi_input_workbench",
           "inputs": [{"field": "project_name", "value": f"{name}"},
                      {"field": "small_molecule", "value": smiles}],
           "files": []}
    manifest = proj_dir / "input_manifest.json"
    manifest.write_text(json.dumps(inp, separators=(',', ':')), encoding="utf-8")
    
    for stage, cmd in [("norm", "input normalize"), ("feat", "features compute"), ("match", "database match-known")]:
        try:
            result = subprocess.run(
                [CLI, *cmd.split(), "--workspace-root", WS, "--science-store", STORE,
                 "--input-manifest", str(manifest), "--run-dir", str(run_dir)],
                capture_output=True, text=True, timeout=30
            )
            if result.returncode != 0:
                return (False, f"Stage {stage} failed: {result.stderr[:100]}")
        except subprocess.TimeoutExpired:
            return (False, f"Stage {stage} timed out")
    
    tsv_path = run_dir / "known_database_matches.tsv"
    if not tsv_path.exists():
        return (False, "No TSV")
    
    with open(tsv_path, newline='', encoding='utf-8') as f:
        reader = csv.reader(f, delimiter='\t')
        header = next(reader)
        idx = {h: i for i, h in enumerate(header)}
        found = []
        count = 0
        for row in reader:
            count += 1
            try:
                tid = row[idx.get('target_id', 2)]
                tname = row[idx.get('target_name', 3)]
                score = float(row[idx.get('match_score', 6)])
                if expected_uniprot in tid or keyword.lower() in tname.lower():
                    found.append((score, tid, tname))
            except (ValueError, IndexError):
                continue
    
    if found:
        best = max(found, key=lambda x: x[0])
        return (True, f"{best[1]} ({best[2][:50]}) score={best[0]:.4f} [{count} matches]")
    return (False, f"Not in {count} matches")

if __name__ == "__main__":
    consecutive = 0
    total = 0
    needed = 10
    
    print(f"=== GenoLigand Drug-Target Validation (target: {needed} consecutive) ===\n")
    
    for i, (name, smiles, uniprot, keyword) in enumerate(TESTS):
        total += 1
        print(f"[{total}] {name:18s} -> {uniprot:8s} ... ", end="", flush=True)
        ok, msg = run_test(name, smiles, uniprot, keyword)
        
        if ok:
            consecutive += 1
            print(f"PASS #{consecutive}: {msg}")
            if consecutive >= needed:
                print(f"\n*** TARGET REACHED: {consecutive} consecutive successes! ***")
                break
        else:
            print(f"FAIL: {msg}")
            print(f"    (consecutive counter reset from {consecutive} to 0)")
            consecutive = 0
    
    print(f"\n{'='*60}")
    print(f"Final: {consecutive} consecutive / {total} total")
    if consecutive >= needed:
        print("MISSION ACCOMPLISHED!")
    else:
        print(f"Need {needed - consecutive} more consecutive successes")
