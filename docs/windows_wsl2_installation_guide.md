# Windows + WSL2 安装说明

## 前置条件

普通用户使用发行包时只需要：

- Windows 11
- PowerShell
- Python 3.11 或更新版本
- WSL2 Linux 后端
- 需要动力学筛选时，本机具备 NVIDIA 驱动和 WSL CUDA 能力

发行包应包含 `gls-cli`、`gls-server` 和科学工具准备入口。源码开发者才需要 Rust 工具链、Git、Java、Nextflow 或手动调试 Python 环境。

## 科学资源由安装流程准备

普通用户不手动配置数据库。安装流程读取随包资源清单，下载、校验、索引并记录来源；高级模式只用于维护或修复科学仓库：

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

如果科学仓库、索引、工具记录或大型资源尚未闭合，安装命令会返回需要处理的资源和下一步动作，WebUI 不会启动。

## 阻塞策略

如果输入、科学仓库、已知数据库索引、宏基因组索引、必要工具、安装状态或科学资源状态缺失，GenoLigand 只显示阻塞原因，候选数量保持为 0。
