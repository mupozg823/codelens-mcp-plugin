# Runtime Knobs & Maintenance

Operational env-var knobs and periodic maintenance for a running CodeLens
deployment. Reference material extracted from `CLAUDE.md`.

## Semantic Edit Backend (`semantic_edit_backend`)

`refactor_extract_function`, `refactor_inline_function`, `refactor_move_to_file`, and `refactor_change_signature` are dual-backend tools:

- **`tree-sitter`** (default) — syntactic-only, regex-style transformation. Fast, no language server required, but degraded: captured locals not detected, no scope analysis, no return-type inference.
- **`lsp`** — LSP-driven `textDocument/codeAction` + `codeAction/resolve` for true `WorkspaceEdit` semantics. Honors the language server's safety rules. Currently `conditional_authoritative_apply` — fixture coverage gates apply.
- **`auto`** — pick LSP when the file extension has a default LSP server mapping (rust/python/ts/js/go/java/kotlin, etc.), otherwise fall back to tree-sitter. Closest CodeLens equivalent of Serena's always-on LSP routing. Use `semantic_edit_backend=auto` per call or `CODELENS_SEMANTIC_EDIT_BACKEND=auto` for the whole session.

Falls back to tree-sitter if no `file_path` is supplied in `auto` mode so capability detection never errors.

## LSP Subprocess Trust Boundary

LSP tools do not treat `command` and `args` as a generic process launcher.
The engine authorizes one immutable tuple before any spawn:

1. `command` must identify a registered `LSP_RECIPES` server.
2. `args` must exactly match that recipe (omitting `args` selects the recipe
   defaults).
3. The executable must already be present in the session pool's canonical
   trust map. Path-qualified input is accepted only when it canonicalizes to
   that same executable.

At pool construction, trusted executables come from the daemon's inherited
`PATH`, conservative platform fallback directories, and
`CODELENS_LSP_PATH_EXTRA`. Project `node_modules/.bin` directories are not
searched implicitly. `register_trusted_lsp_binary` exists for an embedding host
to add an explicit mapping; it is a host configuration API and must never
receive tool-call input.

Treat every directory in `PATH` and `CODELENS_LSP_PATH_EXTRA` as executable
code: it must be operator-owned and not writable by a bound project or remote
client. Restart the daemon after changing those variables so new pools capture
the intended paths. Pre-warm uses this same trust map, so it cannot widen the
launch surface.

This policy prevents direct arbitrary-command and free-form-argument execution.
It does not sandbox a trusted language server after launch; servers may load
project plugins, build scripts, proc macros, or compiler extensions. For
hostile repositories, isolate the daemon at the OS/container layer and omit
LSP tools from the exposed surface. `CODELENS_LSP_PREWARM=off` only disables
eager startup and is not a sandbox.

## Analysis Artifact Cache (LRU + TTL)

`artifact_store` keeps recent analysis results (the `analysis_id` values returned by `review_architecture`, `module_boundary_report`, `dead_code_report`, etc.) so chained calls like `get_analysis_section` can resolve them. Two caps with runtime overrides:

- `CODELENS_MAX_ANALYSIS_ARTIFACTS` (non-zero usize, default `50`) — FIFO eviction count cap.
- `CODELENS_ANALYSIS_TTL_HOURS` (non-zero u64, default `6`) — TTL after which entries expire.

Invalid or `0` values fall back to the compiled defaults. Raise both when chaining many `start_analysis_job` calls within one session, or when a builder depends on a multi-hour-old handle.

## Index Admission Gate (memory pressure)

Heavy background index jobs (`refresh_symbol_index`, `index_embeddings`) defer
while macOS reports memory pressure at warning level or above
(`kern.memorystatus_vm_pressure_level` ≥ 2), polling every 2s with fresh job
heartbeats and honoring cancellation between polls. Non-macOS targets and
probe failures read as normal pressure (fail-open).

- `CODELENS_INDEX_PRESSURE_MAX_DEFER_SECS` (u64 seconds, default `120`) — defer
  budget per job. After it elapses the job proceeds under pressure (with a
  `tracing` warning) so admission gating can never starve a job. `0` disables
  deferral entirely (useful for CI or benchmark runs).

## Index Exclusions (`.codelens/config.json`)

The indexer skips a hardcoded directory set (`EXCLUDED_DIRS` in
`crates/codelens-engine/src/project/exclusions.rs`): VCS and editor state,
`node_modules`, `target`, `dist`, `build`, `out`, `generated`, `vendor`,
`__pycache__`, virtualenvs, caches, git worktrees, and framework build output
(`.next`, `.vercel`, `.turbo`, `.svelte-kit`, `.nuxt`, `.astro`,
`.parcel-cache`).

**The indexer does not read `.gitignore`.** That list is a curated set of names,
not a rule, so anything a repository ignores under a name not on the list is
still indexed and still shows up in analysis output. The cost lands on report
precision rather than index size: generated bundles are referenced by nothing,
so they flood dead-code rankings. Measured 2026-09-05 on a Next.js/Vercel repo,
`.vercel` held 44 indexable JS/TS files against 1,259 real source files and led
the dead-code report; it is excluded by default as of that change, but the
general gap remains.

Exclude them per project with `.codelens/config.json`. All three keys are read
and merged, so use whichever reads best:

```json
{
  "index": {
    "exclude_paths": [".vercel/output", "coverage"],
    "exclude": ["**/*.generated.ts"]
  },
  "exclude_paths": ["fixtures/large"]
}
```

Pattern rules, from `expand_exclude_pattern`:

- A plain path expands to both itself and `<path>/**`, so `".vercel/output"`
  excludes the directory and everything under it.
- A pattern already containing `*`, `?`, `[`, or `{`, or ending in `/`, is used
  verbatim with no expansion.
- A leading `./` is stripped and `\` is normalised to `/`.
- Any pattern containing `..` is dropped, so exclusions cannot escape the
  project root.

Changing this file requires a reindex to take effect
(`refresh_symbol_index`, or `analyze` with `mode=start`).

## Backup Rotation

Three backup patterns accumulate without retention if left unmanaged:

- `${REPO}/.codelens/bin/codelens-mcp-http.bak-pre-*` — daemon redeploy preserves the previous binary by version tag.
- `~/.codelens/index/{symbols,embeddings}.db.bak-*-migration` — in-place schema migrations preserve the previous shape.
- `~/.codelens/index/{symbols,embeddings}.db.bak-readonly-old` — read-only conversion preserves the writable copy.

Run `bash scripts/cleanup-stale-backups.sh [--keep N] [--dry-run]` periodically (or wire into a build/release hook). Defaults to keeping the 2 most recent per pattern.
