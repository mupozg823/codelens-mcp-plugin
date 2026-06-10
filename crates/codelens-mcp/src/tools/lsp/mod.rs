pub mod diagnostics;
pub mod navigation;
pub mod references;
pub mod rename;
pub mod shared;
pub mod status;
pub mod symbols;

// Re-export the tools wired into the dispatch table at
// `crates/codelens-mcp/src/tools/mod.rs` and used from
// report_verifier / workflows.
pub use diagnostics::{get_diagnostics_for_symbol, get_file_diagnostics};
pub use navigation::{find_declaration, find_implementations};
pub use references::find_referencing_symbols;
pub use rename::{plan_symbol_rename, resolve_symbol_target};
pub use status::get_lsp_recipe;
pub use symbols::{get_type_hierarchy, search_workspace_symbols};
