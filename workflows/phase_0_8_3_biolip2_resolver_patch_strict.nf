nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_8_3_biolip2_resolver_patch_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_8_3_biolip2_resolver_patch_strict'
params.python = params.python ?: 'python'

def rootWin() {
  java.nio.file.Paths.get(launchDir.toString()).toAbsolutePath().normalize().toString().replace('\\', '/')
}

def winPath(p) {
  def s = p.toString().replace('\\', '/')
  if (s ==~ /^[A-Za-z]:.*/) {
    return s
  }
  return "${rootWin()}/${s}".replace('\\', '/')
}

def markerScript(stageName, markerName) {
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase083/phase083_cli.py" workflow-step --artifact-dir "\$ART" --stage "${stageName}"
  echo ok > ${markerName}
  """
}

process LOAD_PHASE082_DIAGNOSIS {
  tag 'phase_0_8_3_load_phase082_diagnosis'
  publishDir "${params.outdir}/markers", mode: 'copy'
  output: path 'LOAD_PHASE082_DIAGNOSIS.done'
  script: markerScript('LOAD_PHASE082_DIAGNOSIS', 'LOAD_PHASE082_DIAGNOSIS.done')
}

process LOAD_PHASE082_ARTIFACTS {
  tag 'phase_0_8_3_load_phase082_artifacts'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'LOAD_PHASE082_ARTIFACTS.done'
  script: markerScript('LOAD_PHASE082_ARTIFACTS', 'LOAD_PHASE082_ARTIFACTS.done')
}

process BUILD_DATA_ROOT_MANIFEST {
  tag 'phase_0_8_3_build_data_root_manifest'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_DATA_ROOT_MANIFEST.done'
  script: markerScript('BUILD_DATA_ROOT_MANIFEST', 'BUILD_DATA_ROOT_MANIFEST.done')
}

process BUILD_STRUCTURE_RESOLVER_INDEX {
  tag 'phase_0_8_3_build_structure_resolver_index'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_STRUCTURE_RESOLVER_INDEX.done'
  script: markerScript('BUILD_STRUCTURE_RESOLVER_INDEX', 'BUILD_STRUCTURE_RESOLVER_INDEX.done')
}

process RESOLVE_BIOLIP2_STRUCTURES {
  tag 'phase_0_8_3_resolve_biolip2_structures'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RESOLVE_BIOLIP2_STRUCTURES.done'
  script: markerScript('RESOLVE_BIOLIP2_STRUCTURES', 'RESOLVE_BIOLIP2_STRUCTURES.done')
}

process AUDIT_TRUE_MISSING_STRUCTURES {
  tag 'phase_0_8_3_audit_true_missing_structures'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'AUDIT_TRUE_MISSING_STRUCTURES.done'
  script: markerScript('AUDIT_TRUE_MISSING_STRUCTURES', 'AUDIT_TRUE_MISSING_STRUCTURES.done')
}

process RUN_MMCIF_PARSER_MICROBENCH {
  tag 'phase_0_8_3_run_mmcif_parser_microbench'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_MMCIF_PARSER_MICROBENCH.done'
  script: markerScript('RUN_MMCIF_PARSER_MICROBENCH', 'RUN_MMCIF_PARSER_MICROBENCH.done')
}

process EXTRACT_BIOLIP2_PATCHES {
  tag 'phase_0_8_3_extract_biolip2_patches'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'EXTRACT_BIOLIP2_PATCHES.done'
  script: markerScript('EXTRACT_BIOLIP2_PATCHES', 'EXTRACT_BIOLIP2_PATCHES.done')
}

process SUMMARIZE_PATCH_FAILURES {
  tag 'phase_0_8_3_summarize_patch_failures'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'SUMMARIZE_PATCH_FAILURES.done'
  script: markerScript('SUMMARIZE_PATCH_FAILURES', 'SUMMARIZE_PATCH_FAILURES.done')
}

process VALIDATE_BIOLIP2_PATCH_THRESHOLDS {
  tag 'phase_0_8_3_validate_biolip2_patch_thresholds'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'VALIDATE_BIOLIP2_PATCH_THRESHOLDS.done'
  script: markerScript('VALIDATE_BIOLIP2_PATCH_THRESHOLDS', 'VALIDATE_BIOLIP2_PATCH_THRESHOLDS.done')
}

process RUN_PHASE071_REGRESSION {
  tag 'phase_0_8_3_run_phase071_regression'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_PHASE071_REGRESSION.done'
  script: markerScript('RUN_PHASE071_REGRESSION', 'RUN_PHASE071_REGRESSION.done')
}

process RUN_PHASE082_NON_BIOLIP2_REGRESSION {
  tag 'phase_0_8_3_run_phase082_non_biolip2_regression'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_PHASE082_NON_BIOLIP2_REGRESSION.done'
  script: markerScript('RUN_PHASE082_NON_BIOLIP2_REGRESSION', 'RUN_PHASE082_NON_BIOLIP2_REGRESSION.done')
}

process RENDER_PHASE083_REPORT {
  tag 'phase_0_8_3_render_phase083_report'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RENDER_PHASE083_REPORT.done'
  script: markerScript('RENDER_PHASE083_REPORT', 'RENDER_PHASE083_REPORT.done')
}

process MASTER_VALIDATE_PHASE083 {
  tag 'phase_0_8_3_master_validate_phase083'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'MASTER_VALIDATE_PHASE083.done'
  script: markerScript('MASTER_VALIDATE_PHASE083', 'MASTER_VALIDATE_PHASE083.done')
}

process COLLECT_PHASE083_ARCHIVE {
  tag 'phase_0_8_3_collect_phase083_archive'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'COLLECT_PHASE083_ARCHIVE.done'
  script: markerScript('COLLECT_PHASE083_ARCHIVE', 'COLLECT_PHASE083_ARCHIVE.done')
}

workflow {
  p0 = LOAD_PHASE082_DIAGNOSIS()
  p1 = LOAD_PHASE082_ARTIFACTS(p0)
  p2 = BUILD_DATA_ROOT_MANIFEST(p1)
  p3 = BUILD_STRUCTURE_RESOLVER_INDEX(p2)
  p4 = RESOLVE_BIOLIP2_STRUCTURES(p3)
  p5 = AUDIT_TRUE_MISSING_STRUCTURES(p4)
  p6 = RUN_MMCIF_PARSER_MICROBENCH(p5)
  p7 = EXTRACT_BIOLIP2_PATCHES(p6)
  p8 = SUMMARIZE_PATCH_FAILURES(p7)
  p9 = VALIDATE_BIOLIP2_PATCH_THRESHOLDS(p8)
  p10 = RUN_PHASE071_REGRESSION(p9)
  p11 = RUN_PHASE082_NON_BIOLIP2_REGRESSION(p10)
  p12 = RENDER_PHASE083_REPORT(p11)
  p13 = MASTER_VALIDATE_PHASE083(p12)
  COLLECT_PHASE083_ARCHIVE(p13)
}

