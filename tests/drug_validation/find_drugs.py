import sqlite3

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Find well-annotated targets with many evidence rows and simple drug-like molecules
print("=== Top targets by evidence count (human proteins) ===")
cur.execute("""
    SELECT t.id, t.source, t.target_id, t.target_name, t.organism, COUNT(*) as ev_count
    FROM target t
    JOIN evidence e ON t.id = e.target_fk
    WHERE t.organism LIKE '%Homo sapiens%' AND t.target_class = 'known_human_protein'
    GROUP BY t.id
    ORDER BY ev_count DESC
    LIMIT 30
""")
for row in cur.fetchall():
    tid, src, target_id, name, org, cnt = row
    print(f"  t={tid} [{src}] {target_id}: {name} ({cnt} ligands)")

# Find molecules that are well-known drugs (by checking for common drug names)
# Let's search for specific well-known compounds
drug_checks = [
    ("Aspirin", "CC(=O)Oc1ccccc1C(=O)O"),
    ("Warfarin", "CC(=O)CC(c1ccccc1)c2c(O)c3ccccc3oc2=O"),
    ("Diazepam", "CN1C(=O)CN=C(c2ccccc2)c3cc(Cl)ccc13"),
    ("Omeprazole", "COc1ccc2nc([S@@](=O)Cc3nc(C)c(OC)c(C)c3C)nc2c1"),
    ("Simvastatin", "CCC(C)(C)C(=O)O[C@H]1C[C@@H](C)C=C2C=C[C@H](C)[C@H](CC[C@@H]3C[C@@H](O)CC(=O)O3)[C@@H]21"),
    ("Caffeine", "CN1C(=O)N(C)c2ncn(C)c2C1=O"),
]

print("\n=== Searching for drug molecules in DB ===")
for name, smi in drug_checks:
    cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles = ?", (smi,))
    exact = cur.fetchone()[0]
    cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles LIKE ?", (smi[:30] + '%',))
    like_count = cur.fetchone()[0]
    if exact > 0 or like_count > 0:
        print(f"  {name}: exact={exact}, like_first30={like_count}")
        if exact > 0:
            cur.execute("SELECT id, canonical_smiles, popcount FROM molecule WHERE canonical_smiles = ?", (smi,))
            for r in cur.fetchall():
                # Get targets
                cur.execute("""
                    SELECT t.target_id, t.target_name, e.activity_score
                    FROM evidence e JOIN target t ON e.target_fk = t.id
                    WHERE e.molecule_id = ?
                    ORDER BY e.activity_score DESC
                    LIMIT 5
                """, (r[0],))
                targets = cur.fetchall()
                tnames = [f"{t[0]}({t[1][:30]},s={t[2]:.3f})" for t in targets]
                print(f"    mol={r[0]} pop={r[2]} targets: {'; '.join(tnames[:3])}")

# Find molecules with SMILES that look like common drugs - short, drug-like
print("\n=== Short drug-like molecules with human targets ===")
cur.execute("""
    SELECT m.id, m.canonical_smiles, m.popcount, COUNT(e.molecule_id) as tgt_count
    FROM molecule m
    JOIN evidence e ON m.id = e.molecule_id
    JOIN target t ON e.target_fk = t.id
    WHERE LENGTH(m.canonical_smiles) BETWEEN 20 AND 50
    AND t.organism LIKE '%Homo sapiens%'
    AND m.popcount BETWEEN 15 AND 40
    GROUP BY m.id
    HAVING tgt_count >= 3
    ORDER BY tgt_count DESC
    LIMIT 20
""")
for row in cur.fetchall():
    mid, smi, pop, tcnt = row
    print(f"  mol={mid} pop={pop} targets={tcnt} smiles={smi[:80]}")

conn.close()
