# CodeLens Documentation

## Directory Structure

| Directory | Contents | Audience |
|-----------|----------|----------|
| [`adr/`](adr/) | Architecture Decision Records (ADRs) | Contributors, maintainers |
| [`design/`](design/) | Design specs and policy documents | Contributors, reviewers |
| [`generated/`](generated/) | Auto-generated artifacts (surface manifest, schemas) | CI, tooling |
| [`release-notes/`](release-notes/) | Per-version release notes | Users, operators |
| [`schemas/`](schemas/) | JSON schemas for tool I/O and protocols | Client integrators |
| [`superpowers/`](superpowers/) | Agent skill definitions and implementation plans | Agent orchestrators |
| [`archive/`](archive/) | Superseded or historical documents | Archaeologists |

## Quick Links

- **ADR Index**: [`adr/README.md`](adr/README.md)
- **Surface Manifest**: [`generated/surface-manifest.json`](generated/surface-manifest.json)
- **Migration Guide**: [`migrate-from-codelens.md`](migrate-from-codelens.md)
- **Install Matrix**: See top-level [`README.md`](../README.md)

## Generated Artifacts

The following files are auto-generated and should not be edited manually:

- `generated/surface-manifest.json` — Built from `tool_defs.rs` via `cargo run -- --print-surface-manifest`
- `generated/tool-schemas/` — JSON schemas derived from Rust types

## Contributing

- ADRs: Follow the established format; update the index.
- Design docs: Place in `design/` with a clear scope and target version.
- Release notes: Use `release-plz` + `git-cliff`; do not edit `CHANGELOG.md` directly.
