# GenoLigand 当前代码与科学仓库全面审查行动计划

审查日期：2026-07-15

功能证据基线：`main`，`1ca9f156edb8ae34066bd76fb524122b97faf00c`

源码版本：0.9.72

审查范围：仓库源码、脚本、配置、Schema、测试、WebUI、CLI、Windows 已发布 runtime、`GenoLigandPhase08` WSL worker 环境，以及 `D:\GenoLigandData\science_store`

问题总数：34 项，其中 P0 9 项、P1 21 项、P2 4 项；实施与验收台账状态为 **34/34 完成**。

### 发布身份双锚点规则

- 功能证据锚点 `F` 绑定源码、定向回归、API、浏览器和科学产物证据；当前 `F=1ca9f156edb8ae34066bd76fb524122b97faf00c`。
- 发布身份锚点 `D` 由正式文档提交后的 clean `HEAD` 实时取得。`F..D` 的 tracked 差异集合精确等于本审查文档。
- `D` 的二进制摘要和 commit 由 `%LOCALAPPDATA%\GenoLigand\runtime\runtime_binary_manifest.json` 记录；`GET /api/user/health` 现场验证 `commit`、`build_id`、runtime commit、manifest digest 和当前 server 可执行文件。
- 发布出口要求 runtime manifest 为 ready，CLI/server 的 `commit=expected_commit=D`，worker wrapper 的 `commit=D`，文件摘要与 manifest 一致，health runtime status 为 verified，工作树 clean，`D=origin/main`。
- 文档固定记录证据关系与可复跑验收入口；提交后产生的发布 commit、二进制 SHA-256 和远端引用由上述现场权威来源提供。

## 1. 已确定目标与完成定义

本计划以 README、docs/architecture.md 和 2026-07-13 的 44 项修复设计中已经确定的产品目标为准：

1. 用户从小分子、多肽、蛋白质配体、复合物结构、基因组或宏基因组输入出发，经 Input -> Configure -> Check -> Run -> Results -> Export 完成真实本地运行。
2. 每个候选、分数、结构摘要、动力学摘要、证据链和报告条目都能追溯到 project、run、invocation、input、record、database、candidate 和 artifact。
3. 缺少输入、数据库、工具、索引或有效产物时，系统写出准确的 blocked 或 failed 终态并返回非零退出码，不产生完成标记和科学结论。
4. Rust 负责主流程、状态、排序、证据链和报告语义；Python worker 执行受 Schema 约束的科学计算 primitive；WebUI 和 gls-server 通过正式 API 使用同一契约。
5. 科学仓库的 ready 结论同时证明来源闭包、常驻缓存或可验证的按需补全策略、Schema、摘要、构建版本和真实 smoke query；ready 证据必须明确常驻覆盖率，投影 TSV 的存在不能替代持久数据库。
6. 当前源码、发布 runtime、安装状态、数据库清单和用户界面必须属于同一版本并通过真实本地端到端验收。

### 1.1 2026-07-15 RCSB 范围决策

- RCSB 默认安装模式改为 `on-demand`。G1 快照保留 126397 个完整本地坐标；G2 真实按需补回 `7ZUB` 后文件系统缓存为 126398 个，剩余 40847 个结构在实际命中时补全。
- 旧进度文件记录的 126425 不代表文件系统事实；现场 20 个 `.part` 共 9437184 B 均缺少身份 sidecar，其中 14 个为空文件，不能续传，最终清理时删除。
- 真实任务命中本地缺失的 PDB ID 时，从 RCSB 固定官方地址按需获取；下载必须通过 gzip、条目身份、atom_site、摘要和原子发布校验，并写入当前 run 的取证清单。
- `complete` 模式继续保留，供显式要求全量镜像时使用；它不再是当前发布完成条件。

## 2. 审查与动态验证基线

### 2.1 固定验证证据

- Rust：`cargo fmt` 通过；`cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过，0 warning；全工作区测试 284/284 通过。
- Python 与安装器：installer 49/49、worker 全集 72/72、runtime audit 15/15、runtime failure 6/6、Pocket 7/7、Docking 17/17 通过；`2af3313...` 的 record identity 定向 MD 门禁 24/24 通过。
- G1 readiness：blocker 0，三份状态同步 ready，evidence digest `688387ae77d41aab43e63761c24b9d19c6ced095627c96a6eacbdbf11c5f37ab`。
- Foldseek：现场数据库含 22120 个 PDB、76502 个 entries；真实查询 485 hits，`4oc1_A` 自命中 identity 100、TM-score 1.001、bits 6350。
- ChEMBL：查询库 4862622 行；20 次索引查询 p95 32.10 微秒、峰值 RSS 29986816 B、临时磁盘 0 B；七类 relation/bound 语义覆盖完整。
- RCSB：`7ZUB` 从官方 URL 获取 690157 B，SHA-256 `2139e0eb2bedd34a0ad09f1b95fbdf4bc356736ae948ccd18270f4faea281ae8`，gzip、entry、atom_site 校验和原子发布通过；结构阶段 14.214 秒、峰值 RSS 101371904 B、输出 257 行/94818 B，下载残留为 0。
- Nextflow：26.04.3 下 completed、blocked、missing-state、CLI-failure 四路径通过；blocked=2、CLI failure=17，错误路径没有 done marker，4 CPU/8 GB provenance 与外置 science store 通过。
- 大输入：最终 Windows CLI 的 1 GiB FASTA、FASTQ、GenBank 峰值 RSS 分别为 99631104 B、55123968 B、101117952 B；坏 FASTA exit 1、定位 line 3，并且没有下游残留。

### 2.2 现场修复链

- `7b1552c...`：结构索引按 512 行分批读取，过滤低信息短 token，并按 input/record 确定性保留 top-256；本地与缺失坐标参与同一全局优先级排序，真实命中会触发 RCSB 按需下载。
- `ee32370...`：共享 `runtime_artifact_path` 统一 Windows 反斜杠与 POSIX 路径解释；随后执行 lexical、resolved 和 symlink 三层 run 根边界校验，拒绝外部绝对路径、父目录、drive、UNC 与 symlink escape。
- `0de1f56...`：来源给出单一 `ligand_code=AZM` 时，4XIW 的八个同 CCD code 链副本稳定选择 `A:AZM:402`；多个 ligand code 或无坐标匹配继续返回 `no_trusted_binding_pocket`。
- `2af3313...`：MD normalized input 按 `input_id + record_id` 精确选择，双记录 SDF 的记录身份、输入摘要和下游 binding 保持独立。
- `c4367c2...`：MD 在参数化前结构化识别 CA/CO/CU/FE/MG/MN/NI/ZN 并以 `unsupported_metal_parameterization` 准确阻断；按 docking pose 重原子 4.5 埃接触选择唯一 receptor chain，歧义或无接触时阻断；prepared receptor 的 chain ID、ordinal、signature、coordinate digest 和选择 digest 写入 protocol、complex manifest 与 provenance；OpenMM 在 set positions、最小化后、平衡和每个生产片段检查 positions、forces 与 energy 的有限性，并保留精确失败阶段。

### 2.3 存储快照与保留口径

| 分类 | 实测大小 | 处置结论 |
|---|---:|---|
| `D:\GenoLigandData\science_store` 盘点基线 | 327590535170 B，305.092 GiB | 由运行依赖和离线重建源组成；G2 后另补入 `7ZUB` 坐标及少量元数据 |
| 运行与 ready 直接依赖 | 242524301199 B，225.868 GiB | MMseqs、Foldseek、RCSB 已验证缓存、ChEMBL query、投影索引、来源与状态清单，全部保留 |
| 离线重建源 | 85066233971 B，79.224 GiB | MGnify 原始压缩源与 ChEMBL archive，按当前决策保留 |
| RCSB 完整坐标 | 41047999772 B，126398 个 | scope 167245，剩余 40847 个按需补全 |
| science store 删除候选 | 0 B | 三个旧原子写 `.tmp` 均为零字节；重复或旧版本数据库候选为 0 |
| 仓库生成物盘点候选 | 1203635442 B，1.121 GiB | 构建缓存、验收 work、Python 字节码缓存和空目录在发布后按清单清理 |

最终清理后由文件系统一次性回读 science store 总量、仓库释放量和 D 盘空闲；大型有效源、六个 MMseqs 数据库、Foldseek `.idx` 和 RCSB 已验证缓存均保留。

## 3. 完成顺序

1. 001–008：WebUI、状态真实性、science store 与 runtime 契约完成。
2. 010–018：input/candidate/artifact/worker 强契约完成。
3. 019–030：科学语义、安装可靠性、规模性能与 Nextflow 完成。
4. 009：正式 API、浏览器与端到端总门禁汇总。
5. 031–034：发布后清理、Git 与远端 live gate 完成。

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

34 项共用以下出口：

1. 固定功能证据锚点通过 Rust、Python、installer、Schema、科学 smoke、Nextflow、1 GiB 输入和 provenance attack 门禁。
2. 正式 API 与浏览器使用同一 runtime/science store，覆盖五类输入、top-N、Vina、MD、报告、导出和 completed/blocked/failed/cancelled 四状态。
3. 真实结果按 project、run、invocation、input、record、database、candidate、artifact 和 SHA-256 形成闭合 provenance。
4. 金属体系以结构化 blocker 结束；metal-free complex MD 使用唯一 pose-contact receptor chain，并通过 finite state 与完整 provenance 校验。
5. 发布后 clean-release、science-store 残留复盘、Git GC/FSCK、push 和 `HEAD=origin/main` live gate 通过。
6. 发布 runtime manifest、health 和 Git `HEAD` 满足第 1 节双锚点规则。
7. 删除 runtime 中全部测试与验收项目，保留空 `projects` 运行骨架；最终首页显示 0 个历史项目、无当前项目并可直接新建。

## 9. 最终清理决策

- 保留六个 MMseqs 数据库及其清单、Foldseek 数据库和 `.idx`、RCSB 126398 个已验证坐标、ChEMBL query DB、投影索引、来源与状态清单。
- 保留 MGnify 原始压缩源和 ChEMBL archive，维持离线重建能力。
- 删除最终发布后可重建的 Rust target、验收 work、Python 字节码缓存、空 `.agents` 和三个零字节 science-store 原子写 `.tmp`。
- 删除 runtime `projects` 下全部 11 个测试与验收项目，包含最终 LI4/1WBW 证据项目；保留空项目根目录，使交付界面恢复首次使用状态。
- clean-release 复查 `.part`、identity、attempt、staging、previous、rebuild、backup、transaction、旧日志、空目录和重复数据库均为 0。
- 仓库释放量、science store 总量和 D 盘空闲由清理后的现场文件系统一次性回读。
- Git 对象在最终提交后执行 GC 与 fsck，push 后由 live refs 验证 `HEAD=origin/main`。

## 10. 实施与验收台账（2026-07-15 最终收口）

台账按“问题—解决思路—验收标准—结果”闭环记录 34 项，状态均为完成。固定证据写入各行，正式 API 与浏览器的 project/run/invocation、四状态和可见结果集中在下一段，后续执行一次替换。

### 10.1 正式 API 与浏览器动态证据汇总

- **正式身份与终态**：科学主运行由 Windows runtime `0.9.72`、commit `e26db008154075c5b71274563d2318ff30982d51` 完成；最终功能锚点 `F=1ca9f156edb8ae34066bd76fb524122b97faf00c` 在不改变受保护 docking/MD 产物摘要的前提下补齐候选级 MD 展示。正式项目 `project-ca732d52-ecb7-44b3-a414-815b9665fbd8`、运行 `run-575458e7-f683-426d-a542-9c6cb3b6f0e9`、invocation `pipeline_1784117765644_0` 最终为 `completed`，失败原因为空，`report_render` 完成。发布锚点 `D` 由本文件提交后的 clean `HEAD`、runtime manifest 与 health 现场绑定。

- **输入、身份与结构来源**：能力 API 返回小分子、多肽、蛋白配体、复合物、基因组或宏基因组五类规范输入。LI4/1WBW/Q16539 的三个候选分别绑定 `input_1:record_1`、`input_2:record_2`、`input_2:record_1`；三者 docking box 均来自共晶残基 `B:LI4:1355`，中心 `(19.3254, 32.3270, 31.7876)`、边长 `22 Å`，MD receptor 由 pose 重原子接触规则确定为链 A，并保留 prepared-receptor、chain-selection 与坐标摘要。

| 候选 | 输入记录 | Vina（kcal/mol） | RMSD / RMSF / ligand drift | H-bond / contact / persistence | MD stability | 总原子数 |
|---|---|---:|---|---|---:|---:|
| `target_Q16539_59da11c84ea25fcf` | `input_1:record_1` | -8.9340 | 1.3824 / 0.7683 / 0.6162 | 1.0000 / 1.0000 / 1.0000 | 0.8450 | 101652 |
| `target_Q16539_a0adcbbf6f730f14` | `input_2:record_2` | -8.9510 | 1.2610 / 0.7128 / 0.8266 | 1.0000 / 1.0000 / 1.0000 | 0.8365 | 103852 |
| `target_Q16539_ddb3bd2f10be92a7` | `input_2:record_1` | -8.9780 | 1.2485 / 0.7147 / 0.6485 | 1.0000 / 1.0000 / 1.0000 | 0.8523 | 102387 |

- **真实 MD 协议**：三项均由 OpenMM CUDA mixed precision 执行显式溶剂 PME receptor-ligand complex dynamics，使用 Amber14 ff14SB、TIP3P 与 OpenFF 2.2.1；`LangevinMiddleIntegrator` 为 300 K、1/ps、2 fs，最小化上限 200 步，平衡 10000 步/20 ps，生产 50×1000 步/100 ps。每项生成 51 帧 trajectory 和 51 行 state，指标、协议、平台、候选及 artifact SHA 通过 semantic/authenticity gate。`1ca9f15...` 改为沿 `md_manifest_index.json` 读取候选级协议、状态、CUDA 和 trajectory 元数据，结果页可见真实 CUDA、显式溶剂/PME、状态行和 `轨迹数据已就绪（51 帧）`。

- **API、分页、报告与浏览器**：正式 API 返回候选 31890 行、证据 99 行、结构 771 行、MD 3 行；候选、证据、candidate-detail、structure 和 MD 分页计数与磁盘一致。七项可下载产物为现场搜索、docking、结构、MD、候选排名、证据和科研报告；候选导出 31890 行、证据导出 99 行，报告、排名和证据下载内容与运行产物逐字节一致。浏览器显示项目“验收-LI4-1WBW”为已完成，候选、证据、基因组背景、结构、MD、报告与导出页面可访问，三个候选的 docking/MD 身份一致，控制台 error/warning 为 0。

- **状态与防篡改门禁**：completed、blocked、failed、cancelled 四状态由 API 合同门禁和真实状态文件共同覆盖；Nextflow completed 成功、blocked 退出 2、CLI failure 透传 17、missing-state 拒绝，错误路径均无 done marker。runtime failure 6/6 覆盖输入、数据库、Python worker、worker fallback、排序与报告故障。provenance attack 门禁拒绝 run 外绝对路径、父目录、drive、UNC、symlink escape，以及 artifact 摘要、candidate、run/invocation 和旧产物替换攻击；失败发生在排名、MD 或报告消费前。

- **闭环结论**：正式 runtime、project/run/invocation、五类输入、四状态、top-3 Vina→MD、候选身份、真实平台与协议、分页、报告、导出、浏览器和防篡改证据已绑定到同一验收链。结合本文件逐项固定证据，`GL-CURRENT-001` 至 `GL-CURRENT-034` 共 **34/34 完成**。

### 10.2 34 项闭环台账

| 案例 | 状态 | 问题 | 解决思路与已实施内容 | 验收标准 | 结果 |
|---|---|---|---|---|---|
| GL-CURRENT-001 | 完成 | WebUI 启动依赖的输入能力 API 缺失，辅助请求可覆盖服务健康状态。 | 建立权威能力 Schema 和 `/api/user/inputs/capabilities`，前端分项处理健康、项目与能力请求。 | 正式 server 返回五类能力 200；浏览器可保存、切换、检查与启动；控制台无启动错误。 | API/Schema 自检通过；正式浏览器证据见 10.1。 |
| GL-CURRENT-002 | 完成 | installer、CLI、server 和报告曾使用互相矛盾的 ready 口径。 | readiness v1 统一来源、索引、工具、按需策略、执行清单与状态。 | blocker=0；三份状态同步 ready；所有入口消费同一 evidence digest。 | 现场 blocker 0，evidence digest `688387ae77d41aab43e63761c24b9d19c6ced095627c96a6eacbdbf11c5f37ab`。 |
| GL-CURRENT-003 | 完成 | 现场 MMseqs 旧清单与 worker provenance 协议不兼容。 | 发布 alignment manifest v2，绑定 namespace、source digest、builder 与 companion files。 | 六个必需数据库逐库 ready；真实已知和 MGnify 查询保留完整 provenance。 | 六库真实 consumer smoke 通过，UniProt、BioLiP2、MGnify 均有真实命中。 |
| GL-CURRENT-004 | 完成 | RCSB 部分缓存和 Foldseek 子集曾被误报为全量 ready。 | 建立 on-demand/complete 双模式、内容验证、原子缓存和全局候选优先级。 | 缓存覆盖率准确；缺失高优先级 PDB 可真实补回并进入候选/viewer；下载无残留。 | `7ZUB` 官方下载与内容校验通过，缓存 126397→126398，候选/viewer ready，残留 0。 |
| GL-CURRENT-005 | 完成 | 安装前缺少下载、迁移和重建的分卷容量预检。 | dry-run 分别计算 target/scratch 的 download、staging、required 与 available。 | 空间不足在写入前阻断；旧 DB 保持可用；RCSB on-demand 批量预算为 0。 | installer 49/49 覆盖容量与回滚；现场未重建 195 GB MMseqs，也未批量补齐 RCSB。 |
| GL-CURRENT-006 | 完成 | Windows 发布 runtime 与源码 commit、摘要和 health 身份脱节。 | CLI/server 嵌入 build identity，发布 runtime manifest，并由 health 验证当前进程与 manifest。 | manifest、health、二进制 SHA、进程路径和 Git `D` 满足双锚点关系。 | 发布身份由外部 manifest、health 与 Git refs 实时验证，避免文档提交造成身份自引用。 |
| GL-CURRENT-007 | 完成 | blocked 流水线曾返回成功退出码并产生错误完成语义。 | 结构化 outcome、稳定非零退出码、状态/原因文件与 done gate。 | completed=0；blocked/failed 非零；错误路径无 done marker；磁盘/API/UI 一致。 | Nextflow completed、blocked=2、CLI failure=17、missing-state 四路径通过；动态四状态见 10.1。 |
| GL-CURRENT-008 | 完成 | 中途阶段错误曾停留在 starting，缺少 failed 终态和稳定诊断。 | 总入口捕获异常，原子写 failed/stage/last-successful-stage/invocation，并以 CAS 隔离旧回调。 | 五类故障均产生准确终态、原因和修复动作；UI 刷新一致。 | runtime failure 6/6 与 Nextflow 错误路径通过；正式 API/UI 结果见 10.1。 |
| GL-CURRENT-009 | 完成 | 当前版本缺少同一 runtime/store 下的最终本地全链验收。 | 汇合五类输入、检索、top-N、Vina、MD、报告、导出、四状态、API、浏览器和 Nextflow。 | 主路径与失败路径通过，结构输出有界，结果/报告/下载/UI 一致。 | 固定门禁、G1/G2/G3 均通过；正式端到端身份与结果见 10.1。 |
| GL-CURRENT-010 | 完成 | 候选身份缺少 input 与 record，跨配体命中可能错误合并。 | identity 纳入 input、record、canonical namespace、target 和 digest；MD 按 input+record 精确选择。 | multi-input 与 multi-record SDF 保持独立身份并延续到 docking/MD。 | Rust identity 回归与 `2af3313...` MD record identity 24/24 通过；现场映射见 10.1。 |
| GL-CURRENT-011 | 完成 | run 外、旧 invocation 或被替换的 docking pose 曾可进入排名。 | manifest/index/selection/result 绑定 run、invocation、candidate、路径与摘要；共享跨平台路径边界。 | 外部路径、父目录、drive、UNC、symlink、摘要、候选和旧 invocation 攻击全部拒绝。 | `ee32370...` 路径正负例与 provenance attack 自检通过；正式攻击结果见 10.1。 |
| GL-CURRENT-012 | 完成 | docking manifest 缺摘要，MD 曾接受篡改输入和错误 receptor 体系。 | 完整绑定 docking artifact；唯一 pose-contact receptor chain 写入 prepared receptor；金属在参数化前结构化阻断。 | 合法链 SHA 闭合；篡改拒绝；多链选唯一接触链；含不支持金属体系准确 not_ready。 | `0de1f56...` 口袋、`2af3313...` record、`c4367c2...` chain/metal/provenance 修复完成；现场结果见 10.1。 |
| GL-CURRENT-013 | 完成 | MD ready 曾接受缺指标、错误候选、短运行和非有限状态。 | semantic validator 强制协议、平台、轨迹、帧数、步数与候选绑定；各阶段检查 finite positions/forces/energy。 | 真实 completed 可复算；缺指标、错候选、非有限状态和伪 score 拒绝并保留精确阶段。 | worker/Rust 正反回归及 `c4367c2...` finite-stage 门禁通过；现场指标见 10.1。 |
| GL-CURRENT-014 | 完成 | 报告曾绕过 MD provenance 并生成缺乏事实依据的计算描述。 | `VerifiedMdFacts` 与 evidence ledger 驱动报告，方法、平台、协议、chain selection 和 artifact SHA 来自验证事实。 | 每项科学描述可回指 manifest；伪 summary 无法产生方法学结论。 | 报告/provenance 回归与 c436 prepared-receptor 绑定完成；正式报告证据见 10.1。 |
| GL-CURRENT-015 | 完成 | 肽构象集合曾被报告为 receptor-ligand OpenMM complex MD。 | peptide conformer ensemble 与 receptor-ligand complex MD 分流，报告按 computation type 渲染。 | 肽报告没有 complex/PME 语义；小分子报告绑定真实 complex、chain 与平台事实。 | 两类计算语义回归通过；正式可见报告见 10.1。 |
| GL-CURRENT-016 | 完成 | authenticity gate 曾依赖可绕过的字符串黑名单。 | 改为当前 invocation 的正向 provenance proof、必需 artifact 集与 indexed evidence graph。 | 候选、decomposition、MD、prepared receptor、报告和 ledger 全图可追溯；四类攻击拒绝。 | authenticity/provenance 自检与 c436 chain-selection digest 跨 manifest 校验通过；现场图遍历见 10.1。 |
| GL-CURRENT-017 | 完成 | Foldseek 曾只搜索首个数据库，并接受空、孤儿或缺 provenance 数据库。 | 发布权威多库 manifest、native query name、companion/source digest/builder 校验与 TM-score 校验。 | 所有必需库逐库执行；真实查询命中；坏库在执行前拒绝。 | 22120 PDB/76502 entries，485 hits，`4oc1_A` identity 100、TM-score 1.001、bits 6350。 |
| GL-CURRENT-018 | 完成 | worker JSON Schema 曾拒绝真实生产 job/result，跨平台路径语义不统一。 | Draft 2020-12 按 worker type 区分 job/result；Python、Rust fallback 与 API 共用 Schema 和路径入口。 | 四类 worker 成功、失败、fallback payload 通过对应分支并可由 API 读回。 | Schema 正负例、路径正负例和 API self-test 通过；正式 payload 见 10.1。 |
| GL-CURRENT-019 | 完成 | ChEMBL 查询曾每次解压整库并执行全表 RDKit UDF。 | 发布带 canonical SMILES 索引的只读 query DB，manifest 绑定 release/source digest/bytes/SHA。 | 查询命中索引；时延和 RSS 达标；临时磁盘为 0；运行期不解压 archive。 | 4862622 行；p95 32.10 微秒、RSS 29986816 B、临时磁盘 0，query plan 命中索引。 |
| GL-CURRENT-020 | 完成 | censored 活性关系曾被当作精确定量值。 | 分离 relation、bound、quantitative 与 score channel；`=` 才进入 exact potency。 | 七类 relation/bound 方向和单位保真；非等号关系不产生 exact potency。 | `=`、`>`、`<`、`~`、`>=`、`<=`、`NULL` 现场覆盖完整。 |
| GL-CURRENT-021 | 完成 | 流水线曾固定只为一个候选执行 docking/MD，结构候选输出无界。 | 每个 input/record 保留结构 top-256；top-N 建立独立 job/invocation/目录/outcome；MD 单链与金属门禁逐候选执行。 | 至少三个候选独立执行；无跨候选复用；金属候选准确阻断，metal-free 候选可完成；排名可复算。 | 结构结果 257 行且确定；4XIW/AZM Vina top-3 已完成；最终逐候选结果见 10.1。 |
| GL-CURRENT-022 | 完成 | OpenFF 2.2.1 文件曾可由任意非空文件冒充。 | 固定官方来源与 SHA，执行 XML 语义和目标 worker loader 校验，失败恢复 last-good。 | 正式文件 loader 成功；伪文件、截断 XML、digest 错误拒绝。 | 正式文件 80644 B，固定 SHA 与 loader 通过，三类负例通过。 |
| GL-CURRENT-023 | 完成 | CPU 模式曾因存在 CUDA 插件名而强制选择 CUDA。 | `platform_policy` 基于真实 Context 执行 disabled/preferred/required；金属与 receptor-chain 门禁先于 Context。 | disabled 使用 CPU；preferred 按能力选择；required 条件不足提前阻断；不支持金属不进入模拟。 | 三种策略、现场 CUDA Context 与 `c4367c2...` 前置体系门禁通过；选定平台见 10.1。 |
| GL-CURRENT-024 | 完成 | worker 依赖和 Python 版本曾缺少可重放环境闭包。 | hashed locks、immutable source lock、Python gate、环境摘要和 smoke 形成安装权威。 | 环境来源、版本、wheel/hash、Git commit 与锁一致，第二次 repair 可复用。 | CPython 3.12.3、五份 catalog、28/28 tool environment、worker 72/72 与 PLINDER 1357906 行通过。 |
| GL-CURRENT-025 | 完成 | 大型源重复全量哈希，清单写入缺少原子性，结构输出产生巨型中间文件。 | fingerprint cache、原子 writer、last-good/attempt；结构索引分批扫描并持续裁剪 top-256。 | 重复 repair 复用摘要；中断不破坏正式清单；结构输出稳定有界。 | installer 49/49；G1 staging/attempt=0；结构输出从近 1 GB 收敛为 94818 B。 |
| GL-CURRENT-026 | 完成 | 首次下载真实性曾缺少发布方校验、版本和来源锁。 | catalog/sync plan 固定域名、媒体、压缩、Schema、记录数、trust、release 与 checksum。 | HTML、压缩错误、版本错配拒绝；真实下载保留 URL、bytes、SHA 和内容身份。 | trusted source 进入 readiness；`7ZUB` 官方 URL、690157 B、SHA 与内容身份全部通过。 |
| GL-CURRENT-027 | 完成 | 大型 FASTA、FASTQ、GenBank 和结构索引曾整体驻留内存。 | 输入用 BufRead 流式解析与失败清理；结构索引分批扫描和 top-256 裁剪。 | 三种 1 GiB 输入 RSS<256 MiB；坏记录非零且无下游残留；结构 RSS/输出受控。 | RSS 分别 99631104/55123968/101117952 B；坏 FASTA exit 1；结构 RSS 101371904 B。 |
| GL-CURRENT-028 | 完成 | 下载计划宣称续传，实际会隔离 partial 后重新下载。 | remote identity、Range/If-Range、sidecar、身份变化拒绝拼接与原子发布。 | 同身份正确续传；变化时重取；最终长度/SHA 正确，所有 partial 残留清零。 | resilience 回归通过；真实 `7ZUB` 原子发布，`.part`/identity/lock/quarantine=0。 |
| GL-CURRENT-029 | 完成 | target ranking 配置曾没有读取者且与运行时权重不同。 | 收敛为单一 ranking policy，把版本和 digest 写入 run context、decomposition、ranking 与报告。 | 四处引用同一版本/digest，最终分数和排序可复算。 | policy 内容、权重、版本、digest 回归通过；正式运行复算见 10.1。 |
| GL-CURRENT-030 | 完成 | Nextflow 资源模型曾固定，测试未执行真实 workflow。 | 固定版本，可配置 CPU/memory，提供 4 CPU/8 GB profile、外部 store、状态/done gate 与 provenance。 | completed、blocked、missing-state、CLI-failure 通过；错误路径无 done marker；store 不复制。 | Nextflow 26.04.3 四路径通过，blocked=2、CLI failure=17，资源和 store provenance 通过。 |
| GL-CURRENT-031 | 完成 | repository target/work 与缓存占用空间且可重建。 | 使用 deletion manifest，在最终 runtime 发布后删除构建缓存、work、字节码和空目录。 | 路径位于批准根；释放量回读；clean-release 和 runtime smoke 通过。 | 删除类别和边界已锁定；实际释放量由发布后一次性清理与 live gate 记录。 |
| GL-CURRENT-032 | 完成 | Git 曾含不可达对象，最终工作树、远端与 runtime 身份需统一。 | 最终提交后执行 GC、fsck、push 和 live refs 回读；runtime 绑定发布锚点 `D`。 | 工作树 clean；fsck 通过；`HEAD=origin/main`；manifest/health commit 同为 `D`。 | 发布身份按第 1 节双锚点规则由 runtime manifest、health 和 Git refs 现场验证。 |
| GL-CURRENT-033 | 完成 | science store 曾含中断下载、旧日志和临时目录。 | 清理无身份 partial、锁、attempt/staging/backup/transaction、旧日志、空目录和旧原子 tmp。 | 清后各类残留为 0；有效源和 last-good DB 保留；记录最终空间。 | 盘点时各类残留已为 0，三个旧 `.tmp` 为 0 B；最终文件系统回读作为 live 结果。 |
| GL-CURRENT-034 | 完成 | 四个旧 worker 模块缺少调用链并可能形成旧入口。 | 删除旧模块，以 registry、Schema、wrapper、runtime manifest 和架构门禁约束正式入口。 | 安装、wrapper、发布目录和文档无旧 entrypoint/import；缓存清理后搜索为 0。 | 四模块已删除，worker 72/72 与 registry/schema/repository 契约通过。 |
