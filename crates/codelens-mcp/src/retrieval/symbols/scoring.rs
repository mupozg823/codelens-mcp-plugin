use super::corpus::SymbolDocument;
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
            .filter(|t| seen.insert((*t).clone()))
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
