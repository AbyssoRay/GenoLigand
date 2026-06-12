# GenoLigand 0.9.1 Local WebUI User Guide

## 1. 启动本地服务 / Start The Local Service

在仓库根目录运行：

```powershell
python scripts\phase09\start_phase_0_9_webui.py
```

默认浏览器入口：

```text
http://127.0.0.1:19090/
```

The browser talks to the local `gls-server`. Scientific execution is coordinated through `gls-cli`; WebUI pages read and write local manifests, run state, artifact indexes, and exported reports.

## 2. 项目管理 / Project Management

项目页用于创建、打开、复制、删除、清理研究项目，并继续上次运行。每个项目保存：

- project manifest
- input manifest
- database profile
- run state and historical run summaries
- result artifact index
- report export paths

Use one project per research question so that inputs, database versions, run artifacts, evidence rows, and GitHub sync records remain traceable.

## 3. 输入向导 / Input Wizard

向导页面按科研工作流组织：

1. Project basics
2. Molecule type
3. Small molecule, peptide, protein ligand, or existing complex
4. Species and genome
5. Search scope
6. Database profile
7. Run profile
8. Preflight
9. Review and start

Small molecules support SMILES, SDF, MOL2, PDB, compound name, and CAS helper fields. Peptides support single-letter sequences and FASTA. Protein ligands support FASTA, PDB/mmCIF, predicted structures, and chain selection. Existing complexes support receptor-ligand structures with ligand chain, residue name, or binding-region selection.

Advanced parameters stay folded by default. Open them only when a run requires explicit scientific overrides.

## 4. 物种、数据库和范围 / Species, Databases, And Scope

内置物种配置可直接选择。自定义物种可提供 genome FASTA、GTF/GFF3、transcript FASTA 和可选 VCF；预检查会记录路径、index、hash 和版本状态。

Database Registry shows configured paths, versions, hashes, index status, missing items, storage use, and repair actions for genome, annotation, transcripts, BioLiP2, UniProt, ChEMBL/BindingDB, and structure cache.

Search scope choices are conservative CDS search, expanded transcript search, and whole-genome discovery search.

## 5. 运行和监控 / Run And Monitor

运行模式包括快速环境检查、标准靶点筛选、结构证据增强筛选和物理中筛完整运行。物理中筛会使用 GPU/OpenMM when available and configured.

Before launch, Preflight checks molecule format, sequence validity, structure files, database paths, genome index, disk space, write permissions, OpenMM, CUDA, Nextflow, Git remote, and GitHub push permission.

Run Monitor displays current stage, completed steps, current command, Nextflow run ID, log tail, CPU/GPU/VRAM/RAM/disk status, output directory, failure reason, and resume state.

## 6. 结果查看 / Result Review

Result pages read the selected run artifacts:

- Candidate Ranking: rank, candidate ID, gene, ORF, coordinates, context, scores, and risk flags.
- Candidate Detail: source, ORF sequence, genomic position, frame, flanking regions, evidence modules, risk flags, and experimental validation guidance.
- Genome/ORF Context: CDS, alternative reading frame, UTR ORF, lncRNA ORF, intron ORF, antisense ORF, pseudogene ORF, overlapping ORF, and variant-created ORF.
- Structure and Patch Viewer: structures, binding patches, key residues, docking poses, ligand positions, and interaction residues.
- MD Summary: RMSD, RMSF, ligand drift, hydrogen bond occupancy, contact occupancy, interaction persistence, risk flags, and score decomposition.
- Evidence Ledger: source file, manifest, schema, tool version, artifact hash, validation state, and report section mapping.

Empty result pages do not invent candidate rows or biological conclusions. They show the required fields and wait for computed artifacts from the selected run.

## 7. 报告导出 / Report Export

Report Export writes HTML, Markdown, CSV/TSV candidate table, JSON machine-readable report, artifact bundle manifest, run manifest, tool version manifest, diagnostics report, and GitHub push report.

Generated files are local artifacts. Keep the exported bundle together with the run manifest and evidence ledger when sharing results for review.

## 8. 环境诊断和 GitHub 同步 / Diagnostics And GitHub Sync

Diagnostics checks WSL2, Linux, Rust binary, `gls-cli`, `gls-server`, Nextflow, Java, Python worker, OpenMM, CUDA, NVIDIA device, database paths, write permission, disk space, Git remote, GitHub push permission, and sample project template status.

GitHub Sync shows branch, local commit hash, remote branch, push state, push log, and remote HEAD verification. Use it only when you intend to record and push the current project state.

## 9. Sample Project 模板 / Sample Project Template

The sample project template verifies installation, database wiring, GPU/OpenMM discovery, Nextflow resume behavior, WebUI flow, input capture, preflight, run creation, monitoring, result browsing, report export, and diagnostics.

It contains no precomputed candidates and no biological conclusions. Use it for environment verification, then create a separate research project for real input data.
