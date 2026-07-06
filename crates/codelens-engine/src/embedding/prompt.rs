mod api_calls;
mod auto_hint;
mod docs;
mod hint;
mod nl_tokens;
mod symbol_card;
mod test_filter;

#[cfg(not(test))]
use api_calls::extract_api_calls;
#[cfg(test)]
pub use api_calls::{extract_api_calls, extract_api_calls_inner, is_static_method_ident};
pub use auto_hint::auto_sparse_should_enable;
#[cfg(test)]
pub use auto_hint::{
    auto_hint_mode_enabled, auto_hint_should_enable, language_supports_nl_stack,
    language_supports_sparse_weighting, nl_tokens_enabled,
};
pub use docs::extract_leading_doc;
#[cfg(test)]
pub use hint::hint_char_budget;
pub use hint::{extract_body_hint, hint_line_budget, join_hint_lines};
#[cfg(not(test))]
use nl_tokens::extract_nl_tokens;
#[cfg(test)]
pub use nl_tokens::{
    contains_format_specifier, extract_comment_body, extract_nl_tokens, extract_nl_tokens_inner,
    is_nl_shaped, looks_like_error_or_log_prefix, looks_like_meta_annotation,
    should_reject_literal_strict, strict_comments_enabled, strict_literal_filter_enabled,
};
pub use test_filter::is_test_only_symbol;

use symbol_card::{build_symbol_card, symbol_card_enabled};

/// Split CamelCase/snake_case into space-separated words for embedding matching.
/// "getDonationRankings" → "get Donation Rankings"
/// "build_non_code_ranges" → "build non code ranges"
pub fn split_identifier(name: &str) -> String {
    // Only split if name is CamelCase or snake_case with multiple segments
    if !name.contains('_') && !name.chars().any(|c| c.is_uppercase()) {
        return name.to_string();
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else if ch.is_uppercase()
            && !current.is_empty()
            && (current
                .chars()
                .last()
                .map(|c| c.is_lowercase())
                .unwrap_or(false)
                || chars.get(i + 1).map(|c| c.is_lowercase()).unwrap_or(false))
        {
            // Split at CamelCase boundary, but not for ALL_CAPS
            words.push(current.clone());
            current.clear();
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    if words.len() <= 1 {
        return name.to_string(); // No meaningful split
    }
    words.join(" ")
}

/// Build the embedding text for a symbol.
///
/// Optimized for MiniLM-L12-CodeSearchNet:
/// - No "passage:" prefix (model not trained with prefixes)
/// - Include file context for disambiguation
/// - Signature-focused (body inclusion hurts quality for this model)
///
/// When `CODELENS_EMBED_DOCSTRINGS=1` is set, leading docstrings/comments are
/// appended. Disabled by default because the bundled CodeSearchNet-INT8 model
/// is optimized for code signatures and dilutes on natural language text.
/// Enable when switching to a hybrid code+text model (E5-large, BGE-base, etc).
pub fn build_embedding_text(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> String {
    // File context: use only the filename (not full path) to reduce noise.
    // Full paths like "crates/codelens-engine/src/symbols/mod.rs" add tokens
    // that dilute the semantic signal. "mod.rs" is sufficient context.
    let file_ctx = if sym.file_path.is_empty() {
        String::new()
    } else {
        let filename = sym.file_path.rsplit('/').next().unwrap_or(&sym.file_path);
        format!(" in {}", filename)
    };

    // Include split identifier words for better NL matching
    // e.g. "getDonationRankings" → "get Donation Rankings"
    let split_name = split_identifier(&sym.name);
    let name_with_split = if split_name != sym.name {
        format!("{} ({})", sym.name, split_name)
    } else {
        sym.name.clone()
    };

    // Add parent context from name_path (e.g. "UserService/get_user" → "in UserService")
    let parent_ctx = if !sym.name_path.is_empty() && sym.name_path.contains('/') {
        let parent = sym.name_path.rsplit_once('/').map(|x| x.0).unwrap_or("");
        if parent.is_empty() {
            String::new()
        } else {
            format!(" (in {})", parent)
        }
    } else {
        String::new()
    };

    // Module context: directory name provides domain signal without full path noise.
    // "embedding/mod.rs" → module "embedding", "symbols/ranking.rs" → module "symbols"
    let module_ctx = if sym.file_path.contains('/') {
        let parts: Vec<&str> = sym.file_path.rsplitn(3, '/').collect();
        if parts.len() >= 2 {
            let dir = parts[1];
            // Skip generic dirs like "src"
            if dir != "src" && dir != "crates" {
                format!(" [{dir}]")
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let base = if sym.signature.is_empty() {
        format!(
            "{} {}{}{}{}",
            sym.kind, name_with_split, parent_ctx, module_ctx, file_ctx
        )
    } else {
        format!(
            "{} {}{}{}{}: {}",
            sym.kind, name_with_split, parent_ctx, module_ctx, file_ctx, sym.signature
        )
    };
    // Docstring inclusion: v2 model improved NL understanding (+45%), enabling
    // docstrings by default. Measured: ranked_context +0.020, semantic -0.003 (neutral).
    // Disable via CODELENS_EMBED_DOCSTRINGS=0 if needed.
    let docstrings_disabled = std::env::var("CODELENS_EMBED_DOCSTRINGS")
        .map(|v| v == "0" || v == "false")
        .unwrap_or(false);

    let docstring = source
        .filter(|_| !docstrings_disabled)
        .and_then(|src| extract_leading_doc(src, sym.start_byte as usize, sym.end_byte as usize))
        .unwrap_or_default();
    let body_hint = if docstrings_disabled || !docstring.is_empty() {
        String::new()
    } else {
        source
            .and_then(|src| extract_body_hint(src, sym.start_byte as usize, sym.end_byte as usize))
            .unwrap_or_default()
    };

    let base = if symbol_card_enabled() {
        format!(
            "{} | {}",
            base,
            build_symbol_card(sym, source, !docstring.is_empty(), !body_hint.is_empty())
        )
    } else {
        base
    };

    if docstrings_disabled {
        return base;
    }

    let mut text = if docstring.is_empty() {
        // Fallback: extract the first few meaningful lines from the function
        // body. This captures key API calls (e.g. "tree_sitter::Parser",
        // "stdin()") that help the embedding model match NL queries to
        // symbols without docs.
        if body_hint.is_empty() {
            base
        } else {
            format!("{} — {}", base, body_hint)
        }
    } else {
        // Collect up to hint_line_budget() non-empty docstring lines
        // (rather than only the first) so the embedding model sees
        // multi-sentence explanations in full — up to the runtime
        // char budget via join_hint_lines.
        let line_budget = hint_line_budget();
        let lines: Vec<String> = docstring
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(line_budget)
            .map(str::to_string)
            .collect();
        let hint = join_hint_lines(&lines);
        if hint.is_empty() {
            base
        } else {
            format!("{} — {}", base, hint)
        }
    };

    // v1.5 Phase 2b experiment: optionally append NL tokens harvested from
    // comments and string literals inside the body. Disabled by default;
    // enable with `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` to A/B.
    if let Some(src) = source
        && let Some(nl_tokens) =
            extract_nl_tokens(src, sym.start_byte as usize, sym.end_byte as usize)
        && !nl_tokens.is_empty()
    {
        text.push_str(" · NL: ");
        text.push_str(&nl_tokens);
    }

    // v1.5 Phase 2c experiment: optionally append `Type::method` call-site
    // hints harvested from the body. Disabled by default; enable with
    // `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` to A/B. Orthogonal to
    // Phase 2b — both can be stacked.
    if let Some(src) = source
        && let Some(api_calls) =
            extract_api_calls(src, sym.start_byte as usize, sym.end_byte as usize)
        && !api_calls.is_empty()
    {
        text.push_str(" · API: ");
        text.push_str(&api_calls);
    }

    text
}
