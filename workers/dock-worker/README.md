# Dock Worker Contract

Dock workers are persistent batch workers. They must only receive `PatchRealizationResult.pass_l2 = true` candidates and must not cold-start once per candidate.
