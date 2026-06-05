# Fold Worker Contract

Fold workers are persistent processes used only after L1.25 foldability gating. L1 candidates and G0/G1 candidates are rejected before this worker boundary.

The V1.6 dev worker is executable through:

```text
gls-cli worker-fold-dev
```

It accepts persistent stdin/stdout `FOLD` requests and returns a structure id. Production ColabFold/ESMFold adapters must keep this process alive across candidates and must remain behind the L1.25 and G2/G3 gates.

Production replacement is configured with `GENOLIGAND_FOLD_WORKER_BIN` and optional `GENOLIGAND_FOLD_WORKER_ARGS`. Invalid commands must fail loudly with a worker spawn/protocol error.

`external_line_worker.py` is a production adapter shim for ColabFold/ESMFold-style tools. It expects `GENOLIGAND_FOLD_COMMAND` to start a long-lived line-protocol subprocess that reads `FOLD` lines from stdin and returns one structure id per line. The shim launches that subprocess once and reuses it for the worker lifetime.
