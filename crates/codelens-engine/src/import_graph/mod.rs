mod cache;
mod dead_code;
mod graph;
mod parsers;
mod queries;
mod resolvers;
#[cfg(test)]
mod tests;
mod types;

pub use cache::GraphCache;
pub use dead_code::{DeadCodeEntryV2, find_dead_code, find_dead_code_v2};
pub use parsers::extract_imports_for_file;
pub use parsers::extract_imports_from_source;
pub use queries::{
    get_blast_radius, get_importance, get_importers, is_import_supported, supports_import_graph,
};
pub use resolvers::resolve_module_for_file;
pub use types::{BlastRadiusEntry, DeadCodeEntry, FileNode, ImportanceEntry, ImporterEntry};

pub(crate) use graph::{build_graph_pub, collect_candidate_files};
