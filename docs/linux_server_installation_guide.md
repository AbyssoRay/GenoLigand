# Linux 安装说明

## 前置条件

普通用户使用发行包时只需要：

- Linux 主机
- Python 3.11 或更新版本
- 需要动力学筛选时，本机具备 NVIDIA GPU、CUDA 和 OpenMM 可用条件

发行包应包含 `gls-cli`、`gls-server` 和科学工具准备入口。源码开发者才需要 Rust 工具链、Git、Java、Nextflow 或手动调试 Python 环境。

## 准备科学仓库

```bash
python scripts/setup/install_complete_scientific.py
```

科学仓库由安装流程维护。安装流程读取随包资源清单，下载、校验、索引并记录来源；普通用户只通过 WebUI 提交输入。

## 启动服务

```bash
cd /path/to/GenoLigand
python scripts/setup/install_complete_scientific.py --start-webui
```

打开：

```text
http://127.0.0.1:19090/
```

如果科学仓库、索引、工具记录或大型资源尚未闭合，安装命令会返回需要处理的资源和下一步动作，WebUI 不会启动。

## 主流程

GenoLigand 的工作流是：

```text
输入归一化 -> 特征计算 -> 已知数据库比对 -> 宏基因组潜在序列比对 -> 结构/口袋/对接/动力学证据 -> 联合靶点排序 -> 证据报告
```

缺少研究输入、科学仓库、索引、工具记录、安装状态或科学资源状态时，流程阻塞，不生成候选。
