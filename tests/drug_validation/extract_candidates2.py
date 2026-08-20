import sqlite3, json

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Find diverse molecules with clear human protein targets
# Focus on molecules that have unambiguous UniProt target IDs
cur.execute("""
    SELECT DISTINCT m.id, m.canonical_smiles, m.popcount, 
           t.target_id, t.target_name, t.organism, t.source,
           e.activity_score,
           LENGTH(m.canonical_smiles) as smi_len
    FROM molecule m
    JOIN evidence e ON m.id = e.molecule_id
    JOIN target t ON e.target_fk = t.id
    WHERE t.target_id LIKE 'P%'
    AND t.target_class IN ('known_human_protein', 'known_protein')
    AND LENGTH(m.canonical_smiles) BETWEEN 8 AND 100
    AND m.popcount BETWEEN 8 AND 60
    AND e.activity_score >= 0.5
    ORDER BY e.activity_score DESC
    LIMIT 200
""")

seen = {}
for row in cur.fetchall():
    mid, smi, pop, tid, tname, org, src, score, slen = row
    # Prefer human targets but accept others
    key = tid  # One per target
    if key not in seen or (org == 'Homo sapiens' and 'Homo sapiens' not in seen.get(key, {}).get('organism', '')):
        seen[key] = {
            'smiles': smi,
            'target_id': tid,
            'target_name': tname,
            'organism': org,
            'source': src,
            'activity_score': score,
            'popcount': pop,
            'molecule_id': mid,
            'smi_len': slen
        }

results = sorted(seen.values(), key=lambda x: x['activity_score'], reverse=True)
print(f"Found {len(results)} unique target-molecule pairs\n")

for i, r in enumerate(results[:25]):
    print(f"{i+1}. [{r['source']}] {r['target_id']}: {r['target_name']} ({r['organism']})")
    print(f"   SMILES: {r['smiles']}")
    print(f"   Activity: {r['activity_score']}, Pop: {r['popcount']}, Len: {r['smi_len']}")
    print()

with open(r'd:\Project\rust\GenoLigand\tests\drug_validation\candidates.json', 'w') as f:
    json.dump(results[:50], f, indent=2)

print(f"Saved {min(50, len(results))} candidates")

conn.close()
