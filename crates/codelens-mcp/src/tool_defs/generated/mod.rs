//! Glue module for AUTO-GENERATED tool registry partials.
//!
//! Each child module here is produced by `scripts/regen-tool-defs.py`
//! from `crates/codelens-mcp/tools.toml`. This `mod.rs` is hand-edited
//! to declare which generated modules exist; the script does not
//! touch this file. See ADR-0013.

mod build_generated;

// Re-export the per-category emit functions so callers in `super::build`
// see them as `super::generated::<name>_tools` rather than reaching
// into the implementation submodule path. Visibility expansion is
// intentional: `build_generated.rs` declares its functions
// `pub(super)` (visible to `generated`), and we widen them here to
// `pub(super)` of *this* module (i.e., visible to `tool_defs`). Keeps
// the call site in `build.rs` short and stable across migration PRs.
pub(super) use build_generated::{
    analysis_tools, composite_tools, editing_tools, file_io_tools, lsp_tools, session_tools,
    symbol_tools, workflow_first_tools,
};
