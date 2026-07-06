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
