//! `get_symbols_overview` — file/directory AST symbol tree with
//! token-budget-aware compaction.
//!
//! Stages, in order:
//!   1. **argument parsing** with path alias resolution
//!      (`resolve_path_argument`) and `depth` extraction.
//!   2. **structural fetch** through
//!      `SymbolIndex::get_symbols_overview_cached`.
//!   3. **budget guard** — strip children + truncate symbol list when
//!      the response would exceed the active surface budget and the
//!      caller did not pin `depth` explicitly.
//!   4. **degraded-result classification** — distinguish
//!      `file_not_indexed` / `indexed_no_symbols` /
//!      `unsupported_extension` so callers act on the right signal
//!      (per #183 + #184 follow-up: don't recommend
//!      `refresh_symbol_index` on legitimately empty files).
//!   5. **response annotations** — surface `unknown_args` +
//!      `deprecation_warnings` at the top level.
//!
//! All five stages now concentrate here; `handlers.rs::get_symbols_overview`
//! is a 3-line dispatch into `SymbolQueryPipeline`.

use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, optional_string, success_meta};
use serde_json::{Value, json};

fn resolve_path_argument(arguments: &Value) -> Result<(&str, Vec<Value>), CodeLensError> {
    if let Some(path) = optional_string(arguments, "path") {
        if let Some(alias @ ("file_path" | "relative_path")) =
            optional_string(arguments, "_path_alias_source")
        {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
        return Ok((path, Vec::new()));
    }
    for alias in ["file_path", "relative_path"] {
        if let Some(path) = optional_string(arguments, alias) {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
    }
    Err(CodeLensError::MissingParam("path".to_owned()))
}

fn insert_response_annotations(
    payload: &mut Value,
    unknown_args: &[String],
    deprecation_warnings: &[Value],
) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    if !unknown_args.is_empty() {
        map.insert("unknown_args".to_owned(), json!(unknown_args));
    }
    if !deprecation_warnings.is_empty() {
        map.insert(
            "deprecation_warnings".to_owned(),
            json!(deprecation_warnings),
        );
    }
}

pub(crate) fn run_symbols_overview(state: &AppState, arguments: &Value) -> ToolResult {
    const KNOWN_ARGS: &[&str] = &["path", "file_path", "relative_path", "depth"];
    let (path, deprecation_warnings) = resolve_path_argument(arguments)?;
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
    let explicit_depth = arguments.get("depth").and_then(|v| v.as_u64());
    let depth = explicit_depth.unwrap_or(1) as usize;
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let budget = state.execution_token_budget(&session);
    let mut symbols = state
        .symbol_index()
        .get_symbols_overview_cached(path, depth)?;

    // Token guard: auto-strip children when response would exceed budget.
    // Skip if depth was explicitly requested (user intentionally wants full detail).
    let estimated_chars: usize = symbols.iter().map(|s| 80 + s.children.len() * 120).sum();
    let budget_chars = budget * 4;
    let stripped = explicit_depth.is_none() && estimated_chars > budget_chars;
    if stripped {
        for sym in &mut symbols {
            let child_count = sym.children.len();
            sym.children.clear();
            sym.signature = format!("{} ({child_count} symbols)", sym.signature);
        }
    }

    // Hard limit: truncate if still too large (unless explicit depth)
    let max_symbols = if explicit_depth.is_some() {
        usize::MAX
    } else {
        budget_chars / 80
    };
    let truncated = symbols.len() > max_symbols;
    if truncated {
        symbols.truncate(max_symbols);
    }

    let mut payload = json!({
        "symbols": symbols,
        "count": symbols.len(),
        "truncated": truncated,
        "auto_summarized": stripped,
    });
    // #183 + #184 follow-up: distinguish three empty-result cases so
    // callers (humans + agents) can act on the right signal:
    //
    //   - "file_not_indexed"        — file is on disk + supported extension
    //                                 but no row in the symbol DB yet
    //                                 (watcher lag / .gitignore / project
    //                                 root mismatch)
    //   - "indexed_no_symbols"      — file is on disk, supported, AND the
    //                                 DB has a row for it; the empty list
    //                                 is the truth (empty file, comments
    //                                 only, type-alias-only module, …)
    //                                 Callers should NOT trigger refresh.
    //   - "unsupported_extension"   — extension is not in lang_registry
    //
    // The previous version conflated the first two and pushed clients into
    // unnecessary `refresh_symbol_index` loops on legitimately empty
    // files (Codex P2 on PR #184).
    if symbols.is_empty() {
        let resolved = state.project().resolve(path).ok();
        let on_disk = resolved
            .as_deref()
            .map(std::path::Path::is_file)
            .unwrap_or(false);
        let extension = resolved
            .as_deref()
            .and_then(std::path::Path::extension)
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let supported = extension
            .as_deref()
            .map(codelens_engine::lang_registry::supports_symbols)
            .unwrap_or(false);
        if on_disk && supported {
            let already_indexed = state.symbol_index().file_is_indexed(path).unwrap_or(false);
            if already_indexed {
                payload["degraded_reason"] = json!("indexed_no_symbols");
                payload["recovery_message"] = json!(
                    "File is indexed but has no top-level symbols (empty file, comments only, \
                     or no declarations the active backend recognises). This is not an error; \
                     no recovery action is required."
                );
            } else {
                payload["degraded_reason"] = json!("file_not_indexed");
                payload["fallback_hint"] = json!(["refresh_symbol_index", "find_symbol"]);
                payload["recovery_message"] = json!(
                    "Result is empty because this file is not in the on-disk index yet \
                     (watcher lag, .gitignore, or project root mismatch). \
                     Call `refresh_symbol_index` with this path, or wait ~1-2s after a recent edit."
                );
            }
        } else if on_disk {
            payload["degraded_reason"] = json!("unsupported_extension");
        }
    }
    insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);

    Ok((payload, success_meta(BackendKind::TreeSitter, 0.93)))
}
