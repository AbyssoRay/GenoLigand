import argparse
import json
import sys
from pathlib import Path

from rdkit import Chem, RDLogger

RDLogger.DisableLog("rdApp.*")


def canonical_molecule_record(mol, index: int) -> dict[str, object]:
    return {
        "record_id": f"record_{index}",
        "canonical_smiles": Chem.MolToSmiles(
            mol, canonical=True, isomericSmiles=True
        ),
        "atom_count": mol.GetNumAtoms(),
    }


def molecule_records_from_path(path: Path, fmt: str) -> list[dict[str, object]]:
    if fmt == "sdf":
        supplier = Chem.SDMolSupplier(str(path), removeHs=False)
        records = []
        for index, mol in enumerate(supplier, start=1):
            if mol is None:
                raise ValueError(f"rdkit_structure_parse_failed:record_{index}")
            records.append(canonical_molecule_record(mol, index))
        if not records:
            raise ValueError("rdkit_structure_parse_failed:empty_file")
        return records
    if fmt == "mol":
        mol = Chem.MolFromMolFile(str(path), removeHs=False)
        if mol is None:
            raise ValueError("rdkit_structure_parse_failed:record_1")
        return [canonical_molecule_record(mol, 1)]
    if fmt == "mol2":
        mol = Chem.MolFromMol2File(str(path), removeHs=False)
        charge_lines = path.read_text(encoding="utf-8").splitlines()
        if (
            mol is not None
            and "NO_CHARGES" in {line.strip() for line in charge_lines}
            and not any(atom.GetAtomicNum() == 1 for atom in mol.GetAtoms())
        ):
            for atom in mol.GetAtoms():
                atom.SetNoImplicit(False)
                atom.SetNumRadicalElectrons(0)
                atom.SetFormalCharge(0)
            mol.UpdatePropertyCache(strict=False)
            Chem.SanitizeMol(mol)
        if mol is None:
            raise ValueError("rdkit_structure_parse_failed:record_1")
        return [canonical_molecule_record(mol, 1)]
    raise ValueError(f"unsupported_molecule_format:{fmt}")


def structure_summary_from_path(path: Path, fmt: str) -> dict[str, int]:
    from openmm.app import PDBFile, PDBxFile

    try:
        structure = PDBFile(str(path)) if fmt in {"pdb", "ent"} else PDBxFile(str(path))
        atoms = list(structure.topology.atoms())
        chains = list(structure.topology.chains())
        residues = list(structure.topology.residues())
    except Exception as exc:
        raise ValueError(f"structure_parse_failed:{fmt}:{exc}") from exc
    if not atoms:
        raise ValueError(f"structure_parse_failed:{fmt}:zero_atoms")
    return {
        "atom_count": len(atoms),
        "chain_count": len(chains),
        "residue_count": len(residues),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--path", required=True)
    parser.add_argument("--format", required=True)
    args = parser.parse_args()
    path = Path(args.path)
    fmt = args.format.lower()
    if fmt in {"sdf", "mol", "mol2"}:
        result = {"records": molecule_records_from_path(path, fmt)}
    elif fmt in {"pdb", "cif", "mmcif", "ent"}:
        result = {"structure": structure_summary_from_path(path, fmt)}
    else:
        raise ValueError(f"unsupported_input_format:{fmt}")
    print(json.dumps(result, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"input_resolver_error:{exc}", file=sys.stderr)
        raise SystemExit(2) from None
