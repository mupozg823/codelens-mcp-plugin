//! Lexical retrieval over the rule corpus (P2.1b).
//!
//! Self-contained BM25 implementation — we do NOT reuse the engine's
//! SQLite FTS5 pipeline because the rule corpus is small, static-per-
//! session, and markdown-shaped (`frontmatter_name` + `section_title`
//! + body), which does not fit the `SymbolRow` schema the FTS5 index
//! is tied to.
//!
//! Parameters use the classic BM25 defaults: `k1 = 1.2`, `b = 0.75`.
//! No stemming, no stopword filtering — rule text is short enough that
//! missing these adds more risk of noise than gain. Tokenization is
//! lowercase + split on non-alphanumeric.
//!
//! Cost model: fully on-the-fly, rebuilt each call. A typical rule
//! corpus is < 200 chunks at < 2 kB each → score pass stays under 5 ms
//! even for the wide queries `analyze_change_request` will feed in.

use crate::rule_corpus::RuleSnippet;
use std::collections::HashMap;

/// One rule snippet paired with its BM25 score against the query.
#[derive(Debug, Clone)]
pub struct ScoredSnippet {
    pub snippet: RuleSnippet,
    pub score: f64,
}

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
const MIN_TOKEN_LEN: usize = 2;

/// Score every snippet against `query` via BM25 and return the top
/// `top_k` matches sorted by descending score. Snippets with zero
/// score are dropped so an empty / irrelevant query yields an empty
/// vector rather than arbitrary top rows.
pub fn find_relevant_rules(
    corpus: &[RuleSnippet],
    query: &str,
    top_k: usize,
) -> Vec<ScoredSnippet> {
    if corpus.is_empty() || top_k == 0 {
        return Vec::new();
    }
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let doc_tokens: Vec<Vec<String>> = corpus
        .iter()
        .map(|snippet| tokenize(&searchable_text(snippet)))
        .collect();
    let doc_lengths: Vec<usize> = doc_tokens.iter().map(|tokens| tokens.len()).collect();
    let total_length: usize = doc_lengths.iter().sum();
    let n_docs = corpus.len() as f64;
    let avgdl = if total_length == 0 {
        1.0
    } else {
        total_length as f64 / n_docs
    };

    let mut df: HashMap<&str, usize> = HashMap::new();
    for qt in &query_tokens {
        if df.contains_key(qt.as_str()) {
            continue;
        }
        let count = doc_tokens
            .iter()
            .filter(|tokens| tokens.iter().any(|t| t == qt))
            .count();
        df.insert(qt.as_str(), count);
    }

    let mut scored: Vec<ScoredSnippet> = corpus
        .iter()
        .enumerate()
        .map(|(idx, snippet)| {
            let dl = doc_lengths[idx] as f64;
            let mut score = 0.0_f64;
            for qt in &query_tokens {
                let tf = doc_tokens[idx].iter().filter(|t| *t == qt).count() as f64;
                if tf == 0.0 {
                    continue;
                }
                let docs_with_term = *df.get(qt.as_str()).unwrap_or(&0) as f64;
                // Robertson-Sparck Jones IDF with +1 smoothing — never
                // goes negative even when a term hits every doc.
                let idf = ((n_docs - docs_with_term + 0.5) / (docs_with_term + 0.5) + 1.0).ln();
                let tf_norm =
                    tf * (BM25_K1 + 1.0) / (tf + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avgdl));
                score += idf * tf_norm;
            }
            ScoredSnippet {
                snippet: snippet.clone(),
                score,
            }
        })
        .filter(|s| s.score > 0.0)
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(top_k);
    scored
}

/// Concatenate all searchable fields of a snippet. `frontmatter_name`
/// and `section_title` are counted once each so a title hit contributes
/// roughly one extra occurrence to tf, giving titles a mild boost
/// without a separate field-weight multiplier.
fn searchable_text(snippet: &RuleSnippet) -> String {
    let mut out = String::with_capacity(snippet.content.len() + 128);
    if let Some(name) = snippet.frontmatter_name.as_deref() {
        out.push_str(name);
        out.push('\n');
    }
    if let Some(title) = snippet.section_title.as_deref() {
        out.push_str(title);
        out.push('\n');
    }
    out.push_str(&snippet.content);
    out
}

/// Lowercase-and-split tokenizer. Non-alphanumeric characters split
/// tokens; underscores are preserved so `find_symbol` and `replace_
/// symbol_body` stay as compound tokens useful for identifier-style
/// queries. Tokens shorter than `MIN_TOKEN_LEN` are dropped.
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() >= MIN_TOKEN_LEN {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= MIN_TOKEN_LEN {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_corpus::RuleSource;
    use std::path::PathBuf;

    fn snippet(
        kind: RuleSource,
        name: Option<&str>,
        title: Option<&str>,
        content: &str,
    ) -> RuleSnippet {
        RuleSnippet {
            source_file: PathBuf::from("/tmp/fake.md"),
            source_kind: kind,
            frontmatter_name: name.map(str::to_owned),
            section_title: title.map(str::to_owned),
            content: content.to_owned(),
            line_start: 1,
        }
    }

    #[test]
    fn empty_corpus_yields_empty() {
        let results = find_relevant_rules(&[], "anything", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn empty_query_yields_empty() {
        let corpus = vec![snippet(RuleSource::Global, None, None, "alpha beta gamma")];
        let results = find_relevant_rules(&corpus, "", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn exact_match_outranks_noise() {
        let corpus = vec![
            snippet(
                RuleSource::Global,
                None,
                None,
                "random text about unrelated matters",
            ),
            snippet(
                RuleSource::Memory,
                None,
                Some("Testing Guidance"),
                "always run cargo test before committing, testing matters",
            ),
            snippet(RuleSource::Memory, None, None, "another unrelated snippet"),
        ];
        let results = find_relevant_rules(&corpus, "cargo test", 3);
        assert!(!results.is_empty(), "expected at least one match");
        assert_eq!(
            results[0].snippet.section_title.as_deref(),
            Some("Testing Guidance")
        );
        assert!(results[0].score > 0.0);
    }

    #[test]
    fn top_k_limit_respected() {
        let corpus: Vec<_> = (0..10)
            .map(|i| {
                snippet(
                    RuleSource::Memory,
                    None,
                    None,
                    &format!("alpha entry {i} with some alpha repeats alpha"),
                )
            })
            .collect();
        let results = find_relevant_rules(&corpus, "alpha", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn frontmatter_name_is_searchable() {
        let corpus = vec![
            snippet(
                RuleSource::Memory,
                Some("self dogfooding codelens routing"),
                None,
                "unrelated body with no matching words at all",
            ),
            snippet(
                RuleSource::Memory,
                Some("unrelated memory"),
                None,
                "no match here either",
            ),
        ];
        let results = find_relevant_rules(&corpus, "dogfooding", 2);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].snippet.frontmatter_name.as_deref(),
            Some("self dogfooding codelens routing")
        );
    }

    #[test]
    fn zero_score_snippets_dropped() {
        let corpus = vec![snippet(
            RuleSource::Global,
            None,
            None,
            "totally unrelated content",
        )];
        let results = find_relevant_rules(&corpus, "cargo test framework", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn score_ordering_is_descending() {
        let corpus = vec![
            snippet(
                RuleSource::Global,
                None,
                None,
                "rust rust rust rust embedding retrieval",
            ),
            snippet(RuleSource::Memory, None, None, "rust once only"),
            snippet(RuleSource::Memory, None, None, "rust rust twice"),
        ];
        let results = find_relevant_rules(&corpus, "rust", 3);
        assert_eq!(results.len(), 3);
        assert!(results[0].score >= results[1].score);
        assert!(results[1].score >= results[2].score);
    }

    #[test]
    fn tokenize_splits_on_punctuation_and_preserves_underscores() {
        let tokens = tokenize("find_symbol(name='foo'); cargo-test!");
        assert!(tokens.contains(&"find_symbol".to_string()));
        assert!(tokens.contains(&"name".to_string()));
        assert!(tokens.contains(&"foo".to_string()));
        assert!(tokens.contains(&"cargo".to_string()));
        assert!(tokens.contains(&"test".to_string()));
    }

    #[test]
    fn tokenize_drops_short_tokens() {
        let tokens = tokenize("a an and alpha");
        // `a` dropped (len 1). `an`/`and`/`alpha` kept.
        assert!(!tokens.contains(&"a".to_string()));
        assert!(tokens.contains(&"an".to_string()));
        assert!(tokens.contains(&"and".to_string()));
        assert!(tokens.contains(&"alpha".to_string()));
    }
}
