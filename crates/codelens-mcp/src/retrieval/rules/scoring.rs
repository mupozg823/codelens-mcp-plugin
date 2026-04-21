use super::corpus::RuleSnippet;
use std::collections::HashMap;

/// One rule snippet paired with its BM25F score against the query.
#[derive(Debug, Clone)]
pub struct ScoredSnippet {
    pub snippet: RuleSnippet,
    pub score: f64,
}

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
const MIN_TOKEN_LEN: usize = 2;
const W_NAME: f64 = 3.0;
const W_TITLE: f64 = 2.0;
const W_BODY: f64 = 1.0;
const COORDINATE_THRESHOLD: f64 = 0.8;
const COORDINATE_BONUS: f64 = 1.1;

struct FieldTokens {
    name: Vec<String>,
    title: Vec<String>,
    body: Vec<String>,
}

impl FieldTokens {
    fn total_any(&self) -> bool {
        !self.name.is_empty() || !self.title.is_empty() || !self.body.is_empty()
    }

    fn contains(&self, token: &str) -> bool {
        self.name.iter().any(|t| t == token)
            || self.title.iter().any(|t| t == token)
            || self.body.iter().any(|t| t == token)
    }

    fn weighted_tf(&self, token: &str) -> f64 {
        let tf_name = self.name.iter().filter(|t| *t == token).count() as f64;
        let tf_title = self.title.iter().filter(|t| *t == token).count() as f64;
        let tf_body = self.body.iter().filter(|t| *t == token).count() as f64;
        W_NAME * tf_name + W_TITLE * tf_title + W_BODY * tf_body
    }

    fn weighted_length(&self) -> f64 {
        W_NAME * self.name.len() as f64
            + W_TITLE * self.title.len() as f64
            + W_BODY * self.body.len() as f64
    }
}

fn tokenize_fields(snippet: &RuleSnippet) -> FieldTokens {
    FieldTokens {
        name: tokenize(snippet.frontmatter_name.as_deref().unwrap_or("")),
        title: tokenize(snippet.section_title.as_deref().unwrap_or("")),
        body: tokenize(&snippet.content),
    }
}

/// Score every snippet against `query` via BM25F and return the top
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
    let unique_query_terms: Vec<&String> = {
        let mut seen = std::collections::HashSet::new();
        query_tokens
            .iter()
            .filter(|qt| seen.insert(qt.as_str()))
            .collect()
    };

    let doc_fields: Vec<FieldTokens> = corpus.iter().map(tokenize_fields).collect();
    let doc_weighted_lengths: Vec<f64> = doc_fields.iter().map(|f| f.weighted_length()).collect();
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
            .filter(|fields| fields.total_any() && fields.contains(qt.as_str()))
            .count();
        df.insert(qt.as_str(), count);
    }

    let mut scored: Vec<ScoredSnippet> = corpus
        .iter()
        .enumerate()
        .map(|(idx, snippet)| {
            let fields = &doc_fields[idx];
            let dl = doc_weighted_lengths[idx];
            let mut score = 0.0_f64;
            let mut matched_terms = 0usize;
            for qt in &unique_query_terms {
                let tf_w = fields.weighted_tf(qt.as_str());
                if tf_w == 0.0 {
                    continue;
                }
                matched_terms += 1;
                let docs_with_term = *df.get(qt.as_str()).unwrap_or(&0) as f64;
                let idf = ((n_docs - docs_with_term + 0.5) / (docs_with_term + 0.5) + 1.0).ln();
                let tf_norm = tf_w * (BM25_K1 + 1.0)
                    / (tf_w + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avgdl));
                score += idf * tf_norm;
            }
            let coverage = if unique_query_terms.is_empty() {
                0.0
            } else {
                matched_terms as f64 / unique_query_terms.len() as f64
            };
            if score > 0.0 && coverage >= COORDINATE_THRESHOLD {
                score *= COORDINATE_BONUS;
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

#[allow(dead_code)]
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
    use crate::retrieval::rules::RuleSource;
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
        assert!(!tokens.contains(&"a".to_string()));
        assert!(tokens.contains(&"an".to_string()));
        assert!(tokens.contains(&"and".to_string()));
        assert!(tokens.contains(&"alpha".to_string()));
    }

    #[test]
    fn frontmatter_name_match_outranks_body_only_match() {
        let corpus = vec![
            snippet(
                RuleSource::Memory,
                Some("caching strategy"),
                None,
                "unrelated text no match here",
            ),
            snippet(
                RuleSource::Memory,
                Some("unrelated entry"),
                None,
                "we discuss caching in this paragraph exactly once",
            ),
        ];
        let results = find_relevant_rules(&corpus, "caching", 2);
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].snippet.frontmatter_name.as_deref(),
            Some("caching strategy"),
            "name-field match should rank above body-only match"
        );
        assert!(
            results[0].score > results[1].score,
            "expected {} > {}",
            results[0].score,
            results[1].score
        );
    }

    #[test]
    fn title_match_outranks_body_only_match() {
        let corpus = vec![
            snippet(
                RuleSource::Global,
                None,
                Some("Mutation Gate Protocol"),
                "body content without the keyword",
            ),
            snippet(
                RuleSource::Global,
                None,
                Some("Unrelated Section"),
                "this paragraph mentions mutation once",
            ),
        ];
        let results = find_relevant_rules(&corpus, "mutation", 2);
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].snippet.section_title.as_deref(),
            Some("Mutation Gate Protocol"),
            "title-field match should rank above body-only match"
        );
    }
}
