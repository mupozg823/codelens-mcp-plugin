pub mod api;
pub mod extract;
pub mod js_imports;
pub mod language;
pub mod noise;
pub mod queries;
pub mod resolve;
pub mod types;

#[cfg(test)]
mod tests;

// Public API — keeps lib.rs:60 `pub use call_graph::{...}` working
pub use api::{get_callees, get_callers};
pub use extract::{extract_calls, extract_calls_from_source};
pub use noise::is_noise_callee;
pub use types::{CallEdge, CalleeEntry, CallerEntry};
