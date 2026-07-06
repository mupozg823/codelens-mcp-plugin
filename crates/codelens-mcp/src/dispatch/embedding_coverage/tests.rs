use super::*;
use codelens_engine::{EmbeddingCoverageReport, EmbeddingStaleFileReason, EmbeddingStaleReason};

#[test]
fn model_mismatch_and_file_stale_remediation_are_distinct() {
    assert_eq!(
        recommended_action("model_mismatch"),
        "reindex_embeddings_for_model"
    );
    assert_eq!(recommended_action("stale"), "refresh_embedding_index");
    assert_ne!(
        recommended_action("model_mismatch"),
        recommended_action("stale")
    );
}

#[test]
fn coverage_index_payload_separates_freshness_dimensions() {
    let coverage = EmbeddingCoverageReport {
        model_name: "MiniLM-L12-CodeSearchNet-INT8".to_owned(),
        indexed_symbols: 2,
        indexed_files: 1,
        checked_files: 1,
        ready_files: 0,
        readiness_percent: 0,
        stale_files: 1,
        current_git_sha: Some("current-sha".to_owned()),
        last_index_sha: Some("old-sha".to_owned()),
        stale_file_reasons: vec![EmbeddingStaleFileReason {
            file_path: "main.py".to_owned(),
            reason: EmbeddingStaleReason::EmbeddingKeysChanged,
        }],
        ..EmbeddingCoverageReport::default()
    };

    let payload = coverage_index_payload(&coverage, "MiniLM-L12-CodeSearchNet-INT8");

    assert_eq!(payload["freshness"]["model"]["status"], json!("ready"));
    assert_eq!(payload["freshness"]["git"]["status"], json!("stale"));
    assert_eq!(payload["freshness"]["files"]["status"], json!("stale"));
    assert_eq!(
        payload["freshness"]["files"]["recommended_action"],
        json!("refresh_embedding_index")
    );
    assert_eq!(
        payload["freshness"]["model"]["recommended_action"],
        json!("none")
    );
}
