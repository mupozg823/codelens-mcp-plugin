use crate::AppState;
use crate::dispatch::access::{validate_tool_access, validate_tool_role};
use crate::dispatch::envelope::validate_required_params;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::ToolSurface;
use crate::tool_runtime::ToolResult;
use serde_json::Value;
use std::borrow::Cow;

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

        if is_refactor_gated_mutation_tool(target) {
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
        }
    }
}
