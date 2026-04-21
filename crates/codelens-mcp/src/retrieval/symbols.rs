//! Symbol-aware BM25-F retrieval — corpus builder + scorer.
//!
//! This is the **sparse lane** for code search: identifiers, signatures,
//! path tokens, refactor-preflight shortlists, and frontier-model
//! context packing. Lives alongside the dense (MiniLM) lane rather than
//! replacing it; the dense lane handles long natural-language intent
//! queries.
//!
//! The unit of retrieval is a *symbol* (function, struct, trait, etc.)
//! with its most information-dense slots pulled apart into fields that
//! BM25-F can weight differently, rather than whole source files.
//!
//! ### Field schema (corpus side)
//!
//! - `name_path` — fully qualified path like `dispatch::dispatch_tool`.
//!   Short, high-signal, unambiguous.
//! - `name` — unqualified identifier.
//! - `signature` — parameter names, return type, type parameters.
//! - `file_path` / `module_path` — path-token hits (`mutation`,
//!   `workflow`, `rename`).
//! - `doc_comment` — rustdoc / JSDoc body. Empty until a body parser
//!   lands.
//! - `body_lexical_chunk` — identifiers + string literals + macro call
//!   names extracted from the body, capped so long functions do not
//!   dominate the corpus length. Full body is left to the dense lane.
//!
//! ### Flags
//!
//! - `is_test` / `is_generated` — corpus downweighters (applied at
//!   scoring time, not here). Detected heuristically from path.
//! - `exported` — public-API boost. Detected from signature prefix.
//! - `language` — file extension.
//!
//! ### Field weights (scorer side)
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

mod corpus;
mod scoring;

pub(crate) use corpus::build_symbol_corpus;
pub(crate) use scoring::{search_symbols_bm25f, unique_query_terms};

#[cfg(test)]
use self::corpus::{
    BODY_TOKEN_CAP, SymbolDocument, detect_exported, detect_is_generated, detect_is_test,
    extract_lexical_tokens, from_symbol_info, language_from_path, module_path_from_file,
};
#[cfg(test)]
use codelens_engine::{SymbolInfo, SymbolKind};

#[cfg(test)]
mod tests {
    use super::*;

    // ─── corpus tests (formerly symbol_corpus.rs) ───

    fn make_symbol(
        name: &str,
        name_path: &str,
        file_path: &str,
        signature: &str,
        body: Option<&str>,
    ) -> SymbolInfo {
        SymbolInfo {
            name: name.to_owned(),
            kind: SymbolKind::Function,
            file_path: file_path.to_owned(),
            line: 10,
            column: 4,
            signature: signature.to_owned(),
            name_path: name_path.to_owned(),
            id: format!("{}#function:{}", file_path, name_path),
            provenance: codelens_engine::SymbolProvenance::default(),
            body: body.map(str::to_owned),
            children: Vec::new(),
            start_byte: 0,
            end_byte: body.map(|b| b.len() as u32).unwrap_or(0),
            end_line: 10,
        }
    }

    #[test]
    fn module_path_strips_crate_src_prefix() {
        assert_eq!(
            module_path_from_file("crates/codelens-mcp/src/dispatch/mod.rs"),
            "dispatch::mod"
        );
        assert_eq!(
            module_path_from_file("crates/codelens-engine/src/symbols/scoring.rs"),
            "symbols::scoring"
        );
    }

    #[test]
    fn language_detection_covers_common_extensions() {
        assert_eq!(language_from_path("foo/bar.rs"), "rust");
        assert_eq!(language_from_path("foo/bar.ts"), "typescript");
        assert_eq!(language_from_path("foo/bar.tsx"), "typescript");
        assert_eq!(language_from_path("foo/bar.py"), "python");
        assert_eq!(language_from_path("foo/bar.go"), "go");
        assert_eq!(language_from_path("foo/README.md"), "other");
    }

    #[test]
    fn is_test_detects_common_patterns() {
        assert!(detect_is_test(
            "crates/codelens-mcp/src/integration_tests/workflow.rs"
        ));
        assert!(detect_is_test(
            "crates/codelens-engine/tests/rename_real.rs"
        ));
        assert!(detect_is_test("src/foo_test.rs"));
        assert!(detect_is_test("src/foo.test.ts"));
        assert!(detect_is_test("src/foo.spec.ts"));
        assert!(!detect_is_test("crates/codelens-mcp/src/dispatch/mod.rs"));
    }

    #[test]
    fn is_generated_detects_build_paths() {
        assert!(detect_is_generated("target/debug/build/something.rs"));
        assert!(detect_is_generated("src/generated/types.rs"));
        assert!(detect_is_generated("src/proto.pb.rs"));
        assert!(!detect_is_generated("src/symbols/mod.rs"));
    }

    #[test]
    fn exported_from_rust_pub_signature() {
        assert!(detect_exported(
            "pub fn foo() -> i32",
            &SymbolKind::Function
        ));
        assert!(detect_exported(
            "pub(crate) fn bar()",
            &SymbolKind::Function
        ));
        assert!(!detect_exported(
            "fn private_helper()",
            &SymbolKind::Function
        ));
    }

    #[test]
    fn exported_from_ts_export_keyword() {
        assert!(detect_exported(
            "export function handle(req)",
            &SymbolKind::Function
        ));
        assert!(detect_exported(
            "export default class App",
            &SymbolKind::Class
        ));
        assert!(!detect_exported(
            "function internalOnly()",
            &SymbolKind::Function
        ));
    }

    #[test]
    fn extract_lexical_tokens_keeps_identifiers_unique_and_lowercased() {
        let body = "let user_name = get_user_name(); let user_name = upper(user_name);";
        let tokens = extract_lexical_tokens(body);
        let list: Vec<&str> = tokens.split_whitespace().collect();
        // `user_name` appears 3×, `get_user_name` 1×, `upper` 1×, `let` 2×.
        // After uniquing: 4 tokens.
        assert!(list.contains(&"user_name"));
        assert!(list.contains(&"get_user_name"));
        assert!(list.contains(&"upper"));
        assert!(list.contains(&"let"));
        // Uniqueness: no duplicates.
        let as_set: std::collections::HashSet<&&str> = list.iter().collect();
        assert_eq!(as_set.len(), list.len());
    }

    #[test]
    fn extract_lexical_tokens_caps_at_limit() {
        let body: String = (0..1000).map(|i| format!("tok{i} ")).collect();
        let tokens = extract_lexical_tokens(&body);
        let count = tokens.split_whitespace().count();
        assert!(count <= BODY_TOKEN_CAP);
    }

    #[test]
    fn from_symbol_info_preserves_ids_and_derives_flags() {
        let symbol = make_symbol(
            "evaluate_mutation_gate",
            "mutation_gate::evaluate_mutation_gate",
            "crates/codelens-mcp/src/mutation_gate.rs",
            "pub fn evaluate_mutation_gate(...)",
            Some("let state = foo(); bar();"),
        );
        let d = from_symbol_info(&symbol);
        assert_eq!(d.symbol_id, symbol.id);
        assert_eq!(d.name, "evaluate_mutation_gate");
        assert_eq!(d.name_path, "mutation_gate::evaluate_mutation_gate");
        assert_eq!(d.kind, "function");
        assert_eq!(d.module_path, "mutation_gate");
        assert_eq!(d.language, "rust");
        assert!(d.exported, "pub fn should be flagged exported");
        assert!(!d.is_test);
        assert!(!d.is_generated);
        assert!(d.body_lexical_chunk.contains("state"));
        assert!(d.body_lexical_chunk.contains("foo"));
    }

    #[test]
    fn build_symbol_corpus_maps_many() {
        let symbols = vec![
            make_symbol("a", "mod::a", "src/mod.rs", "pub fn a()", None),
            make_symbol("b", "mod::b", "src/mod.rs", "fn b()", None),
            make_symbol("c", "tests::c", "tests/integration.rs", "fn c()", None),
        ];
        let corpus = build_symbol_corpus(&symbols);
        assert_eq!(corpus.len(), 3);
        assert!(corpus[0].exported);
        assert!(!corpus[1].exported);
        assert!(corpus[2].is_test);
    }

    // ─── retrieval tests (formerly symbol_retrieval.rs) ───

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
