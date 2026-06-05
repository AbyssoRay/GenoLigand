# pLM Worker Contract

The Rust `gls-plm-gate` crate talks to a persistent pLM worker abstraction. The worker must batch by length bucket, expose model id, tokenizer id, and weights hash, and never receive candidates unless `ShortCircuitGateResult.pass_plm = true`.
