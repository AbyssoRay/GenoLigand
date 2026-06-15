# Linux 安装说明

## 前置条件

- Linux 主机
- Rust 工具链
- Git
- Java 和 Nextflow
- Python 3.11 或更新版本
- 可选 NVIDIA GPU、CUDA 和 OpenMM

## 准备科学仓库

```bash
python scripts/setup/install_complete_scientific.py
```

科学仓库由安装流程维护。普通用户只通过 WebUI 提交输入。

## 启动服务

```bash
cd /path/to/GenoLigand
python scripts/setup/install_complete_scientific.py --start-webui
```

打开：

```text
http://127.0.0.1:19090/
```

## 主流程

GenoLigand 的工作流是：

```text
输入归一化 -> 特征计算 -> 已知数据库比对 -> 宏基因组潜在序列比对 -> 结构/口袋/对接/动力学证据 -> 联合靶点排序 -> 证据报告
```

缺少研究输入或科学仓库时，流程阻塞，不生成候选。
