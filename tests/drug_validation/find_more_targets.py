import sqlite3, json

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Find molecules with high activity_score against well-known human targets
# Ensure molecules are simple enough (popcount 10-40) and have clear UniProt IDs
cur.execute("""
    SELECT DISTINCT m.canonical_smiles, m.popcount, 
           t.target_id, t.target_name, t.organism,
           e.activity_score, t.source
    FROM molecule m
    JOIN evidence e ON m.id = e.molecule_id
    JOIN target t ON e.target_fk = t.id
    WHERE t.target_id LIKE 'P%'
    AND t.target_class = 'known_human_protein'
    AND LENGTH(m.canonical_smiles) BETWEEN 10 AND 60
    AND m.popcount BETWEEN 10 AND 40
    AND e.activity_score >= 0.9
    ORDER BY e.activity_score DESC
""")

# Collect diverse targets with their best ligand
seen_targets = {}
for row in cur.fetchall():
    smi, pop, tid, tname, org, score, src = row
    if tid not in seen_targets or score > seen_targets[tid][4]:
        seen_targets[tid] = (smi, pop, tname, org, score, src)

# Print diverse list
results = list(seen_targets.values())
results.sort(key=lambda x: x[4], reverse=True)

# Famous drug targets to prioritize
famous = {'P35354', 'P23219', 'P12821', 'P04035', 'P08588', 'P07550', 
          'P14416', 'P21728', 'P31645', 'P35367', 'P25021', 'P30556',
          'P08913', 'P08172', 'P11229', 'P28223', 'P08908', 'P22303',
          'P00734', 'P00374', 'P10635', 'P05177', 'P56817', 'P00533',
          'O76074', 'P20648', 'P23975', 'P28335', 'P14867', 'P00915'}

print("=== Top drug-target pairs for validation ===\n")
count = 0
for smi, pop, tname, org, score, src in results:
    tid = [k for k, v in seen_targets.items() if v[0] == smi][0] if False else ""
    # Find tid
    for k, v in seen_targets.items():
        if v[0] == smi and v[2] == tname:
            tid = k
            break
    
    star = "**" if tid in famous else "  "
    print(f"{star} [{src}] {tid}: {tname[:60]}")
    print(f"   SMILES: {smi}")
    print(f"   pop={pop} score={score:.4f}")
    print()
    count += 1
    if count >= 30:
        break

conn.close()
