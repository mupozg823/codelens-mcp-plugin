pub fn extract_leading_doc(source: &str, start: usize, end: usize) -> Option<String> {
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
    if safe_start >= safe_end {
        return None;
    }
    let body = &source[safe_start..safe_end];
    let lines: Vec<&str> = body.lines().skip(1).collect();
    if lines.is_empty() {
        return None;
    }

    let mut doc_lines = Vec::new();
    let first_trimmed = lines.first().map(|l| l.trim()).unwrap_or_default();
    if first_trimmed.starts_with("\"\"\"") || first_trimmed.starts_with("'''") {
        let quote = &first_trimmed[..3];
        for line in &lines {
            let t = line.trim();
            doc_lines.push(t.trim_start_matches(quote).trim_end_matches(quote));
            if doc_lines.len() > 1 && t.ends_with(quote) {
                break;
            }
        }
    } else if first_trimmed.starts_with("///") || first_trimmed.starts_with("//!") {
        for line in &lines {
            let t = line.trim();
            if t.starts_with("///") || t.starts_with("//!") {
                doc_lines.push(t.trim_start_matches("///").trim_start_matches("//!").trim());
            } else {
                break;
            }
        }
    } else if first_trimmed.starts_with("/**") {
        for line in &lines {
            let t = line.trim();
            let cleaned = t
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim();
            if !cleaned.is_empty() {
                doc_lines.push(cleaned);
            }
            if t.ends_with("*/") {
                break;
            }
        }
    } else {
        for line in &lines {
            let t = line.trim();
            if t.starts_with("//") || t.starts_with('#') {
                doc_lines.push(t.trim_start_matches("//").trim_start_matches('#').trim());
            } else {
                break;
            }
        }
    }

    if doc_lines.is_empty() {
        return None;
    }
    Some(doc_lines.join(" ").trim().to_owned())
}

/// Lines scanned upward from an item for its doc comment.
const MAX_PRECEDING_LINES: usize = 120;
/// Lines one multi-line attribute or decorator may span.
const MAX_ATTRIBUTE_LINES: usize = 8;

/// The doc comment written above an item: `///` lines (Rust, C#, Swift), a
/// `/** */` block (JS, TS, Java, Kotlin, PHP) or `//` lines (Go). Attribute,
/// annotation and decorator lines between the comment and the item are
/// skipped, a blank line ends the search, and only the text before the first
/// rustdoc heading, code fence or javadoc tag is kept.
///
/// `extract_leading_doc` reads inside the symbol (Python docstrings), so these
/// comments never reached the embedding text: on 2026-10-10 only 4.3% of
/// serde-json's Rust symbols carried a doc, and `to_string` ("Serialize the
/// given data structure as a String of JSON") was embedded without its own.
pub fn extract_preceding_doc(source: &str, start: usize) -> Option<String> {
    let start = start.min(source.len());
    let start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let before = &source[..start];
    let item_line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    let above: Vec<&str> = before[..item_line_start]
        .lines()
        .rev()
        .map(str::trim)
        .take(MAX_PRECEDING_LINES)
        .collect();

    let mut index = 0;
    while let Some(line) = above.get(index) {
        if is_attribute_start(line) {
            index += 1;
            continue;
        }
        // The last line of a multi-line attribute or decorator.
        if (line.ends_with(']') || line.ends_with(')'))
            && let Some(offset) = above[index..]
                .iter()
                .take(MAX_ATTRIBUTE_LINES)
                .position(|candidate| is_attribute_start(candidate))
        {
            index += offset + 1;
            continue;
        }
        break;
    }

    let first = *above.get(index)?;
    let mut doc: Vec<&str> = Vec::new();
    if is_triple_slash_doc(first) {
        while let Some(line) = above.get(index).filter(|line| is_triple_slash_doc(line)) {
            doc.push(line[3..].trim());
            index += 1;
        }
    } else if first.ends_with("*/") {
        let mut opened = false;
        for line in above[index..].iter().take(MAX_PRECEDING_LINES) {
            doc.push(
                line.trim_start_matches("/**")
                    .trim_end_matches("*/")
                    .trim_start_matches('*')
                    .trim(),
            );
            if line.starts_with("/**") {
                opened = true;
                break;
            }
            if line.starts_with("/*") {
                // A plain block comment, not a doc.
                break;
            }
        }
        if !opened {
            return None;
        }
    } else if is_line_comment(first) {
        while let Some(line) = above.get(index).filter(|line| is_line_comment(line)) {
            doc.push(line[2..].trim());
            index += 1;
        }
    }
    doc.reverse();

    let summary: Vec<&str> = doc
        .into_iter()
        .take_while(|line| {
            !(line.starts_with('#') || line.starts_with("```") || line.starts_with('@'))
        })
        .filter(|line| !line.is_empty())
        .collect();
    if summary.is_empty() {
        None
    } else {
        Some(summary.join("\n"))
    }
}

fn is_attribute_start(line: &str) -> bool {
    line.starts_with("#[") || line.starts_with('@')
}

fn is_triple_slash_doc(line: &str) -> bool {
    line.starts_with("///") && !line.starts_with("////")
}

/// A `//` comment that is neither a doc (`///`) nor a module doc (`//!`).
fn is_line_comment(line: &str) -> bool {
    line.starts_with("//") && !line.starts_with("///") && !line.starts_with("//!")
}

/// A symbol's doc: the comment above it, else a docstring or comment inside
/// it (Python).
pub fn extract_symbol_doc(source: &str, start: usize, end: usize) -> Option<String> {
    extract_preceding_doc(source, start).or_else(|| extract_leading_doc(source, start, end))
}
