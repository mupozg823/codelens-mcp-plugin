use super::super::runtime_settings::parse_bool_env;

pub(super) fn symbol_card_enabled() -> bool {
    parse_bool_env("CODELENS_EMBED_SYMBOL_CARD").unwrap_or(true)
}

pub(super) fn build_symbol_card(
    sym: &crate::db::SymbolWithFile,
    source: Option<&str>,
    doc_present: bool,
    body_hint_present: bool,
) -> String {
    let scope = symbol_scope(sym, source);
    format!(
        "card kind={} symbol={} parent={} neighbor={} file={} line={} scope={} signature={} doc={} body={} test_fact={}",
        sym.kind,
        sym.name_path,
        symbol_parent(&sym.name_path).unwrap_or("-"),
        neighbor_context(&sym.file_path),
        sym.file_path,
        sym.line,
        scope,
        presence(!sym.signature.is_empty()),
        presence(doc_present),
        presence(body_hint_present),
        test_fact(scope),
    )
}

fn symbol_scope(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> &'static str {
    if super::is_test_only_symbol(sym, source) || is_probable_test_path(&sym.file_path) {
        "test"
    } else {
        "production"
    }
}

fn presence(value: bool) -> &'static str {
    if value { "present" } else { "absent" }
}

fn test_fact(scope: &str) -> &'static str {
    if scope == "test" {
        "test_symbol"
    } else {
        "production_symbol"
    }
}

fn symbol_parent(name_path: &str) -> Option<&str> {
    name_path
        .rsplit_once('/')
        .or_else(|| name_path.rsplit_once("::"))
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

fn neighbor_context(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let file_name = normalized
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("-");
    let dir_name = normalized.rsplit('/').nth(1).unwrap_or("");
    if !dir_name.is_empty() && dir_name != "src" && dir_name != "crates" {
        format!("module:{dir_name}")
    } else {
        format!("file:{}", file_stem(file_name))
    }
}

fn file_stem(file_name: &str) -> &str {
    file_name
        .split_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name)
}

fn is_probable_test_path(file_path: &str) -> bool {
    file_path.contains("/tests/")
        || file_path.contains("/__tests__/")
        || file_path.contains("/src/test/")
        || file_path.ends_with("_test.py")
        || file_path.ends_with("_test.go")
        || file_path.ends_with("_test.rs")
        || file_path.ends_with("_tests.rs")
        || file_path.ends_with(".test.ts")
        || file_path.ends_with(".test.tsx")
        || file_path.ends_with(".test.js")
        || file_path.ends_with(".spec.ts")
        || file_path.ends_with(".spec.js")
        || file_path.ends_with("Test.java")
        || file_path.ends_with("Tests.java")
        || file_path.ends_with("_test.rb")
}
