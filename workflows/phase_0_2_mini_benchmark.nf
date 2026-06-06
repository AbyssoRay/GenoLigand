nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_2_mini_benchmark'

def sourceFingerprint() {
  def launchPath = java.nio.file.Paths.get(launchDir.toString()).toAbsolutePath().normalize()
  def roots = [
    'Cargo.toml',
    'Cargo.lock',
    'crates',
    'fixtures/phase_0_2',
    'tools/cargo-runner.sh',
    'workflows/phase_0_2_mini_benchmark.nf'
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

process GLS_PHASE_0_2_MINI_BENCHMARK {
  tag 'phase_0_2_mini_benchmark'
  cache 'deep'
  storeDir { "${launchDir}/results/phase_0_2_mini_benchmark/store/${source_fingerprint}" }
  publishDir params.outdir, mode: 'copy'

  input:
  val source_fingerprint

  output:
  path 'hits.tsv'
  path 'stats.tsv'
  path 'orfs.tsv'
  path 'context.tsv'
  path 'cooccur.tsv'
  path 'gate.tsv'
  path 'plm.tsv'
  path 'foldgate.tsv'
  path 'patches.tsv'
  path 'dock.tsv'
  path 'GENOLIGAND_CANDIDATE_REPORT.md'

  script:
  """
  ROOT=\$(cygpath -u "${launchDir}")
  echo "source_fingerprint=${source_fingerprint}" >&2
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- search genome --genome "\$ROOT/fixtures/phase_0_2/mini_genome.fa" --fragments "\$ROOT/fixtures/phase_0_2/fragment_kernel.json" --out hits.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- stats score --hits hits.tsv --out stats.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- orf extend --genome "\$ROOT/fixtures/phase_0_2/mini_genome.fa" --hits hits.tsv --out orfs.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- classify loci --annotation "\$ROOT/fixtures/phase_0_2/mini_annotation.gff3" --orfs orfs.tsv --out context.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- cooccur score --hits hits.tsv --stats stats.tsv --orfs orfs.tsv --context context.tsv --out cooccur.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- gate short-circuit --stats stats.tsv --orfs orfs.tsv --context context.tsv --cooccur cooccur.tsv --out gate.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- gate plm --candidates gate.tsv --profile dev_cpu_smoke --out plm.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- foldgate realize --candidates plm.tsv --out foldgate.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- patch realize --foldgate foldgate.tsv --ligand "\$ROOT/fixtures/phase_0_2/ligand.fixture.yaml" --out patches.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- dock run --patches patches.tsv --mode stub --out dock.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- report markdown --hits hits.tsv --stats stats.tsv --orfs orfs.tsv --context context.tsv --cooccur cooccur.tsv --gate gate.tsv --plm plm.tsv --foldgate foldgate.tsv --patches patches.tsv --dock dock.tsv --out GENOLIGAND_CANDIDATE_REPORT.md
  """
}

workflow {
  Channel
    .value(sourceFingerprint())
    .set { source_fingerprint_ch }

  GLS_PHASE_0_2_MINI_BENCHMARK(source_fingerprint_ch)
}
