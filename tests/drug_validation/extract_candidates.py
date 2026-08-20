import sqlite3, json

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Find molecules that:
# 1. Are in the DB
# 2. Have at least one human protein target with high activity score
# 3. Are drug-like (MW reasonable, not too complex)
# 4. Have clear UniProt target IDs
print("=== Extracting drug-target validation pairs ===\n")

# Find molecules with human protein targets, sorted by activity score
cur.execute("""
    SELECT DISTINCT m.id, m.canonical_smiles, m.popcount, 
           t.target_id, t.target_name, t.organism, t.source,
           e.activity_score,
           LENGTH(m.canonical_smiles) as smi_len
    FROM molecule m
    JOIN evidence e ON m.id = e.molecule_id
    JOIN target t ON e.target_fk = t.id
    WHERE t.target_id LIKE 'P%'  -- UniProt IDs
    AND t.organism LIKE '%Homo sapiens%'
    AND t.target_class = 'known_human_protein'
    AND LENGTH(m.canonical_smiles) BETWEEN 10 AND 80
    AND m.popcount BETWEEN 10 AND 50
    AND e.activity_score >= 0.8
    ORDER BY e.activity_score DESC, t.target_name
    LIMIT 200
""")

pairs = {}
for row in cur.fetchall():
    mid, smi, pop, tid, tname, org, src, score, slen = row
    key = (smi, tid)
    if key not in pairs or score > pairs[key]['score']:
        pairs[key] = {
            'smiles': smi,
            'target_id': tid,
            'target_name': tname,
            'source': src,
            'score': score,
            'popcount': pop,
            'molecule_id': mid
        }

# Sort by target name and print distinct targets with best molecules
seen_targets = set()
results = []
for (smi, tid), info in sorted(pairs.items(), key=lambda x: x[1]['target_name']):
    if tid not in seen_targets:
        seen_targets.add(tid)
        results.append(info)

print(f"Found {len(results)} unique target-molecule pairs")
print()

# Print as a JSON array for easy PowerShell consumption
for i, r in enumerate(results[:30]):
    print(f"{i+1}. [{r['source']}] {r['target_id']}: {r['target_name']}")
    print(f"   SMILES: {r['smiles']}")
    print(f"   Score: {r['score']}, Popcount: {r['popcount']}")
    print()

# Save full list as JSON
with open(r'd:\Project\rust\GenoLigand\tests\drug_validation\candidates.json', 'w') as f:
    json.dump(results[:50], f, indent=2)

print(f"Saved {min(50, len(results))} candidates to candidates.json")

conn.close()
