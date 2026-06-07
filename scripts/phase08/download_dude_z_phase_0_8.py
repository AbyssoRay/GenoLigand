from __future__ import annotations

import argparse
import time
import urllib.request
from html.parser import HTMLParser
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BASE_URL = "http://dudez.docking.org/DOCKING_GRIDS_AND_POSES/"
DEFAULT_OUT_DIR = ROOT / "data/external_phase_0_8/dude_z"


class LinkParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.links: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag != "a":
            return
        for key, value in attrs:
            if key == "href" and value:
                self.links.append(value)


def fetch(url: str, retries: int = 3, timeout: int = 240) -> bytes:
    last_exc: Exception | None = None
    for attempt in range(retries):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "GenoLigand-Phase08/0.8"})
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return response.read()
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
            time.sleep(5 * (attempt + 1))
    raise RuntimeError(f"fetch_failed:{url}:{last_exc!r}")


def links_from_html(data: bytes) -> list[str]:
    parser = LinkParser()
    parser.feed(data.decode("utf-8", errors="replace"))
    return parser.links


def sha256_file(path: Path) -> str:
    import hashlib

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_tsv(path: Path, rows: list[dict[str, object]], fields: list[str]) -> None:
    import csv

    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


def save_url(url: str, path: Path) -> tuple[str, str]:
    if path.exists() and path.stat().st_size > 0:
        return "PASS", "existing_file_reused"
    data = fetch(url)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return "PASS" if path.stat().st_size > 0 else "FAIL", "downloaded"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT_DIR)
    parser.add_argument("--audit", type=Path, default=ROOT / "outputs/phase_0_8/download_dude_z.tsv")
    parser.add_argument("--limit-targets", type=int, default=0)
    args = parser.parse_args()

    rows: list[dict[str, object]] = []
    args.out_dir.mkdir(parents=True, exist_ok=True)
    root_index = fetch(BASE_URL)
    root_index_path = args.out_dir / "dude_z_DOCKING_GRIDS_AND_POSES_index.html"
    root_index_path.write_bytes(root_index)
    rows.append(
        {
            "target": "ALL",
            "file_role": "root_index",
            "url": BASE_URL,
            "local_path": root_index_path,
            "bytes": root_index_path.stat().st_size,
            "sha256": sha256_file(root_index_path),
            "status": "PASS",
            "notes": "official_directory_listing_saved",
        }
    )
    target_names = [
        link.rstrip("/")
        for link in links_from_html(root_index)
        if link.endswith("/") and not link.startswith("?") and "Parent" not in link and link not in {"/"}
    ]
    if args.limit_targets > 0:
        target_names = target_names[: args.limit_targets]

    for target in target_names:
        target_url = f"{BASE_URL}{target}/"
        target_dir = args.out_dir / target
        target_index_path = target_dir / f"{target}_index.html"
        status, notes = save_url(target_url, target_index_path)
        rows.append(
            {
                "target": target,
                "file_role": "target_index",
                "url": target_url,
                "local_path": target_index_path,
                "bytes": target_index_path.stat().st_size if target_index_path.exists() else 0,
                "sha256": sha256_file(target_index_path) if target_index_path.exists() and target_index_path.is_file() else "",
                "status": status,
                "notes": notes,
            }
        )
        for filename in ["INDOCK", "rec.crg.pdb", "xtal-lig.pdb"]:
            url = f"{target_url}{filename}"
            path = target_dir / filename
            try:
                status, notes = save_url(url, path)
            except Exception as exc:  # noqa: BLE001
                status, notes = "FAIL", repr(exc)
            rows.append(
                {
                    "target": target,
                    "file_role": "target_metadata",
                    "url": url,
                    "local_path": path,
                    "bytes": path.stat().st_size if path.exists() else 0,
                    "sha256": sha256_file(path) if path.exists() and path.is_file() else "",
                    "status": status,
                    "notes": notes,
                }
            )

        dudez_url = f"{target_url}DUDE_Z/"
        dudez_index_path = target_dir / "DUDE_Z_index.html"
        try:
            dudez_index = fetch(dudez_url)
            dudez_index_path.write_bytes(dudez_index)
            index_status = "PASS"
            index_notes = "official_dude_z_target_listing_saved"
            pose_files = [link for link in links_from_html(dudez_index) if link.endswith(".mol2")]
        except Exception as exc:  # noqa: BLE001
            index_status = "FAIL"
            index_notes = repr(exc)
            pose_files = []
        rows.append(
            {
                "target": target,
                "file_role": "dude_z_index",
                "url": dudez_url,
                "local_path": dudez_index_path,
                "bytes": dudez_index_path.stat().st_size if dudez_index_path.exists() else 0,
                "sha256": sha256_file(dudez_index_path) if dudez_index_path.exists() and dudez_index_path.is_file() else "",
                "status": index_status,
                "notes": index_notes,
            }
        )
        for filename in pose_files:
            role = "ligand_poses" if "ligand" in filename else "decoy_poses" if "decoy" in filename else "pose_file"
            url = f"{dudez_url}{filename}"
            path = target_dir / "DUDE_Z" / filename
            try:
                status, notes = save_url(url, path)
            except Exception as exc:  # noqa: BLE001
                status, notes = "FAIL", repr(exc)
            rows.append(
                {
                    "target": target,
                    "file_role": role,
                    "url": url,
                    "local_path": path,
                    "bytes": path.stat().st_size if path.exists() else 0,
                    "sha256": sha256_file(path) if path.exists() and path.is_file() else "",
                    "status": status,
                    "notes": notes,
                }
            )

    fields = ["target", "file_role", "url", "local_path", "bytes", "sha256", "status", "notes"]
    write_tsv(args.audit, rows, fields)
    return 0 if rows and all(row["status"] == "PASS" for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
