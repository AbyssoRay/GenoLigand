import sqlite3

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Metadata
print("=== Metadata ===")
cur.execute("SELECT key, value FROM metadata")
for row in cur.fetchall():
    print(f"  {row[0]}: {row[1][:200]}")

# Search for PDE5-related targets
print("\n=== Targets matching PDE5 ===")
cur.execute("SELECT id, source, target_id, target_name, organism, target_class FROM target WHERE target_id LIKE '%PDE5%' OR target_name LIKE '%PDE5%' OR target_name LIKE '%phosphodiesterase%5%' OR target_name LIKE '%cGMP-specific%'")
targets = cur.fetchall()
for t in targets:
    print(f"  id={t[0]} source={t[1]} tid={t[2]} name={t[3]} org={t[4]} class={t[5]}")

# Search for sildenafil by SMILES substring
print("\n=== Molecules with sildenafil substructure SMILES ===")
# Sildenafil SMILES key features: sulfonamide (S(=O)(=O)N), pyrazolo[4,3-d]pyrimidine
# Search for molecules containing sulfonamide
cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles LIKE '%S(=O)(=O)N%'")
print(f"  Molecules with sulfonamide: {cur.fetchone()[0]:,}")

# Search for molecules with 'CCCN' pattern (propyl in sildenafil)
cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles LIKE '%CCCC1=NN%'")
print(f"  Molecules with CCC1=NN: {cur.fetchone()[0]:,}")

# Check the fingerprint blob format
print("\n=== Fingerprint format (first molecule) ===")
cur.execute("SELECT id, canonical_smiles, length(fingerprint), hex(substr(fingerprint,1,32)) FROM molecule LIMIT 3")
for row in cur.fetchall():
    fp_len = row[2]
    fp_hex = row[3]
    print(f"  mol={row[0]} smiles={row[1][:60]} fp_len={fp_len} fp_hex={fp_hex}")

# Check molecule count by fingerprint length
print("\n=== Fingerprint length distribution ===")
cur.execute("SELECT length(fingerprint), COUNT(*) FROM molecule GROUP BY length(fingerprint) ORDER BY COUNT(*) DESC LIMIT 10")
for row in cur.fetchall():
    print(f"  fp_len={row[0]}: {row[1]:,} molecules")

# Check the popcount distribution
print("\n=== Popcount distribution ===")
cur.execute("SELECT popcount, COUNT(*) FROM molecule GROUP BY popcount ORDER BY popcount ASC LIMIT 5")
for row in cur.fetchall():
    print(f"  popcount={row[0]}: {row[1]:,}")
cur.execute("SELECT MAX(popcount), AVG(popcount), MIN(popcount) FROM molecule")
row = cur.fetchone()
print(f"  Max popcount: {row[0]}, Avg: {row[1]:.1f}, Min: {row[2]}")

conn.close()
