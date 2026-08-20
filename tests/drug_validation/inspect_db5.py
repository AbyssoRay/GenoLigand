import sqlite3

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Check if sildenafil SMILES is in the DB
# Sildenafil canonical SMILES: CCCC1=NN(C)C2=C1N=C(Nc1ccc(S(=O)(=O)N3CCN(C)CC3)cc1)c1cc(OC)ccc1-2
sildenafil_smiles = "CCCC1=NN(C)C2=C1N=C(Nc1ccc(S(=O)(=O)N3CCN(C)CC3)cc1)c1cc(OC)ccc1-2"

# Try exact match
cur.execute("SELECT id, canonical_smiles, popcount FROM molecule WHERE canonical_smiles = ?", (sildenafil_smiles,))
exact = cur.fetchall()
print(f"Exact sildenafil SMILES match: {len(exact)}")
for r in exact:
    print(f"  id={r[0]} smiles={r[1][:80]} popcount={r[2]}")

# Try SMILES without @@ (achiral version)
sildenafil_no_chiral = "CCCC1=NN(C)C2=C1N=C(Nc1ccc(S(=O)(=O)N3CCN(C)CC3)cc1)c1cc(OC)ccc1-2"
cur.execute("SELECT id, canonical_smiles, popcount FROM molecule WHERE canonical_smiles = ?", (sildenafil_no_chiral,))
exact2 = cur.fetchall()
print(f"Exact achiral match: {len(exact2)}")
for r in exact2:
    print(f"  id={r[0]} smiles={r[1][:80]} popcount={r[2]}")

# Try LIKE search for the core part
cur.execute("SELECT id, canonical_smiles, popcount FROM molecule WHERE canonical_smiles LIKE '%CCCC1=NN(C)C2=C1N=C%' LIMIT 5")
like = cur.fetchall()
print(f"\nLIKE 'CCCC1=NN(C)C2=C1N=C%': {len(like)}")
for r in like:
    print(f"  id={r[0]} smiles={r[1][:100]} popcount={r[2]}")

# Check how many molecules are associated with PDE5A (id=4565 ChEMBL, id=14077 BindingDB)
print("\n=== Ligands for PDE5A (O76074) ===")
# Get molecule IDs linked to PDE5A through evidence table
cur.execute("""
    SELECT COUNT(*) FROM evidence WHERE target_fk IN (4565, 14077)
""")
print(f"Evidence rows for PDE5A (ChEMBL+BindingDB): {cur.fetchone()[0]}")

# Get some PDE5A ligands
cur.execute("""
    SELECT DISTINCT m.id, m.canonical_smiles, m.popcount 
    FROM molecule m 
    JOIN evidence e ON m.id = e.molecule_id 
    WHERE e.target_fk IN (4565, 14077)
    LIMIT 10
""")
pde5_ligands = cur.fetchall()
print(f"Sample PDE5A ligands: {len(pde5_ligands)}")
for r in pde5_ligands:
    print(f"  mol={r[0]} popcount={r[2]} smiles={r[1][:80]}")

# Search for molecules with 'sildenafil' in metadata
print("\n=== Search for sildenafil-like molecules ===")
# Find molecules with sulfonamide AND pyrazole features
cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles LIKE '%S(=O)(=O)N%' AND canonical_smiles LIKE '%NN%' AND canonical_smiles LIKE '%c1cc%'")
print(f"Sulfonamide + NN + c1cc: {cur.fetchone()[0]}")
cur.execute("SELECT id, canonical_smiles, popcount FROM molecule WHERE canonical_smiles LIKE '%S(=O)(=O)N%' AND canonical_smiles LIKE '%NN%' AND canonical_smiles LIKE '%c1cc%' LIMIT 10")
sulf_pyrazole = cur.fetchall()
for r in sulf_pyrazole:
    print(f"  mol={r[0]} popcount={r[2]} smiles={r[1][:100]}")

# What's the SMILES length distribution?
print("\n=== SMILES length distribution ===")
cur.execute("SELECT MIN(LENGTH(canonical_smiles)), AVG(LENGTH(canonical_smiles)), MAX(LENGTH(canonical_smiles)) FROM molecule")
r = cur.fetchone()
print(f"  Min: {r[0]}, Avg: {r[1]:.0f}, Max: {r[2]}")

# Sildenafil SMILES length
print(f"  Sildenafil SMILES length: {len(sildenafil_smiles)}")

conn.close()
