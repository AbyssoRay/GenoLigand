nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_5_gpu_strict'
params.python = params.python ?: 'python'
params.cuda_home = params.cuda_home ?: 'D:\\Program Files\\CUDA'
params.nvcc = params.nvcc ?: 'D:\\Program Files\\CUDA\\bin\\nvcc.exe'
params.phase04_dir = params.phase04_dir ?: 'acceptance_artifacts_phase_0_4'
params.max_candidates = params.max_candidates ?: 11

def rootWin() { java.nio.file.Paths.get(launchDir.toString()).toAbsolutePath().normalize().toString().replace('\\', '/') }

process GPU_ENV_CHECK {
  label 'gpu'
  publishDir "${params.outdir}/metrics", mode: 'copy'

  output:
  path 'cuda_env.json'

  script:
  """
  CUDA_BASH=\$(cygpath -u "${params.cuda_home}")
  export CUDA_HOME="${params.cuda_home}"
  export PATH="\$CUDA_BASH/bin:\$CUDA_BASH/libnvvp:/usr/bin:/bin:/c/Windows/System32:\$PATH"
  export PYTHONPATH="${rootWin()}/python"
  "${params.python}" "${rootWin()}/scripts/gpu/check_cuda_env.py" --strict --nvcc "${params.nvcc}" --out cuda_env.json
  """
}

process GPU_TENSOR_SMOKE_TEST {
  label 'gpu'
  publishDir "${params.outdir}/metrics", mode: 'copy'

  input:
  path cuda_env

  output:
  path 'cuda_tensor_smoke_test.json'

  script:
  """
  CUDA_BASH=\$(cygpath -u "${params.cuda_home}")
  export CUDA_HOME="${params.cuda_home}"
  export PATH="\$CUDA_BASH/bin:\$CUDA_BASH/libnvvp:/usr/bin:/bin:/c/Windows/System32:\$PATH"
  export PYTHONPATH="${rootWin()}/python"
  "${params.python}" "${rootWin()}/scripts/gpu/gpu_tensor_smoke_test.py" --device cuda:0 --out cuda_tensor_smoke_test.json
  """
}

process EXPORT_CANDIDATE_FASTA {
  publishDir "${params.outdir}/outputs", mode: 'copy'

  input:
  path smoke

  output:
  path 'phase05_inputs'

  script:
  """
  mkdir -p phase05_inputs
  cp "${rootWin()}/${params.phase04_dir}/outputs/candidates/candidate_orfs.fasta" phase05_inputs/candidate_orfs.fasta
  cp "${rootWin()}/${params.phase04_dir}/outputs/candidates/candidate_orfs.tsv" phase05_inputs/candidate_orfs.tsv
  """
}

process PLM_EMBED_GPU {
  label 'gpu'
  publishDir "${params.outdir}/outputs", mode: 'copy'

  input:
  path input_bundle

  output:
  path 'plm_gpu'

  script:
  """
  CUDA_BASH=\$(cygpath -u "${params.cuda_home}")
  export CUDA_HOME="${params.cuda_home}"
  export PATH="\$CUDA_BASH/bin:\$CUDA_BASH/libnvvp:/usr/bin:/bin:/c/Windows/System32:\$PATH"
  export PYTHONPATH="${rootWin()}/python"
  "${params.python}" "${rootWin()}/scripts/plm/embed_candidates_gpu.py" \
    --input-fasta phase05_inputs/candidate_orfs.fasta \
    --candidate-tsv phase05_inputs/candidate_orfs.tsv \
    --output-dir plm_gpu \
    --model-id esm2_t6_8M_UR50D \
    --device cuda:0 \
    --batch-size 4 \
    --memory-probe-out plm_gpu/gpu_memory_probe_plm.json
  """
}

process STRUCTURE_PREDICT_GPU {
  label 'gpu'
  publishDir "${params.outdir}/outputs", mode: 'copy'

  input:
  path input_bundle
  path plm_dir

  output:
  path 'structures'

  script:
  """
  CUDA_BASH=\$(cygpath -u "${params.cuda_home}")
  export CUDA_HOME="${params.cuda_home}"
  export PATH="\$CUDA_BASH/bin:\$CUDA_BASH/libnvvp:/usr/bin:/bin:/c/Windows/System32:\$PATH"
  export PYTHONPATH="${rootWin()}/python"
  "${params.python}" "${rootWin()}/scripts/structure/predict_structures_gpu.py" \
    --input-fasta phase05_inputs/candidate_orfs.fasta \
    --candidate-tsv phase05_inputs/candidate_orfs.tsv \
    --output-dir structures \
    --backend esmfold_hf_gpu \
    --model-id facebook/esmfold_v1 \
    --device cuda:0 \
    --max-candidates "${params.max_candidates}" \
    --mandatory-id p03_k1_cds_001 \
    --mandatory-id p03_k2_alt_frame_001 \
    --mandatory-id p03_d1_low_complexity_001 \
    --mandatory-id p04_d3_shuffled_p03_k1_cds_001 \
    --mandatory-id p04_d4_reversed_p03_k1_cds_001 \
    --mandatory-id p04_d5_random_matched_length_001 \
    --memory-probe-out structures/gpu_memory_probe_structure.json
  """
}

process STRUCTURE_QUALITY {
  publishDir "${params.outdir}/outputs", mode: 'copy'

  input:
  path structures_dir

  output:
  path 'phase05_features'

  script:
  """
  export PYTHONPATH="${rootWin()}/python"
  mkdir -p phase05_features/structure_features phase05_features/patches
  "${params.python}" "${rootWin()}/scripts/structure/compute_structure_quality.py" \
    --manifest structures/structure_manifest.tsv \
    --out-quality phase05_features/structure_features/structure_quality.tsv \
    --out-graph phase05_features/structure_features/ca_graph_summary.tsv \
    --out-patches phase05_features/patches/patch_descriptors.tsv \
    --out-secondary phase05_features/structure_features/secondary_structure_summary.tsv
  echo '{"patch_descriptor_status":"success"}' > phase05_features/patches/patch_manifest.json
  """
}

process PATCH_DESCRIPTOR {
  publishDir "${params.outdir}/metrics", mode: 'copy', pattern: 'patch_descriptor_check.txt'

  input:
  path features_bundle

  output:
  path 'patch_descriptor_check.txt'

  script:
  """
  export PYTHONPATH="${rootWin()}/python"
  "${params.python}" "${rootWin()}/scripts/structure/extract_patch_descriptors.py" --patches phase05_features/patches/patch_descriptors.tsv --min-rows 6
  echo patch_descriptor_ok > patch_descriptor_check.txt
  """
}

process PHASE05_REPORT {
  publishDir "${params.outdir}/outputs", mode: 'copy'

  input:
  path plm_dir
  path structures_dir
  path features_bundle
  path patch_check

  output:
  path 'reports'

  script:
  """
  mkdir -p reports
  "${rootWin()}/target/release/gls-cli.exe" report phase-0-5 \
    --candidates "${rootWin()}/${params.phase04_dir}/outputs/candidates/candidate_orfs.tsv" \
    --sequence-evidence "${rootWin()}/${params.phase04_dir}/outputs/sequence_evidence/sequence_evidence.tsv" \
    --plm-gpu plm_gpu/plm_embeddings.tsv \
    --structure-manifest structures/structure_manifest.tsv \
    --structure-quality phase05_features/structure_features/structure_quality.tsv \
    --patches phase05_features/patches/patch_descriptors.tsv \
    --out reports/candidate_report_phase_0_5.md \
    --score-out reports/structure_score_decomposition.tsv
  cp reports/structure_score_decomposition.tsv reports/structure_evidence_summary.tsv
  """
}

process PHASE05_ACCEPTANCE {
  publishDir "${params.outdir}/metrics", mode: 'copy'

  input:
  path reports

  output:
  path 'phase_0_5_nextflow_acceptance.txt'

  script:
  """
  test -s reports/candidate_report_phase_0_5.md
  test -s reports/structure_score_decomposition.tsv
  echo phase_0_5_nextflow_acceptance_ok > phase_0_5_nextflow_acceptance.txt
  """
}

workflow {
  env_ch = GPU_ENV_CHECK()
  smoke_ch = GPU_TENSOR_SMOKE_TEST(env_ch)
  input_ch = EXPORT_CANDIDATE_FASTA(smoke_ch)
  plm_ch = PLM_EMBED_GPU(input_ch)
  structure_ch = STRUCTURE_PREDICT_GPU(input_ch, plm_ch)
  quality_ch = STRUCTURE_QUALITY(structure_ch)
  patch_ch = PATCH_DESCRIPTOR(quality_ch)
  report_ch = PHASE05_REPORT(plm_ch, structure_ch, quality_ch, patch_ch)
  PHASE05_ACCEPTANCE(report_ch)
}
