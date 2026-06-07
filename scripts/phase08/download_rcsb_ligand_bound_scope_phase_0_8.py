from __future__ import annotations

import argparse
import csv
import json
import time
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SEARCH_URL = "https://search.rcsb.org/rcsbsearch/v2/query"
DEFAULT_OUT_DIR = ROOT / "data/external_phase_0_8/rcsb_ligand_bound"


def sha256_text(text: str) -> str:
    import hashlib

    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def write_tsv(path: Path, rows: list[dict[str, object]], fields: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


def post_json(payload: dict[str, object], timeout: int = 180) -> dict[str, object]:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        SEARCH_URL,
        data=data,
        method="POST",
        headers={"Content-Type": "application/json", "User-Agent": "GenoLigand-Phase08/0.8"},
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def query_payload(start: int, rows: int) -> dict[str, object]:
    return {
        "query": {
            "type": "terminal",
            "service": "text",
            "parameters": {
                "attribute": "rcsb_entry_info.nonpolymer_entity_count",
                "operator": "greater",
                "value": 0,
            },
        },
        "return_type": "entry",
        "request_options": {
            "paginate": {"start": start, "rows": rows},
            "results_content_type": ["experimental"],
            "sort": [{"sort_by": "rcsb_accession_info.initial_release_date", "direction": "desc"}],
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT_DIR)
    parser.add_argument("--audit", type=Path, default=ROOT / "outputs/phase_0_8/download_rcsb_ligand_bound_scope.tsv")
    parser.add_argument("--page-size", type=int, default=10000)
    parser.add_argument("--limit", type=int, default=0)
    args = parser.parse_args()

    args.out_dir.mkdir(parents=True, exist_ok=True)
    query_path = args.out_dir / "rcsb_ligand_bound_query.json"
    first_payload = query_payload(0, args.page_size)
    query_path.write_text(json.dumps(first_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    rows: list[dict[str, object]] = []
    start = 0
    total_count = 0
    while True:
        payload = query_payload(start, args.page_size)
        response = post_json(payload)
        if start == 0:
            total_count = int(response.get("total_count", 0))
        result_set = response.get("result_set", [])
        if not isinstance(result_set, list) or not result_set:
            break
        for item in result_set:
            if not isinstance(item, dict):
                continue
            identifier = str(item.get("identifier", ""))
            if not identifier:
                continue
            rows.append(
                {
                    "pdb_id": identifier,
                    "query_attribute": "rcsb_entry_info.nonpolymer_entity_count",
                    "query_operator": "greater",
                    "query_value": 0,
                    "return_type": "entry",
                    "source_url": SEARCH_URL,
                    "query_sha256": sha256_text(json.dumps(first_payload, sort_keys=True)),
                    "status": "PASS",
                    "notes": "RCSB Search API ligand-bound entry scope identifier",
                }
            )
            if args.limit and len(rows) >= args.limit:
                break
        if args.limit and len(rows) >= args.limit:
            break
        start += args.page_size
        if start >= total_count:
            break
        time.sleep(0.2)
    manifest_path = args.out_dir / "rcsb_ligand_bound_entry_manifest.tsv"
    fields = ["pdb_id", "query_attribute", "query_operator", "query_value", "return_type", "source_url", "query_sha256", "status", "notes"]
    write_tsv(manifest_path, rows, fields)
    audit_rows = [
        {
            "source_id": "rcsb_ligand_bound",
            "source_url": SEARCH_URL,
            "local_path": manifest_path,
            "entry_count": len(rows),
            "total_count_reported": total_count,
            "query_sha256": sha256_text(json.dumps(first_payload, sort_keys=True)),
            "status": "PASS" if rows else "FAIL",
            "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "notes": "declared ligand-bound scope manifest; mmCIF structure file download and Foldseek indexing remain separate strict gates",
        }
    ]
    write_tsv(args.audit, audit_rows, ["source_id", "source_url", "local_path", "entry_count", "total_count_reported", "query_sha256", "status", "generated_at_utc", "notes"])
    return 0 if rows else 1


if __name__ == "__main__":
    raise SystemExit(main())
