pub(crate) mod capabilities;
mod metrics;
mod preset_profile;
mod watch_prune;

#[cfg(test)]
mod tests;

pub use capabilities::get_capabilities;
// Re-export the internal `pub(crate)` surface that existed in the
// pre-decomposition `metrics_config.rs` so the external API is
// preserved verbatim. Nothing outside this module currently reaches
// for these, but they were reachable before and must stay reachable
// — dead-reexport warnings are suppressed to keep pure-relocation.
#[allow(unused_imports)]
pub(crate) use capabilities::{
    DiagnosticsGuidance, DiagnosticsStatus, RuntimeHealthSnapshot, SemanticSearchStatus,
    build_health_summary, collect_runtime_health_snapshot, determine_semantic_search_status,
};
pub use metrics::{export_session_markdown, get_tool_metrics};
pub use preset_profile::{set_preset, set_profile};
pub use watch_prune::{get_watch_status, prune_index_failures};
