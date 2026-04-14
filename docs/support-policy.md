# CodeLens MCP Support Policy

This document defines the release compatibility and support contract for
CodeLens MCP.

## Versioning

CodeLens MCP follows Semantic Versioning for published releases.

- `MAJOR` releases may introduce breaking changes to tool names, profile
  behavior, resource shapes, or transport contracts.
- `MINOR` releases are additive by default. New tools, new output fields, new
  profiles, and new feature flags may be added, but existing published
  contracts should continue to work.
- `PATCH` releases are for bug fixes, documentation, packaging, release
  hygiene, observability fixes, and narrowly scoped behavior corrections. Patch
  releases must not intentionally remove existing tool names or break existing
  response shapes within the same minor line.

## Compatibility Surface

The compatibility contract applies to:

- MCP tool names and profile names
- `schema_version` response envelopes
- documented resource URIs
- release artifact naming and manifest structure
- published installation paths documented in the README and platform setup docs

Within a minor line, additive response fields are allowed. Callers should treat
unknown fields as forward-compatible.

## Support Windows

CodeLens does not currently maintain a separate multi-year branch branded as an
independent LTS edition. Until such a branch is explicitly announced, support is
defined by minor lines:

- Active support: the latest released minor line
- Maintenance support: the immediately previous minor line
- End of support: anything older than the previous minor line

In practice this means:

- new features land only on the latest minor line
- critical fixes may be backported to the previous minor line when risk is low
- older minor lines are best-effort only and may be asked to upgrade first

## Deprecation Policy

Breaking removals should not happen inside a minor line.

- A tool, profile, or documented behavior must be marked deprecated before
  removal.
- Deprecations should remain available for at least one minor release before
  removal unless there is a security or correctness issue that requires faster
  action.
- The replacement path should be documented in release notes or platform docs.

## Release Documentation Requirements

Each release line should ship with:

- a release note under `docs/release-notes/`
- a matching changelog pointer in `CHANGELOG.md`
- release verification assets when applicable:
  `release-manifest.json`, signatures, attestations, SBOM, and archive checksums

Use `python3 scripts/check-release-docs.py` to verify the documentation side of
that contract for the current workspace version.
