use super::types::SymbolInfo;

/// Score a symbol's relevance to a query string.
/// Returns None if no match, Some(1..=100) for match strength.
pub(crate) fn score_symbol(query: &str, symbol: &SymbolInfo) -> Option<i32> {
    let query_lower = query.to_lowercase();

    // Exact full-query match on symbol name
    if symbol.name.eq_ignore_ascii_case(query) {
        return Some(100);
    }
    // Full query substring in symbol name
    if symbol.name.to_lowercase().contains(&query_lower) {
        return Some(60);
    }
    // Full query substring in signature
    if symbol.signature.to_lowercase().contains(&query_lower) {
        return Some(30);
    }
    // Full query substring in name_path
    if symbol.name_path.to_lowercase().contains(&query_lower) {
        return Some(20);
    }

    // Token-level matching: split query into words, score by hit ratio
    let tokens: Vec<&str> = query_lower
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|t| t.len() >= 2)
        .collect();
    if tokens.is_empty() {
        return None;
    }

    let name_lower = symbol.name.to_lowercase();
    let sig_lower = symbol.signature.to_lowercase();
    let path_lower = symbol.file_path.to_lowercase();

    let mut hits = 0i32;
    for token in &tokens {
        if name_lower.contains(token) {
            hits += 3; // name hit = strong signal
        } else if sig_lower.contains(token) {
            hits += 2; // signature hit
        } else if path_lower.contains(token) {
            hits += 1; // file path hit = weak signal
        }
    }

    if hits > 0 {
        // Scale to 1-50 range based on hit ratio
        let max_possible = tokens.len() as i32 * 3;
        let score = (hits * 50 / max_possible).max(1);
        Some(score)
    } else {
        None
    }
}
