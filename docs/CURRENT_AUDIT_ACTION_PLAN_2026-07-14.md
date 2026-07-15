# GenoLigand 当前代码与科学仓库全面审查行动计划

审查日期：2026-07-14

代码基线：main，2618fdb55c679c0106256bdf065693882b19320a

源码版本：0.9.72

审查范围：仓库源码、脚本、配置、Schema、测试、WebUI、CLI、Windows 已发布 runtime、GenoLigandPhase08 WSL worker 环境，以及 D:/GenoLigandData/science_store

问题总数：34 项，其中 P0 9 项、P1 21 项、P2 4 项

## 1. 已确定目标与完成定义

本计划以 README、docs/architecture.md 和 2026-07-13 的 44 项修复设计中已经确定的产品目标为准：

1. 用户从小分子、多肽、蛋白质配体、复合物结构、基因组或宏基因组输入出发，经 Input -> Configure -> Check -> Run -> Results -> Export 完成真实本地运行。
2. 每个候选、分数、结构摘要、动力学摘要、证据链和报告条目都能追溯到 project、run、invocation、input、record、database、candidate 和 artifact。
3. 缺少输入、数据库、工具、索引或有效产物时，系统写出准确的 blocked 或 failed 终态并返回非零退出码，不产生完成标记和科学结论。
4. Rust 负责主流程、状态、排序、证据链和报告语义；Python worker 执行受 Schema 约束的科学计算 primitive；WebUI 和 gls-server 通过正式 API 使用同一契约。
5. 科学仓库的 ready 结论同时证明来源闭包、常驻缓存或可验证的按需补全策略、Schema、摘要、构建版本和真实 smoke query；ready 证据必须明确常驻覆盖率，投影 TSV 的存在不能替代持久数据库。
6. 当前源码、发布 runtime、安装状态、数据库清单和用户界面必须属于同一版本并通过真实本地端到端验收。

### 1.1 2026-07-15 RCSB 范围决策

- RCSB 默认安装模式改为 `on-demand`，保留文件系统中 126397 个完整本地坐标，不再为发布门禁下载全部 153757 个结构。
- 旧进度文件记录的 126425 不代表文件系统事实；现场 20 个 `.part` 共 9437184 B 均缺少身份 sidecar，其中 14 个为空文件，不能续传，最终清理时删除。
- 真实任务命中本地缺失的 PDB ID 时，从 RCSB 固定官方地址按需获取；下载必须通过 gzip、条目身份、atom_site、摘要和原子发布校验，并写入当前 run 的取证清单。
- `complete` 模式继续保留，供显式要求全量镜像时使用；它不再是当前发布完成条件。

## 2. 审查与动态验证基线

### 2.1 已通过的静态与单元验证

- cargo fmt --all -- --check：通过。
- cargo clippy --workspace --all-targets -- -D warnings：通过，0 warning。
- cargo test --workspace -- --test-threads=1：gls-cli 6 项、gls-science 171 项、gls-server 103 项，共 280/280 通过。
- 当前 WSL 项目环境执行 Python worker 测试：70/70 通过。
- 修复后 Python/installer 集中门禁 18/18 通过；76 个改动 Python 文件编译通过，130 个唯一 unittest 和 12 个脚本门禁通过。
- repository、production schema、authenticity、clean release、pipeline、installer、Nextflow、WebUI 静态契约检查：通过。

这些结果证明现有测试基线稳定。动态复现同时确认现有测试没有覆盖真实 API 路由、生产 payload、现场旧清单、跨输入候选身份、外部产物注入和真实安装闭包。

### 2.2 已复现的运行时事实

- 当前 0.9.72 gls-server 的 /api/user/health 和 /api/user/projects 返回 200；WebUI 启动时请求 /api/user/inputs/capabilities 得到 404，页面显示“服务未连接”。
- 当前 0.9.72 gls-cli 在科学仓库 blocked 时写出 run_state=blocked，但进程退出码为 0。
- 流水线阶段抛错时进程退出 1，run_context 仍停留在 starting，缺少 run_state.txt 和 failure_reason.txt。
- 两个不同 input 命中同一 ChEMBL target 时被合并成一个候选。
- run 目录外的绝对 docking pose 被计入排名并获得 0.6000 docking score。
- 修改 docking pose 后，MD 绑定校验仍接受该产物。
- 伪造的 MD summary 在缺少 manifest、轨迹和协议时仍生成 OpenMM complex、显式溶剂和 PME 描述。
- 当前 science store 的 MMseqs 清单为旧五列格式；当前 worker 读取后抛出 mmseqs_target_provenance_missing。
- 当前 science store readiness 统计为 full_index=22125、projection_only=5、incomplete=31；install_status.json 和 install_progress.json 仍报告 ready。

### 2.3 存储快照

| 位置 | 实测大小 | 结论 |
|---|---:|---|
| D:/GenoLigandData/science_store | 277.776 GiB | 绝大部分是有效下载源和 MMseqs/Foldseek 持久数据库 |
| science_store/downloads | 89.347 GiB | MGnify、ChEMBL、RCSB、PLINDER、BindingDB、BioLiP2 有效源需保留 |
| science_store/indexes | 188.422 GiB | MGnify MMseqs 182.042 GiB，其余 MMseqs 和 RCSB Foldseek 为运行依赖 |
| repository/target | 审查时 10.120 GiB | 相关服务退出后已清理 |
| repository/work | 审查时 183.050 MiB | 相关服务退出后已清理 |
| repository/.git | 审查前 261.838 MiB，清理后 12.870 MiB | 约 246.283 MiB 不可达对象已通过 Git GC 回收 |
| science_store 中断下载 | 4.770 GiB | 审查确认后已清理 MGnify 与 RCSB 的 .part 残留 |

未发现大于 100 MiB 的同尺寸重复文件。science_store 中约 277.8 GiB 的完整下载源与索引均有 manifest 或运行时读取链路，不能按体积直接删除。

## 3. 执行顺序

1. P0-001 至 P0-008：恢复 WebUI、状态真实性、仓库闭包和当前 runtime。
2. P1-010 至 P1-018：建立 input/candidate/artifact/worker 的统一强契约。
3. P1-019 至 P1-030：修复科学语义、安装可靠性、资源消耗和工作流。
4. P0-009：执行当前版本最终本地端到端验收并发布。
5. P2-031 至 P2-034：完成代码库和科学仓库存储清理。

## 4. P0 发布阻断案例

### GL-CURRENT-001 WebUI 启动依赖的输入能力 API 不存在

**问题与证据**

- webui/app/core/actions.js:7-32 在启动刷新中调用 GET /api/user/inputs/capabilities。
- crates/gls-server/src/lib.rs 的路由表没有该 endpoint。
- app.js 使用 Promise.all 聚合启动请求，一项 404 使整个页面进入“服务未连接”；真实浏览器已复现。
- tests/webui/check_state_and_paging_contracts.py 只验证字符串和 mock 响应，没有启动真实 server 检查路由。

**解决思路**

- 由 gls-server 暴露权威输入能力描述，数据直接来自服务器实际使用的输入验证契约。
- 前端使用该响应生成输入卡片和校验规则。
- 启动刷新把服务健康、项目列表、输入能力分别记录错误，避免单一辅助请求覆盖已成功的健康状态。

**验收标准**

1. GET /api/user/inputs/capabilities 返回 200，并包含五类输入的扩展名、文本能力、文件能力和约束。
2. 每项能力与服务器实际保存/运行校验共用同一数据源。
3. 全新浏览器打开 0.9.72+ 页面显示服务已就绪，输入卡片、项目切换和运行检查可操作。
4. 删除或改名 endpoint 的服务端集成测试失败。
5. 浏览器控制台无启动 404、未处理 Promise rejection 和错误告警。

### GL-CURRENT-002 科学仓库 ready 判定存在多套互相矛盾的口径

**问题与证据**

- scripts/setup/install_complete_scientific.py:4688-4750 已把 projection_only 和 incomplete 写入 index_readiness_manifest.tsv。
- existing_science_indexes_ready 拒绝这些状态，main 随后通过 base_science_indexes_ready 分支伪造成功并跳过真实 prepare-science-store。
- source_requirements.tsv 只检查已有行，只有表头的空清单可通过核心 preflight。
- 安装状态、服务器 setup status、CLI preflight 和报告渲染各自实现 readiness；现场 31 incomplete、5 projection_only，历史状态仍为 ready，报告仍写“科学资源已就绪”。
- 可选替代路径逐项记为 incomplete，导致一个逻辑来源已经有权威实现时仍产生伪 blocker。

**解决思路**

- 建立一个可序列化的 ScienceStoreReadiness evaluator，输入为必需逻辑来源集合、工具、投影、持久 DB、Schema、摘要、builder 和 smoke query。
- 安装器、gls-cli、gls-server、报告和 WebUI 全部消费同一份 evaluator 输出。
- readiness 按逻辑来源聚合：必需来源必须完整；同组可选候选满足一个权威实现后，其余候选记为 not_selected。
- 删除 base_science_indexes_ready 的伪成功分支。

**验收标准**

1. 删除任一必需 source ID、tool ID 或 index ID 时，安装器、CLI、API、报告返回同一个 blocker code。
2. BindingDB、ChEMBL、MGnify 任一大库出现 used_rows=5000 时状态为 projection_only/needs_repair，安装进程非零。
3. 只有表头的 requirement/index manifest 不能通过。
4. 未选中的同组替代路径不产生 blocker。
5. 当前 36 个不完整条目处理完成前，install_status、setup API、CLI 和报告都不显示 ready。
6. readiness 输出包含稳定 Schema 版本、逐来源证据和修复动作。

### GL-CURRENT-003 现场 MMseqs 清单与当前 worker 协议不兼容

**问题与证据**

- 当前 worker 在 python/genoligand_workers/alignment_worker.py:500-537 强制要求 source_namespace、source_class、source_digest、builder_version。
- 现场 indexes/mmseqs/mmseqs_target_databases.tsv 仍是 source_id、source_path、db_path、status、notes 五列。
- 当前 Rust preflight 在 crates/gls-science/src/lib.rs:3155-3197 主要检查文件和 ready 字符串，没有执行 current-schema consumer smoke。
- 当前 worker 读取现场清单确定抛出 mmseqs_target_provenance_missing。

**解决思路**

- 为 MMseqs/Foldseek manifest 增加显式 schema_version。
- 安装 ready 前由当前 worker 真实解析每个清单并执行最小查询。
- 对来源身份、数据库配套文件和 builder 可以证明的旧库执行原子迁移；证明缺失时进入受空间预检保护的重建。

**验收标准**

1. 现场 6 个 ready MMseqs DB 均可由当前 worker 读取。
2. 已知序列和宏基因组短序列分别执行真实 query 并保留 database_id、namespace、source_digest 和 builder_version。
3. 删除任一 provenance 字段时，在 worker 启动前进入 blocked。
4. 迁移或重建失败时 last-good DB 与 manifest 保持可用，staging 自动清除。

### GL-CURRENT-004 RCSB 下载范围、文件清单和 Foldseek 状态错误地报告 ready

**问题与证据**

- 当前推导 scope 为 153757 个 PDB ID，完整 .cif.gz 为 22124 个，仍缺 131633 个。
- ligand_bound_pdb_scope.tsv 只有 4OC1 一行。
- source_file_manifest.tsv 把 25 个 .part 计入 22149 个 ready 文件。
- install_status.json 仍为 ready，Foldseek 状态没有证明使用同一完整 scope 快照。

**解决思路**

- `on-demand` 与 `complete` 使用显式模式字段；scope manifest、下载结果、结构投影、source file manifest 和 Foldseek source digest 使用同一个不可变缓存快照。
- manifest 生成时排除 `.part`、quarantine、零字节和不可解析坐标；受身份保护的 `.part` 作为可续传状态单独记录。
- `on-demand` 模式发布固定官方 URL、缓存路径、单次请求上限和内容验证策略。Foldseek 清单绑定已经索引的坐标子集并报告覆盖率，不能把子集写成全量。
- 实际候选引用缺失 PDB ID 时自动下载、验证并原子写入缓存；`complete` 模式仍要求所有 scope 坐标与 Foldseek 摘要闭合。

**验收标准**

1. scope manifest 行数等于推导 scope 数，且 `cached_ready_count + missing_count = scope_count`。
2. `on-demand` ready 同时给出缓存覆盖率、固定官方 URL、验证策略和按需补全能力；不得出现 full/complete 字样。
3. `.part` 不进入 source_file_manifest；身份匹配的 partial 可续传，身份变化时不得拼接。
4. Foldseek manifest 的 source digest、文件数和字节数与其实际索引子集一致，并通过真实查询；新增按需坐标明确标记索引刷新需求。
5. 删除一个候选所需的本地坐标后运行真实选择路径，会自动补回正确 PDB、写入摘要与来源证据，并达到 ready；错误 gzip、错误条目和 404 不得发布。
6. 显式 `complete` 模式仍要求 `ready_count = scope_count`，缺失时保持 needs_repair。

### GL-CURRENT-005 安装前缺少下载、迁移和重建的磁盘容量预检

**问题与证据**

- 停止 RCSB 批量下载后的现场快照为：D 盘空闲 22.49 GiB，C 盘空闲 127.316 GiB；剩余 scope 改由运行时按需补齐。
- 现有 MGnify MMseqs DB 为 182.042 GiB。
- 旧五列 manifest 会使 alignment_source_manifest_matches 返回 false，随后创建 staging 原子重建。
- external_sync_execution_manifest.tsv 缺少当前代码要求的 plan_digest，下一次安装会重新同步。
- MGnify include pattern 同时匹配 cluster representatives 和全部 mgy_proteins_N 分片；RCSB 同步会面向 153757 条 scope。

**解决思路**

- 在任何下载、解压或 staging 创建前生成 dry-run：下载、复用、迁移、重建、目标卷、scratch 卷、峰值临时空间和安全余量逐项计费。
- 将 MGnify 产品目标固定为 cluster representatives 或全蛋白分片；当前已确定的代表序列路线应收紧 include pattern。
- manifest 迁移先执行真实 DB probe 和来源身份校验，避免有证据的现有 DB 被无条件重建。
- RCSB `on-demand` 模式的剩余 scope 下载预算为 0；ChEMBL 源 SQLite 和 Foldseek 重建使用 C 盘 scratch，D 盘只承担经过校验的最终发布文件。

**验收标准**

1. dry-run 输出每个项目的 action、download_bytes、staging_bytes、reclaimable_bytes、required_peak_bytes 和 available_bytes。
2. 空间不足时在写入第一个字节前终止。
3. 当前 D 盘 22.49 GiB 空闲环境不会启动 182 GiB 以上的 staging build；需要大 scratch 的步骤明确转到 C 盘并单独验算容量。
4. 重建失败后旧 DB 可继续使用，不留下 .rebuild-*、.previous-*、重复 DB 和新增 partial。
5. MGnify 下载集合与已选定的数据库范围完全一致。
6. 跨卷计划分别报告 scratch/target 的 available、staging 和 required peak，不把两个卷错误相加。

### GL-CURRENT-006 发布 runtime 与当前源码版本脱节

**问题与证据**

- 源码和当前构建版本为 0.9.72。
- C:/Users/BeiChen/AppData/Local/GenoLigand/runtime/bin 中已发布 gls-server 仍带 0.9.36 版本，文件时间停留在 2026-07-03。
- 安装器优先发现 bin/tools/bin 或 PATH 中的旧可执行文件；install_ready 在加入 gls_server 证据前已经计算完成。
- 当前源码构建出的 gls-server 与已发布 runtime 没有 commit/hash 绑定。

**解决思路**

- 建立 runtime_binary_manifest，记录 gls-cli、gls-server、worker wrapper 的版本、commit、SHA-256、构建配置和 smoke 结果。
- 当前源码工作区执行安装时构建并发布当前 HEAD；所有 runtime 组件在计算 install_ready 前完成验证。
- server health 返回版本、commit 和 runtime manifest digest，WebUI 显示同一信息。

**验收标准**

1. 缺 gls-server、错误可执行文件、旧版本或 hash 不匹配均阻塞 install_ready。
2. 发布后的 gls-cli、gls-server 版本和 commit 与当前 release 完全一致。
3. /api/user/health 返回与 runtime manifest 相同的版本和 commit。
4. 重启正式启动入口后，进程路径指向已发布的当前 runtime。

### GL-CURRENT-007 blocked 流水线返回成功退出码

**问题与证据**

- crates/gls-science/src/lib.rs:2824-2832 在 blocked 时写状态后返回 Ok(pipeline_blocked...)。
- crates/gls-cli/src/main.rs:8-13 只在 Err 时退出 1。
- 当前现场运行得到 run_state=blocked、failure_reason 和 blocker 清单，同时进程退出码为 0。

**解决思路**

- 主流程返回结构化 StageOutcome/RunOutcome。
- CLI 将 completed 映射为 0，将 blocked 和 failed 映射为稳定的非零退出码。
- gls-server 和 Nextflow 读取同一个 outcome，不通过 stdout 文本猜测状态。

**验收标准**

1. completed 退出 0。
2. blocked 与 failed 均退出非零，并保留可区分的稳定 code。
3. blocked 完整写出 run_state.txt、failure_reason.txt 和修复动作。
4. blocked/failed 都不生成 done marker，Nextflow 不发布完成结果。
5. 测试必须启动真实 gls-cli 进程并断言 OS exit code。

### GL-CURRENT-008 中途阶段错误没有写入 failed 终态

**问题与证据**

- crates/gls-science/src/lib.rs:2821-2857 的各阶段使用 ? 直接返回，成功尾部才写 completed。
- 合法 CCO 加无法解析的小分子动态复现：CLI 退出 1，run_context.state 仍为 starting，缺少 run_state.txt 和 failure_reason.txt。

**解决思路**

- 在 pipeline 总入口捕获所有阶段错误。
- 原子写入 failed、stage、reason、invocation_id 和最后成功阶段，再向调用方返回错误。
- 完成、取消和失败回调采用 invocation compare-and-set，防止旧运行覆盖新状态。

**验收标准**

1. 输入解析、数据库查询、worker、排序、报告任一阶段故障都写 run_state=failed。
2. failure_reason 包含稳定 error code、失败 stage、invocation 和可执行修复动作。
3. failed 运行没有 completed/done marker。
4. UI 不出现永久 starting，刷新后显示同一 failed 详情。
5. 延迟到达的旧回调不能覆盖当前 invocation 状态。

### GL-CURRENT-009 当前版本最终本地端到端验收尚未完成

**问题与证据**

- 0.9.72 静态与单元测试通过，但 WebUI 当前无法完成启动。
- 已发布 runtime 仍为 0.9.36。
- 现场 science store 同时存在 projection_only、incomplete、旧 manifest 和错误 ready。
- 当前版本没有完成真实 onsite search、docking、MD、报告、导出和 Nextflow completed/blocked 的统一验收证据。

**解决思路**

- 将本案例设为前述修复的最终 release gate。
- 使用同一当前 runtime 和同一修复后的 science store，执行五类用户输入、已知/宏基因组搜索、结构匹配、多个候选 docking、选定候选 MD、报告与导出。
- 同时执行浏览器流程、CLI 流程和真实 Nextflow completed/blocked 流程。

**验收标准**

1. 当前 runtime 版本、commit、worker Schema、science store schema 和 manifest digest 一致。
2. 五类输入从 WebUI 保存、检查、运行、结果分页到导出全部完成。
3. 已知库与宏基因组库各有真实查询，所有 hit 保留 database/source/query provenance。
4. docking 和 MD 产物逐候选绑定且摘要可复算。
5. 报告中的每项计算描述都能回溯到当前 invocation 产物。
6. completed、blocked、failed、cancelled 四条状态路径均通过真实进程与 UI 验收。
7. 发布前 clean release、安装、浏览器和 Nextflow 验收全部在本机通过。

## 5. P1 科学正确性、契约与可靠性案例

### GL-CURRENT-010 候选身份丢失 input 与 record，导致跨配体错误合并

**问题与证据**

- crates/gls-science/src/lib.rs:9188-9223 的候选 identity key 只有 namespace + target。
- ChEMBL/BindingDB 临时表主键缺少 input_id 和 record_id。
- 两个不同 input 同时命中 ChEMBL P00918 时，动态结果只有一个候选，并保留其中一个配体的元数据。
- 同一 input 中来自 ChEMBL、BindingDB、UniProt 的同一 UniProt accession 又因原始数据库名不同而无法规范合并。
- docking/MD 按 input_id 要求恰好一个分子，已经接受的 multi-record SDF 无法逐 record 运行。

**解决思路**

- 统一 CandidateIdentity 为 input_id + record_id + canonical_namespace + canonical_target_id。
- canonical_namespace 使用权威 target 身份，不使用来源数据库显示名。
- multi-record 文件在归一化阶段为每条记录生成稳定 record_id，并在后续阶段保持独立。

**验收标准**

1. 两个输入命中同一靶点时生成两个独立候选。
2. 同一输入、同一 record、同一 UniProt accession 的多来源证据合并到一个候选。
3. multi-record SDF 的每条记录都能独立进入 docking/MD。
4. candidate_id 对相同身份稳定，对 input、record 或 canonical target 任一变化均改变。
5. 现有固化跨 input 合并的测试改为正确反例。

### GL-CURRENT-011 run 外部、旧 invocation 或已替换的 docking pose 可进入排名

**问题与证据**

- crates/gls-science/src/lib.rs:6445-6496 只检查 candidate、status、engine 和 pose 文件存在。
- 绝对路径直接接受，没有约束到当前 run，也没有核对 run、invocation、input、candidate、参数或文件摘要。
- 动态复现中，run 目录外的绝对 PDBQT 获得 0.6000 docking score。

**解决思路**

- docking summary 与 manifest 使用 ArtifactProvenance 强契约。
- 所有路径 canonicalize 后必须位于当前 invocation 目录。
- 排名消费前核对 run、invocation、input、record、candidate、receptor、ligand、参数摘要和产物摘要。

**验收标准**

1. 外部绝对路径、上一次 invocation、其他 candidate 和修改后的 pose 全部拒绝。
2. 当前 invocation 的 Vina pose、manifest、参数与 SHA-256 全部匹配后才计分。
3. 拒绝的 docking 结果保留精确诊断且不污染 final score。

### GL-CURRENT-012 docking manifest 没有产物摘要，MD 接受篡改后的输入

**问题与证据**

- python/genoligand_workers/docking_worker.py:238-287 不生成完整 artifacts 列表。
- python/genoligand_workers/md_summary_worker.py:641-696 将缺失 artifacts 视为空列表，并且只在列表有行时校验摘要。
- 动态复现：完成夹具 docking 后改写 tool_outputs/docking_pose.sdf，resolve_md_invocation 仍成功。
- receptor_sha256 同样采用条件校验。

**解决思路**

- docking 完成时为 receptor coordinate/PDBQT、ligand SDF/PDBQT、pose PDBQT/SDF 和 log 写完整、去重、有序的 artifact 清单。
- MD 将精确 artifact 集合、摘要、路径和 binding 字段设为必需条件。
- 在创建 OpenMM context 前完成全部绑定验证。

**验收标准**

1. 任一 docking 产物改动 1 byte，MD 立即失败且不创建 OpenMM context。
2. 缺少、重复、额外、越界 artifact 均失败。
3. 未篡改的真实 Vina 运行通过并产生可复算的摘要。
4. MD 失败时不生成 MD 分数、轨迹和报告描述。

### GL-CURRENT-013 MD ready 判定允许不完整指标和错误候选

**问题与证据**

- crates/gls-science/src/lib.rs:5592-5952 在 worker PASS 时，只要任一指标文件有数据行就可合成 ready summary。
- 缺失指标留空；指标缺 candidate ID 时会填入 current_run。
- 文件级摘要校验没有验证 TSV 内候选身份、数值范围、单位和完整指标集合。

**解决思路**

- 建立 MD semantic validator，强制唯一 bound candidate、完整指标集合、有限数值、单位和 protocol/state 最低步数与帧数。
- 文件摘要验证通过后再验证内容语义。
- 缺指标、候选不符和物理运行不足均返回非 ready outcome。

**验收标准**

1. 只有 RMSD、缺 RMSF/contact、candidate 不符、NaN、Inf、零步和单帧全部失败。
2. 五类必需指标、protocol、state、trajectory 和同一 candidate 全部匹配后才 ready。
3. validator 的输出同时驱动排名、报告和 WebUI，不再各自按文件行数判断。

### GL-CURRENT-014 报告绕过 MD provenance 并生成没有事实依据的计算描述

**问题与证据**

- crates/gls-science/src/lib.rs:7085-7171 通过 md_summary.tsv 数据行数判断 MD 存在，没有调用 MD binding validator。
- manifest 缺失时继续使用默认文件名和固定叙述。
- 只有 foreign_candidate 伪行、没有 protocol/trajectory/manifest 时，报告仍声称执行了 receptor-ligand complex、OpenFF/RDKit、OpenMM、显式溶剂和 PME。

**解决思路**

- 报告和 evidence ledger 只消费统一 validator 输出的 VerifiedMdFacts。
- 每条自然语言描述由实际 method、platform、system、protocol、trajectory 和 artifact 字段生成。
- 无有效 facts 时显示准确的未执行/失败原因，不生成计算能力描述。

**验收标准**

1. 伪行、旧行、部分指标和无 provenance 行不产生 MD 计算描述。
2. 有效运行显示准确 platform、candidate、轨迹、协议、CUDA/CPU 状态和摘要。
3. 报告中的每项事实都能定位到当前 manifest 字段和 artifact SHA。

### GL-CURRENT-015 肽构象集合被报告成 receptor-ligand OpenMM complex MD

**问题与证据**

- python/genoligand_workers/md_summary_worker.py:186-228、2321-2451 的肽路径执行 RDKit conformer ensemble，没有 receptor。
- 该路径仍写 md_summary、md_trajectory 和 md_stability_score。
- Rust 报告看到任意 md_summary 行后固定描述 OpenMM complex、显式溶剂、PME 和 CUDA 证据。

**解决思路**

- 将 peptide conformer ensemble 与 complex MD 分为不同 method 和 artifact schema。
- 报告与 inventory 按 md_run_manifest.method 生成描述。
- 构象集合不使用 MD score、OpenMM、complex、溶剂、PME 或 CUDA 字段。

**验收标准**

1. 肽输入报告明确为 RDKit 构象集合和无受体模型，md_score=0。
2. 肽构象报告不出现 complex、OpenMM、PME、显式溶剂和 CUDA。
3. 小分子 receptor-ligand 路径继续显示真实 OpenMM 描述。

### GL-CURRENT-016 authenticity gate 依赖可绕过的字符串黑名单

**问题与证据**

- crates/gls-science/src/lib.rs:37-55、6296-6333 主要搜索 fixture、smoke、static_fallback 等子串。
- 缺失产物会跳过；没有禁词的合成行可通过，合法科学文本包含这些词时会误报。
- 伪候选、外部 docking pose 和伪 MD summary 均通过当前 gate。

**解决思路**

- 用正向 provenance proof 替代文本黑名单作为真实性判定。
- 每个最终行必须绑定当前 invocation、允许的 source manifest、query digest、input/record/candidate 和 artifact SHA。
- 文本标记保留为诊断信息。

**验收标准**

1. 没有禁词但缺 provenance 的合成行被拒绝。
2. 合法名称包含 sample/fixture 时，完整 provenance 行可通过。
3. 每个最终候选和报告条目都能沿证据图回溯到当前输入和来源。

### GL-CURRENT-017 Foldseek 只搜索首个数据库，并接受空或孤儿数据库

**问题与证据**

- Rust foldseek_target_database 遇到第一条 ready 行就返回。
- worker 在 manifest 没有可用行时递归扫描任意 .dbtype。
- db_ready 只检查文件存在；空 target.dbtype 动态返回 ready 并被 fallback 选中。
- Foldseek 输出缺少 source_database_id、source_digest、builder_version 和 current query identity，Rust 再用默认值回填。
- alntmscore 被读取但没有进入结构相似性评分。

**解决思路**

- 删除文件系统 fallback，解析 manifest 中全部必需数据库。
- 校验完整 companion 文件、Schema、来源摘要、builder/tool version 和真实 smoke query。
- 每库独立执行并输出 DB/source/query/input/record provenance 与 TM-score。

**验收标准**

1. 空、孤儿、缺 companion 或缺 provenance 的 DB 为 not_ready。
2. 两个 ready DB 都执行并各有 outcome，或 manifest 明确证明唯一必需 DB。
3. hit 绑定当前 run query、input 和 record。
4. 缺 provenance 的 Foldseek 输出被拒绝。
5. TM-score 对最终结构分数有明确、测试覆盖的影响。

### GL-CURRENT-018 发布的 worker JSON Schema 拒绝真实生产 job/result

**问题与证据**

- schemas/worker_job.schema.json 和 worker_result.schema.json 均设置 additionalProperties=false。
- 实际 job 固定包含 run_id、invocation_id；MD 还包含 candidate、input 和 digest 字段。
- 实际 result 增加 expected binding、artifact 和 provenance 字段。
- 当前架构测试没有使用真实序列化 job/result 验证发布 Schema。

**解决思路**

- 定义通用 binding 基类和按 worker type 分支的条件 Schema。
- Rust 与 Python 使用同一个 schema_version。
- 架构测试直接验证真实成功、失败和 fallback payload。

**验收标准**

1. MMseqs、Foldseek、docking、MD 的真实 job/result 全部通过 Draft 2020-12。
2. 删除或改写 run_id、invocation_id、input digest 或 candidate binding 时失败。
3. 加入未知字段时失败。
4. Rust fallback failure result 与 Python failure result 均通过同一 Schema。

### GL-CURRENT-019 ChEMBL 现场查询每次解压整库并执行全表 RDKit UDF 扫描

**问题与证据**

- crates/gls-science/src/lib.rs:3451-3655 每次查询将 5.368 GiB chembl_sqlite.tar.gz 解到临时目录。
- activities join 对扫描过的分子调用 genoligand_canonical_smiles，CPU、内存和临时磁盘随数据库规模增长。
- 现场没有发布可直接查询的 chembl.sqlite。
- ChEMBL/BindingDB 输出使用 current_run_onsite_query 固定文本，缺 source digest、release、query digest 和 invocation。

**解决思路**

- 安装阶段发布只读、版本化的 ChEMBL SQLite。
- 建立 connectivity 与 isomeric canonical SMILES 索引，查询使用索引而非全表 Python UDF。
- 查询结果记录 source SHA、release、query digest、run 和 invocation。

**验收标准**

1. 真实小分子查询不解压 archive。
2. 查询计划不对 activities/molecule 表执行全表 RDKit UDF。
3. 定义并通过可重复的时延、峰值内存和临时磁盘阈值。
4. source digest 或 release 变化后旧结果不可复用。

### GL-CURRENT-020 censored 活性关系被当作精确定量值

**问题与证据**

- ChEMBL 与 BindingDB 代码把任意非等号关系统一评分为 0.65。
- 结构优先级去掉小于、大于、约等符号后，把阈值当作精确 nM。
- 方向相反的上下界获得相同分数；现有测试固化了该行为。

**解决思路**

- 等号值进入定量 score。
- censored 值保留 relation、bound、unit 与证据置信度，采用区间逻辑或非定量证据通道。
- 结构优先级不能把上下界当 exact。

**验收标准**

1. =10 nM 进入定量评分。
2. >10、<10 和约 10 保留各自方向和语义，不产生相同的伪精确分。
3. 结构选择不把 bound 当作 exact potency。
4. 正反向边界和单位换算均有回归测试。

### GL-CURRENT-021 流水线固定只为一个候选执行 docking/MD

**问题与证据**

- crates/gls-science/src/lib.rs:5100-5379 的结构候选选择命中第一条即返回，selection 文件被覆盖为单行。
- docking_worker.py 和后续 MD 路径明确要求恰好一个候选。
- 其他 coordinate-backed top candidates 永远没有 docking/MD outcome，也没有逐候选失败原因。

**解决思路**

- selection 输出配置确定的 top-N 独立候选。
- 每个候选使用独立 job、invocation 子目录、artifact manifest 和 outcome。
- 排名按 CandidateIdentity 汇合，单个候选失败不允许跨候选复用产物。

**验收标准**

1. N 个可对接候选生成 N 个独立 docking outcome。
2. 进入 MD 的每个候选都有成功或精确失败原因。
3. 任一产物都不能绑定到其他候选。
4. 排名 decomposition 显示每个候选实际拥有的结构和 MD 证据。

### GL-CURRENT-022 OpenFF 2.2.1 力场文件可由任意非空文件冒充

**问题与证据**

- scripts/setup/install_complete_scientific.py:3428-3462 对已存在文件只检查非空并计算自身 hash。
- 没有固定官方摘要、XML/offxml 结构验证或 OpenFF ForceField 实际加载。
- 下载 URL 指向可变 main 分支。
- 13 字节 not-an-offxml 动态返回 status=ready。

**解决思路**

- 固定不可变 release URL 和官方 SHA-256。
- 校验 XML/offxml 结构，并使用目标 OpenFF 版本真实加载。
- manifest 记录版本、发布来源、expected/actual digest 和 loader smoke。

**验收标准**

1. 伪文件、截断 XML、摘要不符全部进入 needs_repair。
2. 固定 2.2.1 文件可由 OpenFF 加载后才 ready。
3. 内容变化触发重新获取，last-good 文件原子保留。

### GL-CURRENT-023 CPU 模式会因存在 CUDA 插件名而强制选择 CUDA

**问题与证据**

- 安装器 CUDA probe 只枚举平台名，没有创建最小 Context。
- md_summary_worker.py 固定按 CUDA -> CPU -> Reference 选择，不读取用户 GPU 策略。
- 伪平台列表 CPU/CUDA/Reference 时动态选择 CUDA；代码中没有 gpu_md/require_gpu job 字段。

**解决思路**

- 在 job/config 中显式记录 platform policy。
- CPU 模式明确选择 CPU。
- GPU 模式在安装和运行前创建最小 CUDA Context，记录 device、driver 和 platform properties。
- required、preferred、disabled 三种策略拥有确定结果。

**验收标准**

1. CUDA 插件存在但设备不可用时，CPU 模式在 CPU 完成。
2. GPU-required 模式在安装或 preflight 阶段阻塞。
3. GPU 可用模式真实创建 CUDA Context 并记录设备遥测。
4. 报告显示实际使用的平台。

### GL-CURRENT-024 worker 依赖和 Python 版本没有形成可重放环境

**问题与证据**

- 安装器工具包列表缺少 PLINDER worker 必需的 pyarrow，CUDA requirements 同样缺失。
- 安装器逐包安装 latest 或 Git HEAD，没有消费单一锁定清单。
- 本地 venv 继承任意 sys.executable；当前 Windows 默认 Python 3.14，environment-workers.yml 声明支持小于 3.13。
- 默认 Python 环境的 worker unittest 有两个模块因 numpy/pyarrow 缺失而导入失败，安装器状态仍不会提前报告 pyarrow 缺失。

**解决思路**

- 建立按 CPU/CUDA profile 锁定的唯一 worker 环境清单。
- 列出 numpy、pandas、pyarrow、jsonschema 等全部直接依赖；Git 来源固定 commit 和 hash。
- 安装前执行 Python 支持版本 gate，并自动选择已支持解释器。

**验收标准**

1. 全新受支持 Python 环境一次安装后完成 PLINDER parquet -> canonical。
2. 缺 pyarrow 时 tool_environment 直接进入 needs_repair。
3. Python 3.14 环境显示明确支持范围并选择受支持解释器。
4. 重复安装的包版本、wheel/hash 和 worker smoke 结果一致。

### GL-CURRENT-025 大型源在一次 repair 中被多次全量哈希，执行清单写入非原子

**问题与证据**

- external_plan_row_ready、命中分支和 source manifest 会对同一来源重复内容哈希。
- MGnify 源约 79.3 GB，无变化 repair 可读取约 237.9 GB。
- 摘要缓存只存在当前进程局部阶段，外部同步与后续 manifest 没有复用。
- download/external/source 三类 manifest 直接覆盖正式文件；进程中断可留下截断清单。

**解决思路**

- 每个 installation invocation 建立统一 source fingerprint 对象并跨阶段复用。
- 对稳定文件元数据使用可信持久缓存，变更时执行一次内容 hash。
- 所有 manifest 写 temp，完成 fsync、Schema 和闭包校验后原子 replace；保留 last-good 与 current-attempt。

**验收标准**

1. 79 GB 无变化 repair 的全量内容 hash 次数为 0；变更后最多 1 次。
2. 注入任意行写入崩溃后，正式 manifest 保持完整可读。
3. 修改来源会改变摘要并使相关 DB 失效。
4. 安装诊断能区分 last-good 与失败 attempt。

### GL-CURRENT-026 首次下载真实性没有绑定发布方校验信息

**问题与证据**

- BindingDB、UniProt、Binding MOAD 在 source_catalog.tsv 中没有 expected SHA-256。
- 首次远程下载接受任意非空 payload，并把首次观察摘要写成后续基准。
- UniProt stream 和其他 mutable URL 没有不可变 release/checksum 绑定。

**解决思路**

- 优先使用发布方 checksum、签名和不可变 release ID。
- 没有官方摘要的来源必须执行媒体类型、压缩格式、Schema、记录数、版本元数据和领域 smoke 验证。
- manifest 记录 trust_source 与 release identity。

**验收标准**

1. HTTP 200 HTML、错误压缩包、版本与清单不符全部失败。
2. 固定 release + checksum 的正确文件通过。
3. 上游版本变化显式使旧来源和下游索引失效。
4. 每个 ready 来源都有可审计的信任来源。

### GL-CURRENT-027 大型 FASTA、FASTQ 和 GenBank 输入整体读入内存

**问题与证据**

- input_conversion.rs:606-865 先 fs::read 整文件，再构造完整 String 和全部 records。
- 与 genome/metagenome 的 GB 级用户输入能力不匹配，会形成多份内存物化。

**解决思路**

- 使用 BufRead 流式解析。
- 按 batch 写 spool 和 input manifest，逐 record 计算 ID 与 SHA。
- 分子和小坐标文件继续采用整体读取。

**验收标准**

1. 大型 synthetic multi-FASTA 的峰值内存受 batch 上限控制，不随文件总大小线性增长。
2. FASTA、FASTQ、GenBank 的 record ID、内容摘要和错误行号保持准确。
3. 中途坏记录产生明确 failed 状态，已写 partial spool 不进入后续阶段。

### GL-CURRENT-028 下载计划宣称续传，实际会隔离 partial 后重新下载

**问题与证据**

- download_plan.tsv 写 resume_existing_partial_file。
- MGnify/RCSB 外部同步没有传入可信 SHA-256。
- copy_or_download 对无可信 hash 的非空 partial 先隔离并从零下载。
- 现场 4.766 GiB MGnify partial 因此成为确定的重复下载成本。

**解决思路**

- 使用官方 checksum 或 ETag + Content-Length 建立资源身份。
- 身份一致时发送 Range 并从已验证偏移续传。
- 身份变化时删除旧 partial 后从零下载。

**验收标准**

1. 中断测试的第二次请求从已有偏移继续。
2. 服务端资源身份变化时拒绝拼接。
3. 最终文件长度和 hash 正确。
4. 成功后 .part 与 quarantine 清零。

### GL-CURRENT-029 target ranking 配置没有读取者且与运行时权重不同

**问题与证据**

- configs/target_ranking.yml 权重为 0.55/0.35/0.10/0/0。
- crates/gls-science/src/lib.rs 实际使用 0.50/0.25/0.10/0.05/0.10。
- 全仓没有该配置的读取路径，修改配置不会影响排名。

**解决思路**

- 选择单一权威来源。
- 需要配置时，在启动阶段解析并校验权重总和、Schema 与版本，并将配置摘要写入 run provenance。
- 权重固定时删除失效配置及相关文档声明。

**验收标准**

1. 配置 fixture 改权重后，排名、score decomposition 和报告同步变化；或者删除配置后仓库不再宣称其生效。
2. 每次运行记录实际使用的权重版本和摘要。
3. 权重缺失、非法或总和错误时 preflight 失败。

### GL-CURRENT-030 Nextflow 资源模型固定，现有测试没有执行真实 workflow

**问题与证据**

- workflows/main.nf 固定申请 20 CPU、32 GB，覆盖 profile 可配置性。
- 约 300 GB science_store 被建模为 path 输入，真实 fingerprint/staging 行为没有验收。
- tests/nextflow/check_nextflow_task_contracts.py 是文本正则检查，没有启动 Nextflow。
- 当前 Windows 环境没有 nextflow 可执行文件，本轮无法形成真实 workflow 证据。

**解决思路**

- 将 cpus/memory 参数化并提供 4 CPU/8 GB 本地验收 profile。
- science store 使用只读外部引用方式，避免复制与全目录内容 fingerprint。
- 在项目 WSL 环境安装固定版本 Nextflow，执行 completed 与 blocked 真实 E2E。

**验收标准**

1. 4 CPU/8 GB profile 完成小型真实夹具。
2. science store 不复制到 task work，启动成本与 store 总大小解耦。
3. CLI 非零、缺 run_state 或状态非 completed 时都不生成 done marker。
4. completed 运行正确 publish 完整 run 目录。
5. Nextflow 版本和配置摘要进入运行 provenance。

## 6. P2 存储与旧版本清理案例

### GL-CURRENT-031 repository/target 与 repository/work 占用 10.30 GiB

**本轮状态**

- 最终复验时两个验收服务均已退出，target/work 占用进程为 0。
- 已删除 target 和 work，实测释放 11057515266 B。
- 删除后两个目录均不存在，Git 跟踪状态没有变化。

**问题与证据**

- target 为 10.120 GiB，主体是 Cargo debug、deps 和 incremental。
- work 为 183.050 MiB，主体是 phase-1-task-5-browser、schema-contract-venv、验收日志和运行产物。
- 两个目录都被 .gitignore 排除，源码没有把其中内容作为持久资源。
- 当前 PID 9980 从 work/phase-1-task-5-browser 运行，PID 29024 从 target/debug 运行并使用 work/final-0.9.72-acceptance。

**解决思路**

- 先结束对应验收服务并保存必要结论，再删除整个 target 和 work。
- 后续验收使用命名清晰的短生命周期目录，结束时由清理脚本按绝对路径白名单删除。

**验收标准**

1. 没有进程的 executable 或 command line 指向 target/work。
2. target 与 work 目录删除，释放约 10.30 GiB。
3. git status 不因清理发生变化。
4. 重新 cargo build 和必要测试可从干净状态完成。

### GL-CURRENT-032 .git 含约 246.283 MiB 不可达对象

**本轮状态**

- 已在提交 020a4994 推送后执行 git gc --prune=now。
- git fsck --full 无输出且成功。
- Git pack 从 3 个、242.73 MiB 降为 1 个、12.72 MiB；.git 当前实测 12.870 MiB。

**问题与证据**

- .git 总大小 261.838 MiB。
- git fsck --full --unreachable 在计入 reflog 后仍报告 17701 个不可达对象，约 246.283 MiB。
- 一个 235.24 MiB pack 的全部 14238 个对象均不可达，另有不可达 pack/loose objects。

**解决思路**

- 最终提交和 push 完成后使用 Git 自身垃圾回收处理，不手工删除 pack。
- 回收后执行 fsck、分支、tag 和远端一致性验证。

**验收标准**

1. main 与 origin/main 一致。
2. git fsck --full 无损坏。
3. git count-objects -vH 显著下降。
4. 当前分支、tag、提交和工作树完整。

### GL-CURRENT-033 science_store 含 4.77 GiB 中断下载与少量旧日志/临时目录

**本轮状态**

- 已确认无 install lock、安装器、下载器、MMseqs 或 Foldseek 进程。
- 已删除 26 个 .part、36 个 0.9.6-0.9.8 安装日志、7 个历史索引 tmp 和 14 个空 RCSB 分桶目录。
- 已释放 5121250300 B。
- 2026-07-15 最终前盘点又发现 20 个无身份 RCSB `.part` 共 9437184 B；有效续传身份数为 0，最终清理时全部删除。

**问题与证据**

- downloads/metagenome/mgnify/mgy_proteins_1.fa.gz.part 为 5117050880 B。
- RCSB mmCIF 目录有 25 个 .part，共 4194304 B，其中 23 个为零字节。
- 根目录有 36 个 install_0_9_*.log，版本 0.9.6 至 0.9.8。
- 7 个 MMseqs/Foldseek tmp 目录含历史 createindex.sh，另有 13 个空 RCSB 分桶目录。
- 当前没有 install.lock.json，也没有 installer、curl、MMseqs 或 Foldseek 进程；运行时和 source manifest 都排除 .part。

**解决思路**

- 删除已确认不参与当前下载和运行的 `.part`、旧安装日志、索引 tmp 和空分桶目录；只有身份匹配且状态明确的 partial 才允许保留，本次现场保留数为 0。
- 完成 GL-CURRENT-028 后通过受控中断测试重新生成并验证可续传 partial。

**验收标准**

1. 无身份或身份过期的 `.part`、`install_0_9_*.log` 和历史 tmp 清零；保留项逐个具有 URL、ETag/长度身份和续传用途。
2. 本轮释放 5121250300 B。
3. MMseqs/Foldseek smoke query 保持通过。
4. 后续产生的 partial 都有资源身份、长度和可验证续传状态。

### GL-CURRENT-034 四个旧 worker 模块没有仓库内调用链

**问题与证据**

- python/genoligand_workers/openmm_worker.py、trajectory_worker.py、pdbfixer_worker.py、openff_worker.py 在仓库内没有调用引用。
- 当前生产路径使用 alignment_worker、docking_worker 和 md_summary_worker。
- 这些旧模块扩大协议表面，可能继续携带已经废弃的 payload 和执行假设。

**解决思路**

- 检查安装后的 wrapper、外部文档和 entrypoint manifest，确认没有发布入口。
- 删除无调用模块及其专属测试/配置。
- 增加 orphan worker 检查，要求每个生产 worker 都由 registry 或正式入口引用。

**验收标准**

1. 当前安装和 runtime manifest 中没有四个模块的 entrypoint。
2. 删除后 Python、安装、架构和真实 worker E2E 全部通过。
3. orphan worker 检查可以发现新增但未注册的 worker。

## 7. 优先级总表

| 优先级 | 案例 | 交付结果 |
|---|---|---|
| P0 | 001-008 | WebUI 可启动；状态、退出码、runtime 与 science store ready 真实一致 |
| P1-A | 010-018 | input/candidate/artifact/worker 强契约闭合 |
| P1-B | 019-030 | 科学语义、安装可靠性、规模性能与 Nextflow 完成 |
| P0 release gate | 009 | 当前版本真实本地端到端验收和发布完成 |
| P2 | 031-034 | 构建缓存、Git 对象、下载残留和旧 worker 清理完成 |

## 8. 总体验收出口

全部案例完成后，统一执行以下出口检查：

1. cargo fmt、clippy、workspace all-features tests 全部通过。
2. 当前 WSL worker 环境的 Python tests、生产 Schema 正反例和真实科学 smoke 全部通过。
3. 安装 dry-run、空间不足、首次下载、中断续传、manifest 迁移、last-good 回退测试全部通过。
4. 当前 Windows runtime 启动 WebUI，五类输入完成真实浏览器流程。
5. CLI 的 completed/blocked/failed/cancelled 退出码、状态文件和 done marker 完全一致。
6. known、metagenome、Foldseek、docking、MD 的每个结果都能重算 provenance。
7. 真实 Nextflow completed/blocked 两条流程通过。
8. clean release 检查通过，仓库没有运行产物、缓存、旧验收目录和临时文件。
9. science store 没有无身份 partial、旧 staging、重复数据库和错误 ready。
10. Git 提交推送后 main 与 origin/main 一致，最终工作树保持已知的本机配置状态。

## 9. 本轮清理决策

- 保留 science_store/downloads 下有来源清单的完整科学源。
- 保留 science_store/indexes 下 manifest 引用的 MMseqs/Foldseek 数据库及其配套文件。
- 保留仓库内当前架构、安装指南、用户指南和历史审查文档，作为需求与回归追踪依据。
- 删除已被本计划替代、仍指向 0.9.36 的根目录续作说明。
- 清理本轮审查生成的临时 run、日志、Python cache 和临时脚本。
- 已完成 science store 中 26 个中断下载、36 个旧安装日志、7 个历史索引 tmp 和 14 个空分桶目录的清理。
- 已完成 Git 不可达对象回收，.git 从 261.838 MiB 降至 12.870 MiB。
- 已在两个验收服务退出后删除 target/work，释放 11057515266 B。
- 本轮已确认清理合计释放约 15.31 GiB。

## 10. 实施与验收台账（2026-07-14 当前状态）

本表保留 34 个案例的逐项映射。“验收证据”记录本轮已经实际执行并确认的结果；science store、最终 runtime、正式 API、浏览器和真实产物攻击仍处于现场执行中，完成现场动作后再更新案例结论。

已确认的横向基线：Rust 格式检查与 clippy 通过，Rust 测试共 280/280 通过；Python/installer 集中门禁 18/18 通过，worker 70/70、current installer 42/42、runtime audit 15/15、runtime failure 6/6 通过；Nextflow 的 completed、blocked、missing-state、CLI-failure 四条路径通过；server API、PLINDER published index、runtime provenance attacks 三个驱动的轻量自检通过。

| 案例 | 已实施内容 | 本轮验收证据 | 剩余现场动作 |
|---|---|---|---|
| GL-CURRENT-001 | `schemas/input_capabilities.json`、server 能力接口、WebUI 保存校验与 API 验收驱动已接通。 | Rust 264 项测试通过；`run_server_api_acceptance.py --self-test` 通过，并校验生产形态 job/result 的 Draft 2020-12 Schema。 | 执行中：用最终 release server 和全新浏览器验证五类能力、保存结果、健康状态、启动请求及控制台。 |
| GL-CURRENT-002 | readiness v1 已贯通安装器、CLI、server health/database 与报告消费。 | 当前安装器修复回归 23 项通过；相关 Rust 回归包含在 264 项通过结果中。 | 执行中：修复现场 science store blocker，并核对各入口读取同一 readiness JSON 与稳定 blocker code。 |
| GL-CURRENT-003 | alignment manifest v2、旧 manifest 迁移探针、worker companion/provenance 校验已实现。 | 安装器 23 项与 Python 62 项测试通过。 | 执行中：迁移或重建现场 6 个 MMseqs DB，逐库运行已知序列和宏基因组短序列 smoke，并验证 last-good 保护。 |
| GL-CURRENT-004 | RCSB scope snapshot、坐标清单、Foldseek 来源绑定和 partial 排除已实现。 | 安装器 23 项测试通过，覆盖 scope、coordinate snapshot 与来源绑定回归。 | 执行中：完成现场 RCSB scope、坐标与 Foldseek 索引闭包，再以真实查询确认 ready。 |
| GL-CURRENT-005 | 容量预算与 `--dry-run` 提前阻断已进入当前安装器。 | 安装器 23 项测试通过，包含 staging 前容量阻断。 | 执行中：在 D 盘保存真实逐项字节预算，验证不足容量下没有下载、解压和 staging 副作用，并确认 MGnify 范围。 |
| GL-CURRENT-006 | CLI/server build identity、runtime manifest、SHA-256 校验、health identity 与 WebUI build id 已实现。 | Rust fmt、clippy、264 项测试及安装器 23 项测试通过。 | 执行中：最终提交后 release build、发布并重启正式入口，核对 commit、二进制摘要、manifest、health 与进程路径。 |
| GL-CURRENT-007 | blocked outcome、CLI 稳定退出码、状态文件与 Nextflow done gate 已实现。 | Rust 264 项测试通过；Nextflow completed、blocked、missing-state、CLI-failure 四条路径通过。 | 执行中：用最终 release CLI/runtime 重放 completed、blocked、failed，并归档退出码、状态文件与 done marker 证据。 |
| GL-CURRENT-008 | invocation 绑定、阶段错误包装、failed 原子终态、last-successful-stage 与 compare-and-set 已实现。 | Rust 264 项测试通过；Nextflow missing-state 与 CLI-failure 真实路径通过。 | 执行中：对最终 runtime 的输入、数据库、worker、排序、报告分别注入故障，并在浏览器核对刷新、修复动作和旧回调隔离。 |
| GL-CURRENT-009 | 五类输入、CLI/worker/science store、Nextflow、API 与 WebUI 的验收入口已经形成。 | Rust 264、Python 62、安装器 23 项通过；Nextflow 四路径与 server API 驱动自检通过。 | 执行中：在同一最终 commit/runtime/store 下完成五类输入、检索、top-N、docking/MD、报告、导出和浏览器全链验收。 |
| GL-CURRENT-010 | 逐 record 身份、canonical namespace、candidate digest 与多记录转换已实现。 | Rust 264 项测试通过，包含 candidate identity、多批次及多记录回归。 | 执行中：用同靶点多输入、多来源 record 和 multi-record SDF 跑真实流程，核对独立/合并边界及 docking/MD 延续。 |
| GL-CURRENT-011 | indexed docking manifest/index/selection/result 校验和四类攻击拒绝已实现。 | Rust 264 项测试通过；`check_runtime_provenance_attacks.py --self-test` 通过。 | 执行中：对最终 completed run 的真实 Vina 产物执行外部 pose、内容篡改、跨候选 manifest、旧 invocation 四案。 |
| GL-CURRENT-012 | docking worker 完整 artifact 摘要与 MD 对 docking provenance 的精确绑定已实现。 | Python 62 项与 Rust 264 项测试通过；攻击驱动自检通过。 | 执行中：执行真实 Vina→MD 链并逐类篡改 docking 产物，核对 Context 前拒绝、无 MD/报告完成产物及合法链复算。 |
| GL-CURRENT-013 | MD 指标、协议、平台产物及 indexed semantic validator 已实现。 | Python 62 项与 Rust 264 项测试通过；攻击驱动自检覆盖 MD artifact、单位、候选、score 伪造契约。 | 执行中：在真实 OpenMM completed run 执行四类伪造，并回查指标、单位、帧数、步数、state、trajectory 与重算分数。 |
| GL-CURRENT-014 | `VerifiedMdFacts`、动态 evidence ledger 与基于验证事实的报告生成已实现。 | Rust 264 项测试通过；攻击驱动自检通过。 | 执行中：从真实 MD manifest 生成最终报告，逐句回查方法、平台、system、protocol、trajectory 和 artifact SHA。 |
| GL-CURRENT-015 | peptide conformer 与 complex MD 已分流，报告语义按真实计算类型生成。 | Python 62 项和 Rust 264 项测试通过。 | 执行中：分别运行真实肽构象集合与小分子 complex MD，核对肽报告和小分子平台事实。 |
| GL-CURRENT-016 | 当前 invocation 的正向 provenance proof、artifact 必需性和 indexed authenticity gate 已实现。 | Rust 264 项测试通过；runtime provenance attacks 驱动自检通过。 | 执行中：遍历最终候选、decomposition、报告和 evidence ledger 的全图 provenance，并执行真实产物攻击。 |
| GL-CURRENT-017 | alignment worker 的权威清单、多库执行、companion/provenance 校验、TM-score 与 Rust Foldseek 行校验已实现。 | Python 62 项和 Rust 264 项测试通过。 | 执行中：对现场全部必需 Foldseek DB 运行当前 query，归档逐库 outcome，并验证空库、孤儿库和缺 provenance 库拒绝。 |
| GL-CURRENT-018 | worker job/result Schema、Python I/O 校验和 API 验收驱动的生产形态 payload 验证已实现。 | `run_server_api_acceptance.py --self-test` 通过，真实 job/result 样本由 Draft 2020-12 validator 校验并汇总覆盖。 | 执行中：从最终 runtime 捕获 MMseqs、Foldseek、docking、MD 的成功、失败和 fallback job/result，再完成 API 现场读回。 |
| GL-CURRENT-019 | `chembl_query.sqlite` 发布、只读索引查询与 release/digest provenance 已实现。 | Rust 264 项和安装器 23 项测试通过。 | 执行中：在现场发布库记录查询计划、时延、峰值 RSS、临时磁盘，并确认运行期无 archive 解压和全表 RDKit UDF。 |
| GL-CURRENT-020 | relation/bound 保留、censored 非定量通道与结构优先级规则已实现。 | Rust 264 项测试通过，包含 censored 与结构选择回归。 | 执行中：用现场 ChEMBL/BindingDB 覆盖 `=`、`>`、`<`、约等、方向与单位组合，核对 bound 不进入 exact potency。 |
| GL-CURRENT-021 | top-N 独立候选 job/invocation/index/outcome、worker candidate binding 与排名回并已实现。 | Rust 264 项测试通过，包含多候选独立 docking/MD job 回归。 | 执行中：用至少 3 个真实候选跑 top-N，核对独立目录、逐候选结果、无跨候选复用及 decomposition。 |
| GL-CURRENT-022 | 固定 OpenFF 2.2.1 来源/digest、XML/loader 校验与 last-good 发布已实现。 | 安装器 23 项测试通过，覆盖伪文件、截断 XML 和 digest 拒绝。 | 执行中：在目标 worker 环境加载正式文件，保存 loader 结果并验证失败尝试不替换 last-good。 |
| GL-CURRENT-023 | MD job `platform_policy`、Schema 约束与真实 OpenMM Context 平台选择已实现。 | Python 62 项和 Rust 264 项测试通过。 | 执行中：在本机 CPU/CUDA 环境运行 disabled、preferred、required，保存 Context、设备、驱动与 properties，并验证 required 预检阻断。 |
| GL-CURRENT-024 | worker 环境锁、Python 版本 gate、依赖闭包、source lock 与安装 smoke 已实现。 | 安装器 23 项测试通过；`check_plinder_published_index.py --self-test` 通过。 | 执行中：在全新受支持 Python 环境重复安装并核对来源/版本/hash，再对现场 published PLINDER parquet→canonical→query 运行验收。 |
| GL-CURRENT-025 | invocation fingerprint cache、跨进程持久缓存和 manifest 原子写入已实现。 | 安装器 23 项测试通过，覆盖持久缓存与 atomic writer。 | 执行中：对现场 MGnify repair 记录 hash 次数，并在三类 manifest 注入中断，核对 last-good 与 attempt 诊断。 |
| GL-CURRENT-026 | source catalog/sync plan、媒体/压缩/Schema/记录数验证、trust/release 字段和 source lock 已实现。 | 安装器 23 项测试通过，覆盖首次下载领域验证和可审计信任。 | 执行中：逐来源核对官方 checksum 或不可变 release，并用真实首次下载验证 HTML、错误压缩和版本错配拒绝。 |
| GL-CURRENT-027 | BufRead 流式解析、批量写入、失败清理及 1 GiB RSS 验收驱动已实现。 | 三案通过，RSS 上限 268,435,456 B：FASTA 1,073,745,058 B/260,898 records/99,028,992 B；FASTQ 1,073,748,103 B/130,783/54,435,840 B；GenBank 1,073,742,694 B/260,076/100,122,624 B。 | 执行中：用最终 release CLI 重放三案，并注入中途坏记录，核对错误行号及 normalized/spool/pending 不进入下游。 |
| GL-CURRENT-028 | remote identity、Range/If-Range、partial identity、ETag/长度变化拒绝与完成清理已实现。 | 安装器 23 项测试通过，覆盖同身份续传和身份变化拒绝拼接。 | 执行中：对真实大源执行受控中断续传，核对 Range 偏移、最终长度/hash 以及 `.part`、identity、quarantine 清零。 |
| GL-CURRENT-029 | 单一固定 ranking policy、版本与 digest 已写入 run context、decomposition 和报告。 | Rust 264 项测试通过，包含 policy 内容、版本与 digest 回归。 | 执行中：在真实运行中逐项核对 run context、decomposition 和报告引用同一权重、版本与摘要。 |
| GL-CURRENT-030 | 固定 Nextflow 版本、可配置资源、4 CPU/8 GB profile、外部 store、状态/done gate 与 provenance 已实现。 | `run_nextflow_e2e.py` 的 completed、blocked、missing-state、CLI-failure 四条路径通过。 | 执行中：以最终 release runtime 固化配置 digest、完整日志和 provenance，并复核 science store 未复制到 task work。 |
| GL-CURRENT-031 | `.gitignore`、clean release 门禁和安全清理规则已存在。 | Rust fmt、clippy、264 项测试、Python 62 项与安装器 23 项通过后已形成最终清理输入清单。 | 执行中：全部现场验收结束后停止相关进程，清除 `target/work`、误生成 run、缓存与临时证据，再运行 clean release 门禁。 |
| GL-CURRENT-032 | Git GC/FSCK、对象统计和远端一致性验收步骤已定义。 | 当前工作树修改仍在汇总，最终 Git 验收按第 8 节出口执行。 | 执行中：完成全部修复后提交并 push，再执行安全 GC、`git fsck --full`、对象统计及 `main`/`origin/main` 一致性核对。 |
| GL-CURRENT-033 | 已有旧 `.part`、日志、index tmp、空目录清理记录；安装器新增受身份保护的续传。 | 第 9 节已记录约 15.31 GiB 清理结果；安装器 23 项回归通过。 | 执行中：science store 修复和续传验收结束后重新盘点 D 盘，清除新 attempt/staging/partial，保留有效源与 last-good DB。 |
| GL-CURRENT-034 | 四个旧 worker 模块已删除，worker registry 契约检查入口已存在。 | Python 62 项测试通过，当前 worker 代码与调用链按现有模块运行。 | 执行中：运行最终安装、架构与真实 worker E2E，核对 runtime manifest、wrapper、文档和发布目录无旧 entrypoint、导入及缓存副本。 |
