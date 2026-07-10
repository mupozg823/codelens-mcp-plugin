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
    pub fn submit_message(
        &self,
        name: &str,
        arguments: &Value,
        session: &SessionRequestContext,
        surface: ToolSurface,
    ) -> (
        ToolResult,
        Option<MutationGateAllowance>,
        Option<MutationGateFailure>,
    ) {
        let (target, target_arguments) =
            match crate::tools::verbs::resolve_verb_target(name, arguments) {
                Ok(Some((target, arguments))) => (target, Cow::Owned(arguments)),
                Ok(None) => (name, Cow::Borrowed(arguments)),
                Err(error) => return (Err(error), None, None),
            };
        let target_arguments = target_arguments.as_ref();

        if let Err(error) = validate_required_params(target, target_arguments) {
            return (Err(error), None, None);
        }

        if let Err(error) = validate_tool_role(self.state, target, session) {
            return (Err(error), None, None);
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
            return (Err(error), None, None);
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
                return (Err(CodeLensError::ToolNotFound(detail)), None, None);
            }
        };

        if is_refactor_gated_mutation_tool(target) {
            self.state
                .metrics()
                .record_mutation_preflight_checked_for_session(Some(session.session_id.as_str()));
            match evaluate_mutation_gate(self.state, target, session, surface, target_arguments) {
                Ok(allowance) => (tool(self.state, target_arguments), allowance, None),
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
                    (Err(CodeLensError::Validation(message)), None, Some(failure))
                }
            }
        } else {
            (tool(self.state, target_arguments), None, None)
        }
    }
}
