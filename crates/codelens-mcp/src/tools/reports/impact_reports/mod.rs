mod boundary;
mod helpers;
mod impact;
mod mermaid;
mod refactor;
mod workspace_modules;

use helpers::{
    analysis_completeness_section, build_dead_code_semantic_query, build_module_semantic_query,
    file_name, impact_entry_file, insert_semantic_status, mermaid_escape_label, parent_dir,
    semantic_degraded_note, validate_architecture_scope, verifier_files_for_path,
};
#[cfg(test)]
use mermaid::render_module_mermaid;

pub use boundary::{dead_code_report, module_boundary_report};
pub use impact::{diff_aware_references, impact_report};
pub use mermaid::mermaid_module_graph;
pub use refactor::{refactor_safety_report, semantic_code_review};
