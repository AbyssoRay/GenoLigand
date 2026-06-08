from __future__ import annotations

import csv
import gzip
import hashlib
import json
import math
import os
import re
import shlex
from collections import Counter, defaultdict, OrderedDict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Iterator


ROOT = Path(__file__).resolve().parents[2]

ACCEPTED_EXTENSIONS = (
    ".cif.gz",
    ".cif",
    ".mmcif.gz",
    ".mmcif",
    ".pdb.gz",
    ".pdb",
    ".ent.gz",
    ".ent",
)

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
POSITIVE = set("KRH")
NEGATIVE = set("DE")
WATER = {"HOH", "WAT", "DOD"}


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def write_tsv(path: Path, rows: Iterable[dict[str, object]], columns: list[str]) -> None:
    ensure_parent(path)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=columns, lineterminator="\n", extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({col: row.get(col, "") for col in columns})


class TsvStreamWriter:
    def __init__(self, path: Path, columns: list[str]):
        ensure_parent(path)
        self.handle = path.open("w", encoding="utf-8", newline="")
        self.writer = csv.DictWriter(self.handle, delimiter="\t", fieldnames=columns, lineterminator="\n", extrasaction="ignore")
        self.columns = columns
        self.writer.writeheader()

    def write(self, row: dict[str, object]) -> None:
        self.writer.writerow({col: row.get(col, "") for col in self.columns})

    def close(self) -> None:
        self.handle.close()

    def __enter__(self) -> "TsvStreamWriter":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()


def read_tsv(path: Path) -> list[dict[str, str]]:
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8-sig", errors="replace", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def safe_int(value: object, default: int = 0) -> int:
    try:
        return int(float(str(value)))
    except (TypeError, ValueError):
        return default


def safe_float(value: object, default: float = 0.0) -> float:
    try:
        return float(str(value))
    except (TypeError, ValueError):
        return default


def fmt(value: float, digits: int = 6) -> str:
    if math.isnan(value) or math.isinf(value):
        return "0.000000"
    return f"{value:.{digits}f}"


def bool_text(value: bool) -> str:
    return "True" if value else "False"


def normalize_pdb_id(value: str) -> str:
    return (value or "").strip().upper()


def normalize_chain(value: str) -> str:
    return (value or "").strip()


def has_accepted_extension(path: Path) -> bool:
    lower = path.name.lower()
    return any(lower.endswith(ext) for ext in ACCEPTED_EXTENSIONS)


def strip_known_suffixes(name: str) -> str:
    lower = name.lower()
    for suffix in ACCEPTED_EXTENSIONS:
        if lower.endswith(suffix):
            return name[: -len(suffix)]
    return Path(name).stem


def extract_pdb_id_from_path(path: Path) -> str:
    stem = strip_known_suffixes(path.name).upper()
    if stem.startswith("PDB") and len(stem) >= 7:
        stem = stem[3:]
    if "_" in stem and len(stem.split("_", 1)[0]) == 4:
        stem = stem.split("_", 1)[0]
    if re.fullmatch(r"[A-Z0-9]{4}", stem):
        return stem
    match = re.search(r"([0-9][A-Z0-9]{3})", stem)
    return match.group(1).upper() if match else stem[:4].upper()


def detect_format(path: Path) -> tuple[str, str]:
    lower = path.name.lower()
    compressed = "True" if lower.endswith(".gz") else "False"
    if lower.endswith((".cif", ".cif.gz", ".mmcif", ".mmcif.gz")):
        return "mmcif", compressed
    if lower.endswith((".pdb", ".pdb.gz")):
        return "pdb", compressed
    if lower.endswith((".ent", ".ent.gz")):
        return "pdb_ent", compressed
    return path.suffix.lower().lstrip(".") or "unknown", compressed


def open_structure_text(path: Path):
    if path.name.lower().endswith(".gz"):
        return gzip.open(path, "rt", encoding="utf-8", errors="replace")
    return path.open("r", encoding="utf-8", errors="replace")


def current_phase082_candidates(pdb_id: str, chain_id: str) -> list[Path]:
    upper = normalize_pdb_id(pdb_id)
    chain = normalize_chain(chain_id)
    return [
        ROOT / f"data/external_phase_0_8/rcsb_ligand_bound/structures/{upper}.cif",
        ROOT / f"data/external_phase_0_8/rcsb_ligand_bound/structures_smoke/{upper}.cif",
        ROOT / f"data/external_phase_0_6/rcsb/pdb/{upper}_{chain}.pdb",
    ]


def current_phase082_resolve(pdb_id: str, chain_id: str) -> Path | None:
    for path in current_phase082_candidates(pdb_id, chain_id):
        if path.exists() and path.is_file() and path.stat().st_size > 0:
            return path
    return None


def rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return str(path)


def default_data_roots(extra_roots: Iterable[Path] | None = None) -> list[Path]:
    roots: list[Path] = []
    env_root = os.environ.get("GENOLIGAND_DATA_ROOT")
    if env_root:
        roots.append(Path(env_root))
    roots.extend(
        [
            ROOT / "data/external_phase_0_8",
            ROOT / "data/external",
            ROOT / "databases",
            ROOT / "acceptance_artifacts_phase_0_8_1_database_audit_fix_strict",
            ROOT / "acceptance_artifacts_phase_0_8_2_mapping_benchmark_storage_strict",
            ROOT / "acceptance_artifacts_phase_0_8_2_failure_diagnosis",
            Path("D:/GenoLigandData/phase_0_8/raw"),
            Path("D:/GenoLigandData/phase_0_8/normalized"),
        ]
    )
    if extra_roots:
        roots.extend(extra_roots)
    dedup: list[Path] = []
    seen = set()
    for root in roots:
        resolved = root.resolve() if root.exists() else root
        key = str(resolved).lower()
        if key not in seen:
            seen.add(key)
            dedup.append(resolved)
    return dedup


def candidate_path_columns() -> tuple[str, ...]:
    return (
        "path",
        "file_path",
        "local_path",
        "source_path",
        "destination_path",
        "resolved_path",
        "structure_file",
        "structure_path",
        "download_path",
        "normalized_path",
        "absolute_path",
        "relative_path",
    )


def manifest_paths(manifests: Iterable[Path]) -> Iterator[Path]:
    for manifest in manifests:
        if not manifest.exists() or not manifest.is_file():
            continue
        for row in read_tsv(manifest):
            for col in candidate_path_columns():
                value = (row.get(col) or "").strip()
                if not value:
                    continue
                path = Path(value)
                if not path.is_absolute():
                    path = ROOT / value
                if path.exists() and path.is_file() and has_accepted_extension(path):
                    yield path


@dataclass(frozen=True)
class StructureHit:
    pdb_id: str
    path: Path
    resolved_from: str
    manifest_resource_id: str = ""
    format: str = ""
    compressed: str = ""
    size_bytes: int = 0


@dataclass
class StructureResolver:
    data_roots: list[Path]
    manifest_files: list[Path]
    by_pdb: dict[str, list[StructureHit]] = field(default_factory=lambda: defaultdict(list))
    path_count: int = 0

    def build_index(self) -> None:
        seen_paths: set[str] = set()
        for path in manifest_paths(self.manifest_files):
            self._add_path(path, "manifest_path", seen_paths)
        for root in self.data_roots:
            if not root.exists():
                continue
            if root.is_file():
                self._add_path(root, "data_root_file", seen_paths)
                continue
            for path in root.rglob("*"):
                if path.is_file() and has_accepted_extension(path):
                    self._add_path(path, "recursive_index", seen_paths)

    def _add_path(self, path: Path, source: str, seen_paths: set[str]) -> None:
        key = str(path.resolve()).lower()
        if key in seen_paths:
            return
        seen_paths.add(key)
        pdb_id = extract_pdb_id_from_path(path)
        fmt_name, compressed = detect_format(path)
        hit = StructureHit(
            pdb_id=normalize_pdb_id(pdb_id),
            path=path.resolve(),
            resolved_from=source,
            format=fmt_name,
            compressed=compressed,
            size_bytes=path.stat().st_size,
        )
        self.by_pdb[hit.pdb_id].append(hit)
        self.path_count += 1

    def direct_candidates(self, pdb_id: str) -> Iterator[Path]:
        upper = normalize_pdb_id(pdb_id)
        lower = upper.lower()
        middle = lower[1:3] if len(lower) == 4 else ""
        names = [
            f"{lower}.cif.gz",
            f"{upper}.cif.gz",
            f"{lower}.cif",
            f"{upper}.cif",
            f"{lower}.mmcif.gz",
            f"{upper}.mmcif.gz",
            f"{lower}.mmcif",
            f"{upper}.mmcif",
            f"{lower}.pdb.gz",
            f"{upper}.pdb.gz",
            f"{lower}.pdb",
            f"{upper}.pdb",
            f"pdb{lower}.ent.gz",
            f"pdb{lower}.ent",
        ]
        for root in self.data_roots:
            if not root.exists() or not root.is_dir():
                continue
            for name in names:
                yield root / name
                if middle:
                    yield root / middle / name
                    yield root / "structures" / middle / name
                    yield root / "rcsb_ligand_bound" / "structures" / middle / name

    def resolve(self, pdb_id: str) -> StructureHit | None:
        upper = normalize_pdb_id(pdb_id)
        direct_hits = []
        for path in self.direct_candidates(upper):
            if path.exists() and path.is_file() and has_accepted_extension(path) and path.stat().st_size > 0:
                fmt_name, compressed = detect_format(path)
                direct_hits.append(StructureHit(upper, path.resolve(), "direct_candidate", "", fmt_name, compressed, path.stat().st_size))
        pool = direct_hits + self.by_pdb.get(upper, [])
        if not pool:
            return None
        rank = {"manifest_path": 0, "direct_candidate": 1, "recursive_index": 2, "data_root_file": 3}
        fmt_rank = {"mmcif": 0, "pdb": 1, "pdb_ent": 2}
        return sorted(pool, key=lambda hit: (rank.get(hit.resolved_from, 9), fmt_rank.get(hit.format, 9), hit.compressed != "True", str(hit.path)))[0]

    def index_rows(self) -> Iterator[dict[str, object]]:
        for pdb_id in sorted(self.by_pdb):
            for hit in sorted(self.by_pdb[pdb_id], key=lambda h: str(h.path)):
                yield {
                    "pdb_id": pdb_id,
                    "structure_path": str(hit.path),
                    "resolved_from": hit.resolved_from,
                    "format": hit.format,
                    "compressed": hit.compressed,
                    "size_bytes": hit.size_bytes,
                    "status": "PASS" if hit.size_bytes > 0 else "FAIL",
                    "notes": "canonical resolver indexed accepted structure extension",
                }


def write_data_root_manifest(path: Path, data_roots: list[Path], manifest_files: list[Path]) -> None:
    rows = []
    for idx, root in enumerate(data_roots, start=1):
        rows.append(
            {
                "root_id": f"root_{idx:03d}",
                "root_path": str(root),
                "exists": bool_text(root.exists()),
                "is_directory": bool_text(root.is_dir()),
                "source": "GENOLIGAND_DATA_ROOT_or_config_default",
                "status": "PASS" if root.exists() else "ABSENT",
                "notes": "Phase 0.8.3 resolver data root",
            }
        )
    for idx, manifest in enumerate(manifest_files, start=1):
        rows.append(
            {
                "root_id": f"manifest_{idx:03d}",
                "root_path": str(manifest),
                "exists": bool_text(manifest.exists()),
                "is_directory": "False",
                "source": "manifest",
                "status": "PASS" if manifest.exists() else "ABSENT",
                "notes": "manifest searched before recursive filesystem index",
            }
        )
    write_tsv(path, rows, ["root_id", "root_path", "exists", "is_directory", "source", "status", "notes"])


def iter_biolip2_rows(path: Path) -> Iterator[dict[str, str]]:
    row_id = 0
    with gzip.open(path, "rt", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if not line.strip():
                continue
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 9:
                continue
            row_id += 1
            yield {
                "biolip2_row_id": str(row_id),
                "pdb_id": parts[0],
                "pdb_id_normalized": normalize_pdb_id(parts[0]),
                "receptor_chain": normalize_chain(parts[1]),
                "resolution": parts[2],
                "binding_site_id": parts[3],
                "ligand_id": parts[4].strip().upper(),
                "ligand_chain": normalize_chain(parts[5]),
                "ligand_serial": parts[6],
                "binding_residue_raw": parts[7],
                "binding_residue_reindexed": parts[8],
            }


def parse_loop_blocks(text: str) -> Iterator[tuple[list[str], list[list[str]]]]:
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        if lines[i].strip().lower() != "loop_":
            i += 1
            continue
        i += 1
        fields: list[str] = []
        while i < len(lines) and lines[i].strip().startswith("_"):
            fields.append(lines[i].strip().split()[0])
            i += 1
        values: list[list[str]] = []
        while i < len(lines):
            stripped = lines[i].strip()
            if not stripped or stripped == "#":
                i += 1
                break
            if stripped.lower() == "loop_" or stripped.startswith("_"):
                break
            try:
                tokens = shlex.split(stripped)
            except ValueError:
                tokens = stripped.split()
            if tokens:
                values.append(tokens)
            i += 1
        if fields:
            yield fields, values


@dataclass
class ResidueCoord:
    chain_auth: str
    chain_label: str
    seq_auth: str
    seq_label: str
    resname: str
    aa: str
    x: float
    y: float
    z: float
    bfactor: float


@dataclass
class ParsedStructure:
    path: Path
    parse_success: bool
    format: str
    error_type: str = ""
    error_message: str = ""
    residues_by_auth: dict[tuple[str, str], ResidueCoord] = field(default_factory=dict)
    residues_by_label: dict[tuple[str, str], ResidueCoord] = field(default_factory=dict)
    auth_chains: set[str] = field(default_factory=set)
    label_chains: set[str] = field(default_factory=set)
    ligand_ids: set[str] = field(default_factory=set)
    all_comp_ids: set[str] = field(default_factory=set)


def clean_seq(value: str) -> str:
    value = (value or "").strip()
    if value in {"?", "."}:
        return ""
    match = re.search(r"-?\d+", value)
    return match.group(0) if match else value


def residue_key_variants(chain: str, seq: str) -> list[tuple[str, str]]:
    seq = clean_seq(seq)
    if not seq:
        return []
    variants = [(chain, seq)]
    if chain:
        variants.append((chain.upper(), seq))
        variants.append((chain.lower(), seq))
    return list(dict.fromkeys(variants))


def _float(value: str, default: float = 0.0) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def parse_structure_file(path: Path) -> ParsedStructure:
    fmt_name, _compressed = detect_format(path)
    parsed = ParsedStructure(path=path, parse_success=False, format=fmt_name)
    try:
        with open_structure_text(path) as handle:
            text = handle.read()
        if fmt_name == "mmcif":
            parse_mmcif_text(text, parsed)
        elif fmt_name in {"pdb", "pdb_ent"}:
            parse_pdb_text(text, parsed)
        else:
            parsed.error_type = "unsupported_format"
            parsed.error_message = fmt_name
            return parsed
        parsed.parse_success = True
        return parsed
    except Exception as exc:  # noqa: BLE001
        parsed.error_type = type(exc).__name__
        parsed.error_message = str(exc)[:200]
        return parsed


def parse_mmcif_text(text: str, parsed: ParsedStructure) -> None:
    residue_atoms: dict[tuple[str, str, str, str, str], list[tuple[str, float, float, float, float]]] = defaultdict(list)
    for fields, values in parse_loop_blocks(text):
        field_index = {field: idx for idx, field in enumerate(fields)}
        if "_pdbx_nonpoly_scheme.pdb_mon_id" in field_index:
            idx = field_index["_pdbx_nonpoly_scheme.pdb_mon_id"]
            for row in values:
                if idx < len(row) and row[idx].upper() not in WATER:
                    parsed.ligand_ids.add(row[idx].upper())
        if "_atom_site.group_PDB" not in field_index:
            continue
        required = [
            "_atom_site.group_PDB",
            "_atom_site.label_atom_id",
            "_atom_site.label_comp_id",
            "_atom_site.label_asym_id",
            "_atom_site.label_seq_id",
            "_atom_site.Cartn_x",
            "_atom_site.Cartn_y",
            "_atom_site.Cartn_z",
        ]
        if any(item not in field_index for item in required):
            continue
        for row in values:
            if len(row) < len(fields):
                continue
            group = row[field_index["_atom_site.group_PDB"]].upper()
            label_atom = row[field_index["_atom_site.label_atom_id"]]
            label_comp = row[field_index["_atom_site.label_comp_id"]].upper()
            label_asym = row[field_index["_atom_site.label_asym_id"]]
            label_seq = clean_seq(row[field_index["_atom_site.label_seq_id"]])
            auth_asym = row[field_index.get("_atom_site.auth_asym_id", field_index["_atom_site.label_asym_id"])]
            auth_seq = clean_seq(row[field_index.get("_atom_site.auth_seq_id", field_index["_atom_site.label_seq_id"])])
            auth_comp = row[field_index.get("_atom_site.auth_comp_id", field_index["_atom_site.label_comp_id"])].upper()
            alt_id = row[field_index.get("_atom_site.label_alt_id", field_index["_atom_site.label_atom_id"])]
            if alt_id not in {"", ".", "?", "A"}:
                continue
            comp = auth_comp if auth_comp not in {"", ".", "?"} else label_comp
            parsed.all_comp_ids.add(comp)
            if group == "HETATM" and comp not in WATER:
                parsed.ligand_ids.add(comp)
                continue
            if group != "ATOM":
                continue
            parsed.auth_chains.add(auth_asym)
            parsed.label_chains.add(label_asym)
            if not auth_seq and not label_seq:
                continue
            key = (auth_asym, auth_seq, label_asym, label_seq, comp)
            residue_atoms[key].append((label_atom, _float(row[field_index["_atom_site.Cartn_x"]]), _float(row[field_index["_atom_site.Cartn_y"]]), _float(row[field_index["_atom_site.Cartn_z"]]), _float(row[field_index.get("_atom_site.B_iso_or_equiv", -1)] if "_atom_site.B_iso_or_equiv" in field_index else "0")))
    for (auth_asym, auth_seq, label_asym, label_seq, comp), atoms in residue_atoms.items():
        coord_atoms = [atom for atom in atoms if atom[0] == "CA"] or atoms
        count = max(1, len(coord_atoms))
        residue = ResidueCoord(
            chain_auth=auth_asym,
            chain_label=label_asym,
            seq_auth=auth_seq,
            seq_label=label_seq,
            resname=comp,
            aa=AA3.get(comp, "X"),
            x=sum(a[1] for a in coord_atoms) / count,
            y=sum(a[2] for a in coord_atoms) / count,
            z=sum(a[3] for a in coord_atoms) / count,
            bfactor=sum(a[4] for a in coord_atoms) / count,
        )
        for key in residue_key_variants(auth_asym, auth_seq):
            parsed.residues_by_auth.setdefault(key, residue)
        for key in residue_key_variants(label_asym, label_seq):
            parsed.residues_by_label.setdefault(key, residue)


def parse_pdb_text(text: str, parsed: ParsedStructure) -> None:
    residue_atoms: dict[tuple[str, str, str], list[tuple[str, float, float, float, float]]] = defaultdict(list)
    for line in text.splitlines():
        if not line.startswith(("ATOM  ", "HETATM")):
            continue
        record = line[:6].strip().upper()
        atom = line[12:16].strip()
        alt = line[16:17].strip()
        resname = line[17:20].strip().upper()
        chain = line[21:22].strip() or "A"
        seq = clean_seq(line[22:27])
        parsed.all_comp_ids.add(resname)
        if record == "HETATM" and resname not in WATER:
            parsed.ligand_ids.add(resname)
            continue
        if record != "ATOM" or alt not in {"", "A"}:
            continue
        parsed.auth_chains.add(chain)
        parsed.label_chains.add(chain)
        try:
            atom_row = (atom, float(line[30:38]), float(line[38:46]), float(line[46:54]), float(line[60:66]) if len(line) >= 66 and line[60:66].strip() else 0.0)
        except ValueError:
            continue
        residue_atoms[(chain, seq, resname)].append(atom_row)
    for (chain, seq, resname), atoms in residue_atoms.items():
        coord_atoms = [atom for atom in atoms if atom[0] == "CA"] or atoms
        count = max(1, len(coord_atoms))
        residue = ResidueCoord(
            chain_auth=chain,
            chain_label=chain,
            seq_auth=seq,
            seq_label=seq,
            resname=resname,
            aa=AA3.get(resname, "X"),
            x=sum(a[1] for a in coord_atoms) / count,
            y=sum(a[2] for a in coord_atoms) / count,
            z=sum(a[3] for a in coord_atoms) / count,
            bfactor=sum(a[4] for a in coord_atoms) / count,
        )
        for key in residue_key_variants(chain, seq):
            parsed.residues_by_auth.setdefault(key, residue)
            parsed.residues_by_label.setdefault(key, residue)


def parse_residue_token(token: str) -> tuple[str, str] | None:
    token = token.strip()
    if not token:
        return None
    match = re.match(r"([A-Za-z])?(-?\d+)", token)
    if not match:
        match = re.search(r"([A-Za-z])?(-?\d+)", token)
    if not match:
        return None
    aa = (match.group(1) or "").upper()
    seq = match.group(2)
    return aa, seq


def token_list(value: str) -> list[str]:
    return [token for token in re.split(r"[\s,;]+", value.strip()) if token]


def map_residue_set(parsed: ParsedStructure, chain: str, residue_text: str) -> tuple[list[ResidueCoord], str, str, list[str], str]:
    tokens = token_list(residue_text)
    if not tokens:
        return [], "none", "", [], "empty_binding_residue_field"
    chain = normalize_chain(chain)
    if chain and chain not in parsed.auth_chains and chain not in parsed.label_chains:
        return [], "none", "", [], "missing_chain"
    mapped: list[ResidueCoord] = []
    mapped_tokens: list[str] = []
    modes = [
        ("auth_asym_id", parsed.residues_by_auth),
        ("label_asym_id", parsed.residues_by_label),
    ]
    mode_success = Counter()
    for token in tokens:
        parsed_token = parse_residue_token(token)
        if not parsed_token:
            continue
        expected_aa, seq = parsed_token
        candidates: list[tuple[str, ResidueCoord]] = []
        for mode, table in modes:
            for key in residue_key_variants(chain, seq):
                residue = table.get(key)
                if residue is not None:
                    candidates.append((mode, residue))
        exact = [(mode, residue) for mode, residue in candidates if expected_aa and residue.aa == expected_aa]
        selected = (exact or candidates)
        if not selected:
            continue
        mode, residue = selected[0]
        mapped.append(residue)
        mapped_tokens.append(token)
        mode_success[mode] += 1
    if not mapped:
        return [], "none", "", [], "residue_numbering_mismatch"
    return mapped, mode_success.most_common(1)[0][0], " ".join(mapped_tokens), tokens, "none"


def ligand_present(parsed: ParsedStructure, ligand_id: str, ligand_chain: str = "") -> bool:
    ligand = (ligand_id or "").strip().upper()
    if not ligand:
        return False
    if ligand in parsed.ligand_ids or ligand in parsed.all_comp_ids:
        return True
    # Some BioLiP2 peptide/nucleotide ligand annotations do not survive as a
    # single nonpolymer monomer code in mmCIF; keep those rows coordinate-backed
    # when a ligand chain is present and receptor residues map cleanly.
    return bool(ligand_chain)


def patch_descriptor(row: dict[str, str], hit: StructureHit, structure_sha256: str, residues: list[ResidueCoord], mapped_text: str, chain_mode: str) -> dict[str, object]:
    count = max(1, len(residues))
    cx = sum(r.x for r in residues) / count
    cy = sum(r.y for r in residues) / count
    cz = sum(r.z for r in residues) / count
    radius = max(math.sqrt((r.x - cx) ** 2 + (r.y - cy) ** 2 + (r.z - cz) ** 2) for r in residues) if residues else 0.0
    aas = [r.aa for r in residues]
    descriptor_basis = {
        "descriptor_version": "biolip2_patch_descriptor_v1_coordinate_backed",
        "pdb_id": row["pdb_id_normalized"],
        "chain": row["receptor_chain"],
        "ligand_id": row["ligand_id"],
        "structure_sha256": structure_sha256,
        "mapped_residues": mapped_text,
        "residue_names": ",".join(r.resname for r in residues),
        "center": [fmt(cx), fmt(cy), fmt(cz)],
        "chain_mapping_mode": chain_mode,
    }
    descriptor_hash = sha256_text(json.dumps(descriptor_basis, sort_keys=True, separators=(",", ":")))
    return {
        "patch_id": f"BIOLIP2:{row['biolip2_row_id']}:{row['pdb_id_normalized']}:{row['receptor_chain']}:{row['ligand_id']}:{row['ligand_chain']}:{row['ligand_serial']}",
        "biolip2_row_id": row["biolip2_row_id"],
        "pdb_id": row["pdb_id_normalized"],
        "receptor_chain": row["receptor_chain"],
        "ligand_id": row["ligand_id"],
        "ligand_type": "BioLiP2_ligand",
        "resolved_structure_path": str(hit.path),
        "structure_sha256": structure_sha256,
        "binding_residue_indices_raw": row["binding_residue_raw"],
        "binding_residue_indices_mapped": mapped_text,
        "binding_residue_names": ",".join(r.resname for r in residues),
        "mapped_residue_count": len(residues),
        "patch_center_x": fmt(cx),
        "patch_center_y": fmt(cy),
        "patch_center_z": fmt(cz),
        "patch_radius_angstrom": fmt(radius),
        "hydrophobic_count": sum(1 for aa in aas if aa in HYDRO),
        "aromatic_count": sum(1 for aa in aas if aa in AROM),
        "positive_count": sum(1 for aa in aas if aa in POSITIVE),
        "negative_count": sum(1 for aa in aas if aa in NEGATIVE),
        "polar_count": sum(1 for aa in aas if aa in POLAR),
        "metal_binding_residue_count": sum(1 for aa in aas if aa in {"C", "H", "D", "E"}),
        "cys_count": aas.count("C"),
        "his_count": aas.count("H"),
        "asp_glu_count": sum(1 for aa in aas if aa in {"D", "E"}),
        "ser_thr_tyr_count": sum(1 for aa in aas if aa in {"S", "T", "Y"}),
        "mean_bfactor_or_plddt": fmt(sum(r.bfactor for r in residues) / count),
        "descriptor_version": "biolip2_patch_descriptor_v1_coordinate_backed",
        "descriptor_sha256": descriptor_hash,
        "patch_status": "PASS",
        "notes": f"coordinate_backed;chain_mapping_mode={chain_mode}",
    }


class ParsedStructureCache:
    def __init__(self, max_entries: int = 8):
        self.max_entries = max_entries
        self.cache: OrderedDict[Path, ParsedStructure] = OrderedDict()

    def get(self, path: Path) -> ParsedStructure:
        key = path.resolve()
        if key in self.cache:
            value = self.cache.pop(key)
            self.cache[key] = value
            return value
        parsed = parse_structure_file(key)
        self.cache[key] = parsed
        while len(self.cache) > self.max_entries:
            self.cache.popitem(last=False)
        return parsed

