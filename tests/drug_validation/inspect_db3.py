import sqlite3, os

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity\ligand_similarity.sqlite"
print(f"File: {os.path.getsize(db_path)/1024/1024:.1f} MB")

conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Get schema
cur.execute("SELECT type, name, sql FROM sqlite_master WHERE type='table' ORDER BY name")
tables = cur.fetchall()
print(f"\n=== {len(tables)} Tables ===")
for t in tables:
    print(f"  [{t[0]}] {t[1]}")
    cur.execute(f"PRAGMA table_info([{t[1]}])")
    cols = cur.fetchall()
    for c in cols[:8]:
        print(f"    col: {c[1]} ({c[2]})")
    cur.execute(f"SELECT COUNT(*) FROM [{t[1]}]")
    print(f"    rows: {cur.fetchone()[0]:,}")

# Check for sildenafil-related content
print("\n=== Searching for Sildenafil fingerprints ===")
for t_name, _, _ in tables:
    try:
        cur.execute(f"SELECT * FROM [{t_name}] LIMIT 1")
        cols = [d[0] for d in cur.description]
        for col in cols:
            try:
                cur.execute(f"SELECT COUNT(*) FROM [{t_name}] WHERE [{col}] LIKE '%sildenafil%' OR [{col}] LIKE '%SILDENAFIL%' OR CAST([{col}] AS TEXT) LIKE '%sildenafil%'")
                cnt = cur.fetchone()[0]
                if cnt > 0:
                    print(f"  {t_name}.{col}: {cnt}")
            except:
                pass
    except:
        pass

# Sample some rows
print("\n=== Sample data ===")
for t_name, _, _ in tables[:3]:
    cur.execute(f"SELECT * FROM [{t_name}] LIMIT 3")
    cols = [d[0] for d in cur.description]
    print(f"  Table {t_name}: cols={cols}")
    for row in cur.fetchall():
        print(f"    {[str(v)[:60] for v in row[:5]]}")

conn.close()
