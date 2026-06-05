# GenoLigand-Search V1.6 Codex 工程架构指导

## 0. V1.6 核心原则

V1.6 的目标是修复两个工程隐患：

1. `pLM embedding` 不能成为新的吞吐瓶颈。
2. `gls-patch-realizer` 的几何容差不能依赖人工常数。

因此新增两个一等模块：

```text
crates/gls-shortcircuit/       # L1.20：pLM 前短路求值门控
crates/gls-fixtures/           # 已知复合物 fixture 管理
crates/gls-tolerance-calib/    # 几何容差离线标定
crates/gls-geometry-bench/     # 几何哈希占用、内存和召回曲线基准测试
```

## 1. 不可违反的规则

- 禁止 L1 全量候选直接调用 pLM worker。
- 禁止 L1 全量候选直接调用 fold-worker。
- 禁止 L1 或 L1.25 候选直接进入 docking。
- 禁止把 pLM embedding 作为默认必算特征。
- 禁止在 `gls-patch-realizer` 中硬编码 `radius_tolerance = 2.0` 这类 magic constant。
- 禁止未经过 fixture calibration 的 `ToleranceProfile` 进入生产运行。
- 禁止几何哈希桶无限膨胀；必须有 bucket occupancy 和 memory guard。

## 2. V1.6 数据流

```text
L0 ligand kernel
  -> FragmentGrammar + InteractionConstraintGraph draft
L1 genome search
  -> FragmentHit + OrfCandidate
L1.20 gls-shortcircuit
  -> ShortCircuitGateResult
L1.25 gls-plm-gate
  -> FoldabilityGateResult
L1.5 gls-foldgate + gls-patch-realizer
  -> StructureGateResult + PatchRealizationResult
L2 gls-dock
  -> DockingResult
```

`gls-plm-gate` 只能读取 `ShortCircuitGateResult.pass_plm = true` 的候选。

## 3. L1.20 Short-Circuit Gate

### 3.1 目标

`gls-shortcircuit` 的目标是用极低成本规则把 L1 输出压缩到 pLM 可承受范围。它不是最终科学证据，只是预算控制和候选清洗模块。

### 3.2 分层惰性求值

必须按以下顺序执行。后续层只有在前一层无法决策时才运行：

```text
Layer A: length / stop-to-stop ORF / invalid residues
Layer B: low complexity / repeat / amino-acid entropy
Layer C: fragment information content / FDR / co-occurrence significance
Layer D: exact sequence deduplication / MinHash or SimHash clustering
Layer E: per-shard / per-fragment-family / per-locus-class budget
Layer F: lightweight context priority, e.g. RNA-seq, ATAC-seq, Ribo-seq if provided
Layer G: pLM invocation candidate list
```

### 3.3 数据模型

```rust
pub enum ShortCircuitDecision {
    Reject,
    HoldNoPlm,
    PassPlm,
    ForceKeepDarkCandidate,
}

pub struct ShortCircuitGateResult {
    pub candidate_id: CandidateId,
    pub decision: ShortCircuitDecision,
    pub rule_version: String,
    pub sequence_hash: String,
    pub canonical_cluster_id: Option<String>,
    pub aa_len: usize,
    pub low_complexity_score: f64,
    pub entropy_score: f64,
    pub fragment_info_score: f64,
    pub cooccur_q_value: Option<f64>,
    pub locus_class: LocusClass,
    pub context_priority_score: f64,
    pub budget_rank: u32,
    pub pass_plm: bool,
    pub failure_reason: Option<String>,
}
```

### 3.4 pLM 调用预算

```yaml
plm_budget:
  dev_cpu_smoke:
    max_plm_candidates_per_shard: 0
  dev_gpu_lowmem:
    max_plm_candidates_per_shard: 32
    max_sequence_length: 256
  workstation:
    max_plm_candidates_per_shard: 512
    max_sequence_length: 768
  hpc:
    max_plm_candidates_per_shard: configurable
```

必须实现：

- `sequence_hash + model_id + tokenizer_id + weights_hash` 作为 embedding cache key。
- length bucketing，避免 batch padding 浪费。
- duplicate sequence 只调用一次 pLM。
- cluster representative 先算，cluster member 继承或降级。
- pLM worker 必须支持 persistent process，不得每个候选冷启动。

## 4. Fixture-Calibrated Geometry Tolerances

### 4.1 原则

`radius_tolerance`、`angle_tolerance_deg`、`bucket_size` 不得全局固定。必须通过 fixture 反向标定，生成版本化 `ToleranceProfile`。

### 4.2 Fixture 类型

```rust
pub enum FixtureType {
    PositiveHolo,
    PositivePerturbed,
    HardNegativeSameResidues,
    PocketDecoy,
    PredictedStructureNoise,
}
```

fixture 来源可包括 PDBbind、Binding MOAD、BioLiP2、内部 curated complexes。

### 4.3 ToleranceProfile

```rust
pub struct ToleranceProfile {
    pub profile_id: String,
    pub dataset_id: String,
    pub created_at: String,
    pub interaction_kind: InteractionKind,
    pub ligand_class: String,
    pub structure_confidence_bin: String,
    pub radius_tolerance: f64,
    pub angle_tolerance_deg: f64,
    pub distance_bin_width: f64,
    pub vector_angle_bin_width: f64,
    pub max_bucket_occupancy: usize,
    pub max_seed_triplets: usize,
    pub recall_at_profile: f64,
    pub hard_negative_rejection: f64,
    pub bucket_occupancy_p95: f64,
    pub peak_memory_mb: f64,
    pub runtime_ms_per_fixture: f64,
}
```

### 4.4 Calibration Objective

`gls-tolerance-calib` 的目标不是最大化召回，而是在以下约束下优化 fixture recall：

```text
maximize positive_fixture_recall
subject to:
  hard_negative_false_positive_rate <= threshold
  bucket_occupancy_p95 <= threshold
  peak_memory_mb <= threshold
  runtime_ms_per_fixture <= threshold
```

### 4.5 Runtime Coarse-to-Fine Policy

运行时不能一开始使用最大容差。必须采用：

```text
narrow profile -> standard profile -> relaxed profile
```

只有当候选优先级高且未找到 match 时，才允许进入下一档。每次放宽必须检查：

- bucket occupancy
- seed triplet count
- memory estimate
- hard timeout

## 5. gls-patch-realizer 更新要求

`gls-patch-realizer` 必须从 `ToleranceProfile` 读取容差，而不是直接从配置文件读取裸数值。

核心流程：

1. Load ICG and CPG.
2. Load ToleranceProfile by interaction kind and candidate confidence bin.
3. Build seed node set.
4. Create geometry hash with calibrated bin widths.
5. Reject buckets exceeding `max_bucket_occupancy`.
6. Run RANSAC + Kabsch only on bounded candidates.
7. Score point RMSD, vector angle, pocket overlap, occlusion, SASA, rotamer feasibility.
8. Emit `PatchRealizationResult` and `GeometryBudgetReport`.

## 6. Testing Requirements

新增 golden tests：

- pLM 不会被低复杂度候选调用。
- duplicate sequence 只产生一次 pLM embedding。
- pLM budget 达到上限时仍稳定输出 failure reason。
- ToleranceProfile 缺失时 patch-realizer 必须失败，而不是使用默认值。
- positive fixture 能恢复已知 patch。
- hard negative same-residue fixture 被拒绝。
- 桶占用超过阈值时必须触发 memory guard。
- 容差从 narrow 放宽到 relaxed 的过程必须可审计。

## 7. Definition of Done V1.6

MVP 只有在以下条件满足时才算完成：

1. L1.20 使 pLM 调用量低于预算。
2. pLM embedding cache 可复用且带模型版本。
3. gls-patch-realizer 不含 magic tolerance。
4. ToleranceProfile 由 fixture calibration 生成。
5. geometry hash 有 occupancy 和 memory guard。
6. L2 docking 只能处理 `pass_l2 = true` 候选。
7. 端到端报告包含每一层候选保留率、淘汰原因和资源统计。
