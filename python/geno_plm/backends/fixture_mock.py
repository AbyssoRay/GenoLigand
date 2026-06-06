import hashlib
import struct


def embed_sequence(candidate_id: str, sequence: str, model_id: str, dim: int = 64) -> list[float]:
    seed = f"{candidate_id}\t{model_id}\t{sequence.upper()}".encode()
    output: list[float] = []
    counter = 0
    while len(output) < dim:
        digest = hashlib.sha256(seed + counter.to_bytes(4, "little")).digest()
        for offset in range(0, len(digest), 4):
            value = struct.unpack("<I", digest[offset : offset + 4])[0]
            output.append((value / 0xFFFFFFFF) * 2.0 - 1.0)
            if len(output) == dim:
                break
        counter += 1
    return output
