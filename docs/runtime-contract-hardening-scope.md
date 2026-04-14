# Runtime Contract Hardening Scope

## Purpose

This document defines the first extraction target on branch
`codex/runtime-contract-hardening`.

Goal:

- isolate the first product-core commit
- avoid mixing runtime behavior with verification, release, docs, benchmarks, or research churn

## Commit Target

Target commit name:

- `feat(mcp): harden runtime transport and dispatch contracts`

This commit should contain only runtime contract behavior for the MCP server.

## Must Include

These files are in-scope for the first runtime-contract commit.

### Core runtime entrypoints

- `crates/codelens-mcp/src/main.rs`
- `crates/codelens-mcp/src/protocol.rs`
- `crates/codelens-mcp/src/error.rs`
- `crates/codelens-mcp/src/client_profile.rs`
- `crates/codelens-mcp/src/harness_host.rs`
- `crates/codelens-mcp/src/analysis_handles.rs`

### Dispatch layer

- `crates/codelens-mcp/src/dispatch/access.rs`
- `crates/codelens-mcp/src/dispatch/mod.rs`
- `crates/codelens-mcp/src/dispatch/response.rs`
- `crates/codelens-mcp/src/dispatch/response_support.rs`

### Resource and contract surface

- `crates/codelens-mcp/src/resource_catalog.rs`
- `crates/codelens-mcp/src/resource_context.rs`
- `crates/codelens-mcp/src/resource_profiles.rs`
- `crates/codelens-mcp/src/resources.rs`

### Transport layer

- `crates/codelens-mcp/src/server/router.rs`
- `crates/codelens-mcp/src/server/transport_http.rs`
- `crates/codelens-mcp/src/server/transport_stdio.rs`

### Runtime state

- `crates/codelens-mcp/src/state.rs`
- `crates/codelens-mcp/src/state/analysis.rs`
- `crates/codelens-mcp/src/state/project_runtime.rs`
- `crates/codelens-mcp/src/state/session_runtime.rs`

### Tool surface and runtime-adjacent tool plumbing

- `crates/codelens-mcp/src/tool_defs/build.rs`
- `crates/codelens-mcp/src/tool_defs/mod.rs`
- `crates/codelens-mcp/src/tool_defs/output_schemas.rs`
- `crates/codelens-mcp/src/tool_defs/presets.rs`
- `crates/codelens-mcp/src/tools/filesystem.rs`
- `crates/codelens-mcp/src/tools/mod.rs`
- `crates/codelens-mcp/src/tools/query_analysis.rs`
- `crates/codelens-mcp/src/tools/report_payload.rs`
- `crates/codelens-mcp/src/tools/report_utils.rs`
- `crates/codelens-mcp/src/tools/session/metrics_config.rs`
- `crates/codelens-mcp/src/tools/session/project_ops.rs`
- `crates/codelens-mcp/src/tools/symbols.rs`
- `crates/codelens-mcp/src/tools/workflows.rs`

### Local crate manifest

- `crates/codelens-mcp/Cargo.toml`

## Validated Minimum Standalone Boundary

The broad in-scope list above is still too wide for the first commit.

An independent index-export validation on 2026-04-14 captured one valid
bootstrap/resource contract slice that can stand alone as a first commit.

### Validated first-commit file set

- `crates/codelens-mcp/src/resource_catalog.rs`
- `crates/codelens-mcp/src/resource_context.rs`
- `crates/codelens-mcp/src/resources.rs`
- `crates/codelens-mcp/src/server/router.rs`
- `crates/codelens-mcp/src/server/transport_stdio.rs`
- `crates/codelens-mcp/src/tools/session/project_ops.rs`

### Why this is the minimum

- `resource_context.rs`, `resource_catalog.rs`, and `resources.rs` now share the
  bootstrap contract constants and fallback client-profile handling.
- `server/router.rs` and `tools/session/project_ops.rs` both depend on the same
  resource-request bootstrap path, so they must move together.
- `server/transport_stdio.rs` adds the stdio bootstrap regression test that
  proves Codex can bootstrap through `prepare_harness_session` without first
  calling `tools/list`.

### Keep out of the first commit unless new proof requires them

- `crates/codelens-mcp/src/analysis_handles.rs`
- `crates/codelens-mcp/src/client_profile.rs`
- `crates/codelens-mcp/src/dispatch/**`
- `crates/codelens-mcp/src/error.rs`
- `crates/codelens-mcp/src/harness_host.rs`
- `crates/codelens-mcp/src/main.rs`
- `crates/codelens-mcp/src/protocol.rs`
- `crates/codelens-mcp/src/resource_profiles.rs`
- `crates/codelens-mcp/src/server/transport_http.rs`
- `crates/codelens-mcp/src/state.rs`
- `crates/codelens-mcp/src/state/**`
- `crates/codelens-mcp/src/tool_defs/**`
- `crates/codelens-mcp/src/tools/filesystem.rs`
- `crates/codelens-mcp/src/tools/mod.rs`
- `crates/codelens-mcp/src/tools/query_analysis.rs`
- `crates/codelens-mcp/src/tools/report_payload.rs`
- `crates/codelens-mcp/src/tools/report_utils.rs`
- `crates/codelens-mcp/src/tools/session/metrics_config.rs`
- `crates/codelens-mcp/src/tools/symbols.rs`
- `crates/codelens-mcp/src/tools/workflows.rs`
- `crates/codelens-mcp/Cargo.toml`

These paths are still runtime-adjacent, but today they widen the first review
into broader transport/state changes, retrieval tuning, report-handle schema, or packaging
concerns. They belong in a follow-up contract-surface commit unless a fresh
build failure proves otherwise.

### Validation evidence

Validated from a captured git-index snapshot with:

- `cargo build -p codelens-mcp`
- `cargo build -p codelens-mcp --features http`
- `cargo test -p codelens-mcp transport_stdio::tests -- --nocapture`

## Must Exclude

These files must not be in the first runtime-contract commit.

### Verification bucket

- `crates/codelens-mcp/src/integration_tests/lsp.rs`
- `crates/codelens-mcp/src/integration_tests/mod.rs`
- `crates/codelens-mcp/src/integration_tests/protocol.rs`
- `crates/codelens-mcp/src/integration_tests/readonly.rs`
- `crates/codelens-mcp/src/integration_tests/workflow.rs`
- `crates/codelens-mcp/src/server/http_tests.rs`

### Engine / retrieval / benchmark bucket

- all `crates/codelens-engine/**`
- all `benchmarks/**`
- all `datasets/**`
- all `scripts/finetune/**`

### Release / packaging / ops bucket

- `.github/workflows/**`
- `scripts/generate-release-manifest.py`
- `scripts/quality-gate.sh`
- `scripts/verify-release-artifacts.sh`
- `scripts/check-release-docs.py`
- `scripts/publish-crates-workspace.sh`
- `scripts/verify-github-attestations.sh`
- `scripts/sync-local-bin.sh`
- `Formula/codelens-mcp.rb`
- `CHANGELOG.md`
- `Cargo.toml`
- `Cargo.lock`

### TUI bucket

- all `crates/codelens-tui/**`

### Broad docs bucket

- `README.md`
- `docs/**`
- `crates/codelens-mcp/README.md`

## Review Before Including

These are adjacent to runtime but should only be included if the runtime commit does not build without them.

- `Cargo.toml`
- `Cargo.lock`
- `crates/codelens-mcp/README.md`

Default rule:

- exclude them first
- add them only if the runtime-contract commit is otherwise invalid

## Why This Boundary

The first commit must answer one question only:

- did the runtime contract, transport, dispatch, state, and tool-surface behavior improve safely?

If verification, benchmarks, release, or docs are mixed in, review quality collapses.

## Current Known Risks

### 1. Workspace manifest coupling

If `crates/codelens-mcp/Cargo.toml` introduces dependency changes that require root
workspace manifest updates, the root manifest changes should be added in a tiny follow-up
commit, not bundled with unrelated release work.

### 2. Runtime vs verification drift

`http_tests.rs` and `integration_tests/*` clearly changed alongside runtime files, but
they should remain separate for review clarity.

### 3. Resource surface coupling

`resource_catalog.rs`, `resource_context.rs`, `resource_profiles.rs`, and `resources.rs`
are Bucket 1 material at the product level. In the current branch state,
`resource_catalog.rs`, `resource_context.rs`, and `resources.rs` are part of the
validated minimum standalone boundary, while `resource_profiles.rs` still stays
out until the next isolated build proves it is immediately required.

## Staging Sequence

Use this order:

1. stage only the validated first-commit file set
2. run targeted validation on that exact set
3. commit runtime core
4. stage the remaining Bucket 1 transport/state/contract-surface files
5. inspect if `crates/codelens-mcp/Cargo.toml` or root manifests are actually required
6. handle verification in the next commit after contract-surface review

## Non-Goals

- do not clean all dirty files in one pass
- do not push from `main`
- do not mix benchmark or finetune work into runtime product commits
- do not fold release scripts into runtime review
