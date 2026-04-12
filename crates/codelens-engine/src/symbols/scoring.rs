use super::types::SymbolInfo;

// ── Zero-allocation ASCII case-insensitive helpers ──────────────────

/// ASCII case-insensitive substring search. Returns true if `needle`
/// appears anywhere in `haystack` ignoring ASCII case differences.
///
/// This replaces the previous pattern of allocating
/// `haystack.to_lowercase()` + `haystack_lower.contains(needle_lower)`
/// which paid one `String` allocation per call. Since code identifiers
/// in all 25 supported tree-sitter languages are ASCII, the ASCII-only
/// comparison is both correct and faster than Unicode `to_lowercase`.
fn contains_ascii_ci(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return false;
    }
    if n.is_empty() {
        return true;
    }
    h.windows(n.len())
        .any(|window| window.eq_ignore_ascii_case(n))
}

/// ASCII case-insensitive full-string equality.
fn eq_ascii_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

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
    let lower = query.to_lowercase();
    let snake = lower.replace(|c: char| c.is_whitespace() || c == '-', "_");
    score_symbol_with_lower(query, &lower, &snake, symbol)
}

/// Inner scoring with pre-lowercased query and pre-computed joined-snake
/// form — call this from hot loops where both are invariant across
/// candidates.
///
/// `joined_snake` is the query with whitespace/hyphens replaced by
/// underscores, used for snake_case identifier matching (e.g.
/// "rename symbol" → "rename_symbol"). It is query-derived and
/// identical for every candidate, so computing it once in the caller
/// eliminates one String allocation per candidate in the hot loop.
pub(crate) fn score_symbol_with_lower(
    query: &str,
    query_lower: &str,
    joined_snake: &str,
    symbol: &SymbolInfo,
) -> Option<i32> {
    // Exact full-query match (no allocation needed)
    if symbol.name.eq_ignore_ascii_case(query) {
        return Some(100);
    }

    // ── Zero-alloc substring checks (replaces 4 × to_lowercase()) ──
    // All checks below use contains_ascii_ci / eq_ascii_ci instead of
    // allocating lowered Strings. Code identifiers are ASCII, so
    // ASCII case folding is correct and avoids one String per field.

    if contains_ascii_ci(&symbol.name, query_lower) {
        return Some(60);
    }
    if contains_ascii_ci(&symbol.signature, query_lower) {
        return Some(30);
    }
    if contains_ascii_ci(&symbol.name_path, query_lower) {
        return Some(20);
    }

    // Check if query tokens form the symbol name when joined with underscore
    // e.g. "rename symbol" → "rename_symbol" → exact match bonus
    // `joined_snake` is pre-computed by the caller to avoid one String
    // allocation per candidate in the hot loop.
    if eq_ascii_ci(&symbol.name, joined_snake) {
        return Some(80);
    }
    // Partial: symbol name is a subset of joined tokens
    // e.g. "move symbol to file" → joined = "move_symbol_to_file", contains "move_symbol" → 70
    if contains_ascii_ci(joined_snake, &symbol.name) && symbol.name.contains('_') {
        return Some(70);
    }
    // Reverse: symbol name contains the joined tokens
    // e.g. "extract function" → "refactor_extract_function" contains "extract_function" → 65
    if contains_ascii_ci(&symbol.name, joined_snake) && joined_snake.contains('_') {
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

    let mut name_hits = 0i32;
    let mut sig_hits = 0i32;
    let mut path_hits = 0i32;
    for token in &tokens {
        if contains_ascii_ci(&symbol.name, token) || name_camel_tokens.iter().any(|ct| ct == token)
        {
            name_hits += 1;
        }
        if contains_ascii_ci(&symbol.signature, token) {
            sig_hits += 1;
        }
        if contains_ascii_ci(&symbol.file_path, token) {
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

/// Return true when v1.5 Phase 2e sparse term weighting is enabled via
/// `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` (or `true`/`yes`/`on`).
///
/// Default OFF, mirroring the Phase 2b/2c opt-in policy. Projects that
/// already opt into the Phase 2b/2c embedding hints can stack this knob
/// to tighten top-1 ordering without another index rebuild — the sparse
/// pass reads `SymbolInfo` fields that are already populated on the
/// ranking path.
///
/// v1.5 Phase 2j: when no explicit env var is set, fall through to
/// `crate::embedding::auto_sparse_should_enable()` for language-gated
/// defaults. This intentionally diverges from `nl_tokens_enabled` and
/// `api_calls_enabled`: Phase 2m keeps JS/TS auto-enabled for Phase 2b/2c
/// but auto-disables sparse weighting there because recent JS/TS
/// measurements were negative-or-inert. Explicit env always wins.
pub fn sparse_weighting_enabled() -> bool {
    if let Ok(raw) = std::env::var("CODELENS_RANK_SPARSE_TERM_WEIGHT") {
        let lowered = raw.trim().to_ascii_lowercase();
        return matches!(lowered.as_str(), "1" | "true" | "yes" | "on");
    }
    crate::embedding::auto_sparse_should_enable()
}

/// Maximum sparse coverage bonus added to the blended score when a query
/// reaches 100% term coverage against a symbol's `name + name_path +
/// signature` corpus. Override via `CODELENS_RANK_SPARSE_MAX` (clamped
/// to 5..=50).
///
/// Kept deliberately modest (default 20) because the existing lexical
/// score in `score_symbol_with_lower` already reaches 55 for signature
/// hits. The sparse bonus is a *tie-breaker* — it re-orders the top-K
/// after the main scoring has selected them, not a replacement for the
/// lexical signal.
pub fn sparse_max_bonus() -> f64 {
    std::env::var("CODELENS_RANK_SPARSE_MAX")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .map(|n| n.clamp(5, 50))
        .unwrap_or(20) as f64
}

/// Minimum query-term coverage (as a percentage, 10..=90) a symbol must
/// reach before it receives any sparse bonus. Below this threshold the
/// bonus is `0.0`. Between the threshold and 100% the bonus rises
/// linearly from `0.0` to `sparse_max_bonus()`.
///
/// The default of 60 was a conservative first guess. An initial 4-arm
/// A/B on the 89-query self dataset found that the bonus never fired at
/// 60 because most NL queries only share 1–2 discriminative tokens with
/// their target symbol's `name + name_path + signature` corpus.
/// Override via `CODELENS_RANK_SPARSE_THRESHOLD` for tuning experiments.
pub fn sparse_threshold() -> f64 {
    std::env::var("CODELENS_RANK_SPARSE_THRESHOLD")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .map(|n| n.clamp(10, 90))
        .unwrap_or(60) as f64
        / 100.0
}

/// English/pseudo-stopwords that add no discriminative signal when used
/// as query tokens. Intentionally short — real NL stopwords lists contain
/// ~150 entries, but most of them never show up in code-search queries.
/// We only need the ones that regularly dilute query coverage ("find the
/// function that opens a file" — `the` and `that` are the problem).
const SPARSE_STOPWORDS: &[&str] = &[
    "the", "for", "with", "from", "that", "this", "into", "onto", "over", "not", "and", "any",
    "all", "are", "was", "were", "has", "have", "had", "how", "what", "when", "where", "which",
    "who", "why", "but", "its", "can", "use", "using", "used", "gets", "set", "sets", "new", "let",
];

/// Return true when `token` is found in `corpus` as a whole word — that is,
/// the characters surrounding each occurrence are NOT alphanumeric or `_`.
///
/// Phase 2e uses this instead of `str::contains` so that a query token like
/// `"parse"` matches `parse_json` (snake separator) but not `parser` or
/// `parseRequest` (would already be caught by the lexical `contains` path,
/// which is where we want them scored — not via the sparse bonus).
pub fn has_whole_word(corpus: &str, token: &str) -> bool {
    if token.is_empty() || corpus.len() < token.len() {
        return false;
    }
    let corpus_bytes = corpus.as_bytes();
    let token_bytes = token.as_bytes();
    let mut start = 0;
    while start + token_bytes.len() <= corpus_bytes.len() {
        // Find next occurrence from `start`
        let remaining = &corpus[start..];
        let Some(local_idx) = remaining.find(token) else {
            return false;
        };
        let abs = start + local_idx;
        let end = abs + token_bytes.len();
        let before_ok = abs == 0 || !is_word_byte(corpus_bytes[abs - 1]);
        let after_ok = end == corpus_bytes.len() || !is_word_byte(corpus_bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        start = abs + 1;
    }
    false
}

/// Byte-level helper: true when the byte is part of an ASCII word
/// ([A-Za-z0-9]). `_` is deliberately excluded so that snake_case
/// separators count as word boundaries — e.g. `"parse"` should match
/// `"parse_json_body"` but not `"parser"`. Non-ASCII bytes (UTF-8
/// continuation) default to "word" so multi-byte identifiers stay
/// conservative (no false positives from partial UTF-8 matches).
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || (b & 0x80) != 0
}

/// Tokenize `query_lower` into distinct discriminative terms for the
/// Phase 2e sparse pass:
/// - split on any non-alphanumeric character
/// - drop tokens shorter than 3 characters
/// - drop tokens in `SPARSE_STOPWORDS`
/// - deduplicate while preserving order
///
/// Returns `Vec<String>` (not `Vec<&str>`) so callers can own the tokens
/// independently of the query lifetime — the rank loop already has to
/// outlive the borrow anyway.
pub fn sparse_query_tokens(query_lower: &str) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in query_lower.split(|c: char| !c.is_alphanumeric()) {
        if raw.len() < 3 {
            continue;
        }
        if SPARSE_STOPWORDS.contains(&raw) {
            continue;
        }
        if seen.insert(raw.to_string()) {
            out.push(raw.to_string());
        }
    }
    out
}

/// Text-first variant of the Phase 2e sparse coverage bonus. Does NOT
/// take a `SymbolInfo` so that callers outside the engine crate (notably
/// the MCP `get_ranked_context` post-process) can feed it whatever fields
/// are actually available on their entry type.
///
/// `query_lower` MUST already be lower-cased — the function does not
/// re-lowercase so that callers with a long query can amortise the
/// allocation outside the loop. Pass the *original user query*, not the
/// MCP-expanded retrieval string: the expansion adds dozens of
/// derivative tokens (snake_case, CamelCase, alias groups) that dilute
/// the coverage ratio below any reasonable threshold — that dilution
/// was the exact reason the first 4-arm pilot measured zero effect.
///
/// Returns `0.0` whenever:
/// - the query has fewer than 2 discriminative tokens after stopword
///   filtering (single-token queries already resolve well via the
///   lexical path — `sparse_query_tokens` deduplicates + drops <3 chars),
/// - the coverage ratio is below `sparse_threshold()` (default 0.6).
///
/// Between the threshold and 100% coverage the bonus rises linearly
/// from 0 to `sparse_max_bonus()`. The caller is responsible for
/// gating the whole call with `sparse_weighting_enabled()` so test
/// code can run the inner logic deterministically.
pub fn sparse_coverage_bonus_from_fields(
    query_lower: &str,
    name: &str,
    name_path: &str,
    signature: &str,
    file_path: &str,
) -> f64 {
    let tokens = sparse_query_tokens(query_lower);
    if tokens.len() < 2 {
        return 0.0;
    }
    let mut corpus =
        String::with_capacity(name.len() + name_path.len() + signature.len() + file_path.len() + 3);
    corpus.push_str(name);
    corpus.push(' ');
    corpus.push_str(name_path);
    corpus.push(' ');
    corpus.push_str(signature);
    corpus.push(' ');
    corpus.push_str(file_path);
    let corpus_lower = corpus.to_lowercase();

    let matched = tokens
        .iter()
        .filter(|t| has_whole_word(&corpus_lower, t))
        .count() as f64;
    let total = tokens.len() as f64;
    let coverage = matched / total;

    let threshold = sparse_threshold();
    if coverage < threshold {
        return 0.0;
    }
    // threshold → 0, 100% → sparse_max_bonus(), linear between. Guard
    // against threshold == 1.0 (would divide by zero) by clamping.
    let span = (1.0 - threshold).max(0.01);
    (coverage - threshold) / span * sparse_max_bonus()
}

/// Back-compat wrapper kept for the existing `SymbolInfo`-based unit
/// tests. New call sites should prefer `sparse_coverage_bonus_from_fields`.
#[cfg(test)]
pub(crate) fn sparse_coverage_bonus(query_lower: &str, symbol: &SymbolInfo) -> f64 {
    sparse_coverage_bonus_from_fields(
        query_lower,
        &symbol.name,
        &symbol.name_path,
        &symbol.signature,
        &symbol.file_path,
    )
}

#[cfg(test)]
mod tests {
    use super::super::types::{SymbolInfo, SymbolKind};
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn mk_symbol(name: &str, signature: &str) -> SymbolInfo {
        SymbolInfo {
            name: name.to_string(),
            kind: SymbolKind::Function,
            file_path: "test.rs".into(),
            line: 1,
            column: 0,
            signature: signature.to_string(),
            name_path: name.to_string(),
            id: format!("test.rs#function:{name}"),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
        }
    }

    #[test]
    fn sparse_weighting_gated_off_by_default() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_explicit = std::env::var("CODELENS_RANK_SPARSE_TERM_WEIGHT").ok();
        let previous_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
        let previous_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();
        unsafe {
            std::env::remove_var("CODELENS_RANK_SPARSE_TERM_WEIGHT");
            std::env::remove_var("CODELENS_EMBED_HINT_AUTO");
            std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG");
        }
        let enabled = sparse_weighting_enabled();
        unsafe {
            match previous_explicit {
                Some(value) => std::env::set_var("CODELENS_RANK_SPARSE_TERM_WEIGHT", value),
                None => std::env::remove_var("CODELENS_RANK_SPARSE_TERM_WEIGHT"),
            }
            match previous_auto {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
            }
            match previous_lang {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
            }
        }
        assert!(!enabled, "sparse weighting gate leaked");
    }

    #[test]
    fn sparse_weighting_auto_gate_disables_for_js_ts_but_explicit_env_still_wins() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_explicit = std::env::var("CODELENS_RANK_SPARSE_TERM_WEIGHT").ok();
        let previous_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
        let previous_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();

        unsafe {
            std::env::remove_var("CODELENS_RANK_SPARSE_TERM_WEIGHT");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            sparse_weighting_enabled(),
            "auto+rust should enable sparse weighting"
        );

        unsafe {
            std::env::remove_var("CODELENS_RANK_SPARSE_TERM_WEIGHT");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "typescript");
        }
        assert!(
            !sparse_weighting_enabled(),
            "auto+typescript should disable sparse weighting after Phase 2m split"
        );

        unsafe {
            std::env::set_var("CODELENS_RANK_SPARSE_TERM_WEIGHT", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "typescript");
        }
        assert!(
            sparse_weighting_enabled(),
            "explicit sparse=1 must still win over JS/TS auto-off"
        );

        unsafe {
            std::env::set_var("CODELENS_RANK_SPARSE_TERM_WEIGHT", "0");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            !sparse_weighting_enabled(),
            "explicit sparse=0 must still win over rust auto-on"
        );

        unsafe {
            match previous_explicit {
                Some(value) => std::env::set_var("CODELENS_RANK_SPARSE_TERM_WEIGHT", value),
                None => std::env::remove_var("CODELENS_RANK_SPARSE_TERM_WEIGHT"),
            }
            match previous_auto {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
            }
            match previous_lang {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
            }
        }
    }

    #[test]
    fn sparse_query_tokens_drops_stopwords_and_short_tokens() {
        let tokens = sparse_query_tokens("find the function that opens a file");
        // "find", "function", "opens", "file" survive. "the", "that", "a" dropped.
        assert_eq!(tokens, vec!["find", "function", "opens", "file"]);
    }

    #[test]
    fn sparse_query_tokens_deduplicates() {
        let tokens = sparse_query_tokens("parse json parse xml parse");
        assert_eq!(tokens, vec!["parse", "json", "xml"]);
    }

    #[test]
    fn has_whole_word_respects_word_boundaries() {
        // snake_case separator counts as non-word → match
        assert!(has_whole_word("parse_json_body", "parse"));
        // substring inside a larger identifier → no match
        assert!(!has_whole_word("parser", "parse"));
        assert!(!has_whole_word("parserequest", "parse"));
        // leading/trailing whitespace
        assert!(has_whole_word("parse the file", "parse"));
        assert!(has_whole_word("open file", "file"));
        // empty token / short corpus
        assert!(!has_whole_word("xyz", ""));
        assert!(!has_whole_word("ab", "abc"));
    }

    #[test]
    fn sparse_coverage_bonus_zero_for_single_token_query() {
        let sym = mk_symbol("parse_json", "fn parse_json(input: &str) -> Value");
        // Single token after stopword filtering — short-circuit to 0.
        let bonus = sparse_coverage_bonus("parse", &sym);
        assert_eq!(bonus, 0.0);
    }

    #[test]
    fn sparse_coverage_bonus_zero_below_threshold() {
        let sym = mk_symbol("parse_json", "fn parse_json(input: &str) -> Value");
        // Two query tokens: "parse", "rename". Only "parse" matches → 50% coverage.
        // 50% < 60% threshold → bonus 0.
        let bonus = sparse_coverage_bonus("parse rename", &sym);
        assert_eq!(bonus, 0.0);
    }

    #[test]
    fn sparse_coverage_bonus_full_match_reaches_max() {
        let sym = mk_symbol(
            "parse_json_body",
            "fn parse_json_body(input: &str) -> Value",
        );
        // Tokens: "parse", "json", "body". All three match.
        // coverage = 1.0 → bonus = (1.0 - 0.6) / 0.4 * 20 = 20
        let bonus = sparse_coverage_bonus("parse json body", &sym);
        // Allow small float tolerance for default max = 20
        assert!((bonus - 20.0).abs() < 0.01, "expected ~20, got {bonus}");
    }

    #[test]
    fn sparse_coverage_bonus_ignores_whole_word_false_positives() {
        // "parser" should NOT match token "parse" via the sparse path —
        // word-boundary precision is the whole point of Phase 2e.
        // Two tokens ("parse", "json"), only "json" matches via the
        // signature → 50% coverage → 0 bonus (below threshold).
        let sym = mk_symbol("parser", "fn parser(input: &str) -> Json");
        let bonus = sparse_coverage_bonus("parse json", &sym);
        assert_eq!(bonus, 0.0);
    }
}
