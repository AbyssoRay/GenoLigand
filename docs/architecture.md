# GenoLigand 仓库职责边界

GenoLigand 的产品职责是从用户输入的物质、序列或结构出发，计算相互作用特征，并在已知蛋白与宏基因组潜在序列空间中排序候选靶点。仓库结构必须服务这个主流程，普通用户界面保持干净，科学结果必须可追溯到输入、数据库、运行和产物。

## 目录职责

| 目录 | 唯一职责 | 不承担的职责 |
| --- | --- | --- |
| `crates/` | Rust 产品主逻辑、命令行、后端服务、主流程编排、候选排序、证据链、报告语义 | 直接保存运行产物或数据库状态 |
| `python/` | 受 schema 约束的科学计算 worker | 最终排序、最终分数、报告语义、用户主入口 |
| `scripts/setup/` | 安装、环境准备、工具与数据库准备的内部实现 | 普通用户主入口文案 |
| `configs/` | 科学资源目录和同步规则 | 本机数据库版本、hash、索引产物 |
| `schemas/` | Rust、WebUI、Python worker、项目包之间的数据契约 | 运行产物、示例结果 |
| `webui/` | 普通科研用户界面 | 开发者验收面板、内置候选、固定结果 |
| `workflows/` | Nextflow 调度入口 | 直接调用科学脚本或拥有科学语义 |
| `docs/` | 用户文档和架构说明 | 本机运行状态 |
| `tests/` | 安装、真实性、WebUI、端到端和干净发布防回归检查；`tests/clean_release/check_clean_release.py` 是统一清洁门禁 | 生产数据存放或清理执行 |

## 产品规则

- 普通用户首次打开必须看到干净工作台，不出现示例项目、测试 run、固定候选或开发者调试状态。
- 普通用户界面不暴露 GitHub、commit、remote、schema、manifest、hash、raw JSON、gate、route 等开发者概念。
- 无输入、无数据库、无工具或无运行产物时，结果页保持空状态并给出修复动作，不展示固定候选或预生成分数。
- 所有候选、分数、结构、动力学摘要、证据链和报告都必须能追溯到 project、run、input、database、candidate 和 artifact。
- Rust 拥有主流程、最终排序、最终分数、证据链、报告语义和用户可见解释。
- Python worker 执行科学计算任务，并通过 schema 化 job/result 与 Rust 交换数据。
- Nextflow 通过 `gls-cli` 进入主流程。
- WebUI 和 `gls-server` 只通过正式 API 与 `gls-cli` 进入主流程；安装修复入口可以调用 `scripts/setup/install_complete_scientific.py`。

## 发布边界

仓库只保存代码、配置规则、schema、文档和测试。安装后生成的数据库 manifest、工具版本、hash、索引和运行产物进入用户 science store 或项目运行目录，不写入仓库配置目录。发布前必须执行 `python tests/clean_release/check_clean_release.py --workspace-root <repo> --science-store <store> --runtime-dir <runtime>`，确认仓库没有构建与运行缓存，science store 没有无身份 partial 或旧 staging，runtime 没有失效 PID 或旧验收项目。门禁按 manifest 引用关系分类候选，并保留有效下载源、MMseqs/Foldseek 数据库及力场文件。

产品发布身份由 Cargo workspace 的 `version`、Git `commit` 和 `<version>+<commit>` 形式的 `build_id` 共同确定。CLI、服务端、运行时 manifest 和健康接口必须给出同一组值；发布标签使用 `v<version>` 并指向最终发布提交。
