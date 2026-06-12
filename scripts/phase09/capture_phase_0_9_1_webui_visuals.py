from __future__ import annotations

import argparse
import csv
import os
import subprocess
import sys
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_ARTIFACT = ROOT / "acceptance_artifacts_phase_0_9_1_production_webui"

ROUTES = [
    "dashboard_page",
    "project_create_page",
    "input_wizard_project_basics_page",
    "input_wizard_molecule_type_page",
    "input_wizard_small_molecule_page",
    "input_wizard_peptide_page",
    "input_wizard_protein_page",
    "input_wizard_complex_page",
    "input_wizard_genome_page",
    "input_wizard_search_scope_page",
    "input_wizard_database_profile_page",
    "input_wizard_run_profile_page",
    "input_wizard_preflight_page",
    "input_wizard_review_start_page",
    "run_monitor_page",
    "candidate_ranking_page",
    "candidate_detail_page",
    "genome_orf_context_page",
    "structure_viewer_page",
    "md_summary_page",
    "evidence_ledger_page",
    "database_registry_page",
    "diagnostics_page",
    "settings_page",
    "github_sync_page",
    "report_export_page",
    "error_state_page",
    "empty_state_page",
    "loading_state_page",
]

FORBIDDEN_DOM_TERMS = [
    "".join(map(chr, codepoints))
    for codepoints in [
        [66, 101, 116, 97],
        [68, 69, 77, 79],
        [102, 97, 107, 101],
        [109, 111, 99, 107],
        [112, 108, 97, 99, 101, 104, 111, 108, 100, 101, 114],
        [100, 97, 116, 97, 45, 116, 104, 101, 109, 101, 45, 116, 111, 103, 103, 108, 101],
        [112, 104, 97, 115, 101, 48, 57, 45, 116, 104, 101, 109, 101],
        [83, 97, 109, 112, 108, 101, 32, 112, 114, 111, 106, 101, 99, 116],
        [115, 97, 109, 112, 108, 101, 32, 112, 114, 111, 106, 101, 99, 116],
        [31034, 20363, 39033, 30446],
    ]
]


@dataclass
class RouteAudit:
    route: str
    url: str
    screenshot: Path
    screenshot_bytes: int
    dom_bytes: int
    forbidden_terms: list[str]
    status: str


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Capture GenoLigand 0.9.1 WebUI pages as PNG and write a visual audit manifest."
    )
    parser.add_argument("--base-url", default="http://127.0.0.1:19090/")
    parser.add_argument("--artifact-dir", default=str(DEFAULT_ARTIFACT))
    parser.add_argument("--chrome", default=os.environ.get("CHROME_PATH", ""))
    parser.add_argument("--width", type=int, default=1440)
    parser.add_argument("--height", type=int, default=1000)
    parser.add_argument("--virtual-time-budget", type=int, default=3000)
    args = parser.parse_args()

    base_url = args.base_url.rstrip("/") + "/"
    wait_for_health(base_url)
    chrome = resolve_chrome(args.chrome)
    artifact_dir = Path(args.artifact_dir).resolve()
    screenshot_dir = artifact_dir / "ui" / "screenshots" / "final"
    screenshot_dir.mkdir(parents=True, exist_ok=True)

    audits: list[RouteAudit] = []
    for route in ROUTES:
        url = f"{base_url}#/{route}"
        screenshot = screenshot_dir / f"{route}.png"
        capture_png(chrome, url, screenshot, args.width, args.height, args.virtual_time_budget)
        dom = dump_dom(chrome, url, args.virtual_time_budget)
        terms = [term for term in FORBIDDEN_DOM_TERMS if term in dom]
        screenshot_bytes = screenshot.stat().st_size if screenshot.exists() else 0
        status = "ready" if screenshot_bytes > 2_000 and len(dom) > 500 and not terms else "warning"
        audits.append(
            RouteAudit(
                route=route,
                url=url,
                screenshot=screenshot,
                screenshot_bytes=screenshot_bytes,
                dom_bytes=len(dom.encode("utf-8", errors="replace")),
                forbidden_terms=terms,
                status=status,
            )
        )

    manifest = artifact_dir / "ui" / "visual_audit_manifest.tsv"
    summary = artifact_dir / "ui" / "visual_self_check_summary.tsv"
    write_manifest(manifest, audits)
    warning_count = sum(1 for audit in audits if audit.status != "ready")
    write_summary(summary, audits, warning_count)
    print(f"visual_audit_manifest={manifest}")
    print(f"visual_self_check_summary={summary}")
    print(f"routes={len(audits)} warnings={warning_count}")
    return 0 if warning_count == 0 else 2


def wait_for_health(base_url: str) -> None:
    url = base_url.rstrip("/") + "/api/health"
    deadline = time.time() + 20
    last_error = ""
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=2) as response:
                body = response.read().decode("utf-8", errors="replace")
                if response.status == 200 and '"status":"ready"' in body:
                    return
        except Exception as err:  # noqa: BLE001
            last_error = str(err)
            time.sleep(0.25)
    raise SystemExit(f"health_check_timeout:{last_error}")


def resolve_chrome(explicit: str) -> Path:
    candidates: list[Path] = []
    if explicit:
        candidates.append(Path(explicit))
    candidates.extend(
        [
            Path.home()
            / "AppData/Local/ms-playwright/chromium-1223/chrome-win64/chrome.exe",
            Path.home()
            / "AppData/Local/ms-playwright/chromium-1200/chrome-win64/chrome.exe",
            Path("C:/Program Files/Google/Chrome/Application/chrome.exe"),
            Path("C:/Program Files/Microsoft/Edge/Application/msedge.exe"),
        ]
    )
    for candidate in candidates:
        if candidate.exists():
            return candidate
    raise SystemExit("chrome_not_found: set CHROME_PATH or install Chromium/Chrome/Edge")


def chrome_base_args(chrome: Path, virtual_time_budget: int) -> list[str]:
    return [
        str(chrome),
        "--headless=new",
        "--disable-gpu",
        "--no-sandbox",
        f"--virtual-time-budget={virtual_time_budget}",
    ]


def capture_png(
    chrome: Path,
    url: str,
    output: Path,
    width: int,
    height: int,
    virtual_time_budget: int,
) -> None:
    command = chrome_base_args(chrome, virtual_time_budget) + [
        f"--window-size={width},{height}",
        f"--screenshot={output}",
        url,
    ]
    run_chrome(command)


def dump_dom(chrome: Path, url: str, virtual_time_budget: int) -> str:
    command = chrome_base_args(chrome, virtual_time_budget) + ["--dump-dom", url]
    result = run_chrome(command, capture=True)
    return result.stdout.decode("utf-8", errors="replace")


def run_chrome(command: list[str], capture: bool = False) -> subprocess.CompletedProcess[bytes]:
    result = subprocess.run(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        stderr = result.stderr.decode("utf-8", errors="replace")
        raise SystemExit(f"chrome_failed:{result.returncode}:{stderr}")
    return result


def write_manifest(path: Path, audits: list[RouteAudit]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(
            [
                "route",
                "url",
                "screenshot",
                "screenshot_bytes",
                "dom_bytes",
                "forbidden_terms",
                "status",
            ]
        )
        for audit in audits:
            writer.writerow(
                [
                    audit.route,
                    audit.url,
                    str(audit.screenshot),
                    audit.screenshot_bytes,
                    audit.dom_bytes,
                    ",".join(audit.forbidden_terms),
                    audit.status,
                ]
            )


def write_summary(path: Path, audits: list[RouteAudit], warning_count: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(["metric", "value"])
        writer.writerow(["route_count", len(audits)])
        writer.writerow(["png_rendered_count", sum(1 for audit in audits if audit.screenshot_bytes > 2_000)])
        writer.writerow(["visual_warning_count", warning_count])
        writer.writerow(["all_routes_visual_ready", str(warning_count == 0).lower()])


if __name__ == "__main__":
    raise SystemExit(main())
