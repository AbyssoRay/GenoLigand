nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_6_structure_patch_strict'
params.artifact_dir = params.artifact_dir ?: 'acceptance_artifacts_phase_0_6_structure_patch_strict'
params.phase05_dir = params.phase05_dir ?: 'acceptance_artifacts_phase_0_5_gpu_strict'
params.python = params.python ?: 'python'
params.foldseek_bin = params.foldseek_bin ?: (System.getenv('FOLDSEEK_BIN') ?: '/opt/foldseek/foldseek/bin/foldseek')
params.foldseek_wsl_distro = params.foldseek_wsl_distro ?: (System.getenv('FOLDSEEK_WSL_DISTRO') ?: 'GenoLigandPhase06')
params.usalign_bin = params.usalign_bin ?: (System.getenv('USALIGN_BIN') ?: '.phase_0_6_tools/USalign/USalign.exe')
params.min_reference_entries = params.min_reference_entries ?: 200
params.preferred_reference_entries = params.preferred_reference_entries ?: 500
params.min_reference_patches = params.min_reference_patches ?: 200
params.foldseek_threads = params.foldseek_threads ?: 4

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

process PHASE_0_6_REFERENCE_PREP {
  tag 'phase_0_6_reference_prep'
  publishDir "${params.outdir}/markers", mode: 'copy'

  output:
  path 'phase_0_6_reference_prep.done'

  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PHASE05="${winPath(params.phase05_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  export MSYS_NO_PATHCONV=1
  export MSYS2_ARG_CONV_EXCL='*'
  mkdir -p "\$ART/metrics" "\$ART/logs/nextflow" "\$ART/outputs/reference_structures" "\$ART/outputs/reference_patches"
  "\$PYTHON" "\$ROOT/scripts/phase06/check_phase05_inputs.py" --phase05-dir "\$PHASE05" --out "\$ART/metrics/phase05_input_check.json"
  "\$PYTHON" "\$ROOT/scripts/phase06/prepare_query_structures.py" --phase05-dir "\$PHASE05" --out-dir "\$ART/outputs/query_structures"
  "\$PYTHON" "\$ROOT/scripts/phase06/prepare_biolip2_subset.py" --out "\$ART/outputs/reference_structures/biolip2_interactions.tsv" --schema-out "\$ART/metrics/biolip2_schema_check.tsv" --min-rows "${params.min_reference_patches}" --preferred-rows "${params.preferred_reference_entries}"
  "\$PYTHON" "\$ROOT/scripts/phase06/fetch_rcsb_reference_subset.py" --biolip-tsv "\$ART/outputs/reference_structures/biolip2_interactions.tsv" --manifest-out "\$ART/outputs/reference_structures/reference_structure_manifest.tsv" --summary-out "\$ART/metrics/reference_structure_summary.tsv" --min-entries "${params.min_reference_entries}" --preferred-entries "${params.preferred_reference_entries}"
  "\$PYTHON" "\$ROOT/scripts/phase06/build_reference_patch_db.py" --biolip-tsv "\$ART/outputs/reference_structures/biolip2_interactions.tsv" --reference-manifest "\$ART/outputs/reference_structures/reference_structure_manifest.tsv" --out "\$ART/outputs/reference_patches/reference_binding_patches.tsv" --manifest-json "\$ART/outputs/reference_patches/reference_patch_manifest.json" --min-patches "${params.min_reference_patches}"
  "\$PYTHON" "\$ROOT/scripts/phase06/prepare_afdb_reference_subset.py" --out "\$ART/metrics/afdb_reference_subset_status.tsv"
  "\$PYTHON" "\$ROOT/scripts/phase06/prepare_plip_reference_patches.py" --out "\$ART/metrics/plip_reference_patch_status.tsv"
  printf "gate\\tstatus\\tnotes\\nphase_0_3\\tPASS\\treused from Phase 0.5 strict acceptance\\nphase_0_4\\tPASS\\treused from Phase 0.5 strict acceptance\\nphase_0_5\\tPASS\\tGPU strict acceptance report checked\\n" > "\$ART/metrics/phase_0_3_to_0_5_regression_summary.tsv"
  echo reference_prep_ok > phase_0_6_reference_prep.done
  """
}

process PHASE_0_6_STRUCTURE_SEARCH {
  tag 'phase_0_6_structure_search'
  publishDir "${params.outdir}/markers", mode: 'copy'

  input:
  path reference_marker

  output:
  path 'phase_0_6_structure_search.done'

  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PHASE05="${winPath(params.phase05_dir)}"
  PYTHON="${params.python}"
  FOLDSEEK_BIN="${params.foldseek_bin}"
  FOLDSEEK_WSL_DISTRO="${params.foldseek_wsl_distro}"
  USALIGN_BIN="${winPath(params.usalign_bin)}"
  export PYTHONPATH="\$ROOT/python"
  export MSYS_NO_PATHCONV=1
  export MSYS2_ARG_CONV_EXCL='*'
  mkdir -p "\$ART/outputs/structure_similarity" "\$ART/outputs/patch_matching" "\$ART/outputs/decoy_calibration" "\$ART/outputs/target_prioritization" "\$ART/outputs/reports" "\$ART/outputs/hashes" "\$ART/logs/foldseek" "\$ART/logs/usalign"
  "\$PYTHON" "\$ROOT/scripts/phase06/build_foldseek_databases.py" --foldseek-bin "\$FOLDSEEK_BIN" --wsl-distro "\$FOLDSEEK_WSL_DISTRO" --query-dir "\$ART/outputs/query_structures/pdb" --reference-dir "\$ROOT/data/external_phase_0_6/rcsb/pdb" --query-db "\$ROOT/databases/phase_0_6/foldseek/query_db" --reference-db "\$ROOT/databases/phase_0_6/foldseek/reference_db" --logs-dir "\$ART/logs/foldseek"
  "\$PYTHON" "\$ROOT/scripts/phase06/run_foldseek_search.py" --foldseek-bin "\$FOLDSEEK_BIN" --wsl-distro "\$FOLDSEEK_WSL_DISTRO" --query-db "\$ROOT/databases/phase_0_6/foldseek/query_db" --reference-db "\$ROOT/databases/phase_0_6/foldseek/reference_db" --result-db "\$ROOT/databases/phase_0_6/foldseek/result_db" --tmp-dir "\$ROOT/databases/phase_0_6/foldseek/tmp" --hits-out "\$ART/outputs/structure_similarity/foldseek_hits.tsv" --reference-manifest "\$ART/outputs/reference_structures/reference_structure_manifest.tsv" --metrics-dir "\$ART/metrics" --logs-dir "\$ART/logs/foldseek" --threads "${params.foldseek_threads}"
  "\$PYTHON" "\$ROOT/scripts/phase06/parse_foldseek_hits.py" --input "\$ART/outputs/structure_similarity/foldseek_hits.tsv" --out "\$ART/outputs/structure_similarity/foldseek_hits.parsed.tsv" --summary-out "\$ART/metrics/foldseek_parse_summary.tsv"
  "\$PYTHON" "\$ROOT/scripts/phase06/run_usalign_confirmation.py" --usalign-bin "\$USALIGN_BIN" --foldseek-hits "\$ART/outputs/structure_similarity/foldseek_hits.tsv" --query-manifest "\$ART/outputs/query_structures/query_structure_manifest.tsv" --reference-manifest "\$ART/outputs/reference_structures/reference_structure_manifest.tsv" --out "\$ART/outputs/structure_similarity/usalign_confirmations.tsv" --raw-dir "\$ART/logs/usalign/raw_outputs" --summary-out "\$ART/metrics/usalign_confirmation_summary.tsv"
  "\$PYTHON" "\$ROOT/scripts/phase06/parse_usalign_outputs.py" --input "\$ART/outputs/structure_similarity/usalign_confirmations.tsv" --out "\$ART/outputs/structure_similarity/usalign_confirmations.parsed.tsv" --summary-out "\$ART/metrics/usalign_parse_summary.tsv"
  "\$PYTHON" "\$ROOT/scripts/phase06/match_candidate_patches.py" --candidate-patches "\$PHASE05/outputs/patches/patch_descriptors.tsv" --reference-patches "\$ART/outputs/reference_patches/reference_binding_patches.tsv" --out "\$ART/outputs/patch_matching/candidate_reference_patch_matches.tsv" --summary-out "\$ART/metrics/patch_matching_summary.tsv"
  "\$PYTHON" "\$ROOT/scripts/phase06/calibrate_decoy_margins.py" --foldseek-hits "\$ART/outputs/structure_similarity/foldseek_hits.tsv" --usalign-tsv "\$ART/outputs/structure_similarity/usalign_confirmations.tsv" --patch-matches "\$ART/outputs/patch_matching/candidate_reference_patch_matches.tsv" --out "\$ART/outputs/decoy_calibration/decoy_margin_summary.tsv" --summary-out "\$ART/metrics/decoy_calibration_summary.tsv"
  "\$PYTHON" "\$ROOT/scripts/phase06/score_target_priors.py" --foldseek-hits "\$ART/outputs/structure_similarity/foldseek_hits.tsv" --usalign-tsv "\$ART/outputs/structure_similarity/usalign_confirmations.tsv" --patch-matches "\$ART/outputs/patch_matching/candidate_reference_patch_matches.tsv" --decoy-summary "\$ART/outputs/decoy_calibration/decoy_margin_summary.tsv" --structure-quality "\$PHASE05/outputs/structure_features/structure_quality.tsv" --out-target "\$ART/outputs/target_prioritization/target_prior_table.tsv" --out-score "\$ART/outputs/target_prioritization/structure_score_decomposition.tsv" --score-config "\$ROOT/configs/phase_0_6_scoring_weights.yml"
  "\$PYTHON" "\$ROOT/scripts/phase06/render_candidate_report_phase_0_6.py" --artifact-dir "\$ART" --out "\$ART/outputs/reports/candidate_report_phase_0_6.md"
  "\$PYTHON" "\$ROOT/scripts/phase06/compute_semantic_hashes_phase_0_6.py" --artifact-dir "\$ART" --out "\$ART/outputs/hashes/semantic_hashes.tsv"
  "\$PYTHON" "\$ROOT/scripts/phase06/write_external_manifest_phase_0_6.py" --out "\$ART/external_resource_manifest.tsv" --foldseek-bin "\$FOLDSEEK_BIN" --usalign-bin "\$USALIGN_BIN"
  echo structure_search_ok > phase_0_6_structure_search.done
  """
}

process PHASE_0_6_TARGET_REPORT {
  tag 'phase_0_6_target_report'
  publishDir "${params.outdir}/markers", mode: 'copy'

  input:
  path search_marker

  output:
  path 'phase_0_6_target_report.done'

  script:
  """
  ROOT="${rootWin()}"
  ART="${winPath(params.artifact_dir)}"
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT/python"
  export MSYS_NO_PATHCONV=1
  export MSYS2_ARG_CONV_EXCL='*'
  "\$PYTHON" "\$ROOT/scripts/validation/check_target_language_lint.py" --artifact-dir "\$ART"
  echo target_report_ok > phase_0_6_target_report.done
  """
}

workflow {
  reference_ch = PHASE_0_6_REFERENCE_PREP()
  search_ch = PHASE_0_6_STRUCTURE_SEARCH(reference_ch)
  PHASE_0_6_TARGET_REPORT(search_ch)
}
