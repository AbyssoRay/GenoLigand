# Pocket Worker Contract

Pocket workers emit `CandidatePatchGraph` or a machine-readable failure reason. A pocket worker failure must not be swallowed by L1.5.

The V1.6 dev worker is executable through:

```text
gls-cli worker-pocket-dev
```

It accepts persistent stdin/stdout `POCKET` requests containing candidate id, structure id, and sequence, then returns node records that the Rust adapter converts into `CandidatePatchGraph`. Production P2Rank/fpocket adapters must preserve the same machine-readable failure and CPG boundary.

Production replacement is configured with `GENOLIGAND_POCKET_WORKER_BIN` and optional `GENOLIGAND_POCKET_WORKER_ARGS`. Invalid commands must fail loudly with a worker spawn/protocol error.

`external_line_worker.py` is a production adapter shim for P2Rank/fpocket-style tools. It expects `GENOLIGAND_POCKET_COMMAND` to start a long-lived line-protocol subprocess that reads `POCKET` lines from stdin and returns one CPG node payload per line. The shim launches that subprocess once and reuses it for the worker lifetime.
