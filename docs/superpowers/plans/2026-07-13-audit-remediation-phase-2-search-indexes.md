# Audit-44 Phase 2: Search And Index Consistency

## Goal

修复 GL-AUDIT-003 至 010、015、023、024、035、037 至 039，以及 041 的 MMseqs 部分。所有验证在本地完成，GitHub 只保存提交。

## Success Criteria

- 100 aa 完全相同蛋白在投影索引和查询端共享 token 规则并产生确定命中。
- DNA、RNA、反向互补链和多记录 FASTA 分别产生正确 ORF，不跨记录拼接。
- MMseqs 分离蛋白、核酸和翻译蛋白 query DB，使用显式搜索类型和真实 alignment identity。
- MMseqs/Foldseek/MD 缺输入、缺数据库或执行失败时不再记录 PASS；必需数据库逐个记录 outcome。
- MMseqs 命中保留目标数据库 provenance，并按 manifest 分类为 known 或 metagenome。
- PLINDER 使用 PyArrow batch scanner 解析真实 Parquet；坏 PAR1、缺列和空结果不能 ready。
- ChEMBL censored relation 不进入定量活性分；多输入的 target 聚合没有共享 200 行上限。
- CLI 和 server 共享投影索引完整性门禁，`used 5000` 一致 blocked。
- 外部同步完整计划中任一必需对象失败时 source/install 都不是 ready。
- 来源内容摘要参与 MMseqs/Foldseek 索引 freshness；摘要变化强制重建。
- `.dbtype` 为空、缺配套文件或 smoke query 失败时数据库不是 ready。
- 无供应方 SHA 的成功下载固定实际 SHA 和字节数；坏 partial/416 自动隔离并完整重试。
- ZIP/tar/SQLite 处理按 batch/stream 进行，ZIP 全部受支持成员进入索引。

## Task 1: Establish Explicit Worker Outcomes

**Files:**

- `python/genoligand_workers/common.py`
- `python/genoligand_workers/alignment_worker.py`
- `python/genoligand_workers/md_summary_worker.py`
- `crates/gls-science/src/lib.rs`
- `crates/gls-science/src/tests.rs`
- `tests/authenticity/check_authenticity_contracts.py`

**RED:**

- 加入缺 MMseqs DB、Foldseek DB、结构输入和 MD 输入的测试，断言阶段状态不是 PASS。
- 加入一个目标数据库失败、另一个成功的混合测试，断言总阶段 failed 且保留每库 outcome。

**GREEN:**

- worker 输出结构化 `status`, `reason`, `required`, `database_id`, `row_count`。
- Rust 读取 outcome；`not_ready`、`failed` 和缺 outcome 都不能成为 PASS。
- 没有适用输入时明确记录 `not_applicable`；只有满足该阶段合同的 completed 才 PASS。

**Verify:** focused Python contract tests and `cargo test --offline -p gls-science`.

## Task 2: Close Source And Database Readiness

**Files:**

- `scripts/setup/install_complete_scientific.py`
- `tests/install/check_install_contracts.py`

**RED:**

- 空 `.dbtype`、缺目标配套文件、失败 smoke query 均断言 not ready。
- 同步计划缺一个必需对象时断言 source summary 和 install summary 都不是 ready。
- 修改来源 digest 后断言旧 MMseqs/Foldseek index 不可复用。

**GREEN:**

- readiness 校验非空 `.dbtype`、必需配套文件和本地 smoke query。
- sync outcome 以计划内必需对象为闭包，不以目录任意文件或任意 marker 代替。
- index manifest 记录 source digest、文件数、字节数和 builder version；复用时逐项比较。

**Verify:** `python tests/install/check_install_contracts.py` and local fake-tool smoke fixtures.

## Task 3: Close Download Integrity

**Files:**

- `scripts/setup/install_complete_scientific.py`
- `tests/install/check_install_contracts.py`

**RED:**

- 无 catalog SHA 的既有文件与上次成功 manifest 摘要不符时必须重下。
- 损坏 `.part` 触发校验失败后必须隔离并从零重试。
- HTTP 416 不能直接接受 partial；必须完整校验或清理重试。

**GREEN:**

- 成功下载原子发布后记录 observed SHA-256、字节数、来源版本和完成时间。
- 既有文件只在匹配 catalog SHA 或已提交 observed digest 时复用。
- partial 校验失败移动到受控 quarantine 名称，随后执行一次完整下载；重复失败返回 failed。

**Verify:** local HTTP fixture server tests; no remote validation.

## Task 4: Unify Sequence Tokens And ORF Records

**Files:**

- `crates/gls-science/src/lib.rs`
- `crates/gls-science/src/input_conversion.rs`
- `crates/gls-science/src/tests.rs`

**RED:**

- 100 aa identical database/query sequence must match.
- 反向链 DNA ORF、含 U 的 RNA ORF和六 frame fixture 必须翻译正确。
- inline 和 file multi-FASTA 都必须保留每条 record identity，不能跨记录产生 ORF。

**GREEN:**

- 投影索引和查询调用同一个 protein k-mer tokenizer。
- RNA 先 `U -> T`，每条 record 独立执行正反链三 frame 翻译。
- ORF identity 带原 record ID、strand 和 frame。

**Verify:** focused Rust tests and full `gls-science` suite.

## Task 5: Make MMseqs Typed, Provenanced And Reproducible

**Files:**

- `python/genoligand_workers/alignment_worker.py`
- `crates/gls-science/src/lib.rs`
- `crates/gls-science/src/tests.rs`
- `tests/authenticity/check_authenticity_contracts.py`

**RED:**

- mixed protein/nucleotide inputs must create distinct query databases and commands.
- convertalis must fail unless real alignment backtrace columns provide identity.
- UniProt/BioLiP hits must remain known; MGnify hits must remain metagenome.

**GREEN:**

- preserve `canonical_kind`; build protein, nucleotide and translated-protein query FASTA/DB separately.
- use explicit MMseqs DB/search type and `-a`; consume actual `pident`, alignment length and coverage.
- emit stable `target_database_id`, namespace and source class from target manifest.
- Rust routes rows by source class and rejects unknown/missing provenance.

**Verify:** fake `mmseqs` executable captures commands and deterministic convertalis fixtures.

## Task 6: Stream All Archive Members

**Files:**

- `crates/gls-science/src/lib.rs`
- `crates/gls-science/src/tests.rs`

**RED:**

- multi-member ZIP fixture must include rows from every supported member.
- large synthetic ZIP/tar must remain below a batch-derived memory bound.
- ChEMBL archive extraction must not materialize the SQLite member in a Python/Rust byte vector.

**GREEN:**

- archive readers return iterators/readers and visit every supported member in stable name order.
- TSV/FASTA builders flush bounded row batches directly to output.
- SQLite members stream to a unique temporary file, query once, and clean up after use.

**Verify:** focused Rust tests plus process peak-memory assertion on local synthetic fixtures.

## Task 7: Parse Real PLINDER Parquet

**Files:**

- `python/requirements-workers-cpu.txt`
- `python/genoligand_workers/plinder_index_worker.py`
- `scripts/setup/install_complete_scientific.py`
- `crates/gls-science/src/lib.rs`
- `crates/gls-science/src/tests.rs`
- `tests/install/check_install_contracts.py`

**RED:**

- a real small PAR1 fixture produces target/structure rows from Parquet values, not filename placeholders.
- corrupt Parquet, missing semantic columns and zero valid rows all fail readiness.

**GREEN:**

- pin PyArrow in the local worker runtime requirements.
- inspect schema first, resolve supported PLINDER 2024-06 v2 column aliases, then scan in batches of at most 4096 rows.
- publish a canonical TSV atomically with source digest, row count and schema mapping in its manifest.
- Rust consumes only this canonical projection; `.parquet` is never passed to UTF-8 readers.

**Verify:** generated small Parquet fixture and a metadata-only check against the local 1.018 GB PLINDER file.

## Task 8: Correct ChEMBL And BindingDB Aggregation

**Files:**

- `crates/gls-science/src/lib.rs`
- `crates/gls-science/src/tests.rs`

**RED:**

- `> 10 nM` and `< 10 nM` remain evidence but do not receive exact `10 nM` quantitative score.
- two molecule inputs with more than 200 unique targets each retain all unique targets.

**GREEN:**

- quantitative activity scoring accepts normalized `=` only; relation participates in best-hit comparison.
- aggregate and deduplicate per `(input_id, source_namespace, target_id)` without a scientific row cap.
- UI/report pagination remains separate from scientific computation.

**Verify:** focused ChEMBL/BindingDB Rust fixtures.

## Task 9: Share Projection Completeness Gates

**Files:**

- `crates/gls-science/src/lib.rs`
- `crates/gls-server/src/lib.rs`
- `crates/gls-science/src/tests.rs`
- `crates/gls-server/src/lib.rs` tests

**RED:**

- the same `used 5000` store fixture must be blocked by direct CLI pipeline and server preflight.

**GREEN:**

- expose one science-store readiness evaluator from `gls-science`.
- CLI pipeline blockers and server preflight consume the same structured result and reasons.

**Verify:** focused CLI/server tests and `python tests/e2e/check_pipeline_contracts.py`.

## Task 10: Phase Verification And Review

Run locally:

```text
cargo fmt --all -- --check
cargo clippy --offline --workspace --all-targets --all-features -- -D warnings
cargo test --offline --workspace --all-features
python tests/install/check_install_contracts.py
python tests/authenticity/check_authenticity_contracts.py
python tests/architecture/check_repository_contracts.py
python tests/e2e/check_pipeline_contracts.py
python tests/webui/check_static_webui_contracts.py
```

Then run focused local MMseqs/Foldseek/PyArrow fixtures through the installed WSL worker environment, perform an independent read-only review, update the audit ledger, increment the patch version for each commit, and push `codex/fix-audit-44` with `[skip ci]`.
