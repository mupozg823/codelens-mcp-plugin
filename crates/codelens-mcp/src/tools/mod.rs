pub mod composite;
pub mod filesystem;
pub mod graph;
pub mod lsp;
pub mod memory;
pub mod mutation;
pub(crate) mod query_analysis;
mod report_contract;
pub(crate) mod report_jobs;
mod report_payload;
mod report_utils;
mod report_verifier;
pub mod reports;
pub mod rules;
pub mod session;
pub(crate) mod suggestions;
pub mod symbols;
pub(crate) mod transparency;
pub mod workflows;

use crate::AppState;
pub use crate::tool_runtime::{
    ToolResult, optional_bool, optional_string, optional_usize, required_string, success_meta,
};
// Re-export the recommendation-engine API so `crate::tools::*` consumers keep
// working after the split out of `tools/mod.rs`. `suggest_next` itself is only
// called from integration tests that go through `#[cfg(test)]`; internal
// callers use `suggest_next_contextual`, which wraps it.
#[allow(unused_imports)]
pub(crate) use suggestions::{
    composite_guidance_for_chain, infer_harness_phase, suggest_next, suggest_next_contextual,
    suggestion_reasons_for,
};

/// Rough token count estimate: 1 token ≈ 4 bytes of UTF-8 text.
/// Accuracy: ~±30% vs tiktoken cl100k_base. Sufficient for budget control,
/// not for precise measurement. JSON-heavy output tends to undercount.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Parse LSP args from arguments, falling back to defaults for the given command.
pub fn parse_lsp_args(arguments: &serde_json::Value, command: &str) -> Vec<String> {
    arguments
        .get("args")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| default_lsp_args_for_command(command))
}

pub fn default_lsp_command_for_path(file_path: &str) -> Option<String> {
    codelens_engine::default_lsp_command_for_path(file_path).map(str::to_owned)
}

pub fn default_lsp_args_for_command(command: &str) -> Vec<String> {
    codelens_engine::default_lsp_args_for_command(command)
        .unwrap_or(&[])
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect()
}
