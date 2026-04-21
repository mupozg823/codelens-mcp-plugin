//! MCP handler for `find_relevant_rules` — exposes the rule corpus
//! BM25 retrieval surface built in `rule_retrieval`.
//!
//! Kept as a separate leaf module so the dispatch-table entry stays
//! one line and the handler implementation never leaks into
//! `tools/mod.rs`.

use super::{AppState, ToolResult, required_string};
use crate::protocol::BackendKind;
use crate::tool_runtime::success_meta;
use serde_json::{Value, json};

const DEFAULT_TOP_K: usize = 3;
const MAX_TOP_K: usize = 20;
const PREVIEW_CHAR_CAP: usize = 400;

pub fn find_relevant_rules(state: &AppState, arguments: &Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let requested_k = arguments
        .get("top_k")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TOP_K as u64) as usize;
    let top_k = requested_k.clamp(1, MAX_TOP_K);

    let project = state.project();
    let corpus = crate::retrieval::rules::load_rule_corpus(project.as_path());
    let scored = crate::retrieval::rules::find_relevant_rules(&corpus, query, top_k);

    let rules: Vec<Value> = scored
        .iter()
        .map(|s| {
            json!({
                "source_file": s.snippet.source_file.to_string_lossy(),
                "source_kind": s.snippet.source_kind.as_label(),
                "frontmatter_name": s.snippet.frontmatter_name,
                "section_title": s.snippet.section_title,
                "content_preview": truncate_preview(&s.snippet.content, PREVIEW_CHAR_CAP),
                "line_start": s.snippet.line_start,
                "score": round_to_4dp(s.score),
            })
        })
        .collect();

    Ok((
        json!({
            "query": query,
            "corpus_size": corpus.len(),
            "top_k": top_k,
            "rules": rules,
            "count": rules.len(),
        }),
        success_meta(BackendKind::Filesystem, 0.90),
    ))
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

fn round_to_4dp(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncates_long_text() {
        let long = "a".repeat(500);
        let preview = truncate_preview(&long, 100);
        assert_eq!(preview.chars().count(), 101); // 100 chars + ellipsis
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn preview_keeps_short_text_verbatim() {
        let text = "short content";
        assert_eq!(truncate_preview(text, 100), "short content");
    }

    #[test]
    fn round_to_4dp_trims_float_tail() {
        assert_eq!(round_to_4dp(0.123456789), 0.1235);
        assert_eq!(round_to_4dp(1.0), 1.0);
    }
}
