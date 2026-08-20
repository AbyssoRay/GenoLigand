import sqlite3
import os

db_path = r"D:\GenoLigandData\science_store\indexes\ligand_similarity.sqlite"
print(f"File exists: {os.path.exists(db_path)}")
print(f"File size: {os.path.getsize(db_path) / 1024 / 1024:.1f} MB")

conn = sqlite3.connect(db_path)
cur = conn.cursor()

# Show ALL objects
cur.execute("SELECT type, name, sql FROM sqlite_master")
all_objs = cur.fetchall()
print(f"\n=== All objects in DB ({len(all_objs)}) ===")
for obj in all_objs:
    print(f"  [{obj[0]}] {obj[1]}")
    if obj[2]:
        print(f"    SQL: {obj[2][:200]}")

if len(all_objs) == 0:
    # Maybe it's not a standard sqlite? Let's try raw
    cur.execute("PRAGMA database_list")
    print(f"\nDatabases: {cur.fetchall()}")
    
    cur.execute("PRAGMA schema_version")
    print(f"Schema version: {cur.fetchone()}")

conn.close()
