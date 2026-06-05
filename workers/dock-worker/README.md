# Dock Worker Contract

Dock workers are persistent batch workers. They must only receive batch-manifest entries derived from `PatchRealizationResult.pass_l2 = true` candidates and must not cold-start once per candidate. Vina result output is parsed back into `DockingResult` with a machine-readable failure reason when no score can be found.

The V1.6 dev worker is executable through:

```text
gls-cli worker-dock-dev
```

It accepts persistent stdin/stdout `DOCK` batch requests and returns one score per accepted manifest entry. Production AutoDock Vina adapters must keep the worker pool alive and must never receive candidates whose `PatchRealizationResult.pass_l2` is false.

Production replacement is configured with `GENOLIGAND_DOCK_WORKER_BIN` and optional `GENOLIGAND_DOCK_WORKER_ARGS`. Invalid commands must fail loudly with a worker spawn/protocol error.

`vina_worker.py` is the AutoDock Vina adapter. Set `GENOLIGAND_DOCK_WORKER_BIN` to this script, `GENOLIGAND_VINA_COMMAND` to the Vina executable, and `GENOLIGAND_DOCK_INPUT_DIR` to a directory containing `receptor_<candidate>.pdbqt` and `ligand_<candidate>.pdbqt` files, where unsupported filename characters in candidate ids are replaced by underscores.
