mod conversation;
mod prepare;
mod queryable_projects;

pub(crate) use conversation::{prepare_for_new_conversation, summarize_changes};
pub(crate) use prepare::{activate_project, prepare_harness_session};
pub(crate) use queryable_projects::{
    add_queryable_project, list_queryable_projects, query_project, remove_queryable_project,
};

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
pub(crate) use conversation::auto_set_embed_hint_lang;
