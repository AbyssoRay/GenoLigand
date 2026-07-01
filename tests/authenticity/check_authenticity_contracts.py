from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    architecture = read("docs/architecture.md")
    science = read("crates/gls-science/src/lib.rs")
    server = read("crates/gls-server/src/lib.rs")
    md_worker = read("python/genoligand_workers/md_summary_worker.py")

    require("无输入、无数据库、无工具或无运行产物时" in architecture, "authenticity rule missing from architecture docs")
    require("candidate_count" in science, "science pipeline must write candidate count metadata")
    require("user_candidate_rows" in server, "server must derive candidate rows from run artifacts")
    require("candidate_detail_json" in server, "server must expose candidate details from run artifacts")
    require("structure_viewer_payload" in server, "server must expose viewer data only from run structure artifacts")
    require("md_chart_rows" in server, "server must derive MD charts from run MD artifacts")
    require("structure_viewer.pdb" in server, "viewer must be backed by explicit structure coordinate artifacts")
    require("input_conversion_manifest.tsv" in science, "science pipeline must write input conversion manifest")
    require("read_input_conversion_manifest" in md_worker, "MD worker must prefer canonical converted inputs")
    require("read_peptide_sequence_input" in md_worker, "MD worker must accept peptide sequence inputs")
    require("MolFromFASTA" in md_worker, "MD worker must build peptide molecules through RDKit peptide conversion")
    require("rdkit_peptide_conformer_ensemble" in md_worker, "peptide MD artifacts must identify the real conformer calculation method")
    require("no_receptor_contact_model" in md_worker, "peptide MD artifacts must mark missing receptor contact modeling")
    require("当前运行没有可展示的候选假设" in science, "report must preserve empty-result language")
    forbidden_report_claims = ["已确认靶点", "确认靶点", "已确认机制", "确认机制", "真实亲和力", "真实靶点"]
    for term in forbidden_report_claims:
        require(term not in science and term not in server, f"confirmed biological claim is not allowed: {term}")

    print("authenticity_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
