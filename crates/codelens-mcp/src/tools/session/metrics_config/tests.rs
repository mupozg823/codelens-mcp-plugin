// ── Phase 4a tests: capability reporting correctness ─────────────────

#[cfg(feature = "semantic")]
use super::capabilities::SemanticSearchStatus;

/// Phase 4a AC1: the LSP fallback helper must resolve a binary
/// that exists in a known install directory even when the daemon
/// `PATH` does not include it. We synthesise this situation with
/// the `CODELENS_LSP_PATH_EXTRA` env var pointing at a temp
/// directory containing a dummy file named after the query.
#[test]
fn lsp_binary_exists_finds_via_env_override() {
    let tempdir = std::env::temp_dir().join(format!(
        "codelens-phase4a-lsp-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&tempdir).expect("mkdir tempdir");
    #[cfg(windows)]
    let fake_binary = tempdir.join("phase4a-fake-lsp-server.cmd");
    #[cfg(not(windows))]
    let fake_binary = tempdir.join("phase4a-fake-lsp-server");
    std::fs::write(&fake_binary, "").expect("touch fake binary");

    let previous = std::env::var_os("CODELENS_LSP_PATH_EXTRA");
    let extra_path = std::env::join_paths([tempdir.as_path()]).expect("join extra LSP search path");
    // SAFETY: this test is synchronous and does not spawn worker
    // threads that race against env mutation.
    unsafe {
        std::env::set_var("CODELENS_LSP_PATH_EXTRA", &extra_path);
    }

    // Fast path (`which`) will fail for this fabricated binary
    // name; the env-override fallback must catch it.
    assert!(
        codelens_engine::lsp_binary_exists("phase4a-fake-lsp-server"),
        "env override fallback must resolve the dummy binary"
    );

    // Restore env
    unsafe {
        match previous {
            Some(v) => std::env::set_var("CODELENS_LSP_PATH_EXTRA", v),
            None => std::env::remove_var("CODELENS_LSP_PATH_EXTRA"),
        }
    }
    let _ = std::fs::remove_file(&fake_binary);
    let _ = std::fs::remove_dir(&tempdir);
}

/// Phase 4a AC1 negative: unknown binaries should still return
/// false so we don't produce false positives in the capability
/// report.
#[test]
fn lsp_binary_exists_returns_false_for_unknown_binary() {
    let unique = format!(
        "phase4a-definitely-not-installed-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    assert!(
        !codelens_engine::lsp_binary_exists(&unique),
        "helper must not return true for a nonexistent binary"
    );
}

/// Phase 4a AC2/AC4: the `SemanticSearchStatus::reason_str`
/// mapping must emit a distinct remediation message for each
/// non-available variant, and `None` for `Available`.
#[cfg(feature = "semantic")]
#[test]
fn semantic_search_status_reason_strings_are_distinct() {
    assert_eq!(SemanticSearchStatus::Available.reason_str(), None);
    let reasons = [
        SemanticSearchStatus::ModelAssetsUnavailable
            .reason_str()
            .unwrap(),
        SemanticSearchStatus::NotInActiveSurface
            .reason_str()
            .unwrap(),
        SemanticSearchStatus::IndexMissing.reason_str().unwrap(),
        SemanticSearchStatus::FeatureDisabled.reason_str().unwrap(),
    ];
    // All four distinct, all four mention an actionable remediation
    for (i, r) in reasons.iter().enumerate() {
        for (j, s) in reasons.iter().enumerate() {
            if i != j {
                assert_ne!(
                    r, s,
                    "SemanticSearchStatus reasons at indices {i} and {j} must be distinct"
                );
            }
        }
        assert!(
            !r.is_empty(),
            "SemanticSearchStatus reason {i} must be non-empty"
        );
    }
}

/// Phase 4a AC3: `is_available` returns true only for
/// `Available`.
#[cfg(feature = "semantic")]
#[test]
fn semantic_search_status_is_available_only_for_available_variant() {
    assert!(SemanticSearchStatus::Available.is_available());
    assert!(!SemanticSearchStatus::ModelAssetsUnavailable.is_available());
    assert!(!SemanticSearchStatus::NotInActiveSurface.is_available());
    assert!(!SemanticSearchStatus::IndexMissing.is_available());
    assert!(!SemanticSearchStatus::FeatureDisabled.is_available());
}

/// Phase 4a AC4: both Codex profiles must now expose
/// `semantic_search` and `index_embeddings`. This guards against
/// accidental removal in future preset edits.
#[cfg(feature = "semantic")]
#[test]
fn planner_readonly_and_builder_minimal_expose_semantic_search() {
    use crate::tool_defs::{ToolProfile, ToolSurface, is_tool_in_surface};

    for profile in [ToolProfile::PlannerReadonly, ToolProfile::BuilderMinimal] {
        let surface = ToolSurface::Profile(profile);
        assert!(
            is_tool_in_surface("semantic_search", surface),
            "{profile:?} must expose semantic_search (Phase 4a §capability-reporting AC4)"
        );
        assert!(
            is_tool_in_surface("index_embeddings", surface),
            "{profile:?} must expose index_embeddings (Phase 4a §capability-reporting AC4)"
        );
    }
}

/// Phase 4b AC5: the compile-time `build_info` constants must
/// be populated (non-empty) so `get_capabilities` can report
/// meaningful values. A `"unknown"` git SHA is acceptable
/// (e.g. `cargo publish` outside a git checkout), but an empty
/// string would indicate the build script did not run.
#[test]
fn build_info_constants_are_populated() {
    assert!(
        !crate::build_info::BUILD_VERSION.is_empty(),
        "BUILD_VERSION must match CARGO_PKG_VERSION and be non-empty"
    );
    assert!(
        !crate::build_info::BUILD_GIT_SHA.is_empty(),
        "BUILD_GIT_SHA must be non-empty (at minimum 'unknown')"
    );
    assert!(
        !crate::build_info::BUILD_TIME.is_empty(),
        "BUILD_TIME must be non-empty RFC 3339 UTC"
    );
    // BUILD_TIME shape: YYYY-MM-DDTHH:MM:SSZ, 20 chars
    assert_eq!(
        crate::build_info::BUILD_TIME.len(),
        20,
        "BUILD_TIME should be exactly 20 chars (RFC 3339 UTC)"
    );
    assert!(
        crate::build_info::BUILD_TIME.ends_with('Z'),
        "BUILD_TIME should end with Z (UTC marker)"
    );
    // BUILD_GIT_DIRTY parses to bool without panicking
    let _ = crate::build_info::build_git_dirty();
}
