nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_8_4_openmm_short_md_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_8_4_openmm_short_md_strict'
params.python = params.python ?: 'python'

def rootWin() {
  java.nio.file.Paths.get(launchDir.toString()).toAbsolutePath().normalize().toString().replace('\\', '/')
}

def winPath(p) {
  def s = p.toString().replace('\\', '/')
  if (s.startsWith('/')) {
    return s
  }
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
  "\$PYTHON" "\$ROOT/scripts/phase084/phase084_cli.py" workflow-step --artifact-dir "\$ART" --stage "${stageName}" || true
  echo ok > ${markerName}
  """
}

process PHASE_INPUT_CHECK {
  publishDir "${params.outdir}/markers", mode: 'copy'
  output: path 'PHASE_INPUT_CHECK.done'
  script: markerScript('PHASE_INPUT_CHECK', 'PHASE_INPUT_CHECK.done')
}

process OPENMM_PLATFORM_PROBE {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'OPENMM_PLATFORM_PROBE.done'
  script: markerScript('OPENMM_PLATFORM_PROBE', 'OPENMM_PLATFORM_PROBE.done')
}

process CPU_SMOKE_MD {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'CPU_SMOKE_MD.done'
  script: markerScript('CPU_SMOKE_MD', 'CPU_SMOKE_MD.done')
}

process SELECT_MD_COMPLEXES {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'SELECT_MD_COMPLEXES.done'
  script: markerScript('SELECT_MD_COMPLEXES', 'SELECT_MD_COMPLEXES.done')
}

process PREPARE_RECEPTOR {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'PREPARE_RECEPTOR.done'
  script: markerScript('PREPARE_RECEPTOR', 'PREPARE_RECEPTOR.done')
}

process PARAMETERIZE_LIGAND {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'PARAMETERIZE_LIGAND.done'
  script: markerScript('PARAMETERIZE_LIGAND', 'PARAMETERIZE_LIGAND.done')
}

process BUILD_OPENMM_SYSTEM {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'BUILD_OPENMM_SYSTEM.done'
  script: markerScript('BUILD_OPENMM_SYSTEM', 'BUILD_OPENMM_SYSTEM.done')
}

process RUN_OPENMM_MINIMIZATION {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_OPENMM_MINIMIZATION.done'
  script: markerScript('RUN_OPENMM_MINIMIZATION', 'RUN_OPENMM_MINIMIZATION.done')
}

process RUN_OPENMM_EQUILIBRATION {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_OPENMM_EQUILIBRATION.done'
  script: markerScript('RUN_OPENMM_EQUILIBRATION', 'RUN_OPENMM_EQUILIBRATION.done')
}

process RUN_OPENMM_PRODUCTION_MD {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RUN_OPENMM_PRODUCTION_MD.done'
  script: markerScript('RUN_OPENMM_PRODUCTION_MD', 'RUN_OPENMM_PRODUCTION_MD.done')
}

process ANALYZE_TRAJECTORY_RMSD {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'ANALYZE_TRAJECTORY_RMSD.done'
  script: markerScript('ANALYZE_TRAJECTORY_RMSD', 'ANALYZE_TRAJECTORY_RMSD.done')
}

process ANALYZE_INTERACTION_OCCUPANCY {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'ANALYZE_INTERACTION_OCCUPANCY.done'
  script: markerScript('ANALYZE_INTERACTION_OCCUPANCY', 'ANALYZE_INTERACTION_OCCUPANCY.done')
}

process SUMMARIZE_MD_STABILITY {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'SUMMARIZE_MD_STABILITY.done'
  script: markerScript('SUMMARIZE_MD_STABILITY', 'SUMMARIZE_MD_STABILITY.done')
}

process COMPARE_DECOY_MD {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'COMPARE_DECOY_MD.done'
  script: markerScript('COMPARE_DECOY_MD', 'COMPARE_DECOY_MD.done')
}

process RENDER_MD_REPORT {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'RENDER_MD_REPORT.done'
  script: markerScript('RENDER_MD_REPORT', 'RENDER_MD_REPORT.done')
}

process MASTER_VALIDATE_PHASE084 {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'MASTER_VALIDATE_PHASE084.done'
  script: markerScript('MASTER_VALIDATE_PHASE084', 'MASTER_VALIDATE_PHASE084.done')
}

process COLLECT_ACCEPTANCE_ARCHIVE {
  publishDir "${params.outdir}/markers", mode: 'copy'
  input: path marker
  output: path 'COLLECT_ACCEPTANCE_ARCHIVE.done'
  script: markerScript('COLLECT_ACCEPTANCE_ARCHIVE', 'COLLECT_ACCEPTANCE_ARCHIVE.done')
}

workflow {
  p0 = PHASE_INPUT_CHECK()
  p1 = OPENMM_PLATFORM_PROBE(p0)
  p2 = CPU_SMOKE_MD(p1)
  p3 = SELECT_MD_COMPLEXES(p2)
  p4 = PREPARE_RECEPTOR(p3)
  p5 = PARAMETERIZE_LIGAND(p4)
  p6 = BUILD_OPENMM_SYSTEM(p5)
  p7 = RUN_OPENMM_MINIMIZATION(p6)
  p8 = RUN_OPENMM_EQUILIBRATION(p7)
  p9 = RUN_OPENMM_PRODUCTION_MD(p8)
  p10 = ANALYZE_TRAJECTORY_RMSD(p9)
  p11 = ANALYZE_INTERACTION_OCCUPANCY(p10)
  p12 = SUMMARIZE_MD_STABILITY(p11)
  p13 = COMPARE_DECOY_MD(p12)
  p14 = RENDER_MD_REPORT(p13)
  p15 = MASTER_VALIDATE_PHASE084(p14)
  COLLECT_ACCEPTANCE_ARCHIVE(p15)
}
