# pLM Worker Contract

The Rust `gls-plm-gate` crate talks to a persistent pLM worker abstraction. The worker must batch by length bucket, expose model id, tokenizer id, and weights hash, acquire a GPU batch semaphore before invocation, and never receive candidates unless `ShortCircuitGateResult.pass_plm = true`.

The V1.6 dev worker is executable through the CLI itself:

```text
gls-cli worker-plm-dev
```

It is a persistent stdin/stdout line worker. Each request is one tab-separated `EMBED` line carrying model id, tokenizer id, weights hash, bucket length, and one or more cache-key/sequence pairs. The response is one `OK` line with semicolon-separated embedding vectors. Production ESM/ESMFold workers must preserve this batching and cache-key contract.

Production replacement is configured with `GENOLIGAND_PLM_WORKER_BIN` and optional `GENOLIGAND_PLM_WORKER_ARGS`. If those variables point to a missing or invalid command, the pipeline fails with a machine-readable worker error instead of falling back to the dev worker.

`external_line_worker.py` is a production adapter shim for ESM/ESMFold-style embedding tools. It expects `GENOLIGAND_PLM_EMBED_COMMAND` to start a long-lived line-protocol subprocess that reads `EMBED` lines from stdin and returns one embedding payload per line. The shim launches that subprocess once and reuses it for the worker lifetime.
