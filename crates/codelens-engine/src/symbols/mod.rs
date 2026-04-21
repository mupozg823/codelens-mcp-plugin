mod api;
mod index;
mod parser;
mod ranking;
mod reader;
pub mod scoring;
mod support;
#[cfg(test)]
mod tests;
mod types;
mod writer;

pub use api::{find_symbol, find_symbol_range, get_symbols_overview};
pub use index::SymbolIndex;
pub use scoring::{
    sparse_coverage_bonus_from_fields, sparse_max_bonus, sparse_threshold, sparse_weighting_enabled,
};
pub(crate) use support::{collect_candidate_files, file_modified_ms};
pub(crate) use types::ReadDb;
pub use types::{
    IndexStats, RankedContextEntry, RankedContextResult, SymbolInfo, SymbolKind, SymbolProvenance,
    make_symbol_id, parse_symbol_id,
};

// Re-export language_for_path so downstream crate modules keep working.
pub(crate) use crate::lang_config::{LanguageConfig, language_for_path};
