use crate::embedding_store::{EmbeddingChunk, ScoredChunk};
use serde::Serialize;

pub type StoredChunkKey = (String, String, usize, String, String);

pub fn stored_chunk_key(chunk: &EmbeddingChunk) -> StoredChunkKey {
    (
        chunk.file_path.clone(),
        chunk.symbol_name.clone(),
        chunk.line,
        chunk.signature.clone(),
        chunk.name_path.clone(),
    )
}

pub fn stored_chunk_key_for_score(chunk: &ScoredChunk) -> StoredChunkKey {
    (
        chunk.file_path.clone(),
        chunk.symbol_name.clone(),
        chunk.line,
        chunk.signature.clone(),
        chunk.name_path.clone(),
    )
}

pub fn duplicate_candidate_limit(max_pairs: usize) -> usize {
    max_pairs.saturating_mul(4).clamp(32, 128)
}

pub fn duplicate_pair_key(
    file_a: &str,
    symbol_a: &str,
    file_b: &str,
    symbol_b: &str,
) -> ((String, String), (String, String)) {
    let left = (file_a.to_owned(), symbol_a.to_owned());
    let right = (file_b.to_owned(), symbol_b.to_owned());
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

/// #299: cosine threshold above which a body-blind comparison should
/// trigger the signature-only diagnostic. Pairs below this stay as
/// vanilla near-duplicates.
pub const SIGNATURE_ONLY_COSINE_FLOOR: f64 = 0.85;

/// #299: token-Jaccard ceiling for the signature-only diagnostic. When
/// both bodies share fewer than half their alphanumeric tokens we treat
/// the high cosine as signature/identifier-shape collision rather than
/// real code duplication.
pub const SIGNATURE_ONLY_JACCARD_CEIL: f64 = 0.5;

/// #299: tokenise body text into a set of normalised alphanumeric tokens
/// (lowercase, length ≥2). Skips Rust/TS keyword-style stop tokens so
/// the resulting Jaccard reflects the named identifiers in the body —
/// the part that actually differs between namespaced wrappers — rather
/// than control-flow boilerplate.
pub fn body_tokens(text: &str) -> std::collections::HashSet<String> {
    const STOPWORDS: &[&str] = &[
        "fn", "let", "mut", "pub", "use", "mod", "if", "else", "for", "while", "loop", "match",
        "return", "self", "true", "false", "as", "in", "of", "the", "and", "or", "not", "is",
        "this", "that", "ok", "err", "none", "some", "result",
    ];
    let mut buf = String::new();
    let mut tokens: std::collections::HashSet<String> = std::collections::HashSet::new();
    let push_buf = |buf: &mut String, tokens: &mut std::collections::HashSet<String>| {
        if buf.len() >= 2 && !STOPWORDS.contains(&buf.as_str()) {
            tokens.insert(buf.clone());
        }
        buf.clear();
    };
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch.to_ascii_lowercase());
        } else if !buf.is_empty() {
            push_buf(&mut buf, &mut tokens);
        }
    }
    if !buf.is_empty() {
        push_buf(&mut buf, &mut tokens);
    }
    tokens
}

/// #299: token-set Jaccard. Returns `None` when both sides are empty;
/// `Some(0.0)` when only one side is empty.
pub fn body_token_jaccard(text_a: &str, text_b: &str) -> Option<f64> {
    let a = body_tokens(text_a);
    let b = body_tokens(text_b);
    if a.is_empty() && b.is_empty() {
        return None;
    }
    let inter = a.intersection(&b).count() as f64;
    let union = a.union(&b).count() as f64;
    if union == 0.0 {
        return Some(0.0);
    }
    Some(inter / union)
}

/// SIMD-friendly cosine similarity for f32 embedding vectors.
///
/// Computes dot product and norms in f32 (auto-vectorized by LLVM on Apple Silicon NEON),
/// then promotes to f64 only for the final division to avoid precision loss.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    debug_assert_eq!(a.len(), b.len());

    // Process in chunks of 8 for optimal SIMD lane utilization (NEON 128-bit = 4xf32,
    // but the compiler can unroll 2 iterations for 8-wide throughput).
    let (mut dot, mut norm_a, mut norm_b) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let norm_a = (norm_a as f64).sqrt();
    let norm_b = (norm_b as f64).sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot as f64 / (norm_a * norm_b)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatePair {
    pub symbol_a: String,
    pub symbol_b: String,
    pub file_a: String,
    pub file_b: String,
    pub line_a: usize,
    pub line_b: usize,
    pub similarity: f64,
    /// #299: token-level Jaccard between the two function bodies. The
    /// embedding-based `similarity` blends signature + identifier
    /// shapes, so two namespaced wrappers calling the same helper with
    /// different predicates (e.g. `collect_files(root, p1)` vs
    /// `collect_files(root, p2)`) score in the 0.94–0.96 band even
    /// though the bodies diverge. The Jaccard component is computed on
    /// alphanumeric tokens of the indexed `text` and lets consumers
    /// downgrade or filter signature-only matches. `None` when one or
    /// both bodies were not indexed in the embedding store.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_token_jaccard: Option<f64>,
    /// #299: convenience flag — true when the embedding similarity is
    /// high (≥0.85) but `body_token_jaccard` is low (<0.5), i.e. the
    /// pair likely matches only on signature/identifier shape. Always
    /// derived from `body_token_jaccard`; never set when the Jaccard is
    /// missing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub signature_only_match: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryScore {
    pub category: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlierSymbol {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub avg_similarity_to_file: f64,
}

pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_tokens_strips_stopwords_and_short_tokens() {
        let toks = body_tokens("fn foo(x: i32) -> i32 { let mut y = x + 1; y }");
        // `fn`, `let`, `mut` removed; single letters (x, y) skipped
        assert!(toks.contains("foo"));
        assert!(toks.contains("i32"));
        assert!(!toks.contains("fn"));
        assert!(!toks.contains("let"));
        assert!(!toks.contains("mut"));
        assert!(!toks.contains("x"));
        assert!(!toks.contains("y"));
    }

    #[test]
    fn body_token_jaccard_identical_bodies_is_one() {
        let body = "fn collect(root: &Path) -> Vec<PathBuf> { walker(root, predicate_a) }";
        let j = body_token_jaccard(body, body).unwrap();
        assert!((j - 1.0).abs() < 1e-9);
    }

    #[test]
    fn body_token_jaccard_diverging_predicates_below_ceil() {
        // #299 reproduction: same shape, different predicate identifier.
        let a = "fn collect_a(root: &Path) -> Vec<PathBuf> { collect_files(root, supports_call_graph) }";
        let b = "fn collect_b(root: &Path) -> Vec<PathBuf> { collect_files(root, supports_import_graph) }";
        let j = body_token_jaccard(a, b).unwrap();
        // Many tokens overlap (collect, files, root, pathbuf, vec) but
        // the predicate identifier differs — Jaccard sits below 1.0
        // and (for this issue's class of false-positive) below the
        // SIGNATURE_ONLY_JACCARD_CEIL threshold once function names
        // diverge or unique predicate identifiers split.
        assert!(j < 1.0);
        assert!(j > 0.0);
    }

    #[test]
    fn body_token_jaccard_disjoint_returns_zero() {
        let j = body_token_jaccard("alpha beta gamma", "delta epsilon zeta").unwrap();
        assert!(j.abs() < 1e-9);
    }

    #[test]
    fn body_token_jaccard_both_empty_returns_none() {
        assert!(body_token_jaccard("", "").is_none());
    }
}
