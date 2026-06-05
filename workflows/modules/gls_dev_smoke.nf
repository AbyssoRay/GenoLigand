process GLS_DEV_SMOKE {
  tag 'gls-cli dev-smoke'
  cache 'deep'
  storeDir { "${launchDir}/results/dev_cpu_smoke/store/${source_fingerprint}" }
  publishDir params.outdir, mode: 'copy'

  input:
  val source_fingerprint

  output:
  path 'report.md'

  script:
  """
  echo "source_fingerprint=${source_fingerprint}" >&2
  bash ${launchDir}/tools/cargo-runner.sh run --manifest-path ${launchDir}/Cargo.toml -p gls-cli -- dev-smoke > report.md
  """
}
