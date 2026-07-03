//! Tree-sitter language configuration: parser + query per language.
//! Extracted from symbols.rs to reduce its size.

use crate::lang_registry;
use std::path::Path;
use tree_sitter::Language;

pub(crate) struct LanguageConfig {
    pub extension: &'static str,
    pub language: Language,
    pub query: &'static str,
}

/// Resolve tree-sitter config for a file path via the unified language registry.
/// Extension-less well-known files (Makefile, Dockerfile, Containerfile) resolve
/// by lowercased file name; everything else by lowercased extension.
pub(crate) fn language_for_path(path: &Path) -> Option<LanguageConfig> {
    let key = match path.extension() {
        Some(ext) => ext.to_str()?.to_ascii_lowercase(),
        None => path.file_name()?.to_str()?.to_ascii_lowercase(),
    };
    let entry = lang_registry::for_extension(&key)?;
    config_for_canonical(entry.canonical)
}

/// Map canonical extension to tree-sitter Language + Query.
/// This is the single place to add new language support.
fn config_for_canonical(canonical: &str) -> Option<LanguageConfig> {
    let (ext, lang, query) = match canonical {
        "py" => ("py", tree_sitter_python::LANGUAGE.into(), PYTHON_QUERY),
        "js" => (
            "js",
            tree_sitter_javascript::LANGUAGE.into(),
            JAVASCRIPT_QUERY,
        ),
        "ts" => (
            "ts",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            TYPESCRIPT_QUERY,
        ),
        "tsx" => (
            "tsx",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            TYPESCRIPT_QUERY,
        ),
        #[cfg(feature = "lang-extra")]
        "go" => ("go", tree_sitter_go::LANGUAGE.into(), GO_QUERY),
        #[cfg(feature = "lang-extra")]
        "java" => ("java", tree_sitter_java::LANGUAGE.into(), JAVA_QUERY),
        #[cfg(feature = "lang-extra")]
        "kt" => ("kt", tree_sitter_kotlin::LANGUAGE.into(), KOTLIN_QUERY),
        "rs" => ("rs", tree_sitter_rust::LANGUAGE.into(), RUST_QUERY),
        "c" => ("c", tree_sitter_c::LANGUAGE.into(), C_QUERY),
        "cpp" => ("cpp", tree_sitter_cpp::LANGUAGE.into(), CPP_QUERY),
        #[cfg(feature = "lang-extra")]
        "php" => ("php", tree_sitter_php::LANGUAGE_PHP.into(), PHP_QUERY),
        #[cfg(feature = "lang-extra")]
        "swift" => ("swift", tree_sitter_swift::LANGUAGE.into(), SWIFT_QUERY),
        #[cfg(feature = "lang-extra")]
        "scala" => ("scala", tree_sitter_scala::LANGUAGE.into(), SCALA_QUERY),
        #[cfg(feature = "lang-extra")]
        "rb" => ("rb", tree_sitter_ruby::LANGUAGE.into(), RUBY_QUERY),
        #[cfg(feature = "lang-extra")]
        "cs" => ("cs", tree_sitter_c_sharp::LANGUAGE.into(), CSHARP_QUERY),
        #[cfg(feature = "lang-extra")]
        "dart" => ("dart", tree_sitter_dart::LANGUAGE.into(), DART_QUERY),
        // Phase 6a: new languages
        "lua" => ("lua", tree_sitter_lua::LANGUAGE.into(), LUA_QUERY),
        #[cfg(feature = "lang-extra")]
        "zig" => ("zig", tree_sitter_zig::LANGUAGE.into(), ZIG_QUERY),
        #[cfg(feature = "lang-extra")]
        "ex" => ("ex", tree_sitter_elixir::LANGUAGE.into(), ELIXIR_QUERY),
        #[cfg(feature = "lang-extra")]
        "hs" => ("hs", tree_sitter_haskell::LANGUAGE.into(), HASKELL_QUERY),
        #[cfg(feature = "lang-extra")]
        "ml" => ("ml", tree_sitter_ocaml::LANGUAGE_OCAML.into(), OCAML_QUERY),
        #[cfg(feature = "lang-extra")]
        "erl" => ("erl", tree_sitter_erlang::LANGUAGE.into(), ERLANG_QUERY),
        #[cfg(feature = "lang-extra")]
        "r" => ("r", tree_sitter_r::LANGUAGE.into(), R_QUERY),
        "sh" => ("sh", tree_sitter_bash::LANGUAGE.into(), BASH_QUERY),
        #[cfg(feature = "lang-extra")]
        "jl" => ("jl", tree_sitter_julia::LANGUAGE.into(), JULIA_QUERY),
        "css" => ("css", tree_sitter_css::LANGUAGE.into(), CSS_QUERY),
        "html" => ("html", tree_sitter_html::LANGUAGE.into(), HTML_QUERY),
        "toml" => ("toml", tree_sitter_toml_updated::language(), TOML_QUERY),
        "yaml" => ("yaml", tree_sitter_yaml::LANGUAGE.into(), YAML_QUERY),
        #[cfg(feature = "lang-extra")]
        "clj" => ("clj", tree_sitter_clojure::LANGUAGE.into(), CLOJURE_QUERY),
        // P2.2 Stage 1: make/dockerfile/vim/fsharp run on tree-sitter 0.25
        // (grammar crates depend on tree-sitter-language ^0.1, core-agnostic).
        #[cfg(feature = "lang-extra")]
        "mk" => ("mk", tree_sitter_make::LANGUAGE.into(), MAKE_QUERY),
        #[cfg(feature = "lang-extra")]
        "dockerfile" => (
            "dockerfile",
            tree_sitter_containerfile::LANGUAGE.into(),
            DOCKERFILE_QUERY,
        ),
        #[cfg(feature = "lang-extra")]
        "vim" => ("vim", tree_sitter_vim::language(), VIM_QUERY),
        #[cfg(feature = "lang-extra")]
        "fs" => (
            "fs",
            tree_sitter_fsharp::LANGUAGE_FSHARP.into(),
            FSHARP_QUERY,
        ),
        // Perl blocked: tree-sitter-perl requires core ^0.26.3, mutually
        // exclusive with tree-sitter-clojure 0.1.0 (core ^0.25.6) under the
        // `links = "tree-sitter"` single-version constraint.
        _ => return None,
    };
    Some(LanguageConfig {
        extension: ext,
        language: lang,
        query,
    })
}

const PYTHON_QUERY: &str = r#"
    (class_definition name: (identifier) @class.name) @class.def
    (function_definition name: (identifier) @function.name) @function.def
    (decorated_definition definition: (class_definition name: (identifier) @class.name)) @class.def
    (decorated_definition definition: (function_definition name: (identifier) @function.name)) @function.def
    (assignment left: (identifier) @variable.name) @variable.def
"#;

const JAVASCRIPT_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (function_declaration name: (identifier) @function.name) @function.def
    (method_definition name: (property_identifier) @method.name) @method.def
    (lexical_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
    (variable_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
"#;

const TYPESCRIPT_QUERY: &str = r#"
    (class_declaration name: (type_identifier) @class.name) @class.def
    (abstract_class_declaration name: (type_identifier) @class.name) @class.def
    (function_declaration name: (identifier) @function.name) @function.def
    (method_definition name: (property_identifier) @method.name) @method.def
    (interface_declaration name: (type_identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (type_alias_declaration name: (type_identifier) @type_alias.name) @type_alias.def
    (module name: (identifier) @module.name) @module.def
    (lexical_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
"#;

#[cfg(feature = "lang-extra")]
const GO_QUERY: &str = r#"
    (function_declaration name: (identifier) @function.name) @function.def
    (method_declaration name: (field_identifier) @method.name) @method.def
    (type_declaration (type_spec name: (type_identifier) @class.name type: (struct_type))) @class.def
    (type_declaration (type_spec name: (type_identifier) @interface.name type: (interface_type))) @interface.def
    (type_declaration (type_spec name: (type_identifier) @type_alias.name)) @type_alias.def
    (const_declaration (const_spec name: (identifier) @variable.name)) @variable.def
    (var_declaration (var_spec name: (identifier) @variable.name)) @variable.def
"#;

#[cfg(feature = "lang-extra")]
const JAVA_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (interface_declaration name: (identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (method_declaration name: (identifier) @method.name) @method.def
    (constructor_declaration name: (identifier) @method.name) @method.def
    (field_declaration declarator: (variable_declarator name: (identifier) @variable.name)) @variable.def
    (annotation_type_declaration name: (identifier) @interface.name) @interface.def
    (record_declaration name: (identifier) @class.name) @class.def
"#;

#[cfg(feature = "lang-extra")]
const KOTLIN_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (object_declaration name: (identifier) @class.name) @class.def
    (function_declaration name: (identifier) @function.name) @function.def
    (companion_object) @class.def
"#;

const RUST_QUERY: &str = r#"
    (struct_item name: (type_identifier) @class.name) @class.def
    (enum_item name: (type_identifier) @enum.name) @enum.def
    (trait_item name: (type_identifier) @interface.name) @interface.def
    (function_item name: (identifier) @function.name) @function.def
    (const_item name: (identifier) @variable.name) @variable.def
    (static_item name: (identifier) @variable.name) @variable.def
    (type_item name: (type_identifier) @type_alias.name) @type_alias.def
    (macro_definition name: (identifier) @function.name) @function.def
    (mod_item name: (identifier) @module.name) @module.def
"#;

const C_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(type_definition declarator: (type_identifier) @typealias.name) @typealias.def
"#;

const CPP_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(function_definition declarator: (function_declarator declarator: (qualified_identifier name: (identifier) @method.name))) @method.def
(class_specifier name: (type_identifier) @class.name) @class.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(namespace_definition (namespace_identifier) @module.name) @module.def
(type_definition declarator: (type_identifier) @type_alias.name) @type_alias.def
(template_declaration (class_specifier name: (type_identifier) @class.name)) @class.def
"#;

#[cfg(feature = "lang-extra")]
const PHP_QUERY: &str = r#"
(class_declaration name: (name) @class.name) @class.def
(interface_declaration name: (name) @interface.name) @interface.def
(trait_declaration name: (name) @interface.name) @interface.def
(enum_declaration name: (name) @enum.name) @enum.def
(function_definition name: (name) @function.name) @function.def
(method_declaration name: (name) @method.name) @method.def
"#;

#[cfg(feature = "lang-extra")]
const SWIFT_QUERY: &str = r#"
(class_declaration name: (type_identifier) @class.name) @class.def
(protocol_declaration name: (type_identifier) @interface.name) @interface.def
(function_declaration name: (simple_identifier) @function.name) @function.def
"#;

#[cfg(feature = "lang-extra")]
const SCALA_QUERY: &str = r#"
    (class_definition name: (identifier) @class.name) @class.def
    (object_definition name: (identifier) @class.name) @class.def
    (trait_definition name: (identifier) @interface.name) @interface.def
    (function_definition name: (identifier) @function.name) @function.def
"#;

#[cfg(feature = "lang-extra")]
const RUBY_QUERY: &str = r#"
    (class name: [(constant) (scope_resolution)] @class.name) @class.def
    (module name: [(constant) (scope_resolution)] @module.name) @module.def
    (method name: [(identifier) (constant) (simple_symbol) (delimited_symbol) (setter)] @method.name) @method.def
    (singleton_method name: [(identifier) (constant) (simple_symbol) (delimited_symbol) (setter)] @method.name) @method.def
"#;

#[cfg(feature = "lang-extra")]
const CSHARP_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (struct_declaration name: (identifier) @class.name) @class.def
    (interface_declaration name: (identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (method_declaration name: (identifier) @method.name) @method.def
    (constructor_declaration name: (identifier) @method.name) @method.def
    (namespace_declaration name: (identifier) @module.name) @module.def
"#;

#[cfg(feature = "lang-extra")]
const DART_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (mixin_declaration name: (identifier) @class.name) @class.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (class_member (method_signature (function_signature name: (identifier) @method.name))) @method.def
    (function_signature name: (identifier) @function.name) @function.def
"#;

// --- Phase 6a: new language queries ---

const LUA_QUERY: &str = r#"
    (function_declaration name: (identifier) @function.name) @function.def
    (function_declaration name: (dot_index_expression) @function.name) @function.def
"#;

#[cfg(feature = "lang-extra")]
const ZIG_QUERY: &str = r#"
    (function_declaration name: (identifier) @function.name) @function.def
"#;

#[cfg(feature = "lang-extra")]
const ELIXIR_QUERY: &str = r#"
    (call target: (identifier) (arguments (alias) @class.name) (do_block)) @class.def
    (call target: (identifier) (arguments (call target: (identifier) @function.name))) @function.def
"#;

#[cfg(feature = "lang-extra")]
const HASKELL_QUERY: &str = r#"
    (function name: (variable) @function.name) @function.def
    (signature name: (variable) @function.name) @function.def
"#;

#[cfg(feature = "lang-extra")]
const OCAML_QUERY: &str = r#"
    (value_definition (let_binding pattern: (value_name) @function.name)) @function.def
    (type_definition (type_binding name: (type_constructor) @class.name)) @class.def
"#;

#[cfg(feature = "lang-extra")]
const ERLANG_QUERY: &str = r#"
    (fun_decl clause: (function_clause name: (atom) @function.name)) @function.def
"#;

#[cfg(feature = "lang-extra")]
const R_QUERY: &str = r#"
    (binary_operator lhs: (identifier) @function.name rhs: (function_definition)) @function.def
"#;

const BASH_QUERY: &str = r#"
    (function_definition name: (word) @function.name) @function.def
"#;

#[cfg(feature = "lang-extra")]
const JULIA_QUERY: &str = r#"
    (function_definition (signature (call_expression (identifier) @function.name))) @function.def
    (struct_definition name: (identifier) @class.name) @class.def
    (module_definition name: (identifier) @module.name) @module.def
"#;

// Perl query deferred until tree-sitter 0.26 upgrade

/// Quality benchmark: all 25 languages must parse and extract symbols correctly.
/// This is the acceptance test for language support quality.
#[cfg(test)]
fn assert_extracts(
    lang_name: &str,
    lang: tree_sitter::Language,
    query_str: &str,
    source: &str,
    expected: &[&str],
) {
    use streaming_iterator::StreamingIterator;
    use tree_sitter::{Parser, Query, QueryCursor};

    let query =
        Query::new(&lang, query_str).unwrap_or_else(|e| panic!("{lang_name} query compile: {e}"));
    let mut parser = Parser::new();
    parser.set_language(&lang).unwrap();
    let tree = parser
        .parse(source, None)
        .unwrap_or_else(|| panic!("{lang_name} parse failed"));
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut found: Vec<String> = Vec::new();
    while let Some(m) = matches.next() {
        for cap in m.captures {
            let cap_name = &query.capture_names()[cap.index as usize];
            if cap_name.ends_with(".name") {
                found.push(
                    String::from_utf8_lossy(&source.as_bytes()[cap.node.byte_range()]).to_string(),
                );
            }
        }
    }
    for exp in expected {
        assert!(
            found.contains(&exp.to_string()),
            "{lang_name}: expected '{exp}' not found. Got: {found:?}"
        );
    }
}
// const PERL_QUERY: &str = r#"
//     (subroutine_declaration_statement name: (bareword) @function.name) @function.def
//     (package_statement name: (package_name) @class.name) @class.def
// "#;

// `lang-extra` gate: the entire tests module exercises queries for the
// niche tree-sitter languages (canonical list in Cargo.toml `lang-extra`).
// Under `--no-default-features` builds those crates are not linked and the
// test bodies would fail to compile. `assert_extracts` above remains under
// `#[cfg(test)]` only — it is a generic helper that takes a
// `tree_sitter::Language` and is language-agnostic.
#[cfg(all(test, feature = "lang-extra"))]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use tree_sitter::Query;

    #[test]
    fn all_new_queries_compile() {
        let cases: Vec<(&str, tree_sitter::Language, &str)> = vec![
            ("lua", tree_sitter_lua::LANGUAGE.into(), LUA_QUERY),
            ("zig", tree_sitter_zig::LANGUAGE.into(), ZIG_QUERY),
            ("elixir", tree_sitter_elixir::LANGUAGE.into(), ELIXIR_QUERY),
            (
                "haskell",
                tree_sitter_haskell::LANGUAGE.into(),
                HASKELL_QUERY,
            ),
            (
                "ocaml",
                tree_sitter_ocaml::LANGUAGE_OCAML.into(),
                OCAML_QUERY,
            ),
            ("erlang", tree_sitter_erlang::LANGUAGE.into(), ERLANG_QUERY),
            ("r", tree_sitter_r::LANGUAGE.into(), R_QUERY),
            ("bash", tree_sitter_bash::LANGUAGE.into(), BASH_QUERY),
            ("julia", tree_sitter_julia::LANGUAGE.into(), JULIA_QUERY),
            ("make", tree_sitter_make::LANGUAGE.into(), MAKE_QUERY),
            (
                "dockerfile",
                tree_sitter_containerfile::LANGUAGE.into(),
                DOCKERFILE_QUERY,
            ),
            ("vim", tree_sitter_vim::language(), VIM_QUERY),
            (
                "fsharp",
                tree_sitter_fsharp::LANGUAGE_FSHARP.into(),
                FSHARP_QUERY,
            ),
        ];
        for (name, lang, query_str) in cases {
            let result = Query::new(&lang, query_str);
            assert!(
                result.is_ok(),
                "{name} query failed to compile: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn new_langs_parse_and_extract_symbols() {
        let cases: Vec<(&str, tree_sitter::Language, &str, &str, &[&str])> = vec![
            (
                "lua",
                tree_sitter_lua::LANGUAGE.into(),
                LUA_QUERY,
                "function greet(name)\n  print(name)\nend\n\nlocal function helper()\n  return 42\nend",
                &["greet", "helper"],
            ),
            (
                "zig",
                tree_sitter_zig::LANGUAGE.into(),
                ZIG_QUERY,
                "pub fn add(a: i32, b: i32) i32 {\n    return a + b;\n}",
                &["add"],
            ),
            (
                "haskell",
                tree_sitter_haskell::LANGUAGE.into(),
                HASKELL_QUERY,
                "factorial :: Int -> Int\nfactorial 0 = 1\nfactorial n = n * factorial (n - 1)\n\ndata Color = Red | Green | Blue",
                &["factorial"],
            ),
            (
                "bash",
                tree_sitter_bash::LANGUAGE.into(),
                BASH_QUERY,
                "greet() {\n    echo \"Hello $1\"\n}\n\nhelper() {\n    return 0\n}",
                &["greet", "helper"],
            ),
            (
                "r",
                tree_sitter_r::LANGUAGE.into(),
                R_QUERY,
                "greet <- function(name) {\n  paste(\"Hello\", name)\n}",
                &["greet"],
            ),
        ];

        for (name, lang, query_str, source, expected_names) in cases {
            let query = Query::new(&lang, query_str)
                .unwrap_or_else(|e| panic!("{name} query compile error: {e}"));
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&lang).unwrap();
            let tree = parser.parse(source, None).unwrap();
            let mut cursor = tree_sitter::QueryCursor::new();
            let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

            let mut found_names: Vec<String> = Vec::new();
            use streaming_iterator::StreamingIterator;
            while let Some(m) = matches.next() {
                for cap in m.captures {
                    let cap_name = &query.capture_names()[cap.index as usize];
                    if cap_name.ends_with(".name") {
                        let text = &source.as_bytes()[cap.node.byte_range()];
                        found_names.push(String::from_utf8_lossy(text).to_string());
                    }
                }
            }

            for exp in expected_names {
                assert!(
                    found_names.contains(&exp.to_string()),
                    "{name}: expected symbol '{exp}' not found. Got: {found_names:?}"
                );
            }
        }
    }

    /// P2.2 Stage 1 smoke: make/dockerfile/vim/fsharp symbol extraction.
    #[test]
    fn stage1_langs_extract_symbols() {
        super::assert_extracts(
            "make",
            tree_sitter_make::LANGUAGE.into(),
            MAKE_QUERY,
            "CC := gcc\n\nbuild:\n\t$(CC) -o app main.c\n\nclean:\n\trm -f app\n",
            &["CC", "build", "clean"],
        );
        super::assert_extracts(
            "dockerfile",
            tree_sitter_containerfile::LANGUAGE.into(),
            DOCKERFILE_QUERY,
            "FROM rust:1.79 AS builder\nARG VERSION=1\nENV APP_HOME=/app\nRUN cargo build\n",
            &["builder", "VERSION", "APP_HOME"],
        );
        super::assert_extracts(
            "vim",
            tree_sitter_vim::language(),
            VIM_QUERY,
            "function! MyFunc(arg)\n  return a:arg\nendfunction\n\nfunction s:helper()\n  return 1\nendfunction\n",
            &["MyFunc", "s:helper"],
        );
        super::assert_extracts(
            "fsharp",
            tree_sitter_fsharp::LANGUAGE_FSHARP.into(),
            FSHARP_QUERY,
            "module MyApp\n\nlet add x y = x + y\n\ntype Point = { X: int; Y: int }\n",
            &["MyApp", "add", "Point"],
        );
    }

    /// Extension-less build files must resolve through the filename fallback.
    #[test]
    fn extensionless_build_files_resolve_by_filename() {
        for name in ["Makefile", "makefile", "Dockerfile", "Containerfile"] {
            assert!(
                language_for_path(Path::new(name)).is_some(),
                "{name} should resolve via filename fallback"
            );
        }
        for name in [
            "build.mk",
            "deploy.dockerfile",
            "app.containerfile",
            "plugin.vim",
            "Library.fs",
            "script.fsx",
        ] {
            assert!(
                language_for_path(Path::new(name)).is_some(),
                "{name} should resolve via extension"
            );
        }
        assert!(language_for_path(Path::new("README")).is_none());
    }

    /// Quality benchmark: original 16 languages symbol extraction.
    #[test]
    fn original_16_langs_extract_symbols() {
        super::assert_extracts(
            "python",
            tree_sitter_python::LANGUAGE.into(),
            PYTHON_QUERY,
            "class Foo:\n    def bar(self):\n        pass\ndef baz():\n    pass",
            &["Foo", "bar", "baz"],
        );
        super::assert_extracts(
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            JAVASCRIPT_QUERY,
            "class App {}\nfunction main() {}\nconst x = 1;",
            &["App", "main", "x"],
        );
        super::assert_extracts(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            TYPESCRIPT_QUERY,
            "interface User {}\nclass Service {}\nfunction init() {}\nenum Color { Red }\ntype ID = string;",
            &["User", "Service", "init", "Color", "ID"],
        );
        super::assert_extracts(
            "go",
            tree_sitter_go::LANGUAGE.into(),
            GO_QUERY,
            "func main() {}\ntype Config struct {}",
            &["main", "Config"],
        );
        super::assert_extracts(
            "java",
            tree_sitter_java::LANGUAGE.into(),
            JAVA_QUERY,
            "class App {\n    void run() {}\n}\ninterface Service {}\nenum Status { OK }",
            &["App", "run", "Service", "Status"],
        );
        super::assert_extracts(
            "kotlin",
            tree_sitter_kotlin::LANGUAGE.into(),
            KOTLIN_QUERY,
            "class App\nobject Singleton\nfun main() {}",
            &["App", "Singleton", "main"],
        );
        super::assert_extracts(
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            RUST_QUERY,
            "struct Foo {}\nenum Bar {}\ntrait Baz {}\nfn main() {}\nconst X: i32 = 1;\ntype Alias = i32;",
            &["Foo", "Bar", "Baz", "main", "X", "Alias"],
        );
        super::assert_extracts(
            "c",
            tree_sitter_c::LANGUAGE.into(),
            C_QUERY,
            "void greet() {}\nstruct Point {};\nenum Color {};",
            &["greet", "Point", "Color"],
        );
        super::assert_extracts(
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            CPP_QUERY,
            "class Widget {};\nvoid update() {}\nnamespace ui {}",
            &["Widget", "update", "ui"],
        );
        super::assert_extracts(
            "php",
            tree_sitter_php::LANGUAGE_PHP.into(),
            PHP_QUERY,
            "<?php\nclass App {}\nfunction main() {}",
            &["App", "main"],
        );
        super::assert_extracts(
            "swift",
            tree_sitter_swift::LANGUAGE.into(),
            SWIFT_QUERY,
            "class ViewController {}\nprotocol Delegate {}\nfunc run() {}",
            &["ViewController", "Delegate", "run"],
        );
        super::assert_extracts(
            "scala",
            tree_sitter_scala::LANGUAGE.into(),
            SCALA_QUERY,
            "class App\nobject Main\ntrait Service\ndef run() = {}",
            &["App", "Main", "Service", "run"],
        );
        super::assert_extracts(
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            RUBY_QUERY,
            "class Foo\n  def bar\n  end\nend\nmodule Baz\nend",
            &["Foo", "bar", "Baz"],
        );
        super::assert_extracts(
            "csharp",
            tree_sitter_c_sharp::LANGUAGE.into(),
            CSHARP_QUERY,
            "class App {}\ninterface IService {}\nenum Status {}\nnamespace UI {}",
            &["App", "IService", "Status", "UI"],
        );
        super::assert_extracts(
            "dart",
            tree_sitter_dart::LANGUAGE.into(),
            DART_QUERY,
            "class App {}\nmixin Scroll {}\nenum Color { red }",
            &["App", "Scroll", "Color"],
        );
    }
}

// ── Phase 3: new languages ────────────────────────────────────────────────

const CSS_QUERY: &str = r#"
    (rule_set (selectors (class_selector (class_name) @class.name)) @class.def)
    (rule_set (selectors (id_selector (id_name) @class.name)) @class.def)
"#;

const HTML_QUERY: &str = r#"
    (element (start_tag (tag_name) @class.name)) @class.def
"#;

const TOML_QUERY: &str = r#"
    (table (bare_key) @class.name) @class.def
"#;

const YAML_QUERY: &str = r#"
    (block_mapping_pair key: (flow_node) @class.name) @class.def
"#;

#[cfg(feature = "lang-extra")]
const CLOJURE_QUERY: &str = r#"
    (list_lit (sym_lit) @func.name) @func.def
"#;

// ── P2.2 Stage 1: make/dockerfile/vim/fsharp ─────────────────────────────

#[cfg(feature = "lang-extra")]
const MAKE_QUERY: &str = r#"
    (rule (targets (word) @function.name)) @function.def
    (variable_assignment name: (word) @variable.name) @variable.def
    (define_directive name: (word) @variable.name) @variable.def
"#;

// Node names come from tree-sitter-containerfile (Dockerfile-syntax compatible);
// tree-sitter-dockerfile itself pins core ^0.20 and cannot resolve in this workspace.
#[cfg(feature = "lang-extra")]
const DOCKERFILE_QUERY: &str = r#"
    (from_instruction as: (image_alias) @class.name) @class.def
    (arg_instruction (arg_pair name: (unquoted_string) @variable.name)) @variable.def
    (env_instruction (env_pair name: (unquoted_string) @variable.name)) @variable.def
"#;

#[cfg(feature = "lang-extra")]
const VIM_QUERY: &str = r#"
    (function_definition (function_declaration name: [(identifier) (scoped_identifier)] @function.name)) @function.def
"#;

#[cfg(feature = "lang-extra")]
const FSHARP_QUERY: &str = r#"
    (named_module name: (long_identifier) @module.name) @module.def
    (module_defn (identifier) @module.name) @module.def
    (function_or_value_defn (function_declaration_left (identifier) @function.name)) @function.def
    (type_definition (_ (type_name type_name: (identifier) @class.name))) @class.def
"#;
