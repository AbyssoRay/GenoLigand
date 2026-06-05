use gls_types::{CandidateId, IdpTrendResult};

pub fn compute_idp_trend(
    candidate_id: CandidateId,
    ensemble_hit_rate: f64,
    cluster_enrichment: f64,
    contact_consistency: f64,
) -> IdpTrendResult {
    let combined = (ensemble_hit_rate + cluster_enrichment + contact_consistency) / 3.0;
    let trend_label = if combined >= 0.7 {
        "trend_only_high"
    } else if combined >= 0.4 {
        "trend_only_medium"
    } else {
        "trend_only_low"
    }
    .to_string();
    IdpTrendResult {
        candidate_id,
        ensemble_hit_rate,
        cluster_enrichment,
        contact_consistency,
        trend_label,
    }
}

pub fn render_trend_line(result: &IdpTrendResult) -> String {
    format!(
        "{}: trend only; ensemble_hit_rate={:.3}; cluster_enrichment={:.3}; contact_consistency={:.3}; label={}",
        result.candidate_id,
        result.ensemble_hit_rate,
        result.cluster_enrichment,
        result.contact_consistency,
        result.trend_label
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idp_report_is_trend_only_without_macro_free_energy() {
        let result = compute_idp_trend(CandidateId::from("s3"), 0.8, 0.7, 0.6);
        let line = render_trend_line(&result);

        assert!(line.contains("trend only"));
        assert!(!line.contains("delta_g_macro"));
        assert!(!line.contains("macro free energy"));
    }
}
