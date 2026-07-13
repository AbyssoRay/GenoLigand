import argparse
import json
from pathlib import Path

from rdkit import Chem


def molecule_from_path(path: Path, fmt: str):
    if fmt in {"sdf", "mol"}:
        supplier = Chem.SDMolSupplier(str(path), removeHs=False)
        return next((mol for mol in supplier if mol is not None), None)
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
        return mol
    raise ValueError(f"unsupported_molecule_format:{fmt}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--path", required=True)
    parser.add_argument("--format", required=True)
    args = parser.parse_args()
    path = Path(args.path)
    mol = molecule_from_path(path, args.format.lower())
    if mol is None:
        raise ValueError("rdkit_structure_parse_failed")
    print(
        json.dumps(
            {
                "canonical_smiles": Chem.MolToSmiles(
                    mol, canonical=True, isomericSmiles=True
                ),
                "atom_count": mol.GetNumAtoms(),
            },
            ensure_ascii=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
