import os

path = os.path.expandvars(r"%LOCALAPPDATA%\GenoLigand\runtime\projects")
# Find the captopril project
for d in os.listdir(path):
    if d.startswith("dvq-Captopril"):
        tsv = os.path.join(path, d, "runs", "run-1", "known_database_matches.tsv")
        with open(tsv) as f:
            lines = f.readlines()
        header = lines[0].strip().split('\t')
        print(f"Cols: {header[:10]}")
        print(f"Total entries: {len(lines)-1}")
        print()
        seen_targets = {}
        for l in lines[1:]:
            c = l.strip().split('\t')
            if len(c) >= 9:
                tid = c[2]
                tn = c[3]
                score = float(c[7])
                source = c[1]
                if tid not in seen_targets or score > seen_targets[tid][0]:
                    seen_targets[tid] = (score, tn, source)
        
        # Sort by score
        sorted_targets = sorted(seen_targets.items(), key=lambda x: x[1][0], reverse=True)
        print("=== Unique Targets (sorted by match_score) ===")
        for tid, (score, tn, src) in sorted_targets:
            ace_flag = " <--- ACE!" if ("angiotensin" in tn.lower() or "ACE" in tid or tid == "P12821") else ""
            print(f"  {score:.4f} [{src}] {tid}: {tn[:70]}{ace_flag}")
        break
