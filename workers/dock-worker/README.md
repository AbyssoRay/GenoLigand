# Dock Worker Contract

Dock workers are persistent batch workers. They must only receive batch-manifest entries derived from `PatchRealizationResult.pass_l2 = true` candidates and must not cold-start once per candidate. Vina result output is parsed back into `DockingResult` with a machine-readable failure reason when no score can be found.
