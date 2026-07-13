nextflow.enable.dsl = 2

process GENOLIGAND_SCIENCE_PIPELINE {
  tag "genoligand_pipeline"
  cpus 20
  memory '32 GB'
  publishDir params.outdir, mode: 'copy', overwrite: true

  input:
  path input_manifest
  path science_store
  path gls_cli

  output:
  path "run_output", emit: run_output
  path "pipeline_done.txt"

  script:
  """
  mkdir -p "run_output"
  if "./${gls_cli}" pipeline run \
    --input-manifest "${input_manifest}" \
    --science-store "${science_store}" \
    --run-dir "run_output" \
    --workspace-root "${launchDir}"
  then
    :
  else
    cli_status=\$?
    printf "GenoLigand CLI failed with exit code %s\\n" "\${cli_status}" >&2
    exit "\${cli_status}"
  fi

  if [ ! -f "run_output/run_state.txt" ]; then
    printf "GenoLigand run state is missing\\n" >&2
    exit 1
  fi
  run_state=\$(tr -d '\\r\\n' < "run_output/run_state.txt")
  if [ "\${run_state}" != "completed" ]; then
    printf "GenoLigand run did not complete: %s\\n" "\${run_state}" >&2
    exit 1
  fi

  printf "run_dir\\t%s\\n" "run_output" > pipeline_done.txt
  """
}

workflow {
  GENOLIGAND_SCIENCE_PIPELINE(
    file(params.input_manifest, checkIfExists: true),
    file(params.science_store, checkIfExists: true),
    file(params.gls_cli, checkIfExists: true)
  )
}
