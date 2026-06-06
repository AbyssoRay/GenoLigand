from __future__ import annotations

import csv
import gzip
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from typing import Iterable


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


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


def read_json(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write_json(path: Path, payload: dict[str, object]) -> None:
    ensure_parent(path)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_text(text: str) -> str:
    return sha256_bytes(text.encode("utf-8"))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def safe_id(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", value).strip("_") or "id"


def fmt(value: float, digits: int = 6) -> str:
    if value != value or value in (float("inf"), float("-inf")):
        return "NA"
    return f"{value:.{digits}f}"


def clamp(value: float, lo: float = 0.0, hi: float = 1.0) -> float:
    return max(lo, min(hi, value))


def download(url: str, out: Path, timeout: int = 120) -> None:
    ensure_parent(out)
    req = urllib.request.Request(url, headers={"User-Agent": "GenoLigand-Phase06/0.6"})
    with urllib.request.urlopen(req, timeout=timeout) as response, out.open("wb") as handle:
        shutil.copyfileobj(response, handle)


def command_text(command: list[str], cwd: Path | None = None, timeout: int = 120) -> tuple[int, str]:
    completed = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=timeout, check=False)
    return completed.returncode, (completed.stdout + completed.stderr)


def run_logged(command: list[str], stdout: Path, stderr: Path, cwd: Path | None = None, timeout: int = 3600) -> int:
    ensure_parent(stdout)
    ensure_parent(stderr)
    with stdout.open("w", encoding="utf-8", errors="replace") as out, stderr.open("w", encoding="utf-8", errors="replace") as err:
        try:
            completed = subprocess.run(command, cwd=cwd, text=True, stdout=out, stderr=err, timeout=timeout, check=False)
            return completed.returncode
        except Exception as exc:
            err.write(repr(exc) + "\n")
            return 1


def win_to_wsl(path: Path | str) -> str:
    raw = str(path)
    if re.match(r"^[A-Za-z]:\\", raw):
        drive = raw[0].lower()
        rest = raw[2:].replace("\\", "/").lstrip("/")
        return f"/mnt/{drive}/{rest}"
    return raw.replace("\\", "/")


def wsl_command(distro: str, command: list[str]) -> list[str]:
    linux_path = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    return ["wsl.exe", "-d", distro, "--", "env", f"PATH={linux_path}", *command]


def require_columns(rows: list[dict[str, str]], columns: list[str], label: str) -> None:
    if not rows:
        raise SystemExit(f"{label}_empty")
    missing = [col for col in columns if col not in rows[0]]
    if missing:
        raise SystemExit(f"{label}_missing_columns:{','.join(missing)}")


AA3 = {
    "ALA": "A",
    "ARG": "R",
    "ASN": "N",
    "ASP": "D",
    "CYS": "C",
    "GLN": "Q",
    "GLU": "E",
    "GLY": "G",
    "HIS": "H",
    "ILE": "I",
    "LEU": "L",
    "LYS": "K",
    "MET": "M",
    "PHE": "F",
    "PRO": "P",
    "SER": "S",
    "THR": "T",
    "TRP": "W",
    "TYR": "Y",
    "VAL": "V",
}
HYDRO = set("AILMFWYV")
AROM = set("FWYH")
POLAR = set("STNQCY")
POS = set("KRH")
NEG = set("DE")


def parse_pdb_atoms(path: Path) -> list[dict[str, object]]:
    atoms = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if not line.startswith(("ATOM  ", "HETATM")):
            continue
        try:
            atoms.append(
                {
                    "record": line[:6].strip(),
                    "atom": line[12:16].strip(),
                    "resname": line[17:20].strip(),
                    "chain": line[21].strip() or "A",
                    "resid": int(line[22:26]),
                    "x": float(line[30:38]),
                    "y": float(line[38:46]),
                    "z": float(line[46:54]),
                    "bfactor": float(line[60:66]) if len(line) >= 66 and line[60:66].strip() else 0.0,
                }
            )
        except ValueError:
            continue
    return atoms


def ca_residues(path: Path) -> list[dict[str, object]]:
    residues = []
    seen: set[tuple[str, int]] = set()
    for atom in parse_pdb_atoms(path):
        if atom["record"] != "ATOM" or atom["atom"] != "CA":
            continue
        key = (str(atom["chain"]), int(atom["resid"]))
        if key in seen:
            continue
        seen.add(key)
        aa = AA3.get(str(atom["resname"]), "X")
        residues.append({**atom, "aa": aa})
    return residues


def pdb_summary(path: Path) -> dict[str, object]:
    atoms = parse_pdb_atoms(path)
    residues = ca_residues(path)
    plddts = [float(row["bfactor"]) for row in residues]
    return {
        "residue_count": len(residues),
        "atom_count": len(atoms),
        "chain_count": len({row["chain"] for row in residues}),
        "mean_plddt": fmt(sum(plddts) / max(1, len(plddts))) if residues else "NA",
    }


def residue_number(token: str) -> int | None:
    match = re.search(r"-?\d+", token)
    return int(match.group(0)) if match else None


def patch_features_from_residues(candidate_id: str, residues: list[dict[str, object]], radius: float = 8.0) -> dict[str, object]:
    aas = [str(row.get("aa", "X")) for row in residues]
    xs = [float(row.get("x", 0.0)) for row in residues]
    ys = [float(row.get("y", 0.0)) for row in residues]
    zs = [float(row.get("z", 0.0)) for row in residues]
    plddts = [float(row.get("bfactor", 0.0)) for row in residues]
    count = max(1, len(residues))
    center = (sum(xs) / count, sum(ys) / count, sum(zs) / count)
    basis = "|".join([candidate_id, "".join(aas), ",".join(str(row.get("resid", "")) for row in residues)])
    return {
        "center_x": fmt(center[0]),
        "center_y": fmt(center[1]),
        "center_z": fmt(center[2]),
        "radius_angstrom": fmt(radius),
        "residue_count": len(residues),
        "mean_bfactor_or_plddt": fmt(sum(plddts) / count),
        "hydrophobic_count": sum(1 for aa in aas if aa in HYDRO),
        "aromatic_count": sum(1 for aa in aas if aa in AROM),
        "positive_count": sum(1 for aa in aas if aa in POS),
        "negative_count": sum(1 for aa in aas if aa in NEG),
        "polar_count": sum(1 for aa in aas if aa in POLAR),
        "metal_binding_residue_count": sum(1 for aa in aas if aa in {"C", "H", "D", "E"}),
        "cys_count": aas.count("C"),
        "his_count": aas.count("H"),
        "asp_glu_count": sum(1 for aa in aas if aa in {"D", "E"}),
        "asn_gln_count": sum(1 for aa in aas if aa in {"N", "Q"}),
        "ser_thr_tyr_count": sum(1 for aa in aas if aa in {"S", "T", "Y"}),
        "charge_balance": sum(1 for aa in aas if aa in POS) - sum(1 for aa in aas if aa in NEG),
        "hydrophobic_fraction": fmt(sum(1 for aa in aas if aa in HYDRO) / count),
        "aromatic_fraction": fmt(sum(1 for aa in aas if aa in AROM) / count),
        "distance_histogram_bins": "NA",
        "principal_axis_eigenvalues": "NA",
        "local_ca_graph_edges": "NA",
        "patch_descriptor_sha256": sha256_text(basis),
    }


def parse_float(value: str, default: float = 0.0) -> float:
    try:
        if value in {"", "NA", "nan"}:
            return default
        return float(value)
    except ValueError:
        return default


def normalized_hash_file(path: Path, key_columns: list[str], float_precision: int = 6) -> str:
    if path.suffix.lower() == ".md":
        lines = []
        for line in path.read_text(encoding="utf-8-sig", errors="replace").splitlines():
            if "generated_at" in line.lower() or "started_at" in line.lower():
                continue
            lines.append(re.sub(r"[A-Za-z]:\\[^ )]+", "<ABS_PATH>", line))
        return sha256_text("\n".join(lines))
    rows = read_tsv(path)
    normalized = []
    for row in rows:
        clean = {}
        for key, value in row.items():
            if re.match(r"^-?\d+\.\d+$", str(value)):
                clean[key] = fmt(float(value), float_precision)
            elif isinstance(value, str):
                clean[key] = re.sub(r"[A-Za-z]:\\[^\t]+", "<ABS_PATH>", value)
            else:
                clean[key] = value
        normalized.append(clean)
    normalized.sort(key=lambda r: tuple(str(r.get(col, "")) for col in key_columns))
    return sha256_text(json.dumps(normalized, sort_keys=True, ensure_ascii=True))
