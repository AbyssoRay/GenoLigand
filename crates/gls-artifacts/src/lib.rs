use gls_types::{Provenance, stable_hash};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub kind: String,
    pub hash: String,
    pub provenance: Provenance,
}

impl ArtifactRecord {
    pub fn new(kind: impl Into<String>, payload: &str, provenance: Provenance) -> Self {
        let kind = kind.into();
        let hash = stable_hash(payload);
        let artifact_id = format!("{kind}:{hash}");
        Self {
            artifact_id,
            kind,
            hash,
            provenance,
        }
    }

    pub fn to_json_line(&self) -> String {
        format!(
            "{{\"artifact_id\":\"{}\",\"kind\":\"{}\",\"hash\":\"{}\",\"source\":\"{}\",\"version\":\"{}\"}}",
            escape_json(&self.artifact_id),
            escape_json(&self.kind),
            self.hash,
            escape_json(&self.provenance.source),
            escape_json(&self.provenance.version)
        )
    }
}

pub fn hash_payload(kind: &str, payload: &str) -> String {
    stable_hash(&format!("{kind}\n{payload}"))
}

pub fn provenance_for_tool(tool: &str, version: &str, params: &[(&str, String)]) -> Provenance {
    let mut encoded = String::new();
    for (key, value) in params {
        encoded.push_str(key);
        encoded.push('=');
        encoded.push_str(value);
        encoded.push('\n');
    }
    Provenance::new(tool, version, &encoded)
}

fn escape_json(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            _ => vec![ch],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_json_line_carries_hash_and_provenance() {
        let provenance = provenance_for_tool("gls-test", "1", &[("a", "b".to_string())]);
        let record = ArtifactRecord::new("fragment_grammar", "payload", provenance);
        let line = record.to_json_line();

        assert!(line.contains("\"kind\":\"fragment_grammar\""));
        assert!(line.contains("\"source\":\"gls-test\""));
    }
}
