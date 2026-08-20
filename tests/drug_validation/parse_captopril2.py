import csv, os

path = os.path.expandvars(r"%LOCALAPPDATA%\GenoLigand\runtime\projects")
for d in os.listdir(path):
    if d.startswith("dvq-Captopril"):
        tsv = os.path.join(path, d, "runs", "run-1", "known_database_matches.tsv")
        with open(tsv, newline='') as f:
            reader = csv.reader(f, delimiter='\t')
            header = next(reader)
            print(f"Cols ({len(header)}): {header}")
            print()
            seen = {}
            for row in reader:
                if len(row) >= 10:
                    tid = row[2]
                    tn = row[3]
                    score_str = row[6]  # match_score
                    source = row[1]
                    try:
                        score = float(score_str)
                    except:
                        # might be empty or evidence spillover
                        continue
                    if tid not in seen or score > seen[tid][0]:
                        seen[tid] = (score, tn, source)
            
            sorted_t = sorted(seen.items(), key=lambda x: x[1][0], reverse=True)
            for tid, (sc, tn, src) in sorted_t:
                flag = " <-- ACE" if ("angiotensin" in tn.lower() or tid == "P12821") else ""
                print(f"  {sc:.4f} [{src}] {tid}: {tn[:70]}{flag}")
        break
