# GenoLigand

GenoLigand 是本地运行的科研软件，用于从小分子、多肽、蛋白质配体、复合物结构或自定义基因组/宏基因组输入出发，计算相互作用特征，并在已知蛋白数据库与潜在序列空间中排序候选靶点。

## 安装并启动

Windows / WSL2：

```powershell
.\install.ps1
```

Linux：

```bash
./install.sh
```

安装入口会准备 `gls-cli`、`gls-server`、Python worker、科学工具、数据库仓库和索引。完成后打开本地 WebUI，输入自己的研究对象，按 `Input -> Configure -> Check -> Run -> Results -> Export` 完成一次运行。

## 用户流程

1. 在输入区选择小分子、多肽、蛋白质配体、复合物结构或自定义基因组/宏基因组。
2. 配置数据库范围、搜索范围、结构与动力学证据选项。
3. 运行检查，确认输入、数据库、运行环境、GPU、存储和导出条件可用。
4. 启动运行并在 Run Monitor 查看阶段、下一步动作、资源状态、日志和产物树。
5. 在 Results 查看候选排序、候选详情、证据链、Genome/ORF、Structure/Patch、MD Summary。
6. 导出科研报告、候选表、可复现包、项目包或支持包。

## 科学结果边界

GenoLigand 输出的是计算预测结果。报告应表达候选假设、证据强度、优先级和后续实验验证建议，不表达已完成实验确认。

每个候选、分数、结构摘要、动力学摘要、证据链和报告条目都必须追溯到 project、run、input、database、candidate 和 artifact。缺少有效输入、数据库、工具或运行产物时，界面显示阻塞状态和修复动作，不生成候选结果。

## 开发结构

仓库职责见 [docs/architecture.md](docs/architecture.md)。Nextflow 调度 `gls-cli`，Rust 负责主流程、候选排序、证据链和报告语义，Python worker 执行 schema 约束的科学计算任务。

常用开发检查：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
python -m py_compile scripts/setup/install_complete_scientific.py
```
