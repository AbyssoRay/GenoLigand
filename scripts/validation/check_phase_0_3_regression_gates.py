import argparse
import csv


EXPECTED_CONTEXT = {
    "p03_k1_cds_001": "K1_known_cds_same_frame",
    "p03_k2_alt_frame_001": "K2_known_locus_alt_frame",
    "p03_k3_utr_001": "K3_utr_orf",
    "p03_k4_lnc_001": "K4_lncRNA_or_nc_transcript_orf",
    "p03_k5_intronic_001": "K5_intronic_orf",
    "p03_k6_antisense_001": "K6_antisense_orf",
    "p03_d1_low_complexity_001": "K_LOW_COMPLEXITY_DECOY",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True)
    parser.add_argument("--gate", required=True)
    parser.add_argument("--cooccurrence", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    context = by_id(read_tsv(args.context))
    gate = by_id(read_tsv(args.gate))
    cooccur = by_id(read_tsv(args.cooccurrence))
    rows = []
    for candidate, expected in EXPECTED_CONTEXT.items():
        observed = context.get(candidate, {}).get("context_class", "")
        rows.append(check(f"context:{candidate}", observed == expected, expected, observed, args.context))
    rows.append(
        check(
            "D1 low complexity blocked",
            gate.get("p03_d1_low_complexity_001", {}).get("pass_plm") == "false",
            "pass_plm=false",
            gate.get("p03_d1_low_complexity_001", {}).get("pass_plm", ""),
            args.gate,
        )
    )
    rows.append(
        check(
            "D2 low information flagged",
            "low_information_background" in gate.get("p03_d2_background_001", {}).get("risk_flags", ""),
            "risk_flags contains low_information_background",
            gate.get("p03_d2_background_001", {}).get("risk_flags", ""),
            args.gate,
        )
    )
    k1 = float(cooccur.get("p03_k1_cds_001", {}).get("cooccurrence_score", "0") or "0")
    d2 = float(cooccur.get("p03_d2_background_001", {}).get("cooccurrence_score", "0") or "0")
    rows.append(check("K1 above D2", k1 > d2, "K1 score > D2 score", f"k1={k1};d2={d2}", args.cooccurrence))
    write_tsv(args.out, rows)
    if any(row["status"] != "PASS" for row in rows):
        raise SystemExit("phase_0_3_regression_failed")
    print(f"phase_0_3_regression_ok\trows={len(rows)}")
    return 0


def check(name, passed, expected, observed, artifact):
    return {
        "gate_id": name.replace(" ", "_"),
        "gate_name": name,
        "status": "PASS" if passed else "FAIL",
        "expected": expected,
        "observed": observed,
        "artifact": artifact,
        "notes": "",
    }


def read_tsv(path):
    with open(path, "r", encoding="utf-8-sig", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def by_id(rows):
    return {row["candidate_id"]: row for row in rows}


def write_tsv(path, rows):
    fields = ["gate_id", "gate_name", "status", "expected", "observed", "artifact", "notes"]
    with open(path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    raise SystemExit(main())
