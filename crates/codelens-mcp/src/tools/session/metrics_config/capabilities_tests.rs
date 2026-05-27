use super::*;

#[test]
fn detail_default_is_full() {
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({})),
        CapabilitiesDetail::Full
    );
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({"file_path": "x.rs"})),
        CapabilitiesDetail::Full,
        "unrelated args do not flip the default"
    );
}

#[test]
fn detail_accepts_compact_and_full_case_insensitive() {
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({"detail": "compact"})),
        CapabilitiesDetail::Compact
    );
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({"detail": "COMPACT"})),
        CapabilitiesDetail::Compact
    );
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({"detail": "full"})),
        CapabilitiesDetail::Full
    );
}

#[cfg(feature = "scip-backend")]
#[test]
fn scip_status_when_compiled_with_fresh_index_is_enabled() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), b"[package]\nname=\"x\"\n").unwrap();
    std::fs::write(dir.path().join("Cargo.lock"), b"# dummy\n").unwrap();
    let index_path = dir.path().join("index.scip");
    std::fs::write(&index_path, b"placeholder").unwrap();
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    filetime::set_file_mtime(
        dir.path().join("Cargo.toml"),
        filetime::FileTime::from_system_time(past),
    )
    .unwrap();
    filetime::set_file_mtime(
        dir.path().join("Cargo.lock"),
        filetime::FileTime::from_system_time(past),
    )
    .unwrap();

    let (status, hint) = scip_status_for_response(true, dir.path());
    assert_eq!(status, "enabled");
    assert!(hint.is_none(), "fresh index needs no hint");
}

#[cfg(feature = "scip-backend")]
#[test]
fn scip_status_when_index_predates_cargo_lock_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), b"[package]\nname=\"x\"\n").unwrap();
    std::fs::write(dir.path().join("Cargo.lock"), b"# dummy\n").unwrap();
    let index_path = dir.path().join("index.scip");
    std::fs::write(&index_path, b"placeholder").unwrap();
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
    filetime::set_file_mtime(&index_path, filetime::FileTime::from_system_time(past)).unwrap();

    let (status, hint) = scip_status_for_response(true, dir.path());
    assert_eq!(status, "stale_index");
    let hint = hint.expect("stale_index must surface a regeneration hint");
    assert!(
        hint.contains("scripts/generate-scip-index.sh"),
        "regenerate hint must reference the helper script (got: {hint})"
    );
    assert!(
        hint.to_lowercase().contains("regenerate")
            || hint.to_lowercase().contains("refresh")
            || hint.contains("Cargo.lock"),
        "hint must indicate the regen rationale (got: {hint})"
    );
}

#[cfg(feature = "scip-backend")]
#[test]
fn scip_status_when_compiled_without_index_emits_setup_hint() {
    let dir = tempfile::tempdir().unwrap();
    let (status, hint) = scip_status_for_response(false, dir.path());
    assert_eq!(status, "available_no_index");
    let hint = hint.expect("setup hint required when index missing");
    assert!(
        hint.contains("scripts/generate-scip-index.sh"),
        "hint must point at the helper script (got: {hint})"
    );
    assert!(
        hint.contains("rust-analyzer scip"),
        "hint must reference the underlying tool (got: {hint})"
    );
}

#[cfg(not(feature = "scip-backend"))]
#[test]
fn scip_status_when_feature_disabled_is_not_compiled() {
    let dir = tempfile::tempdir().unwrap();
    for scip_available in [false, true] {
        let (status, hint) = scip_status_for_response(scip_available, dir.path());
        assert_eq!(status, "not_compiled");
        assert!(hint.is_none());
    }
}

#[cfg(feature = "semantic")]
#[test]
fn model_status_reflects_engine_helper_when_compiled() {
    let (status, hint) = model_status_for_response();
    if codelens_engine::embedding_model_assets_available() {
        assert_eq!(status, "loaded");
        assert!(hint.is_none(), "loaded state must not carry a setup hint");
    } else {
        assert_eq!(status, "missing");
        let hint = hint.expect("missing state must surface a setup hint");
        assert!(
            hint.contains("CODELENS_MODEL_DIR"),
            "hint must name the env var users have to set (got: {hint})"
        );
        assert!(
            hint.contains("model.onnx"),
            "hint must name the canonical model asset (got: {hint})"
        );
    }
}

#[cfg(not(feature = "semantic"))]
#[test]
fn model_status_when_feature_disabled_is_not_compiled() {
    let (status, hint) = model_status_for_response();
    assert_eq!(status, "not_compiled");
    assert!(hint.is_none());
}

#[test]
fn detail_unknown_value_falls_back_to_full() {
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({"detail": "minimal"})),
        CapabilitiesDetail::Full
    );
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({"detail": ""})),
        CapabilitiesDetail::Full
    );
    assert_eq!(
        CapabilitiesDetail::from_value(&json!({"detail": 42})),
        CapabilitiesDetail::Full
    );
}

#[test]
fn partial_index_coverage_warning_carries_breakdown() {
    let stats = codelens_engine::IndexStats {
        indexed_files: 12,
        supported_files: 30,
        stale_files: 0,
    };
    #[cfg(feature = "semantic")]
    let status = SemanticSearchStatus::Available;
    #[cfg(not(feature = "semantic"))]
    let status = SemanticSearchStatus::FeatureDisabled;
    let summary = build_health_summary(Some(&stats), &status, &json!({}));

    let warnings = summary
        .get("warnings")
        .and_then(|v| v.as_array())
        .expect("warnings array");
    let warning = warnings
        .iter()
        .find(|w| w.get("code") == Some(&json!("partial_index_coverage")))
        .expect("partial_index_coverage warning emitted");
    assert_eq!(warning["recommended_action"], json!("refresh_symbol_index"));
    assert_eq!(warning["action_target"], json!("symbol_index"));
    assert_eq!(warning["indexed_files"], json!(12));
    assert_eq!(warning["supported_files"], json!(30));
    assert_eq!(warning["unindexed_files"], json!(18));
    assert_eq!(
        warning["remediation"]["tool"],
        json!("refresh_symbol_index"),
        "remediation must name the exact tool to call"
    );
    assert_eq!(warning["remediation"]["method"], json!("tool_call"));
    assert!(
        warning["remediation"]["args"].is_object(),
        "remediation.args must be an object (even if empty)"
    );
}

#[test]
fn stale_index_warning_carries_breakdown() {
    let stats = codelens_engine::IndexStats {
        indexed_files: 30,
        supported_files: 30,
        stale_files: 5,
    };
    #[cfg(feature = "semantic")]
    let status = SemanticSearchStatus::Available;
    #[cfg(not(feature = "semantic"))]
    let status = SemanticSearchStatus::FeatureDisabled;
    let summary = build_health_summary(Some(&stats), &status, &json!({}));

    let warnings = summary
        .get("warnings")
        .and_then(|v| v.as_array())
        .expect("warnings array");
    let warning = warnings
        .iter()
        .find(|w| w.get("code") == Some(&json!("stale_index")))
        .expect("stale_index warning emitted");
    assert_eq!(warning["recommended_action"], json!("refresh_symbol_index"));
    assert_eq!(warning["action_target"], json!("symbol_index"));
    assert_eq!(warning["stale_files"], json!(5));
    assert_eq!(warning["indexed_files"], json!(30));
    assert_eq!(warning["supported_files"], json!(30));
    assert_eq!(
        warning["remediation"]["tool"],
        json!("refresh_symbol_index")
    );
    assert_eq!(warning["remediation"]["method"], json!("tool_call"));
    assert!(warning["remediation"]["args"].is_object());
}
