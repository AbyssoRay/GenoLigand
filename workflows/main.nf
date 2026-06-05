nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/dev_cpu_smoke'

process GLS_DEV_SMOKE {
  tag 'gls-cli dev-smoke'
  cache 'deep'
  publishDir params.outdir, mode: 'copy'

  input:
  path cargo_lock

  output:
  path 'report.md'

  script:
  """
  bash ${launchDir}/tools/cargo-runner.sh run --manifest-path ${launchDir}/Cargo.toml -p gls-cli -- dev-smoke > report.md
  """
}

workflow {
  Channel
    .fromPath("${launchDir}/Cargo.lock")
    .set { cargo_lock_ch }

  GLS_DEV_SMOKE(cargo_lock_ch)
}
