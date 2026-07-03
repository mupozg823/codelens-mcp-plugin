pub(super) mod budget;
pub(super) mod envelope;
pub(super) mod payload_compact;
pub(super) mod routing_hint;
pub(super) mod text_channel;
pub(super) mod truncation;

// Re-export the 10 symbols consumed by dispatch/response.rs
pub(crate) use budget::{budget_hint, effective_budget_for_tool, max_result_size_chars_for_tool};
pub(crate) use envelope::success_jsonrpc_response;
pub(crate) use payload_compact::{compact_response_payload, trim_scaffold_for_lean};
pub(crate) use routing_hint::{apply_contextual_guidance, routing_hint_for_payload};
pub(crate) use text_channel::text_payload_for_response;
pub(crate) use truncation::{bounded_result_payload, enrich_recovery_hint_for_signals};
