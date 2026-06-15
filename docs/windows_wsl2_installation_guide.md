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

普通用户不手动配置数据库。发行包或管理员预置数据源目录；高级模式可单独维护科学仓库：

```powershell
python scripts\setup\install_complete_scientific.py
```

完整安装应准备 ChEMBL、BindingDB、UniProt、RCSB、BioLiP2、PLINDER、Binding MOAD，以及宏基因组潜在序列索引、结构索引、口袋索引、工具版本清单、安装状态和科学资源状态。

## 启动 WebUI

```powershell
cd D:\Project\rust\GenoLigand
python scripts\setup\install_complete_scientific.py --start-webui
```

打开：

```text
http://127.0.0.1:19090/
```

## 阻塞策略

如果输入、科学仓库、已知数据库索引、宏基因组索引、必要工具、安装状态或科学资源状态缺失，GenoLigand 只显示阻塞原因，候选数量保持为 0。
