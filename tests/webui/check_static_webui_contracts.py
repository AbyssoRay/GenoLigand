from __future__ import annotations

import sys
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WEB = ROOT / "webui" / "app"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    required = [
        "state/app-state.js",
        "api/client.js",
        "pages/input.js",
        "pages/configure.js",
        "pages/check.js",
        "pages/run.js",
        "pages/results.js",
        "pages/export.js",
        "core/actions.js",
        "core/selectors.js",
        "components/common.js",
        "components/viewers.js",
        "styles/tokens.css",
        "styles/base.css",
        "styles/layout.css",
        "styles/components.css",
        "styles/pages.css",
    ]
    for rel in required:
        require((WEB / rel).is_file(), f"missing split web asset: {rel}")

    index = read(WEB / "index.html")
    require(
        'rel="icon" href="/favicon.svg" type="image/svg+xml"' in index,
        "index.html must declare a first-party favicon",
    )
    for rel in required[:12]:
        require(f'src="/{rel}"' in index, f"index.html does not load {rel}")
    for step in ["configure", "check", "run", "results", "export"]:
        require(
            f'data-step="{step}" class="step disabled" disabled' in index,
            f"initial step navigation must disable {step}",
        )

    user_text = "\n".join(
        read(path)
        for path in [WEB / "index.html", *[WEB / rel for rel in required[:12]]]
    )
    forbidden = ["GitHub", "commit", "remote", "schema", "manifest", "hash", "raw JSON", "gate", "route"]
    for term in forbidden:
        require(term not in user_text, f"user web assets expose developer term: {term}")

    common = read(WEB / "components/common.js")
    require(
        'known_nonhuman_protein: "已知非人源蛋白数据库"' in common,
        "known_nonhuman_protein must render as a user-readable scope label",
    )
    require('{ key: "md_support_label", label: "MD 支持" }' in common, "ranking must show MD support")
    require('{ key: "risk_note", label: "风险提示" }' in common, "ranking must show risk notes")

    run_page = read(WEB / "pages/run.js")
    require(
        "runStageRows().map(renderStage).join(\"\")" in run_page,
        "run view must render backend stage readiness instead of fixed completed labels",
    )
    require(
        "stage.status === \"not_generated\"" in run_page,
        "run view must show not-generated stages for missing current-run artifacts",
    )
    require(
        'class="stage-main"' in run_page and 'class="stage-reason"' in run_page,
        "run view must keep long stage reasons in the text column",
    )
    require(
        "const completed = runState === \"completed\"" not in run_page,
        "run view must not mark every stage complete solely from run state",
    )
    components = read(WEB / "styles/components.css")
    require(".stage-main" in components, "stage text column must have a stable width")
    require(".stage-reason" in components, "stage reasons must wrap as normal text")

    selector_probe = subprocess.run(
        [
            "node",
            "-e",
            read(WEB / "core/selectors.js")
            + """
const checks = {
  invalidSmallMoleculeText: inputValueUsable("small_molecule", "lqtu0we9ur"),
  invalidPeptide: inputValueUsable("peptide", "lqtu0we9ur"),
  invalidProteinLigand: inputValueUsable("protein_ligand", "lqtu0we9ur"),
  invalidGenome: inputValueUsable("genome_or_metagenome", "lqtu0we9ur"),
  validSmallMoleculeSmiles: inputValueUsable("small_molecule", "CCO"),
  validSmallMoleculePeptide: inputValueUsable("small_molecule", "IKVAV"),
  validPeptide: inputValueUsable("peptide", "ACDEFG"),
  validProteinLigand: inputValueUsable("protein_ligand", ">protein\\nMSTNPKPQR"),
  validGenome: inputValueUsable("genome_or_metagenome", ">orf1\\nATGCGTACGTAA")
};
console.log(JSON.stringify(checks));
""",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    selector_checks = json.loads(selector_probe.stdout)
    require(not selector_checks["invalidSmallMoleculeText"], "small molecule input must reject random alphanumeric text")
    require(not selector_checks["invalidPeptide"], "peptide input must reject random alphanumeric text")
    require(
        not selector_checks["invalidProteinLigand"],
        "protein input must reject random alphanumeric text",
    )
    require(not selector_checks["invalidGenome"], "genome input must reject random alphanumeric text")
    require(selector_checks["validSmallMoleculeSmiles"], "small molecule input must accept SMILES")
    require(selector_checks["validSmallMoleculePeptide"], "small molecule input must accept peptide text for backend conversion")
    require(selector_checks["validPeptide"], "peptide input must accept amino-acid sequences")
    require(selector_checks["validProteinLigand"], "protein input must accept FASTA amino-acid sequences")
    require(selector_checks["validGenome"], "genome input must accept FASTA nucleotide sequences")

    mojibake_terms = [
        chr(0x9428),
        chr(0x7ECC),
        chr(0x7039),
        chr(0x6748),
        chr(0x93C1),
        chr(0x93C8),
        chr(0x95C7),
        chr(0x6DC7),
        chr(0x59AB) + chr(0x20AC),
        chr(0x5A34),
    ]
    for term in mojibake_terms:
        require(term not in user_text, f"user web assets contain mojibake text: {term}")

    require("示例项目" not in user_text, "first-run UI must not seed example projects")
    require("components/viewers.js" in index, "index.html must load the structure and MD visual components")
    print("static_webui_contracts\tPASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
