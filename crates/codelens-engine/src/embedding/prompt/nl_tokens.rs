use super::auto_hint::nl_tokens_enabled;
use super::hint::join_hint_lines;

pub fn is_nl_shaped(s: &str) -> bool {
    let s = s.trim();
    if s.chars().count() < 4 {
        return false;
    }
    if s.contains('/') || s.contains('\\') || s.contains("::") {
        return false;
    }
    if !s.contains(' ') {
        return false;
    }
    let non_ws: usize = s.chars().filter(|c| !c.is_whitespace()).count();
    if non_ws == 0 {
        return false;
    }
    let alpha: usize = s.chars().filter(|c| c.is_alphabetic()).count();
    (alpha * 100) / non_ws >= 60
}

pub fn strict_comments_enabled() -> bool {
    std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS")
        .map(|raw| {
            let lowered = raw.to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

pub fn looks_like_meta_annotation(body: &str) -> bool {
    let trimmed = body.trim_start();
    let word_end = trimmed
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(trimmed.len());
    if word_end == 0 {
        return false;
    }
    let first_word = &trimmed[..word_end];
    let upper = first_word.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "TODO"
            | "FIXME"
            | "HACK"
            | "XXX"
            | "BUG"
            | "REVIEW"
            | "REFACTOR"
            | "TEMP"
            | "TEMPORARY"
            | "DEPRECATED"
    )
}

pub fn strict_literal_filter_enabled() -> bool {
    std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS")
        .map(|raw| {
            let lowered = raw.to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

pub fn contains_format_specifier(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 1 < len {
        if bytes[i] == b'%' {
            let next = bytes[i + 1];
            if matches!(next, b's' | b'd' | b'r' | b'f' | b'x' | b'o' | b'i' | b'u') {
                return true;
            }
        }
        i += 1;
    }
    for window in s.split('{').skip(1) {
        let Some(close_idx) = window.find('}') else {
            continue;
        };
        let inside = &window[..close_idx];
        if inside.is_empty() {
            return true;
        }
        if inside.chars().any(|c| c.is_whitespace()) {
            continue;
        }
        if inside.starts_with(':') {
            return true;
        }
        let ident_end = inside.find(':').unwrap_or(inside.len());
        let ident = &inside[..ident_end];
        if !ident.is_empty()
            && ident
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return true;
        }
    }
    false
}

pub fn looks_like_error_or_log_prefix(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    const PREFIXES: &[&str] = &[
        "invalid ",
        "cannot ",
        "could not ",
        "unable to ",
        "failed to ",
        "expected ",
        "unexpected ",
        "missing ",
        "not found",
        "error: ",
        "error ",
        "warning: ",
        "warning ",
        "sending ",
        "received ",
        "starting ",
        "stopping ",
        "calling ",
        "connecting ",
        "disconnecting ",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p))
}

#[cfg(test)]
pub fn should_reject_literal_strict(s: &str) -> bool {
    contains_format_specifier(s) || looks_like_error_or_log_prefix(s)
}

pub fn extract_nl_tokens(source: &str, start: usize, end: usize) -> Option<String> {
    if !nl_tokens_enabled() {
        return None;
    }
    extract_nl_tokens_inner(source, start, end)
}

pub fn extract_nl_tokens_inner(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
    let safe_start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let safe_end = end.min(source.len());
    let safe_end = if source.is_char_boundary(safe_end) {
        safe_end
    } else {
        source.floor_char_boundary(safe_end)
    };
    let body = &source[safe_start..safe_end];

    let mut tokens: Vec<String> = Vec::new();
    let strict_comments = strict_comments_enabled();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(cleaned) = extract_comment_body(trimmed)
            && is_nl_shaped(&cleaned)
            && (!strict_comments || !looks_like_meta_annotation(&cleaned))
        {
            tokens.push(cleaned);
        }
    }

    let strict_literals = strict_literal_filter_enabled();
    let mut chars = body.chars().peekable();
    let mut in_string = false;
    let mut current = String::new();
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                let _ = chars.next();
            } else if c == '"' {
                if is_nl_shaped(&current)
                    && (!strict_literals
                        || (!contains_format_specifier(&current)
                            && !looks_like_error_or_log_prefix(&current)))
                {
                    tokens.push(current.clone());
                }
                current.clear();
                in_string = false;
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_string = true;
        }
    }

    if tokens.is_empty() {
        return None;
    }
    Some(join_hint_lines(&tokens))
}

pub fn extract_comment_body(trimmed: &str) -> Option<String> {
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("///") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("//!") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return Some(rest.trim().to_string());
    }
    if trimmed.starts_with("#[") || trimmed.starts_with("#!") {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('#') {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("/**") {
        return Some(rest.trim_end_matches("*/").trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("/*") {
        return Some(rest.trim_end_matches("*/").trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix('*') {
        let rest = rest.trim_end_matches("*/").trim();
        if rest.is_empty() {
            return None;
        }
        if rest.contains(';') || rest.contains('{') {
            return None;
        }
        return Some(rest.to_string());
    }
    None
}
