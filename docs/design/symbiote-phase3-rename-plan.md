# Symbiote MCP — Phase 3 Rename Execution Plan

Status: planning baseline for the v2.0.0 cutover session
Date: 2026-04-18
Parent: [ADR-0007](../adr/ADR-0007-symbiote-rebrand.md)
Related: [Symbiote UX / Agent Flows](symbiote-ux-flows-v1.md), [Migration guide](../migrate-from-codelens.md)

This document exists to prevent a blind repo-wide string replacement.
The remaining `codelens-*` surfaces span crates, binaries, release
artifacts, host config generation, runtime identifiers, and generated
manifests. A safe cutover needs ordered batches with explicit rollback
boundaries.

## 1. Goal

Ship the public primary-name cutover from **CodeLens MCP** to
**Symbiote MCP** only after trademark clearance, without introducing:

- partial rename states where install, docs, and release artifacts
  disagree
- broken host adapters for Claude Code, Codex, Cursor, Cline, or
  Windsurf
- irreversible crates.io publishes that strand users on a dead name
- noisy code churn from renaming historical documentation or generated
  files by hand

## 2. Non-goals

- Do not mass-edit historical release notes just to remove old names.
- Do not rename every filesystem path in the same session if Cargo
  package renames are sufficient for the public cutover.
- Do not hand-edit generated files (`docs/generated/*`, `*.cdx.json`,
  release manifests) if the source registry can regenerate them.
- Do not cut over the public primary install name before legal
  clearance.

## 3. Current rename surface inventory

The remaining Phase 3 surface is larger than a single crate rename.

| Bucket | Representative surfaces | Why it matters |
| --- | --- | --- |
| Workspace + crates | root `Cargo.toml`, `crates/codelens-engine/Cargo.toml`, `crates/codelens-mcp/Cargo.toml`, `crates/codelens-tui/Cargo.toml` | crates.io publish names and cargo metadata are irreversible once shipped |
| Binaries + commands | `codelens-mcp`, `codelens-tui`, README install snippets, attach/detach output, Docker entrypoints | users copy these commands directly |
| Distribution metadata | `install.sh`, `Formula/codelens-mcp.rb`, `mcp.json`, GitHub release workflow artifact names | install channels must agree on archive names and binary names |
| Runtime identity | `server_name`, telemetry service names, resource URI prefixes, env vars, state directories | hosts and dashboards observe these names at runtime |
| Host adapters | generated config blocks for Codex, Claude Code, Cursor, Cline, Windsurf | attach/detach must keep host-native setup friction low |
| Docs + IA | `README.md`, `docs/index.md`, setup and migration docs | public docs become the source of truth during cutover |
| Generated artifacts | `docs/generated/surface-manifest.json`, SBOM files, release checksum manifests | must be regenerated from source, not manually patched |

## 4. Atomic batches

The cutover should ship as a small number of explicit batches. Each
batch has an owner, a clear verification target, and a rollback story.

### Batch A — Compatibility foundation must already exist

Entry condition before the rename session starts:

- `symbiote://...` compatibility alias is already emitted and accepted.
- `SYMBIOTE_*` environment variables are already accepted alongside
  `CODELENS_*`.
- `docs/migrate-from-codelens.md` is already published and reviewed.
- `attach` / `detach` host flows are stable enough that command-string
  replacement is the only host-facing delta.

If these are missing, stop. Do not begin the public rename.

### Batch B — Runtime primary-name flip

Change the runtime to prefer Symbiote while retaining CodeLens aliases
for one compatibility window.

Primary surfaces:

- runtime `server_name`
- server card identity
- telemetry service name / tracer name
- resource URI primary prefix
- environment-variable precedence and docs
- state / cache directory naming policy if changed

Rules:

- Old `codelens://...` URIs remain accepted for one minor version.
- Old `CODELENS_*` env vars remain accepted through the documented
  sunset horizon.
- Host-facing output should say "Symbiote MCP" first and explicitly note
  the legacy alias where needed.

Rollback:

- Safe before release tag if package/binary names have not been
  published yet.
- After release, keep the compatibility aliases rather than trying to
  revert the runtime prefix again.

### Batch C — Public binary + install channel cutover

This is the highest user-impact batch and must move together.

Primary surfaces:

- binary name: `codelens-mcp` -> `symbiote-mcp`
- Homebrew formula name and caveats
- `install.sh` download targets and verification text
- `mcp.json` install metadata
- README install table and host examples
- Docker snippets and release archive names

Rules:

- Do not rename the binary in docs until install artifacts with the new
  name actually exist.
- Keep one explicit migration note in every install surface:
  "formerly `codelens-mcp`".
- `attach` / `detach` must continue to generate correct host config
  without requiring manual edits beyond the renamed executable.

Rollback:

- Before publishing release artifacts, revert normally.
- After publishing release archives, keep a redirecting migration story
  instead of trying to unpublish.

### Batch D — Cargo package publish cutover

This batch is irreversible and therefore must happen after runtime and
binary names are already validated locally.

Primary surfaces:

- package names: `codelens-engine`, `codelens-mcp`, `codelens-tui`
- docs.rs / crates.io badges
- dependency snippets
- release workflow publish order

Recommended constraint:

- Prefer renaming package names first and keeping crate directory paths
  stable during the first release if that materially reduces diff size.
  Filesystem path cleanup can be a later follow-up.

Publish policy:

- New `symbiote-*` packages are published first.
- Old `codelens-*` crate pages remain as historical entries with
  README-level migration guidance; do not yank them.
- Do not promise transparent cargo aliasing that crates.io cannot
  actually provide.

Rollback:

- There is no true rollback after publish. The only safe fallback is
  forward guidance plus patch releases on the old line if absolutely
  necessary.

### Batch E — Repository / release pipeline rename

Primary surfaces:

- GitHub repository name and URLs
- release workflow artifact patterns
- tap update workflow
- checksum / SBOM / airgap bundle names
- release verification docs

Rules:

- Treat this as a release-engineering change, not just a docs change.
- Archive names, checksum lookup, Formula templating, and OCI packaging
  must be updated as one set.
- Generated SBOM filenames must be regenerated by the workflow, not
  renamed manually in-tree.

Rollback:

- GitHub repo rename is redirect-friendly, but scripts that depend on
  raw URLs still need immediate validation.

### Batch F — Post-GA cleanup

Primary surfaces:

- residual comments and internal references that are not user-facing
- optional crate directory renames
- internal test fixtures that intentionally mention `codelens-*`
- archived docs indexing polish

Rules:

- Historical release notes may keep historical names.
- Test fixtures that assert legacy behavior should only change if the
  behavior itself changes.

## 5. Work order

The cutover session should run in this order:

1. Freeze the migration doc and install copy.
2. Land Batch B locally and verify dual-namespace runtime behavior.
3. Land Batch C locally and verify all attach/detach flows with dry-run
   or isolated temp configs.
4. Run release-workflow dry runs for Batch E naming.
5. Only then execute Batch D package-publish preparation.
6. Publish and tag.
7. Follow with Batch F cleanup in a smaller, low-risk session.

If a step fails, revert only the current unpublished batch. Do not stack
additional rename work on top of a failing batch.

## 6. Must-change-together sets

These surfaces should not be split across separate PRs unless the split
is purely preparatory and backward-compatible.

### Set 1 — Binary distribution contract

- binary name
- release archives
- checksums
- Homebrew formula
- installer script
- README install examples

Reason: users verify one surface against another immediately.

### Set 2 — Host attachment contract

- attach/detach generated commands
- platform setup docs
- host adapter resource examples
- migration guide diffs

Reason: host setup is the first-run critical path.

### Set 3 — Runtime identity contract

- server card name
- `server_name` manifest fields
- telemetry service name
- URI prefix policy
- env-var precedence

Reason: hosts, CI, and observability dashboards rely on the same
identity string.

### Set 4 — Crates.io public contract

- package names
- badges
- cargo install snippets
- publish workflow
- crate README copy

Reason: package metadata is irreversible once pushed.

## 7. Surfaces that should remain legacy-compatible

These should not hard-break at v2.0.0:

- `codelens://...` resource URIs during the declared bridge window
- `CODELENS_*` environment variables during the declared bridge window
- old docs links that GitHub can redirect automatically
- user configs that still contain the `codelens` server key, unless the
  host adapter requires a new key for technical reasons

The public product name can change before every legacy token is removed.
Compatibility matters more than aesthetic purity.

## 8. Verification checklist

Run this after the rename branch is complete and before any publish:

### Build + tests

- `cargo check`
- `cargo test -p codelens-engine`
- `cargo test -p codelens-mcp`
- `cargo test -p codelens-mcp --features http`

### Release / packaging

- release workflow dry run produces only `symbiote-*` primary artifacts
- Formula templating resolves the new archive names
- installer script downloads and verifies the renamed binary

### Host adapters

- `attach codex` writes the expected config using the new executable
- `attach claude-code`, `attach cursor`, `attach cline`, `attach windsurf`
  all emit correct host-native examples
- `detach --dry-run` removes only the generated Symbiote entries

### Runtime compatibility

- `symbiote://...` and `codelens://...` resources both resolve during
  the bridge window
- `SYMBIOTE_*` and `CODELENS_*` env vars both function during the bridge
  window
- surface manifest and server card show the new primary product name

### Search audit

Run targeted searches and inspect only actionable remaining hits:

- public/install surface residuals
- runtime identity residuals
- package publish residuals

Historical release notes, migration docs, and compatibility shims are
allowed to retain `codelens-*`.

## 9. Practical constraints discovered in the current repository

These are the main reasons the cutover must be staged:

- Release workflows still package `codelens-mcp-*` archives and publish
  a `Formula/codelens-mcp.rb` artifact path.
- Root workspace metadata still points to
  `github.com/mupozg823/codelens-mcp-plugin`.
- Runtime manifests still expose `server_name: "codelens-mcp"` in the
  generated surface.
- Install metadata still advertises `cargo install codelens-mcp`,
  `brew install .../codelens-mcp`, and the existing raw GitHub install
  URL.
- Generated SBOM and manifest files still encode old package and binary
  names; they need regeneration from updated sources.

## 10. Decision rule for the implementation session

If a rename change does not improve one of these outcomes, it should not
ship in the Phase 3 cutover:

- lower first-run user confusion
- keep host attachment working with minimal edits
- preserve compatibility during the bridge window
- produce a coherent release artifact set
- reduce irreversible publish risk

That is the bar. Cosmetic global replacement is not.
