//! Symbol-document corpus for BM25-F sparse retrieval (R.1).
//!
//! A deliberate parallel to `rule_corpus`: rather than indexing entire
//! source files, the unit of retrieval here is a *symbol* (function,
//! struct, trait, etc.) with its most information-dense slots pulled
//! apart into fields BM25-F can weight differently.
//!
//! This is the **sparse lane** for code search. It does not replace the
//! existing FTS5 symbol index (which is file-centric and tied to the
//! SQLite schema). It is a separate, in-memory corpus built from the
//! same `SymbolInfo` data, optimised for the `bm25_symbol_search` tool
//! shipped in R.2.
//!
//! ### Field schema
//!
//! - `name_path` — top weight. Fully qualified path like
//!   `dispatch::dispatch_tool`. Short, high-signal, unambiguous.
//! - `name` — the unqualified identifier.
//! - `signature` — parameter names, return type, type parameters.
//! - `file_path` / `module_path` — path-token hits (e.g. `mutation`,
//!   `workflow`, `rename`).
//! - `doc_comment` — rustdoc / JSDoc body. Left empty at R.1; populated
//!   once a body parser lands.
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
//! ### Scope at R.1
//!
//! This module only defines the struct, a `from_symbol_info` conversion,
//! the lexical-token extractor, and the heuristic flag detectors. The
//! BM25-F scorer that consumes a `Vec<SymbolDocument>` lands in R.2 as
//! the `bm25_symbol_search` MCP tool. No integration with the active
//! `SymbolIndex` yet — that wiring follows once the scorer is proven on
//! unit fixtures.

#![allow(dead_code)]

use codelens_engine::{SymbolInfo, SymbolKind};

const BODY_TOKEN_CAP: usize = 300;
const MIN_TOKEN_LEN: usize = 2;

#[derive(Debug, Clone)]
pub struct SymbolDocument {
    pub symbol_id: String,
    pub name: String,
    pub name_path: String,
    pub kind: String,
    pub signature: String,
    pub file_path: String,
    pub module_path: String,
    pub doc_comment: String,
    pub body_lexical_chunk: String,
    pub language: &'static str,
    pub line_start: usize,
    pub is_test: bool,
    pub is_generated: bool,
    pub exported: bool,
}

pub fn build_symbol_corpus(infos: &[SymbolInfo]) -> Vec<SymbolDocument> {
    infos.iter().map(from_symbol_info).collect()
}

pub fn from_symbol_info(info: &SymbolInfo) -> SymbolDocument {
    let body_lexical_chunk = info
        .body
        .as_deref()
        .map(extract_lexical_tokens)
        .unwrap_or_default();

    SymbolDocument {
        symbol_id: info.id.clone(),
        name: info.name.clone(),
        name_path: info.name_path.clone(),
        kind: kind_label(&info.kind).to_owned(),
        signature: info.signature.clone(),
        file_path: info.file_path.clone(),
        module_path: module_path_from_file(&info.file_path),
        doc_comment: String::new(),
        body_lexical_chunk,
        language: language_from_path(&info.file_path),
        line_start: info.line,
        is_test: detect_is_test(&info.file_path),
        is_generated: detect_is_generated(&info.file_path),
        exported: detect_exported(&info.signature, &info.kind),
    }
}

/// Extract identifier-like tokens from a symbol body. Keeps only
/// lexical tokens (letters, digits, underscore) of length ≥
/// `MIN_TOKEN_LEN`, unique, and capped at `BODY_TOKEN_CAP` to prevent
/// one long function from dominating BM25F doc length.
fn extract_lexical_tokens(body: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut current = String::new();
    for ch in body.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            flush_token(&mut current, &mut seen, &mut tokens);
            if tokens.len() >= BODY_TOKEN_CAP {
                return tokens.join(" ");
            }
        }
    }
    flush_token(&mut current, &mut seen, &mut tokens);
    if tokens.len() > BODY_TOKEN_CAP {
        tokens.truncate(BODY_TOKEN_CAP);
    }
    tokens.join(" ")
}

fn flush_token(
    current: &mut String,
    seen: &mut std::collections::HashSet<String>,
    tokens: &mut Vec<String>,
) {
    if current.is_empty() {
        return;
    }
    if current.len() >= MIN_TOKEN_LEN {
        let lowered = current.to_ascii_lowercase();
        if seen.insert(lowered.clone()) {
            tokens.push(lowered);
        }
    }
    current.clear();
}

fn kind_label(kind: &SymbolKind) -> &'static str {
    kind.as_label()
}

fn language_from_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        "rust"
    } else if lower.ends_with(".ts") || lower.ends_with(".tsx") {
        "typescript"
    } else if lower.ends_with(".js") || lower.ends_with(".jsx") {
        "javascript"
    } else if lower.ends_with(".py") {
        "python"
    } else if lower.ends_with(".go") {
        "go"
    } else if lower.ends_with(".java") {
        "java"
    } else if lower.ends_with(".rb") {
        "ruby"
    } else if lower.ends_with(".swift") {
        "swift"
    } else if lower.ends_with(".kt") || lower.ends_with(".kts") {
        "kotlin"
    } else if lower.ends_with(".cpp") || lower.ends_with(".cc") || lower.ends_with(".hpp") {
        "cpp"
    } else if lower.ends_with(".c") || lower.ends_with(".h") {
        "c"
    } else {
        "other"
    }
}

/// Convert `crates/codelens-mcp/src/dispatch/mod.rs` into
/// `dispatch::mod` — strips leading `crates/<name>/src/` prefix, drops
/// the extension, and replaces path separators with `::`.
fn module_path_from_file(file_path: &str) -> String {
    let without_ext = match file_path.rfind('.') {
        Some(idx) => &file_path[..idx],
        None => file_path,
    };
    let stripped = match without_ext.find("/src/") {
        Some(idx) => &without_ext[idx + "/src/".len()..],
        None => without_ext,
    };
    stripped.replace('/', "::")
}

fn detect_is_test(file_path: &str) -> bool {
    let lower = file_path.to_ascii_lowercase();
    lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.starts_with("tests/")
        || lower.starts_with("test/")
        || lower.ends_with("_test.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with("_spec.rb")
        || lower.ends_with("_test.go")
        || lower.contains("/integration_tests/")
}

fn detect_is_generated(file_path: &str) -> bool {
    let lower = file_path.to_ascii_lowercase();
    lower.contains("/generated/")
        || lower.contains(".generated.")
        || lower.contains("/build/")
        || lower.contains("/target/")
        || lower.ends_with(".g.rs")
        || lower.ends_with(".pb.rs")
        || lower.ends_with(".pb.go")
}

fn detect_exported(signature: &str, kind: &SymbolKind) -> bool {
    let trimmed = signature.trim_start();
    if trimmed.starts_with("pub ")
        || trimmed.starts_with("pub(")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("export default")
    {
        return true;
    }
    // Python: a leading underscore on name means private by convention,
    // but we don't have access to `name` here — that caller handles.
    // Java/Kotlin: `public` keyword.
    if trimmed.starts_with("public ") {
        return true;
    }
    // Type/trait declarations are usually exported when visible in the
    // index at all — keep a small allowlist so the flag is informative.
    matches!(
        kind,
        SymbolKind::Interface | SymbolKind::TypeAlias | SymbolKind::Enum
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let doc = from_symbol_info(&symbol);
        assert_eq!(doc.symbol_id, symbol.id);
        assert_eq!(doc.name, "evaluate_mutation_gate");
        assert_eq!(doc.name_path, "mutation_gate::evaluate_mutation_gate");
        assert_eq!(doc.kind, "function");
        assert_eq!(doc.module_path, "mutation_gate");
        assert_eq!(doc.language, "rust");
        assert!(doc.exported, "pub fn should be flagged exported");
        assert!(!doc.is_test);
        assert!(!doc.is_generated);
        assert!(doc.body_lexical_chunk.contains("state"));
        assert!(doc.body_lexical_chunk.contains("foo"));
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
}
