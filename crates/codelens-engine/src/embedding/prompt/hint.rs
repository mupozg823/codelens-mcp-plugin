const DEFAULT_HINT_TOTAL_CHAR_BUDGET: usize = 60;
const DEFAULT_HINT_LINES: usize = 1;

pub fn hint_char_budget() -> usize {
    std::env::var("CODELENS_EMBED_HINT_CHARS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .map(|n| n.clamp(60, 512))
        .unwrap_or(DEFAULT_HINT_TOTAL_CHAR_BUDGET)
}

pub fn hint_line_budget() -> usize {
    std::env::var("CODELENS_EMBED_HINT_LINES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .map(|n| n.clamp(1, 10))
        .unwrap_or(DEFAULT_HINT_LINES)
}

pub fn join_hint_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let joined = lines
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" · ");
    let budget = hint_char_budget();
    if joined.chars().count() > budget {
        let truncated: String = joined.chars().take(budget).collect();
        format!("{truncated}...")
    } else {
        joined
    }
}

pub fn extract_body_hint(source: &str, start: usize, end: usize) -> Option<String> {
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

    let max_lines = hint_line_budget();
    let mut collected: Vec<String> = Vec::with_capacity(max_lines);

    let mut past_signature = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if !past_signature {
            if trimmed.ends_with('{') || trimmed.ends_with(':') || trimmed == "{" {
                past_signature = true;
            }
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed == "}"
        {
            continue;
        }
        collected.push(trimmed.to_string());
        if collected.len() >= max_lines {
            break;
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(join_hint_lines(&collected))
    }
}
