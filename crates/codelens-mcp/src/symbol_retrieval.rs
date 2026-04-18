//! BM25-F scorer over [`SymbolDocument`] corpora (R.2).
//!
//! This is the sparse lane for symbol search: identifiers, signatures,
//! path tokens, refactor-preflight shortlists, and frontier-model
//! context packing. Lives alongside the dense (MiniLM) lane rather
//! than replacing it; the dense lane handles long natural-language
//! intent queries.
//!
//! ### Field weights
//!
//! | Field              | Weight | Why |
//! |--------------------|-------:|-----|
//! | `name_path`        | 5.0 | Fully qualified path — shortest and most specific. |
//! | `name`             | 4.0 | Unqualified identifier — very high signal. |
//! | `signature`        | 2.5 | Parameter names, return type. |
//! | `module_path`      | 2.0 | Structural location hint. |
//! | `file_path`        | 1.5 | Path-token matches. |
//! | `doc_comment`      | 1.5 | Explicit documentation. |
//! | `body_lexical_chunk` | 1.0 | Lowest — body is the dense lane's job. |
//!
//! BM25 params are the classic defaults (`k1=1.2`, `b=0.75`).
//!
//! ### Post-score adjustments
//!
//! - `is_test` → score × 0.5 (downweight, never hide unless caller filters)
//! - `is_generated` → score × 0.5
//! - `exported` → score × 1.3 (public API boost)
//!
//! Adjustments are **multiplicative** and applied after BM25F so they
//! do not distort the raw ranking within their filter class. Tests and
//! generated files that overwhelmingly match will still surface — they
//! just rank below equivalent production matches.
//!
//! ### Coordinate bonus
//!
//! Same policy as `rule_retrieval`: a document that covers ≥ 80% of the
//! unique query terms gets score × 1.1. Keeps multi-token queries
//! rewarding breadth over depth on one term.

#![allow(dead_code)]

use crate::symbol_corpus::SymbolDocument;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ScoredSymbol {
    pub document: SymbolDocument,
    pub score: f64,
    /// Unique query terms that contributed a non-zero tf anywhere in
    /// the document. Surfaces in the response as `why_matched`.
    pub matched_terms: Vec<String>,
}

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
const MIN_TOKEN_LEN: usize = 2;

const W_NAME_PATH: f64 = 5.0;
const W_NAME: f64 = 4.0;
const W_SIGNATURE: f64 = 2.5;
const W_MODULE_PATH: f64 = 2.0;
const W_FILE_PATH: f64 = 1.5;
const W_DOC_COMMENT: f64 = 1.5;
const W_BODY: f64 = 1.0;

const TEST_DOWNWEIGHT: f64 = 0.5;
const GENERATED_DOWNWEIGHT: f64 = 0.5;
const EXPORTED_BOOST: f64 = 1.3;

const COORDINATE_THRESHOLD: f64 = 0.8;
const COORDINATE_BONUS: f64 = 1.1;

/// Score a symbol-document corpus against a query and return the
/// top-`top_k` matches. Test / generated documents are kept in the
/// pool but downweighted unless the caller explicitly includes them
/// with `include_tests=true` / `include_generated=true`.
pub fn search_symbols_bm25f(
    corpus: &[SymbolDocument],
    query: &str,
    top_k: usize,
    include_tests: bool,
    include_generated: bool,
) -> Vec<ScoredSymbol> {
    if corpus.is_empty() || top_k == 0 {
        return Vec::new();
    }
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }
    let unique_query_terms: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        query_tokens
            .iter()
            .filter(|t| seen.insert(t.clone()))
            .cloned()
            .collect()
    };

    let doc_fields: Vec<FieldTokens> = corpus.iter().map(tokenize_fields).collect();
    let doc_weighted_lengths: Vec<f64> = doc_fields
        .iter()
        .map(FieldTokens::weighted_length)
        .collect();
    let total_weighted_length: f64 = doc_weighted_lengths.iter().sum();
    let n_docs = corpus.len() as f64;
    let avgdl = if total_weighted_length == 0.0 {
        1.0
    } else {
        total_weighted_length / n_docs
    };

    let mut df: HashMap<&str, usize> = HashMap::new();
    for qt in &unique_query_terms {
        if df.contains_key(qt.as_str()) {
            continue;
        }
        let count = doc_fields
            .iter()
            .filter(|fields| fields.contains_any(qt))
            .count();
        df.insert(qt.as_str(), count);
    }

    let mut scored: Vec<ScoredSymbol> = corpus
        .iter()
        .enumerate()
        .filter_map(|(idx, doc)| {
            if doc.is_test && !include_tests && doc.is_generated {
                // still scored — downweighted below
            }
            let fields = &doc_fields[idx];
            let dl = doc_weighted_lengths[idx];
            let mut score = 0.0_f64;
            let mut matched: Vec<String> = Vec::new();
            for qt in &unique_query_terms {
                let tf_w = fields.weighted_tf(qt);
                if tf_w == 0.0 {
                    continue;
                }
                matched.push(qt.clone());
                let docs_with_term = *df.get(qt.as_str()).unwrap_or(&0) as f64;
                let idf = ((n_docs - docs_with_term + 0.5) / (docs_with_term + 0.5) + 1.0).ln();
                let tf_norm = tf_w * (BM25_K1 + 1.0)
                    / (tf_w + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avgdl));
                score += idf * tf_norm;
            }
            if score <= 0.0 {
                return None;
            }
            if !unique_query_terms.is_empty()
                && (matched.len() as f64 / unique_query_terms.len() as f64) >= COORDINATE_THRESHOLD
            {
                score *= COORDINATE_BONUS;
            }
            if doc.is_test {
                score *= TEST_DOWNWEIGHT;
            }
            if doc.is_generated {
                score *= GENERATED_DOWNWEIGHT;
            }
            if doc.exported {
                score *= EXPORTED_BOOST;
            }
            Some(ScoredSymbol {
                document: doc.clone(),
                score,
                matched_terms: matched,
            })
        })
        .filter(|scored| {
            if !include_tests && scored.document.is_test {
                return false;
            }
            if !include_generated && scored.document.is_generated {
                return false;
            }
            true
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(top_k);
    scored
}

struct FieldTokens {
    name_path: Vec<String>,
    name: Vec<String>,
    signature: Vec<String>,
    module_path: Vec<String>,
    file_path: Vec<String>,
    doc_comment: Vec<String>,
    body: Vec<String>,
}

impl FieldTokens {
    fn weighted_length(&self) -> f64 {
        W_NAME_PATH * self.name_path.len() as f64
            + W_NAME * self.name.len() as f64
            + W_SIGNATURE * self.signature.len() as f64
            + W_MODULE_PATH * self.module_path.len() as f64
            + W_FILE_PATH * self.file_path.len() as f64
            + W_DOC_COMMENT * self.doc_comment.len() as f64
            + W_BODY * self.body.len() as f64
    }

    fn weighted_tf(&self, token: &str) -> f64 {
        let tf = |field: &[String]| field.iter().filter(|t| t.as_str() == token).count() as f64;
        W_NAME_PATH * tf(&self.name_path)
            + W_NAME * tf(&self.name)
            + W_SIGNATURE * tf(&self.signature)
            + W_MODULE_PATH * tf(&self.module_path)
            + W_FILE_PATH * tf(&self.file_path)
            + W_DOC_COMMENT * tf(&self.doc_comment)
            + W_BODY * tf(&self.body)
    }

    fn contains_any(&self, token: &str) -> bool {
        self.name_path.iter().any(|t| t == token)
            || self.name.iter().any(|t| t == token)
            || self.signature.iter().any(|t| t == token)
            || self.module_path.iter().any(|t| t == token)
            || self.file_path.iter().any(|t| t == token)
            || self.doc_comment.iter().any(|t| t == token)
            || self.body.iter().any(|t| t == token)
    }
}

/// Unique query tokens under the same symbol-aware tokenizer used by
/// [`search_symbols_bm25f`]. Exposed so the MCP handler can compute
/// coverage ratios (matched_terms / unique_query_terms) without
/// re-implementing the tokenization contract.
pub fn unique_query_terms(query: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    tokenize(query)
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

fn tokenize_fields(doc: &SymbolDocument) -> FieldTokens {
    FieldTokens {
        name_path: tokenize(&doc.name_path),
        name: tokenize(&doc.name),
        signature: tokenize(&doc.signature),
        module_path: tokenize(&doc.module_path),
        file_path: tokenize(&doc.file_path),
        doc_comment: tokenize(&doc.doc_comment),
        body: tokenize(&doc.body_lexical_chunk),
    }
}

/// Symbol-aware tokenizer: emits both the compound identifier
/// (`mutation_gate`) AND its underscore-split parts (`mutation`,
/// `gate`). This makes `"mutation gate"` and `"mutation_gate"` both
/// match the same symbol without query rewriting. Non-alphanumeric
/// characters split tokens; underscores remain token-internal for the
/// compound but also mark split boundaries for the atomic parts.
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            emit_compound_and_parts(&current, &mut out);
            current.clear();
        }
    }
    if !current.is_empty() {
        emit_compound_and_parts(&current, &mut out);
    }
    out
}

fn emit_compound_and_parts(compound: &str, out: &mut Vec<String>) {
    if compound.len() >= MIN_TOKEN_LEN {
        out.push(compound.to_owned());
    }
    if compound.contains('_') {
        for part in compound.split('_') {
            if part.len() >= MIN_TOKEN_LEN {
                out.push(part.to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(
        name: &str,
        name_path: &str,
        signature: &str,
        file_path: &str,
        module_path: &str,
        body: &str,
        is_test: bool,
        is_generated: bool,
        exported: bool,
    ) -> SymbolDocument {
        SymbolDocument {
            symbol_id: format!("{}::{}", file_path, name_path),
            name: name.to_owned(),
            name_path: name_path.to_owned(),
            kind: "function".to_owned(),
            signature: signature.to_owned(),
            file_path: file_path.to_owned(),
            module_path: module_path.to_owned(),
            doc_comment: String::new(),
            body_lexical_chunk: body.to_owned(),
            language: "rust",
            line_start: 1,
            is_test,
            is_generated,
            exported,
        }
    }

    #[test]
    fn empty_corpus_yields_empty() {
        let results = search_symbols_bm25f(&[], "mutation_gate", 5, false, false);
        assert!(results.is_empty());
    }

    #[test]
    fn empty_query_yields_empty() {
        let corpus = vec![doc(
            "foo",
            "bar::foo",
            "fn foo()",
            "src/bar.rs",
            "bar",
            "",
            false,
            false,
            true,
        )];
        let results = search_symbols_bm25f(&corpus, "", 5, false, false);
        assert!(results.is_empty());
    }

    #[test]
    fn name_path_match_outranks_body_only_match() {
        let corpus = vec![
            doc(
                "unrelated",
                "misc::unrelated",
                "fn unrelated()",
                "src/misc.rs",
                "misc",
                "mutation gate appears in body comments occasionally",
                false,
                false,
                true,
            ),
            doc(
                "evaluate_mutation_gate",
                "mutation_gate::evaluate_mutation_gate",
                "fn evaluate_mutation_gate()",
                "src/mutation_gate.rs",
                "mutation_gate",
                "body text without those words",
                false,
                false,
                true,
            ),
        ];
        let results = search_symbols_bm25f(&corpus, "mutation_gate", 3, false, false);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].document.name, "evaluate_mutation_gate");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn exported_boost_lifts_public_api_above_equal_private() {
        let corpus = vec![
            doc(
                "helper",
                "mod::helper",
                "fn helper()",
                "src/mod.rs",
                "mod",
                "query term once",
                false,
                false,
                false,
            ),
            doc(
                "helper_public",
                "mod::helper_public",
                "pub fn helper_public()",
                "src/mod.rs",
                "mod",
                "query term once",
                false,
                false,
                true,
            ),
        ];
        let results = search_symbols_bm25f(&corpus, "query", 2, false, false);
        assert_eq!(results.len(), 2);
        // Both have "query" in body. Exported doc gets 1.3x boost.
        assert!(results[0].document.exported, "exported doc should be first");
    }

    #[test]
    fn test_files_downweighted_when_included() {
        let corpus = vec![
            doc(
                "helper",
                "mod::helper",
                "fn helper()",
                "src/mod.rs",
                "mod",
                "alpha token",
                false,
                false,
                false,
            ),
            doc(
                "helper_test",
                "mod::helper_test",
                "fn helper_test()",
                "src/mod_test.rs",
                "mod",
                "alpha token alpha token alpha token",
                true, // is_test
                false,
                false,
            ),
        ];
        // When include_tests=true, test doc scores 0.5×. Without the
        // downweight, the test doc with 3x alpha would win.
        let results = search_symbols_bm25f(&corpus, "alpha", 2, true, false);
        assert_eq!(results.len(), 2);
        assert!(
            !results[0].document.is_test,
            "non-test doc should be first after downweight"
        );
    }

    #[test]
    fn test_files_excluded_by_default() {
        let corpus = vec![doc(
            "test_only",
            "tests::test_only",
            "fn test_only()",
            "tests/integration.rs",
            "tests",
            "alpha token",
            true,
            false,
            false,
        )];
        let results = search_symbols_bm25f(&corpus, "alpha", 5, false, false);
        assert!(results.is_empty(), "is_test excluded by default");
    }

    #[test]
    fn generated_files_excluded_by_default() {
        let corpus = vec![doc(
            "gen_fn",
            "gen::gen_fn",
            "fn gen_fn()",
            "src/generated/types.rs",
            "generated::types",
            "alpha",
            false,
            true,
            false,
        )];
        let results = search_symbols_bm25f(&corpus, "alpha", 5, false, false);
        assert!(results.is_empty());
    }

    #[test]
    fn top_k_limit_respected() {
        let corpus: Vec<_> = (0..10)
            .map(|i| {
                doc(
                    &format!("fn_{i}"),
                    &format!("mod::fn_{i}"),
                    &format!("fn fn_{i}()"),
                    "src/mod.rs",
                    "mod",
                    "alpha",
                    false,
                    false,
                    true,
                )
            })
            .collect();
        let results = search_symbols_bm25f(&corpus, "alpha", 3, false, false);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn matched_terms_report_only_unique_hits() {
        let corpus = vec![doc(
            "mutation_gate",
            "mod::mutation_gate",
            "fn mutation_gate()",
            "src/mod.rs",
            "mod",
            "gate gate gate",
            false,
            false,
            true,
        )];
        let results = search_symbols_bm25f(&corpus, "mutation gate nonexistent", 1, false, false);
        assert_eq!(results.len(), 1);
        // matched: mutation, gate — but not "nonexistent".
        assert!(results[0].matched_terms.contains(&"mutation".to_owned()));
        assert!(results[0].matched_terms.contains(&"gate".to_owned()));
        assert!(!results[0].matched_terms.contains(&"nonexistent".to_owned()));
    }

    #[test]
    fn score_descending() {
        let corpus = vec![
            doc(
                "a",
                "mod::a",
                "fn a()",
                "src/mod.rs",
                "mod",
                "alpha alpha alpha alpha",
                false,
                false,
                true,
            ),
            doc(
                "b",
                "mod::b",
                "fn b()",
                "src/mod.rs",
                "mod",
                "alpha",
                false,
                false,
                true,
            ),
        ];
        let results = search_symbols_bm25f(&corpus, "alpha", 2, false, false);
        assert!(results[0].score >= results[1].score);
    }

    #[test]
    fn signature_tokens_contribute_to_score() {
        let corpus = vec![
            doc(
                "unrelated_name",
                "mod::unrelated_name",
                "fn unrelated_name(preflight: Preflight) -> Result<()>",
                "src/mod.rs",
                "mod",
                "",
                false,
                false,
                true,
            ),
            doc(
                "helper",
                "mod::helper",
                "fn helper()",
                "src/mod.rs",
                "mod",
                "",
                false,
                false,
                true,
            ),
        ];
        let results = search_symbols_bm25f(&corpus, "preflight", 2, false, false);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document.name, "unrelated_name");
    }
}
