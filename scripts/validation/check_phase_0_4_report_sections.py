import argparse


REQUIRED = [
    "## Input Summary",
    "## Sequence Evidence Summary",
    "## pLM Embedding Summary",
    "## Similarity Search Summary",
    "## Sequence Risk Flags",
    "## Sequence Evidence Ranking Table",
    "## Sequence Evidence Controls",
    "## CUDA / Device Metadata",
    "## Phase 0.4 Interpretation Cautions",
    "## Reproducibility Metadata",
    "Phase 0.4 sequence evidence is an audit and prioritization layer.",
    "Synthetic semantic controls are retained for software validation and are not biological discoveries.",
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("report")
    args = parser.parse_args()
    text = open(args.report, "r", encoding="utf-8-sig").read()
    missing = [section for section in REQUIRED if section not in text]
    if missing:
        raise SystemExit("phase_0_4_report_missing:" + ",".join(missing))
    print("phase_0_4_report_sections_ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
