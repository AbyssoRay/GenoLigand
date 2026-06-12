# GenoLigand 0.9.1

GenoLigand 是本地运行的配体感知靶点发现科研工作台。0.9.1 提供正式 WebUI：用户在浏览器中创建项目、配置输入、管理数据库、启动本地工作流、查看候选与证据、导出报告、运行环境诊断，并在需要时同步到 GitHub。计算后端由本机 `gls-server` 提供服务，科学执行入口由 `gls-cli` 统一调度。

GenoLigand 输出的是候选优先级和证据整理，不直接声称实验结合、生物活性、机制或临床相关性。

## 启动本地 WebUI

在仓库根目录执行：

```powershell
cd D:\Project\rust\GenoLigand
python scripts\phase09\start_phase_0_9_webui.py
```

脚本会构建并启动 `gls-server`，默认监听：

```text
http://127.0.0.1:19090/
```

如需使用现有二进制：

```powershell
python scripts\phase09\start_phase_0_9_webui.py --no-build
```

如端口被占用：

```powershell
python scripts\phase09\start_phase_0_9_webui.py --port 19190
```

保持启动终端开启即可使用 WebUI，按 `Ctrl+C` 停止服务。

## WebUI 功能

- 项目管理：创建、打开、复制、删除、清理研究项目，继续上次运行并查看历史运行摘要。
- 输入向导：配置项目基本信息、分子类型、物种、数据库、搜索范围、运行模式、预检查和最终启动。
- 小分子输入：支持 SMILES、SDF、MOL2、PDB、化合物名称和 CAS 号。
- 多肽输入：支持单字母氨基酸序列和 FASTA，并提供 IKVAV、YIGSR、RGD 快速填入。
- 蛋白质配体输入：支持 FASTA、PDB/mmCIF、已有预测结构和链选择。
- 已有复合物输入：支持 receptor-ligand complex，并可指定配体链、小分子 residue name 或结合区域。
- 物种与基因组：支持预设物种和自定义 genome FASTA、GTF/GFF3、transcript FASTA、可选 VCF。
- 数据库管理：查看 genome、annotation、transcript、BioLiP2、UniProt、ChEMBL/BindingDB 和结构缓存的路径、版本、hash、索引、缺失项、空间和修复动作。
- 运行模式：快速环境自检、标准靶点筛选、结构证据增强筛选、物理中筛完整运行。
- 预检查：覆盖分子格式、序列合法性、结构文件、数据库、基因组索引、磁盘、写入权限、OpenMM、CUDA、Nextflow、Git remote 和 GitHub 权限。
- 运行监控：显示阶段、命令、Nextflow run ID、日志尾部、CPU/GPU/显存/内存/磁盘占用、输出目录、失败原因和恢复状态。
- 结果查看：候选排序、候选详情、Genome/ORF context、结构与 patch、MD 摘要、evidence ledger。
- 报告导出：HTML、Markdown、CSV/TSV、JSON、artifact bundle、run manifest、tool version manifest、diagnostics report、GitHub push report。
- 环境诊断：覆盖 WSL2、Linux、Rust binary、gls-cli、gls-server、Nextflow、Java、Python worker、OpenMM、CUDA、NVIDIA 设备、数据库路径、写入权限、磁盘、Git remote、GitHub 推送权限和环境验证项目模板。
- GitHub 同步：显示 branch、commit hash、remote branch、push 状态、push 日志和 remote HEAD 验证。

## 页面语言和界面配色

WebUI 固定使用暗色界面。右上角提供 `中文 / EN` 按钮，所有主要页面都支持中文和 English。

## 环境验证项目模板

WebUI 提供环境验证项目模板，用于验证安装、数据库、GPU/OpenMM、Nextflow 恢复、WebUI 流程、输入、预检查、运行、监控、结果浏览、报告导出和诊断是否可用。模板只保存验证配置，不包含预计算候选、不包含实验结论，也不会替代用户自己的研究项目。

## 安装指南

更完整的使用说明位于：

```text
docs/local_webui_user_guide.md
docs/windows_wsl2_installation_guide.md
docs/linux_server_installation_guide.md
```

## 本地服务接口

常用本地接口：

```text
GET  /api/health
GET  /api/version
GET  /api/projects
POST /api/projects
GET  /api/databases
GET  /api/diagnostics
GET  /api/github/status
```

健康接口返回 `ready` 表示本地服务已可响应浏览器请求。

## 停止服务

在启动终端按 `Ctrl+C`。如需手动查找进程：

```powershell
Get-Process gls-server
```

停止指定进程：

```powershell
Stop-Process -Name gls-server
```

## 许可证

GenoLigand 按 `EULA.txt` 中的项目许可证分发。
