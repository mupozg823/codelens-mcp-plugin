use crate::embedding::runtime_settings::parse_bool_env;

pub fn nl_tokens_enabled() -> bool {
    if let Some(explicit) = parse_bool_env("CODELENS_EMBED_HINT_INCLUDE_COMMENTS") {
        return explicit;
    }
    auto_hint_should_enable()
}

pub fn auto_hint_mode_enabled() -> bool {
    parse_bool_env("CODELENS_EMBED_HINT_AUTO").unwrap_or(true)
}

pub fn auto_hint_lang() -> Option<String> {
    std::env::var("CODELENS_EMBED_HINT_AUTO_LANG")
        .ok()
        .map(|raw| raw.trim().to_ascii_lowercase())
}

pub fn language_supports_nl_stack(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "rs" | "rust"
            | "cpp"
            | "cc"
            | "cxx"
            | "c++"
            | "c"
            | "go"
            | "golang"
            | "java"
            | "kt"
            | "kotlin"
            | "scala"
            | "cs"
            | "csharp"
            | "ts"
            | "typescript"
            | "tsx"
            | "js"
            | "javascript"
            | "jsx"
    )
}

pub fn language_supports_sparse_weighting(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "rs" | "rust"
            | "cpp"
            | "cc"
            | "cxx"
            | "c++"
            | "c"
            | "go"
            | "golang"
            | "java"
            | "kt"
            | "kotlin"
            | "scala"
            | "cs"
            | "csharp"
    )
}

pub fn auto_hint_should_enable() -> bool {
    if !auto_hint_mode_enabled() {
        return false;
    }
    match auto_hint_lang() {
        Some(lang) => language_supports_nl_stack(&lang),
        None => false,
    }
}

pub fn auto_sparse_should_enable() -> bool {
    if !auto_hint_mode_enabled() {
        return false;
    }
    match auto_hint_lang() {
        Some(lang) => language_supports_sparse_weighting(&lang),
        None => false,
    }
}

pub fn api_calls_enabled() -> bool {
    if let Some(explicit) = parse_bool_env("CODELENS_EMBED_HINT_INCLUDE_API_CALLS") {
        return explicit;
    }
    auto_hint_should_enable()
}
