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

    let mut name_hits = 0i32;
    let mut sig_hits = 0i32;
    let mut path_hits = 0i32;
    for token in &tokens {
        if name_lower.contains(token) {
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

    let score = if name_hits > 0 {
        // Base: 15-55 depending on how many query tokens hit the name
        // name_ratio=1.0 → 55, name_ratio=0.25 → 21
        let base = (15.0 + name_ratio * 40.0) as i32;
        // Bonus for sig confirmation (tokens also in signature)
        let sig_bonus = (sig_ratio * 5.0) as i32;
        (base + sig_bonus).min(55)
    } else if sig_hits > 0 {
        // Signature-only matches: 5-25
        (5.0 + sig_ratio * 20.0) as i32
    } else {
        // Path-only: very weak signal, 1-5
        let path_ratio = path_hits as f64 / total_tokens as f64;
        (1.0 + path_ratio * 4.0).max(1.0) as i32
    };

    Some(score)
}
