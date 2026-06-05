# plan.md — GenoLigand-Search V1.6 构建计划

## 0. V1.6 总目标

在 V1.5 基础上解决两个问题：

1. pLM（蛋白质语言模型）embedding 的隐形吞吐成本。
2. 3D patch realization 的几何容差标定风险。

V1.6 的 MVP 仍只要求 L0-L2 端到端闭环；L3 短程 MD、L4 ABFE/FEP、L5 QM/MM 均不阻塞 MVP。

## 1. 总原则

1. Rust 负责核心数据流、统计、搜索、几何和候选追踪。
2. Nextflow 负责跨节点编排。
3. Python/C++ 科学工具只能通过持久化 worker 调用。
4. L1 全量候选不能直接调用 pLM。
5. L1/L1.25 候选不能直接折叠或 docking。
6. L2 docking 只能处理 `PatchRealizationResult.pass_l2 = true` 的候选。
7. 几何容差必须由 fixture calibration 产生。
8. 不允许 magic constants。

## 2. Phase 0 — Schema 与契约冻结

交付物：

- `schemas/short_circuit_gate.schema.json`
- `schemas/tolerance_profile.schema.json`
- `schemas/geometry_budget_report.schema.json`
- 更新 `schemas/patch_realization.schema.json`
- `docs/pipeline_contract.md`

验收：

- `gls-plm-gate` 只能接受 `pass_plm=true`。
- `gls-patch-realizer` 必须要求 `ToleranceProfile`。
- Schema 有正例、反例和版本字段。

## 3. Phase 1 — Rust Workspace 与 CI

交付物：

- Cargo workspace
- `gls-types`
- `gls-artifacts`
- `gls-cli`
- CI pipeline

验收：

- `cargo fmt --check` 通过。
- `cargo clippy --workspace -- -D warnings` 通过。
- `cargo test --workspace` 通过。

## 4. Phase 2 — L0 Mock Ligand Kernel

交付物：

- `gls-ligand`
- mock `InteractionConstraintGraph`
- mock `FragmentGrammar`

验收：

- 给定 ligand YAML，可生成稳定 ICG 与 fragment grammar。
- 输出带 provenance 和 hash。

## 5. Phase 3 — L1 Genome Search 与统计校正

交付物：

- `gls-genome`
- `gls-fragment`
- `gls-stats`

任务：

- FASTA 分片读取。
- 六框翻译。
- 精确片段和类别片段匹配。
- 背景概率、P 值、q 值、FDR。
- 输出 `FragmentHit` 与 `OrfCandidate`。

验收：

- 高频低信息片段被惩罚。
- 多片段共现显著性可复现。
- 反向互补、阅读框和 stop-to-stop 区间有 property tests。

## 6. Phase 4 — L1.20 Short-Circuit Gate

交付物：

- `gls-shortcircuit`
- `ShortCircuitGateResult`
- `plm_budget.yaml`

任务：

1. 实现长度、stop-to-stop、非法字符规则。
2. 实现低复杂度、重复序列、氨基酸熵评分。
3. 接入 fragment information 与 co-occurrence q value。
4. 实现 sequence hash 去重。
5. 实现 MinHash/SimHash 近似聚类。
6. 实现 per-shard、per-family、per-locus-class top-K。
7. 输出 `pass_plm` 与机器可读失败原因。

验收：

- 10^5 级候选可在 CPU 上完成短路筛选。
- pLM 调用候选数低于配置预算。
- duplicate sequence 不重复进入 pLM。
- 暗基因组候选不会因无注释被直接删除。

## 7. Phase 5 — L1.25 pLM Gate

交付物：

- `gls-plm-gate`
- `workers/plm-worker`
- embedding cache

任务：

- 只读取 `pass_plm=true` 候选。
- 实现 length bucketing。
- 实现 persistent worker。
- 实现 cache key：`sequence_hash + model_id + tokenizer_id + weights_hash`。
- 输出 `FoldabilityGateResult`。

验收：

- pLM worker 不对 rejected/hold 候选运行。
- 同一序列多次出现只计算一次 embedding。
- 达到预算上限时系统稳定失败并输出 reason。

## 8. Phase 6 — Fixture 管理

交付物：

- `gls-fixtures`
- fixture manifest
- positive/hard-negative split

任务：

- 从 PDBbind、Binding MOAD 或内部 curated 集合导入已知复合物。
- 生成 positive holo、positive perturbed、hard negative、pocket decoy fixture。
- 保存 ligand、pocket、interaction labels、resolution、interaction kind。

验收：

- 每个 fixture 有 hash、来源、license note 和 split 标签。
- hard negative 与 positive 不得泄漏。

## 9. Phase 7 — Tolerance Calibration

交付物：

- `gls-tolerance-calib`
- `ToleranceProfile`
- calibration report

任务：

- 针对不同 interaction kind 网格或贝叶斯搜索容差。
- 记录 positive recall、hard-negative rejection、bucket occupancy、peak memory、runtime。
- 输出 narrow、standard、relaxed 三档 ToleranceProfile。

验收：

- 没有 ToleranceProfile 时 patch-realizer 必须拒绝运行。
- fixture positive recall 达到设定阈值。
- hard-negative false positive rate 低于阈值。
- bucket occupancy_p95 低于阈值。

## 10. Phase 8 — L1.5 Structure Realization Gate

交付物：

- `gls-foldgate`
- `workers/fold-worker`
- `workers/pocket-worker`
- `CandidatePatchGraph`

任务：

- 只接收 G2/G3 候选。
- ORF 延伸。
- 结构生成或局部结构生成。
- P2Rank/fpocket 口袋检测。
- 构建 CPG。

验收：

- G0/G1 候选不能进入 L1.5。
- pocket worker 失败时有明确 failure reason。

## 11. Phase 9 — 3D Patch Realizer

交付物：

- `gls-patch-realizer`
- `gls-geometry`
- `GeometryBudgetReport`

任务：

- 读取 ICG、CPG 和 ToleranceProfile。
- 实现 calibrated geometric hashing。
- 实现 RANSAC + Kabsch 对齐。
- 实现向量兼容、SASA、occlusion、pocket overlap 评分。
- 实现 max_bucket_occupancy、max_seed_triplets、memory guard。

验收：

- positive fixture 能恢复已知 patch。
- hard negative same-residue fixture 被拒绝。
- 容差放宽过程可审计。
- 严禁全残基暴力组合。

## 12. Phase 10 — IDP/IDR Trend Branch

交付物：

- `gls-idp-trend`

任务：

- 计算 ensemble hit rate、cluster enrichment、contact consistency。
- 输出 trend label。
- 明确不输出宏观自由能。

验收：

- S3 候选不输出 `ΔG_macro`。
- 报告中标注 “trend only”。

## 13. Phase 11 — L2 Docking MVP

交付物：

- `gls-dock`
- `workers/dock-worker`

任务：

- AutoDock Vina adapter。
- 批量 docking manifest。
- 持久化 worker pool。
- docking result parser。

验收：

- 只处理 `pass_l2 = true` 候选。
- 不为每个候选冷启动容器。

## 14. Phase 12 — 报告与 Nextflow

交付物：

- `gls-report`
- HTML/Markdown report
- Nextflow workflow modules

验收：

- 示例 ligand + genome shard 能输出端到端报告。
- 报告显示 L1、L1.20、L1.25、L1.5、L2 每层候选数量、失败原因和资源消耗。
- `nextflow run workflows/main.nf -profile dev_cpu_smoke` 可运行。
- `-resume` 可复用中间结果。

## 15. Post-MVP

- L3 短程 MD 与 MM/GBSA。
- L4 ABFE/FEP validation pack。
- L5 QM/MM special chemistry pack。
- 原生泛基因组图 motif search。
- Rust/C++ OpenMM FFI spike。
- 真实 xTB/CREST/DFT ligand kernel。
- 严格 IDP 宏观自由能，仅限有可靠 `p_i` 的候选。

## 16. Definition of Done V1.6

1. L0-L2 端到端闭环。
2. L1.20 真实降低 pLM 输入量。
3. pLM embedding cache 可复用。
4. L1.5 不能绕过。
5. ToleranceProfile 由 fixture calibration 生成。
6. patch-realizer 没有 magic tolerance。
7. geometry hash 有内存与桶占用保护。
8. 正负 fixture 测试通过。
9. IDP/IDR 只输出趋势指标。
10. 所有候选有 provenance、artifact hash、参数和工具版本。
