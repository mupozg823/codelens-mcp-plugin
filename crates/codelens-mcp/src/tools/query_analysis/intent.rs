#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetrievalQueryAnalysis {
    pub original_query: String,
    pub semantic_query: String,
    pub expanded_query: String,
    pub prefer_lexical_only: bool,
    pub natural_language: bool,
    pub prefer_sparse_symbol_search: bool,
}

pub(super) fn query_prefers_lexical_only(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains(char::is_whitespace) {
        return false;
    }
    let looks_path_like = trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("::");
    let identifier_chars_only = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    looks_path_like || identifier_chars_only
}

pub(super) fn query_prefers_sparse_symbol_search(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if query_prefers_lexical_only(trimmed) {
        return true;
    }
    let token_count = trimmed
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|t| !t.is_empty())
        .count();
    (2..=4).contains(&token_count) && !is_natural_language_query(trimmed)
}

fn is_natural_language_query(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && !query_prefers_lexical_only(trimmed)
        && trimmed.split_whitespace().count() >= 3
}

pub(super) fn has_entrypoint_cue(query_lower: &str) -> bool {
    query_lower.contains("entrypoint")
        || query_lower.contains("handler")
        || query_lower.contains("primary implementation")
}

pub(super) fn has_helper_cue(query_lower: &str) -> bool {
    query_lower.contains("helper") || query_lower.contains("internal helper")
}

pub(super) fn has_builder_cue(query_lower: &str) -> bool {
    query_lower.contains("builder")
        || query_lower.contains("build ")
        || query_lower.contains(" construction")
}

fn exact_retrieval_aliases(query_lower: &str) -> Option<&'static [&'static str]> {
    if query_lower.contains("find word matches in files") {
        Some(&["find_word_matches_in_files", "word_matches_in_files"])
    } else if query_lower.contains("find all word matches") {
        Some(&["find_all_word_matches", "all_word_matches"])
    } else if has_builder_cue(query_lower)
        && query_lower.contains("embedding")
        && query_lower.contains("text")
    {
        Some(&["build_embedding_text", "embedding_text"])
    } else {
        None
    }
}

pub(super) fn specific_find_aliases(query_lower: &str) -> &'static [&'static str] {
    if query_lower.contains("find word matches in files") {
        &["find_word_matches_in_files", "word_matches_in_files"]
    } else if query_lower.contains("find all word matches") {
        &["find_all_word_matches", "all_word_matches"]
    } else if query_lower.contains("find") && has_helper_cue(query_lower) {
        &["find_symbol", "find"]
    } else {
        &[]
    }
}

fn split_identifier_terms(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty()
        || trimmed.contains(char::is_whitespace)
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("::")
    {
        return None;
    }

    let mut split = String::with_capacity(trimmed.len() + 4);
    let mut last_emitted_is_lowercase = false;
    let mut in_segment = false;
    let mut iter = trimmed.chars().peekable();

    while let Some(ch) = iter.next() {
        if ch == '_' || ch == '-' {
            if !split.is_empty() && !split.ends_with(' ') {
                split.push(' ');
            }
            in_segment = false;
            last_emitted_is_lowercase = false;
            continue;
        }

        let next_is_lowercase = iter.peek().map(|c| c.is_lowercase()).unwrap_or(false);
        if ch.is_uppercase() && in_segment && (last_emitted_is_lowercase || next_is_lowercase) {
            split.push(' ');
        }

        for lowered in ch.to_lowercase() {
            split.push(lowered);
            last_emitted_is_lowercase = lowered.is_lowercase();
        }
        in_segment = true;
    }

    split.contains(' ').then_some(split)
}

fn semantic_identifier_query(alias: &str) -> String {
    match split_identifier_terms(alias) {
        Some(split) if split != alias => format!("{alias} {split}"),
        _ => alias.to_owned(),
    }
}

pub(crate) fn analyze_retrieval_query(query: &str) -> RetrievalQueryAnalysis {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return RetrievalQueryAnalysis {
            original_query: String::new(),
            semantic_query: String::new(),
            expanded_query: String::new(),
            prefer_lexical_only: false,
            natural_language: false,
            prefer_sparse_symbol_search: false,
        };
    }

    let prefer_lexical_only = query_prefers_lexical_only(trimmed);
    let natural_language = is_natural_language_query(trimmed);
    let prefer_sparse_symbol_search = query_prefers_sparse_symbol_search(trimmed);
    let lowered = trimmed.to_ascii_lowercase();
    let exact_aliases = exact_retrieval_aliases(&lowered);
    let alias_expansion_phrase = trimmed.contains(' ')
        && (has_entrypoint_cue(&lowered) || has_helper_cue(&lowered) || has_builder_cue(&lowered));

    let semantic_query = if let Some(aliases) = exact_aliases {
        semantic_identifier_query(aliases[0])
    } else if natural_language && !alias_expansion_phrase {
        trimmed.to_owned()
    } else if alias_expansion_phrase && has_builder_cue(&lowered) {
        // Builder queries: semantic query uses identifier-only form so the
        // embedding model matches code symbols, not NL prose.
        // "which builder creates build embedding text" → "build_embedding_text embedding_text"
        let expanded = super::expansion::expand_retrieval_query(trimmed);
        let identifiers: Vec<&str> = expanded
            .split_whitespace()
            .filter(|t| t.contains('_') || t.chars().any(|c| c.is_uppercase()))
            .collect();
        if identifiers.is_empty() {
            expanded
        } else {
            identifiers.join(" ")
        }
    } else if alias_expansion_phrase {
        super::expansion::expand_retrieval_query(trimmed)
    } else if let Some(split) = split_identifier_terms(trimmed) {
        if split != trimmed {
            format!("{trimmed} {split}")
        } else {
            trimmed.to_owned()
        }
    } else {
        trimmed.to_owned()
    };

    let expanded_query = if let Some(aliases) = exact_aliases {
        aliases[0].to_owned()
    } else if natural_language {
        super::expansion::expand_retrieval_query(trimmed)
    } else {
        trimmed.to_owned()
    };

    RetrievalQueryAnalysis {
        original_query: trimmed.to_owned(),
        semantic_query,
        expanded_query,
        prefer_lexical_only,
        natural_language,
        prefer_sparse_symbol_search,
    }
}

pub(crate) fn semantic_query_for_retrieval(query: &str) -> String {
    analyze_retrieval_query(query).semantic_query
}
