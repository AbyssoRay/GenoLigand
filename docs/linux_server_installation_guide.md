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
cargo run -p gls-cli -- install prepare-science-store --science-store /data/genoligand/science_store
```

科学仓库由安装流程维护。普通用户只通过 WebUI 提交输入。

## 启动服务

```bash
cd /path/to/GenoLigand
cargo run -p gls-server -- --host 127.0.0.1 --port 19090
```

打开：

```text
http://127.0.0.1:19090/
```

## 主流程

GenoLigand 的生产工作流是：

```text
输入归一化 -> 特征计算 -> 已知数据库比对 -> 宏基因组潜在序列比对 -> 结构/口袋/对接/动力学证据 -> 联合靶点排序 -> 证据报告
```

缺少真实输入或真实科学仓库时，流程阻塞，不生成候选。

