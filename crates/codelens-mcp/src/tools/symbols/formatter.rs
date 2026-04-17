use codelens_engine::SymbolInfo;

pub(super) fn truncate_body_preview(
    body: &str,
    max_lines: usize,
    max_chars: usize,
) -> (String, bool) {
    let mut truncated = false;
    let lines = body.lines().take(max_lines).collect::<Vec<_>>();
    if body.lines().count() > max_lines {
        truncated = true;
    }
    let mut preview = lines.join("\n");
    if preview.len() > max_chars {
        let mut boundary = max_chars.min(preview.len());
        while boundary > 0 && !preview.is_char_boundary(boundary) {
            boundary -= 1;
        }
        preview.truncate(boundary);
        truncated = true;
    }
    if truncated {
        preview.push_str("\n... [truncated; rerun with body_full=true for the full body]");
    }
    (preview, truncated)
}

pub(super) fn compact_symbol_bodies(
    symbols: &mut [SymbolInfo],
    max_symbols_with_body: usize,
    max_body_lines: usize,
    max_body_chars: usize,
) -> usize {
    let mut truncated_count = 0;
    for (idx, symbol) in symbols.iter_mut().enumerate() {
        if let Some(body) = symbol.body.as_ref() {
            if idx >= max_symbols_with_body {
                symbol.body = None;
                truncated_count += 1;
                continue;
            }
            let (preview, truncated) = truncate_body_preview(body, max_body_lines, max_body_chars);
            if truncated {
                symbol.body = Some(preview);
                truncated_count += 1;
            }
        }
    }
    truncated_count
}

pub(super) fn count_branches(lines: &[&str]) -> i32 {
    lines.iter().map(|line| count_branches_in_line(line)).sum()
}

fn count_branches_in_line(line: &str) -> i32 {
    let mut count = 0i32;
    // "if" already counts the branch in "else if", so no separate else-if handling needed.
    for token in [
        "if", "elif", "for", "while", "catch", "except", "case", "and", "or",
    ] {
        count += count_word_occurrences(line, token);
    }
    count += line.match_indices("&&").count() as i32;
    count += line.match_indices("||").count() as i32;
    count
}

fn count_word_occurrences(line: &str, needle: &str) -> i32 {
    line.match_indices(needle)
        .filter(|(index, _)| {
            let start_ok = *index == 0
                || !line[..*index]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let end = index + needle.len();
            let end_ok = end == line.len()
                || !line[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            start_ok && end_ok
        })
        .count() as i32
}
