nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_3'

def sourceFingerprint() {
  def launchPath = java.nio.file.Paths.get(launchDir.toString()).toAbsolutePath().normalize()
  def roots = [
    'Cargo.toml',
    'Cargo.lock',
    'crates',
    'fixtures/phase_0_3',
    'tools/cargo-runner.sh',
    'workflows/phase_0_3.nf'
  ]
  def paths = []
  roots.each { rel ->
    def path = launchPath.resolve(rel)
    if (java.nio.file.Files.isDirectory(path)) {
      def stream = java.nio.file.Files.walk(path)
      stream
        .filter { java.nio.file.Files.isRegularFile(it) }
        .forEach { paths << it }
      stream.close()
    }
    else if (java.nio.file.Files.exists(path)) {
      paths << path
    }
  }

  def digest = java.security.MessageDigest.getInstance('SHA-256')
  paths
    .sort { left, right -> launchPath.relativize(left).toString() <=> launchPath.relativize(right).toString() }
    .each { path ->
      def relative = launchPath.relativize(path).toString().replace('\\', '/')
      digest.update(relative.getBytes('UTF-8'))
      digest.update(0 as byte)
      digest.update(java.nio.file.Files.readAllBytes(path))
      digest.update(0 as byte)
    }
  digest.digest().encodeHex().toString()
}

process GLS_PHASE_0_3 {
  tag 'phase_0_3'
  cache 'deep'
  storeDir { "${launchDir}/results/phase_0_3/store/${source_fingerprint}" }
  publishDir params.outdir, mode: 'copy'

  input:
  val source_fingerprint

  output:
  path 'hits.tsv'
  path 'stats.tsv'
  path 'orf_extensions.tsv'
  path 'context.tsv'
  path 'cooccurrence.tsv'
  path 'orf_gate.tsv'
  path 'candidate_report.md'

  script:
  """
  ROOT=\$(cygpath -u "${launchDir}")
  echo "source_fingerprint=${source_fingerprint}" >&2
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- search genome --genome "\$ROOT/fixtures/phase_0_3/genome.fa" --fragments "\$ROOT/fixtures/phase_0_3/fragment_kernel.json" --out hits.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- stats score --hits hits.tsv --out stats.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- orf extend --genome "\$ROOT/fixtures/phase_0_3/genome.fa" --hits hits.tsv --out orf_extensions.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- context classify --annotation "\$ROOT/fixtures/phase_0_3/annotation.gff3" --orfs orf_extensions.tsv --out context.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- cooccur score --hits hits.tsv --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --out cooccurrence.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- gate short-circuit --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --cooccur cooccurrence.tsv --evidence "\$ROOT/fixtures/phase_0_3/evidence_table.tsv" --out orf_gate.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- report candidate --hits hits.tsv --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --cooccur cooccurrence.tsv --gate orf_gate.tsv --evidence "\$ROOT/fixtures/phase_0_3/evidence_table.tsv" --out candidate_report.md
  """
}

workflow {
  Channel
    .value(sourceFingerprint())
    .set { source_fingerprint_ch }

  GLS_PHASE_0_3(source_fingerprint_ch)
}
