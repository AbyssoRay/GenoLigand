from __future__ import annotations

import csv
import hashlib
import json
import re
from pathlib import Path
from typing import Iterable


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def read_fasta(path: Path) -> list[dict[str, str]]:
    records: list[dict[str, str]] = []
    current_id: str | None = None
    current_desc = ""
    chunks: list[str] = []
    for raw in path.read_text(encoding="utf-8-sig").splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith(">"):
            if current_id is not None:
                records.append({"id": current_id, "description": current_desc, "sequence": "".join(chunks)})
            current_desc = line[1:]
            current_id = current_desc.split()[0].split("|")[0]
            chunks = []
        else:
            chunks.append(line.replace(" ", "").upper())
    if current_id is not None:
        records.append({"id": current_id, "description": current_desc, "sequence": "".join(chunks)})
    return records


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def write_tsv(path: Path, rows: Iterable[dict[str, object]], fieldnames: list[str]) -> None:
    ensure_parent(path)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({name: row.get(name, "") for name in fieldnames})


def write_json(path: Path, payload: dict[str, object]) -> None:
    ensure_parent(path)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def read_json(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_text(text: str) -> str:
    return sha256_bytes(text.encode("utf-8"))


def safe_id(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", value).strip("_") or "candidate"


def bool_text(value: bool) -> str:
    return "true" if value else "false"


def fmt(value: float, digits: int = 6) -> str:
    if value != value or value in (float("inf"), float("-inf")):
        return "nan"
    return f"{value:.{digits}f}"


def require_columns(rows: list[dict[str, str]], columns: list[str], label: str) -> None:
    if not rows:
        raise SystemExit(f"{label}_empty")
    missing = [name for name in columns if name not in rows[0]]
    if missing:
        raise SystemExit(f"{label}_missing_columns:{','.join(missing)}")
