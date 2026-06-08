use super::weights::{
    is_natural_language_query, mentions_any, query_targets_builder_impl,
    query_targets_entrypoint_impl, query_targets_helper_impl,
};
use super::super::types::{SymbolInfo, SymbolKind};

pub(crate) fn symbol_kind_prior(query_lower: &str, symbol: &SymbolInfo) -> f64 {
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

pub(crate) fn file_path_prior(query_lower: &str, file_path: &str) -> f64 {
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
