use codelens_engine::SymbolInfo;
use serde_json::{Value, json};

/// Phase O1 — per-symbol presentation level.
///
/// Every symbol in a response picks one of these three levels based on
/// rank and whether the caller asked for bodies explicitly. The level
/// is emitted to the response as `presentation_level` so the harness
/// can tell "dropped on cap" from "intentionally thin" without re-
/// deriving the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SymbolPresentation {
    /// Name + kind + file + line only. No signature, no body. Used for
    /// symbols past the presentation cap when the caller wanted a
    /// broad search without paying per-symbol signature bytes.
    IdOnly,
    /// IdOnly + signature + name_path + id. The default for most
    /// symbols when `include_body` is false.
    Signature,
    /// Signature + body (truncated by line/char limit). Reserved for
    /// the top-N symbols when the caller explicitly requested bodies.
    SignatureBody,
}

impl SymbolPresentation {
    pub(super) fn as_label(self) -> &'static str {
        match self {
            SymbolPresentation::IdOnly => "id_only",
            SymbolPresentation::Signature => "signature",
            SymbolPresentation::SignatureBody => "signature_body",
        }
    }
}

/// Pick the presentation level for a symbol at the given rank.
///
/// * `explicit_body` — did the caller pass `include_body=true`?
/// * `rank` — 0-based position in the result list.
/// * `body_cap` — how many top symbols may promote to L2 when
///   `explicit_body` is true.
/// * `presentation_cap` — how many top symbols keep at least L1;
///   beyond this cap they drop to L0.
pub(super) fn select_presentation(
    explicit_body: bool,
    rank: usize,
    body_cap: usize,
    presentation_cap: usize,
) -> SymbolPresentation {
    if explicit_body && rank < body_cap {
        SymbolPresentation::SignatureBody
    } else if rank < presentation_cap {
        SymbolPresentation::Signature
    } else {
        SymbolPresentation::IdOnly
    }
}

/// Observability stats for a run of [`render_symbols_with_presentation`].
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct PresentationStats {
    pub(super) id_only: usize,
    pub(super) signature: usize,
    pub(super) signature_body_full: usize,
    pub(super) signature_body_truncated: usize,
}

/// Render a list of symbols as JSON with per-symbol presentation level
/// applied. Truncates bodies according to the line/char limits on L2
/// symbols, drops signature/body for L0 symbols, and always attaches a
/// `presentation_level` string field so downstream consumers can
/// detect the shape without inferring it.
pub(super) fn render_symbols_with_presentation(
    symbols: &[SymbolInfo],
    explicit_body: bool,
    body_cap: usize,
    presentation_cap: usize,
    max_body_lines: usize,
    max_body_chars: usize,
    body_full: bool,
) -> (Vec<Value>, PresentationStats) {
    let mut stats = PresentationStats::default();
    let rendered = symbols
        .iter()
        .enumerate()
        .map(|(idx, sym)| {
            let level = select_presentation(explicit_body, idx, body_cap, presentation_cap);
            let mut value = serde_json::to_value(sym).unwrap_or_else(|_| json!({}));
            if let Some(obj) = value.as_object_mut() {
                obj.insert("presentation_level".to_owned(), json!(level.as_label()));
                match level {
                    SymbolPresentation::IdOnly => {
                        obj.remove("signature");
                        obj.remove("body");
                        stats.id_only += 1;
                    }
                    SymbolPresentation::Signature => {
                        obj.remove("body");
                        stats.signature += 1;
                    }
                    SymbolPresentation::SignatureBody => {
                        if !body_full
                            && let Some(body) = obj
                                .get("body")
                                .and_then(|b| b.as_str())
                                .map(ToOwned::to_owned)
                        {
                            let (preview, truncated) =
                                truncate_body_preview(&body, max_body_lines, max_body_chars);
                            if truncated {
                                obj.insert("body".to_owned(), json!(preview));
                                stats.signature_body_truncated += 1;
                            } else {
                                stats.signature_body_full += 1;
                            }
                        } else {
                            stats.signature_body_full += 1;
                        }
                    }
                }
            }
            value
        })
        .collect();
    (rendered, stats)
}

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
