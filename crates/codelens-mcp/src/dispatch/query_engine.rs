use crate::AppState;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::ToolSurface;
use crate::tool_runtime::ToolResult;
use serde_json::Value;

use super::table::DISPATCH_TABLE;
use crate::mutation::gate::{
    MutationGateAllowance, MutationGateFailure, evaluate_mutation_gate,
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
        let handler = match DISPATCH_TABLE.get(name).copied() {
            Some(handler) => handler,
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
                    let result = handler(self.state, arguments);
                    broadcast_workflow_cache_invalidation(self.state, name, &result);
                    (result, allowance, None)
                }
                Err(failure) => {
                    if failure.missing_preflight || failure.stale {
                        self.state
                            .metrics()
                            .record_mutation_without_preflight_for_session(Some(
                                session.session_id.as_str(),
                            ));
                    }
                    if failure.rename_without_symbol_preflight {
                        self.state
                            .metrics()
                            .record_rename_without_symbol_preflight_for_session(Some(
                                session.session_id.as_str(),
                            ));
                    }
                    self.state
                        .metrics()
                        .record_mutation_preflight_gate_denied_for_session(
                            failure.stale,
                            Some(session.session_id.as_str()),
                        );
                    let message = failure.message.clone();
                    (Err(CodeLensError::Validation(message)), None, Some(failure))
                }
            }
        } else {
            let result = handler(self.state, arguments);
            broadcast_workflow_cache_invalidation(self.state, name, &result);
            (result, None, None)
        }
    }
}

/// Phase P5 slice 2b: after a successful mutation tool, drop every
/// entry in the process-wide workflow cache so a sibling session
/// reading impact_report / review_architecture immediately after
/// the mutation recomputes rather than serving a pre-mutation
/// artifact. Non-mutation tools and failed mutations are no-ops so
/// the read hot path stays zero-overhead.
fn broadcast_workflow_cache_invalidation(state: &AppState, tool_name: &str, result: &ToolResult) {
    if result.is_err() {
        return;
    }
    if !crate::tools::suggestions::MUTATION_TOOLS.contains(&tool_name) {
        return;
    }
    state.workflow_cache().invalidate_all();
}
