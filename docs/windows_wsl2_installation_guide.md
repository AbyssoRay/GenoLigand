# Windows + WSL2 安装说明

## 前置条件

- Windows 11
- PowerShell
- Rust 工具链
- Git
- WSL2 Linux 后端
- Java 和 Nextflow
- Python 3.11 或更新版本
- 需要动力学筛选时，准备 NVIDIA 驱动、WSL CUDA 和 OpenMM

## 科学资源由安装流程准备

普通用户不手动配置数据库。管理员或高级模式负责准备科学仓库：

```powershell
cargo run -p gls-cli -- install prepare-science-store --science-store D:\GenoLigandData\science_store
```

完整安装应准备 ChEMBL、BindingDB、UniProt、RCSB、BioLiP2、PLINDER、Binding MOAD，以及宏基因组潜在序列索引、结构索引、口袋索引和工具版本清单。

## 启动 WebUI

```powershell
cd D:\Project\rust\GenoLigand
cargo run -p gls-server -- --host 127.0.0.1 --port 19090
```

打开：

```text
http://127.0.0.1:19090/
```

## 阻塞策略

如果输入、科学仓库、已知数据库索引、宏基因组索引或必要工具缺失，GenoLigand 只显示阻塞原因，候选数量保持为 0。

