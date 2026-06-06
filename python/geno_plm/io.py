import csv
import hashlib
import math
import os
import struct
from pathlib import Path


def read_fasta(path: str | os.PathLike) -> list[dict]:
    records: list[dict] = []
    current_header: str | None = None
    current_sequence: list[str] = []
    with open(path, "r", encoding="utf-8-sig") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            if line.startswith(">"):
                if current_header is not None:
                    records.append(_record(current_header, "".join(current_sequence)))
                current_header = line[1:]
                current_sequence = []
            else:
                current_sequence.append(line)
    if current_header is not None:
        records.append(_record(current_header, "".join(current_sequence)))
    return records


def _record(header: str, sequence: str) -> dict:
    candidate_id = header.split("|", 1)[0]
    return {
        "candidate_id": candidate_id,
        "header": header,
        "sequence": sequence.upper().rstrip("*"),
        "sequence_sha256": hashlib.sha256(sequence.upper().rstrip("*").encode()).hexdigest(),
    }


def write_tsv(path: str | os.PathLike, rows: list[dict], fieldnames: list[str]) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({key: row.get(key, "") for key in fieldnames})


def read_tsv(path: str | os.PathLike) -> list[dict]:
    with open(path, "r", encoding="utf-8-sig", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def sha256_file(path: str | os.PathLike) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def vector_stats(vector: list[float]) -> tuple[float, float]:
    if not vector:
        return 0.0, 0.0
    mean_abs = sum(abs(value) for value in vector) / len(vector)
    l2 = math.sqrt(sum(value * value for value in vector))
    return mean_abs, l2


def write_npy_float32(path: str | os.PathLike, vector: list[float]) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    header = {
        "descr": "<f4",
        "fortran_order": False,
        "shape": (len(vector),),
    }
    header_text = repr(header).replace("False", "False")
    header_bytes = (header_text + " ").encode("latin1")
    padding = 16 - ((10 + len(header_bytes) + 1) % 16)
    header_bytes = header_bytes + (b" " * padding) + b"\n"
    with open(path, "wb") as handle:
        handle.write(b"\x93NUMPY")
        handle.write(bytes([1, 0]))
        handle.write(struct.pack("<H", len(header_bytes)))
        handle.write(header_bytes)
        handle.write(struct.pack("<" + "f" * len(vector), *vector))


def read_npy_float32(path: str | os.PathLike) -> list[float]:
    with open(path, "rb") as handle:
        magic = handle.read(6)
        if magic != b"\x93NUMPY":
            raise ValueError(f"not a npy file: {path}")
        major, minor = handle.read(2)
        if (major, minor) != (1, 0):
            raise ValueError("only npy v1.0 is supported")
        header_len = struct.unpack("<H", handle.read(2))[0]
        header = handle.read(header_len).decode("latin1")
        start = header.index("(") + 1
        end = header.index(",", start)
        length = int(header[start:end].strip())
        payload = handle.read()
    return list(struct.unpack("<" + "f" * length, payload[: 4 * length]))


def all_finite(vector: list[float]) -> bool:
    return all(math.isfinite(value) for value in vector)
