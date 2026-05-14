use std::path::Path;
use tree_sitter::Language;

use super::queries::*;

pub(crate) struct CallLanguageConfig {
    /// Stable language/cache key. JS and TS can share query text but not compiled queries.
    pub(crate) language_key: &'static str,
    pub(crate) language: Language,
    /// Query to find function definitions: captures @func.name
    pub(crate) func_query: &'static str,
    /// Query to find call sites: captures @callee
    pub(crate) call_query: &'static str,
}

pub(crate) fn call_language_for_path(path: &Path) -> Option<CallLanguageConfig> {
    let lang_config = crate::lang_config::language_for_path(path)?;
    // Map canonical extension to call graph queries (not all languages support this)
    let (language_key, func_query, call_query) = match lang_config.extension {
        "py" => ("py", PYTHON_FUNC_QUERY, PYTHON_CALL_QUERY),
        "js" => ("js", JS_FUNC_QUERY, JS_JSX_CALL_QUERY),
        "ts" => ("ts", JS_FUNC_QUERY, JS_CALL_QUERY),
        "tsx" => ("tsx", JS_FUNC_QUERY, JS_JSX_CALL_QUERY),
        "go" => ("go", GO_FUNC_QUERY, GO_CALL_QUERY),
        "java" => ("java", JAVA_FUNC_QUERY, JAVA_CALL_QUERY),
        "kt" => ("kt", KOTLIN_FUNC_QUERY, KOTLIN_CALL_QUERY),
        "rs" => ("rs", RUST_FUNC_QUERY, RUST_CALL_QUERY),
        _ => return None,
    };
    Some(CallLanguageConfig {
        language_key,
        language: lang_config.language,
        func_query,
        call_query,
    })
}

pub(crate) fn call_language_key_for_path(path: &str) -> Option<&'static str> {
    match Path::new(path).extension().and_then(|value| value.to_str()) {
        Some("py") => Some("py"),
        Some("js") => Some("js"),
        Some("jsx") => Some("jsx"),
        Some("ts") => Some("ts"),
        Some("tsx") => Some("tsx"),
        Some("go") => Some("go"),
        Some("java") => Some("java"),
        Some("kt") => Some("kt"),
        Some("rs") => Some("rs"),
        _ => None,
    }
}

pub(crate) fn same_call_language(a: &str, b: &str) -> bool {
    call_language_key_for_path(a)
        .is_some_and(|a_lang| Some(a_lang) == call_language_key_for_path(b))
}

pub(crate) fn shared_parent_component_count(a: &str, b: &str) -> usize {
    let a_components: Vec<String> = Path::new(a)
        .parent()
        .into_iter()
        .flat_map(|path| path.components())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    let b_components: Vec<String> = Path::new(b)
        .parent()
        .into_iter()
        .flat_map(|path| path.components())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    a_components
        .iter()
        .zip(b_components.iter())
        .take_while(|(a, b)| a == b)
        .count()
}

pub(crate) fn best_path_proximity_candidate<'a>(
    caller_file: &str,
    defs: &'a [String],
) -> Option<&'a String> {
    defs.iter()
        .filter(|def| {
            same_call_language(caller_file, def)
                && shared_parent_component_count(caller_file, def) > 0
        })
        .max_by_key(|def| shared_parent_component_count(caller_file, def))
}
