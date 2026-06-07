nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_8_full_database_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_8_full_database_strict'
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
  "\$PYTHON" "\$ROOT/scripts/phase08/workflow_step_phase_0_8.py" --artifact-dir "\$ART" --stage "${stageName}"
  echo ok > ${markerName}
  """
}

process COLLECT_PHASE071_INPUTS {
  tag 'phase_0_8_collect_phase071_inputs'
  publishDir "${params.outdir}/markers", mode: 'copy'
  output:
  path 'COLLECT_PHASE071_INPUTS.done'
  script:
  markerScript('COLLECT_PHASE071_INPUTS', 'COLLECT_PHASE071_INPUTS.done')
}

process DOWNLOAD_CHEMBL {
  tag 'phase_0_8_download_chembl'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_CHEMBL.done'
  script:
  markerScript('DOWNLOAD_CHEMBL', 'DOWNLOAD_CHEMBL.done')
}

process DOWNLOAD_BINDINGDB {
  tag 'phase_0_8_download_bindingdb'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_BINDINGDB.done'
  script:
  markerScript('DOWNLOAD_BINDINGDB', 'DOWNLOAD_BINDINGDB.done')
}

process DOWNLOAD_UNIPROT_HUMAN {
  tag 'phase_0_8_download_uniprot_human'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_UNIPROT_HUMAN.done'
  script:
  markerScript('DOWNLOAD_UNIPROT_HUMAN', 'DOWNLOAD_UNIPROT_HUMAN.done')
}

process DOWNLOAD_AFDB_HUMAN {
  tag 'phase_0_8_download_afdb_human'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_AFDB_HUMAN.done'
  script:
  markerScript('DOWNLOAD_AFDB_HUMAN', 'DOWNLOAD_AFDB_HUMAN.done')
}

process QUERY_DOWNLOAD_RCSB_LIGAND_BOUND {
  tag 'phase_0_8_query_download_rcsb_ligand_bound'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'QUERY_DOWNLOAD_RCSB_LIGAND_BOUND.done'
  script:
  markerScript('QUERY_DOWNLOAD_RCSB_LIGAND_BOUND', 'QUERY_DOWNLOAD_RCSB_LIGAND_BOUND.done')
}

process DOWNLOAD_BIOLIP2 {
  tag 'phase_0_8_download_biolip2'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_BIOLIP2.done'
  script:
  markerScript('DOWNLOAD_BIOLIP2', 'DOWNLOAD_BIOLIP2.done')
}

process DOWNLOAD_PLINDER {
  tag 'phase_0_8_download_plinder'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_PLINDER.done'
  script:
  markerScript('DOWNLOAD_PLINDER', 'DOWNLOAD_PLINDER.done')
}

process DOWNLOAD_BINDING_MOAD {
  tag 'phase_0_8_download_binding_moad'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_BINDING_MOAD.done'
  script:
  markerScript('DOWNLOAD_BINDING_MOAD', 'DOWNLOAD_BINDING_MOAD.done')
}

process RECONSTRUCT_OPENBINDBENCH {
  tag 'phase_0_8_reconstruct_openbindbench'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RECONSTRUCT_OPENBINDBENCH.done'
  script:
  markerScript('RECONSTRUCT_OPENBINDBENCH', 'RECONSTRUCT_OPENBINDBENCH.done')
}

process DOWNLOAD_DUDE_Z {
  tag 'phase_0_8_download_dude_z'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOWNLOAD_DUDE_Z.done'
  script:
  markerScript('DOWNLOAD_DUDE_Z', 'DOWNLOAD_DUDE_Z.done')
}

process OPTIONAL_DOWNLOAD_PDBBIND {
  tag 'phase_0_8_optional_download_pdbbind'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'OPTIONAL_DOWNLOAD_PDBBIND.done'
  script:
  markerScript('OPTIONAL_DOWNLOAD_PDBBIND', 'OPTIONAL_DOWNLOAD_PDBBIND.done')
}

process NORMALIZE_CHEMBL {
  tag 'phase_0_8_normalize_chembl'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'NORMALIZE_CHEMBL.done'
  script:
  markerScript('NORMALIZE_CHEMBL', 'NORMALIZE_CHEMBL.done')
}

process NORMALIZE_BINDINGDB {
  tag 'phase_0_8_normalize_bindingdb'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'NORMALIZE_BINDINGDB.done'
  script:
  markerScript('NORMALIZE_BINDINGDB', 'NORMALIZE_BINDINGDB.done')
}

process NORMALIZE_UNIPROT {
  tag 'phase_0_8_normalize_uniprot'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'NORMALIZE_UNIPROT.done'
  script:
  markerScript('NORMALIZE_UNIPROT', 'NORMALIZE_UNIPROT.done')
}

process NORMALIZE_AFDB {
  tag 'phase_0_8_normalize_afdb'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'NORMALIZE_AFDB.done'
  script:
  markerScript('NORMALIZE_AFDB', 'NORMALIZE_AFDB.done')
}

process NORMALIZE_RCSB {
  tag 'phase_0_8_normalize_rcsb'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'NORMALIZE_RCSB.done'
  script:
  markerScript('NORMALIZE_RCSB', 'NORMALIZE_RCSB.done')
}

process NORMALIZE_BIOLIP2 {
  tag 'phase_0_8_normalize_biolip2'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'NORMALIZE_BIOLIP2.done'
  script:
  markerScript('NORMALIZE_BIOLIP2', 'NORMALIZE_BIOLIP2.done')
}

process NORMALIZE_OPEN_BENCHMARKS {
  tag 'phase_0_8_normalize_open_benchmarks'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'NORMALIZE_OPEN_BENCHMARKS.done'
  script:
  markerScript('NORMALIZE_OPEN_BENCHMARKS', 'NORMALIZE_OPEN_BENCHMARKS.done')
}

process STANDARDIZE_COMPOUNDS {
  tag 'phase_0_8_standardize_compounds'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'STANDARDIZE_COMPOUNDS.done'
  script:
  markerScript('STANDARDIZE_COMPOUNDS', 'STANDARDIZE_COMPOUNDS.done')
}

process MAP_TARGETS_TO_UNIPROT {
  tag 'phase_0_8_map_targets_to_uniprot'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'MAP_TARGETS_TO_UNIPROT.done'
  script:
  markerScript('MAP_TARGETS_TO_UNIPROT', 'MAP_TARGETS_TO_UNIPROT.done')
}

process MAP_STRUCTURES_TO_UNIPROT {
  tag 'phase_0_8_map_structures_to_uniprot'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'MAP_STRUCTURES_TO_UNIPROT.done'
  script:
  markerScript('MAP_STRUCTURES_TO_UNIPROT', 'MAP_STRUCTURES_TO_UNIPROT.done')
}

process BUILD_DUCKDB_WAREHOUSE {
  tag 'phase_0_8_build_duckdb_warehouse'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_DUCKDB_WAREHOUSE.done'
  script:
  markerScript('BUILD_DUCKDB_WAREHOUSE', 'BUILD_DUCKDB_WAREHOUSE.done')
}

process BUILD_SEQUENCE_INDEX {
  tag 'phase_0_8_build_sequence_index'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_SEQUENCE_INDEX.done'
  script:
  markerScript('BUILD_SEQUENCE_INDEX', 'BUILD_SEQUENCE_INDEX.done')
}

process BUILD_FOLDSEEK_AFDB_INDEX {
  tag 'phase_0_8_build_foldseek_afdb_index'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_FOLDSEEK_AFDB_INDEX.done'
  script:
  markerScript('BUILD_FOLDSEEK_AFDB_INDEX', 'BUILD_FOLDSEEK_AFDB_INDEX.done')
}

process BUILD_FOLDSEEK_RCSB_INDEX {
  tag 'phase_0_8_build_foldseek_rcsb_index'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_FOLDSEEK_RCSB_INDEX.done'
  script:
  markerScript('BUILD_FOLDSEEK_RCSB_INDEX', 'BUILD_FOLDSEEK_RCSB_INDEX.done')
}

process EXTRACT_BENCHMARK_PAIRS {
  tag 'phase_0_8_extract_benchmark_pairs'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'EXTRACT_BENCHMARK_PAIRS.done'
  script:
  markerScript('EXTRACT_BENCHMARK_PAIRS', 'EXTRACT_BENCHMARK_PAIRS.done')
}

process BUILD_BENCHMARK_SPLITS {
  tag 'phase_0_8_build_benchmark_splits'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_BENCHMARK_SPLITS.done'
  script:
  markerScript('BUILD_BENCHMARK_SPLITS', 'BUILD_BENCHMARK_SPLITS.done')
}

process AUDIT_LEAKAGE {
  tag 'phase_0_8_audit_leakage'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'AUDIT_LEAKAGE.done'
  script:
  markerScript('AUDIT_LEAKAGE', 'AUDIT_LEAKAGE.done')
}

process EVALUATE_PHASE07_CONTROLS {
  tag 'phase_0_8_evaluate_phase07_controls'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'EVALUATE_PHASE07_CONTROLS.done'
  script:
  markerScript('EVALUATE_PHASE07_CONTROLS', 'EVALUATE_PHASE07_CONTROLS.done')
}

process RENDER_PHASE08_REPORTS {
  tag 'phase_0_8_render_phase08_reports'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RENDER_PHASE08_REPORTS.done'
  script:
  markerScript('RENDER_PHASE08_REPORTS', 'RENDER_PHASE08_REPORTS.done')
}

process MASTER_VALIDATE_PHASE08 {
  tag 'phase_0_8_master_validate_phase08'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'MASTER_VALIDATE_PHASE08.done'
  script:
  markerScript('MASTER_VALIDATE_PHASE08', 'MASTER_VALIDATE_PHASE08.done')
}

process COLLECT_ACCEPTANCE_ARCHIVE {
  tag 'phase_0_8_collect_acceptance_archive'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'COLLECT_ACCEPTANCE_ARCHIVE.done'
  script:
  markerScript('COLLECT_ACCEPTANCE_ARCHIVE', 'COLLECT_ACCEPTANCE_ARCHIVE.done')
}

workflow {
  p0 = COLLECT_PHASE071_INPUTS()
  p1 = DOWNLOAD_CHEMBL(p0)
  p2 = DOWNLOAD_BINDINGDB(p1)
  p3 = DOWNLOAD_UNIPROT_HUMAN(p2)
  p4 = DOWNLOAD_AFDB_HUMAN(p3)
  p5 = QUERY_DOWNLOAD_RCSB_LIGAND_BOUND(p4)
  p6 = DOWNLOAD_BIOLIP2(p5)
  p7 = DOWNLOAD_PLINDER(p6)
  p8 = DOWNLOAD_BINDING_MOAD(p7)
  p9 = RECONSTRUCT_OPENBINDBENCH(p8)
  p10 = DOWNLOAD_DUDE_Z(p9)
  p11 = OPTIONAL_DOWNLOAD_PDBBIND(p10)
  p12 = NORMALIZE_CHEMBL(p11)
  p13 = NORMALIZE_BINDINGDB(p12)
  p14 = NORMALIZE_UNIPROT(p13)
  p15 = NORMALIZE_AFDB(p14)
  p16 = NORMALIZE_RCSB(p15)
  p17 = NORMALIZE_BIOLIP2(p16)
  p18 = NORMALIZE_OPEN_BENCHMARKS(p17)
  p19 = STANDARDIZE_COMPOUNDS(p18)
  p20 = MAP_TARGETS_TO_UNIPROT(p19)
  p21 = MAP_STRUCTURES_TO_UNIPROT(p20)
  p22 = BUILD_DUCKDB_WAREHOUSE(p21)
  p23 = BUILD_SEQUENCE_INDEX(p22)
  p24 = BUILD_FOLDSEEK_AFDB_INDEX(p23)
  p25 = BUILD_FOLDSEEK_RCSB_INDEX(p24)
  p26 = EXTRACT_BENCHMARK_PAIRS(p25)
  p27 = BUILD_BENCHMARK_SPLITS(p26)
  p28 = AUDIT_LEAKAGE(p27)
  p29 = EVALUATE_PHASE07_CONTROLS(p28)
  p30 = RENDER_PHASE08_REPORTS(p29)
  p31 = MASTER_VALIDATE_PHASE08(p30)
  COLLECT_ACCEPTANCE_ARCHIVE(p31)
}
