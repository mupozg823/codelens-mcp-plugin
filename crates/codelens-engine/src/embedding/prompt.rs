use super::runtime::parse_bool_env;

/// Split CamelCase/snake_case into space-separated words for embedding matching.
/// "getDonationRankings" → "get Donation Rankings"
/// "build_non_code_ranges" → "build non code ranges"
pub fn split_identifier(name: &str) -> String {
    // Only split if name is CamelCase or snake_case with multiple segments
    if !name.contains('_') && !name.chars().any(|c| c.is_uppercase()) {
        return name.to_string();
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else if ch.is_uppercase()
            && !current.is_empty()
            && (current
                .chars()
                .last()
                .is_some_and(|c| c.is_lowercase())
                || chars.get(i + 1).is_some_and(|c| c.is_lowercase()))
        {
            // Split at CamelCase boundary, but not for ALL_CAPS
            words.push(current.clone());
            current.clear();
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    if words.len() <= 1 {
        return name.to_string(); // No meaningful split
    }
    words.join(" ")
}

pub fn is_test_only_symbol(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> bool {
    let fp = &sym.file_path;

    // ── Path-based detection (language-agnostic) ─────────────────────
    // Rust
    if fp.contains("/tests/") || fp.ends_with("_tests.rs") {
        return true;
    }
    // JS/TS — Jest __tests__ directory
    if fp.contains("/__tests__/") || fp.contains("\\__tests__\\") {
        return true;
    }
    // Python
    if fp.ends_with("_test.py") {
        return true;
    }
    // Go
    if fp.ends_with("_test.go") {
        return true;
    }
    // JS/TS — .test.* / .spec.*
    if fp.ends_with(".test.ts")
        || fp.ends_with(".test.tsx")
        || fp.ends_with(".test.js")
        || fp.ends_with(".test.jsx")
        || fp.ends_with(".spec.ts")
        || fp.ends_with(".spec.js")
    {
        return true;
    }
    // Java/Kotlin — Maven src/test/ layout
    if fp.contains("/src/test/") {
        return true;
    }
    // Java — *Test.java / *Tests.java
    if fp.ends_with("Test.java") || fp.ends_with("Tests.java") {
        return true;
    }
    // Ruby
    if fp.ends_with("_test.rb") || fp.contains("/spec/") {
        return true;
    }

    // ── Rust name_path patterns ───────────────────────────────────────
    if sym.name_path.starts_with("tests::")
        || sym.name_path.contains("::tests::")
        || sym.name_path.starts_with("test::")
        || sym.name_path.contains("::test::")
    {
        return true;
    }

    let Some(source) = source else {
        return false;
    };

    let start = usize::try_from(sym.start_byte.max(0))
        .unwrap_or(0)
        .min(source.len());

    // ── Source-based: Rust attributes ────────────────────────────────
    let window_start = start.saturating_sub(2048);
    let attrs = String::from_utf8_lossy(&source.as_bytes()[window_start..start]);
    if attrs.contains("#[test]")
        || attrs.contains("#[tokio::test]")
        || attrs.contains("#[cfg(test)]")
        || attrs.contains("#[cfg(all(test")
    {
        return true;
    }

    // ── Source-based: Python ─────────────────────────────────────────
    // Function names starting with `test_` or class names starting with `Test`
    if fp.ends_with(".py") {
        if sym.name.starts_with("test_") {
            return true;
        }
        // Class whose name starts with "Test" — also matches TestCase subclasses
        if sym.kind == "class" && sym.name.starts_with("Test") {
            return true;
        }
    }

    // ── Source-based: Go ─────────────────────────────────────────────
    // func TestXxx(...) pattern; file must end with _test.go (already caught above),
    // but guard on .go extension for any edge-case non-test files with Test* helpers.
    if fp.ends_with(".go") && sym.name.starts_with("Test") && sym.kind == "function" {
        return true;
    }

    // ── Source-based: Java / Kotlin ──────────────────────────────────
    if fp.ends_with(".java") || fp.ends_with(".kt") {
        let before = &source[..start];
        let window = if before.len() > 200 {
            &before[before.len() - 200..]
        } else {
            before
        };
        if window.contains("@Test")
            || window.contains("@ParameterizedTest")
            || window.contains("@RepeatedTest")
        {
            return true;
        }
    }

    false
}

/// Build the embedding text for a symbol.
///
/// Optimized for MiniLM-L12-CodeSearchNet:
/// - No "passage:" prefix (model not trained with prefixes)
/// - Include file context for disambiguation
/// - Signature-focused (body inclusion hurts quality for this model)
///
/// When `CODELENS_EMBED_DOCSTRINGS=1` is set, leading docstrings/comments are
/// appended. Disabled by default because the bundled CodeSearchNet-INT8 model
/// is optimized for code signatures and dilutes on natural language text.
/// Enable when switching to a hybrid code+text model (E5-large, BGE-base, etc).
pub fn build_embedding_text(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> String {
    // File context: use only the filename (not full path) to reduce noise.
    // Full paths like "crates/codelens-engine/src/symbols/mod.rs" add tokens
    // that dilute the semantic signal. "mod.rs" is sufficient context.
    let file_ctx = if sym.file_path.is_empty() {
        String::new()
    } else {
        let filename = sym.file_path.rsplit('/').next().unwrap_or(&sym.file_path);
        format!(" in {}", filename)
    };

    // Include split identifier words for better NL matching
    // e.g. "getDonationRankings" → "get Donation Rankings"
    let split_name = split_identifier(&sym.name);
    let name_with_split = if split_name != sym.name {
        format!("{} ({})", sym.name, split_name)
    } else {
        sym.name.clone()
    };

    // Add parent context from name_path (e.g. "UserService/get_user" → "in UserService")
    let parent_ctx = if !sym.name_path.is_empty() && sym.name_path.contains('/') {
        let parent = sym.name_path.rsplit_once('/').map_or("", |x| x.0);
        if parent.is_empty() {
            String::new()
        } else {
            format!(" (in {})", parent)
        }
    } else {
        String::new()
    };

    // Module context: directory name provides domain signal without full path noise.
    // "embedding/mod.rs" → module "embedding", "symbols/ranking.rs" → module "symbols"
    let module_ctx = if sym.file_path.contains('/') {
        let parts: Vec<&str> = sym.file_path.rsplitn(3, '/').collect();
        if parts.len() >= 2 {
            let dir = parts[1];
            // Skip generic dirs like "src"
            if dir != "src" && dir != "crates" {
                format!(" [{dir}]")
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let base = if sym.signature.is_empty() {
        format!(
            "{} {}{}{}{}",
            sym.kind, name_with_split, parent_ctx, module_ctx, file_ctx
        )
    } else {
        format!(
            "{} {}{}{}{}: {}",
            sym.kind, name_with_split, parent_ctx, module_ctx, file_ctx, sym.signature
        )
    };

    // Docstring inclusion: v2 model improved NL understanding (+45%), enabling
    // docstrings by default. Measured: ranked_context +0.020, semantic -0.003 (neutral).
    // Disable via CODELENS_EMBED_DOCSTRINGS=0 if needed.
    let docstrings_disabled = std::env::var("CODELENS_EMBED_DOCSTRINGS")
        .is_ok_and(|v| v == "0" || v == "false");

    if docstrings_disabled {
        return base;
    }

    let docstring = source
        .and_then(|src| extract_leading_doc(src, sym.start_byte as usize, sym.end_byte as usize))
        .unwrap_or_default();

    let mut text = if docstring.is_empty() {
        // Fallback: extract the first few meaningful lines from the function
        // body. This captures key API calls (e.g. "tree_sitter::Parser",
        // "stdin()") that help the embedding model match NL queries to
        // symbols without docs.
        let body_hint = source
            .and_then(|src| extract_body_hint(src, sym.start_byte as usize, sym.end_byte as usize))
            .unwrap_or_default();
        if body_hint.is_empty() {
            base
        } else {
            format!("{} — {}", base, body_hint)
        }
    } else {
        // Collect up to hint_line_budget() non-empty docstring lines
        // (rather than only the first) so the embedding model sees
        // multi-sentence explanations in full — up to the runtime
        // char budget via join_hint_lines.
        let line_budget = hint_line_budget();
        let lines: Vec<String> = docstring
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(line_budget)
            .map(str::to_string)
            .collect();
        let hint = join_hint_lines(&lines);
        if hint.is_empty() {
            base
        } else {
            format!("{} — {}", base, hint)
        }
    };

    // v1.5 Phase 2b experiment: optionally append NL tokens harvested from
    // comments and string literals inside the body. Disabled by default;
    // enable with `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` to A/B.
    if let Some(src) = source
        && let Some(nl_tokens) =
            extract_nl_tokens(src, sym.start_byte as usize, sym.end_byte as usize)
        && !nl_tokens.is_empty()
    {
        text.push_str(" · NL: ");
        text.push_str(&nl_tokens);
    }

    // v1.5 Phase 2c experiment: optionally append `Type::method` call-site
    // hints harvested from the body. Disabled by default; enable with
    // `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` to A/B. Orthogonal to
    // Phase 2b — both can be stacked.
    if let Some(src) = source
        && let Some(api_calls) =
            extract_api_calls(src, sym.start_byte as usize, sym.end_byte as usize)
        && !api_calls.is_empty()
    {
        text.push_str(" · API: ");
        text.push_str(&api_calls);
    }

    text
}

/// Maximum total characters collected from body-hint or docstring lines.
/// Kept conservative to avoid diluting signature signal for the bundled
/// MiniLM-L12-CodeSearchNet INT8 model. Override via
/// `CODELENS_EMBED_HINT_CHARS` for experiments (clamped to 60..=512).
///
/// History: a v1.5 Phase 2 PoC briefly raised this to 180 / 3 lines in an
/// attempt to close the NL query MRR gap. The 2026-04-11 A/B measurement
/// (`benchmarks/embedding-quality-v1.5-hint1` vs `-phase2`) showed
/// `hybrid -0.005`, `NL hybrid -0.008`, `NL semantic_search -0.041`, so
/// the defaults reverted to the pre-PoC values. The infrastructure
/// (`join_hint_lines`, `hint_line_budget`, env overrides) stayed so the
/// next experiment does not need a rewrite.
const DEFAULT_HINT_TOTAL_CHAR_BUDGET: usize = 60;

/// Maximum number of meaningful lines to collect from a function body.
/// Overridable via `CODELENS_EMBED_HINT_LINES` (clamped to 1..=10).
const DEFAULT_HINT_LINES: usize = 1;

pub fn hint_char_budget() -> usize {
    std::env::var("CODELENS_EMBED_HINT_CHARS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .map_or(DEFAULT_HINT_TOTAL_CHAR_BUDGET, |n| n.clamp(60, 512))
}

pub fn hint_line_budget() -> usize {
    std::env::var("CODELENS_EMBED_HINT_LINES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .map_or(DEFAULT_HINT_LINES, |n| n.clamp(1, 10))
}

/// Join collected hint lines, capping at the runtime-configured char
/// budget (default 60 chars; override via `CODELENS_EMBED_HINT_CHARS`).
///
/// Each line is separated by " · " so the embedding model sees a small
/// structural boundary between logically distinct body snippets. The final
/// result is truncated with a trailing "..." on char-boundaries only.
pub fn join_hint_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let joined = lines
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" · ");
    let budget = hint_char_budget();
    if joined.chars().count() > budget {
        let truncated: String = joined.chars().take(budget).collect();
        format!("{truncated}...")
    } else {
        joined
    }
}

/// Extract up to `hint_line_budget()` meaningful lines from a function body
/// (skipping braces, blank lines, and comments). Used as a fallback when no
/// docstring is available so the embedding model still sees the core API
/// calls / return values.
///
/// Historically this returned only the first meaningful line clipped at 60
/// chars. The 180-char / 3-line budget was introduced in v1.5 Phase 2 to
/// close the NL-query gap (MRR 0.528) on cases where the discriminating
/// keyword lives in line 2 or 3 of the body.
pub fn extract_body_hint(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
    let safe_start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let safe_end = end.min(source.len());
    let safe_end = if source.is_char_boundary(safe_end) {
        safe_end
    } else {
        source.floor_char_boundary(safe_end)
    };
    let body = &source[safe_start..safe_end];

    let max_lines = hint_line_budget();
    let mut collected: Vec<String> = Vec::with_capacity(max_lines);

    // Skip past the signature: everything until we see a line ending with '{' or ':'
    // (opening brace of the function body), then start looking for meaningful lines.
    let mut past_signature = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if !past_signature {
            // Keep skipping until we find the opening brace/colon
            if trimmed.ends_with('{') || trimmed.ends_with(':') || trimmed == "{" {
                past_signature = true;
            }
            continue;
        }
        // Skip comments, blank lines, closing braces
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed == "}"
        {
            continue;
        }
        collected.push(trimmed.to_string());
        if collected.len() >= max_lines {
            break;
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(join_hint_lines(&collected))
    }
}

/// Return true when NL-token collection is enabled via
/// `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` (or `true`/`yes`/`on`).
///
/// v1.5 Phase 2b infrastructure — kept off by default pending A/B
/// measurement against the fixed 89-query dataset.
///
/// v1.5 Phase 2j: when no explicit env var is set, fall through to
/// `auto_hint_should_enable()` which consults `CODELENS_EMBED_HINT_AUTO` +
/// `CODELENS_EMBED_HINT_AUTO_LANG` for language-gated defaults.
pub fn nl_tokens_enabled() -> bool {
    if let Some(explicit) = parse_bool_env("CODELENS_EMBED_HINT_INCLUDE_COMMENTS") {
        return explicit;
    }
    auto_hint_should_enable()
}

/// Return true when v1.5 Phase 2j auto-detection mode is enabled.
///
/// **v1.6.0 default change (§8.14)**: this returns `true` by default.
/// Users opt **out** with `CODELENS_EMBED_HINT_AUTO=0` (or `false` /
/// `no` / `off`). The previous v1.5.x behaviour was the other way
/// around — default OFF, opt in with `=1`. The flip ships as part of
/// v1.6.0 after the five-dataset measurement (§8.7, §8.8, §8.13,
/// §8.11, §8.12) validated:
///
/// 1. Rust / C / C++ / Go / Java / Kotlin / Scala / C# projects hit
///    the §8.7 stacked arm (+2.4 % to +15.2 % hybrid MRR).
/// 2. TypeScript / JavaScript projects validated the Phase 2b/2c
///    embedding hints on `facebook/jest` and later `microsoft/typescript`.
///    Subsequent app/runtime follow-ups (`vercel/next.js`,
///    `facebook/react` production subtree) motivated splitting Phase 2e
///    out of the JS/TS auto path, but not removing JS/TS from the
///    embedding-hint default.
/// 3. Python projects hit the §8.8 baseline (no change) — the
///    §8.11 language gate + §8.12 MCP auto-set means Python is
///    auto-detected and the stack stays OFF without user action.
/// 4. Ruby / PHP / Lua / shell / untested-dynamic projects fall
///    through to the conservative default-off branch (same as
///    Python behaviour — no regression).
///
/// The dominant language is supplied by the MCP tool layer via the
/// `CODELENS_EMBED_HINT_AUTO_LANG` env var, which is set
/// automatically on startup (`main.rs`) and on MCP
/// `activate_project` calls by `compute_dominant_language` (§8.12).
/// The engine only reads the env var — it does not walk the
/// filesystem itself.
///
/// Explicit `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` /
/// `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` /
/// `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` (or their `=0` counterparts)
/// always win over the auto decision — users who want to force a
/// configuration still can, the auto mode is a better default, not
/// a lock-in.
///
/// **Opt-out**: set `CODELENS_EMBED_HINT_AUTO=0` to restore v1.5.x
/// behaviour (no auto-detection, all Phase 2 gates default off unless
/// their individual env vars are set).
pub fn auto_hint_mode_enabled() -> bool {
    parse_bool_env("CODELENS_EMBED_HINT_AUTO").unwrap_or(true)
}

/// Return the language tag supplied by the MCP tool layer via
/// `CODELENS_EMBED_HINT_AUTO_LANG`, or `None` when unset. The tag is
/// compared against `language_supports_nl_stack` to decide whether
/// the Phase 2b / 2c / 2e stack should be auto-enabled.
///
/// Accepted tags are the canonical extensions from
/// `crates/codelens-engine/src/lang_config.rs` (`rs`, `py`, `js`,
/// `ts`, `go`, `rb`, `java`, `kt`, `scala`, `cs`, `cpp`, `c`, …) plus
/// a handful of long-form aliases (`rust`, `python`, `javascript`,
/// `typescript`, `golang`) for users who set the env var by hand.
pub fn auto_hint_lang() -> Option<String> {
    std::env::var("CODELENS_EMBED_HINT_AUTO_LANG")
        .ok()
        .map(|raw| raw.trim().to_ascii_lowercase())
}

/// Return true when `lang` is a language where the v1.5 embedding-hint
/// stack (Phase 2b comments + Phase 2c API-call extraction) has been
/// measured to net-positive (§8.2, §8.4, §8.6, §8.7, §8.13, §8.15) or
/// where the language's static typing + snake_case naming + comment-first
/// culture makes the mechanism behave the same way it does on Rust.
///
/// This gate is intentionally separate from the Phase 2e sparse
/// re-ranker. As of the §8.15 / §8.16 / §8.17 follow-up arc, JS/TS stays
/// enabled here because tooling/compiler repos are positive and short-file
/// runtime repos are inert, but JS/TS is disabled in the **sparse**
/// auto-gate because Phase 2e is negative-or-null on that family.
///
/// The list is intentionally conservative — additions require an actual
/// external-repo A/B following the §8.7 methodology, not a
/// language-similarity argument alone.
///
/// **Supported** (measured or by static-typing analogy):
/// - `rs`, `rust` (§8.2, §8.4, §8.6, §8.7: +2.4 %, +7.1 %, +15.2 %)
/// - `cpp`, `cc`, `cxx`, `c++`
/// - `c`
/// - `go`, `golang`
/// - `java`
/// - `kt`, `kotlin`
/// - `scala`
/// - `cs`, `csharp`
/// - `ts`, `typescript`, `tsx` (§8.13: `facebook/jest` +7.3 % hybrid MRR)
/// - `js`, `javascript`, `jsx`
///
/// **Unsupported** (measured regression or untested dynamic-typed):
/// - `py`, `python` (§8.8 regression)
/// - `rb`, `ruby`
/// - `php`
/// - `lua`, `r`, `jl`
/// - `sh`, `bash`
/// - anything else
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

/// Return true when `lang` is a language where the Phase 2e sparse
/// coverage re-ranker should be auto-enabled when the user has not set
/// `CODELENS_RANK_SPARSE_TERM_WEIGHT` explicitly.
///
/// This is deliberately narrower than `language_supports_nl_stack`.
/// Phase 2e remains positive on Rust-style codebases, but the JS/TS
/// measurement arc now says:
///
/// - `facebook/jest`: marginal positive
/// - `microsoft/typescript`: negative
/// - `vercel/next.js`: slight negative
/// - `facebook/react` production subtree: exact no-op
///
/// So the conservative Phase 2m policy is:
/// - keep Phase 2b/2c auto-eligible on JS/TS
/// - disable **auto** Phase 2e on JS/TS
/// - preserve explicit env override for users who want to force it on
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

/// Combined decision: Phase 2j auto mode is enabled AND the detected
/// language supports the Phase 2b/2c embedding-hint stack. This is the
/// `else` branch that `nl_tokens_enabled` and `api_calls_enabled` fall
/// through to when no explicit env var is set.
pub fn auto_hint_should_enable() -> bool {
    if !auto_hint_mode_enabled() {
        return false;
    }
    match auto_hint_lang() {
        Some(lang) => language_supports_nl_stack(&lang),
        None => false, // auto mode on but no language tag → conservative OFF
    }
}

/// Combined decision: Phase 2j auto mode is enabled AND the detected
/// language supports auto-enabling the Phase 2e sparse re-ranker.
///
/// This intentionally differs from `auto_hint_should_enable()` after the
/// §8.15 / §8.16 / §8.17 JS/TS follow-up arc: embedding hints stay
/// auto-on for JS/TS, but sparse weighting does not.
pub fn auto_sparse_should_enable() -> bool {
    if !auto_hint_mode_enabled() {
        return false;
    }
    match auto_hint_lang() {
        Some(lang) => language_supports_sparse_weighting(&lang),
        None => false,
    }
}

/// Heuristic: does this string look like natural language rather than
/// a code identifier, path, or numeric literal?
///
/// Criteria:
/// - at least 4 characters
/// - no path / scope separators (`/`, `\`, `::`)
/// - must contain a space (multi-word)
/// - alphabetic character ratio >= 60%
pub fn is_nl_shaped(s: &str) -> bool {
    let s = s.trim();
    if s.chars().count() < 4 {
        return false;
    }
    if s.contains('/') || s.contains('\\') || s.contains("::") {
        return false;
    }
    if !s.contains(' ') {
        return false;
    }
    let non_ws: usize = s.chars().filter(|c| !c.is_whitespace()).count();
    if non_ws == 0 {
        return false;
    }
    let alpha: usize = s.chars().filter(|c| c.is_alphabetic()).count();
    (alpha * 100) / non_ws >= 60
}

/// Return true when the v1.5 Phase 2i strict comment filter is enabled
/// via `CODELENS_EMBED_HINT_STRICT_COMMENTS=1` (or `true`/`yes`/`on`).
///
/// Phase 2i extends Phase 2h (§8.9) with a comment-side analogue of the
/// literal filter. Phase 2h recovered ~8 % of the Python regression by
/// rejecting format/error/log string literals in Pass 2; Phase 2i
/// targets the remaining ~92 % by rejecting meta-annotation comments
/// (`# TODO`, `# FIXME`, `# HACK`, `# XXX`, `# BUG`, `# REVIEW`,
/// `# REFACTOR`, `# TEMP`, `# DEPRECATED`) in Pass 1. Conservative
/// prefix list — `# NOTE`, `# WARN`, `# SAFETY` are retained because
/// they often carry behaviour-descriptive content even on Rust.
///
/// Default OFF (same policy as every Phase 2 knob). Orthogonal to
/// `CODELENS_EMBED_HINT_STRICT_LITERALS` so both may be stacked.
pub fn strict_comments_enabled() -> bool {
    std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS")
        .is_ok_and(|raw| {
            let lowered = raw.to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
}

/// Heuristic: does `body` (the comment text *after* the `//` / `#` prefix
/// has been stripped by `extract_comment_body`) look like a meta-annotation
/// rather than behaviour-descriptive prose?
///
/// Recognises the following prefixes (case-insensitive, followed by
/// `:`, `(`, or whitespace):
/// - `TODO`, `FIXME`, `HACK`, `XXX`, `BUG`
/// - `REVIEW`, `REFACTOR`, `TEMP`, `TEMPORARY`, `DEPRECATED`
///
/// Deliberately excluded (kept as behaviour signal):
/// - `NOTE`, `NOTES`, `WARN`, `WARNING`
/// - `SAFETY` (Rust `unsafe` block justifications)
/// - `PANIC` (Rust invariant docs)
///
/// The exclusion list is based on the observation that Rust projects
/// use `// SAFETY:` and `// NOTE:` to document *why* a block behaves a
/// certain way — that text is exactly the NL retrieval signal Phase 2b
/// is trying to capture. The inclusion list targets the "I'll fix this
/// later" noise that poisons the embedding on both languages but is
/// especially common on mature Python projects.
pub fn looks_like_meta_annotation(body: &str) -> bool {
    let trimmed = body.trim_start();
    // Find the end of the first "word" (alphanumerics only — a colon,
    // paren, or whitespace terminates the marker).
    let word_end = trimmed
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(trimmed.len());
    if word_end == 0 {
        return false;
    }
    let first_word = &trimmed[..word_end];
    let upper = first_word.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "TODO"
            | "FIXME"
            | "HACK"
            | "XXX"
            | "BUG"
            | "REVIEW"
            | "REFACTOR"
            | "TEMP"
            | "TEMPORARY"
            | "DEPRECATED"
    )
}

/// Return true when the v1.5 Phase 2h strict NL literal filter is enabled
/// via `CODELENS_EMBED_HINT_STRICT_LITERALS=1` (or `true`/`yes`/`on`).
///
/// Phase 2h addresses the Phase 3b Python regression (§8.8). The default
/// Phase 2b Pass 2 scanner accepts any `is_nl_shaped` string literal from
/// the body, which on Python captures a lot of generic error / log / format
/// strings (`raise ValueError("Invalid URL %s" % url)`, `logging.debug(...)`,
/// `fmt.format(...)`). These pass the NL-shape test but carry zero
/// behaviour-descriptive signal and pollute the embedding. The strict
/// filter rejects string literals that look like format templates or
/// common error / log prefixes, while leaving comments (Pass 1) untouched.
///
/// Default OFF (same policy as every Phase 2 knob — opt-in first,
/// measure, then consider flipping the default).
pub fn strict_literal_filter_enabled() -> bool {
    std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS")
        .is_ok_and(|raw| {
            let lowered = raw.to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
}

/// Heuristic: does `s` contain a C / Python / Rust format specifier?
///
/// Recognises:
/// - C / Python `%` style: `%s`, `%d`, `%r`, `%f`, `%x`, `%o`, `%i`, `%u`
/// - Python `.format` / f-string style: `{name}`, `{0}`, `{:fmt}`, `{name:fmt}`
///
/// Rust `format!` / `println!` style `{}` / `{:?}` / `{name}` is caught by
/// the same `{...}` branch. Generic `{...}` braces used for JSON-like
/// content (e.g. `"{name: foo, id: 1}"`) are distinguished from format
/// placeholders by requiring the inside to be either empty, prefix-colon
/// (`:fmt`), a single identifier, or an identifier followed by `:fmt`.
pub fn contains_format_specifier(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 1 < len {
        if bytes[i] == b'%' {
            let next = bytes[i + 1];
            if matches!(next, b's' | b'd' | b'r' | b'f' | b'x' | b'o' | b'i' | b'u') {
                return true;
            }
        }
        i += 1;
    }
    // Python `.format` / f-string / Rust `format!` style `{...}`
    //
    // Real format placeholders never contain whitespace inside the braces:
    // `{}`, `{0}`, `{name}`, `{:?}`, `{:.2f}`, `{name:fmt}`. JSON-like
    // content such as `{name: foo, id: 1}` DOES contain whitespace. The
    // whitespace check is therefore the single simplest and most robust
    // way to distinguish the two without a full format-spec parser.
    for window in s.split('{').skip(1) {
        let Some(close_idx) = window.find('}') else {
            continue;
        };
        let inside = &window[..close_idx];
        // `{}` — Rust empty placeholder
        if inside.is_empty() {
            return true;
        }
        // Any whitespace inside the braces → JSON-like, not a format spec.
        if inside.chars().any(|c| c.is_whitespace()) {
            continue;
        }
        // `{:fmt}` — anonymous format spec
        if inside.starts_with(':') {
            return true;
        }
        // `{name}`, `{0}`, `{name:fmt}` — identifier (or digit), optionally
        // followed by `:fmt`. We already rejected whitespace-containing
        // inputs above, so here we only need to check the identifier chars.
        let ident_end = inside.find(':').unwrap_or(inside.len());
        let ident = &inside[..ident_end];
        if !ident.is_empty()
            && ident
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return true;
        }
    }
    false
}

/// Heuristic: does `s` look like a generic error message, log line, or
/// low-value imperative string that an NL query would never try to match?
///
/// The prefix list is intentionally short — covering the patterns the
/// Phase 3b `psf/requests` post-mortem flagged as the largest regression
/// sources. False negatives (real behaviour strings misclassified as
/// errors) would cost retrieval quality, but because the filter only
/// runs on string literals and leaves comments alone, a missed NL string
/// in one symbol will typically have a comment covering the same
/// behaviour on the same symbol.
pub fn looks_like_error_or_log_prefix(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    const PREFIXES: &[&str] = &[
        "invalid ",
        "cannot ",
        "could not ",
        "unable to ",
        "failed to ",
        "expected ",
        "unexpected ",
        "missing ",
        "not found",
        "error: ",
        "error ",
        "warning: ",
        "warning ",
        "sending ",
        "received ",
        "starting ",
        "stopping ",
        "calling ",
        "connecting ",
        "disconnecting ",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Test-only variant: bypass the env gate so the unit tests can exercise
/// the filter logic deterministically (mirrors `extract_nl_tokens_inner`
/// vs `extract_nl_tokens` policy). Inlined here instead of a `#[cfg(test)]`
/// helper so the release binary path never calls it.
#[cfg(test)]
pub fn should_reject_literal_strict(s: &str) -> bool {
    contains_format_specifier(s) || looks_like_error_or_log_prefix(s)
}

/// Collect natural-language tokens from a function body: line comments,
/// block comments, and string literals that look like NL prose.
///
/// v1.5 Phase 2b experiment. The hypothesis is that the bundled
/// CodeSearchNet-INT8 model struggles with NL queries (hybrid MRR 0.472)
/// because the symbol text it sees is pure code, whereas NL queries target
/// behavioural descriptions that live in *comments* and *string literals*.
///
/// Unlike `extract_body_hint` (which skips comments) this function only
/// keeps comments + NL-shaped string literals and ignores actual code.
///
/// Gated by `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1`. Returns `None` when
/// the gate is off so the default embedding text is untouched.
pub fn extract_nl_tokens(source: &str, start: usize, end: usize) -> Option<String> {
    if !nl_tokens_enabled() {
        return None;
    }
    extract_nl_tokens_inner(source, start, end)
}

/// Env-independent core of `extract_nl_tokens`, exposed to the test module
/// so unit tests can run deterministically without touching env vars
/// (which would race with the other tests that set
/// `CODELENS_EMBED_HINT_INCLUDE_COMMENTS`).
pub fn extract_nl_tokens_inner(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
    let safe_start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let safe_end = end.min(source.len());
    let safe_end = if source.is_char_boundary(safe_end) {
        safe_end
    } else {
        source.floor_char_boundary(safe_end)
    };
    let body = &source[safe_start..safe_end];

    let mut tokens: Vec<String> = Vec::new();

    // ── Pass 1: comments ─────────────────────────────────────────────
    // v1.5 Phase 2i: when CODELENS_EMBED_HINT_STRICT_COMMENTS=1 is set,
    // reject meta-annotation comments (`# TODO`, `# FIXME`, `# HACK`,
    // ...) while keeping behaviour-descriptive comments untouched. This
    // is the comment-side analogue of the Phase 2h literal filter
    // (§8.9) and targets the remaining ~92 % of the Python regression
    // that Phase 2h's literal-only filter left behind.
    let strict_comments = strict_comments_enabled();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(cleaned) = extract_comment_body(trimmed)
            && is_nl_shaped(&cleaned)
            && (!strict_comments || !looks_like_meta_annotation(&cleaned))
        {
            tokens.push(cleaned);
        }
    }

    // ── Pass 2: double-quoted string literals ────────────────────────
    // Simplified scanner — handles escape sequences but does not track
    // multi-line strings or raw strings. Good enough for NL-shaped
    // heuristic filtering where false negatives are acceptable.
    //
    // v1.5 Phase 2h: when CODELENS_EMBED_HINT_STRICT_LITERALS=1 is set,
    // also reject format templates and generic error / log prefixes. This
    // addresses the Phase 3b Python regression documented in §8.8 —
    // comments (Pass 1) stay untouched so Rust projects keep their wins.
    let strict_literals = strict_literal_filter_enabled();
    let mut chars = body.chars().peekable();
    let mut in_string = false;
    let mut current = String::new();
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                // Skip escape sequence
                let _ = chars.next();
            } else if c == '"' {
                if is_nl_shaped(&current)
                    && (!strict_literals
                        || (!contains_format_specifier(&current)
                            && !looks_like_error_or_log_prefix(&current)))
                {
                    tokens.push(current.clone());
                }
                current.clear();
                in_string = false;
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_string = true;
        }
    }

    if tokens.is_empty() {
        return None;
    }
    Some(join_hint_lines(&tokens))
}

/// Return true when API-call extraction is enabled via
/// `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` (or `true`/`yes`/`on`).
///
/// v1.5 Phase 2c infrastructure — kept off by default pending A/B
/// measurement. Orthogonal to `CODELENS_EMBED_HINT_INCLUDE_COMMENTS`
/// so both may be stacked.
///
/// v1.5 Phase 2j: explicit env > auto mode, same policy as Phase 2b.
pub fn api_calls_enabled() -> bool {
    if let Some(explicit) = parse_bool_env("CODELENS_EMBED_HINT_INCLUDE_API_CALLS") {
        return explicit;
    }
    auto_hint_should_enable()
}

/// Heuristic: does `ident` look like a Rust/C++ *type* (PascalCase) rather
/// than a module or free function (snake_case)?
///
/// Phase 2c API-call extractor relies on this filter to keep the hint
/// focused on static-method call sites (`Parser::new`, `HashMap::with_capacity`)
/// and drop module-scoped free functions (`std::fs::read_to_string`).
/// We intentionally accept only an ASCII uppercase first letter; stricter
/// than PascalCase detection but deliberate — the goal is high-precision
/// Type filtering, not lexical accuracy.
pub fn is_static_method_ident(ident: &str) -> bool {
    ident.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Collect `Type::method` call sites from a function body.
///
/// v1.5 Phase 2c experiment. Hypothesis: exposing the Types a function
/// interacts with (via their static-method call sites) adds a lexical
/// bridge between NL queries ("parse json", "open database") and symbols
/// whose body references the relevant type (`Parser::new`, `Connection::open`).
/// This is orthogonal to Phase 2b (comments + NL-shaped literals), which
/// targets *explanatory* natural language rather than *type* hints.
///
/// Gated by `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1`. Returns `None` when
/// the gate is off so the default embedding text is untouched.
pub fn extract_api_calls(source: &str, start: usize, end: usize) -> Option<String> {
    if !api_calls_enabled() {
        return None;
    }
    extract_api_calls_inner(source, start, end)
}

/// Env-independent core of `extract_api_calls`, exposed to the test module
/// so unit tests can run deterministically without touching env vars
/// (which would race with other tests that set
/// `CODELENS_EMBED_HINT_INCLUDE_API_CALLS`).
///
/// Scans the body for `Type::method` byte patterns where:
/// - `Type` starts with an ASCII uppercase letter and consists of
///   `[A-Za-z0-9_]*` (plain ASCII — non-ASCII identifiers are skipped
///   on purpose to minimise noise).
/// - `method` is any identifier (start `[A-Za-z_]`, continue `[A-Za-z0-9_]*`).
///
/// Duplicate `Type::method` pairs collapse into a single entry to avoid
/// biasing the embedding toward repeated calls in hot loops.
pub fn extract_api_calls_inner(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
    let safe_start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let safe_end = end.min(source.len());
    let safe_end = if source.is_char_boundary(safe_end) {
        safe_end
    } else {
        source.floor_char_boundary(safe_end)
    };
    if safe_start >= safe_end {
        return None;
    }
    let body = &source[safe_start..safe_end];
    let bytes = body.as_bytes();
    let len = bytes.len();

    let mut calls: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut i = 0usize;
    while i < len {
        let b = bytes[i];
        // Walk forward until we find the start of an ASCII identifier.
        if !(b == b'_' || b.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        let ident_start = i;
        while i < len {
            let bb = bytes[i];
            if bb == b'_' || bb.is_ascii_alphanumeric() {
                i += 1;
            } else {
                break;
            }
        }
        let ident_end = i;

        // Must be immediately followed by `::`.
        if i + 1 >= len || bytes[i] != b':' || bytes[i + 1] != b':' {
            continue;
        }

        let type_ident = &body[ident_start..ident_end];
        if !is_static_method_ident(type_ident) {
            // `snake_module::foo` — not a Type. Skip past the `::` so we
            // don't rescan the same characters, but keep walking.
            i += 2;
            continue;
        }

        // Skip the `::`
        let mut j = i + 2;
        if j >= len || !(bytes[j] == b'_' || bytes[j].is_ascii_alphabetic()) {
            i = j;
            continue;
        }
        let method_start = j;
        while j < len {
            let bb = bytes[j];
            if bb == b'_' || bb.is_ascii_alphanumeric() {
                j += 1;
            } else {
                break;
            }
        }
        let method_end = j;

        let method_ident = &body[method_start..method_end];
        let call = format!("{type_ident}::{method_ident}");
        if seen.insert(call.clone()) {
            calls.push(call);
        }
        i = j;
    }

    if calls.is_empty() {
        return None;
    }
    Some(join_hint_lines(&calls))
}

/// Peel the comment prefix off a trimmed line, returning the inner text
/// if the line is recognisably a `//`, `#`, `/* */`, or leading-`*` comment.
pub fn extract_comment_body(trimmed: &str) -> Option<String> {
    if trimmed.is_empty() {
        return None;
    }
    // `//` and `///` and `//!` (Rust doc comments)
    if let Some(rest) = trimmed.strip_prefix("///") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("//!") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return Some(rest.trim().to_string());
    }
    // `#[...]` attribute, `#!...` shebang — NOT comments
    if trimmed.starts_with("#[") || trimmed.starts_with("#!") {
        return None;
    }
    // `#` line comment (Python, bash, ...)
    if let Some(rest) = trimmed.strip_prefix('#') {
        return Some(rest.trim().to_string());
    }
    // Block-comment line: `/**`, `/*`, or continuation `*`
    if let Some(rest) = trimmed.strip_prefix("/**") {
        return Some(rest.trim_end_matches("*/").trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("/*") {
        return Some(rest.trim_end_matches("*/").trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix('*') {
        // Block-comment continuation. Only accept if the rest looks textual
        // (avoid e.g. `*const T` pointer types).
        let rest = rest.trim_end_matches("*/").trim();
        if rest.is_empty() {
            return None;
        }
        // Reject obvious code continuations
        if rest.contains(';') || rest.contains('{') {
            return None;
        }
        return Some(rest.to_string());
    }
    None
}

/// Extract the leading docstring or comment block from a symbol's body.
/// Supports: Python triple-quote, Rust //!//// doc comments, JS/TS /** */ blocks.
pub fn extract_leading_doc(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
    // Clamp to nearest char boundary to avoid panicking on multi-byte UTF-8
    let safe_start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let safe_end = end.min(source.len());
    let safe_end = if source.is_char_boundary(safe_end) {
        safe_end
    } else {
        source.floor_char_boundary(safe_end)
    };
    if safe_start >= safe_end {
        return None;
    }
    let body = &source[safe_start..safe_end];
    let lines: Vec<&str> = body.lines().skip(1).collect(); // skip the signature line
    if lines.is_empty() {
        return None;
    }

    let mut doc_lines = Vec::new();

    // Python: triple-quote docstrings
    let first_trimmed = lines.first().map(|l| l.trim()).unwrap_or_default();
    if first_trimmed.starts_with("\"\"\"") || first_trimmed.starts_with("'''") {
        let quote = &first_trimmed[..3];
        for line in &lines {
            let t = line.trim();
            doc_lines.push(t.trim_start_matches(quote).trim_end_matches(quote));
            if doc_lines.len() > 1 && t.ends_with(quote) {
                break;
            }
        }
    }
    // Rust: /// or //! doc comments (before the body, captured by tree-sitter)
    else if first_trimmed.starts_with("///") || first_trimmed.starts_with("//!") {
        for line in &lines {
            let t = line.trim();
            if t.starts_with("///") || t.starts_with("//!") {
                doc_lines.push(t.trim_start_matches("///").trim_start_matches("//!").trim());
            } else {
                break;
            }
        }
    }
    // JS/TS: /** ... */ block comments
    else if first_trimmed.starts_with("/**") {
        for line in &lines {
            let t = line.trim();
            let cleaned = t
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim();
            if !cleaned.is_empty() {
                doc_lines.push(cleaned);
            }
            if t.ends_with("*/") {
                break;
            }
        }
    }
    // Generic: leading // or # comment block
    else {
        for line in &lines {
            let t = line.trim();
            if t.starts_with("//") || t.starts_with('#') {
                doc_lines.push(t.trim_start_matches("//").trim_start_matches('#').trim());
            } else {
                break;
            }
        }
    }

    if doc_lines.is_empty() {
        return None;
    }
    Some(doc_lines.join(" ").trim().to_owned())
}
