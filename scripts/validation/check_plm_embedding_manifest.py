from geno_plm.cli import main


if __name__ == "__main__":
    raise SystemExit(main(["validate", "--manifest", __import__("sys").argv[1]]))
