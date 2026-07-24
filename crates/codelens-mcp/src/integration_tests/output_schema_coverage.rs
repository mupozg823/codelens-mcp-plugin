//! ADR-0016 decision 6 coverage lock: every publicly-surfaced default tool
//! must ship an `outputSchema`.
//!
//! Names are hardcoded as literals (no CORE-20 constant exists yet) and
//! pinned to `docs/adr/ADR-0016-default-surface-twenty.md` decision 2 and
//! the disposition table in
//! `docs/design/workflow-first-tool-surface-migration.md`. Semantic
//! feature-gated members are checked separately because they are absent
//! from the build under `--features http` / `--no-default-features`.

use crate::tool_defs::tool_definition;

/// CORE-20 members present in every build (semantic-independent).
/// ADR-0016 decision 2: CORE-10 + CORE-20 remainder minus `semantic_search`*.
const CORE_20_ALWAYS: &[&str] = &[
    "prepare_harness_session",
    "search",
    "overview",
    "graph",
    "diagnose",
    "review",
    "plan_safe_refactor",
    "verify_change_readiness",
    "get_changed_files",
    "get_current_config",
    "find_symbol",
    "get_ranked_context",
    "find_referencing_symbols",
    "refresh_symbol_index",
    "get_watch_status",
    "start_analysis_job",
    "get_analysis_job",
    "cancel_analysis_job",
    "get_analysis_section",
];

/// Profile-gated public tools (reviewer-graph + ci-audit) from the ADR-0016
/// decision 3 disposition table. Semantic-independent subset.
const PROFILE_PUBLIC_ALWAYS: &[&str] = &[
    "get_complexity",
    "get_symbol_importance",
    "audit_builder_session",
    "audit_planner_session",
    "audit_log_query",
    "audit_tool_surface_consistency",
    "find_phantom_modules",
    "find_redundant_definitions",
    "find_over_visible_apis",
];

/// Default-surface tools present only when the `semantic` feature is active
/// (ADR-0016 decision 2: `semantic_search`* note; `classify_symbol` is the
/// semantic reviewer-graph member).
#[cfg(feature = "semantic")]
const SEMANTIC_GATED: &[&str] = &["semantic_search", "classify_symbol"];

fn assert_has_output_schema(names: &[&str]) {
    for &name in names {
        let tool = tool_definition(name).unwrap_or_else(|| {
            panic!("ADR-0016 default-surface tool `{name}` is not registered in this build")
        });
        assert!(
            tool.output_schema.is_some(),
            "ADR-0016 decision 6: default-surface tool `{name}` is missing an outputSchema"
        );
    }
}

#[test]
fn core20_and_profile_public_tools_declare_output_schema() {
    assert_has_output_schema(CORE_20_ALWAYS);
    assert_has_output_schema(PROFILE_PUBLIC_ALWAYS);
    #[cfg(feature = "semantic")]
    assert_has_output_schema(SEMANTIC_GATED);
}

fn assert_has_full_annotations(names: &[&str]) {
    for &name in names {
        let tool = tool_definition(name).unwrap_or_else(|| {
            panic!("ADR-0016 default-surface tool `{name}` is not registered in this build")
        });
        let annotations = tool
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("ADR-0016 decision 6: `{name}` is missing annotations"));
        assert!(
            annotations.read_only_hint.is_some(),
            "ADR-0016 decision 6: `{name}` is missing readOnlyHint"
        );
        assert!(
            annotations.destructive_hint.is_some(),
            "ADR-0016 decision 6: `{name}` is missing destructiveHint"
        );
        assert!(
            annotations.idempotent_hint.is_some(),
            "ADR-0016 decision 6: `{name}` is missing idempotentHint"
        );
    }
}

/// ADR-0016 decision 6 second half: public tools ship read-only /
/// idempotent / destructive annotations, not just schemas.
#[test]
fn core20_and_profile_public_tools_declare_full_annotations() {
    assert_has_full_annotations(CORE_20_ALWAYS);
    assert_has_full_annotations(PROFILE_PUBLIC_ALWAYS);
    #[cfg(feature = "semantic")]
    assert_has_full_annotations(SEMANTIC_GATED);
}
