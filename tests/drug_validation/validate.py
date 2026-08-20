"""GenoLigand Drug-Target Fast Validator
Runs stages 1-3 and checks if expected target appears in known_database_matches.tsv
"""
import subprocess, os, json, csv, sys, tempfile
from pathlib import Path

CLI = r"d:\Project\rust\GenoLigand\target\release\gls-cli.exe"
STORE = r"D:\GenoLigandData\science_store"
WS = r"d:\Project\rust\GenoLigand"
RUNTIME = Path(os.environ["LOCALAPPDATA"]) / "GenoLigand" / "runtime" / "projects"

TESTS = [
    # (name, smiles, expected_uniprot, expected_target_name_keyword)
    ("Captopril", "CC(CS)C(=O)N1CCCC1C(=O)O", "P12821", "ACE"),
    ("Aspirin", "CC(=O)Oc1ccccc1C(=O)O", "P35354", "PTGS2"),
    ("Isoproterenol", "CC(C)NCC(O)c1ccc(O)c(O)c1", "P08588", "ADRB1"),
    ("Salbutamol", "CC(C)(C)NCC(O)c1cc(O)cc(O)c1", "P07550", "ADRB2"),
    ("Dopamine", "NCCc1ccc(O)c(O)c1", "P14416", "DRD2"),
    ("Diphenhydramine", "CN1CCN(C(c2ccccc2)c2ccccc2)CC1", "P35367", "HRH1"),
    ("SalicylicAcid", "O=C(O)c1ccc(O)cc1", "P23219", "PTGS1"),
    ("ACEinhibitor", "CC(NP(=O)(O)O)C(=O)C1CCCC1C(=O)O", "P12821", "ACE"),
    ("Statin", "O=C(O)C[C@H](O)C[C@H](O)CCc1c(Cl)cc(Cl)cc1OCc1ccccc1", "P04035", "HMGCR"),
    ("Acetaminophen", "CC(=O)Nc1ccc(O)cc1", "P23219", "PTGS1"),
    ("Epinephrine", "CNCC(O)c1ccc(O)c(O)c1", "P08588", "ADRB1"),
    ("Histamine", "NCCc1cnc[nH]1", "P35367", "HRH1"),
]

def run_drug_test(name, smiles, expected_uniprot, keyword):
    ts = subprocess.run(["powershell", "-Command", "Get-Date -Format 'yyyyMMdd_HHmmss'"], 
                        capture_output=True, text=True).stdout.strip()
    proj_name = f"dvpy-{name}-{ts}"
    proj_dir = RUNTIME / proj_name
    run_dir = proj_dir / "runs" / "run-1"
    run_dir.mkdir(parents=True, exist_ok=True)
    
    inp = {
        "input_kind": "multi_input_workbench",
        "inputs": [
            {"field": "project_name", "value": f"{name} -> {expected_uniprot}"},
            {"field": "small_molecule", "value": smiles}
        ],
        "files": []
    }
    manifest = proj_dir / "input_manifest.json"
    manifest.write_text(json.dumps(inp, separators=(',', ':')), encoding="utf-8")
    
    # Run stages 1-3
    for stage, cmd in [("normalize", "input normalize"), ("features", "features compute"), ("match", "database match-known")]:
        result = subprocess.run(
            [CLI, *cmd.split(), "--workspace-root", WS, "--science-store", STORE,
             "--input-manifest", str(manifest), "--run-dir", str(run_dir)],
            capture_output=True, text=True, timeout=60
        )
        if result.returncode != 0:
            return (False, f"Stage '{stage}' failed: {result.stderr[:200]}")
    
    # Parse known_database_matches.tsv
    tsv_path = run_dir / "known_database_matches.tsv"
    if not tsv_path.exists():
        return (False, "No known_database_matches.tsv generated")
    
    with open(tsv_path, newline='', encoding='utf-8') as f:
        reader = csv.reader(f, delimiter='\t')
        header = next(reader)
        idx = {h: i for i, h in enumerate(header)}
        
        found_targets = []
        match_count = 0
        for row in reader:
            match_count += 1
            try:
                tid = row[idx.get('target_id', 2)]
                tname = row[idx.get('target_name', 3)]
                score = float(row[idx.get('match_score', 6)])
                
                if expected_uniprot in tid or keyword.lower() in tname.lower():
                    found_targets.append((score, tid, tname))
            except (ValueError, IndexError):
                continue
    
    if found_targets:
        best = max(found_targets, key=lambda x: x[0])
        return (True, f"Found {best[1]} ({best[2][:50]}) score={best[0]:.4f}, total matches={match_count}")
    else:
        return (False, f"Target {expected_uniprot}/{keyword} not in {match_count} matches")

if __name__ == "__main__":
    success = 0
    fail = 0
    for i, (name, smiles, uniprot, keyword) in enumerate(TESTS):
        print(f"[{i+1}/{len(TESTS)}] {name} -> {uniprot} ... ", end="", flush=True)
        ok, msg = run_drug_test(name, smiles, uniprot, keyword)
        if ok:
            print(f"✅ PASS: {msg}")
            success += 1
        else:
            print(f"❌ FAIL: {msg}")
            fail += 1
    
    print(f"\n{'='*60}")
    print(f"RESULTS: {success} PASS, {fail} FAIL out of {len(TESTS)}")
