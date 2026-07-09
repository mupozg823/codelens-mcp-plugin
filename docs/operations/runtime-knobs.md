# Runtime Knobs & Maintenance

Operational env-var knobs and periodic maintenance for a running CodeLens
deployment. Reference material extracted from `CLAUDE.md`.

## Semantic Edit Backend (`semantic_edit_backend`)

`refactor_extract_function`, `refactor_inline_function`, `refactor_move_to_file`, and `refactor_change_signature` are dual-backend tools:

- **`tree-sitter`** (default) — syntactic-only, regex-style transformation. Fast, no language server required, but degraded: captured locals not detected, no scope analysis, no return-type inference.
- **`lsp`** — LSP-driven `textDocument/codeAction` + `codeAction/resolve` for true `WorkspaceEdit` semantics. Honors the language server's safety rules. Currently `conditional_authoritative_apply` — fixture coverage gates apply.
- **`auto`** — pick LSP when the file extension has a default LSP server mapping (rust/python/ts/js/go/java/kotlin, etc.), otherwise fall back to tree-sitter. Closest CodeLens equivalent of Serena's always-on LSP routing. Use `semantic_edit_backend=auto` per call or `CODELENS_SEMANTIC_EDIT_BACKEND=auto` for the whole session.

Falls back to tree-sitter if no `file_path` is supplied in `auto` mode so capability detection never errors.

## Analysis Artifact Cache (LRU + TTL)

`artifact_store` keeps recent analysis results (the `analysis_id` values returned by `review_architecture`, `module_boundary_report`, `dead_code_report`, etc.) so chained calls like `get_analysis_section` can resolve them. Two caps with runtime overrides:

- `CODELENS_MAX_ANALYSIS_ARTIFACTS` (non-zero usize, default `50`) — FIFO eviction count cap.
- `CODELENS_ANALYSIS_TTL_HOURS` (non-zero u64, default `6`) — TTL after which entries expire.

Invalid or `0` values fall back to the compiled defaults. Raise both when chaining many `start_analysis_job` calls within one session, or when a builder depends on a multi-hour-old handle.

## Backup Rotation

Three backup patterns accumulate without retention if left unmanaged:

- `${REPO}/.codelens/bin/codelens-mcp-http.bak-pre-*` — daemon redeploy preserves the previous binary by version tag.
- `~/.codelens/index/{symbols,embeddings}.db.bak-*-migration` — in-place schema migrations preserve the previous shape.
- `~/.codelens/index/{symbols,embeddings}.db.bak-readonly-old` — read-only conversion preserves the writable copy.

Run `bash scripts/cleanup-stale-backups.sh [--keep N] [--dry-run]` periodically (or wire into a build/release hook). Defaults to keeping the 2 most recent per pattern.
