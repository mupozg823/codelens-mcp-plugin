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
pub(crate) fn language_for_path(path: &Path) -> Option<LanguageConfig> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let entry = lang_registry::for_extension(&ext)?;
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
        "go" => ("go", tree_sitter_go::LANGUAGE.into(), GO_QUERY),
        "java" => ("java", tree_sitter_java::LANGUAGE.into(), JAVA_QUERY),
        "kt" => ("kt", tree_sitter_kotlin::LANGUAGE.into(), KOTLIN_QUERY),
        "rs" => ("rs", tree_sitter_rust::LANGUAGE.into(), RUST_QUERY),
        "c" => ("c", tree_sitter_c::LANGUAGE.into(), C_QUERY),
        "cpp" => ("cpp", tree_sitter_cpp::LANGUAGE.into(), CPP_QUERY),
        "php" => ("php", tree_sitter_php::LANGUAGE_PHP.into(), PHP_QUERY),
        "swift" => ("swift", tree_sitter_swift::LANGUAGE.into(), SWIFT_QUERY),
        "scala" => ("scala", tree_sitter_scala::LANGUAGE.into(), SCALA_QUERY),
        "rb" => ("rb", tree_sitter_ruby::LANGUAGE.into(), RUBY_QUERY),
        "cs" => ("cs", tree_sitter_c_sharp::LANGUAGE.into(), CSHARP_QUERY),
        "dart" => ("dart", tree_sitter_dart::LANGUAGE.into(), DART_QUERY),
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
    (function_declaration name: (identifier) @function.name) @function.def
    (method_definition name: (property_identifier) @method.name) @method.def
    (interface_declaration name: (type_identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (type_alias_declaration name: (type_identifier) @type_alias.name) @type_alias.def
    (lexical_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
"#;

const GO_QUERY: &str = r#"
    (function_declaration name: (identifier) @function.name) @function.def
    (method_declaration name: (field_identifier) @method.name) @method.def
    (type_declaration (type_spec name: (type_identifier) @class.name)) @class.def
"#;

const JAVA_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (interface_declaration name: (identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (method_declaration name: (identifier) @method.name) @method.def
    (constructor_declaration name: (identifier) @method.name) @method.def
"#;

const KOTLIN_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (object_declaration name: (identifier) @class.name) @class.def
    (function_declaration name: (identifier) @function.name) @function.def
"#;

const RUST_QUERY: &str = r#"
    (struct_item name: (type_identifier) @class.name) @class.def
    (enum_item name: (type_identifier) @enum.name) @enum.def
    (trait_item name: (type_identifier) @interface.name) @interface.def
    (function_item name: (identifier) @function.name) @function.def
    (const_item name: (identifier) @variable.name) @variable.def
    (static_item name: (identifier) @variable.name) @variable.def
    (type_item name: (type_identifier) @typealias.name) @typealias.def
"#;

const C_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(type_definition declarator: (type_identifier) @typealias.name) @typealias.def
"#;

const CPP_QUERY: &str = r#"
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(class_specifier name: (type_identifier) @class.name) @class.def
(struct_specifier name: (type_identifier) @class.name) @class.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(namespace_definition name: (identifier) @module.name) @module.def
"#;

const PHP_QUERY: &str = r#"
(class_declaration name: (name) @class.name) @class.def
(interface_declaration name: (name) @interface.name) @interface.def
(trait_declaration name: (name) @interface.name) @interface.def
(enum_declaration name: (name) @enum.name) @enum.def
(function_definition name: (name) @function.name) @function.def
(method_declaration name: (name) @method.name) @method.def
"#;

const SWIFT_QUERY: &str = r#"
(class_declaration name: (type_identifier) @class.name) @class.def
(protocol_declaration name: (type_identifier) @interface.name) @interface.def
(function_declaration name: (simple_identifier) @function.name) @function.def
"#;

const SCALA_QUERY: &str = r#"
    (class_definition name: (identifier) @class.name) @class.def
    (object_definition name: (identifier) @class.name) @class.def
    (trait_definition name: (identifier) @interface.name) @interface.def
    (function_definition name: (identifier) @function.name) @function.def
"#;

const RUBY_QUERY: &str = r#"
    (class name: [(constant) (scope_resolution)] @class.name) @class.def
    (module name: [(constant) (scope_resolution)] @module.name) @module.def
    (method name: [(identifier) (constant) (simple_symbol) (delimited_symbol) (setter)] @method.name) @method.def
    (singleton_method name: [(identifier) (constant) (simple_symbol) (delimited_symbol) (setter)] @method.name) @method.def
"#;

const CSHARP_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (struct_declaration name: (identifier) @class.name) @class.def
    (interface_declaration name: (identifier) @interface.name) @interface.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (method_declaration name: (identifier) @method.name) @method.def
    (constructor_declaration name: (identifier) @method.name) @method.def
    (namespace_declaration name: (identifier) @module.name) @module.def
"#;

const DART_QUERY: &str = r#"
    (class_declaration name: (identifier) @class.name) @class.def
    (mixin_declaration name: (identifier) @class.name) @class.def
    (enum_declaration name: (identifier) @enum.name) @enum.def
    (class_member (method_signature (function_signature name: (identifier) @method.name))) @method.def
    (function_signature name: (identifier) @function.name) @function.def
"#;
