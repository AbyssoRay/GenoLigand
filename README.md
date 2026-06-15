# GenoLigand

GenoLigand 是本地运行的靶点发现科研软件。它的核心目标是：用户输入一个物质、序列或结构后，软件先计算其相互作用特征，再在已知蛋白数据库和宏基因组潜在序列空间中比对，给出可能靶点的计算预测、排序和证据链。

GenoLigand 不声称实验验证、体内结合、药效或机制结论。所有结论都必须表述为计算预测。

## 一条命令准备并启动

```powershell
cd D:\Project\rust\GenoLigand
python scripts\setup\install_complete_scientific.py --start-webui
```

打开：

```text
http://127.0.0.1:19090/
```

该命令会选择用户数据目录作为科学仓库，检查工具、数据库来源、索引和修复动作，然后启动本地 WebUI。

## 准备科学仓库

安装流程负责准备数据库、索引、工具版本和许可证清单。普通用户不需要配置数据库路径。维护人员可单独运行：

```powershell
python scripts\setup\install_complete_scientific.py
```

科学仓库必须包含：

- `science_store_manifest.tsv`
- `indexes/known_targets.tsv`
- `indexes/metagenome_targets.tsv`
- `indexes/tool_versions.tsv`

缺少研究输入、数据库索引或必要工具时，主流程阻塞并输出零候选。

## 主流程

```text
输入归一化 -> 特征计算 -> 已知数据库比对 -> 宏基因组潜在序列比对 -> 结构/口袋/对接/动力学证据 -> 联合靶点排序 -> 证据报告
```

CLI（命令行界面）入口：

```powershell
cargo run -p gls-cli -- pipeline run `
  --input-manifest path\to\input_manifest.json `
  --science-store D:\GenoLigandData\science_store `
  --run-dir outputs\current_pipeline
```

支持的输入类型：

- 小分子：SMILES、SDF、MOL2、化合物名称或登记号。
- 多肽：短肽序列、修饰说明或结构文件。
- 蛋白质配体：FASTA、蛋白序列或结构文件。
- 复合物：已有蛋白-配体结构。
- 自定义基因组/宏基因组：基因组、宏基因组、转录组或蛋白序列文件。

## 主要输出

- `input_normalized_manifest.json`
- `feature_manifest.json`
- `known_database_matches.tsv`
- `metagenome_potential_matches.tsv`
- `structure_patch_summary.tsv`
- `docking_summary.tsv`
- `md_summary.tsv`
- `target_candidate_ranking.tsv`
- `candidate_detail.json`
- `score_decomposition.tsv`
- `evidence_ledger.tsv`
- `research_report.md`
