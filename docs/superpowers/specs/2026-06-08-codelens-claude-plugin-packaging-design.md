# CodeLens Claude Code Plugin Packaging — Design

- **Date**: 2026-06-08
- **Status**: Approved (design); ready for implementation plan
- **Scope**: SP-4 productization, sub-project 1 of 3 (this doc covers `.claude-plugin` packaging only)
- **Branch**: `codelens-sp4-plugin-packaging`
- **Base**: `main` @ `a0e8a1a` (= `origin/main`)

## Problem

The repository is named `codelens-mcp-plugin` but ships **no Claude Code plugin manifest**. CodeLens
already has a strong binary-distribution story (`cargo install`, `install.sh`, Homebrew tap, GitHub
Releases) and bundle-ready assets (3 skills, 1 agent), but a user cannot run
`/plugin install codelens` to get the MCP server wired plus the CodeLens-specific skills/agent in one
step.

### Central constraint

A Claude Code plugin is git-cloned by the plugin system; it does **not** build Rust. CodeLens is a
compiled, platform-specific Rust binary too large to ship in a plugin repo. Therefore the binary is a
**prerequisite installed out-of-band** (via the existing `install.sh` / Release / cargo channels),
and the plugin's job is to **wire the already-installed binary** plus bundle the CodeLens-specific
skills and agent.

## Decisions (from grill session)

| # | Branch | Decision |
|---|--------|----------|
| D1 | MCP connection model | **stdio** — `mcpServers.codelens.command = "codelens-mcp"`. Bare invocation defaults to `--transport stdio` (verified in `crates/codelens-mcp/src/main.rs`). No ports, no launchd, per-session process lifecycle. HTTP daemon (7838/7839) documented as opt-in power-user upgrade. |
| D2 | Bundle scope | **mcpServers + 3 skills + 1 agent**. Hooks excluded: `clang-linker.sh` is repo-internal build tooling (not user-facing); `post-edit-diagnostics.sh` documented as README opt-in (auto-installing a per-Edit hook is intrusive for a fresh install). |
| D3 | Prerequisite / feature baseline | **`install.sh` / GitHub Release as the primary prerequisite** → semantic search bundled and working out-of-box (model dir included in tarball). `cargo install codelens-mcp` documented as a lean BM25+AST-only alternative (no model, `semantic_search` gracefully absent). Consistent with ADR-0012 (`default = []`). |
| D4 | Distribution channel | **In-repo single-plugin marketplace** (Sentry/Supabase pattern). `/plugin marketplace add mupozg823/codelens-mcp-plugin` → `/plugin install codelens@codelens`. |
| D5 | Manifest location | `.claude-plugin/plugin.json` + `.claude-plugin/marketplace.json`, plugin `source: "./"`. |

## Assumptions accepted

- **A1 — Plugin versioning**: `plugin.json.version` is an **independent plugin semver starting at `1.0.0`**, decoupled from the crate version (`1.13.32`). The manifest changes rarely; coupling it to every crate bump adds churn.
- **A2 — CI gate**: a `scripts/validate-plugin-manifest.py --check` step is added to `ci.yml`, mirroring the existing `regen-tool-defs.py --check` / `surface-manifest.py --check` deterministic-drift gates.
- **A3 — Failure mode**: "binary not on PATH" surfaces as a Claude Code MCP connection failure. The manifest cannot auto-detect this; it is handled by a README prerequisite section + pointer to `codelens-mcp doctor`. Accepted limitation.

## Open questions (deferred)

- **Official marketplace submission** (anthropics `claude-plugins-official` PR) — owner: user; when: after in-repo marketplace is validated. A later SP-4 sub-project.
- **Self-bootstrapping wrapper** (stdio command = a launcher that installs the binary on first connect) — owner: user; when: v2 candidate. Adds complexity/risk (running an installer on first MCP connect).

## Design

### 1. `.claude-plugin/plugin.json` (new)

```jsonc
{
  "name": "codelens",
  "displayName": "CodeLens",
  "description": "Compressed code-intelligence MCP for planner/reviewer/refactor harnesses — AST + call-graph + hybrid retrieval, mutation-gated refactoring.",
  "version": "1.0.0",
  "author": { "name": "mupozg823" },
  "homepage": "https://github.com/mupozg823/codelens-mcp-plugin",
  "repository": "https://github.com/mupozg823/codelens-mcp-plugin",
  "license": "Apache-2.0",
  "keywords": ["code-intelligence", "tree-sitter", "mcp", "ast", "refactoring"],
  "mcpServers": { "codelens": { "command": "codelens-mcp" } },
  "skills": "./skills/",
  "agents": "./agents/"
}
```

- MCP server key is `codelens` so tool names stay `mcp__codelens__*`, matching the existing tool
  references in `agents/codelens-explorer.md` and the 3 skills.
- `skills` / `agents` reuse the existing top-level directories — **no new skill/agent code**.
- No `hooks` key (D2).

### 2. `.claude-plugin/marketplace.json` (new)

```jsonc
{
  "name": "codelens",
  "owner": { "name": "mupozg823" },
  "plugins": [
    {
      "name": "codelens",
      "source": "./",
      "description": "CodeLens MCP + analyze/review/onboard skills + read-only explorer agent."
    }
  ]
}
```

Install flow: `/plugin marketplace add mupozg823/codelens-mcp-plugin` →
`/plugin install codelens@codelens`.

### 3. `scripts/validate-plugin-manifest.py` (new)

Deterministic validator, `--check` mode for CI. Validates:

1. `.claude-plugin/plugin.json` and `.claude-plugin/marketplace.json` are valid JSON.
2. Required `plugin.json` fields present: `name`, `version`, `description`, `mcpServers`.
3. `skills` path (`./skills/`) and `agents` path (`./agents/`) exist on disk and are non-empty.
4. `mcpServers.codelens.command` is a non-empty string (the expected binary name `codelens-mcp`).
5. `marketplace.json`: `name`, `owner`, `plugins[]` present; each plugin's `source` resolves to a
   directory containing `.claude-plugin/plugin.json`; plugin `name` matches `plugin.json.name`.

Exit non-zero with a clear message on any failure. Built TDD (failing cases first: broken JSON,
missing field, dangling skills path, name mismatch).

### 4. `.github/workflows/ci.yml` (1-line addition)

Add `python3 scripts/validate-plugin-manifest.py --check` adjacent to the existing
`regen-tool-defs.py --check` / `surface-manifest.py --check` steps.

### 5. `README.md` (new section)

A "Claude Code Plugin" section:

- Prerequisite: install the binary first — **`install.sh` / GitHub Release recommended** (semantic
  bundled); cargo-default noted as the lean alternative.
- Two-line install (`/plugin marketplace add …` → `/plugin install …`).
- What you get: `mcp__codelens__*` tools + `codelens-analyze` / `codelens-review` /
  `codelens-onboard` skills + `codelens-explorer` agent.
- Failure mode: if tools don't appear, the binary isn't on PATH — run `codelens-mcp doctor`.
- Opt-in: enabling `post-edit-diagnostics.sh` as a `PostToolUse(Edit)` hook (documented, not
  auto-installed).

### 6. `agents/codelens-explorer.md` (no change)

`semantic_search` stays in its tool list. With the recommended `install.sh`/Release prerequisite the
tool is present; on a cargo-default build it is gracefully absent and the agent's other AST/graph
tools continue to function.

## File changes summary

| Path | Action |
|------|--------|
| `.claude-plugin/plugin.json` | new |
| `.claude-plugin/marketplace.json` | new |
| `scripts/validate-plugin-manifest.py` | new (TDD) |
| `.github/workflows/ci.yml` | +1 step |
| `README.md` | + "Claude Code Plugin" section |
| `agents/codelens-explorer.md` | unchanged |
| `skills/*`, hooks | unchanged |

## Verification

- **Deterministic (authority)**:
  - `validate-plugin-manifest.py` unit tests pass; `--check` exits 0 on the real manifests.
  - Existing CI gates unaffected: `cargo check/clippy/nextest` (no Rust source touched),
    `regen-tool-defs.py --check`, `surface-manifest.py --check` still green.
- **Manual install probe (external-dependency, isolate-verify)**: in a Claude Code session,
  `/plugin marketplace add <local repo path>` → `/plugin install codelens` → confirm
  `mcp__codelens__*` tools, the 3 skills, and the explorer agent are exposed. Requires a fresh
  session (plugin/MCP registration is session-scoped) — flagged as a user-run step.

## Out of scope (this sub-project)

- codelens CLI / TUI (SP-4 sub-project 2).
- mcp-remote / hosted remote MCP (SP-4 sub-project 3).
- Reviving the stale `codex/cold-semantic-core` worktree (old architecture: pre-`codelens-core →
  codelens-engine` rename, pre-diet `-19021 LOC`).
- Auto-installing the binary, bundling the ONNX model in the plugin repo, or hosting a remote
  endpoint.
