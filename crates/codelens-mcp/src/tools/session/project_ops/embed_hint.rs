use codelens_engine::compute_dominant_language;

/// v1.5 Phase 2j MCP follow-up: auto-detect and export the dominant source
/// language for the given project so the engine's `auto_hint_should_enable`
/// gate can consult `language_supports_nl_stack` on the next embedding call.
///
/// Applied at two entry points:
///   1. Startup in `main.rs` — covers one-shot CLI (`--cmd`) and stdio MCP.
///   2. `activate_project` — covers MCP-driven project switches.
///
/// Only fires when:
///   (1) auto mode is explicitly enabled via `CODELENS_EMBED_HINT_AUTO=1`
///       (default-OFF policy held — no automatic behaviour change),
///   (2) the user has not already set `CODELENS_EMBED_HINT_AUTO_LANG`
///       themselves (explicit > auto, same rule as the three per-gate
///       env vars).
///
/// The detection walk is capped at 16k files inside
/// `compute_dominant_language` so even large monorepos pay a bounded cost.
/// When the walk yields no confident answer (fewer than 3 source files, or
/// no known-extension files at all), we leave the env var unset and the
/// engine falls through to the conservative default (stack OFF).
pub fn auto_set_embed_hint_lang(project_path: &std::path::Path) {
    // v1.6.0 flip (§8.14): default-ON semantics. Unset env means "auto
    // mode ON", explicit `CODELENS_EMBED_HINT_AUTO=0`/`false`/`no`/`off`
    // is the opt-out. Must stay in lock-step with the engine's
    // `auto_hint_mode_enabled()` in `crates/codelens-engine/src/embedding/mod.rs`.
    let auto_hint_gate_enabled = std::env::var("CODELENS_EMBED_HINT_AUTO")
        .ok()
        .map(|v| {
            let lowered = v.trim().to_ascii_lowercase();
            match lowered.as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => true, // unknown value → fall through to default-on
            }
        })
        .unwrap_or(true);
    let user_forced_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").is_ok();
    if !auto_hint_gate_enabled || user_forced_lang {
        return;
    }
    let Some(lang) = compute_dominant_language(project_path) else {
        return;
    };
    // Export to the process environment so the engine's gate functions
    // (`nl_tokens_enabled`, `api_calls_enabled`, `sparse_weighting_enabled`)
    // read it on the next call. Process-scoped — startup sets it once, and
    // `activate_project` re-writes it on project switch (handled via
    // `user_forced_lang` short-circuit: if we switch projects we'd have to
    // clear the var first, which is an acceptable follow-up limitation).
    //
    // SAFETY: `set_var` is unsafe on modern Rust because env-var mutation
    // is not thread-safe. Both call sites (startup main + single-threaded
    // MCP request handler) run before the engine has spawned worker
    // threads that read env, so the concurrent-read hazard does not apply.
    unsafe {
        std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", lang);
    }
}
