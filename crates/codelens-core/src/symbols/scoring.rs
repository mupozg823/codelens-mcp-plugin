use super::types::SymbolInfo;

/// Check if any query token is a common programming action verb.
fn query_has_action_verb(tokens: &[&str]) -> bool {
    const ACTION_VERBS: &[&str] = &[
        "find",
        "get",
        "search",
        "detect",
        "start",
        "run",
        "read",
        "write",
        "move",
        "change",
        "rename",
        "replace",
        "extract",
        "route",
        "embed",
        "build",
        "create",
        "delete",
        "update",
        "compute",
        "calculate",
        "apply",
        "handle",
        "parse",
        "index",
        "watch",
        "listen",
        "fetch",
        "send",
        "load",
        "save",
        "open",
        "close",
        "connect",
        "check",
        "validate",
        "verify",
        "transform",
        "convert",
        "process",
        "execute",
        "call",
        "invoke",
        "inline",
        "refactor",
        "analyze",
        "import",
        "export",
    ];
    tokens.iter().any(|t| ACTION_VERBS.contains(t))
}

/// Split a CamelCase or PascalCase name into lowercase tokens.
/// e.g. "FileWatcher" → ["file", "watcher"], "getHTTPResponse" → ["get", "http", "response"]
fn split_camel_case(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();

    for i in 0..chars.len() {
        let c = chars[i];
        if c == '_' || c == '-' {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
            continue;
        }
        if c.is_uppercase() && !current.is_empty() {
            // Check if this starts a new word (not a consecutive uppercase like "HTTP")
            let prev_lower = i > 0 && chars[i - 1].is_lowercase();
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if prev_lower || (next_lower && current.len() > 1) {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        }
        current.push(c);
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens
}

/// Score a symbol's relevance to a query string.
/// Returns None if no match, Some(1..=100) for match strength.
///
/// Accepts pre-computed `query_lower` to avoid repeated allocation
/// when scoring many symbols against the same query.
pub(crate) fn score_symbol(query: &str, symbol: &SymbolInfo) -> Option<i32> {
    score_symbol_with_lower(query, &query.to_lowercase(), symbol)
}

/// Inner scoring with pre-lowercased query — call this from hot loops.
pub(crate) fn score_symbol_with_lower(
    query: &str,
    query_lower: &str,
    symbol: &SymbolInfo,
) -> Option<i32> {
    // Exact full-query match (no allocation needed)
    if symbol.name.eq_ignore_ascii_case(query) {
        return Some(100);
    }

    // Compute name_lower once, reuse for substring + token checks
    let name_lower = symbol.name.to_lowercase();

    if name_lower.contains(query_lower) {
        return Some(60);
    }
    // Full query substring in signature
    let sig_lower = symbol.signature.to_lowercase();
    if sig_lower.contains(query_lower) {
        return Some(30);
    }
    // Full query substring in name_path
    if symbol.name_path.to_lowercase().contains(query_lower) {
        return Some(20);
    }
    let _is_multi_word = query_lower.contains(' ');

    // Check if query tokens form the symbol name when joined with underscore
    // e.g. "rename symbol" → "rename_symbol" → exact match bonus
    let joined_snake = query_lower.replace(|c: char| c.is_whitespace() || c == '-', "_");
    if name_lower == joined_snake {
        return Some(80);
    }
    // Partial: symbol name is a subset of joined tokens
    // e.g. "move symbol to file" → joined = "move_symbol_to_file", contains "move_symbol" → 70
    if joined_snake.contains(&name_lower) && name_lower.contains('_') {
        return Some(70);
    }
    // Reverse: symbol name contains the joined tokens
    // e.g. "extract function" → "refactor_extract_function" contains "extract_function" → 65
    if name_lower.contains(&joined_snake) && joined_snake.contains('_') {
        return Some(65);
    }

    // Token-level matching: split query into words, score by hit ratio
    let tokens: Vec<&str> = query_lower
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|t| t.len() >= 2)
        .collect();
    if tokens.is_empty() {
        return None;
    }

    // Split CamelCase name into tokens for matching (e.g. FileWatcher → ["file","watcher"])
    let name_camel_tokens = split_camel_case(&symbol.name);

    let path_lower = symbol.file_path.to_lowercase();

    let mut name_hits = 0i32;
    let mut sig_hits = 0i32;
    let mut path_hits = 0i32;
    for token in &tokens {
        if name_lower.contains(token) || name_camel_tokens.iter().any(|ct| ct == token) {
            name_hits += 1;
        }
        // Non-exclusive: check sig and path independently
        if sig_lower.contains(token) {
            sig_hits += 1;
        }
        if path_lower.contains(token) {
            path_hits += 1;
        }
    }

    let total_tokens = tokens.len() as i32;
    if name_hits == 0 && sig_hits == 0 && path_hits == 0 {
        return None;
    }

    // Score formula: name hits dominate, sig/path are secondary
    // name_ratio: 0.0-1.0 portion of query tokens found in name
    // Boost for high name coverage (most tokens match the symbol name)
    let name_ratio = name_hits as f64 / total_tokens as f64;
    let sig_ratio = sig_hits as f64 / total_tokens as f64;

    let base_score = if name_hits > 0 {
        let base = (15.0 + name_ratio * 40.0) as i32;
        let sig_bonus = (sig_ratio * 5.0) as i32;
        (base + sig_bonus).min(55)
    } else if sig_hits > 0 {
        (5.0 + sig_ratio * 20.0) as i32
    } else {
        // Path-only: very weak signal, 1-5
        let path_ratio = path_hits as f64 / total_tokens as f64;
        (1.0 + path_ratio * 4.0).max(1.0) as i32
    };

    // Kind-aware boost: action queries prefer functions, noun queries prefer types.
    // Detects action intent by checking if any query token is a common verb.
    let kind_boost = if query_has_action_verb(&tokens) {
        match symbol.kind {
            super::types::SymbolKind::Function | super::types::SymbolKind::Method => 8,
            _ => 0,
        }
    } else {
        match symbol.kind {
            super::types::SymbolKind::Class
            | super::types::SymbolKind::Interface
            | super::types::SymbolKind::Enum => 5,
            _ => 0,
        }
    };

    Some(base_score + kind_boost)
}
