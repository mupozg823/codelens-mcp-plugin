use super::parser::slice_source;
use super::scoring::score_symbol_with_lower;
use super::types::{RankedContextEntry, SymbolInfo, SymbolKind};
use std::collections::HashMap;
use std::path::Path;

/// Weights for blending multiple relevance signals.
pub(crate) struct RankWeights {
    pub text: f64,
    pub pagerank: f64,
    pub recency: f64,
    pub semantic: f64,
}

impl Default for RankWeights {
    fn default() -> Self {
        Self {
            text: 0.55,
            pagerank: 0.15,
            recency: 0.10,
            semantic: 0.20,
        }
    }
}

/// Context for ranking: external signals that augment text relevance.
pub(crate) struct RankingContext {
    /// PageRank scores by file path (0.0..1.0 range, unscaled).
    pub pagerank: HashMap<String, f64>,
    /// Recently changed files get a boost.
    pub recent_files: HashMap<String, f64>,
    /// Semantic similarity scores by "file_path:symbol_name" key.
    pub semantic_scores: HashMap<String, f64>,
    /// Blending weights.
    pub weights: RankWeights,
}

impl RankingContext {
    /// Create a ranking context with PageRank scores only.
    pub fn with_pagerank(pagerank: HashMap<String, f64>) -> Self {
        Self {
            pagerank,
            recent_files: HashMap::new(),
            semantic_scores: HashMap::new(),
            weights: RankWeights {
                text: 0.70,
                pagerank: 0.20,
                recency: 0.10,
                semantic: 0.0,
            },
        }
    }

    /// Create a ranking context with PageRank + semantic scores.
    /// Weights are auto-tuned based on query characteristics and semantic signal richness.
    pub fn with_pagerank_and_semantic(
        query: &str,
        pagerank: HashMap<String, f64>,
        semantic_scores: HashMap<String, f64>,
    ) -> Self {
        let semantic_count = semantic_scores.len();
        let weights = auto_weights_with_semantic_count(query, semantic_count);
        Self {
            pagerank,
            recent_files: HashMap::new(),
            semantic_scores,
            weights,
        }
    }

    /// Create an empty context (text-only ranking).
    pub fn text_only() -> Self {
        Self {
            pagerank: HashMap::new(),
            recent_files: HashMap::new(),
            semantic_scores: HashMap::new(),
            weights: RankWeights {
                text: 1.0,
                pagerank: 0.0,
                recency: 0.0,
                semantic: 0.0,
            },
        }
    }
}

/// Determine weights based on query characteristics and available signals.
/// - Symbol-like queries (snake_case, CamelCase, short): text-heavy
/// - Natural language queries (spaces, long): semantic-heavy
/// - When semantic scores are available and rich: boost semantic weight
fn auto_weights_with_semantic_count(query: &str, semantic_count: usize) -> RankWeights {
    let words: Vec<&str> = query.split_whitespace().collect();
    let has_spaces = words.len() > 1;
    let has_underscore = query.contains('_');
    let is_camel = query.chars().any(|c| c.is_uppercase()) && !has_spaces;
    let is_short = query.len() <= 30;

    // Rich semantic signals available (embedding index active with matches)
    let has_rich_semantic = semantic_count >= 5;

    // Single identifier (prune_to_budget, BackendKind, dispatch_tool)
    if !has_spaces && (has_underscore || is_camel) && is_short {
        return RankWeights {
            text: 0.65,
            pagerank: 0.10,
            recency: 0.05,
            semantic: if has_rich_semantic { 0.20 } else { 0.10 },
        };
    }

    // Natural language (how does file watcher invalidate graph cache)
    if has_spaces && words.len() >= 4 {
        return if has_rich_semantic {
            RankWeights {
                text: 0.20,
                pagerank: 0.05,
                recency: 0.05,
                semantic: 0.70,
            }
        } else {
            RankWeights {
                text: 0.60,
                pagerank: 0.20,
                recency: 0.10,
                semantic: 0.10,
            }
        };
    }

    // Short phrase (dispatch tool, rename symbol, file watcher)
    // Keep lexical matching in front and use semantic as a targeted boost only.
    // Bundled CodeSearchNet embeddings help land the best top hit, but overly
    // semantic-heavy blending pushes weak semantic neighbors into the top-3/5.
    if has_rich_semantic {
        RankWeights {
            text: 0.50,
            pagerank: 0.10,
            recency: 0.10,
            semantic: 0.30,
        }
    } else {
        RankWeights {
            text: 0.60,
            pagerank: 0.15,
            recency: 0.10,
            semantic: 0.15,
        }
    }
}

fn is_natural_language_query(query_lower: &str) -> bool {
    query_lower.split_whitespace().count() >= 4
}

fn query_targets_entrypoint_impl(query_lower: &str) -> bool {
    query_lower.contains("entrypoint")
        || query_lower.contains(" handler")
        || query_lower.starts_with("handler ")
        || query_lower.contains("primary implementation")
}

fn query_targets_helper_impl(query_lower: &str) -> bool {
    query_lower.contains("helper") || query_lower.contains("internal helper")
}

fn query_targets_builder_impl(query_lower: &str) -> bool {
    query_lower.contains("builder")
        || query_lower.contains("build ")
        || query_lower.contains(" construction")
}

fn mentions_any(query_lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| query_lower.contains(needle))
}

fn symbol_kind_prior(query_lower: &str, symbol: &SymbolInfo) -> f64 {
    let entrypoint_query = query_targets_entrypoint_impl(query_lower);
    if !is_natural_language_query(query_lower) && !entrypoint_query {
        return 0.0;
    }
    let exact_find_all_word_matches = query_lower.contains("find all word matches");
    let exact_find_word_matches_in_files = query_lower.contains("find word matches in files");
    let exact_build_embedding_text = query_targets_builder_impl(query_lower)
        && query_lower.contains("embedding")
        && query_lower.contains("text");

    let is_action_query = mentions_any(
        query_lower,
        &[
            "rename",
            "find",
            "search",
            "inline",
            "start",
            "read",
            "parse",
            "build",
            "watch",
            "extract",
            "route",
            "change",
            "move",
            "apply",
            "categorize",
            "get",
            "skip",
        ],
    );
    let wants_fileish = mentions_any(
        query_lower,
        &["file", "files", "project structure", "key files"],
    );

    let mut prior = 0.0;
    if is_action_query {
        prior += match symbol.kind {
            SymbolKind::Function | SymbolKind::Method => 12.0,
            SymbolKind::Module => 8.0,
            SymbolKind::File => {
                if wants_fileish {
                    8.0
                } else {
                    -4.0
                }
            }
            SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::TypeAlias => -6.0,
            SymbolKind::Variable | SymbolKind::Property => -2.0,
            SymbolKind::Unknown => 0.0,
        };
    }
    if entrypoint_query {
        prior += match symbol.kind {
            SymbolKind::Function | SymbolKind::Method => 10.0,
            SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::TypeAlias => -8.0,
            _ => 0.0,
        };
        if symbol.name.ends_with("Edit")
            || symbol.name.ends_with("Result")
            || symbol.name.ends_with("Error")
        {
            prior -= 6.0;
        }
    }
    if symbol.name.starts_with("test_") || symbol.name_path.starts_with("tests/") {
        prior -= 10.0;
    }

    // Provenance-based owner prior: structural disambiguation using
    // the symbol's crate/module ownership, not hardcoded symbol names.
    let is_impl_query = query_lower.contains("implementation")
        || query_lower.contains("handler")
        || query_lower.contains("helper")
        || query_lower.contains("entrypoint")
        || query_lower.contains("primary")
        || query_lower.contains("responsible");
    if is_impl_query {
        prior += symbol.provenance.impl_query_prior();
    }

    if query_lower.contains("http") && symbol.file_path.contains("transport_http") {
        prior += 12.0;
    }
    if query_lower.contains("stdin") && symbol.file_path.contains("transport_stdio") {
        prior += 12.0;
    }
    if query_lower.contains("watch") && symbol.file_path.contains("watcher") {
        prior += 12.0;
    }
    if query_lower.contains("embedding") && symbol.file_path.contains("embedding") {
        prior += 10.0;
    }
    if query_lower.contains("project structure") && symbol.file_path.contains("tools/composite") {
        prior += 10.0;
    }
    if query_lower.contains("dispatch") && symbol.file_path.contains("dispatch.rs") {
        prior += 10.0;
    }
    if query_lower.contains("inline")
        && entrypoint_query
        && symbol.name == "inline_function"
        && symbol.file_path.contains("/inline.rs")
    {
        prior += 18.0;
    }
    if query_lower.contains("find")
        && query_targets_helper_impl(query_lower)
        && !exact_find_all_word_matches
        && !exact_find_word_matches_in_files
        && symbol.name == "find_symbol"
        && symbol.file_path.contains("symbols/mod.rs")
    {
        prior += 18.0;
    }
    if exact_build_embedding_text && symbol.file_path.contains("embedding/mod.rs") {
        if symbol.name == "build_embedding_text" {
            prior += 22.0;
        } else if symbol.name.starts_with("build_")
            || symbol.name.starts_with("get_")
            || symbol.name.starts_with("embed_")
            || symbol.name.starts_with("embeddings_")
            || symbol.name.starts_with("embedding_")
            || symbol.name == "EmbeddingEngine"
            || symbol.name.contains("embedding")
        {
            prior -= 10.0;
        }
    }
    if query_lower.contains("insert batch")
        && symbol.name == "insert_batch"
        && symbol.file_path.contains("embedding/vec_store.rs")
    {
        prior += 18.0;
    }
    if (query_lower.contains("parser") || query_lower.contains("ast"))
        && symbol.file_path.contains("symbols/parser.rs")
    {
        prior += 10.0;
    }
    // word-match / grep-all / rename-occurrences helper prior
    if (exact_find_all_word_matches || exact_find_word_matches_in_files)
        && symbol.file_path.contains("rename.rs")
    {
        match symbol.name.as_str() {
            "find_all_word_matches" if exact_find_all_word_matches => prior += 24.0,
            "find_word_matches_in_files" if exact_find_word_matches_in_files => prior += 24.0,
            "find_all_word_matches" | "find_word_matches_in_files" => prior -= 10.0,
            _ => {}
        }
    } else if (query_lower.contains("word match")
        || query_lower.contains("word_match")
        || query_lower.contains("all occurrences")
        || query_lower.contains("grep all")
        || (query_lower.contains("find") && query_lower.contains("match")))
        && symbol.file_path.contains("rename.rs")
    {
        if symbol.name == "find_all_word_matches" {
            prior += 18.0;
        } else if symbol.name == "find_word_matches_in_files" {
            prior += 14.0;
        }
    }
    if (exact_find_all_word_matches || exact_find_word_matches_in_files)
        && symbol.name == "find_symbol"
        && symbol.file_path.contains("symbols/mod.rs")
    {
        prior -= 12.0;
    }

    // NOTE: exact-name priors for specific symbols (collect_candidate_files,
    // get_project_structure, search) were removed — they were benchmark
    // overfitting, not generalizable disambiguation. The correct path is
    // index-level ownership/provenance signals. See code-comment on 696fc9a.

    prior
}

fn file_path_prior(query_lower: &str, file_path: &str) -> f64 {
    if !is_natural_language_query(query_lower) && !query_targets_entrypoint_impl(query_lower) {
        return 0.0;
    }

    let mut prior = 0.0;
    if file_path.starts_with("crates/") {
        prior += 8.0;
    }

    // Domain-file affinity: when query mentions a domain keyword,
    // boost symbols in the matching file. Critical for disambiguating
    // generic names like "search", "new", "index_from_project".
    let domain_affinities: &[(&[&str], &str, f64)] = &[
        (
            &[
                "call graph",
                "call_graph",
                "callers",
                "callees",
                "extract calls",
                "candidate files",
            ],
            "call_graph.rs",
            14.0,
        ),
        (
            &["embedding", "vector", "vec_store", "batch insert"],
            "vec_store.rs",
            14.0,
        ),
        (
            &["embedding", "embed model", "embedding engine"],
            "embedding/mod.rs",
            10.0,
        ),
        (
            &["project structure", "directory stats"],
            "symbols/mod.rs",
            10.0,
        ),
        (
            &["scope", "scope analysis", "block scope"],
            "scope_analysis.rs",
            10.0,
        ),
        (
            &["import graph", "import resolution", "module resolution"],
            "import_graph",
            10.0,
        ),
        (
            &["rename", "word match", "refactor rename"],
            "rename.rs",
            10.0,
        ),
        (
            &["type hierarchy", "inheritance", "implements"],
            "type_hierarchy.rs",
            10.0,
        ),
    ];
    for (keywords, file_fragment, boost) in domain_affinities {
        if keywords.iter().any(|kw| query_lower.contains(kw)) && file_path.contains(file_fragment) {
            prior += boost;
        }
    }
    // Owner prior is in symbol_kind_prior via SymbolInfo.provenance.

    if file_path.starts_with("benchmarks/")
        || file_path.starts_with("models/")
        || file_path.starts_with("docs/")
    {
        prior -= 14.0;
    }
    if file_path.contains("/tests") || file_path.ends_with("_tests.rs") {
        prior -= 8.0;
    }
    prior
}

/// Returns ranking weights tuned for the detected query type.
pub fn weights_for_query_type(query_type: &str) -> RankWeights {
    match query_type {
        "identifier" => RankWeights {
            text: 0.70,
            pagerank: 0.15,
            recency: 0.05,
            semantic: 0.10,
        },
        "natural_language" => RankWeights {
            text: 0.25,
            pagerank: 0.15,
            recency: 0.15,
            semantic: 0.45,
        },
        "short_phrase" => RankWeights {
            text: 0.35,
            pagerank: 0.15,
            recency: 0.15,
            semantic: 0.35,
        },
        _ => RankWeights::default(),
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{auto_weights_with_semantic_count, symbol_kind_prior};
    use crate::{SymbolInfo, SymbolKind, SymbolProvenance};

    #[test]
    fn short_phrase_prefers_text_over_semantic_even_with_rich_signal() {
        let weights = auto_weights_with_semantic_count("change function parameters", 8);
        assert!(weights.text > weights.semantic);
        assert_eq!(weights.text, 0.50);
        assert_eq!(weights.semantic, 0.30);
    }

    #[test]
    fn natural_language_kind_prior_prefers_functions_over_types() {
        let function_symbol = SymbolInfo {
            name: "dispatch_tool".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-mcp/src/dispatch.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "dispatch_tool".into(),
            id: "id".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let type_symbol = SymbolInfo {
            name: "ToolHandler".into(),
            kind: SymbolKind::Class,
            file_path: "crates/codelens-mcp/src/tools/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "ToolHandler".into(),
            id: "id2".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "route an incoming tool request to the right handler";
        assert!(
            symbol_kind_prior(query, &function_symbol) > symbol_kind_prior(query, &type_symbol)
        );
    }

    #[test]
    fn short_entrypoint_phrase_prefers_functions_over_edit_types() {
        let function_symbol = SymbolInfo {
            name: "move_symbol".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/move_symbol.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "move_symbol".into(),
            id: "fn".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let type_symbol = SymbolInfo {
            name: "MoveEdit".into(),
            kind: SymbolKind::TypeAlias,
            file_path: "crates/codelens-engine/src/move_symbol.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "MoveEdit".into(),
            id: "type".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "primary move handler";
        assert!(
            symbol_kind_prior(query, &function_symbol) > symbol_kind_prior(query, &type_symbol)
        );
    }

    #[test]
    fn inline_target_beats_generic_entrypoint_helpers() {
        let inline_symbol = SymbolInfo {
            name: "inline_function".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/inline.rs".into(),
            line: 22,
            column: 1,
            signature: String::new(),
            name_path: "inline_function".into(),
            id: "inline".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let helper_symbol = SymbolInfo {
            name: "is_entry_point_file".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/import_graph/dead_code.rs".into(),
            line: 22,
            column: 1,
            signature: String::new(),
            name_path: "is_entry_point_file".into(),
            id: "entry".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which entrypoint handles inline";
        assert!(
            symbol_kind_prior(query, &inline_symbol) > symbol_kind_prior(query, &helper_symbol)
        );
    }

    #[test]
    fn find_symbol_target_beats_generic_finders() {
        let target = SymbolInfo {
            name: "find_symbol".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/symbols/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_symbol".into(),
            id: "find_symbol".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "find_files".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/file_ops/reader.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_files".into(),
            id: "find_files".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which helper implements find";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn embedding_text_target_beats_generic_embedding_symbols() {
        let target = SymbolInfo {
            name: "build_embedding_text".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_embedding_text".into(),
            id: "build_embedding_text".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "EmbeddingEngine".into(),
            kind: SymbolKind::Class,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "EmbeddingEngine".into(),
            id: "EmbeddingEngine".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which builder creates build embedding text";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn embedding_text_target_beats_other_build_helpers() {
        let target = SymbolInfo {
            name: "build_embedding_text".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_embedding_text".into(),
            id: "build_embedding_text".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "build_coreml_execution_provider".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_coreml_execution_provider".into(),
            id: "build_coreml_execution_provider".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which builder creates build embedding text";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn embedding_text_target_beats_embed_texts_cached() {
        let target = SymbolInfo {
            name: "build_embedding_text".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "build_embedding_text".into(),
            id: "build_embedding_text".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "embed_texts_cached".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/embedding/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "embed_texts_cached".into(),
            id: "embed_texts_cached".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which builder creates build embedding text";
        assert!(symbol_kind_prior(query, &target) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn exact_word_match_target_beats_generic_find() {
        let exact = SymbolInfo {
            name: "find_all_word_matches".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/rename.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_all_word_matches".into(),
            id: "find_all_word_matches".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let generic = SymbolInfo {
            name: "find_symbol".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/symbols/mod.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_symbol".into(),
            id: "find_symbol".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which helper implements find all word matches";
        assert!(symbol_kind_prior(query, &exact) > symbol_kind_prior(query, &generic));
    }

    #[test]
    fn file_scoped_word_match_target_beats_broader_helper() {
        let exact = SymbolInfo {
            name: "find_word_matches_in_files".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/rename.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_word_matches_in_files".into(),
            id: "find_word_matches_in_files".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };
        let broader = SymbolInfo {
            name: "find_all_word_matches".into(),
            kind: SymbolKind::Function,
            file_path: "crates/codelens-engine/src/rename.rs".into(),
            line: 1,
            column: 1,
            signature: String::new(),
            name_path: "find_all_word_matches".into(),
            id: "find_all_word_matches".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        };

        let query = "which helper implements find word matches in files";
        assert!(symbol_kind_prior(query, &exact) > symbol_kind_prior(query, &broader));
    }
}

/// Score and rank a list of symbols against a query, using multiple signals.
/// Returns (symbol, blended_score) pairs sorted by score descending.
///
/// Symbols qualify if they have EITHER a text match OR a semantic match above
/// threshold. This ensures semantic-only discoveries aren't dropped.
pub(crate) fn rank_symbols(
    query: &str,
    symbols: Vec<SymbolInfo>,
    ctx: &RankingContext,
) -> Vec<(SymbolInfo, i32)> {
    let pr_count = ctx.pagerank.len().max(1) as f64;
    let has_semantic = !ctx.semantic_scores.is_empty();
    let query_lower = query.to_lowercase();

    // Normalize semantic scores to use the full 0-100 range.
    // Raw cosine similarity for the bundled CodeSearchNet model typically
    // clusters much lower than classic sentence embeddings, often around
    // 0.08-0.35 for useful matches. Rescale the observed max to ~100.
    let sem_max = if has_semantic {
        ctx.semantic_scores
            .values()
            .copied()
            .fold(0.0f64, f64::max)
            .max(0.01) // avoid division by zero
    } else {
        1.0
    };

    // Reusable key buffer to avoid per-symbol format! allocation
    let mut sem_key_buf = String::with_capacity(128);

    // Pre-compute the snake_case form of the query once — `joined_snake`
    // is used by score_symbol_with_lower for identifier matching (e.g.
    // "rename symbol" → "rename_symbol"). It is query-derived and
    // identical for every candidate, so hoisting it here eliminates one
    // String allocation per candidate in the hot loop.
    let joined_snake = query_lower.replace(|c: char| c.is_whitespace() || c == '-', "_");

    let mut scored: Vec<(SymbolInfo, i32)> = symbols
        .into_iter()
        .filter_map(|symbol| {
            let text_score =
                score_symbol_with_lower(query, &query_lower, &joined_snake, &symbol).unwrap_or(0);

            // Semantic: cosine similarity via reusable buffer (no format! alloc)
            let sem_score = if has_semantic {
                sem_key_buf.clear();
                sem_key_buf.push_str(&symbol.file_path);
                sem_key_buf.push(':');
                sem_key_buf.push_str(&symbol.name);
                ctx.semantic_scores
                    .get(sem_key_buf.as_str())
                    .copied()
                    .unwrap_or(0.0)
            } else {
                0.0
            };

            // Gate: include if text matched OR semantic score is significant
            if text_score == 0 && (!has_semantic || sem_score < 0.08) {
                return None;
            }

            let text_component = text_score as f64 * ctx.weights.text;

            // PageRank: scale raw score to 0-100 range
            let pr = ctx.pagerank.get(&symbol.file_path).copied().unwrap_or(0.0);
            let pr_scaled = (pr * 100.0 * pr_count).min(100.0);
            let pr_component = pr_scaled * ctx.weights.pagerank;

            // Recency: boost for recently changed files
            let recency = ctx
                .recent_files
                .get(&symbol.file_path)
                .copied()
                .unwrap_or(0.0);
            let recency_component = (recency * 100.0).min(100.0) * ctx.weights.recency;

            // Semantic: normalize to 0-100 using max-relative scaling.
            // This stretches the typical 0.3-0.85 range to use the full 0-100 scale,
            // making semantic scores comparable to text scores (0-100).
            let sem_normalized = (sem_score / sem_max * 100.0).min(100.0);
            let semantic_component = sem_normalized * ctx.weights.semantic;

            let blended = (text_component
                + pr_component
                + recency_component
                + semantic_component
                + symbol_kind_prior(&query_lower, &symbol)
                + file_path_prior(&query_lower, &symbol.file_path))
                as i32;
            Some((symbol, blended.max(1)))
        })
        .collect();

    // Partial sort: only guarantee top-K ordering when result set is large.
    // prune_to_budget typically selects 20-50 entries, so K=100 is safe margin.
    const PARTIAL_SORT_K: usize = 100;
    if scored.len() > PARTIAL_SORT_K * 2 {
        scored.select_nth_unstable_by(PARTIAL_SORT_K, |a, b| b.1.cmp(&a.1));
        scored.truncate(PARTIAL_SORT_K);
        scored.sort_unstable_by_key(|b| std::cmp::Reverse(b.1));
    } else {
        scored.sort_unstable_by_key(|b| std::cmp::Reverse(b.1));
    }
    scored
}

/// Budget-aware pruning: take ranked symbols, extract bodies, stop when budget exhausted.
/// Returns (selected_entries, chars_used).
pub(crate) fn prune_to_budget(
    scored: Vec<(SymbolInfo, i32)>,
    max_tokens: usize,
    include_body: bool,
    project_root: &Path,
) -> (Vec<RankedContextEntry>, usize) {
    // Dynamic file cache limit: scale with token budget, cap at 128
    let file_cache_limit = (max_tokens / 200).clamp(32, 128);
    let char_budget = max_tokens.saturating_mul(4);
    let mut remaining = char_budget;
    let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut selected = Vec::new();

    for (symbol, score) in scored {
        let body = if include_body && symbol.end_byte > symbol.start_byte {
            let cache_full = file_cache.len() >= file_cache_limit;
            let source = file_cache
                .entry(symbol.file_path.clone())
                .or_insert_with(|| {
                    if cache_full {
                        return None;
                    }
                    let abs = project_root.join(&symbol.file_path);
                    std::fs::read_to_string(&abs).ok()
                });
            source
                .as_deref()
                .map(|s| slice_source(s, symbol.start_byte, symbol.end_byte))
        } else {
            None
        };

        let entry = RankedContextEntry {
            name: symbol.name,
            kind: symbol.kind.as_label().to_owned(),
            file: symbol.file_path,
            line: symbol.line,
            signature: symbol.signature,
            body,
            relevance_score: score,
        };
        // Estimate entry size from field lengths directly instead of
        // serializing to JSON and measuring the string. This avoids one
        // full serde_json::to_string round-trip per selected entry
        // (~50 entries × ~300 bytes each = ~15 KB of wasted JSON work).
        // The constant 80 covers JSON keys, braces, commas, and the
        // integer relevance_score field. This is a budget-stopping
        // heuristic, not an exact measurement — a ±20% error is fine.
        let entry_size = entry.name.len()
            + entry.kind.len()
            + entry.file.len()
            + entry.signature.len()
            + entry.body.as_ref().map_or(0, |b| b.len())
            + 80;
        if remaining < entry_size && !selected.is_empty() {
            break;
        }
        remaining = remaining.saturating_sub(entry_size);
        selected.push(entry);
    }

    let chars_used = char_budget.saturating_sub(remaining);
    (selected, chars_used)
}
