use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

// ── JavaScript / TypeScript ───────────────────────────────────────────────────
// `JS_IMPORT_FROM_RE` / `JS_REEXPORT_FROM_RE` are the single source of truth
// for the `import … from "mod"` / `export … from "mod"` forms. They are shared
// with `call_graph::js_imports`, which additionally reads the `clause` group to
// build its binding index. Both expose two named groups: `clause` (the binding
// spec) and `module` (the specifier). The three module-only forms below capture
// `module` alone. All five name the specifier `module` so `extract_js_imports`
// can read it uniformly.
pub(crate) static JS_IMPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\bimport\s+(?P<clause>[^;]+?)\s+from\s+["'](?P<module>[^"']+)["']"#).unwrap()
});
pub(super) static JS_IMPORT_SIDE_EFFECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\bimport\s+["'](?P<module>[^"']+)["']"#).unwrap());
pub(super) static JS_REQUIRE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"require\(\s*["'](?P<module>[^"']+)["']\s*\)"#).unwrap());
pub(super) static JS_DYNAMIC_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"import\(\s*["'](?P<module>[^"']+)["']\s*\)"#).unwrap());
pub(crate) static JS_REEXPORT_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\bexport\s+(?P<clause>[^;]+?)\s+from\s+["'](?P<module>[^"']+)["']"#).unwrap()
});

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
// `async def` too: without a line, a decorated `@router.post` handler skipped
// the decorator check and read as dead (shadow-partner, 2026-10-27).
pub(super) static TLF_PY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(?:async\s+)?def ([A-Za-z_][A-Za-z0-9_]*)").unwrap());
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
        "scala" | "sc" => extract_scala_imports(content),
        "swift" => extract_swift_imports(content),
        "css" | "scss" | "less" => extract_css_imports(content),
        _ => Vec::new(),
    }
}

// ── Language-specific extractors ─────────────────────────────────────────────

/// Python imports, one logical statement at a time (a parenthesised
/// `from m import (…)` list or a `\` continuation is joined first). The former
/// regexes let `\s` run across newlines, so `import os\nimport sys` was one
/// match and every module after the first line was lost.
///
/// `from m import a, b` yields `m` and the candidates `m.a`, `m.b`, because
/// `a` may be a submodule (`from api import auth`, `from . import ledger`);
/// the resolver keeps a candidate only when that file exists.
pub(super) fn extract_python_imports(content: &str) -> Vec<String> {
    python_import_statements(content)
        .into_iter()
        .flat_map(|statement| statement.modules)
        .collect()
}

/// One Python import statement.
pub(crate) struct PythonImport {
    /// `import a, b` → `a`, `b`; `from m import x, y` → `m`, then the
    /// submodule candidates `m.x`, `m.y`.
    pub(crate) modules: Vec<String>,
    /// A `from m import …` statement: `modules[1..]` are the name candidates.
    pub(crate) from_names: bool,
    pub(crate) timing: ImportTiming,
}

/// When an import binds at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportTiming {
    /// When the module loads.
    Eager,
    /// When the enclosing code runs: a Python import inside a function body, a
    /// JS dynamic `import()`. Such an edge cannot fail a module load.
    Deferred,
    /// Never: `import type`, Python `if TYPE_CHECKING:`.
    Erased,
}

/// Every Python import statement with its [`ImportTiming`]. Block nesting is
/// read from indentation, tracked as a stack: an import under `def` is
/// deferred, under `if TYPE_CHECKING:` erased, anywhere else (module level,
/// `class`, `try`, plain `if`) eager. A misread can only move an import
/// towards eager.
pub(crate) fn python_import_statements(content: &str) -> Vec<PythonImport> {
    const BLOCK_KEYWORDS: &[&str] = &[
        "async", "class", "def", "elif", "else", "except", "finally", "for", "if", "try", "while",
        "with",
    ];
    // Statements that never sit inside brackets: such a line ends any
    // bracket run, so a miscounted bracket cannot hide the imports after it.
    const STATEMENT_KEYWORDS: &[&str] = &[
        "async", "class", "def", "elif", "except", "finally", "from", "import", "try", "while",
        "with",
    ];
    let mut statements = Vec::new();
    let mut blocks: Vec<(usize, Option<ImportTiming>)> = Vec::new();
    // Open brackets of a statement spanning lines (a wrapped `def f(\n a,\n):`
    // signature, a long call): its continuation lines leave `blocks` alone.
    let mut open_brackets = 0i32;
    let mut triple_quote: Option<&'static [u8]> = None;
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let in_docstring = triple_quote.is_some();
        let brackets = python_bracket_delta(line, &mut triple_quote);
        let mut statement = strip_python_comment(line).trim().to_owned();
        if statement.is_empty() {
            continue;
        }
        let keyword = statement
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default();
        let starts_statement = STATEMENT_KEYWORDS.contains(&keyword);
        if (open_brackets > 0 && !starts_statement) || (in_docstring && !starts_statement) {
            open_brackets = (open_brackets + brackets).max(0);
            continue;
        }
        open_brackets = 0;
        let indent = line.len() - line.trim_start().len();
        // A docstring line is prose: it may name a keyword but opens no block.
        if !in_docstring {
            while blocks.last().is_some_and(|&(open, _)| open >= indent) {
                blocks.pop();
            }
        }
        // A one-line block (`if TYPE_CHECKING: import b`) has its body after
        // the header's colon; that body is read below, inside the block.
        let mut inline_block = None;
        if !in_docstring && BLOCK_KEYWORDS.contains(&keyword) {
            let (header, body) = match python_top_level(&statement, b':').first() {
                Some(&colon) => (&statement[..colon], statement[colon + 1..].trim()),
                None => (statement.as_str(), ""),
            };
            let timing = if keyword == "def"
                || python_keyword_rest(header, "async")
                    .is_some_and(|rest| python_keyword_rest(rest.trim_start(), "def").is_some())
            {
                Some(ImportTiming::Deferred)
            } else if python_keyword_rest(header, "if")
                .is_some_and(|condition| is_type_checking_flag(condition.trim()))
            {
                Some(ImportTiming::Erased)
            } else {
                None
            };
            if body.is_empty() {
                blocks.push((indent, timing));
                open_brackets = brackets.max(0);
                continue;
            }
            statement = body.to_owned();
            inline_block = timing;
        }
        if !python_top_level_statements(&statement).any(|simple| {
            python_keyword_rest(simple, "import").is_some()
                || python_keyword_rest(simple, "from").is_some()
        }) {
            open_brackets = brackets.max(0);
            continue;
        }
        let enclosing = || {
            blocks
                .iter()
                .map(|&(_, timing)| timing)
                .chain([inline_block])
        };
        let timing = if enclosing().any(|timing| timing == Some(ImportTiming::Erased)) {
            ImportTiming::Erased
        } else if enclosing().any(|timing| timing == Some(ImportTiming::Deferred)) {
            ImportTiming::Deferred
        } else {
            ImportTiming::Eager
        };
        loop {
            // Brackets inside string literals do not open a continuation:
            // `import os; LABEL = "("` ends on its line.
            let continued = statement.ends_with('\\');
            if !continued && python_bracket_delta(&statement, &mut None) <= 0 {
                break;
            }
            if continued {
                statement.pop();
            }
            let Some(next) = lines.next() else {
                break;
            };
            statement.push(' ');
            statement.push_str(strip_python_comment(next).trim());
        }
        statements.extend(
            python_top_level_statements(&statement).filter_map(|simple| {
                let (modules, from_names) = python_import_modules(simple)?;
                Some(PythonImport {
                    modules,
                    from_names,
                    timing,
                })
            }),
        );
    }
    statements
}

/// `(modules, from_names)` of one simple statement when it is an import; see
/// [`PythonImport`].
fn python_import_modules(statement: &str) -> Option<(Vec<String>, bool)> {
    if let Some(rest) = python_keyword_rest(statement, "import") {
        let modules = rest
            .split(',')
            .filter_map(|item| item.split_whitespace().next())
            .filter(|module| is_python_module_path(module))
            .map(str::to_owned)
            .collect();
        return Some((modules, false));
    }
    let (module, names) = split_from_import(python_keyword_rest(statement, "from")?)?;
    let module = module.trim();
    if !is_python_module_path(module) {
        return None;
    }
    let mut modules = vec![module.to_owned()];
    let separator = if module.ends_with('.') { "" } else { "." };
    let names = names.trim().trim_start_matches('(').trim_end_matches(')');
    for item in names.split(',') {
        if let Some(name) = item.split_whitespace().next()
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !name.is_empty()
        {
            modules.push(format!("{module}{separator}{name}"));
        }
    }
    Some((modules, true))
}

/// The simple statements of one logical line: `import b; VALUE = 1` is two.
fn python_top_level_statements(line: &str) -> impl Iterator<Item = &str> {
    let mut start = 0;
    python_top_level(line, b';')
        .into_iter()
        .chain([line.len()])
        .map(move |end| {
            let simple = line[start..end].trim();
            start = end + 1;
            simple
        })
}

/// Byte offsets of `target` in `line` outside string literals and brackets —
/// the `;` between simple statements, the `:` ending a block header (never
/// the `:=` of an assignment expression). Compares bytes, like
/// [`python_bracket_delta`].
fn python_top_level(line: &str, target: u8) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            byte if byte == target
                && depth == 0
                && !(target == b':' && bytes.get(i + 1) == Some(&b'=')) =>
            {
                found.push(i);
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// Bracket balance of one Python line outside string literals and comments;
/// `triple_quote` carries an open `"""`/`'''` string across lines. Brackets in
/// prose (a docstring's «(see §3») used to be counted and held every later
/// line — imports included — as a continuation. Compares bytes: every byte it
/// looks for is ASCII, which never occurs inside a multi-byte UTF-8 character.
fn python_bracket_delta(line: &str, triple_quote: &mut Option<&'static [u8]>) -> i32 {
    let bytes = line.as_bytes();
    let mut delta = 0;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(quote) = *triple_quote {
            if bytes[i..].starts_with(quote) {
                *triple_quote = None;
                i += quote.len();
            } else {
                i += 1;
            }
            continue;
        }
        match bytes[i] {
            b'#' => break,
            b'"' | b'\'' => {
                let triple: &'static [u8] = if bytes[i] == b'"' { b"\"\"\"" } else { b"'''" };
                if bytes[i..].starts_with(triple) {
                    *triple_quote = Some(triple);
                    i += triple.len();
                    continue;
                }
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'(' | b'[' | b'{' => {
                delta += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                delta -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    delta
}

/// The whole condition is `TYPE_CHECKING` or a dotted reference to it
/// (`typing.TYPE_CHECKING`). `not typing.TYPE_CHECKING` or
/// `enabled or typing.TYPE_CHECKING` also end with the flag but run at runtime.
fn is_type_checking_flag(condition: &str) -> bool {
    condition
        .rsplit_once('.')
        .map_or(condition == "TYPE_CHECKING", |(qualifier, flag)| {
            flag == "TYPE_CHECKING"
                && qualifier.split('.').all(|part| {
                    part.chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                        && part.chars().all(|c| c.is_alphanumeric() || c == '_')
                })
        })
}

/// The text after `keyword` when `statement` starts with it as a whole word.
fn python_keyword_rest<'a>(statement: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = statement.strip_prefix(keyword)?;
    rest.starts_with(char::is_whitespace).then_some(rest)
}

/// `(module, names)` of a `from module import names` remainder. The keyword
/// must stand alone between whitespace (a tab counts) and the names: `from
/// importlib import x` is not split inside `importlib`.
fn split_from_import(rest: &str) -> Option<(&str, &str)> {
    rest.match_indices("import").find_map(|(at, keyword)| {
        let names = &rest[at + keyword.len()..];
        (rest[..at].ends_with(char::is_whitespace)
            && names.starts_with(|c: char| c.is_whitespace() || c == '('))
        .then(|| (&rest[..at], names))
    })
}

/// `line` up to its comment: the first `#` outside a string literal, so
/// `LABEL = "#"; import b` keeps its import.
fn strip_python_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'#' => return &line[..i],
            quote @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            _ => {}
        }
        i += 1;
    }
    line
}

/// `pkg.mod`, `.mod`, `..` — rejects prose such as a docstring's
/// «from the docs import this».
fn is_python_module_path(module: &str) -> bool {
    !module.is_empty()
        && module
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

pub(super) fn extract_js_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for regex in [
        &*JS_IMPORT_FROM_RE,
        &*JS_IMPORT_SIDE_EFFECT_RE,
        &*JS_REQUIRE_RE,
        &*JS_DYNAMIC_IMPORT_RE,
        &*JS_REEXPORT_FROM_RE,
    ] {
        for capture in regex.captures_iter(content) {
            let Some(module) = capture.name("module") else {
                continue;
            };
            imports.push(module.as_str().trim().to_owned());
        }
    }
    imports
}

/// The whole clause of an `import type …` / `export type …` statement:
/// `type X`, `type { A, B as C }`, `type * as ns`, `type *`.
///
/// Anchored on both ends because `[^;]+?` in the shared regexes runs across
/// lines in semicolon-free files: `export type T = {…}` followed by
/// `export { x } from "./m"` is captured as ONE clause. Such a run-on must stay
/// a value import, so the clause has to be exactly one type-only form.
static JS_TYPE_ONLY_CLAUSE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^type(?:\s*\{[^{}]*\}|\s*\*(?:\s+as\s+[A-Za-z_$][\w$]*)?|\s+[A-Za-z_$][\w$]*)$")
        .unwrap()
});

/// True for `import type …` / `export type …` clauses, which TypeScript (and
/// Flow) erase in every compiler mode, so the target module is never loaded.
///
/// Inline-only braces (`import { type A } from "m"`) are NOT type-only here:
/// under `verbatimModuleSyntax` they compile to `import "m"`, a real load.
/// `import type from "m"` (a default binding named `type`) and
/// `import type, { A } from "m"` are value imports too.
pub(crate) fn is_type_only_js_clause(clause: &str) -> bool {
    JS_TYPE_ONLY_CLAUSE_RE.is_match(clause.trim())
}

/// [`extract_js_imports`] with each specifier's [`ImportTiming`]: `import type`
/// is erased, a dynamic `import()` inside a function deferred, everything else
/// eager. A module named under several timings binds at the earliest; callers
/// combine them.
pub(crate) fn js_imports_by_timing(path: &Path, content: &str) -> Vec<(String, ImportTiming)> {
    let mut imports = Vec::new();
    for regex in [&*JS_IMPORT_FROM_RE, &*JS_REEXPORT_FROM_RE] {
        for capture in regex.captures_iter(content) {
            let Some(module) = capture.name("module") else {
                continue;
            };
            let timing = if capture
                .name("clause")
                .is_some_and(|clause| is_type_only_js_clause(clause.as_str()))
            {
                ImportTiming::Erased
            } else {
                ImportTiming::Eager
            };
            imports.push((module.as_str().trim().to_owned(), timing));
        }
    }
    for regex in [&*JS_IMPORT_SIDE_EFFECT_RE, &*JS_REQUIRE_RE] {
        for capture in regex.captures_iter(content) {
            if let Some(module) = capture.name("module") {
                imports.push((module.as_str().trim().to_owned(), ImportTiming::Eager));
            }
        }
    }
    // A module-level `await import()` blocks the module's own evaluation, so
    // only a call proven to sit inside a function is deferred.
    let mut functions = None;
    for capture in JS_DYNAMIC_IMPORT_RE.captures_iter(content) {
        let (Some(call), Some(module)) = (capture.get(0), capture.name("module")) else {
            continue;
        };
        let functions = functions.get_or_insert_with(|| js_function_ranges(path, content));
        let timing = if functions
            .iter()
            .any(|function| function.contains(&call.start()))
        {
            ImportTiming::Deferred
        } else {
            ImportTiming::Eager
        };
        imports.push((module.as_str().trim().to_owned(), timing));
    }
    imports
}

/// Byte ranges of the code in a JS/TS file that runs when a function is
/// called, not when the module loads: the parameters and body of every
/// declaration, expression, arrow and method. A method's computed name
/// (`[await import('./a')]() {}`) runs when the class is defined, so it is not
/// included. Empty when the file does not parse.
fn js_function_ranges(path: &Path, content: &str) -> Vec<std::ops::Range<usize>> {
    let Some(config) = crate::lang_config::language_for_path(path) else {
        return Vec::new();
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&config.language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content, None) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "function_declaration"
                | "function_expression"
                | "function"
                | "generator_function_declaration"
                | "generator_function"
                | "arrow_function"
                | "method_definition"
        ) {
            ranges.extend(
                ["parameters", "parameter", "body"]
                    .into_iter()
                    .filter_map(|field| node.child_by_field_name(field))
                    .map(|part| part.byte_range()),
            );
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    ranges
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

/// Same extraction as `extract_rust_imports`, but each import is paired with the
/// byte offset of the regex match that produced it. Used by `super::cfg_test` to
/// drop imports that sit inside a `#[cfg(test)]`-gated item.
pub(super) fn rust_imports_with_offsets(content: &str) -> Vec<(String, usize)> {
    let mut imports = Vec::new();

    for cap in RS_MOD_RE.captures_iter(content) {
        let Some(whole) = cap.get(0) else { continue };
        if let Some(m) = cap.get(1) {
            imports.push((m.as_str().to_owned(), whole.start()));
        }
    }

    for cap in RS_USE_RE.captures_iter(content) {
        let Some(whole) = cap.get(0) else { continue };
        let start = whole.start();
        let base = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(brace) = cap.get(2) {
            for item in brace.as_str().split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    imports.push((format!("{base}::{item}"), start));
                }
            }
        } else if !base.is_empty() {
            imports.push((base.to_owned(), start));
        }
    }
    imports
}

pub(super) fn extract_rust_imports(content: &str) -> Vec<String> {
    rust_imports_with_offsets(content)
        .into_iter()
        .map(|(module, _)| module)
        .collect()
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

// ── Scala ────────────────────────────────────────────────────────────────────
static SCALA_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*import\s+([A-Za-z0-9_\.]+)").unwrap());

fn extract_scala_imports(content: &str) -> Vec<String> {
    SCALA_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_owned()))
        .collect()
}

// ── Swift ────────────────────────────────────────────────────────────────────
static SWIFT_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*import\s+(?:class\s+|struct\s+|enum\s+|protocol\s+|func\s+)?([A-Za-z0-9_\.]+)",
    )
    .unwrap()
});

fn extract_swift_imports(content: &str) -> Vec<String> {
    SWIFT_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_owned()))
        .collect()
}

// ── CSS/SCSS ─────────────────────────────────────────────────────────────────
static CSS_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)@import\s+(?:url\()?["']([^"']+)["']\)?"#).unwrap());

fn extract_css_imports(content: &str) -> Vec<String> {
    CSS_IMPORT_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_owned()))
        .collect()
}

// ── extract_imports_for_file (public wrapper) ────────────────────────────────

/// Extract raw import strings from a file. Public for use by the indexer.
pub fn extract_imports_for_file(path: &Path) -> Vec<String> {
    extract_imports(path)
}

// ── collect_top_level_funcs ──────────────────────────────────────────────────

/// Lightweight regex-based top-level function name extractor.
/// Fills `funcs` map with (name -> line_number). Does not overwrite existing entries.
pub(crate) fn collect_top_level_funcs(
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
                // Callers seed names from the call graph with line 0; a
                // definition line found here must replace that placeholder or
                // decorator and `#[test]` checks never see these functions.
                let entry = funcs.entry(name).or_insert(line);
                if *entry == 0 {
                    *entry = line;
                }
            }
        }
    }
}
