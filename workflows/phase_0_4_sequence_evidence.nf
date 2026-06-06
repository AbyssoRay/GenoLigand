nextflow.enable.dsl = 2

params.outdir = params.outdir ?: 'results/phase_0_4'
params.python = params.python ?: 'python'
params.plm_backend = params.plm_backend ?: 'fixture_mock'
params.plm_device = params.plm_device ?: 'cpu'
params.model_id = params.model_id ?: (params.plm_backend == 'fixture_mock' ? 'fixture_mock_64' : 'esm2_t6_8M_UR50D')
params.similarity_backend = params.similarity_backend ?: 'fixture_mock'
params.similarity_db = params.similarity_db ?: ''
params.threads = params.threads ?: '4'

def sourceFingerprint() {
  def launchPath = java.nio.file.Paths.get(launchDir.toString()).toAbsolutePath().normalize()
  def roots = [
    'Cargo.toml',
    'Cargo.lock',
    'crates',
    'configs',
    'fixtures/phase_0_3',
    'python/geno_plm',
    'scripts/similarity',
    'tools/cargo-runner.sh',
    'workflows/phase_0_4_sequence_evidence.nf'
  ]
  def paths = []
  roots.each { rel ->
    def path = launchPath.resolve(rel)
    if (java.nio.file.Files.isDirectory(path)) {
      def stream = java.nio.file.Files.walk(path)
      stream.filter { java.nio.file.Files.isRegularFile(it) }.forEach { paths << it }
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

process PHASE_0_4_SEQUENCE_EVIDENCE {
  tag "phase_0_4_${params.plm_backend}_${params.similarity_backend}"
  cache 'deep'
  storeDir { "${launchDir}/results/phase_0_4/store/${source_fingerprint}_${params.plm_backend}_${params.similarity_backend}" }
  publishDir params.outdir, mode: 'copy'

  input:
  val source_fingerprint

  output:
  path 'outputs'

  script:
  """
  ROOT=\$(cygpath -u "${launchDir}")
  ROOT_PY=\$(cygpath -m "${launchDir}")
  PYTHON="${params.python}"
  export PYTHONPATH="\$ROOT_PY/python"
  mkdir -p outputs/candidates outputs/sequence_features outputs/plm_embeddings outputs/similarity outputs/sequence_evidence outputs/reports
  echo "source_fingerprint=${source_fingerprint}" >&2
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- search genome --genome "\$ROOT/fixtures/phase_0_3/genome.fa" --fragments "\$ROOT/fixtures/phase_0_3/fragment_kernel.json" --out hits.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- stats score --hits hits.tsv --out stats.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- orf extend --genome "\$ROOT/fixtures/phase_0_3/genome.fa" --hits hits.tsv --out orf_extensions.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- context classify --annotation "\$ROOT/fixtures/phase_0_3/annotation.gff3" --orfs orf_extensions.tsv --out context.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- cooccur score --hits hits.tsv --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --out cooccurrence.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- gate short-circuit --stats stats.tsv --orfs orf_extensions.tsv --context context.tsv --cooccur cooccurrence.tsv --evidence "\$ROOT/fixtures/phase_0_3/evidence_table.tsv" --out orf_gate.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- sequence export-fasta --orfs orf_extensions.tsv --context context.tsv --cooccur cooccurrence.tsv --gate orf_gate.tsv --out-fasta outputs/candidates/candidate_orfs.fasta --out-tsv outputs/candidates/candidate_orfs.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- sequence features --candidates outputs/candidates/candidate_orfs.tsv --config "\$ROOT/configs/phase_0_4.yaml" --out outputs/sequence_features/sequence_features.tsv
  "\$PYTHON" -m geno_plm.cli embed --input-fasta outputs/candidates/candidate_orfs.fasta --output-dir outputs/plm_embeddings --backend "${params.plm_backend}" --model-id "${params.model_id}" --device "${params.plm_device}" --include mean --repr-layer final --toks-per-batch 4096 --seed 3407 --metadata outputs/plm_embeddings/embedding_metadata.json --allow-cpu-fallback
  if [ "${params.similarity_backend}" = "fixture_mock" ]; then
    "\$PYTHON" "\$ROOT_PY/scripts/similarity/fixture_similarity.py" --candidate-tsv outputs/candidates/candidate_orfs.tsv --out-summary outputs/similarity/similarity_summary.tsv --out-hits outputs/similarity/similarity_hits.tsv
  else
    "\$PYTHON" "\$ROOT_PY/scripts/similarity/run_similarity.py" --query-fasta outputs/candidates/candidate_orfs.fasta --candidate-tsv outputs/candidates/candidate_orfs.tsv --db "${params.similarity_db}" --backend "${params.similarity_backend}" --out-hits outputs/similarity/similarity_hits.tsv --out-summary outputs/similarity/similarity_summary.tsv --threads "${params.threads}"
  fi
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- sequence score --candidates outputs/candidates/candidate_orfs.tsv --features outputs/sequence_features/sequence_features.tsv --embeddings outputs/plm_embeddings/embedding_manifest.tsv --similarity outputs/similarity/similarity_summary.tsv --weights "\$ROOT/configs/sequence_evidence_weights.yaml" --out outputs/sequence_evidence/sequence_evidence.tsv
  bash "\$ROOT/tools/cargo-runner.sh" run --manifest-path "\$ROOT/Cargo.toml" -p gls-cli -- report phase-0-4 --candidates outputs/candidates/candidate_orfs.tsv --features outputs/sequence_features/sequence_features.tsv --embeddings outputs/plm_embeddings/embedding_manifest.tsv --similarity outputs/similarity/similarity_summary.tsv --evidence outputs/sequence_evidence/sequence_evidence.tsv --out outputs/reports/candidate_report_phase_0_4.md
  cp hits.tsv stats.tsv orf_extensions.tsv context.tsv cooccurrence.tsv orf_gate.tsv outputs/
  """
}

workflow {
  Channel.value(sourceFingerprint()).set { source_fingerprint_ch }
  PHASE_0_4_SEQUENCE_EVIDENCE(source_fingerprint_ch)
}
