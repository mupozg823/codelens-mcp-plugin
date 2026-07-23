use crate::AppState;
use crate::dispatch::access::{validate_tool_access, validate_tool_role};
use crate::dispatch::envelope::validate_required_params;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::ToolSurface;
use crate::tool_runtime::ToolResult;
use serde_json::Value;
use std::borrow::Cow;
use std::sync::Arc;

use super::table::DISPATCH_TABLE;
use crate::mutation_gate::{
    MutationFailureKind, MutationGateAllowance, MutationGateFailure, evaluate_mutation_gate,
    is_refactor_gated_mutation_tool,
};
use crate::operation::ResolvedOperation;

pub(super) struct QuerySubmission<'a> {
    pub(super) result: ToolResult,
    pub(super) gate_allowance: Option<MutationGateAllowance>,
    pub(super) gate_failure: Option<MutationGateFailure>,
    pub(super) operation: ResolvedOperation<'a>,
}

/// Orchestrates tool discovery, validation, and lifecycle execution.
/// Modeled after Claude Code's QueryEngine.
pub struct QueryEngine<'a> {
    state: &'a AppState,
}

/// End-of-request fence for symbol-backed reads. Holding the exact index Arc
/// prevents a session rebind from making the final comparison against another
/// project's token.
struct SymbolGenerationFence {
    index: Arc<codelens_engine::SymbolIndex>,
    project: String,
    before: u64,
}

impl SymbolGenerationFence {
    fn new(index: Arc<codelens_engine::SymbolIndex>, project: String) -> Self {
        let before = index.committed_generation();
        Self {
            index,
            project,
            before,
        }
    }

    fn capture_if_required(state: &AppState, target: &str) -> Option<Self> {
        crate::tool_defs::tool_symbol_generation_consistent(target).then(|| {
            Self::new(
                state.symbol_index(),
                state.project().as_path().display().to_string(),
            )
        })
    }

    fn finish(self, result: ToolResult) -> ToolResult {
        let Ok(payload) = result else {
            return result;
        };
        let after = self.index.committed_generation();
        if after == self.before {
            return Ok(payload);
        }
        Err(CodeLensError::IndexGenerationChanged {
            project: self.project,
            before: self.before,
            after,
        })
    }
}

impl<'a> QueryEngine<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    /// Submits a tool execution message, enforcing enabled status and concurrency/mutation gates.
    pub fn submit_message<'b>(
        &self,
        name: &'b str,
        arguments: &'b Value,
        session: &SessionRequestContext,
        surface: ToolSurface,
    ) -> QuerySubmission<'b> {
        let (target, target_arguments, operation) =
            match crate::tools::verbs::resolve_verb_target(name, arguments) {
                Ok(Some((target, target_arguments))) => (
                    target,
                    Cow::Owned(target_arguments),
                    ResolvedOperation::resolved(
                        target,
                        arguments.get("mode").and_then(Value::as_str),
                    ),
                ),
                Ok(None) => (
                    name,
                    Cow::Borrowed(arguments),
                    ResolvedOperation::direct(name),
                ),
                Err(error) => {
                    return QuerySubmission {
                        result: Err(error),
                        gate_allowance: None,
                        gate_failure: None,
                        operation: ResolvedOperation::unresolved(
                            arguments.get("mode").and_then(Value::as_str),
                        ),
                    };
                }
            };
        let target_arguments = target_arguments.as_ref();

        if let Err(error) = validate_required_params(target, target_arguments) {
            return QuerySubmission {
                result: Err(error),
                gate_allowance: None,
                gate_failure: None,
                operation,
            };
        }

        if let Err(error) = validate_tool_role(self.state, target, session) {
            return QuerySubmission {
                result: Err(error),
                gate_allowance: None,
                gate_failure: None,
                operation,
            };
        }
        // A default-listed facade is an explicit bootstrap contract: callers
        // may invoke it before loading a namespace/tier. Its internal target
        // inherits only that deferred-loading allowance; target profile, role,
        // trusted-client, daemon-mode, and mutation checks remain unchanged.
        let access_result = if target != name && crate::tool_defs::tool_default_listed(name) {
            let mut facade_session = session.clone();
            facade_session.full_tool_exposure = true;
            validate_tool_access(target, &facade_session, surface, self.state)
        } else {
            validate_tool_access(target, session, surface, self.state)
        };
        if let Err(error) = access_result {
            return QuerySubmission {
                result: Err(error),
                gate_allowance: None,
                gate_failure: None,
                operation,
            };
        }

        let tool = match DISPATCH_TABLE.get(target) {
            Some(t) => t,
            None => {
                // Tombstoned names (#346) get a replacement hint instead of a
                // bare unknown-tool error; the JSON-RPC error code is the same.
                let detail = match crate::tools::tombstone_guidance(target) {
                    Some(guidance) => format!("{name} — {guidance}"),
                    None => target.to_owned(),
                };
                return QuerySubmission {
                    result: Err(CodeLensError::ToolNotFound(detail)),
                    gate_allowance: None,
                    gate_failure: None,
                    operation,
                };
            }
        };

        // Capture after verb resolution and access validation so facades
        // inherit the target's contract and rejected calls never pin an index.
        let generation_fence = SymbolGenerationFence::capture_if_required(self.state, target);

        let mut submission = if is_refactor_gated_mutation_tool(target) {
            self.state
                .metrics()
                .record_mutation_preflight_checked_for_session(Some(session.session_id.as_str()));
            match evaluate_mutation_gate(self.state, target, session, surface, target_arguments) {
                Ok(allowance) => QuerySubmission {
                    result: tool(self.state, target_arguments),
                    gate_allowance: allowance,
                    gate_failure: None,
                    operation: operation.dispatched(),
                },
                Err(failure) => {
                    if matches!(
                        failure.kind,
                        MutationFailureKind::MissingPreflight | MutationFailureKind::StalePreflight
                    ) {
                        self.state
                            .metrics()
                            .record_mutation_without_preflight_for_session(Some(
                                session.session_id.as_str(),
                            ));
                    }
                    if matches!(
                        failure.kind,
                        MutationFailureKind::SymbolPreflightRequired
                            | MutationFailureKind::SymbolMismatch
                    ) {
                        self.state
                            .metrics()
                            .record_rename_without_symbol_preflight_for_session(Some(
                                session.session_id.as_str(),
                            ));
                    }
                    self.state
                        .metrics()
                        .record_mutation_preflight_gate_denied_for_session(
                            matches!(failure.kind, MutationFailureKind::StalePreflight),
                            Some(session.session_id.as_str()),
                        );
                    let message = failure.message.clone();
                    QuerySubmission {
                        result: Err(CodeLensError::Validation(message)),
                        gate_allowance: None,
                        gate_failure: Some(failure),
                        operation,
                    }
                }
            }
        } else {
            QuerySubmission {
                result: tool(self.state, target_arguments),
                gate_allowance: None,
                gate_failure: None,
                operation: operation.dispatched(),
            }
        };
        submission.result = match generation_fence {
            Some(fence) => fence.finish(submission.result),
            None => submission.result,
        };
        submission
    }
}

#[cfg(test)]
mod generation_consistency_tests {
    use super::*;
    use crate::protocol::BackendKind;
    use crate::tool_runtime::success_meta;
    use codelens_engine::{ProjectRoot, SymbolIndex};
    use serde_json::json;
    use std::fs;
    use std::sync::Arc;

    #[test]
    fn generation_consistency_metadata_is_explicit_and_excludes_writers() {
        for target in [
            "find_symbol",
            "get_callers",
            "impact_report",
            "review_architecture",
        ] {
            assert!(
                crate::tool_defs::tool_symbol_generation_consistent(target),
                "{target} must fence one logical symbol-backed response"
            );
        }
        for target in [
            "refresh_symbol_index",
            "prepare_harness_session",
            "start_analysis_job",
            "get_analysis_job",
        ] {
            assert!(
                !crate::tool_defs::tool_symbol_generation_consistent(target),
                "{target} must not reject its own mutation/job response"
            );
        }
    }

    #[test]
    fn verb_facade_inherits_resolved_target_generation_contract() {
        let (target, _) = crate::tools::verbs::resolve_verb_target(
            "graph",
            &json!({"mode": "callers", "symbol": "dispatch_tool"}),
        )
        .expect("resolve graph facade")
        .expect("graph facade target");
        assert_eq!(target, "get_callers");
        assert!(crate::tool_defs::tool_symbol_generation_consistent(target));
    }

    #[test]
    fn successful_payload_is_discarded_when_generation_changes_mid_request() {
        let root = tempfile::tempdir().expect("temp project");
        let src = root.path().join("src");
        fs::create_dir_all(&src).expect("create src");
        let file = src.join("lib.rs");
        fs::write(&file, "pub fn before() {}\n").expect("write source");
        let project = ProjectRoot::new(root.path()).expect("project root");
        let index = Arc::new(SymbolIndex::new(project).expect("symbol index"));
        let fence =
            SymbolGenerationFence::new(Arc::clone(&index), root.path().display().to_string());

        index
            .index_files(std::slice::from_ref(&file))
            .expect("commit newer generation during request");
        let result = fence.finish(Ok((
            json!({"must_be_discarded": true}),
            success_meta(BackendKind::Sqlite, 1.0),
        )));

        match result {
            Err(CodeLensError::IndexGenerationChanged {
                project,
                before,
                after,
            }) => {
                assert_eq!(project, root.path().display().to_string());
                assert!(after > before, "before={before}, after={after}");
            }
            other => panic!("expected generation retry error, got {other:?}"),
        }
    }

    #[test]
    fn stable_generation_returns_payload_and_existing_error_unchanged() {
        let root = tempfile::tempdir().expect("temp project");
        let project = ProjectRoot::new(root.path()).expect("project root");
        let index = Arc::new(SymbolIndex::new(project).expect("symbol index"));
        let stable =
            SymbolGenerationFence::new(Arc::clone(&index), root.path().display().to_string())
                .finish(Ok((
                    json!({"stable": true}),
                    success_meta(BackendKind::Sqlite, 1.0),
                )));
        assert!(stable.is_ok());

        let original = SymbolGenerationFence::new(index, root.path().display().to_string())
            .finish(Err(CodeLensError::Validation("original".to_owned())));
        assert!(
            matches!(original, Err(CodeLensError::Validation(message)) if message == "original")
        );
    }
}
