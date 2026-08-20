"""Diagnose why sildenafil is not in the compact database"""
import sqlite3, sys
sys.path.insert(0, r"d:\Project\rust\GenoLigand\python")

# Try to use the same RDKit standardization as the compact DB builder
try:
    from rdkit import Chem
    from rdkit.Chem import rdMolStandardize, AllChem
    HAS_RDKIT = True
except ImportError:
    HAS_RDKIT = False
    print("RDKit not available")

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"

# Sildenafil SMILES (from PubChem)
sildenafil_smiles = "CCCC1=NN(C)C2=C1N=C(Nc1ccc(S(=O)(=O)N3CCN(C)CC3)cc1)c1cc(OC)ccc1-2"

print("=== Step 1: Standardize sildenafil SMILES ===")
if HAS_RDKIT:
    mol = Chem.MolFromSmiles(sildenafil_smiles)
    print(f"  MolFromSmiles: {'OK' if mol else 'FAILED'}")
    if mol:
        print(f"  Heavy atoms: {mol.GetNumHeavyAtoms()}")
        print(f"  MW: {AllChem.CalcExactMolWt(mol):.2f}")
        
        # Apply the same standardization as the compact DB builder
        reionizer = rdMolStandardize.Reionizer()
        tautomer = rdMolStandardize.TautomerCanonicalizer()
        
        cleaned = rdMolStandardize.Cleanup(Chem.Mol(mol))
        parent = rdMolStandardize.FragmentParent(cleaned)
        reionized = reionizer.reionize(parent)
        standardized = tautomer.Canonicalize(reionized)
        Chem.SanitizeMol(standardized)
        
        canon_smi = Chem.MolToSmiles(standardized)
        print(f"  Canonical SMILES: {canon_smi}")
        print(f"  Original SMILES:  {sildenafil_smiles}")
        print(f"  Match: {canon_smi == sildenafil_smiles}")
        
        # Compute fingerprint
        fp = AllChem.GetMorganFingerprintAsBitVect(standardized, 2, 2048, useChirality=True)
        popcount = fp.GetNumOnBits()
        print(f"  Fingerprint popcount: {popcount}")
        print(f"  Fingerprint bits set: {[i for i, b in enumerate(fp) if b][:20]}...")
        
        # Check bitbound for threshold=0.65
        import math
        t = 0.65
        low = math.ceil(t * popcount)
        high = math.floor(popcount / t)
        print(f"  Bitbound for t={t}: low={low}, high={high}")
else:
    canon_smi = sildenafil_smiles

print("\n=== Step 2: Search compact database ===")
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Exact search
cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles = ?", (canon_smi,))
exact = cur.fetchone()[0]
print(f"  Exact match: {exact}")

# LIKE search on the core scaffold
cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles LIKE '%CCCC1=NN%'")
like_nn = cur.fetchone()[0]
print(f"  LIKE 'CCCC1=NN%': {like_nn}")

# Search for molecules with sulfonamide N3CCN (piperazine-sulfonamide)
cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles LIKE '%S(=O)(=O)N%CCN%' AND canonical_smiles LIKE '%NN%'")
sulf_pip_pyrazole = cur.fetchone()[0]
print(f"  Sulfonamide-piperazine-pyrazole: {sulf_pip_pyrazole}")

# Check what sildenafil's target PDE5A (O76074) has as ligands
cur.execute("""
    SELECT m.canonical_smiles, m.popcount
    FROM molecule m
    JOIN evidence e ON m.id = e.molecule_id
    JOIN target t ON e.target_fk = t.id
    WHERE t.target_id = 'O76074'
    ORDER BY m.popcount DESC
    LIMIT 10
""")
print("\n=== PDE5A (O76074) ligands in DB (largest by popcount) ===")
for row in cur.fetchall():
    smi, pop = row
    print(f"  pop={pop} len={len(smi)} smiles={smi[:80]}")

# Check if any molecule in DB has similar popcount to sildenafil
if HAS_RDKIT:
    print(f"\n=== Molecules with popcount near {popcount} ===")
    cur.execute("SELECT COUNT(*) FROM molecule WHERE popcount BETWEEN ? AND ?", 
                (popcount - 3, popcount + 3))
    near = cur.fetchone()[0]
    print(f"  Molecules with popcount {popcount-3}-{popcount+3}: {near}")
    
    # If sildenafil's popcount is outside the DB range
    cur.execute("SELECT MIN(popcount), MAX(popcount), AVG(popcount) FROM molecule")
    minp, maxp, avgp = cur.fetchone()
    print(f"  DB popcount range: {minp}-{maxp}, avg={avgp:.1f}")

# Check if there are any molecules with the pyrazolo-pyrimidine scaffold
print("\n=== Pyrazolo-pyrimidine scaffold search ===")
cur.execute("SELECT COUNT(*) FROM molecule WHERE canonical_smiles LIKE '%1=NN(C)C%' AND canonical_smiles LIKE '%N=C%'")
pyrazolo = cur.fetchone()[0]
print(f"  Pyrazolo-N-containing: {pyrazolo}")

# Check ChEMBL source data directly (if available)
chembl_path = r"D:\GenoLigandData\science_store\indexes\chembl\chembl_query.sqlite"
import os
if os.path.exists(chembl_path):
    cconn = sqlite3.connect(chembl_path)
    cc = cconn.cursor()
    cc.execute("SELECT COUNT(*) FROM genoligand_onsite_activity WHERE canonical_isomeric_smiles LIKE '%CCCC1=NN%'")
    chembl_count = cc.fetchone()[0]
    print(f"\n  ChEMBL source: molecules with CCCC1=NN: {chembl_count}")
    cconn.close()
else:
    print(f"\n  ChEMBL source DB not found at {chembl_path}")

conn.close()
