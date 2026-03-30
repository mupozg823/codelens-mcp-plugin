use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

// ── Python ────────────────────────────────────────────────────────────────────
pub(super) static PY_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*import\s+([A-Za-z0-9_.,\s]+)").unwrap());
pub(super) static PY_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*from\s+([A-Za-z0-9_\.]+)\s+import\s+").unwrap());

// ── JavaScript / TypeScript ───────────────────────────────────────────────────
pub(super) static JS_IMPORT_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\bimport\s+[^;]*?\sfrom\s+["']([^"']+)["']"#).unwrap());
pub(super) static JS_IMPORT_SIDE_EFFECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\bimport\s+["']([^"']+)["']"#).unwrap());
pub(super) static JS_REQUIRE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"require\(\s*["']([^"']+)["']\s*\)"#).unwrap());
pub(super) static JS_DYNAMIC_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"import\(\s*["']([^"']+)["']\s*\)"#).unwrap());
pub(super) static JS_REEXPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\bexport\s+[^;]*?\sfrom\s+["']([^"']+)["']"#).unwrap());

// ── Go ────────────────────────────────────────────────────────────────────────
pub(super) static GO_SINGLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*import\s+"([^"]+)""#).unwrap());
pub(super) static GO_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""([^"]+)""#).unwrap());
pub(super) static GO_BLOCK_SECTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)\bimport\s*\(([^)]*)\)"#).unwrap());

// ── Java ──────────────────────────────────────────────────────────────────────
pub(super) static JAVA_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*import\s+(?:static\s+)?([A-Za-z0-9_.]+)\s*;").unwrap());

// ── Kotlin ────────────────────────────────────────────────────────────────────
pub(super) static KT_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*import\s+([A-Za-z0-9_.]+)(?:\s+as\s+[A-Za-z0-9_]+)?").unwrap()
});

// ── Rust ──────────────────────────────────────────────────────────────────────
pub(super) static RS_USE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([A-Za-z0-9_]+(?:::[A-Za-z0-9_]+)*)(?:::\{([^}]+)\})?")
        .unwrap()
});
pub(super) static RS_MOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z0-9_]+)\s*;").unwrap()
});

// ── Ruby ──────────────────────────────────────────────────────────────────────
pub(super) static RB_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:require|require_relative|load)\s+["']([^"']+)["']"#).unwrap()
});

// ── C / C++ ───────────────────────────────────────────────────────────────────
pub(super) static C_INCLUDE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*#\s*include\s+[<"]([^>"]+)[>"]"#).unwrap());

// ── PHP ───────────────────────────────────────────────────────────────────────
pub(super) static PHP_USE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*use\s+([A-Za-z0-9_\\]+)\s*;").unwrap());
pub(super) static PHP_REQ_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:require|require_once|include|include_once)\s+["']([^"']+)["']\s*;"#)
        .unwrap()
});

// ── C# ───────────────────────────────────────────────────────────────────────
pub(super) static CS_USING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*using\s+(?:static\s+)?([A-Za-z0-9_.]+)\s*;").unwrap());

// ── Dart ─────────────────────────────────────────────────────────────────────
pub(super) static DART_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*import\s+["']([^"']+)["']"#).unwrap());
pub(super) static DART_EXPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*export\s+["']([^"']+)["']"#).unwrap());

// ── collect_top_level_funcs patterns ─────────────────────────────────────────
pub(super) static TLF_PY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^def ([A-Za-z_][A-Za-z0-9_]*)").unwrap());
pub(super) static TLF_JS_RE1: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^function ([A-Za-z_][A-Za-z0-9_]*)").unwrap());
pub(super) static TLF_JS_RE2: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:export\s+)?(?:async\s+)?function ([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});
pub(super) static TLF_GO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^func ([A-Za-z_][A-Za-z0-9_]*)").unwrap());
pub(super) static TLF_JVM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(?:public|private|protected|static|\s)+\s+\w+\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")
        .unwrap()
});
pub(super) static TLF_RS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:pub(?:\([^)]*\))?\s+)?fn ([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});

// ── extract_imports dispatcher ───────────────────────────────────────────────

pub(super) fn extract_imports(path: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    extract_imports_from_source(path, &content)
}

/// Extract imports from already-loaded source content (avoids re-reading disk).
pub fn extract_imports_from_source(path: &Path, content: &str) -> Vec<String> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "py" => extract_python_imports(content),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => extract_js_imports(content),
        "go" => extract_go_imports(content),
        "java" => extract_java_imports(content),
        "kt" | "kts" => extract_kotlin_imports(content),
        "rs" => extract_rust_imports(content),
        "rb" => extract_ruby_imports(content),
        "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => extract_c_imports(content),
        "php" => extract_php_imports(content),
        "cs" => extract_csharp_imports(content),
        "dart" => extract_dart_imports(content),
        _ => Vec::new(),
    }
}

// ── Language-specific extractors ─────────────────────────────────────────────

pub(super) fn extract_python_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for capture in PY_IMPORT_RE.captures_iter(content) {
        let Some(modules) = capture.get(1) else {
            continue;
        };
        for module in modules.as_str().split(',') {
            let module = module.trim().split_whitespace().next().unwrap_or_default();
            if !module.is_empty() {
                imports.push(module.to_owned());
            }
        }
    }
    for capture in PY_FROM_RE.captures_iter(content) {
        let Some(module) = capture.get(1) else {
            continue;
        };
        imports.push(module.as_str().trim().to_owned());
    }
    imports
}

pub(super) fn extract_js_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for regex in [
        &*JS_IMPORT_FROM_RE,
        &*JS_IMPORT_SIDE_EFFECT_RE,
        &*JS_REQUIRE_RE,
        &*JS_DYNAMIC_IMPORT_RE,
        &*JS_REEXPORT_RE,
    ] {
        for capture in regex.captures_iter(content) {
            let Some(module) = capture.get(1) else {
                continue;
            };
            imports.push(module.as_str().trim().to_owned());
        }
    }
    imports
}

pub(super) fn extract_go_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for cap in GO_SINGLE_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            imports.push(m.as_str().to_owned());
        }
    }
    for section in GO_BLOCK_SECTION_RE.captures_iter(content) {
        if let Some(body) = section.get(1) {
            for cap in GO_BLOCK_RE.captures_iter(body.as_str()) {
                if let Some(m) = cap.get(1) {
                    imports.push(m.as_str().to_owned());
                }
            }
        }
    }
    imports
}

pub(super) fn extract_java_imports(content: &str) -> Vec<String> {
    JAVA_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

pub(super) fn extract_kotlin_imports(content: &str) -> Vec<String> {
    KT_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

pub(super) fn extract_rust_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();

    for cap in RS_MOD_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            imports.push(m.as_str().to_owned());
        }
    }

    for cap in RS_USE_RE.captures_iter(content) {
        let base = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(brace) = cap.get(2) {
            for item in brace.as_str().split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    imports.push(format!("{base}::{item}"));
                }
            }
        } else if !base.is_empty() {
            imports.push(base.to_owned());
        }
    }
    imports
}

pub(super) fn extract_ruby_imports(content: &str) -> Vec<String> {
    RB_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

pub(super) fn extract_c_imports(content: &str) -> Vec<String> {
    C_INCLUDE_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

pub(super) fn extract_php_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for re in [&*PHP_USE_RE, &*PHP_REQ_RE] {
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                imports.push(m.as_str().to_owned());
            }
        }
    }
    imports
}

pub(super) fn extract_csharp_imports(content: &str) -> Vec<String> {
    CS_USING_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_owned()))
        .collect()
}

pub(super) fn extract_dart_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for re in [&*DART_IMPORT_RE, &*DART_EXPORT_RE] {
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                let path = m.as_str();
                if !path.starts_with("dart:") {
                    imports.push(path.to_owned());
                }
            }
        }
    }
    imports
}

// ── extract_imports_for_file (public wrapper) ────────────────────────────────

/// Extract raw import strings from a file. Public for use by the indexer.
pub fn extract_imports_for_file(path: &Path) -> Vec<String> {
    extract_imports(path)
}

// ── collect_top_level_funcs ──────────────────────────────────────────────────

/// Lightweight regex-based top-level function name extractor.
/// Fills `funcs` map with (name -> line_number). Does not overwrite existing entries.
pub(super) fn collect_top_level_funcs(
    path: &Path,
    source: &str,
    funcs: &mut HashMap<String, usize>,
) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let regexes: &[&Regex] = match ext.as_str() {
        "py" => &[&*TLF_PY_RE],
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" => &[&*TLF_JS_RE1, &*TLF_JS_RE2],
        "go" => &[&*TLF_GO_RE],
        "java" | "kt" | "cs" => &[&*TLF_JVM_RE],
        "rs" => &[&*TLF_RS_RE],
        "dart" => &[&*TLF_PY_RE, &*TLF_JVM_RE],
        _ => return,
    };

    for re in regexes {
        for cap in re.captures_iter(source) {
            let Some(m) = cap.get(1) else { continue };
            let name = m.as_str().to_owned();
            if !name.is_empty() {
                let offset = m.start();
                let line = source[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
                funcs.entry(name).or_insert(line);
            }
        }
    }
}
