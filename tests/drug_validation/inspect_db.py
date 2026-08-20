import sqlite3

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity.sqlite"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Show tables
cur.execute("SELECT name FROM sqlite_master WHERE type='table'")
tables = [r[0] for r in cur.fetchall()]
print("=== Tables ===")
for t in tables:
    print(f"  {t}")
    cur.execute(f"SELECT COUNT(*) FROM [{t}]")
    print(f"    rows: {cur.fetchone()[0]}")
    cur.execute(f"PRAGMA table_info([{t}])")
    cols = cur.fetchall()
    for c in cols[:10]:
        print(f"    col: {c[1]} ({c[2]})")

# Check for PDE5A-related entries
print("\n=== Searching for PDE5A ===")
for table in tables:
    try:
        cur.execute(f"SELECT * FROM [{table}] LIMIT 1")
        cols = [d[0] for d in cur.description]
        for col in cols:
            try:
                cur.execute(f"SELECT COUNT(*) FROM [{table}] WHERE [{col}] LIKE '%PDE5A%' OR [{col}] LIKE '%PDE5%' OR [{col}] LIKE '%O76074%'")
                cnt = cur.fetchone()[0]
                if cnt > 0:
                    print(f"  {table}.{col}: {cnt} matches")
            except:
                pass
    except:
        pass

# Check for Sildenafil-related entries
print("\n=== Searching for Sildenafil / Viagra ===")
for table in tables:
    try:
        cur.execute(f"SELECT * FROM [{table}] LIMIT 1")
        cols = [d[0] for d in cur.description]
        for col in cols:
            try:
                cur.execute(f"SELECT COUNT(*) FROM [{table}] WHERE [{col}] LIKE '%sildenafil%' OR [{col}] LIKE '%SILDENAFIL%' OR [{col}] LIKE '%viagra%'")
                cnt = cur.fetchone()[0]
                if cnt > 0:
                    print(f"  {table}.{col}: {cnt} matches")
            except:
                pass
    except:
        pass

# Sample the data
print("\n=== Sample data from first table ===")
for table in tables[:1]:
    cur.execute(f"SELECT * FROM [{table}] LIMIT 5")
    cols = [d[0] for d in cur.description]
    print(f"  Table: {table}, cols: {cols}")
    for row in cur.fetchall():
        print(f"    {row[:5]}")

conn.close()
