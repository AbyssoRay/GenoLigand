# GenoLigand 44 项审计问题完整修复设计

## 1. 目标

完整修复 `docs/CODE_AUDIT_2026-07-11.md` 中的 `GL-AUDIT-001` 至 `GL-AUDIT-044`，使输入身份、科学计算、候选身份、产物来源、运行生命周期、安装完整性、工作流和 WebUI 状态形成一致的可验证系统。

完成条件不是现有测试继续通过，而是每个审计编号都有对应的失败回归测试、实现修复、局部通过证据和最终本地验收证据。

## 2. 约束

- 所有测试、科学运行和浏览器验收在本机执行。
- GitHub 用于存储提交，不运行 GitHub Actions。
- 保留 Rust、Python、JavaScript 和 Nextflow 技术栈。
- 科学解析和轨迹计算使用成熟库：RDKit、PyArrow、MMseqs2、Foldseek、AutoDock Vina、MDTraj/MDAnalysis、OpenMM。
- 每个阶段递增补丁版本、提交并推送 `main`，禁止新建其他分支。
- 不以文字调整、状态改名或宽松 fallback 代替实际科学能力。
- 运行所需工具从 GenoLigand 已安装 runtime/WSL 环境解析，不能以历史 `tool_versions.tsv` 代替实际探针。

## 3. 方案选择

采用契约驱动的分阶段修复。保留现有模块边界，在输入、阶段状态、候选身份和产物来源处建立四个强契约；随后修复科学算法、运行状态和安装完整性。这样可以逐阶段验证，同时避免大规模重写造成行为不可追踪。

不采用以下方案：

- 整体重写流水线：改动范围过大，无法为 44 项建立清晰的前后证据。
- 外围 gate 包裹现有实现：无法修复 docking、MD、序列 token、来源版本和旧产物污染等根因。

## 4. 核心数据契约

### 4.1 ResolvedInput

输入归一化后，每条记录必须包含：

```text
input_id
record_id
declared_field
canonical_kind
canonical_value
format
source_kind = inline | file
source_path
content_sha256
```

规则：

1. 文件输入先检查存在、普通文件和可读性，再读取内容。
2. `canonical_value` 保存解析后的科学内容，不保存文件路径。
3. FASTA 的每条记录保持独立 `record_id`，不得跨记录拼接。
4. SDF/MOL2 由 RDKit 解析为带立体化学的 canonical SMILES；解析失败即输入失败。
5. PDB/mmCIF 保留坐标 artifact 和内容摘要；结构特征从坐标内容生成。
6. UI 和服务器调用同一输入能力描述，名称/CAS 需要本地化学解析器实际转换后才可保存。

### 4.2 StageOutcome

worker 和 Rust 阶段统一使用：

```text
completed
not_applicable
blocked
failed
```

语义：

- `completed`：算法真实执行并产生通过 schema、来源和新鲜度检查的结果。
- `not_applicable`：输入类型在科学上不需要该阶段；允许流水线继续，运行摘要显示 NOT_APPLICABLE。
- `blocked`：缺工具、缺数据库、缺必需输入或完整性 gate 未通过；流水线退出非零。
- `failed`：算法启动后失败、输出损坏或部分必需数据库失败；流水线退出非零。

`PASS` 只能由 `completed` 映射。缺输入、缺工具和部分失败不能映射为 PASS。

### 4.3 CandidateIdentity

候选内部身份由以下元组构成：

```text
input_id + canonical_namespace + canonical_target_id
```

`candidate_id` 使用该元组的 SHA-256 前 16 个十六进制字符，格式为 `target_<digest>`。原始 ID、显示 ID、来源数据库和 canonical namespace 分字段保存。

合并规则：

1. 只有 canonical namespace 和 canonical target ID 同时相等才可合并。
2. 已知数据库可通过规范化 UniProt accession 合并。
3. 没有共享规范 accession 的 metagenome ORF 保持来源命名空间，不与人类已知靶点合并。
4. 同一靶点对应不同 input 时保持独立候选。
5. 元数据按证据质量和完整度选择，不采用首行覆盖。

### 4.4 ArtifactProvenance

每个可评分或可下载产物必须记录：

```text
run_id
invocation_id
input_digest
candidate_id
source_digest
tool_name
tool_version
parameters_digest
artifact_sha256
artifact_path
```

规则：

1. `artifact_path` 必须解析在当前 run 目录内。
2. 消费产物前核对 run、输入、候选、参数和内容摘要。
3. 重跑先创建新 invocation 目录；不复用旧 worker/tool 输出。
4. authenticity gate 缺失、损坏或包含 blocked 行时，候选、证据和报告均不可下载。

## 5. 八阶段实施设计

### 阶段 1：输入解析与类型识别

覆盖 `001`、`002`、`028`、`043` 中的输入部分、`044` 中的 CLI 路径。

- 把文件解析与内联文本分类分开。
- 字段提示先限定候选类型，再由结构解析器验证；字符集启发式不能覆盖显式小分子字段。
- 名称和 CAS 通过本地 RDKit/数据库名称解析表转换；无法转换时返回具体原因。
- CLI 必填路径保持必填，空路径和隐式当前目录回退直接报错。
- 输入 manifest 和转换 TSV 增加 `record_id`、`source_kind`、`source_path`、`content_sha256`。

### 阶段 2：序列、现场搜索与索引一致性

覆盖 `003` 至 `010`、`015`、`023`、`024`、`035`、`037` 至 `039`、`041` 的 MMseqs 部分。

- 长序列索引和查询共享同一个 token 函数和 k 值。
- DNA/RNA 统一为 DNA 字母表后执行六 frame 翻译；每条 FASTA 独立处理。
- 蛋白、核酸和翻译蛋白建立独立 MMseqs query DB，选择正确搜索模式。
- 每个持久数据库单独记录 outcome 和 provenance；任一必需数据库失败时阶段失败。
- worker 输出保留 `target_database`，Rust 按数据库 manifest 分类为 known 或 metagenome。
- PLINDER 使用 PyArrow batch scanner 读取真实 Parquet，不生成文件名伪记录。
- ChEMBL 定量评分只使用关系为 `=` 的标准化活性；censored 活性保留为证据但不进入定量分。
- ChEMBL/BindingDB 对每个 input 聚合全部唯一 target，不使用全局 200 行硬截断。
- ChEMBL 在安装阶段发布可直接查询的 SQLite；BindingDB/ZIP 使用流式成员读取。
- 来源和索引 manifest 记录 SHA-256，来源摘要变化时强制重建。
- MMseqs/Foldseek readiness 需要非空数据库文件、配套索引和本地 smoke query。
- MMseqs 启用真实 alignment 输出 identity，不使用估算 pident 作为定量证据。

### 阶段 3：候选身份、分类与排名

覆盖 `017`、`018`、`042`、`043` 的候选匹配部分。

- 引入 CandidateIdentity。
- 规范化 organism：NCBI taxonomy ID 优先，别名映射其次。
- target class 使用来源明确字段；字符串推断只能作为低优先级 fallback。
- 数据库候选分子双方使用 RDKit isomeric canonical SMILES；先按连接性筛选，再按立体化学确认。
- 排名输出保留每个 evidence source，元数据按质量规则选择。

### 阶段 4：Docking 和 MD 科学计算

覆盖 `011` 至 `014`、`034`、`041` 的 Vina 部分。

- docking manifest 直接携带选定 candidate/input 的配体和受体，不再从全局表取第一行。
- 搜索框使用已选 pocket 或共晶配体中心；没有可信 pocket 时阶段 blocked。
- 受体准备只移除水和明确无关小分子，保留金属、辅因子及其 provenance。
- Vina seed 由 invocation digest 确定并写入 manifest。
- MD 先处理 PBC，再以受体骨架 Kabsch 对齐；RMSF 和 ligand drift 在同一参考系计算。
- contact occupancy 按帧统计“存在接触的配体原子/残基比例”，不以全部原子笛卡尔积为分母。
- H-bond 使用 MDTraj/MDAnalysis 的供体、受体、距离和角度定义。
- 生产段数、每段步数和总采样帧数必须大于最低值；零步或单帧不能评分。
- fallback 物理量先按 Å 阈值换算，再 clamp 最终无量纲分数。

### 阶段 5：流水线状态、产物与 Nextflow

覆盖 `006`、`013`、`015`、`016`、`026`、`027`、`033`。

- 引入 StageOutcome 和 ArtifactProvenance。
- blocked/failed 写完整状态后返回非零 CLI 退出码。
- projection/index readiness 在 CLI 核心和 server preflight 使用同一函数。
- JSON Schema 由生产结构生成或在测试中直接校验生产对象，字段名称保持一致。
- Nextflow 使用 `path` 输入和输出，CLI/runtime 路径显式传入；结果目录作为工作流产物发布。
- `pipeline_done.txt` 只在 CLI 成功且 run state 为 completed 时生成。
- 本地 Nextflow 验收必须实际执行成功和 blocked 两个场景。

### 阶段 6：项目、运行与进程生命周期

覆盖 `019` 至 `022`、`029`、`030`、`032`。

- 项目和运行 ID 使用 UUID v4。
- ServerOptions 持有共享 run registry 和 project locks。
- start、save input、cancel、delete 通过同一项目锁串行化。
- 后台命令保留 Child/process-group 句柄；cancel/delete 终止完整进程树并等待退出。
- 完成回调采用 compare-and-set，只能更新仍属于当前 invocation 的状态。
- stdout/stderr 流式追加到日志，设置有界日志保留策略。
- setup status 与 progress 从同一 reconciler 产生；活动锁存在时不因时间阈值启动第二安装器。
- 项目状态区分 draft、input_ready、running、completed、blocked、failed。

### 阶段 7：WebUI 状态和有界结果读取

覆盖 `019`、`028` 至 `031`。

- 增加 GET project input manifest API；打开项目时重新加载项目名称和输入。
- 输入能力描述由服务器返回，前端不维护另一套正则契约。
- 项目列表显示 draft 和 input_ready，可管理空草稿。
- TSV 分页使用 BufRead 流式扫描，内存与 page size 相关；总数统计不保留全表。
- 前端轮询使用单一 in-flight promise/AbortController，上一轮完成前不发下一轮。
- 本地浏览器验收覆盖项目 A/B 切换、名称/CAS、文件路径、运行取消和大型分页。

### 阶段 8：安装、运行时发布和最终闭环

覆盖 `023` 至 `025`、`035` 至 `040`、`044`。

- 安装锁使用原子 create-new，记录 PID、进程启动时间和 heartbeat；只有确认 owner 不存在时才回收。
- 来源同步以完整计划为单位提交，任何必需对象失败都不能 ready。
- 下载 partial 在校验失败时隔离并重新下载；没有可信摘要的来源生成并固定本次内容摘要与版本元数据。
- 归档解析全部流式进行；ZIP 的所有支持成员都被遍历。
- PLM embedding 文件名使用 header 内容摘要；validator 重算文件 SHA-256。
- CPU 配置只要求 CPU/Reference；CUDA 作为启用 GPU MD 时的能力 gate。
- WSL runtime 从明确配置或所有可用发行版中按能力探针选择，不按名称猜测。
- cargo build 失败立即终止发布；二进制发布按当前 PID/instance ID 停止实例，不按进程名批量终止。
- 最终执行 44 项验收矩阵和真实本地端到端运行。

## 6. 错误处理

1. 所有解析错误包含 input ID、record ID、格式和可执行修复原因。
2. 外部工具错误包含数据库 ID、命令退出码、stderr 摘要和 invocation ID。
3. 部分数据库失败不生成“完成”结果；已生成的部分行保留在诊断目录，不进入排名。
4. 原子状态文件先写同目录临时文件，再 rename 覆盖。
5. 产物摘要或 provenance 不匹配时，状态为 failed，不能回退读取旧文件。
6. 安装和运行取消后等待子进程退出，再更新最终状态。

## 7. 验收矩阵

| 审计项 | 阶段 | 权威验收证据 |
|---|---:|---|
| 001 | 1 | SDF/FASTA/PDB 内容测试；不存在路径失败；特征与文件内容一致 |
| 002 | 1 | `CCCCCCCC` 保持小分子；`caffeine` 经名称解析；序列字段仍正确分类 |
| 003 | 2 | 100 aa 完全相同序列在索引/查询中产生确定命中 |
| 004 | 2 | 反向互补 ORF、RNA ORF、多 FASTA 记录测试 |
| 005 | 2 | 混合核酸/蛋白建立独立 query DB 和正确搜索模式 |
| 006 | 2/5 | 任一必需工具/数据库失败使 stage failed、CLI 非零、无 PASS |
| 007 | 2 | UniProt/BioLiP 命中保持 known provenance 和权重 |
| 008 | 2 | 真实 PAR1 fixture 通过 PyArrow 产生真实 PLINDER 行；坏 Parquet 失败 |
| 009 | 2 | `> 10 nM` 不获得精确 10 nM 的定量强效分 |
| 010 | 2 | 两个 input 各超过 200 个 target 时两者全部聚合输出 |
| 011 | 4 | 两输入/对映体 fixture 的 ligand、candidate、receptor 完全绑定 |
| 012 | 4 | pocket 中心进入 Vina box；Zn/heme 等所需组分保留 |
| 013 | 4/5 | 同 run 重跑和外部绝对路径均不能复用旧产物 |
| 014 | 4 | 整体平移/旋转不改变指标；H-bond 几何和接触定义测试 |
| 015 | 5 | `used 5000` 在 server 与 CLI 都 blocked |
| 016 | 5 | gate 缺失/损坏/blocked 时所有研究产物不可下载 |
| 017 | 3 | 相同 raw ID、不同 namespace 输出不同候选 |
| 018 | 3 | `A:B` 与 `A/B` 输出不同稳定 candidate ID |
| 019 | 6/7 | 项目 A/B 切换后表单与各自 manifest 一致 |
| 020 | 6 | cancel/delete 后进程树退出，状态不会被完成回调覆盖 |
| 021 | 6 | 运行中保存输入被串行化，旧 run 不能覆盖新选择 |
| 022 | 6 | 并发创建/启动产生唯一 UUID 和唯一目录 |
| 023 | 2/8 | 任一必需同步对象失败时 source/install 均非 ready |
| 024 | 2/8 | 修改源摘要后索引强制重建并包含新增唯一 target |
| 025 | 6/8 | 并发安装只有一个 lock owner；活动 heartbeat 不被回收 |
| 026 | 5 | 本地 Nextflow 从任务目录执行并发布完整 run 目录 |
| 027 | 5 | 坏 manifest/blocked store 返回非零，且无 done marker |
| 028 | 1/7 | 名称、CAS、存在文件可保存；不存在文件和无效名称返回原因 |
| 029 | 6 | status/progress 使用同一 reconciler，不出现 ready/false 矛盾 |
| 030 | 6/7 | draft 和 input_ready 状态准确且草稿可见可删除 |
| 031 | 7 | 百万行 synthetic TSV 首页读取保持有界内存且无重叠轮询 |
| 032 | 6 | 运行中日志持续增长，大输出不被完整驻留内存 |
| 033 | 5 | 四类生产 JSON 全部通过对应 schema 校验 |
| 034 | 4 | 零步/单帧拒绝评分；4 Å/3 Å fallback 按原始单位换算 |
| 035 | 2/8 | 空 `.dbtype` 和缺配套文件失败；真实 smoke query 才 ready |
| 036 | 8 | CPU-only 达到 ready；GPU MD 启用时 CUDA gate 生效；能力型 WSL 选择 |
| 037 | 8 | 损坏 partial 自动隔离重下；内容摘要与版本被固定 |
| 038 | 2/8 | 大型 ZIP/tar synthetic fixture 的峰值内存受 batch 上限约束 |
| 039 | 2/8 | 多成员 ZIP 的全部支持成员均进入索引 |
| 040 | 8 | 碰撞 header 生成不同 embedding 文件；validator 检出篡改 |
| 041 | 2/4 | MMseqs true identity；同 seed 的 Vina 结果和 manifest 可重放 |
| 042 | 3 | organism 别名、taxonomy 和高质量元数据选择测试 |
| 043 | 1/3 | 等价 canonical 形式匹配；相反对映体不互相匹配 |
| 044 | 1/8 | CLI 空/缺路径失败；build 失败不发布旧 binary；实例级停止测试 |

## 8. 最终本地验收

必须按以下顺序执行并保存结果摘要：

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --all-features`
4. 所有 tracked Python 文件 `py_compile`
5. 所有 tracked JavaScript 文件 `node --check`
6. 六个 tracked contract scripts
7. 44 项验收矩阵测试
8. 本地 Nextflow completed/blocked 场景
9. 本地 WebUI 浏览器场景，覆盖项目切换、输入、运行、取消、分页和下载 gate
10. 使用 GenoLigand runtime/WSL 的一次真实 onsite search、docking 和 MD 运行，核对 provenance、候选绑定和可重放参数
11. 清理测试夹具和构建产物，确认 Git 工作树干净

只有上述证据全部成立，并且 `docs/CODE_AUDIT_2026-07-11.md` 的 44 项都被更新为已修复及对应证据，目标才完成。
