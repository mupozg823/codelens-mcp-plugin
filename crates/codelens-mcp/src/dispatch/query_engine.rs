use crate::AppState;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::ToolSurface;
use crate::tool_runtime::ToolResult;
use serde_json::Value;

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
        let tool = match DISPATCH_TABLE.get(name) {
            Some(t) => t,
            None => {
                return (
                    Err(CodeLensError::ToolNotFound(name.to_owned())),
                    None,
                    None,
                );
            }
        };

        if is_refactor_gated_mutation_tool(name) {
            self.state
                .metrics()
                .record_mutation_preflight_checked_for_session(Some(session.session_id.as_str()));
            match evaluate_mutation_gate(self.state, name, session, surface, arguments) {
                Ok(allowance) => {
                    let result = tool(self.state, arguments);
                    (result, allowance, None)
                }
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
            let result = tool(self.state, arguments);
            (result, None, None)
        }
    }
}
