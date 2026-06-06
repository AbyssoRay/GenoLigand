nextflow.enable.dsl = 2

import java.nio.file.Files
import java.nio.file.Paths
import java.security.MessageDigest

include { GLS_DEV_SMOKE } from './modules/gls_dev_smoke.nf'

params.outdir = params.outdir ?: 'results/dev_cpu_smoke'

def sourceFingerprint() {
  def launchPath = Paths.get(launchDir.toString()).toAbsolutePath().normalize()
  def roots = [
    'Cargo.toml',
    'Cargo.lock',
    'crates',
    'schemas',
    'schema_examples',
    'docs',
    'workers',
    'workflows',
    'config',
    'tools',
    'nextflow.config'
  ]
  def paths = []
  roots.each { rel ->
    def path = launchPath.resolve(rel)
    if (Files.isDirectory(path)) {
      def stream = Files.walk(path)
      try {
        stream
          .filter { Files.isRegularFile(it) }
          .forEach { paths << it }
      }
      finally {
        stream.close()
      }
    }
    else if (Files.exists(path)) {
      paths << path
    }
  }

  def digest = MessageDigest.getInstance('SHA-256')
  paths
    .sort { left, right -> launchPath.relativize(left).toString() <=> launchPath.relativize(right).toString() }
    .each { path ->
      def relative = launchPath.relativize(path).toString().replace('\\', '/')
      digest.update(relative.getBytes('UTF-8'))
      digest.update((byte) 0)
      digest.update(Files.readAllBytes(path))
      digest.update((byte) 0)
    }
  digest.digest().encodeHex().toString()
}

workflow {
  Channel
    .value(sourceFingerprint())
    .set { source_fingerprint_ch }

  GLS_DEV_SMOKE(source_fingerprint_ch)
}
