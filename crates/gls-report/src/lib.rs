use gls_types::{EndToEndReport, StageResourceUsage};
use std::collections::BTreeMap;

pub fn stage_usage(
    stage: impl Into<String>,
    candidates_in: usize,
    candidates_out: usize,
    failure_reasons: impl IntoIterator<Item = String>,
    cpu_ms: f64,
    peak_memory_mb: f64,
) -> StageResourceUsage {
    let mut failures = BTreeMap::<String, usize>::new();
    for reason in failure_reasons {
        *failures.entry(reason).or_default() += 1;
    }
    StageResourceUsage {
        stage: stage.into(),
        candidates_in,
        candidates_out,
        failures,
        cpu_ms,
        peak_memory_mb,
    }
}

pub fn render_markdown(report: &EndToEndReport) -> String {
    let mut out = String::from("# GenoLigand-Search V1.6 Report\n\n");
    out.push_str("| Stage | Candidates In | Candidates Out | Retention | CPU ms | Peak MB | Failure Reasons |\n");
    out.push_str("|---|---:|---:|---:|---:|---:|---|\n");
    for stage in &report.stages {
        let retention = if stage.candidates_in == 0 {
            0.0
        } else {
            stage.candidates_out as f64 / stage.candidates_in as f64
        };
        let failures = stage
            .failures
            .iter()
            .map(|(reason, count)| format!("{reason}:{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "| {} | {} | {} | {:.3} | {:.1} | {:.1} | {} |\n",
            stage.stage,
            stage.candidates_in,
            stage.candidates_out,
            retention,
            stage.cpu_ms,
            stage.peak_memory_mb,
            failures
        ));
    }
    if !report.notes.is_empty() {
        out.push_str("\n## Notes\n");
        for note in &report.notes {
            out.push_str("- ");
            out.push_str(note);
            out.push('\n');
        }
    }
    if !report.audit_records.is_empty() {
        out.push_str("\n## Candidate Audit\n");
        out.push_str("| Candidate | Stage | Tool | Version | Parameters Hash | Artifact Hash |\n");
        out.push_str("|---|---|---|---|---|---|\n");
        for audit in &report.audit_records {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                audit.candidate_id,
                audit.stage,
                audit.tool_name,
                audit.tool_version,
                audit.parameters_hash,
                audit.artifact_hash
            ));
        }
    }
    out
}

pub fn render_html(report: &EndToEndReport) -> String {
    let markdown = render_markdown(report);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>GenoLigand-Search V1.6 Report</title></head><body><pre>{}</pre></body></html>",
        escape_html(&markdown)
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_shows_layer_counts_failures_and_resources() {
        let report = EndToEndReport {
            stages: vec![stage_usage(
                "L1.20",
                10,
                2,
                ["plm_budget_exhausted".to_string()],
                12.0,
                4.0,
            )],
            audit_records: vec![gls_types::CandidateAuditRecord {
                candidate_id: gls_types::CandidateId::from("c1"),
                stage: "L1.20".to_string(),
                tool_name: "gls-shortcircuit".to_string(),
                tool_version: "v1.6".to_string(),
                parameters_hash: "p".to_string(),
                artifact_hash: "a".to_string(),
            }],
            notes: vec!["trend only for IDP/IDR".to_string()],
        };
        let markdown = render_markdown(&report);

        assert!(markdown.contains("Candidates In"));
        assert!(markdown.contains("plm_budget_exhausted:1"));
        assert!(markdown.contains("Peak MB"));
        assert!(markdown.contains("Candidate Audit"));
        assert!(markdown.contains("gls-shortcircuit"));
    }
}
