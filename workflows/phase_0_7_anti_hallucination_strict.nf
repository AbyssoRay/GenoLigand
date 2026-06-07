nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_7_1_audit_fix_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_7_1_audit_fix_strict'
params.phase07_dir = params.phase07_dir ?: 'acceptance_artifacts_phase_0_7_anti_hallucination_strict'
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
  PHASE07="${winPath(params.phase07_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/audit/workflow_step_phase_0_7_1.py" --artifact-dir "\$ART" --phase07-dir "\$PHASE07" --stage "${stageName}"
  echo ok > ${markerName}
  """
}

process LIGAND_PREPARE {
  tag 'phase_0_7_1_ligand_prepare_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  output:
  path 'LIGAND_PREPARE.done'
  script:
  markerScript('LIGAND_PREPARE', 'LIGAND_PREPARE.done')
}

process RECEPTOR_PREPARE {
  tag 'phase_0_7_1_receptor_prepare_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RECEPTOR_PREPARE.done'
  script:
  markerScript('RECEPTOR_PREPARE', 'RECEPTOR_PREPARE.done')
}

process DOCKING_BOX_PREPARE {
  tag 'phase_0_7_1_docking_box_prepare_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DOCKING_BOX_PREPARE.done'
  script:
  markerScript('DOCKING_BOX_PREPARE', 'DOCKING_BOX_PREPARE.done')
}

process VINA_DOCKING {
  tag 'phase_0_7_1_vina_docking_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'VINA_DOCKING.done'
  script:
  markerScript('VINA_DOCKING', 'VINA_DOCKING.done')
}

process REDOCKING_RMSD {
  tag 'phase_0_7_1_redocking_rmsd_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'REDOCKING_RMSD.done'
  script:
  markerScript('REDOCKING_RMSD', 'REDOCKING_RMSD.done')
}

process INTERACTION_FINGERPRINT {
  tag 'phase_0_7_1_interaction_fingerprint_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'INTERACTION_FINGERPRINT.done'
  script:
  markerScript('INTERACTION_FINGERPRINT', 'INTERACTION_FINGERPRINT.done')
}

process DECOY_CALIBRATION {
  tag 'phase_0_7_1_decoy_calibration_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'DECOY_CALIBRATION.done'
  script:
  markerScript('DECOY_CALIBRATION', 'DECOY_CALIBRATION.done')
}

process EVIDENCE_LEDGER {
  tag 'phase_0_7_1_evidence_ledger_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'EVIDENCE_LEDGER.done'
  script:
  markerScript('EVIDENCE_LEDGER', 'EVIDENCE_LEDGER.done')
}

process REPORT_LINT {
  tag 'phase_0_7_1_report_lint_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'REPORT_LINT.done'
  script:
  markerScript('REPORT_LINT', 'REPORT_LINT.done')
}

process PHASE07_REPORT {
  tag 'phase_0_7_1_report_audit'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'PHASE07_REPORT.done'
  script:
  markerScript('PHASE07_REPORT', 'PHASE07_REPORT.done')
}

workflow {
  ligand_ch = LIGAND_PREPARE()
  receptor_ch = RECEPTOR_PREPARE(ligand_ch)
  box_ch = DOCKING_BOX_PREPARE(receptor_ch)
  docking_ch = VINA_DOCKING(box_ch)
  redock_ch = REDOCKING_RMSD(docking_ch)
  ifp_ch = INTERACTION_FINGERPRINT(redock_ch)
  decoy_ch = DECOY_CALIBRATION(ifp_ch)
  evidence_ch = EVIDENCE_LEDGER(decoy_ch)
  lint_ch = REPORT_LINT(evidence_ch)
  PHASE07_REPORT(lint_ch)
}
