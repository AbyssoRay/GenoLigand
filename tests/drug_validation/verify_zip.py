import zipfile, sys, time

path = r"D:\GenoLigandData\science_store\downloads\bindingdb\bindingdb_all.tsv.zip"
print("Opening zip...")
z = zipfile.ZipFile(path)
info = z.infolist()[0]
print(f"Entry: {info.filename} ({info.file_size:,} bytes)")
print("Streaming through entry to verify CRC (decompresses ~9GB, takes a few min)...")
start = time.time()
with z.open(info) as f:
    total = 0
    while True:
        chunk = f.read(8 * 1024 * 1024)
        if not chunk:
            break
        total += len(chunk)
        if total % (1024 * 1024 * 1024) < 8 * 1024 * 1024:
            print(f"  {total / 1e9:.1f} GB verified...")
elapsed = time.time() - start
print(f"CRC verification COMPLETE: {total:,} bytes read in {elapsed:.0f}s")
print("ZIP integrity: OK")
z.close()
