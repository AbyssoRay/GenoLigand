nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_8_2_mapping_benchmark_storage_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_8_2_mapping_benchmark_storage_strict'
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
  "\$PYTHON" "\$ROOT/scripts/phase082/phase082_cli.py" workflow-step --artifact-dir "\$ART" --stage "${stageName}"
  echo ok > ${markerName}
  """
}

process LOAD_PHASE081_ARTIFACTS {
  tag 'phase_0_8_2_load_phase081_artifacts'
  publishDir "${params.outdir}/markers", mode: 'copy'
  output: path 'LOAD_PHASE081_ARTIFACTS.done'
  script: markerScript('LOAD_PHASE081_ARTIFACTS', 'LOAD_PHASE081_ARTIFACTS.done')
}

process RECOMPUTE_TARGET_MAPPING {
  tag 'phase_0_8_2_recompute_target_mapping'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RECOMPUTE_TARGET_MAPPING.done'
  script: markerScript('RECOMPUTE_TARGET_MAPPING', 'RECOMPUTE_TARGET_MAPPING.done')
}

process BUILD_RCSB_CHAIN_MAPPING {
  tag 'phase_0_8_2_build_rcsb_chain_mapping'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_RCSB_CHAIN_MAPPING.done'
  script: markerScript('BUILD_RCSB_CHAIN_MAPPING', 'BUILD_RCSB_CHAIN_MAPPING.done')
}

process BUILD_BIOLIP2_PATCHES {
  tag 'phase_0_8_2_build_biolip2_patches'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_BIOLIP2_PATCHES.done'
  script: markerScript('BUILD_BIOLIP2_PATCHES', 'BUILD_BIOLIP2_PATCHES.done')
}

process REBUILD_MEASURED_ACTIVITY_BENCHMARK {
  tag 'phase_0_8_2_rebuild_measured_activity_benchmark'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'REBUILD_MEASURED_ACTIVITY_BENCHMARK.done'
  script: markerScript('REBUILD_MEASURED_ACTIVITY_BENCHMARK', 'REBUILD_MEASURED_ACTIVITY_BENCHMARK.done')
}

process EXPORT_VS_ACTIVE_DECOY_BENCHMARK {
  tag 'phase_0_8_2_export_vs_active_decoy_benchmark'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'EXPORT_VS_ACTIVE_DECOY_BENCHMARK.done'
  script: markerScript('EXPORT_VS_ACTIVE_DECOY_BENCHMARK', 'EXPORT_VS_ACTIVE_DECOY_BENCHMARK.done')
}

process SEPARATE_BENCHMARK_CLASSES {
  tag 'phase_0_8_2_separate_benchmark_classes'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'SEPARATE_BENCHMARK_CLASSES.done'
  script: markerScript('SEPARATE_BENCHMARK_CLASSES', 'SEPARATE_BENCHMARK_CLASSES.done')
}

process RUN_COUNT_SANITY_CHECKS {
  tag 'phase_0_8_2_run_count_sanity_checks'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_COUNT_SANITY_CHECKS.done'
  script: markerScript('RUN_COUNT_SANITY_CHECKS', 'RUN_COUNT_SANITY_CHECKS.done')
}

process BUILD_SQL_QUERY_LEDGER {
  tag 'phase_0_8_2_build_sql_query_ledger'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_SQL_QUERY_LEDGER.done'
  script: markerScript('BUILD_SQL_QUERY_LEDGER', 'BUILD_SQL_QUERY_LEDGER.done')
}

process BUILD_METRIC_DEFINITION_REGISTRY {
  tag 'phase_0_8_2_build_metric_definition_registry'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_METRIC_DEFINITION_REGISTRY.done'
  script: markerScript('BUILD_METRIC_DEFINITION_REGISTRY', 'BUILD_METRIC_DEFINITION_REGISTRY.done')
}

process BUILD_EVIDENCE_LEDGER {
  tag 'phase_0_8_2_build_evidence_ledger'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_EVIDENCE_LEDGER.done'
  script: markerScript('BUILD_EVIDENCE_LEDGER', 'BUILD_EVIDENCE_LEDGER.done')
}

process INVENTORY_STORAGE_BEFORE {
  tag 'phase_0_8_2_inventory_storage_before'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'INVENTORY_STORAGE_BEFORE.done'
  script: markerScript('INVENTORY_STORAGE_BEFORE', 'INVENTORY_STORAGE_BEFORE.done')
}

process BUILD_CLEANUP_DRY_RUN {
  tag 'phase_0_8_2_build_cleanup_dry_run'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_CLEANUP_DRY_RUN.done'
  script: markerScript('BUILD_CLEANUP_DRY_RUN', 'BUILD_CLEANUP_DRY_RUN.done')
}

process EXECUTE_CLEANUP_PLAN {
  tag 'phase_0_8_2_execute_cleanup_plan'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'EXECUTE_CLEANUP_PLAN.done'
  script: markerScript('EXECUTE_CLEANUP_PLAN', 'EXECUTE_CLEANUP_PLAN.done')
}

process INVENTORY_STORAGE_AFTER {
  tag 'phase_0_8_2_inventory_storage_after'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'INVENTORY_STORAGE_AFTER.done'
  script: markerScript('INVENTORY_STORAGE_AFTER', 'INVENTORY_STORAGE_AFTER.done')
}

process RUN_POST_CLEANUP_VALIDATION {
  tag 'phase_0_8_2_run_post_cleanup_validation'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_POST_CLEANUP_VALIDATION.done'
  script: markerScript('RUN_POST_CLEANUP_VALIDATION', 'RUN_POST_CLEANUP_VALIDATION.done')
}

process RENDER_PHASE082_REPORT {
  tag 'phase_0_8_2_render_phase082_report'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RENDER_PHASE082_REPORT.done'
  script: markerScript('RENDER_PHASE082_REPORT', 'RENDER_PHASE082_REPORT.done')
}

process RUN_PHASE082_MASTER_VALIDATION {
  tag 'phase_0_8_2_run_phase082_master_validation'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_PHASE082_MASTER_VALIDATION.done'
  script: markerScript('RUN_PHASE082_MASTER_VALIDATION', 'RUN_PHASE082_MASTER_VALIDATION.done')
}

process COLLECT_PHASE082_ACCEPTANCE_ARCHIVE {
  tag 'phase_0_8_2_collect_phase082_acceptance_archive'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'COLLECT_PHASE082_ACCEPTANCE_ARCHIVE.done'
  script: markerScript('COLLECT_PHASE082_ACCEPTANCE_ARCHIVE', 'COLLECT_PHASE082_ACCEPTANCE_ARCHIVE.done')
}

workflow {
  p0 = LOAD_PHASE081_ARTIFACTS()
  p1 = RECOMPUTE_TARGET_MAPPING(p0)
  p2 = BUILD_RCSB_CHAIN_MAPPING(p1)
  p3 = BUILD_BIOLIP2_PATCHES(p2)
  p4 = REBUILD_MEASURED_ACTIVITY_BENCHMARK(p3)
  p5 = EXPORT_VS_ACTIVE_DECOY_BENCHMARK(p4)
  p6 = SEPARATE_BENCHMARK_CLASSES(p5)
  p7 = RUN_COUNT_SANITY_CHECKS(p6)
  p8 = BUILD_SQL_QUERY_LEDGER(p7)
  p9 = BUILD_METRIC_DEFINITION_REGISTRY(p8)
  p10 = BUILD_EVIDENCE_LEDGER(p9)
  p11 = INVENTORY_STORAGE_BEFORE(p10)
  p12 = BUILD_CLEANUP_DRY_RUN(p11)
  p13 = EXECUTE_CLEANUP_PLAN(p12)
  p14 = INVENTORY_STORAGE_AFTER(p13)
  p15 = RUN_POST_CLEANUP_VALIDATION(p14)
  p16 = RENDER_PHASE082_REPORT(p15)
  p17 = RUN_PHASE082_MASTER_VALIDATION(p16)
  COLLECT_PHASE082_ACCEPTANCE_ARCHIVE(p17)
}

