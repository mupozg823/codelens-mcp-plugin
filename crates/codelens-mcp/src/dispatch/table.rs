//! Static dispatch table: structural tools + feature-gated semantic handler registrations.

use crate::tools;
use std::collections::HashMap;
use std::sync::LazyLock;

pub(crate) static DISPATCH_TABLE: LazyLock<
    HashMap<&'static str, crate::tool_defs::tool::ToolHandler>,
> = LazyLock::new(|| {
    let m = tools::dispatch_table();
    #[cfg(feature = "semantic")]
    let mut m = m;
    #[cfg(feature = "semantic")]
    {
        m.insert(
            "semantic_search",
            std::sync::Arc::new(super::semantic::semantic_search_handler),
        );
        m.insert(
            "index_embeddings",
            std::sync::Arc::new(super::semantic::index_embeddings_handler),
        );
        m.insert(
            "embedding_coverage_report",
            std::sync::Arc::new(super::embedding_coverage::embedding_coverage_report_handler),
        );
        m.insert(
            "find_similar_code",
            std::sync::Arc::new(super::semantic::find_similar_code_handler),
        );
        m.insert(
            "find_code_duplicates",
            std::sync::Arc::new(super::semantic::find_code_duplicates_handler),
        );
        m.insert(
            "classify_symbol",
            std::sync::Arc::new(super::semantic::classify_symbol_handler),
        );
        m.insert(
            "find_misplaced_code",
            std::sync::Arc::new(super::semantic::find_misplaced_code_handler),
        );
    }
    m
});
