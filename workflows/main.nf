nextflow.enable.dsl = 2

process GENOLIGAND_SCIENCE_PIPELINE {
  tag "genoligand_pipeline"
  cpus params.pipeline_cpus
  memory params.pipeline_memory
  publishDir params.outdir, mode: 'copy', overwrite: true

  input:
  path input_manifest
  val science_store
  path gls_cli
  val nextflow_version
  path workflow_config

  output:
  path "run_output", emit: run_output
  path "pipeline_done.txt"

  script:
  """
  if [ ! -d "${science_store}" ]; then
    printf "GenoLigand science store is not a directory: %s\\n" "${science_store}" >&2
    exit 1
  fi
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

  config_sha256=\$(sha256sum "${workflow_config}" | awk '{print \$1}')
  printf "schema_version\\tnextflow_version\\tconfig_sha256\\tcpus\\tmemory\\n" > "run_output/nextflow_provenance.tsv"
  printf "genoligand_nextflow_provenance_v1\\t${nextflow_version}\\t%s\\t${task.cpus}\\t${task.memory}\\n" "\${config_sha256}" >> "run_output/nextflow_provenance.tsv"
  printf "run_dir\\t%s\\n" "run_output" > pipeline_done.txt
  """
}

workflow {
  def pinnedNextflowVersion = file("${projectDir}/../.nextflow-version", checkIfExists: true).text.trim()
  def runtimeNextflowVersion = workflow.nextflow.version.toString()
  if (runtimeNextflowVersion != pinnedNextflowVersion) {
    error "Nextflow ${runtimeNextflowVersion} does not match pinned version ${pinnedNextflowVersion}"
  }
  GENOLIGAND_SCIENCE_PIPELINE(
    file(params.input_manifest, checkIfExists: true),
    file(params.science_store, checkIfExists: true).toAbsolutePath().toString(),
    file(params.gls_cli, checkIfExists: true),
    runtimeNextflowVersion,
    file("${projectDir}/../nextflow.config", checkIfExists: true)
  )
}
