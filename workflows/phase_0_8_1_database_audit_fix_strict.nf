nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_8_1_database_audit_fix_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_8_1_database_audit_fix_strict'
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
  "\$PYTHON" "\$ROOT/scripts/phase081/phase081_cli.py" workflow-step --artifact-dir "\$ART" --stage "${stageName}"
  echo ok > ${markerName}
  """
}

process LOAD_PHASE08_ARTIFACTS {
  tag 'phase_0_8_1_load_phase08_artifacts'
  publishDir "${params.outdir}/markers", mode: 'copy'
  output:
  path 'LOAD_PHASE08_ARTIFACTS.done'
  script:
  markerScript('LOAD_PHASE08_ARTIFACTS', 'LOAD_PHASE08_ARTIFACTS.done')
}

process HASH_PHASE08_INPUTS_BEFORE {
  tag 'phase_0_8_1_hash_phase08_inputs_before'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'HASH_PHASE08_INPUTS_BEFORE.done'
  script:
  markerScript('HASH_PHASE08_INPUTS_BEFORE', 'HASH_PHASE08_INPUTS_BEFORE.done')
}

process LOAD_PHASE071_REGRESSION {
  tag 'phase_0_8_1_load_phase071_regression'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'LOAD_PHASE071_REGRESSION.done'
  script:
  markerScript('LOAD_PHASE071_REGRESSION', 'LOAD_PHASE071_REGRESSION.done')
}

process RECOMPUTE_SOURCE_GATE_VALUES {
  tag 'phase_0_8_1_recompute_source_gate_values'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RECOMPUTE_SOURCE_GATE_VALUES.done'
  script:
  markerScript('RECOMPUTE_SOURCE_GATE_VALUES', 'RECOMPUTE_SOURCE_GATE_VALUES.done')
}

process RECOMPUTE_WAREHOUSE_TABLE_COUNTS {
  tag 'phase_0_8_1_recompute_warehouse_table_counts'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RECOMPUTE_WAREHOUSE_TABLE_COUNTS.done'
  script:
  markerScript('RECOMPUTE_WAREHOUSE_TABLE_COUNTS', 'RECOMPUTE_WAREHOUSE_TABLE_COUNTS.done')
}

process RECOMPUTE_BENCHMARK_COUNTS {
  tag 'phase_0_8_1_recompute_benchmark_counts'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RECOMPUTE_BENCHMARK_COUNTS.done'
  script:
  markerScript('RECOMPUTE_BENCHMARK_COUNTS', 'RECOMPUTE_BENCHMARK_COUNTS.done')
}

process RUN_BENCHMARK_COUNT_SANITY {
  tag 'phase_0_8_1_run_benchmark_count_sanity'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_BENCHMARK_COUNT_SANITY.done'
  script:
  markerScript('RUN_BENCHMARK_COUNT_SANITY', 'RUN_BENCHMARK_COUNT_SANITY.done')
}

process RUN_MAPPING_SANITY {
  tag 'phase_0_8_1_run_mapping_sanity'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_MAPPING_SANITY.done'
  script:
  markerScript('RUN_MAPPING_SANITY', 'RUN_MAPPING_SANITY.done')
}

process RUN_OPENBINDBENCH_AUDIT {
  tag 'phase_0_8_1_run_openbindbench_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_OPENBINDBENCH_AUDIT.done'
  script:
  markerScript('RUN_OPENBINDBENCH_AUDIT', 'RUN_OPENBINDBENCH_AUDIT.done')
}

process RUN_DUDEZ_OR_DUDE_AUDIT {
  tag 'phase_0_8_1_run_dudez_or_dude_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_DUDEZ_OR_DUDE_AUDIT.done'
  script:
  markerScript('RUN_DUDEZ_OR_DUDE_AUDIT', 'RUN_DUDEZ_OR_DUDE_AUDIT.done')
}

process RUN_LEAKAGE_AUDIT_EXPANDED {
  tag 'phase_0_8_1_run_leakage_audit_expanded'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_LEAKAGE_AUDIT_EXPANDED.done'
  script:
  markerScript('RUN_LEAKAGE_AUDIT_EXPANDED', 'RUN_LEAKAGE_AUDIT_EXPANDED.done')
}

process BUILD_SOURCE_GATE_EVIDENCE_LEDGER {
  tag 'phase_0_8_1_build_source_gate_evidence_ledger'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_SOURCE_GATE_EVIDENCE_LEDGER.done'
  script:
  markerScript('BUILD_SOURCE_GATE_EVIDENCE_LEDGER', 'BUILD_SOURCE_GATE_EVIDENCE_LEDGER.done')
}

process BUILD_METRIC_LEVEL_EVIDENCE_LEDGER {
  tag 'phase_0_8_1_build_metric_level_evidence_ledger'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_METRIC_LEVEL_EVIDENCE_LEDGER.done'
  script:
  markerScript('BUILD_METRIC_LEVEL_EVIDENCE_LEDGER', 'BUILD_METRIC_LEVEL_EVIDENCE_LEDGER.done')
}

process RUN_DIRECT_REPORT_VALUE_CHECK {
  tag 'phase_0_8_1_run_direct_report_value_check'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_DIRECT_REPORT_VALUE_CHECK.done'
  script:
  markerScript('RUN_DIRECT_REPORT_VALUE_CHECK', 'RUN_DIRECT_REPORT_VALUE_CHECK.done')
}

process RUN_CLAIM_LINT {
  tag 'phase_0_8_1_run_claim_lint'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_CLAIM_LINT.done'
  script:
  markerScript('RUN_CLAIM_LINT', 'RUN_CLAIM_LINT.done')
}

process RUN_PLACEHOLDER_AUDIT {
  tag 'phase_0_8_1_run_placeholder_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_PLACEHOLDER_AUDIT.done'
  script:
  markerScript('RUN_PLACEHOLDER_AUDIT', 'RUN_PLACEHOLDER_AUDIT.done')
}

process HASH_OUTPUTS {
  tag 'phase_0_8_1_hash_outputs'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'HASH_OUTPUTS.done'
  script:
  markerScript('HASH_OUTPUTS', 'HASH_OUTPUTS.done')
}

process COMPARE_SEMANTIC_HASHES {
  tag 'phase_0_8_1_compare_semantic_hashes'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'COMPARE_SEMANTIC_HASHES.done'
  script:
  markerScript('COMPARE_SEMANTIC_HASHES', 'COMPARE_SEMANTIC_HASHES.done')
}

process MASTER_VALIDATE_PHASE081 {
  tag 'phase_0_8_1_master_validate_phase081'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'MASTER_VALIDATE_PHASE081.done'
  script:
  markerScript('MASTER_VALIDATE_PHASE081', 'MASTER_VALIDATE_PHASE081.done')
}

process RENDER_PHASE081_REPORT {
  tag 'phase_0_8_1_render_phase081_report'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RENDER_PHASE081_REPORT.done'
  script:
  markerScript('RENDER_PHASE081_REPORT', 'RENDER_PHASE081_REPORT.done')
}

workflow {
  p0 = LOAD_PHASE08_ARTIFACTS()
  p1 = HASH_PHASE08_INPUTS_BEFORE(p0)
  p2 = LOAD_PHASE071_REGRESSION(p1)
  p3 = RECOMPUTE_SOURCE_GATE_VALUES(p2)
  p4 = RECOMPUTE_WAREHOUSE_TABLE_COUNTS(p3)
  p5 = RECOMPUTE_BENCHMARK_COUNTS(p4)
  p6 = RUN_BENCHMARK_COUNT_SANITY(p5)
  p7 = RUN_MAPPING_SANITY(p6)
  p8 = RUN_OPENBINDBENCH_AUDIT(p7)
  p9 = RUN_DUDEZ_OR_DUDE_AUDIT(p8)
  p10 = RUN_LEAKAGE_AUDIT_EXPANDED(p9)
  p11 = BUILD_SOURCE_GATE_EVIDENCE_LEDGER(p10)
  p12 = BUILD_METRIC_LEVEL_EVIDENCE_LEDGER(p11)
  p13 = RUN_DIRECT_REPORT_VALUE_CHECK(p12)
  p14 = RUN_CLAIM_LINT(p13)
  p15 = RUN_PLACEHOLDER_AUDIT(p14)
  p16 = HASH_OUTPUTS(p15)
  p17 = COMPARE_SEMANTIC_HASHES(p16)
  p18 = MASTER_VALIDATE_PHASE081(p17)
  RENDER_PHASE081_REPORT(p18)
}

