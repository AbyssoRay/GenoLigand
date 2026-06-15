nextflow.enable.dsl = 2

process GENOLIGAND_SCIENCE_PIPELINE {
  tag "genoligand_pipeline"
  cpus 20
  memory '32 GB'

  input:
  val input_manifest
  val science_store
  val outdir
  val gls_cli

  output:
  path "pipeline_done.txt"

  script:
  """
  mkdir -p "${outdir}"
  "${gls_cli}" pipeline run \
    --input-manifest "${input_manifest}" \
    --science-store "${science_store}" \
    --run-dir "${outdir}" \
    --workspace-root "${launchDir}"
  printf "run_dir\\t%s\\n" "${outdir}" > pipeline_done.txt
  """
}

workflow {
  GENOLIGAND_SCIENCE_PIPELINE(
    params.input_manifest.toString(),
    params.science_store.toString(),
    params.outdir.toString(),
    params.gls_cli.toString()
  )
}
