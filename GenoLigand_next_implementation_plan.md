# GenoLigand 后续实现方案

本文档是下一轮直接编码用的实施清单。后续不再重新讨论方向，只按本文件的目标、文件、函数、输出和验收项推进。

## 1. 软件目标

GenoLigand 面向本地科研使用。用户只输入研究对象，软件完成计算、检索、排序和报告。

目标流程如下：

1. 用户输入小分子、多肽、蛋白质配体、复合物结构，或自定义基因组/宏基因组。
2. 安装流程准备数据库、索引、工具环境、来源记录、完整性记录和许可证记录。
3. 运行流程先把输入归一化，再计算可检索特征。
4. 系统在已知蛋白数据库中检索候选。
5. 系统在宏基因组和潜在蛋白序列空间中检索候选。
6. 系统补充结构、口袋、界面、对接和短分子动力学证据。
7. Rust（系统核心语言）合并证据、排序候选、写证据链和报告语义。
8. Python worker（计算工人进程）只执行受结构约束的计算原语，不写最终分数、排序或报告解释。
9. 数据库、工具或输入缺失时阻塞运行，候选数为 `0`。

## 2. 当前已完成基础

不要重复实现以下内容：

1. 当前版本号为 `0.9.2`。
2. WebUI（网页界面）已经压缩为输入、配置、检查、运行、结果、导出。
3. 普通用户主界面不显示 GitHub、提交号、远端仓库、内部验证门和原始调试信息。
4. 五类输入已经进入统一入口：
   - 小分子。
   - 多肽。
   - 蛋白质配体。
   - 复合物结构。
   - 自定义基因组/宏基因组。
5. 无输入时候选数为 `0`，输出数为 `0`。
6. 科学仓库缺失时运行被阻塞，候选数为 `0`。
7. Nextflow（流程调度工具）主工作流只调用 `gls-cli`，不直接调用 Python。
8. Rust 已生成并校验 worker job（计算任务）和 worker result（计算结果）结构。
9. 安装器已生成三类资源记录：
   - `source_file_manifest.tsv`：来源文件记录。
   - `hash_manifest.tsv`：完整性记录。
   - `license_manifest.tsv`：许可证记录。
10. 后端摘要已把内部资源记录转换成用户可理解的“来源记录、完整性记录、许可证记录”。
11. WebUI 检查页已展示来源记录准备状态，但不显示内部路径、哈希值或清单字段。

## 3. 不得恢复的内容

后续实现不得恢复以下内容：

1. 历史阶段验收脚本、工作流、报告生成器或产物引用。
2. 用户界面中的自证式文案，例如“真正流程”“真实流程”等。
3. 固定候选、内置结果表、内置结构、内置分数。
4. 要求普通用户手动配置数据库路径。
5. 普通用户界面中的开发者概念，例如提交号、远端仓库、结构约束、内部清单、哈希值、原始 JSON、验证门、路由。
6. Python worker 写最终排序、最终分数、报告结论或用户解释。
7. 第三方工具原始报告直接进入最终用户报告。

## 4. 待实现总清单

后续工作分为八个编码任务：

1. 安装闭环：补齐数据库下载、索引构建、工具版本检测和修复动作。
2. 输入归一化：五类输入都写入统一输入记录。
3. 特征计算：五类输入都产生可检索特征。
4. 已知数据库检索：从特征检索已知蛋白候选。
5. 宏基因组检索：从特征检索潜在蛋白序列候选。
6. 结构和动力学证据：把结构、口袋、对接和短分子动力学结果转换成 GenoLigand 表格。
7. 候选排序、证据链和报告：Rust 写最终排序、证据链、分数拆解和报告。
8. WebUI 与后端：只展示用户需要的状态、阻塞原因、候选、证据和导出文件。

## 5. 任务 A：安装闭环

### 目标

安装命令必须准备完整科学资源。用户运行主流程前不需要手动配置数据库。

### 需要修改的文件

1. `scripts/setup/install_complete_scientific.py`
2. `crates/gls-science/src/lib.rs`
3. `configs/source_catalog.tsv`
4. `configs/source_sync_plan.tsv`
5. `configs/tool_adapters.yml`
6. `README.md`

### 命令入口

保留并完善：

```text
gls-cli install prepare-science-store
```

### 必须生成的科学仓库结构

```text
science_store/
  science_store_manifest.tsv
  install_status.json
  source_requirements.tsv
  indexes/
    known_targets.tsv
    metagenome_targets.tsv
    structure_targets.tsv
    tool_versions.tsv
  warehouse/
    compounds.parquet
    targets.parquet
    activities.parquet
    structures.parquet
    interactions.parquet
    metagenome_orfs.parquet
  manifests/
    source_file_manifest.tsv
    license_manifest.tsv
    hash_manifest.tsv
```

### 实现步骤

1. `install_complete_scientific.py` 读取 `configs/source_catalog.tsv` 和 `configs/source_sync_plan.tsv`。
2. 对每个资源检查本地文件是否存在、文件数量、字节数、完整性记录和许可证记录。
3. 对缺失资源执行下载或同步；下载失败时写清楚失败原因，不写 ready。
4. 调用 `gls-cli install prepare-science-store` 重建索引。
5. `gls-cli` 从原始文件构建以下索引：
   - 已知蛋白索引 `known_targets.tsv`。
   - 宏基因组潜在序列索引 `metagenome_targets.tsv`。
   - 结构相互作用索引 `structure_targets.tsv`。
   - 工具版本索引 `tool_versions.tsv`。
6. `install_status.json` 只有在全部必需资源、工具、来源记录、完整性记录和许可证记录就绪时写 `ready`。
7. 任何缺失都写 `needs_repair`，并返回用户可执行的下一步动作。

### 验收项

1. 科学仓库目录存在。
2. `science_store_manifest.tsv` 存在。
3. `install_status.json` 存在。
4. `known_targets.tsv` 存在且数据行数大于 `0`。
5. `metagenome_targets.tsv` 存在且数据行数大于 `0`。
6. `structure_targets.tsv` 存在且数据行数大于 `0`。
7. `tool_versions.tsv` 存在且必需工具状态齐全。
8. 资源缺失时主流程阻塞，候选数为 `0`。

## 6. 任务 B：输入归一化

### 目标

五类输入必须统一写入 `input_normalized_manifest.tsv`。每条记录保留输入类型、规范化值、验证状态和可计算 token（检索片段）。

### 需要修改的文件

1. `crates/gls-science/src/lib.rs`
2. `crates/gls-server/src/lib.rs`
3. `webui/app/app.js`

### 输出文件

```text
input_normalized_manifest.tsv
```

建议列：

```text
input_id
input_kind
raw_label
normalized_value
file_count
feature_tokens
validation_status
validation_message
```

### 各输入类型处理

1. 小分子：
   - 接收 SMILES（简化分子线性输入规范）、SDF（结构数据文件）、MOL2（分子结构文件）、化合物名称或登记号。
   - 优先用 RDKit（化学信息学工具）或 Open Babel（分子格式工具）规范化。
   - 无法解析时阻塞，不生成候选。
2. 多肽：
   - 接收短肽序列、修饰说明或结构文件。
   - 识别氨基酸序列、长度、修饰标记。
   - 非法字符阻塞。
3. 蛋白质配体：
   - 接收 FASTA（序列文件）、蛋白序列或结构文件。
   - 识别序列长度、氨基酸组成、结构文件引用。
4. 复合物结构：
   - 接收结构文件或复合物描述。
   - 提取链、配体、界面关键词。
5. 自定义基因组/宏基因组：
   - 接收 FASTA、核酸序列或蛋白序列文件。
   - 识别 ORF（开放阅读框）候选。

### 验收项

1. 五类输入都有测试覆盖。
2. 每类输入至少产生一条规范化记录，或产生明确阻塞原因。
3. 空输入不生成候选。
4. 非法输入不生成候选。
5. 用户界面只显示用户可理解的错误。

## 7. 任务 C：特征计算

### 目标

`features compute` 必须基于归一化输入生成特征，不得只复制用户文本。

### 需要修改的文件

1. `crates/gls-science/src/lib.rs`
2. `python/genoligand_workers/openff_worker.py`
3. `python/genoligand_workers/pdbfixer_worker.py`
4. `python/genoligand_workers/openmm_worker.py`
5. `python/genoligand_workers/trajectory_worker.py`
6. `python/genoligand_workers/worker_io.py`

### 输出文件

```text
feature_manifest.tsv
```

建议列：

```text
input_id
input_kind
feature_family
feature_name
feature_value
feature_source
feature_token
evidence_artifact
```

### 实现步骤

1. 小分子：
   - 计算规范 SMILES。
   - 计算 InChIKey（国际化合物标识键）。
   - 计算 Morgan 指纹或等价分子指纹。
   - 计算分子量、氢键供体、氢键受体、环数、电荷。
2. 多肽：
   - 计算长度、氨基酸组成、电荷倾向、疏水片段。
   - 有结构文件时进入结构特征分支。
3. 蛋白质配体：
   - 计算序列 k-mer（固定长度序列片段）。
   - 计算结构域或 motif（序列模式）候选。
4. 复合物：
   - 解析链、配体、接触片段。
   - 生成界面 token。
5. 自定义基因组/宏基因组：
   - 识别 ORF。
   - 翻译蛋白片段。
   - 生成序列 token。

### 验收项

1. `feature_manifest.tsv` 数据行数大于输入行数。
2. 每个输入类型至少有一个类型专属 `feature_family`。
3. 每条特征有 `feature_source`。
4. 无可计算特征时阻塞或给出明确原因，不生成候选。

## 8. 任务 D：已知数据库检索

### 目标

`database match-known` 使用安装阶段准备的已知数据库索引，输出已知蛋白候选。

### 需要修改的文件

1. `crates/gls-science/src/lib.rs`
2. `configs/sequence_evidence_weights.yaml`
3. `configs/target_ranking.yml`

### 输出文件

```text
known_database_matches.tsv
```

建议列：

```text
input_id
source
target_id
target_name
organism
target_class
match_score
matched_features
evidence
source_row_id
```

### 实现步骤

1. 从 `feature_manifest.tsv` 读取 token。
2. 从 `science_store/indexes/known_targets.tsv` 读取已知蛋白索引。
3. 按输入类型选择匹配方式：
   - 小分子：化合物相似性、已知活性、结构结合证据。
   - 多肽：肽序列相似性、已知肽-蛋白互作。
   - 蛋白质配体：序列域、互作证据、结构相似性。
   - 复合物：链、配体、界面相似性。
   - 自定义基因组/宏基因组：翻译蛋白与已知蛋白相似性。
4. 输出命中分数。
5. 不满足阈值的记录不得写入候选。

### 验收项

1. 索引缺失时阻塞。
2. 特征缺失时不匹配。
3. 每条命中可以追溯到 source row。
4. 结果不含固定候选。

## 9. 任务 E：宏基因组潜在序列检索

### 目标

`metagenome match-potential` 检索宏基因组、微生物蛋白、未注释 ORF 或潜在序列索引。

### 需要修改的文件

1. `crates/gls-science/src/lib.rs`
2. `scripts/setup/install_complete_scientific.py`
3. `configs/source_catalog.tsv`
4. `configs/source_sync_plan.tsv`

### 输出文件

```text
metagenome_potential_matches.tsv
```

建议列：

```text
input_id
source
target_id
target_name
organism
target_class
match_score
matched_features
evidence
sequence_origin
```

### 实现步骤

1. 安装阶段构建 `metagenome_targets.tsv`。
2. 核酸输入先识别 ORF，再翻译序列。
3. 蛋白输入直接做序列检索。
4. 小分子和多肽通过口袋、结构或已知互作桥接索引。
5. 每条潜在序列保留来源：
   - 数据集名。
   - 序列编号。
   - 原始文件。
   - 完整性记录。
6. 输出候选时标记 `candidate_scope = metagenome_potential`。

### 验收项

1. 宏基因组索引缺失时阻塞。
2. 宏基因组命中与已知蛋白命中分开记录。
3. 潜在序列候选保留序列来源。
4. 不把潜在序列候选写成已知人体蛋白。

## 10. 任务 F：结构、口袋、界面和对接证据

### 目标

`structure evaluate` 把结构证据统一转换为 GenoLigand 输出格式。

### 需要修改的文件

1. `crates/gls-science/src/lib.rs`
2. `python/genoligand_workers/pdbfixer_worker.py`
3. `python/genoligand_workers/openff_worker.py`
4. `configs/tool_adapters.yml`

### 输出文件

```text
structure_patch_summary.tsv
docking_summary.tsv
```

### 实现步骤

1. 从候选排序读取候选列表。
2. 从结构索引读取候选结构证据。
3. 对有结构文件的候选调用受约束 worker：
   - 结构清理。
   - 链与配体识别。
   - 口袋描述。
   - 界面残基提取。
4. 对可对接的小分子或多肽调用对接引擎。
5. 第三方工具输出必须转换为 GenoLigand 表格。
6. 对接失败写失败原因，不写分数。

### 验收项

1. 无结构证据时 `structure_patch_summary.tsv` 只写表头或明确无匹配。
2. 对接未运行时 `docking_summary.tsv` 不出现分数。
3. 对接成功时有工具版本、参数摘要、输入结构和输出姿态记录。
4. 第三方原始报告不进入最终报告。

## 11. 任务 G：短分子动力学筛选

### 目标

`dynamics screen` 只对满足结构、参数和候选证据条件的记录运行短分子动力学，并输出摘要。

### 需要修改的文件

1. `crates/gls-science/src/lib.rs`
2. `python/genoligand_workers/openmm_worker.py`
3. `python/genoligand_workers/trajectory_worker.py`
4. `configs/tool_adapters.yml`

### 输出文件

```text
md_summary.tsv
```

建议列：

```text
candidate_id
rmsd
rmsf
ligand_drift
hydrogen_bond_occupancy
contact_occupancy
interaction_persistence
md_risk_flags
md_score_decomposition
md_stability_score
```

### 实现步骤

1. 只对具备结构、参数和候选证据的记录运行。
2. Rust 生成 worker job。
3. Python worker 执行 OpenMM（分子动力学工具）计算原语。
4. Python worker 返回轨迹摘要和平台信息。
5. Rust 校验 worker result。
6. Rust 转换为 `md_summary.tsv`。
7. Rust 决定是否把动力学分量加入最终排序。

### 验收项

1. OpenMM 不可用时记录不可用，不生成动力学分数。
2. GPU（图形处理器）不可用时按配置阻塞或 CPU（中央处理器）降级。
3. 动力学分数由 Rust 写入。
4. Python worker 不写最终排序。

## 12. 任务 H：候选排序、证据链和报告

### 目标

Rust 统一拥有排序、证据链和报告含义。

### 需要修改的文件

1. `crates/gls-science/src/lib.rs`
2. `crates/gls-server/src/lib.rs`
3. `webui/app/app.js`

### 输出文件

```text
target_candidate_ranking.tsv
candidate_detail.json
score_decomposition.tsv
evidence_ledger.tsv
research_report.md
```

### 实现步骤

1. 合并以下证据：
   - 已知数据库命中。
   - 宏基因组潜在序列命中。
   - 结构相互作用证据。
   - 对接证据。
   - 动力学证据。
2. 使用 `configs/target_ranking.yml` 中的权重。
3. 输出每个分量的分数和来源。
4. 对低置信度候选写风险标记。
5. 报告只写计算预测和证据摘要，不写实验确认。
6. 用户报告中不出现文件路径、哈希值、结构约束字段或内部表名。

### 验收项

1. 候选数等于实际排序表行数。
2. 每个候选都有 score decomposition（分数拆解）。
3. 每个报告 claim（报告断言）都映射到 evidence ledger（证据账本）。
4. 空结果报告不声称已有候选。

## 13. 任务 I：WebUI 和后端

### 目标

WebUI 只展示用户需要的状态。开发者信息只留在内部或高级模式。

### 需要修改的文件

1. `crates/gls-server/src/lib.rs`
2. `webui/app/app.js`
3. `webui/app/styles.css`

### 页面要求

1. 输入页：
   - 五类输入都能保存。
   - 字段级错误即时显示。
2. 配置页：
   - 用户只选择搜索范围和运行强度。
   - 数据库路径不暴露。
3. 检查页：
   - 按输入、数据库、运行环境、GPU、存储、导出分组。
   - 每个阻塞项有下一步动作。
4. 运行页：
   - 显示阶段、当前动作、资源提示、日志摘要。
   - 原始日志默认折叠。
5. 结果页：
   - 候选排序、候选详情、证据链、基因组/ORF、结构与口袋、动力学摘要分区显示。
6. 导出页：
   - 科研报告、候选表、复现实验包、支持包、项目包分开。

### 验收项

1. 普通界面不出现开发者概念。
2. 无科学资源时运行按钮不能生成候选。
3. 结果页只读取当前运行产物。
4. 用户可下载的文件只来自允许清单。

## 14. 必跑验证

每轮提交前运行：

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
node --check webui/app/app.js
python -m py_compile python/genoligand_workers/worker_io.py python/genoligand_workers/openff_worker.py python/genoligand_workers/openmm_worker.py python/genoligand_workers/pdbfixer_worker.py python/genoligand_workers/trajectory_worker.py scripts/setup/install_complete_scientific.py
```

## 15. 必加测试

在 `crates/gls-science/src/lib.rs` 增加或保留：

1. 五类输入都能归一化。
2. 空输入阻塞，候选数为 `0`。
3. 科学资源缺失阻塞，候选数为 `0`。
4. 特征缺失时数据库不匹配。
5. 已知数据库命中可追溯 source row。
6. 宏基因组候选不被写成已知人体蛋白。
7. worker job 和 worker result 结构校验通过。
8. Python 不拥有最终分数。
9. Python 不拥有报告语义。
10. 第三方原始报告不进入用户报告。

在 `crates/gls-server/src/lib.rs` 增加或保留：

1. 用户 API（应用程序接口）不暴露本地路径。
2. 用户 API 不暴露命令行。
3. 用户 API 不暴露内部表名。
4. 用户 API 不暴露结构约束字段。
5. 用户 API 不暴露哈希值。
6. 用户 API 阻塞状态为中文可理解文本。

## 16. 浏览器验证

启动本地服务：

```text
gls-server --host 127.0.0.1 --port 19194 --artifact-dir <runtime_dir> --workspace-root <repo_root>
```

检查：

1. 首屏没有固定项目。
2. 首屏没有固定运行。
3. 首屏没有固定候选。
4. 五类输入卡片存在。
5. 无资源时运行被阻塞。
6. 结果页不显示不存在的候选。

## 17. 清理要求

提交前执行：

1. 删除临时服务目录。
2. 删除 Python `__pycache__`。
3. 删除临时截图。
4. 删除临时输入和临时运行目录。
5. 确认没有旧验收目录。
6. 确认没有历史阶段文件。
7. 确认没有根目录临时包、压缩包、日志包。

检查命令：

```text
rg --files . -g "*历史阶段关键字*" -g "!target/**" -g "!.git/**"
rg -n "历史阶段关键字" . -S -g "!target/**" -g "!.git/**"
git status --short --untracked-files=all
```

## 18. 完成判定

下一轮完成后必须满足：

1. 五类输入都进入同一主流程。
2. 科学资源由安装流程准备。
3. 已知数据库通道可执行。
4. 宏基因组潜在序列通道可执行。
5. 结构证据通道可执行或明确阻塞。
6. 动力学证据通道可执行或明确阻塞。
7. Rust 写最终排序。
8. Rust 写证据链。
9. Rust 写报告语义。
10. Python 只执行计算原语。
11. 无输入时候选数为 `0`。
12. 资源缺失时候选数为 `0`。
13. 用户界面不展示开发者概念。
14. 用户界面不展示固定候选或固定结果。
15. 所有测试命令通过。
16. 临时文件清理完成。

## 19. 建议提交顺序

建议分三次提交：

1. `Complete science store installation closure`
   - 安装闭环。
   - 数据库索引。
   - 工具版本记录。
2. `Implement all-input feature and matching pipeline`
   - 五类输入特征。
   - 已知数据库检索。
   - 宏基因组检索。
3. `Wire structure dynamics evidence and user result views`
   - 结构证据。
   - 动力学证据。
   - 用户结果页。
   - 导出。

每次提交前都运行第 14 节命令。
