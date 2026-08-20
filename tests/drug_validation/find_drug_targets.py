import sqlite3, json

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Target known drug targets with drug-like molecules
# Famous drug targets
targets_of_interest = [
    ('P35354', 'PTGS2', 'COX-2'),
    ('P23219', 'PTGS1', 'COX-1'),
    ('P12821', 'ACE', 'Angiotensin-converting enzyme'),
    ('P04035', 'HMGCR', 'HMG-CoA reductase'),
    ('P08588', 'ADRB1', 'Beta-1 adrenergic receptor'),
    ('P07550', 'ADRB2', 'Beta-2 adrenergic receptor'),
    ('P14416', 'DRD2', 'D2 dopamine receptor'),
    ('P21728', 'DRD1', 'D1 dopamine receptor'),
    ('P31645', 'SLC6A4', 'Serotonin transporter'),
    ('P35367', 'HRH1', 'Histamine H1 receptor'),
    ('P25021', 'HRH2', 'Histamine H2 receptor'),
    ('P30556', 'AGTR1', 'Angiotensin II receptor type 1'),
    ('P08913', 'ADRA2A', 'Alpha-2A adrenergic receptor'),
    ('P08172', 'CHRM2', 'Muscarinic M2 receptor'),
    ('P11229', 'CHRM1', 'Muscarinic M1 receptor'),
    ('P28223', 'HTR2A', '5-HT2A receptor'),
    ('P08908', 'HTR1A', '5-HT1A receptor'),
    ('P28335', 'HTR2C', '5-HT2C receptor'),
    ('P22303', 'ACHE', 'Acetylcholinesterase'),
    ('P00734', 'F2', 'Thrombin'),
    ('P00374', 'DHFR', 'Dihydrofolate reductase'),
    ('P10635', 'CYP2D6', 'Cytochrome P450 2D6'),
    ('P05177', 'CYP1A2', 'Cytochrome P450 1A2'),
    ('Q9BQB6', 'VKORC1', 'Vitamin K epoxide reductase'),
    ('P56817', 'BACE1', 'Beta-secretase 1'),
    ('P00533', 'EGFR', 'EGF receptor'),
    ('O76074', 'PDE5A', 'cGMP-specific PDE 5A'),
    ('P20648', 'ATP4A', 'H+/K+ ATPase'),
    ('Q13936', 'CACNA1C', 'L-type Ca channel'),
    ('P23975', 'SLC6A2', 'Norepinephrine transporter'),
]

print("=== Searching for drug-like molecules targeting well-known proteins ===\n")

results = []
for tid, gene, desc in targets_of_interest:
    cur.execute("""
        SELECT DISTINCT m.id, m.canonical_smiles, m.popcount,
               t.target_id, t.target_name, t.organism, t.source,
               e.activity_score
        FROM molecule m
        JOIN evidence e ON m.id = e.molecule_id
        JOIN target t ON e.target_fk = t.id
        WHERE (t.target_id = ? OR t.target_id LIKE ?)
        AND LENGTH(m.canonical_smiles) BETWEEN 15 AND 100
        AND m.popcount BETWEEN 12 AND 55
        AND e.activity_score >= 0.5
        ORDER BY e.activity_score DESC
        LIMIT 5
    """, (tid, f'%{gene}%'))
    
    rows = cur.fetchall()
    if rows:
        print(f"--- {gene} ({tid}): {desc} ---")
        for row in rows[:3]:
            mid, smi, pop, t_id, t_name, org, src, score = row
            print(f"  [{src}] score={score} pop={pop} len={len(smi)}")
            print(f"  SMILES: {smi}")
            results.append({
                'gene': gene, 'target_id': t_id, 'target_name': t_name,
                'smiles': smi, 'score': score, 'popcount': pop,
                'source': src, 'description': desc
            })
        print()

print(f"\nTotal pairs: {len(results)}")

with open(r'd:\Project\rust\GenoLigand\tests\drug_validation\drug_targets.json', 'w') as f:
    json.dump(results, f, indent=2)

conn.close()
