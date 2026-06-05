nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/dev_cpu_smoke'

process GLS_DEV_SMOKE {
  tag 'gls-cli dev-smoke'
  publishDir params.outdir, mode: 'copy'

  output:
  path 'report.md'

  script:
  """
  cargo run -p gls-cli -- dev-smoke > report.md
  """
}

workflow {
  GLS_DEV_SMOKE()
}
