nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_7_anti_hallucination_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_7_anti_hallucination_strict'
params.phase06_dir = params.phase06_dir ?: 'acceptance_artifacts_phase_0_6_structure_patch_strict'
params.phase05_dir = params.phase05_dir ?: 'acceptance_artifacts_phase_0_5_gpu_strict'
params.python = params.python ?: 'python'
params.vina_bin = params.vina_bin ?: (System.getenv('GENOLIGAND_PHASE07_VINA') ?: 'vina')
params.ligand_prep_command = params.ligand_prep_command ?: (System.getenv('GENOLIGAND_PHASE07_LIGAND_PREP') ?: 'mk_prepare_ligand.py')
params.receptor_prep_command = params.receptor_prep_command ?: (System.getenv('GENOLIGAND_PHASE07_RECEPTOR_PREP') ?: 'mk_prepare_receptor.py')
params.obabel_bin = params.obabel_bin ?: (System.getenv('GENOLIGAND_PHASE07_OBABEL') ?: 'obabel')

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

process REGRESSION_PHASE03_TO_06 {
  tag 'phase_0_7_regression'
  publishDir "${params.outdir}/markers", mode: 'copy'
  output:
  path 'REGRESSION_PHASE03_TO_06.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PHASE06="${winPath(params.phase06_dir)}"
  PHASE05="${winPath(params.phase05_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  mkdir -p "\$ART/metrics"
  "\$PYTHON" "\$ROOT/scripts/phase07/check_phase06_inputs.py" --artifact-dir "\$ART" --phase06-dir "\$PHASE06" --phase05-dir "\$PHASE05"
  echo ok > REGRESSION_PHASE03_TO_06.done
  """
}

process VALIDATE_LIGAND_INPUT {
  tag 'phase_0_7_ligand_input'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'VALIDATE_LIGAND_INPUT.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  mkdir -p "\$ART/outputs/ligands" "\$ART/outputs/controls"
  "\$PYTHON" "\$ROOT/scripts/phase07/validate_ligand_input.py" --artifact-dir "\$ART"
  "\$PYTHON" "\$ROOT/scripts/phase07/select_positive_control.py" --artifact-dir "\$ART"
  echo ok > VALIDATE_LIGAND_INPUT.done
  """
}

process STANDARDIZE_LIGAND {
  tag 'phase_0_7_standardize_ligand'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'STANDARDIZE_LIGAND.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/standardize_ligands.py" --artifact-dir "\$ART"
  echo ok > STANDARDIZE_LIGAND.done
  """
}

process GENERATE_CONFORMERS {
  tag 'phase_0_7_generate_conformers'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'GENERATE_CONFORMERS.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  "\$PYTHON" "\$ROOT/scripts/validation/check_ligand_standardization_phase_0_7.py" --artifact-dir "\$ART"
  echo ok > GENERATE_CONFORMERS.done
  """
}

process PREPARE_LIGAND_FOR_DOCKING {
  tag 'phase_0_7_prepare_ligand'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'PREPARE_LIGAND_FOR_DOCKING.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/prepare_ligands_for_docking.py" --artifact-dir "\$ART" --ligand-prep-command "${params.ligand_prep_command}"
  echo ok > PREPARE_LIGAND_FOR_DOCKING.done
  """
}

process PREPARE_RECEPTORS {
  tag 'phase_0_7_prepare_receptors'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'PREPARE_RECEPTORS.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PHASE06="${winPath(params.phase06_dir)}"
  PHASE05="${winPath(params.phase05_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/prepare_receptors.py" --artifact-dir "\$ART" --phase06-dir "\$PHASE06" --phase05-dir "\$PHASE05" --receptor-prep-command "${params.receptor_prep_command}" --obabel-bin "${params.obabel_bin}"
  echo ok > PREPARE_RECEPTORS.done
  """
}

process GENERATE_DOCKING_BOXES {
  tag 'phase_0_7_generate_boxes'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'GENERATE_DOCKING_BOXES.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PHASE06="${winPath(params.phase06_dir)}"
  PHASE05="${winPath(params.phase05_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/generate_docking_boxes.py" --artifact-dir "\$ART" --phase06-dir "\$PHASE06" --phase05-dir "\$PHASE05"
  echo ok > GENERATE_DOCKING_BOXES.done
  """
}

process RUN_POSITIVE_CONTROL_REDOCKING {
  tag 'phase_0_7_run_docking'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_POSITIVE_CONTROL_REDOCKING.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/run_docking.py" --artifact-dir "\$ART" --vina-bin "${params.vina_bin}" --obabel-bin "${params.obabel_bin}"
  echo ok > RUN_POSITIVE_CONTROL_REDOCKING.done
  """
}

process RUN_QUERY_DOCKING {
  tag 'phase_0_7_query_docking_control_only'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_QUERY_DOCKING.done'
  script:
  """
  echo control_only_no_query_ligand > RUN_QUERY_DOCKING.done
  """
}

process RUN_NEGATIVE_CONTROL_DOCKING {
  tag 'phase_0_7_negative_control_docking'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RUN_NEGATIVE_CONTROL_DOCKING.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  "\$PYTHON" "\$ROOT/scripts/validation/check_real_docking_runs_phase_0_7.py" --artifact-dir "\$ART"
  echo ok > RUN_NEGATIVE_CONTROL_DOCKING.done
  """
}

process PARSE_DOCKING_OUTPUTS {
  tag 'phase_0_7_parse_docking'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'PARSE_DOCKING_OUTPUTS.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  "\$PYTHON" "\$ROOT/scripts/phase07/parse_docking_outputs.py" --artifact-dir "\$ART"
  echo ok > PARSE_DOCKING_OUTPUTS.done
  """
}

process COMPUTE_REDOCKING_RMSD {
  tag 'phase_0_7_redocking_rmsd'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'COMPUTE_REDOCKING_RMSD.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/compute_redocking_rmsd.py" --artifact-dir "\$ART"
  echo ok > COMPUTE_REDOCKING_RMSD.done
  """
}

process GENERATE_INTERACTION_FINGERPRINTS {
  tag 'phase_0_7_interactions'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'GENERATE_INTERACTION_FINGERPRINTS.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/generate_interaction_fingerprints.py" --artifact-dir "\$ART"
  echo ok > GENERATE_INTERACTION_FINGERPRINTS.done
  """
}

process CALIBRATE_DECOYS {
  tag 'phase_0_7_calibrate_decoys'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'CALIBRATE_DECOYS.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/calibrate_decoys.py" --artifact-dir "\$ART"
  "\$PYTHON" "\$ROOT/scripts/phase07/build_target_prior.py" --artifact-dir "\$ART"
  echo ok > CALIBRATE_DECOYS.done
  """
}

process BUILD_EVIDENCE_LEDGER {
  tag 'phase_0_7_ledger'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'BUILD_EVIDENCE_LEDGER.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/build_evidence_ledger.py" --artifact-dir "\$ART"
  echo ok > BUILD_EVIDENCE_LEDGER.done
  """
}

process RENDER_PHASE07_REPORT {
  tag 'phase_0_7_report'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'RENDER_PHASE07_REPORT.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/render_candidate_report_phase_0_7.py" --artifact-dir "\$ART"
  "\$PYTHON" "\$ROOT/scripts/phase07/build_evidence_ledger.py" --artifact-dir "\$ART"
  echo ok > RENDER_PHASE07_REPORT.done
  """
}

process ANTI_HALLUCINATION_LINT {
  tag 'phase_0_7_lint'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'ANTI_HALLUCINATION_LINT.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  "\$PYTHON" "\$ROOT/scripts/validation/check_report_claims_phase_0_7.py" --artifact-dir "\$ART"
  "\$PYTHON" "\$ROOT/scripts/validation/check_no_placeholder_artifacts_phase_0_7.py" --artifact-dir "\$ART"
  echo ok > ANTI_HALLUCINATION_LINT.done
  """
}

process COLLECT_ACCEPTANCE_ARCHIVE {
  tag 'phase_0_7_archive'
  publishDir "${params.outdir}/markers", mode: 'copy'
  input:
  path marker
  output:
  path 'COLLECT_ACCEPTANCE_ARCHIVE.done'
  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  "\$PYTHON" "\$ROOT/scripts/phase07/write_tool_versions_phase_0_7.py" --artifact-dir "\$ART" --vina-bin "${params.vina_bin}" --ligand-prep-command "${params.ligand_prep_command}" --receptor-prep-command "${params.receptor_prep_command}" --obabel-bin "${params.obabel_bin}"
  "\$PYTHON" "\$ROOT/scripts/phase07/write_external_manifest_phase_0_7.py" --artifact-dir "\$ART" --vina-bin "${params.vina_bin}" --ligand-prep-command "${params.ligand_prep_command}" --receptor-prep-command "${params.receptor_prep_command}" --obabel-bin "${params.obabel_bin}"
  "\$PYTHON" "\$ROOT/scripts/phase07/compute_semantic_hashes_phase_0_7.py" --artifact-dir "\$ART"
  echo ok > COLLECT_ACCEPTANCE_ARCHIVE.done
  """
}

workflow {
  regression_ch = REGRESSION_PHASE03_TO_06()
  ligand_ch = VALIDATE_LIGAND_INPUT(regression_ch)
  standard_ch = STANDARDIZE_LIGAND(ligand_ch)
  conformer_ch = GENERATE_CONFORMERS(standard_ch)
  ligand_prep_ch = PREPARE_LIGAND_FOR_DOCKING(conformer_ch)
  receptor_ch = PREPARE_RECEPTORS(ligand_prep_ch)
  boxes_ch = GENERATE_DOCKING_BOXES(receptor_ch)
  redock_ch = RUN_POSITIVE_CONTROL_REDOCKING(boxes_ch)
  query_ch = RUN_QUERY_DOCKING(redock_ch)
  negative_ch = RUN_NEGATIVE_CONTROL_DOCKING(query_ch)
  parse_ch = PARSE_DOCKING_OUTPUTS(negative_ch)
  rmsd_ch = COMPUTE_REDOCKING_RMSD(parse_ch)
  ifp_ch = GENERATE_INTERACTION_FINGERPRINTS(rmsd_ch)
  decoy_ch = CALIBRATE_DECOYS(ifp_ch)
  ledger_ch = BUILD_EVIDENCE_LEDGER(decoy_ch)
  report_ch = RENDER_PHASE07_REPORT(ledger_ch)
  lint_ch = ANTI_HALLUCINATION_LINT(report_ch)
  COLLECT_ACCEPTANCE_ARCHIVE(lint_ch)
}
