mod boundary;
mod helpers;
mod impact;
mod mermaid;
mod refactor;

use helpers::{
    build_dead_code_semantic_query, build_module_semantic_query, file_name, impact_entry_file,
    insert_semantic_status, mermaid_escape_label, parent_dir, push_unique, semantic_degraded_note,
};
#[cfg(test)]
use mermaid::render_module_mermaid;

pub use boundary::{dead_code_report, module_boundary_report};
pub use impact::{diff_aware_references, impact_report};
pub use mermaid::mermaid_module_graph;
pub use refactor::{refactor_safety_report, semantic_code_review};
