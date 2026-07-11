# GenoLigand 全面代码审查报告

- 审查日期：2026-07-10 至 2026-07-11
- 代码快照：`909f5cc0f4afbf968f695b6cd073bb00e8326a7c`
- 审查范围：Rust、Python、WebUI、Nextflow、安装器、配置、Schema、测试与本地运行链路
- 范围约束：按用户要求排除网络安全内容
- 结果：44 项确认问题，其中高严重度 27 项，中严重度 17 项
- 代码修改：本报告阶段未修复问题

## 1. 审查方法

1. 枚举全部 tracked 源码、配置、Schema、工作流和测试文件，按模块独立审查。
2. 从用户输入开始，沿 WebUI、HTTP 路由、Rust 核心、Python worker、科学仓库和结果展示追踪数据流。
3. 对高影响候选使用隔离在 `work/` 下的最小夹具做本地复现。
4. 用现有测试和契约检查验证当前基线，检查测试盲区与被测试固化的错误行为。
5. 合并重复症状，按根因形成一个问题；没有正常产品路径或缺少源码证据的候选未收入确认清单。

证据标记：

- `D`：已通过本地命令、API 或浏览器动态复现。
- `S`：已通过完整源码调用链和现有测试交叉确认。

## 2. 本地验证基线

以下检查全部在本机执行，未使用 GitHub Actions：

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 通过 |
| `cargo test --workspace --all-features` | 通过，133 个测试，0 失败 |
| 64 个 tracked Python 文件 `py_compile` | 通过 |
| 13 个 tracked JavaScript 文件 `node --check` | 通过 |
| repository/authenticity/clean-release/pipeline/install/static-webui contracts | 全部通过 |
| 本地 WebUI 启动与浏览器流程 | 通过，审计端口 `127.0.0.1:19141` |

现有测试通过说明测试基线稳定，同时也表明下列问题大多处在未覆盖的跨模块契约、异常路径、并发路径和科学语义中。

## 3. 高严重度问题

### GL-AUDIT-001 文件路径被当作科学内容处理

- 证据：`D`
- 位置：`crates/gls-science/src/input_conversion.rs:127-141,288-340`；`crates/gls-science/src/lib.rs:2626-2677,6461-6669`；`python/genoligand_workers/alignment_worker.py:217-233`
- 根因：路径输入按扩展名分类后直接把路径字符串写入 `canonical_value`，没有读取文件、检查存在性或解析格式。后续特征和比对逻辑从路径字符生成化学 token、序列或复合物特征。
- 影响：SDF、MOL2、FASTA、PDB 等路径输入生成与文件内容无关的结果。
- 复现：不存在的 `Z:\definitely_missing\ghost.sdf` 被标记 `converted/declared_structure_file`，并生成 29 个特征行和哈希。

### GL-AUDIT-002 小分子名称和合法 SMILES 被误分类为核酸或多肽

- 证据：`D`
- 位置：`crates/gls-science/src/input_conversion.rs:143-242`
- 根因：核酸字符集判定先于带字段提示的 SMILES 判定，氨基酸字符集判定又先于通用小分子判定。
- 影响：合法辛烷 SMILES `CCCCCCCC` 进入宏基因组流程；化合物名称 `caffeine` 进入多肽流程，已知数据库、对接和 MD 路径随之改变。
- 复现：本地 normalize 输出分别为 `genome_or_metagenome` 和 `peptide`。

### GL-AUDIT-003 长蛋白索引与查询使用不兼容的 token

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:1648-1658,6603-6607,6872-6880,7025-7086`
- 根因：长度超过 96 的数据库序列编码为稀疏 6-mer，查询端生成 3-mer，匹配端只计算精确 token 交集。
- 影响：完全相同的长蛋白也可能得到零序列匹配，已知靶点被漏检或降权。

### GL-AUDIT-004 ORF 翻译缺失反向链和 RNA 支持

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:6758-6821`；`crates/gls-science/src/input_conversion.rs:432-450`
- 根因：翻译只遍历正向三个 frame，只识别 DNA 起止密码子；输入分类同时接受 `U`。多条 FASTA 还会先被压成一条连续序列。
- 影响：反向链 ORF、RNA ORF 和跨记录边界的序列得到系统性错误结果。

### GL-AUDIT-005 核酸和蛋白被合并为单一 MMseqs 查询库

- 证据：`S`
- 位置：`python/genoligand_workers/alignment_worker.py:73-81,217-234,304-306`
- 根因：worker 丢弃 `canonical_kind`，把蛋白和 genome/metagenome 写入同一 FASTA，再执行一次自动类型 `createdb`。
- 影响：混合输入中至少一类序列被按错误数据库类型解释，搜索模式也没有切换到核酸或翻译搜索。

### GL-AUDIT-006 MMseqs、Foldseek 和 MD 的 not-ready/失败结果被记为 PASS

- 证据：`D/S`
- 位置：`python/genoligand_workers/alignment_worker.py:20-64,83-124`；`python/genoligand_workers/md_summary_worker.py:48-65`；`crates/gls-science/src/lib.rs:3531-3545,3578-3639`
- 根因：外部数据库失败后直接 `continue`；无输入、无数据库、无结构等分支返回正常 JSON。Rust 只校验结果 schema，并把调用记为 PASS。
- 影响：流水线以 completed 结束，同时现场搜索、结构或 MD 实际未执行。
- 复现：本地投影夹具中 MMseqs、Foldseek、MD 均显示 `PASS`，engine 字段分别是 `not_ready` 原因。

### GL-AUDIT-007 MMseqs 已知数据库命中被统一记为宏基因组证据

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:3251-3287,3390-3411,6927-6931`；`python/genoligand_workers/alignment_worker.py:321-328`
- 根因：Rust 把全部 MMseqs 行追加到 `metagenome_potential_matches.tsv`，且不解析 worker 实际输出的 `target_database` provenance 键。
- 影响：UniProt、BioLiP 等已知蛋白命中被降为宏基因组权重并错误标注来源。

### GL-AUDIT-008 PLINDER Parquet 没有解析却可进入 ready

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:925-935,1030-1038,1286-1324,1819-1828,1893-1897`
- 根因：真实 Parquet 被交给 UTF-8 文本结构解析器，失败后退化为基于文件名的一条伪记录；ready 判定只要求目录存在任意文件和共享索引存在。
- 影响：PLINDER 注释缺失，安装状态仍显示 ready；当前本机约 1 GB 的真实 `PAR1` 文件会经历无效整文件读取。

### GL-AUDIT-009 ChEMBL 活性关系符被忽略

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:2852-2873,2940-2976,6925-6927`
- 根因：`standard_relation` 被读取但未进入评分和 best-hit 选择。
- 影响：例如 `> 10 nM` 会按精确 `10 nM` 强效值获得最高分，检出限约束被错误解释。

### GL-AUDIT-010 ChEMBL/BindingDB 的 200 行上限由全部输入共享

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:2757-2776,2973-2978,3004-3021,3242-3247`
- 根因：两个 onsite 查询都在合并全部输入后对全局 best map 截取 200 行。
- 影响：多小分子运行中，一个输入的高分命中可挤掉另一个输入的全部已知靶点；这属于科学结果硬截断，不是展示分页。

### GL-AUDIT-011 对接配体没有与所选候选和输入绑定

- 证据：`S`
- 位置：`python/genoligand_workers/docking_worker.py:167-198,304-319`；`crates/gls-science/src/lib.rs:3118-3169,3877-3923`
- 根因：候选结构选择记录了 `input_id`，对接 worker 却从所有 known matches 取第一个 `ligand_smiles`，找不到时再取第一个小分子。数据库匹配还会丢弃立体化学。
- 影响：多输入运行或对映体场景中，受体、候选、配体和后续 MD 可属于不同对象。

### GL-AUDIT-012 对接框偏离真实口袋并删除金属/辅因子

- 证据：`S`
- 位置：`python/genoligand_workers/docking_worker.py:14-16,71-103,225-251`
- 根因：准备受体时删除全部 heterogen，再以全受体重原子质心建立固定 22 Å 立方框，没有使用共晶配体或口袋坐标。
- 影响：远离蛋白质心的真实口袋不在搜索框内，Zn、heme 等活性必需组分也被移除。

### GL-AUDIT-013 旧 docking/MD 产物可进入当前运行

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:3341-3355,4353-4365,4575-4585,4800-4826,4864-4956`；`python/genoligand_workers/md_summary_worker.py:1399-1423`
- 根因：同一 `run_dir` 重跑前不清理 worker/tool 输出，结果没有输入摘要或 invocation ID；绝对产物路径还可指向当前运行目录之外。
- 影响：当前候选可继承上一轮 docking affinity、pose、MD summary、轨迹和稳定性分数。

### GL-AUDIT-014 MD 指标不具备坐标和化学不变性

- 证据：`S`
- 位置：`python/genoligand_workers/md_summary_worker.py:1254-1305,1350-1392`
- 根因：RMSF 和配体 drift 使用未对齐的绝对坐标；contact occupancy 以全部受体×配体原子对为分母；H-bond 只按 N/O/S 距离判断，不检查供受体和角度。
- 影响：整体平移、旋转、PBC 和受体大小会改变稳定性分数，非氢键近邻也会被计为氢键。

### GL-AUDIT-015 CLI 核心流水线不检查行数受限的投影索引

- 证据：`D`
- 位置：`crates/gls-science/src/lib.rs:2422-2486`
- 根因：`pipeline_blockers` 只检查若干文件和状态，不读取 `index_build_manifest.tsv` 或 `science_index_readiness.tsv` 的完整性信息。
- 影响：服务器 preflight 会拒绝的 `used 5000` 投影仓库，CLI 和 Nextflow 仍能完成运行。
- 复现：最小 store 标记 ready 且 BindingDB manifest 为 `used 5000`，`gls-cli pipeline run` 返回 `pipeline_completed`。

### GL-AUDIT-016 authenticity gate 缺失时下载逻辑放行

- 证据：`D`
- 位置：`crates/gls-server/src/lib.rs:4095-4116`
- 根因：`authenticity_gate.tsv` 读取失败时 `authenticity_gate_allows_download` 返回 `true`。
- 影响：旧运行、迁移运行或 gate 丢失的 completed 目录可展示候选、证据和研究报告。
- 复现：构造无 gate 的 completed run 后，三个产物均可见，报告下载返回 HTTP 200。

### GL-AUDIT-017 不同数据库命名空间的相同 target_id 被合并

- 证据：`D`
- 位置：`crates/gls-science/src/lib.rs:6895-6941`
- 根因：候选合并表只以 `target_id` 为键，不包含来源或命名空间。
- 影响：人类已知靶点和无关宏基因组 ORF 可合并为同一候选，分数和证据相互污染。
- 复现：known 与 metagenome 均使用 `SHARED`，输出为单行并同时包含两类分数。

### GL-AUDIT-018 candidate_id 清洗会产生碰撞

- 证据：`D`
- 位置：`crates/gls-science/src/lib.rs:6911-6913,7261-7270`
- 根因：候选 ID 把多种标点统一替换为下划线，没有碰撞检测或稳定摘要。
- 影响：`A:B` 和 `A/B` 都变成 `target_A_B`，以 candidate_id 关联的结构、docking、MD 和证据可错配。

### GL-AUDIT-019 打开历史项目会沿用另一个项目的输入表单

- 证据：`D`
- 位置：`webui/app/core/actions.js:61-74`；`crates/gls-server/src/lib.rs:355-376`
- 根因：打开项目只读取项目和运行数据，没有读取该项目的 input manifest；用户 API 也没有对应的 GET inputs 路由。表单状态保持全局值。
- 影响：查看项目 A 时会显示项目 B 的名称和输入，继续保存会覆盖 A 的 manifest。
- 复现：A 使用 `CCO`、B 使用 `CCC`，打开 A 后表单仍显示 `CCC` 和 B 的项目名。

### GL-AUDIT-020 取消运行和删除项目不会停止后台进程

- 证据：`S`
- 位置：`crates/gls-server/src/lib.rs:390-410,1003-1147`
- 根因：取消接口只写 `cancelled` 状态；服务器没有保留 `Child` 句柄。删除项目同样没有终止命令。
- 影响：后台流程继续消耗资源和写文件，完成回调还会覆盖取消状态；删除目录后写入可重新创建部分状态。

### GL-AUDIT-021 运行中保存输入会使项目和后台运行状态分叉

- 证据：`S`
- 位置：`crates/gls-server/src/lib.rs:638-679,1003-1147`
- 根因：保存输入无运行锁，直接删除 `latest_run_id.txt`；旧后台线程结束后又把旧 run ID 写回。
- 影响：界面可启动第二个运行，两个运行读取/写入同一项目状态，最后显示哪个运行由完成时序决定。

### GL-AUDIT-022 项目/运行 ID 与启动流程存在并发碰撞

- 证据：`S`
- 位置：`crates/gls-server/src/lib.rs:574-580,1003-1054,3977-3984,4362-4366`
- 根因：ID 只使用当前毫秒；检查运行中状态、建目录、写 latest run 之间没有锁或排他创建。全局 run 查找按 ID 返回首个目录。
- 影响：双击或多标签并发可创建并行运行，同毫秒碰撞时还可能混写目录或跨项目读取结果。

### GL-AUDIT-023 外部同步部分失败仍可汇总为 ready

- 证据：`S`
- 位置：`scripts/setup/install_complete_scientific.py:2347-2416,3263-3307,3363-3388`
- 根因：执行结果失败后，汇总阶段仍以来源目录存在任意文件或 ready marker 有任意 payload 判定成功。
- 影响：RCSB、同步来源等缺失部分文件时科学仓库仍显示 ready，缺失记录不会触发修复。

### GL-AUDIT-024 来源数据更新不会使 MMseqs/Foldseek 索引失效

- 证据：`S`
- 位置：`scripts/setup/install_complete_scientific.py:2889-3004,3470-3585,4004-4042`
- 根因：索引复用只检查旧状态、行数和文件存在性，不比较来源摘要与索引构建摘要。
- 影响：更新 BindingDB、UniProt、RCSB 或 BioLiP 后，安装器可继续使用旧索引，原始来源和搜索结果不一致。

### GL-AUDIT-025 安装锁存在竞态，长任务还会被误判为陈旧

- 证据：`S`
- 位置：`scripts/setup/install_complete_scientific.py` 的 lock acquire/release 路径；`crates/gls-server/src/lib.rs:2945-2967`
- 根因：锁在读取和 `open("x")` 之间出现时，`FileExistsError` 分支会删除另一进程的活动锁；服务器又把 30 分钟未更新的 running 进度视为陈旧并允许再次启动。
- 影响：两个安装器可并行下载、解压、建索引和发布同一 science store。

### GL-AUDIT-026 Nextflow 输入和输出没有按任务工作目录建模

- 证据：`S`
- 位置：`workflows/main.nf:1-29`
- 根因：输入 manifest、science store、CLI 和输出目录都声明为 `val` 或相对字符串；process 在 Nextflow task work dir 中执行。实际结果目录也没有作为 `path` 输出或发布。
- 影响：默认相对路径解析到错误目录；即使命令执行，Nextflow 只保留 `pipeline_done.txt`，科学结果不进入工作流产物。

### GL-AUDIT-027 blocked 流水线仍返回成功退出码

- 证据：`S`
- 位置：`crates/gls-science/src/input_conversion.rs:53-57`；`crates/gls-science/src/lib.rs:2364-2375`；`crates/gls-cli/src/main.rs:8-12`；`workflows/main.nf:20-25`
- 根因：JSON 解析失败被转换为空值；blocker 路径返回 `Ok`，CLI 因此退出 0；Nextflow 随后写出完成标记。
- 影响：缺输入、坏 manifest 或 science store 未就绪会被自动化系统记录为成功。

## 4. 中严重度问题

### GL-AUDIT-028 WebUI 输入承诺与客户端验证不一致

- 证据：`D`
- 位置：`webui/app/state/app-state.js:35-74`；`webui/app/pages/input.js`
- 根因：界面承诺支持化合物名称、登记号和本机结构文件，但验证器只接受类 SMILES 文本或带特定扩展名的字符串，不检查路径存在性。
- 影响：`acetazolamide` 和 `59-66-5` 无法保存，不存在的 `.sdf` 路径却可以保存。

### GL-AUDIT-029 安装状态与安装进度可同时给出相反结论

- 证据：`D`
- 位置：`crates/gls-server/src/lib.rs:2860-2886`
- 根因：setup status 依据实时 preflight，setup progress 直接映射历史 `install_progress.json`，两者没有收敛。
- 影响：同一页面会同时显示 `database_ready=false` 和“科学仓库已就绪/100%”。

### GL-AUDIT-030 项目生命周期状态没有反映已保存输入

- 证据：`D/S`
- 位置：`crates/gls-server/src/lib.rs:583-623`；`webui/app/pages/project.js:55-76`
- 根因：没有运行时项目状态固定为 `created`，用户文案统一映射为“等待输入”；空项目又被项目列表隐藏。
- 影响：已保存有效输入仍显示等待输入；新建后未保存的项目会在磁盘积累且无法从界面管理。

### GL-AUDIT-031 结果分页会全量读取文件，1 秒轮询可重叠

- 证据：`S`
- 位置：`crates/gls-server/src/lib.rs:4273-4321`；`webui/app/core/actions.js:138-153,273-287`
- 根因：分页先 `read_to_string` 全文件，再收集、过滤全部行，最后切片；前端定时器没有 in-flight guard，并连续读取多个结果端点。
- 影响：大型候选/证据表第一页仍占用 O(文件大小) 内存和 CPU，慢请求会产生重叠轮询。

### GL-AUDIT-032 流水线输出被完整缓冲且运行中不可见

- 证据：`S`
- 位置：`crates/gls-server/src/lib.rs:1080-1133`
- 根因：后台命令使用 `.output()`，将 stdout/stderr 全部缓存在内存，结束后一次性覆盖 `run.log`。
- 影响：长运行无法提供实时日志，大输出会显著增加内存占用，监控页面长期停留在初始日志。

### GL-AUDIT-033 发布的 JSON Schema 与生产对象系统性漂移

- 证据：`S`
- 位置：`schemas/project.schema.json`、`schemas/run.schema.json`、`schemas/artifact.schema.json`、`schemas/install_status.schema.json`；`crates/gls-server/src/lib.rs:618-623,1310-1317,4064-4071`
- 根因：必填字段、字段名和 `additionalProperties:false` 与实际 JSON 冲突；运行时没有 schema 校验，架构测试只检查文件存在。
- 影响：按仓库公开契约校验生产对象会立即失败，跨 Rust/WebUI/package 的字段漂移无法被测试发现。

### GL-AUDIT-034 MD fallback 会截断物理量，零步运行可获得满稳定分

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:4407-4428,5014-5042`；`python/genoligand_workers/md_summary_worker.py` 的环境整数和 frame 汇总路径
- 根因：fallback 在按 4 Å/3 Å 标度换算前先把 RMSD 和 drift clamp 到 `[0,1]`；生产段或每段步数可配置为 0，形成单帧零 RMSD/零 drift。
- 影响：明显不稳定或没有生产采样的轨迹可获得被抬高的 MD 分数，甚至稳定性 1.0。

### GL-AUDIT-035 空 `.dbtype` 文件被判定为可用数据库

- 证据：`S`
- 位置：`scripts/setup/install_complete_scientific.py` 的 MMseqs/Foldseek readiness；`python/genoligand_workers/alignment_worker.py` 的数据库检查
- 根因：ready 判定只检查 `.dbtype` 路径存在，不检查内容、配套索引或实际查询。
- 影响：损坏或未完成数据库进入 ready，随后现场搜索失败并被 GL-AUDIT-006 掩盖。

### GL-AUDIT-036 CPU 环境被 CUDA 检查阻塞，WSL 选择又过度限定

- 证据：`S`
- 位置：`scripts/setup/install_complete_scientific.py:800-916,1042-1056`；`python/requirements-workers-cpu.txt`；`python/genoligand_workers/md_summary_worker.py:807-817`
- 根因：安装器把 CUDA 计入全局必需工具，尽管 worker 支持 CPU/Reference；WSL 查找只接受特定命名模式，普通可用发行版不会被选择。
- 影响：CPU 主机、不运行 MD 的安装，以及已有普通 WSL2 环境都可能无法达到 ready。

### GL-AUDIT-037 无 SHA 来源和断点文件缺少内容完整性闭环

- 证据：`S`
- 位置：`configs/source_catalog.tsv`；`scripts/setup/install_complete_scientific.py:1823-1844` 及下载恢复路径
- 根因：BindingDB、UniProt、Binding MOAD 等来源没有预期 SHA；现存文件和 HTTP 416 分支可按大小/存在性接受，损坏 `.part` 也缺少自动清理重试闭环。
- 影响：截断或旧版本来源可能被标记 ready，带 SHA 的来源则可能持续在同一坏 partial 上失败。

### GL-AUDIT-038 大型归档读取采用多份整文件内存物化

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:671-729,1610-1629,2740-2776,2900-2919,6050-6070,6305-6426`
- 根因：BindingDB ZIP 成员先整文件读为 String，再构造完整记录、去重 map 和输出 String；每次 ChEMBL 查询还把完整 SQLite 成员读入 Python 堆并重新解压。
- 影响：首次 BindingDB 建索引和每次 ChEMBL 小分子查询都有高内存、磁盘和 I/O 峰值，完整数据规模下易终止或超时。

### GL-AUDIT-039 ZIP 数据源只读取第一个受支持成员

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:1610-1629,1160-1168,6065-6070`
- 根因：遍历 ZIP 时遇到首个匹配扩展名后立即返回。
- 影响：包含多个 FASTA/TSV 成员的来源会静默丢弃后续序列和记录。

### GL-AUDIT-040 PLM 文件名碰撞且 validator 不重算摘要

- 证据：`S`
- 位置：`python/geno_plm/cli.py:111-137,171-199`；`scripts/validation/check_plm_embedding_manifest.py`
- 根因：FASTA header 清洗后的名字直接作为 `.npy` 文件名；`A/B` 与 `A?B` 都变成 `A_B.mean.npy`。validator 只检查 SHA 字段非空，不读取文件重算。
- 影响：后一个向量覆盖前一个，两个候选可引用同一 embedding 且通过验证。

### GL-AUDIT-041 比对与对接结果缺少可重放性

- 证据：`S`
- 位置：`python/genoligand_workers/alignment_worker.py:88-110,309-328,399-408`；`python/genoligand_workers/docking_worker.py:71-139`
- 根因：MMseqs 没有启用真实 alignment，却把估算 `pident` 当作真实 identity 并赋予主要权重；Vina 未设置或记录随机 seed。
- 影响：身份百分比和候选分数不准确，相同输入的 pose/affinity 又无法精确重放。

### GL-AUDIT-042 候选分类和元数据采用脆弱的字符串/首行规则

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:6883-6941`
- 根因：human 分类只接受精确字符串 `Homo sapiens`；合并后名称、organism、target_class 采用先到行，投影行可覆盖后续现场高质量元数据。
- 影响：`human`、`Homo sapiens (Human)` 等被归为非人类，候选名称和分类可能与最佳证据不一致。

### GL-AUDIT-043 精确小分子查询丢失立体化学且直接比较原始 canonical 字符串

- 证据：`S`
- 位置：`crates/gls-science/src/lib.rs:2830-2844,2923-2958,3118-3169`
- 根因：查询端使用 `isomericSmiles=False`，SQL 又直接对数据库原始 `canonical_smiles` 做字符串 IN 和 map 查找，没有双方统一规范化。
- 影响：对映体可互相匹配，等价但字符串形式不同的分子也会假阴性。

### GL-AUDIT-044 安装发布和 CLI 路径回退可复用错误对象

- 证据：`S`
- 位置：`scripts/setup/install_complete_scientific.py` 的 cargo build、binary publish 和进程终止路径；`crates/gls-cli/src/main.rs:56-91,134-158`
- 根因：构建失败时可继续复用旧二进制，发布时按进程名终止全部同名服务；CLI 又接受空 science-store 路径，并为文档中必填路径使用当前目录默认值。
- 影响：安装状态可能对应旧版本程序，其他本地实例被一并终止；CLI 可把索引写入错误目录或读取错误项目输入。

## 5. 现有测试为何没有发现这些问题

1. 许多测试检查文件存在、字段存在和命令返回 `Ok`，没有验证科学内容与输入身份的一致性。
2. worker 测试把 `not_ready` 视为 schema 合法结果，没有把“算法未执行”与“运行成功”区分。
3. 安装测试大量使用占位文件、伪 Parquet、空 `.dbtype` 和手工 ready manifest，固化了宽松 readiness。
4. WebUI 契约测试以静态字符串为主，没有覆盖历史项目切换、真实表单状态和并发运行。
5. 并发、重跑、部分失败、来源升级、损坏断点和大文件场景缺少测试。
6. Schema 测试只确认文件存在，没有用生产 JSON 进行实际校验。

## 6. 修复顺序建议

1. 先处理输入身份、候选身份、旧产物复用、projection gate 和 worker PASS/not-ready，这些问题会直接改变研究结论。
2. 随后处理 docking/MD 科学语义、ChEMBL 活性关系和序列索引/查询一致性。
3. 再处理运行并发、取消生命周期、安装锁、来源完整性和索引失效。
4. 最后收敛 WebUI 状态、分页、Schema、可重放性与大文件性能。

本报告记录的是当前快照中的确认问题；修复阶段应为每个问题先增加最小失败测试，再逐项修改和验证。
