use super::*;
use crate::db::{IndexDb, NewSymbol};
use std::sync::Mutex;

/// Serialize tests that load the fastembed ONNX model to avoid file lock contention.
static MODEL_LOCK: Mutex<()> = Mutex::new(());

/// Serialize tests that mutate `CODELENS_EMBED_HINT_*` env vars.
/// The v1.6.0 default flip (§8.14) exposed a pre-existing race where
/// parallel env-var mutating tests interfere with each other — the
/// old `unwrap_or(false)` default happened to mask the race most of
/// the time, but `unwrap_or(true)` no longer does. All tests that
/// read or mutate `CODELENS_EMBED_HINT_*` should take this lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());

macro_rules! skip_without_embedding_model {
    () => {
        if !super::embedding_model_assets_available() {
            eprintln!("skipping embedding test: CodeSearchNet model assets unavailable");
            return;
        }
    };
}

/// Helper: create a temp project with seeded symbols.
fn make_project_with_source() -> (tempfile::TempDir, ProjectRoot) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Write a source file so body extraction works
    let source = "def hello():\n    print('hi')\n\ndef world():\n    return 42\n";
    write_python_file_with_symbols(
        root,
        "main.py",
        source,
        "hash1",
        &[
            ("hello", "def hello():", "hello"),
            ("world", "def world():", "world"),
        ],
    );

    let project = ProjectRoot::new_exact(root).unwrap();
    (dir, project)
}

fn write_python_file_with_symbols(
    root: &std::path::Path,
    relative_path: &str,
    source: &str,
    hash: &str,
    symbols: &[(&str, &str, &str)],
) {
    std::fs::write(root.join(relative_path), source).unwrap();
    let db_path = crate::db::index_db_path(root);
    let db = IndexDb::open(&db_path).unwrap();
    let file_id = db
        .upsert_file(relative_path, 100, hash, source.len() as i64, Some("py"))
        .unwrap();

    let new_symbols: Vec<NewSymbol<'_>> = symbols
        .iter()
        .map(|(name, signature, name_path)| {
            let start = source.find(signature).unwrap() as i64;
            let end = source[start as usize..]
                .find("\n\ndef ")
                .map(|offset| start + offset as i64)
                .unwrap_or(source.len() as i64);
            let line = source[..start as usize]
                .bytes()
                .filter(|&b| b == b'\n')
                .count() as i64
                + 1;
            NewSymbol {
                name,
                kind: "function",
                line,
                column_num: 0,
                start_byte: start,
                end_byte: end,
                signature,
                name_path,
                parent_id: None,
            }
        })
        .collect();
    db.insert_symbols(file_id, &new_symbols).unwrap();
}

fn replace_file_embeddings_with_sentinels(
    engine: &EmbeddingEngine,
    file_path: &str,
    sentinels: &[(&str, f32)],
) {
    let mut chunks = engine.store.embeddings_for_files(&[file_path]).unwrap();
    for chunk in &mut chunks {
        if let Some((_, value)) = sentinels
            .iter()
            .find(|(symbol_name, _)| *symbol_name == chunk.symbol_name)
        {
            chunk.embedding = vec![*value; chunk.embedding.len()];
        }
    }
    engine.store.delete_by_file(&[file_path]).unwrap();
    engine.store.insert(&chunks).unwrap();
}

fn write_minimal_model_assets(model_dir: &std::path::Path) {
    std::fs::create_dir_all(model_dir).unwrap();
    for asset in [
        "model.onnx",
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ] {
        std::fs::write(model_dir.join(asset), b"{}").unwrap();
    }
}

#[test]
fn build_embedding_text_with_signature() {
    let sym = crate::db::SymbolWithFile {
        name: "hello".into(),
        kind: "function".into(),
        file_path: "main.py".into(),
        line: 1,
        signature: "def hello():".into(),
        name_path: "hello".into(),
        start_byte: 0,
        end_byte: 10,
    };
    let text = build_embedding_text(&sym, Some("def hello(): pass"));
    assert_eq!(text, "function hello in main.py: def hello():");
}

#[test]
fn build_embedding_text_without_source() {
    let sym = crate::db::SymbolWithFile {
        name: "MyClass".into(),
        kind: "class".into(),
        file_path: "app.py".into(),
        line: 5,
        signature: "class MyClass:".into(),
        name_path: "MyClass".into(),
        start_byte: 0,
        end_byte: 50,
    };
    let text = build_embedding_text(&sym, None);
    assert_eq!(text, "class MyClass (My Class) in app.py: class MyClass:");
}

#[test]
fn build_embedding_text_empty_signature() {
    let sym = crate::db::SymbolWithFile {
        name: "CONFIG".into(),
        kind: "variable".into(),
        file_path: "config.py".into(),
        line: 1,
        signature: String::new(),
        name_path: "CONFIG".into(),
        start_byte: 0,
        end_byte: 0,
    };
    let text = build_embedding_text(&sym, None);
    assert_eq!(text, "variable CONFIG in config.py");
}

#[test]
fn filters_direct_test_symbols_from_embedding_index() {
    let source = "#[test]\nfn alias_case() {}\n";
    let sym = crate::db::SymbolWithFile {
        name: "alias_case".into(),
        kind: "function".into(),
        file_path: "src/lib.rs".into(),
        line: 2,
        signature: "fn alias_case() {}".into(),
        name_path: "alias_case".into(),
        start_byte: source.find("fn alias_case").unwrap() as i64,
        end_byte: source.len() as i64,
    };

    assert!(is_test_only_symbol(&sym, Some(source)));
}

#[test]
fn filters_cfg_test_module_symbols_from_embedding_index() {
    let source = "#[cfg(all(test, feature = \"semantic\"))]\nmod semantic_tests {\n    fn helper_case() {}\n}\n";
    let sym = crate::db::SymbolWithFile {
        name: "helper_case".into(),
        kind: "function".into(),
        file_path: "src/lib.rs".into(),
        line: 3,
        signature: "fn helper_case() {}".into(),
        name_path: "helper_case".into(),
        start_byte: source.find("fn helper_case").unwrap() as i64,
        end_byte: source.len() as i64,
    };

    assert!(is_test_only_symbol(&sym, Some(source)));
}

#[test]
fn extract_python_docstring() {
    let source =
        "def greet(name):\n    \"\"\"Say hello to a person.\"\"\"\n    print(f'hi {name}')\n";
    let doc = extract_leading_doc(source, 0, source.len()).unwrap();
    assert!(doc.contains("Say hello to a person"));
}

#[test]
fn extract_rust_doc_comment() {
    let source = "fn dispatch_tool() {\n    /// Route incoming tool requests.\n    /// Handles all MCP methods.\n    let x = 1;\n}\n";
    let doc = extract_leading_doc(source, 0, source.len()).unwrap();
    assert!(doc.contains("Route incoming tool requests"));
    assert!(doc.contains("Handles all MCP methods"));
}

#[test]
fn extract_leading_doc_returns_none_for_no_doc() {
    let source = "def f():\n    return 1\n";
    assert!(extract_leading_doc(source, 0, source.len()).is_none());
}

#[test]
fn extract_body_hint_finds_first_meaningful_line() {
    let source = "pub fn parse_symbols(\n    project: &ProjectRoot,\n) -> Vec<SymbolInfo> {\n    let mut parser = tree_sitter::Parser::new();\n    parser.set_language(lang);\n}\n";
    let hint = extract_body_hint(source, 0, source.len());
    assert!(hint.is_some());
    assert!(hint.unwrap().contains("tree_sitter::Parser"));
}

#[test]
fn extract_body_hint_skips_comments() {
    let source = "fn foo() {\n    // setup\n    let x = bar();\n}\n";
    let hint = extract_body_hint(source, 0, source.len());
    assert_eq!(hint.unwrap(), "let x = bar();");
}

#[test]
fn extract_body_hint_returns_none_for_empty() {
    let source = "fn empty() {\n}\n";
    let hint = extract_body_hint(source, 0, source.len());
    assert!(hint.is_none());
}

#[test]
fn extract_body_hint_multi_line_collection_via_env_override() {
    // Default is 1 line / 60 chars (v1.4.0 parity after the v1.5 Phase 2
    // PoC revert). Override the line budget via env to confirm the
    // multi-line path still works — this is the knob future experiments
    // will use without recompiling.
    let previous_lines = std::env::var("CODELENS_EMBED_HINT_LINES").ok();
    let previous_chars = std::env::var("CODELENS_EMBED_HINT_CHARS").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_LINES", "3");
        std::env::set_var("CODELENS_EMBED_HINT_CHARS", "200");
    }

    let source = "\
fn route_request() {
    let kind = detect_request_kind();
    let target = dispatch_table.get(&kind);
    return target.handle();
}
";
    let hint = extract_body_hint(source, 0, source.len()).expect("hint present");

    let env_restore = || unsafe {
        match &previous_lines {
            Some(value) => std::env::set_var("CODELENS_EMBED_HINT_LINES", value),
            None => std::env::remove_var("CODELENS_EMBED_HINT_LINES"),
        }
        match &previous_chars {
            Some(value) => std::env::set_var("CODELENS_EMBED_HINT_CHARS", value),
            None => std::env::remove_var("CODELENS_EMBED_HINT_CHARS"),
        }
    };

    let all_three = hint.contains("detect_request_kind")
        && hint.contains("dispatch_table")
        && hint.contains("target.handle");
    let has_separator = hint.contains(" · ");
    env_restore();

    assert!(all_three, "missing one of three body lines: {hint}");
    assert!(has_separator, "missing · separator: {hint}");
}

// Note: we intentionally do NOT have a test that verifies the "default"
// 60-char / 1-line behaviour via `extract_body_hint`. Such a test is
// flaky because cargo test runs tests in parallel and the env-overriding
// tests below (`CODELENS_EMBED_HINT_CHARS`, `CODELENS_EMBED_HINT_LINES`)
// can leak their variables into this one. The default constants
// themselves are compile-time checked and covered by
// `extract_body_hint_finds_first_meaningful_line` /
// `extract_body_hint_skips_comments` which assert on the exact single-line
// shape and implicitly depend on the default budget.

#[test]
fn hint_line_budget_respects_env_override() {
    // SAFETY: test block is serialized by crate-level test harness; we
    // restore the variable on exit.
    let previous = std::env::var("CODELENS_EMBED_HINT_LINES").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_LINES", "5");
    }
    let budget = super::hint_line_budget();
    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_HINT_LINES", value),
            None => std::env::remove_var("CODELENS_EMBED_HINT_LINES"),
        }
    }
    assert_eq!(budget, 5);
}

#[test]
fn is_nl_shaped_accepts_multi_word_prose() {
    assert!(super::is_nl_shaped("skip comments and string literals"));
    assert!(super::is_nl_shaped("failed to open database"));
    assert!(super::is_nl_shaped("detect client version"));
}

#[test]
fn is_nl_shaped_rejects_code_and_paths() {
    // Path-like tokens (both slash flavors)
    assert!(!super::is_nl_shaped("crates/codelens-engine/src"));
    assert!(!super::is_nl_shaped("C:\\Users\\foo"));
    // Module-path-like
    assert!(!super::is_nl_shaped("std::sync::Mutex"));
    // Single-word identifier
    assert!(!super::is_nl_shaped("detect_client"));
    // Too short
    assert!(!super::is_nl_shaped("ok"));
    assert!(!super::is_nl_shaped(""));
    // High non-alphabetic ratio
    assert!(!super::is_nl_shaped("1 2 3 4 5"));
}

#[test]
fn extract_comment_body_strips_comment_markers() {
    assert_eq!(
        super::extract_comment_body("/// rust doc comment"),
        Some("rust doc comment".to_string())
    );
    assert_eq!(
        super::extract_comment_body("// regular line comment"),
        Some("regular line comment".to_string())
    );
    assert_eq!(
        super::extract_comment_body("# python line comment"),
        Some("python line comment".to_string())
    );
    assert_eq!(
        super::extract_comment_body("/* inline block */"),
        Some("inline block".to_string())
    );
    assert_eq!(
        super::extract_comment_body("* continuation line"),
        Some("continuation line".to_string())
    );
}

#[test]
fn extract_comment_body_rejects_rust_attributes_and_shebangs() {
    assert!(super::extract_comment_body("#[derive(Debug)]").is_none());
    assert!(super::extract_comment_body("#[test]").is_none());
    assert!(super::extract_comment_body("#!/usr/bin/env python").is_none());
}

#[test]
fn extract_nl_tokens_gated_off_by_default() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Default: no env, no NL tokens regardless of body content.
    let previous = std::env::var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS").ok();
    unsafe {
        std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS");
    }
    let source = "\
fn skip_things() {
    // skip comments and string literals during search
    let lit = \"scan for matching tokens\";
}
";
    let result = extract_nl_tokens(source, 0, source.len());
    unsafe {
        if let Some(value) = previous {
            std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", value);
        }
    }
    assert!(result.is_none(), "gate leaked: {result:?}");
}

#[test]
fn auto_hint_mode_defaults_on_unless_explicit_off() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // v1.6.0 flip (§8.14): default-ON semantics.
    //
    // Case 1: env var unset → default ON (the v1.6.0 flip).
    // Case 2: env var="0" (or "false"/"no"/"off") → explicit OFF
    //   (opt-out preserved).
    // Case 3: env var="1" (or "true"/"yes"/"on") → explicit ON
    //   (still works — explicit always wins).
    let previous = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();

    // Case 1: unset → ON (flip)
    unsafe {
        std::env::remove_var("CODELENS_EMBED_HINT_AUTO");
    }
    let default_enabled = super::auto_hint_mode_enabled();
    assert!(
        default_enabled,
        "v1.6.0 default flip: auto hint mode should be ON when env unset"
    );

    // Case 2: explicit OFF
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "0");
    }
    let explicit_off = super::auto_hint_mode_enabled();
    assert!(
        !explicit_off,
        "explicit CODELENS_EMBED_HINT_AUTO=0 must still disable (opt-out escape hatch)"
    );

    // Case 3: explicit ON
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
    }
    let explicit_on = super::auto_hint_mode_enabled();
    assert!(
        explicit_on,
        "explicit CODELENS_EMBED_HINT_AUTO=1 must still enable"
    );

    // Restore
    unsafe {
        match previous {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
        }
    }
}

#[test]
fn language_supports_nl_stack_classifies_correctly() {
    // Supported — measured or static-typed analogue
    assert!(super::language_supports_nl_stack("rs"));
    assert!(super::language_supports_nl_stack("rust"));
    assert!(super::language_supports_nl_stack("cpp"));
    assert!(super::language_supports_nl_stack("c++"));
    assert!(super::language_supports_nl_stack("c"));
    assert!(super::language_supports_nl_stack("go"));
    assert!(super::language_supports_nl_stack("golang"));
    assert!(super::language_supports_nl_stack("java"));
    assert!(super::language_supports_nl_stack("kt"));
    assert!(super::language_supports_nl_stack("kotlin"));
    assert!(super::language_supports_nl_stack("scala"));
    assert!(super::language_supports_nl_stack("cs"));
    assert!(super::language_supports_nl_stack("csharp"));
    // §8.13 Phase 3c: TypeScript / JavaScript added after
    // facebook/jest external-repo A/B (+7.3 % hybrid MRR).
    assert!(super::language_supports_nl_stack("ts"));
    assert!(super::language_supports_nl_stack("typescript"));
    assert!(super::language_supports_nl_stack("tsx"));
    assert!(super::language_supports_nl_stack("js"));
    assert!(super::language_supports_nl_stack("javascript"));
    assert!(super::language_supports_nl_stack("jsx"));
    // Case-insensitive
    assert!(super::language_supports_nl_stack("Rust"));
    assert!(super::language_supports_nl_stack("RUST"));
    assert!(super::language_supports_nl_stack("TypeScript"));
    // Leading/trailing whitespace is tolerated
    assert!(super::language_supports_nl_stack("  rust  "));
    assert!(super::language_supports_nl_stack("  ts  "));

    // Unsupported — measured regression or untested dynamic
    assert!(!super::language_supports_nl_stack("py"));
    assert!(!super::language_supports_nl_stack("python"));
    assert!(!super::language_supports_nl_stack("rb"));
    assert!(!super::language_supports_nl_stack("ruby"));
    assert!(!super::language_supports_nl_stack("php"));
    assert!(!super::language_supports_nl_stack("lua"));
    assert!(!super::language_supports_nl_stack("sh"));
    // Unknown defaults to unsupported
    assert!(!super::language_supports_nl_stack("klingon"));
    assert!(!super::language_supports_nl_stack(""));
}

#[test]
fn language_supports_sparse_weighting_classifies_correctly() {
    assert!(super::language_supports_sparse_weighting("rs"));
    assert!(super::language_supports_sparse_weighting("rust"));
    assert!(super::language_supports_sparse_weighting("cpp"));
    assert!(super::language_supports_sparse_weighting("go"));
    assert!(super::language_supports_sparse_weighting("java"));
    assert!(super::language_supports_sparse_weighting("kotlin"));
    assert!(super::language_supports_sparse_weighting("csharp"));

    assert!(!super::language_supports_sparse_weighting("ts"));
    assert!(!super::language_supports_sparse_weighting("typescript"));
    assert!(!super::language_supports_sparse_weighting("tsx"));
    assert!(!super::language_supports_sparse_weighting("js"));
    assert!(!super::language_supports_sparse_weighting("javascript"));
    assert!(!super::language_supports_sparse_weighting("jsx"));
    assert!(!super::language_supports_sparse_weighting("py"));
    assert!(!super::language_supports_sparse_weighting("python"));
    assert!(!super::language_supports_sparse_weighting("klingon"));
    assert!(!super::language_supports_sparse_weighting(""));
}

#[test]
fn auto_hint_should_enable_requires_both_gate_and_supported_lang() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
    let prev_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();

    // Case 1: gate explicitly off → never enable, regardless of language.
    // v1.6.0 flip (§8.14): `unset` now means default-ON, so to test
    // "gate off" we must set the env var to an explicit "0".
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "0");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
    }
    assert!(
        !super::auto_hint_should_enable(),
        "gate-off (explicit =0) with lang=rust must stay disabled"
    );

    // Case 2: gate on, supported language → enable
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
    }
    assert!(
        super::auto_hint_should_enable(),
        "gate-on + lang=rust must enable"
    );

    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "typescript");
    }
    assert!(
        super::auto_hint_should_enable(),
        "gate-on + lang=typescript must keep Phase 2b/2c enabled"
    );

    // Case 3: gate on, unsupported language → disable
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
    }
    assert!(
        !super::auto_hint_should_enable(),
        "gate-on + lang=python must stay disabled"
    );

    // Case 4: gate on, no language tag → conservative disable
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG");
    }
    assert!(
        !super::auto_hint_should_enable(),
        "gate-on + no lang tag must stay disabled"
    );

    // Restore
    unsafe {
        match prev_auto {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
        }
        match prev_lang {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
        }
    }
}

#[test]
fn auto_sparse_should_enable_requires_both_gate_and_sparse_supported_lang() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
    let prev_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();

    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "0");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
    }
    assert!(
        !super::auto_sparse_should_enable(),
        "gate-off (explicit =0) must disable sparse auto gate"
    );

    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
    }
    assert!(
        super::auto_sparse_should_enable(),
        "gate-on + lang=rust must enable sparse auto gate"
    );

    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "typescript");
    }
    assert!(
        !super::auto_sparse_should_enable(),
        "gate-on + lang=typescript must keep sparse auto gate disabled"
    );

    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
    }
    assert!(
        !super::auto_sparse_should_enable(),
        "gate-on + lang=python must keep sparse auto gate disabled"
    );

    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG");
    }
    assert!(
        !super::auto_sparse_should_enable(),
        "gate-on + no lang tag must keep sparse auto gate disabled"
    );

    unsafe {
        match prev_auto {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
        }
        match prev_lang {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
        }
    }
}

#[test]
fn nl_tokens_enabled_explicit_env_wins_over_auto() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev_explicit = std::env::var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS").ok();
    let prev_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
    let prev_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();

    // Explicit ON beats auto-OFF-for-python
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
    }
    assert!(
        super::nl_tokens_enabled(),
        "explicit=1 must win over auto+python=off"
    );

    // Explicit OFF beats auto-ON-for-rust
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", "0");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
    }
    assert!(
        !super::nl_tokens_enabled(),
        "explicit=0 must win over auto+rust=on"
    );

    // No explicit, auto+rust → on via fallback
    unsafe {
        std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
    }
    assert!(
        super::nl_tokens_enabled(),
        "no explicit + auto+rust must enable"
    );

    // No explicit, auto+python → off via fallback
    unsafe {
        std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
    }
    assert!(
        !super::nl_tokens_enabled(),
        "no explicit + auto+python must stay disabled"
    );

    // Restore
    unsafe {
        match prev_explicit {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS"),
        }
        match prev_auto {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
        }
        match prev_lang {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
        }
    }
}

#[test]
fn strict_comments_gated_off_by_default() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS").ok();
    unsafe {
        std::env::remove_var("CODELENS_EMBED_HINT_STRICT_COMMENTS");
    }
    let enabled = super::strict_comments_enabled();
    unsafe {
        if let Some(value) = previous {
            std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", value);
        }
    }
    assert!(!enabled, "strict comments gate leaked");
}

#[test]
fn looks_like_meta_annotation_detects_rejected_prefixes() {
    // All case variants of the rejected prefix list must match.
    assert!(super::looks_like_meta_annotation("TODO: fix later"));
    assert!(super::looks_like_meta_annotation("todo handle edge case"));
    assert!(super::looks_like_meta_annotation("FIXME this is broken"));
    assert!(super::looks_like_meta_annotation(
        "HACK: workaround for bug"
    ));
    assert!(super::looks_like_meta_annotation("XXX not implemented yet"));
    assert!(super::looks_like_meta_annotation(
        "BUG in the upstream crate"
    ));
    assert!(super::looks_like_meta_annotation("REVIEW before merging"));
    assert!(super::looks_like_meta_annotation(
        "REFACTOR this block later"
    ));
    assert!(super::looks_like_meta_annotation("TEMP: remove before v2"));
    assert!(super::looks_like_meta_annotation(
        "DEPRECATED use new_api instead"
    ));
    // Leading whitespace inside the comment body is handled.
    assert!(super::looks_like_meta_annotation(
        "   TODO: with leading ws"
    ));
}

#[test]
fn looks_like_meta_annotation_preserves_behaviour_prefixes() {
    // Explicitly-excluded prefixes — kept as behaviour signal.
    assert!(!super::looks_like_meta_annotation(
        "NOTE: this branch handles empty input"
    ));
    assert!(!super::looks_like_meta_annotation(
        "WARN: overflow is possible"
    ));
    assert!(!super::looks_like_meta_annotation(
        "SAFETY: caller must hold the lock"
    ));
    assert!(!super::looks_like_meta_annotation(
        "PANIC: unreachable by construction"
    ));
    // Behaviour-descriptive prose must pass through.
    assert!(!super::looks_like_meta_annotation(
        "parse json body from request"
    ));
    assert!(!super::looks_like_meta_annotation(
        "walk directory respecting gitignore"
    ));
    assert!(!super::looks_like_meta_annotation(
        "compute cosine similarity between vectors"
    ));
    // Empty / edge inputs
    assert!(!super::looks_like_meta_annotation(""));
    assert!(!super::looks_like_meta_annotation("   "));
    assert!(!super::looks_like_meta_annotation("123 numeric prefix"));
}

#[test]
fn strict_comments_filters_meta_annotations_during_extraction() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", "1");
    }
    let source = "\
fn handle_request() {
    // TODO: handle the error path properly
    // parse json body from the incoming request
    // FIXME: this can panic on empty input
    // walk directory respecting the gitignore rules
    let _x = 1;
}
";
    let result = super::extract_nl_tokens_inner(source, 0, source.len());
    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", value),
            None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_COMMENTS"),
        }
    }
    let hint = result.expect("behaviour comments must survive");
    // The first real behaviour comment must appear. The hint is capped
    // by the default 60-char budget, so we only assert on a short
    // substring that's guaranteed to fit.
    assert!(
        hint.contains("parse json body"),
        "behaviour comment dropped: {hint}"
    );
    // TODO / FIXME must NOT appear anywhere in the hint (they were
    // rejected before join, so they cannot be there even partially).
    assert!(!hint.contains("TODO"), "TODO annotation leaked: {hint}");
    assert!(!hint.contains("FIXME"), "FIXME annotation leaked: {hint}");
}

#[test]
fn strict_comments_is_orthogonal_to_strict_literals() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Enabling strict_comments must NOT affect the Pass-2 literal path.
    // A format-specifier literal should still pass through Pass 2
    // when the literal filter is off, regardless of the comment gate.
    let prev_c = std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS").ok();
    let prev_l = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", "1");
        std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS");
    }
    // Source kept short so the 60-char hint budget does not truncate
    // either of the two substrings we assert on.
    let source = "\
fn handle() {
    // handles real behaviour
    let fmt = \"format error string\";
}
";
    let result = super::extract_nl_tokens_inner(source, 0, source.len());
    unsafe {
        match prev_c {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_COMMENTS"),
        }
        match prev_l {
            Some(v) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", v),
            None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS"),
        }
    }
    let hint = result.expect("tokens must exist");
    // Comment survives (not a meta-annotation).
    assert!(hint.contains("handles real"), "comment dropped: {hint}");
    // String literal still appears — strict_literals was OFF, so the
    // Pass-2 filter is inactive for this test.
    assert!(
        hint.contains("format error string"),
        "literal dropped: {hint}"
    );
}

#[test]
fn strict_literal_filter_gated_off_by_default() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
    unsafe {
        std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS");
    }
    let enabled = super::strict_literal_filter_enabled();
    unsafe {
        if let Some(value) = previous {
            std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", value);
        }
    }
    assert!(!enabled, "strict literal filter gate leaked");
}

#[test]
fn contains_format_specifier_detects_c_and_python_style() {
    // C / Python `%` style
    assert!(super::contains_format_specifier("Invalid URL %s"));
    assert!(super::contains_format_specifier("got %d matches"));
    assert!(super::contains_format_specifier("value=%r"));
    assert!(super::contains_format_specifier("size=%f"));
    // Python `.format` / f-string / Rust `format!` style
    assert!(super::contains_format_specifier("sending request to {url}"));
    assert!(super::contains_format_specifier("got {0} items"));
    assert!(super::contains_format_specifier("{:?}"));
    assert!(super::contains_format_specifier("value: {x:.2f}"));
    assert!(super::contains_format_specifier("{}"));
    // Plain prose with no format specifier
    assert!(!super::contains_format_specifier(
        "skip comments and string literals"
    ));
    assert!(!super::contains_format_specifier("failed to open database"));
    // JSON-like brace content should not count as a format specifier
    // (multi-word content inside braces)
    assert!(!super::contains_format_specifier("{name: foo, id: 1}"));
}

#[test]
fn looks_like_error_or_log_prefix_rejects_common_patterns() {
    assert!(super::looks_like_error_or_log_prefix("Invalid URL format"));
    assert!(super::looks_like_error_or_log_prefix(
        "Cannot decode response"
    ));
    assert!(super::looks_like_error_or_log_prefix("could not open file"));
    assert!(super::looks_like_error_or_log_prefix(
        "Failed to send request"
    ));
    assert!(super::looks_like_error_or_log_prefix(
        "Expected int, got str"
    ));
    assert!(super::looks_like_error_or_log_prefix(
        "sending request to server"
    ));
    assert!(super::looks_like_error_or_log_prefix(
        "received response headers"
    ));
    assert!(super::looks_like_error_or_log_prefix(
        "starting worker pool"
    ));
    // Real behaviour strings must pass
    assert!(!super::looks_like_error_or_log_prefix(
        "parse json body from request"
    ));
    assert!(!super::looks_like_error_or_log_prefix(
        "compute cosine similarity between vectors"
    ));
    assert!(!super::looks_like_error_or_log_prefix(
        "walk directory tree respecting gitignore"
    ));
}

#[test]
fn strict_mode_rejects_format_and_error_literals_during_extraction() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // The env gate is bypassed by calling the inner function directly,
    // BUT the inner function still reads the strict-literal env var.
    // So we have to set it explicitly for this test.
    let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", "1");
    }
    let source = "\
fn handle_request() {
    let err = \"Invalid URL %s\";
    let log = \"sending request to the upstream server\";
    let fmt = \"received {count} items in batch\";
    let real = \"parse json body from the incoming request\";
}
";
    let result = super::extract_nl_tokens_inner(source, 0, source.len());
    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", value),
            None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS"),
        }
    }
    let hint = result.expect("some token should survive");
    // The one real behaviour-descriptive literal must land in the hint.
    assert!(
        hint.contains("parse json body"),
        "real literal was filtered out: {hint}"
    );
    // None of the three low-value literals should appear.
    assert!(
        !hint.contains("Invalid URL"),
        "format-specifier literal leaked: {hint}"
    );
    assert!(
        !hint.contains("sending request"),
        "log-prefix literal leaked: {hint}"
    );
    assert!(
        !hint.contains("received {count}"),
        "python fstring literal leaked: {hint}"
    );
}

#[test]
fn strict_mode_leaves_comments_untouched() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Comments (Pass 1) should NOT be filtered by the strict flag —
    // the §8.8 post-mortem identified string literals as the
    // regression source, not comments.
    let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", "1");
    }
    let source = "\
fn do_work() {
    // Invalid inputs are rejected by this guard clause.
    // sending requests in parallel across worker threads.
    let _lit = \"format spec %s\";
}
";
    let result = super::extract_nl_tokens_inner(source, 0, source.len());
    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", value),
            None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS"),
        }
    }
    let hint = result.expect("comments should survive strict mode");
    // Both comments should land in the hint even though they start with
    // error/log-style prefixes — the filter only touches string literals.
    assert!(
        hint.contains("Invalid inputs") || hint.contains("rejected by this guard"),
        "strict mode swallowed a comment: {hint}"
    );
    // And the low-value string literal should NOT be in the hint.
    assert!(
        !hint.contains("format spec"),
        "format-specifier literal leaked under strict mode: {hint}"
    );
}

#[test]
fn should_reject_literal_strict_composes_format_and_prefix() {
    // The test-only helper must mirror the production filter logic:
    // a literal is rejected iff it is a format specifier OR an error/log
    // prefix (the production filter uses exactly this disjunction).
    assert!(super::should_reject_literal_strict("Invalid URL %s"));
    assert!(super::should_reject_literal_strict(
        "sending request to server"
    ));
    assert!(super::should_reject_literal_strict("value: {x:.2f}"));
    // Real behaviour strings pass through.
    assert!(!super::should_reject_literal_strict(
        "parse json body from the incoming request"
    ));
    assert!(!super::should_reject_literal_strict(
        "compute cosine similarity between vectors"
    ));
}

#[test]
fn is_static_method_ident_accepts_pascal_and_rejects_snake() {
    assert!(super::is_static_method_ident("HashMap"));
    assert!(super::is_static_method_ident("Parser"));
    assert!(super::is_static_method_ident("A"));
    // snake_case / module-like — the filter must reject these so
    // `std::fs::read_to_string` does not leak into API hints.
    assert!(!super::is_static_method_ident("std"));
    assert!(!super::is_static_method_ident("fs"));
    assert!(!super::is_static_method_ident("_private"));
    assert!(!super::is_static_method_ident(""));
}

#[test]
fn extract_api_calls_gated_off_by_default() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Default: no env, no API-call hint regardless of body content.
    let previous = std::env::var("CODELENS_EMBED_HINT_INCLUDE_API_CALLS").ok();
    unsafe {
        std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_API_CALLS");
    }
    let source = "\
fn make_parser() {
    let p = Parser::new();
    let _ = HashMap::with_capacity(8);
}
";
    let result = extract_api_calls(source, 0, source.len());
    unsafe {
        if let Some(value) = previous {
            std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_API_CALLS", value);
        }
    }
    assert!(result.is_none(), "gate leaked: {result:?}");
}

#[test]
fn extract_api_calls_captures_type_method_patterns() {
    // Uses the env-independent inner to avoid racing with other tests.
    let source = "\
fn open_db() {
    let p = Parser::new();
    let map = HashMap::with_capacity(16);
    let _ = tree_sitter::Parser::new();
}
";
    let hint = super::extract_api_calls_inner(source, 0, source.len())
        .expect("api calls should be produced");
    assert!(hint.contains("Parser::new"), "missing Parser::new: {hint}");
    assert!(
        hint.contains("HashMap::with_capacity"),
        "missing HashMap::with_capacity: {hint}"
    );
}

#[test]
fn extract_api_calls_rejects_module_prefixed_free_functions() {
    // Pure module paths must not surface as Type hints — the whole
    // point of `is_static_method_ident` is to drop these.
    let source = "\
fn read_config() {
    let _ = std::fs::read_to_string(\"foo\");
    let _ = crate::util::parse();
}
";
    let hint = super::extract_api_calls_inner(source, 0, source.len());
    // If any API hint is produced, it must not contain the snake_case
    // module prefixes; otherwise `None` is acceptable too.
    if let Some(hint) = hint {
        assert!(!hint.contains("std::fs"), "lowercase module leaked: {hint}");
        assert!(
            !hint.contains("fs::read_to_string"),
            "module-prefixed free function leaked: {hint}"
        );
        assert!(!hint.contains("crate::util"), "crate path leaked: {hint}");
    }
}

#[test]
fn extract_api_calls_deduplicates_repeated_calls() {
    let source = "\
fn hot_loop() {
    for _ in 0..10 {
        let _ = Parser::new();
        let _ = Parser::new();
    }
    let _ = Parser::new();
}
";
    let hint = super::extract_api_calls_inner(source, 0, source.len())
        .expect("api calls should be produced");
    let first = hint.find("Parser::new").expect("hit");
    let rest = &hint[first + "Parser::new".len()..];
    assert!(
        !rest.contains("Parser::new"),
        "duplicate not deduplicated: {hint}"
    );
}

#[test]
fn extract_api_calls_returns_none_when_body_has_no_type_calls() {
    let source = "\
fn plain() {
    let x = 1;
    let y = x + 2;
}
";
    assert!(super::extract_api_calls_inner(source, 0, source.len()).is_none());
}

#[test]
fn extract_nl_tokens_collects_comments_and_string_literals() {
    // Calls the env-independent inner to avoid racing with other tests
    // that mutate `CODELENS_EMBED_HINT_INCLUDE_COMMENTS`. The gate is
    // covered by `extract_nl_tokens_gated_off_by_default` above.
    let source = "\
fn search_for_matches() {
    // skip comments and string literals during search
    let error = \"failed to open database\";
    let single = \"tok\";
    let path = \"src/foo/bar\";
    let keyword = match kind {
        Kind::Ident => \"detect client version\",
        _ => \"\",
    };
}
";
    // Override the char budget locally so long hints are not truncated
    // before the assertions read them. We use the inner function which
    // still reads `CODELENS_EMBED_HINT_CHARS`, but we do NOT set it —
    // the default 60-char budget is enough for at least the first
    // discriminator to land in the output.
    let hint = super::extract_nl_tokens_inner(source, 0, source.len())
        .expect("nl tokens should be produced");
    // At least one NL-shaped token must land in the hint. The default
    // 60-char budget may truncate later ones; we assert on the first
    // few discriminators only.
    let has_first_nl_signal = hint.contains("skip comments")
        || hint.contains("failed to open")
        || hint.contains("detect client");
    assert!(has_first_nl_signal, "no NL signal produced: {hint}");
    // Short single-token literals must never leak in.
    assert!(!hint.contains(" tok "), "short literal leaked: {hint}");
    // Path literals must never leak in.
    assert!(!hint.contains("src/foo/bar"), "path literal leaked: {hint}");
}

#[test]
fn hint_char_budget_respects_env_override() {
    let previous = std::env::var("CODELENS_EMBED_HINT_CHARS").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_CHARS", "120");
    }
    let budget = super::hint_char_budget();
    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_HINT_CHARS", value),
            None => std::env::remove_var("CODELENS_EMBED_HINT_CHARS"),
        }
    }
    assert_eq!(budget, 120);
}

#[test]
fn embedding_to_bytes_roundtrip() {
    let floats = vec![1.0f32, -0.5, 0.0, 3.25];
    let bytes = embedding_to_bytes(&floats);
    assert_eq!(bytes.len(), 4 * 4);
    // Verify roundtrip
    let recovered: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    assert_eq!(floats, recovered);
}

#[test]
fn duplicate_pair_key_is_order_independent() {
    let a = duplicate_pair_key("a.py", "foo", "b.py", "bar");
    let b = duplicate_pair_key("b.py", "bar", "a.py", "foo");
    assert_eq!(a, b);
}

#[test]
fn text_embedding_cache_updates_recency() {
    let mut cache = TextEmbeddingCache::new(2);
    cache.insert("a".into(), vec![1.0]);
    cache.insert("b".into(), vec![2.0]);
    assert_eq!(cache.get("a"), Some(vec![1.0]));
    cache.insert("c".into(), vec![3.0]);

    assert_eq!(cache.get("a"), Some(vec![1.0]));
    assert_eq!(cache.get("b"), None);
    assert_eq!(cache.get("c"), Some(vec![3.0]));
}

#[test]
fn text_embedding_cache_can_be_disabled() {
    let mut cache = TextEmbeddingCache::new(0);
    cache.insert("a".into(), vec![1.0]);
    assert_eq!(cache.get("a"), None);
}

#[test]
fn query_embedding_cache_persists_and_prunes() {
    let _lock = ENV_LOCK.lock().unwrap();
    let previous = std::env::var("CODELENS_QUERY_EMBED_CACHE_SIZE").ok();
    unsafe {
        std::env::set_var("CODELENS_QUERY_EMBED_CACHE_SIZE", "2");
    }

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("embeddings.db");
    let store = SqliteVecStore::new(&db_path, 2, "model-a").unwrap();

    store
        .put_query_embedding("cache-v1:a", &[1.0, 1.5])
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    store
        .put_query_embedding("cache-v1:b", &[2.0, 2.5])
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    store
        .put_query_embedding("cache-v1:c", &[3.0, 3.5])
        .unwrap();

    assert_eq!(store.get_query_embedding("cache-v1:a").unwrap(), None);
    assert_eq!(
        store.get_query_embedding("cache-v1:b").unwrap(),
        Some(vec![2.0, 2.5])
    );
    assert_eq!(
        store.get_query_embedding("cache-v1:c").unwrap(),
        Some(vec![3.0, 3.5])
    );
    assert_eq!(store.query_cache_stats().unwrap().entries, 2);
    assert_eq!(store.query_cache_stats().unwrap().max_entries, 2);

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_QUERY_EMBED_CACHE_SIZE", value),
            None => std::env::remove_var("CODELENS_QUERY_EMBED_CACHE_SIZE"),
        }
    }
}

#[test]
fn engine_new_and_index() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).expect("engine should load");
    assert!(!engine.is_indexed());

    let count = engine.index_from_project(&project).unwrap();
    assert_eq!(count, 2, "should index 2 symbols");
    assert!(engine.is_indexed());
}

#[test]
fn engine_search_returns_results() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let results = engine.search("hello function", 10).unwrap();
    assert!(!results.is_empty(), "search should return results");
    for r in &results {
        assert!(
            r.score >= -1.0 && r.score <= 1.0,
            "score should be in [-1,1]: {}",
            r.score
        );
    }
}

#[test]
fn engine_incremental_index() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();
    assert_eq!(engine.store.count().unwrap(), 2);

    // Re-index only main.py — should replace its embeddings
    let count = engine.index_changed_files(&project, &["main.py"]).unwrap();
    assert_eq!(count, 2);
    assert_eq!(engine.store.count().unwrap(), 2);
}

#[test]
fn engine_reindex_preserves_symbol_count() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();
    assert_eq!(engine.store.count().unwrap(), 2);

    let count = engine.index_from_project(&project).unwrap();
    assert_eq!(count, 2);
    assert_eq!(engine.store.count().unwrap(), 2);
}

#[test]
fn full_reindex_reuses_unchanged_embeddings() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    replace_file_embeddings_with_sentinels(&engine, "main.py", &[("hello", 11.0), ("world", 22.0)]);

    let count = engine.index_from_project(&project).unwrap();
    assert_eq!(count, 2);

    let hello = engine
        .store
        .get_embedding("main.py", "hello")
        .unwrap()
        .expect("hello should exist");
    let world = engine
        .store
        .get_embedding("main.py", "world")
        .unwrap()
        .expect("world should exist");
    assert!(hello.embedding.iter().all(|value| *value == 11.0));
    assert!(world.embedding.iter().all(|value| *value == 22.0));
}

#[test]
fn full_reindex_reuses_unchanged_sibling_after_edit() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    replace_file_embeddings_with_sentinels(&engine, "main.py", &[("hello", 11.0), ("world", 22.0)]);

    let updated_source =
        "def hello():\n    print('hi')\n\ndef world(name):\n    return name.upper()\n";
    write_python_file_with_symbols(
        dir.path(),
        "main.py",
        updated_source,
        "hash2",
        &[
            ("hello", "def hello():", "hello"),
            ("world", "def world(name):", "world"),
        ],
    );

    let count = engine.index_from_project(&project).unwrap();
    assert_eq!(count, 2);

    let hello = engine
        .store
        .get_embedding("main.py", "hello")
        .unwrap()
        .expect("hello should exist");
    let world = engine
        .store
        .get_embedding("main.py", "world")
        .unwrap()
        .expect("world should exist");
    assert!(hello.embedding.iter().all(|value| *value == 11.0));
    assert!(world.embedding.iter().any(|value| *value != 22.0));
    assert_eq!(engine.store.count().unwrap(), 2);
}

#[test]
fn full_reindex_removes_deleted_files() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (dir, project) = make_project_with_source();
    write_python_file_with_symbols(
        dir.path(),
        "extra.py",
        "def bonus():\n    return 7\n",
        "hash-extra",
        &[("bonus", "def bonus():", "bonus")],
    );

    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();
    assert_eq!(engine.store.count().unwrap(), 3);

    std::fs::remove_file(dir.path().join("extra.py")).unwrap();
    let db_path = crate::db::index_db_path(dir.path());
    let db = IndexDb::open(&db_path).unwrap();
    db.delete_file("extra.py").unwrap();

    let count = engine.index_from_project(&project).unwrap();
    assert_eq!(count, 2);
    assert_eq!(engine.store.count().unwrap(), 2);
    assert!(
        engine
            .store
            .embeddings_for_files(&["extra.py"])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn engine_model_change_recreates_db() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();

    // First engine with default model
    let engine1 = EmbeddingEngine::new(&project).unwrap();
    engine1.index_from_project(&project).unwrap();
    assert_eq!(engine1.store.count().unwrap(), 2);
    drop(engine1);

    // Second engine with same model should preserve data
    let engine2 = EmbeddingEngine::new(&project).unwrap();
    assert!(engine2.store.count().unwrap() >= 2);
}

#[test]
fn inspect_existing_index_returns_model_and_count() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let info = EmbeddingEngine::inspect_existing_index(&project)
        .unwrap()
        .expect("index info should exist");
    assert_eq!(info.model_name, engine.model_name());
    assert_eq!(info.indexed_symbols, 2);
}

#[test]
fn inspect_existing_index_recovers_from_corrupt_db() {
    let (_dir, project) = make_project_with_source();
    let index_dir = project.as_path().join(".codelens/index");
    let db_path = index_dir.join("embeddings.db");
    let wal_path = index_dir.join("embeddings.db-wal");
    let shm_path = index_dir.join("embeddings.db-shm");

    std::fs::write(&db_path, b"not a sqlite database").unwrap();
    std::fs::write(&wal_path, b"bad wal").unwrap();
    std::fs::write(&shm_path, b"bad shm").unwrap();

    let info = EmbeddingEngine::inspect_existing_index(&project).unwrap();
    assert!(info.is_none());

    assert!(db_path.is_file());

    let backup_names: Vec<String> = std::fs::read_dir(&index_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".corrupt-"))
        .collect();

    assert!(
        backup_names
            .iter()
            .any(|name| name.starts_with("embeddings.db.corrupt-")),
        "expected quarantined embedding db, found {backup_names:?}"
    );
}

#[test]
fn store_can_fetch_single_embedding_without_loading_all() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let chunk = engine
        .store
        .get_embedding("main.py", "hello")
        .unwrap()
        .expect("embedding should exist");
    assert_eq!(chunk.file_path, "main.py");
    assert_eq!(chunk.symbol_name, "hello");
    assert!(!chunk.embedding.is_empty());
}

#[test]
fn find_similar_code_uses_index_and_excludes_target_symbol() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let matches = engine.find_similar_code("main.py", "hello", 5).unwrap();
    assert!(!matches.is_empty());
    assert!(
        matches
            .iter()
            .all(|m| !(m.file_path == "main.py" && m.symbol_name == "hello"))
    );
}

#[test]
fn delete_by_file_removes_rows_in_one_batch() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let deleted = engine.store.delete_by_file(&["main.py"]).unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(engine.store.count().unwrap(), 0);
}

#[test]
fn store_streams_embeddings_grouped_by_file() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let mut groups = Vec::new();
    engine
        .store
        .for_each_file_embeddings(&mut |file_path, chunks| {
            groups.push((file_path, chunks.len()));
            Ok(())
        })
        .unwrap();

    assert_eq!(groups, vec![("main.py".to_string(), 2)]);
}

#[test]
fn store_fetches_embeddings_for_specific_files() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let chunks = engine.store.embeddings_for_files(&["main.py"]).unwrap();
    assert_eq!(chunks.len(), 2);
    assert!(chunks.iter().all(|chunk| chunk.file_path == "main.py"));
}

#[test]
fn store_fetches_embeddings_for_scored_chunks() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let scored = engine.search_scored("hello world function", 2).unwrap();
    let chunks = engine.store.embeddings_for_scored_chunks(&scored).unwrap();

    assert_eq!(chunks.len(), scored.len());
    assert!(scored.iter().all(|candidate| chunks.iter().any(|chunk| {
        chunk.file_path == candidate.file_path
            && chunk.symbol_name == candidate.symbol_name
            && chunk.line == candidate.line
            && chunk.signature == candidate.signature
            && chunk.name_path == candidate.name_path
    })));
}

#[test]
fn find_misplaced_code_returns_per_file_outliers() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let outliers = engine.find_misplaced_code(5).unwrap();
    assert_eq!(outliers.len(), 2);
    assert!(outliers.iter().all(|item| item.file_path == "main.py"));
}

#[test]
fn find_duplicates_uses_batched_candidate_embeddings() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    replace_file_embeddings_with_sentinels(&engine, "main.py", &[("hello", 5.0), ("world", 5.0)]);

    let duplicates = engine.find_duplicates(0.99, 4).unwrap();
    assert!(!duplicates.is_empty());
    assert!(duplicates.iter().any(|pair| {
        (pair.symbol_a == "main.py:hello" && pair.symbol_b == "main.py:world")
            || (pair.symbol_a == "main.py:world" && pair.symbol_b == "main.py:hello")
    }));
}

#[test]
fn search_scored_returns_raw_chunks() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let chunks = engine.search_scored("world function", 5).unwrap();
    assert!(!chunks.is_empty());
    for c in &chunks {
        assert!(!c.file_path.is_empty());
        assert!(!c.symbol_name.is_empty());
    }
}

#[test]
fn search_scored_reuses_persisted_query_embedding() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();

    let query = "world function";
    let chunks = engine.search_scored(query, 5).unwrap();
    assert!(!chunks.is_empty());
    let cached = engine
        .cached_query_embedding_for_test(query)
        .unwrap()
        .expect("query embedding should be persisted after search");
    assert_eq!(engine.query_cache_stats().unwrap().entries, 1);
    drop(engine);

    let restarted = EmbeddingEngine::new(&project).unwrap();
    let restarted_cached = restarted
        .cached_query_embedding_for_test(query)
        .unwrap()
        .expect("query embedding should survive engine restart");
    assert_eq!(restarted_cached, cached);
    assert!(!restarted.search_scored(query, 5).unwrap().is_empty());
}

#[test]
fn configured_embedding_model_name_defaults_to_codesearchnet() {
    let _lock = MODEL_LOCK.lock().unwrap();
    let previous_dir = std::env::var("CODELENS_MODEL_DIR").ok();
    let previous_model = std::env::var("CODELENS_EMBED_MODEL").ok();
    unsafe {
        std::env::remove_var("CODELENS_MODEL_DIR");
        std::env::remove_var("CODELENS_EMBED_MODEL");
    }

    assert_eq!(configured_embedding_model_name(), CODESEARCH_MODEL_NAME);

    unsafe {
        match previous_dir {
            Some(value) => std::env::set_var("CODELENS_MODEL_DIR", value),
            None => std::env::remove_var("CODELENS_MODEL_DIR"),
        }
        match previous_model {
            Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
            None => std::env::remove_var("CODELENS_EMBED_MODEL"),
        }
    }
}

#[test]
fn resolve_model_dir_accepts_direct_model_dir_override() {
    let _lock = MODEL_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let model_dir = dir.path().join("merged-lora-model");
    write_minimal_model_assets(&model_dir);

    let previous = std::env::var("CODELENS_MODEL_DIR").ok();
    unsafe {
        std::env::set_var("CODELENS_MODEL_DIR", &model_dir);
    }

    let resolved = resolve_model_dir().unwrap();

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_MODEL_DIR", value),
            None => std::env::remove_var("CODELENS_MODEL_DIR"),
        }
    }

    assert_eq!(resolved, model_dir);
}

#[test]
fn executable_model_roots_include_installed_sidecar_layouts() {
    let exe_dir = std::path::Path::new("/opt/codelens/bin");
    let roots = executable_model_roots(exe_dir);

    assert!(roots.contains(&std::path::PathBuf::from("/opt/codelens/bin/models")));
    assert!(roots.contains(&std::path::PathBuf::from("/opt/codelens/models")));
    assert!(roots.contains(&std::path::PathBuf::from(
        "/opt/codelens/share/codelens/models"
    )));
}

#[test]
fn configured_embedding_model_name_prefers_manifest_name_from_model_dir() {
    let _lock = MODEL_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let model_dir = dir.path().join("merged-lora-model");
    write_minimal_model_assets(&model_dir);
    std::fs::write(
        model_dir.join("model-manifest.json"),
        r#"{
            "model_name": "MiniLM-L12-CodeSearchNet-LoRA-Merged-v1",
            "base_model": "MiniLM-L12-CodeSearchNet-INT8",
            "adapter_type": "lora",
            "lora_merged_from": "scripts/finetune/output/lora-python/model",
            "export_backend": "onnx"
        }"#,
    )
    .unwrap();

    let previous_dir = std::env::var("CODELENS_MODEL_DIR").ok();
    let previous_model = std::env::var("CODELENS_EMBED_MODEL").ok();
    unsafe {
        std::env::set_var("CODELENS_MODEL_DIR", &model_dir);
        std::env::remove_var("CODELENS_EMBED_MODEL");
    }

    let configured = configured_embedding_model_name();

    unsafe {
        match previous_dir {
            Some(value) => std::env::set_var("CODELENS_MODEL_DIR", value),
            None => std::env::remove_var("CODELENS_MODEL_DIR"),
        }
        match previous_model {
            Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
            None => std::env::remove_var("CODELENS_EMBED_MODEL"),
        }
    }

    assert_eq!(configured, "MiniLM-L12-CodeSearchNet-LoRA-Merged-v1");
}

#[test]
fn requested_embedding_model_override_ignores_default_model_name() {
    let _lock = MODEL_LOCK.lock().unwrap();
    let previous = std::env::var("CODELENS_EMBED_MODEL").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_MODEL", CODESEARCH_MODEL_NAME);
    }

    let result = requested_embedding_model_override().unwrap();

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
            None => std::env::remove_var("CODELENS_EMBED_MODEL"),
        }
    }

    assert_eq!(result, None);
}

#[cfg(not(feature = "model-bakeoff"))]
#[test]
fn requested_embedding_model_override_requires_bakeoff_feature() {
    let _lock = MODEL_LOCK.lock().unwrap();
    let previous = std::env::var("CODELENS_EMBED_MODEL").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_MODEL", "all-MiniLM-L12-v2");
    }

    let err = requested_embedding_model_override().unwrap_err();

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
            None => std::env::remove_var("CODELENS_EMBED_MODEL"),
        }
    }

    assert!(err.to_string().contains("model-bakeoff"));
}

#[cfg(feature = "model-bakeoff")]
#[test]
fn requested_embedding_model_override_accepts_alternative_model() {
    let _lock = MODEL_LOCK.lock().unwrap();
    let previous = std::env::var("CODELENS_EMBED_MODEL").ok();
    unsafe {
        std::env::set_var("CODELENS_EMBED_MODEL", "all-MiniLM-L12-v2");
    }

    let result = requested_embedding_model_override().unwrap();

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
            None => std::env::remove_var("CODELENS_EMBED_MODEL"),
        }
    }

    assert_eq!(result.as_deref(), Some("all-MiniLM-L12-v2"));
}

#[test]
fn recommended_embed_threads_caps_macos_style_load() {
    let threads = recommended_embed_threads();
    assert!(threads >= 1);
    assert!(threads <= 8);
}

#[cfg(all(target_os = "macos", not(feature = "coreml")))]
#[test]
fn macos_runtime_preference_reports_cpu_when_coreml_is_not_compiled() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("CODELENS_EMBED_PROVIDER").ok();
    unsafe {
        std::env::remove_var("CODELENS_EMBED_PROVIDER");
    }

    let default_preference = configured_embedding_runtime_preference();

    unsafe {
        std::env::set_var("CODELENS_EMBED_PROVIDER", "coreml");
    }
    let explicit_coreml_preference = configured_embedding_runtime_preference();

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_PROVIDER", value),
            None => std::env::remove_var("CODELENS_EMBED_PROVIDER"),
        }
    }

    assert_eq!(default_preference, "cpu");
    assert_eq!(explicit_coreml_preference, "cpu");
}

#[cfg(all(target_os = "macos", feature = "coreml"))]
#[test]
fn macos_runtime_preference_reports_coreml_when_coreml_is_compiled() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("CODELENS_EMBED_PROVIDER").ok();
    unsafe {
        std::env::remove_var("CODELENS_EMBED_PROVIDER");
    }

    let default_preference = configured_embedding_runtime_preference();

    unsafe {
        std::env::set_var("CODELENS_EMBED_PROVIDER", "coreml");
    }
    let explicit_coreml_preference = configured_embedding_runtime_preference();

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_EMBED_PROVIDER", value),
            None => std::env::remove_var("CODELENS_EMBED_PROVIDER"),
        }
    }

    assert_eq!(default_preference, "coreml_preferred");
    assert_eq!(explicit_coreml_preference, "coreml");
}

#[test]
fn embed_batch_size_has_safe_default_floor() {
    assert!(embed_batch_size() >= 1);
    if cfg!(target_os = "macos") {
        assert!(embed_batch_size() <= DEFAULT_MACOS_EMBED_BATCH_SIZE);
    }
}
