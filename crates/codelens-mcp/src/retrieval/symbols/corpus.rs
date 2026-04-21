use codelens_engine::{SymbolInfo, SymbolKind};

pub(crate) const BODY_TOKEN_CAP: usize = 300;
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
/// lexical tokens (letters, digits, underscore) of length >=
/// `MIN_TOKEN_LEN`, unique, and capped at `BODY_TOKEN_CAP` to prevent
/// one long function from dominating BM25F doc length.
pub(crate) fn extract_lexical_tokens(body: &str) -> String {
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

pub(crate) fn language_from_path(path: &str) -> &'static str {
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
pub(crate) fn module_path_from_file(file_path: &str) -> String {
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

pub(crate) fn detect_is_test(file_path: &str) -> bool {
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

pub(crate) fn detect_is_generated(file_path: &str) -> bool {
    let lower = file_path.to_ascii_lowercase();
    lower.contains("/generated/")
        || lower.contains(".generated.")
        || lower.contains("/build/")
        || lower.contains("/target/")
        || lower.ends_with(".g.rs")
        || lower.ends_with(".pb.rs")
        || lower.ends_with(".pb.go")
}

pub(crate) fn detect_exported(signature: &str, kind: &SymbolKind) -> bool {
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
