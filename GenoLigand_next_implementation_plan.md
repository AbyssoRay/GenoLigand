# GenoLigand 后续实现方案

本文档用于下一轮直接编码实施。目标是不再重新讨论方向，只按下面的文件、函数、输入输出和验收条件推进。

## 1. 总目标

GenoLigand 要完成一个面向科研用户的本地主流程：

1. 用户只提供研究对象输入。
2. 软件安装阶段准备数据库、索引、工具环境和版本清单。
3. 运行阶段从输入出发计算特征。
4. 系统在已知蛋白数据库和宏基因组潜在序列空间中检索候选。
5. 系统补充结构、口袋、对接和短分子动力学证据。
6. Rust 核心统一完成候选合并、排序、证据链和报告语义。
7. Python 只执行受结构约束的计算原语，不拥有最终分数、结论或报告含义。
8. 数据库、工具或输入缺失时只阻塞，不生成候选。

## 2. 当前已完成基础

以下内容已经存在，后续不要重做：

1. 版本号已是 `0.9.2`。
2. WebUI（网页界面）首屏默认面向普通科研用户。
3. 普通用户主导航已压缩为输入、配置、检查、运行、结果、导出。
4. 五类输入已经进入统一入口：
   - 小分子
   - 多肽
   - 蛋白质配体
   - 复合物结构
   - 自定义基因组或宏基因组
5. 无输入或科学资源缺失时，候选数为 `0`，输出数为 `0`。
6. Nextflow（流程调度工具）主工作流只调用 `gls-cli`，不直接调用 Python。
7. 当前已补入 schema-bound worker（结构约束计算工人进程）闭环：
   - Rust 生成 worker job（计算任务）JSON。
   - Python worker 返回 worker result（计算结果）JSON。
   - Rust 校验结果字段。
   - Rust 写入 `worker_runtime_summary.tsv`。
   - Python worker 不拥有最终分数和报告语义。

## 3. 禁止回退事项

后续实施不得恢复以下路径或行为：

1. 不恢复旧 `phase_0_*` 验收脚本、工作流、报告生成器或旧产物引用。
2. 不在用户界面写“真实流程”“真正计算”等自证式文案。
3. 不使用固定候选、内置结果表、内置结构、内置分数。
4. 不让用户负责准备数据库路径。
5. 不让普通用户看到 commit（提交号）、remote（远端仓库）、schema（结构约束）、manifest（清单）、hash（哈希）、raw JSON（原始 JSON）、gate（验证门）等开发者概念。
6. 不让 Python worker 写最终排序、最终分数、报告结论或用户解释。
7. 不把第三方工具报告直接放进用户报告。

## 4. 待实现总清单

后续工作分为九个编码任务：

1. 安装闭环：准备完整科学资源目录。
2. 输入归一化：五类输入都生成统一输入记录。
3. 特征计算：五类输入都产生可检索特征。
4. 数据库检索：已知蛋白通道和宏基因组潜在序列通道都可执行。
5. 结构证据：结构、口袋、界面、对接证据进入 GenoLigand 格式。
6. 动力学证据：短分子动力学摘要进入 GenoLigand 格式。
7. 排序与证据：Rust 合并各通道证据并输出候选排序。
8. WebUI 和后端：只展示用户可理解状态。
9. 验证与清理：保证无旧验收、无固定结果、无临时数据残留。

## 5. 任务 A：安装闭环

### 目标

安装命令必须能准备主流程需要的数据库、索引、工具和清单。用户运行主流程前，不需要手动配置数据库路径。

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

### 需要生成的目录结构

安装完成后，科学资源目录必须至少包含：

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

1. 在 `scripts/setup/install_complete_scientific.py` 中统一准备资源根目录。
2. 安装流程读取 `configs/source_catalog.tsv`，逐项检查资源是否存在。
3. 对每个资源写入：
   - 资源名称
   - 来源说明
   - 本地路径
   - 文件数量
   - 字节数
   - SHA-256（安全哈希算法）覆盖率
   - 许可证说明
   - 解析状态
4. 调用 `gls-cli install prepare-science-store` 重建索引。
5. `install_status.json` 只有在所有必需资源和必需工具就绪时才写 `ready`。
6. 否则写 `needs_repair`，并列出用户可理解的修复项。

### 验收条件

1. 科学资源目录存在。
2. `science_store_manifest.tsv` 存在。
3. `install_status.json` 存在。
4. `known_targets.tsv` 存在且行数大于 `0`。
5. `metagenome_targets.tsv` 存在且行数大于 `0`。
6. `structure_targets.tsv` 存在且行数大于 `0`。
7. `tool_versions.tsv` 存在且必需工具状态齐全。
8. 资源缺失时主流程阻塞，候选数为 `0`。

## 6. 任务 B：输入归一化

### 目标

五类输入必须统一变成 `input_normalized_manifest.tsv`，并保留输入来源、类型、规范化值和可计算 token（检索片段）。

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
   - 明显非法字符阻塞。
3. 蛋白质配体：
   - 接收 FASTA（序列文件）、蛋白序列或结构文件。
   - 识别序列长度、氨基酸组成、结构文件引用。
4. 复合物结构：
   - 接收结构文件或复合物描述。
   - 提取链、配体、界面关键词。
5. 自定义基因组或宏基因组：
   - 接收 FASTA、核酸序列、蛋白序列文件。
   - 识别 ORF（开放阅读框）候选。

### 验收条件

1. 五类输入都有测试覆盖。
2. 每类输入至少能产生一条规范化记录，或产生明确阻塞原因。
3. 空输入不会生成候选。
4. 非法输入不会生成候选。
5. 用户界面只显示用户可理解错误。

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
   - 计算简单物化特征：分子量、氢键供体、氢键受体、环数、电荷。
2. 多肽：
   - 计算长度、氨基酸组成、电荷倾向、疏水片段。
   - 如有结构文件，进入结构特征分支。
3. 蛋白质配体：
   - 计算序列 k-mer（固定长度序列片段）。
   - 计算结构域或 motif（序列模式）候选。
4. 复合物：
   - 解析链、配体、接触片段。
   - 生成界面 token。
5. 自定义基因组或宏基因组：
   - 识别 ORF。
   - 翻译蛋白片段。
   - 生成序列 token。

### 验收条件

1. `feature_manifest.tsv` 行数大于输入行数。
2. 每个输入类型至少有一个类型专属 feature_family。
3. 每条特征有 `feature_source`。
4. 无可计算特征时阻塞或给出明确原因，不生成候选。

## 8. 任务 D：已知数据库检索

### 目标

`database match-known` 必须使用安装阶段准备的已知数据库索引，输出候选命中。

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
   - 多肽：肽序列相似性、已知肽蛋白互作。
   - 蛋白质配体：序列域、互作证据、结构相似性。
   - 复合物：链、配体、界面相似性。
   - 自定义基因组或宏基因组：翻译蛋白与已知蛋白相似性。
4. 输出命中分数。
5. 不满足阈值的记录不得写入候选。

### 验收条件

1. 索引缺失时阻塞。
2. 特征缺失时不匹配。
3. 每条命中可以追溯到 source row。
4. 结果不含固定候选。

## 9. 任务 E：宏基因组潜在序列检索

### 目标

`metagenome match-potential` 必须检索宏基因组、微生物蛋白、未注释 ORF 或潜在序列索引。

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
2. 对核酸输入先 ORF 识别，再翻译序列。
3. 对蛋白输入直接做序列检索。
4. 对小分子和多肽，使用口袋、结构或已知互作桥接索引。
5. 对每条潜在序列保留来源：
   - 数据集名
   - 序列编号
   - 原始文件
   - 哈希
6. 输出候选时明确 `candidate_scope = metagenome_potential`。

### 验收条件

1. 宏基因组索引缺失时阻塞。
2. 宏基因组命中与已知蛋白命中分开记录。
3. 潜在序列候选保留序列来源。
4. 不把潜在序列候选写成已知人体蛋白。

## 10. 任务 F：结构、口袋、界面和对接证据

### 目标

`structure evaluate` 必须把结构证据统一转换为 GenoLigand 输出格式。

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
   - 结构清理
   - 链与配体识别
   - 口袋描述
   - 界面残基提取
4. 对可对接的小分子或多肽调用对接引擎。
5. 第三方工具输出必须转换为 GenoLigand 表格。
6. 对接失败写失败原因，不写分数。

### 验收条件

1. 无结构证据时 `structure_patch_summary.tsv` 只写表头或明确无匹配。
2. 对接未运行时 `docking_summary.tsv` 不得出现分数。
3. 对接成功时必须有工具版本、参数摘要、输入结构、输出姿势路径。
4. 第三方原始报告不进入最终报告。

## 11. 任务 G：短分子动力学筛选

### 目标

`dynamics screen` 必须只对满足条件的候选运行短分子动力学，并输出摘要。

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
7. Rust 决定是否把 MD 分量加入最终排序。

### 验收条件

1. OpenMM 不可用时记录不可用，不生成 MD 分数。
2. GPU（图形处理器）不可用时按配置决定阻塞或 CPU（中央处理器）降级。
3. MD 分数由 Rust 写入。
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
   - 已知数据库命中
   - 宏基因组潜在序列命中
   - 结构互作证据
   - 对接证据
   - 动力学证据
2. 使用 `configs/target_ranking.yml` 中的权重。
3. 输出每个分量的分数和来源。
4. 对低置信度候选写风险标记。
5. 报告只写计算预测和证据摘要，不写实验确认。
6. 用户报告不出现文件路径、哈希、结构约束字段、内部表名。

### 验收条件

1. 候选数等于实际排序行数。
2. 每个候选都有 score decomposition。
3. 每个报告 claim 都能映射到 evidence ledger。
4. 空结果报告不声称已有候选。

## 13. 任务 I：WebUI 和后端

### 目标

WebUI 只展示用户需要的状态，开发者信息仅留在内部或高级模式。

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
   - 候选排序、候选详情、证据链、基因组或 ORF、结构与口袋、动力学摘要分区显示。
6. 导出页：
   - 科研报告、候选表、复现实验包、支持包、项目包分开。

### 验收条件

1. 普通界面不出现开发者概念。
2. 无科学资源时运行按钮不能产生候选。
3. 结果页只读取当前运行产物。
4. 用户可下载的文件只来自允许清单。

## 14. 任务 J：验证体系

### 必跑命令

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
node --check webui/app/app.js
python -m py_compile python/genoligand_workers/worker_io.py python/genoligand_workers/openff_worker.py python/genoligand_workers/openmm_worker.py python/genoligand_workers/pdbfixer_worker.py python/genoligand_workers/trajectory_worker.py scripts/setup/install_complete_scientific.py
```

### 必加测试

在 `crates/gls-science/src/lib.rs` 增加或保留以下测试方向：

1. 五类输入都能归一化。
2. 空输入阻塞，候选数为 `0`。
3. 科学资源缺失阻塞，候选数为 `0`。
4. 特征缺失时数据库不匹配。
5. 已知数据库命中可以追溯 source row。
6. 宏基因组候选不被写成已知人体蛋白。
7. worker job/result 结构校验通过。
8. Python 不拥有最终分数。
9. Python 不拥有报告语义。
10. 第三方原始报告不进入用户报告。

在 `crates/gls-server/src/lib.rs` 增加或保留以下测试方向：

1. 用户 API（应用程序接口）不暴露本地路径。
2. 用户 API 不暴露命令行。
3. 用户 API 不暴露内部表名。
4. 用户 API 不暴露结构约束字段。
5. 用户 API 不暴露哈希。
6. 用户 API 阻塞状态为中文可理解文本。

### 浏览器验证

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

## 15. 清理要求

提交前执行：

1. 删除所有临时服务目录。
2. 删除 Python `__pycache__`。
3. 删除临时截图。
4. 删除临时输入和临时运行目录。
5. 确认没有旧验收目录。
6. 确认没有旧 `phase_0_*` 文件。
7. 确认没有根目录临时包、压缩包、日志包。

检查命令：

```text
rg --files . -g "*phase_0*" -g "*acceptance*" -g "*review_packet*" -g "!target/**" -g "!.git/**"
rg -n "phase_0_|acceptance|review_packet" . -S -g "!target/**" -g "!.git/**"
git status --short --untracked-files=all
```

## 16. 完成判定

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

## 17. 建议提交顺序

建议分三次提交，便于回滚：

1. `Implement science store installation closure`
   - 安装闭环
   - 数据库清单
   - 工具版本清单
2. `Implement all-input feature and matching pipeline`
   - 五类输入特征
   - 已知数据库检索
   - 宏基因组检索
3. `Wire structure dynamics evidence and user result views`
   - 结构证据
   - 动力学证据
   - 用户结果页
   - 导出

每次提交前都运行第 14 节命令。
