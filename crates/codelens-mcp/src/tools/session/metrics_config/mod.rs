pub(crate) mod capabilities;
mod metrics;
mod watch_prune;

#[cfg(test)]
mod tests;

pub use capabilities::get_capabilities;
// Re-export the internal `pub(crate)` surface that existed in the
// pre-decomposition `metrics_config.rs` so the external API is
// preserved verbatim. Nothing outside this module currently reaches
// for these, but they were reachable before and must stay reachable
// — dead-reexport warnings are suppressed to keep pure-relocation.
#[cfg(test)]
pub(crate) use capabilities::SemanticSearchStatus;
pub(crate) use capabilities::collect_runtime_health_snapshot;
pub use metrics::{export_session_markdown, get_tool_metrics};
pub use watch_prune::{get_watch_status, prune_index_failures};
