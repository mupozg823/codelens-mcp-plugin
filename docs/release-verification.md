# CodeLens MCP — Release Verification and Packaging Status

This document is the operational reference for what the current release pipeline produces, how to verify a published release, and what still needs to land before CodeLens can claim supply-chain-grade packaging.

It is intentionally split into:

- **current verified state**
- **current gaps**
- **next implementation roadmap**

That keeps public packaging claims grounded in what the repository actually ships today.

---

## Current release outputs

The tag-driven GitHub release workflow in [`.github/workflows/release.yml`](../.github/workflows/release.yml) currently builds and publishes three release artifacts:

| Target | Archive | Payload |
| ------ | ------- | ------- |
| `darwin-arm64` | `codelens-mcp-darwin-arm64.tar.gz` | `codelens-mcp` |
| `linux-x86_64` | `codelens-mcp-linux-x86_64.tar.gz` | `codelens-mcp` |
| `windows-x86_64` | `codelens-mcp-windows-x86_64.zip` | `codelens-mcp.exe` |

The workflow also publishes:

- `checksums-sha256.txt`

and then uses those release checksums to update the Homebrew tap formula.

### What is currently true

- release archives are built in CI from tagged source
- SHA-256 checksums are published alongside the assets
- Homebrew is derived from the published release checksums rather than from a separate manual path
- release notes can be generated from GitHub plus repository-maintained notes under [`docs/release-notes`](release-notes)

### What is **not** currently true

- no SBOM is generated in the release workflow
- no provenance statement is generated in the release workflow
- no artifact signing step exists
- no OCI image is produced
- no air-gapped bundle is produced

Those are roadmap items, not shipped capabilities.

---

## How to verify a published release

### 1. Download a release bundle

Download the published archives and `checksums-sha256.txt` from a tagged GitHub release into a single directory.

### 2. Run the local verification script

```bash
scripts/verify-release-artifacts.sh ./release-bundle
```

By default the script requires all current release targets:

- `darwin-arm64`
- `linux-x86_64`
- `windows-x86_64`

It verifies:

1. every asset referenced by `checksums-sha256.txt` exists
2. every checksum matches
3. each tarball contains exactly one `codelens-mcp` binary
4. each zip contains exactly one `codelens-mcp.exe`

### 3. Verify only a subset of targets when needed

```bash
scripts/verify-release-artifacts.sh ./release-bundle \
  --require-targets darwin-arm64,linux-x86_64
```

This is useful for partial mirrors or internal staging environments.

---

## Release packaging status by enterprise gate

| Gate | Current state | Status |
| ---- | ------------- | ------ |
| Reproducible tagged binary builds | GitHub Actions release workflow | Partial |
| Published checksums | `checksums-sha256.txt` | Present |
| Local artifact verification path | `scripts/verify-release-artifacts.sh` | Present |
| Homebrew derivation from published assets | Implemented in release workflow | Present |
| SBOM generation | Not implemented | Missing |
| Provenance generation | Not implemented | Missing |
| Signature verification path | Not implemented | Missing |
| OCI image publishing | Not implemented | Missing |
| Air-gapped bundle | Not implemented | Missing |

Interpretation:

- CodeLens already has a workable release path for standard developer installs.
- CodeLens does **not** yet meet a strict enterprise supply-chain bar.
- The missing pieces are packaging/provenance work, not more retrieval features.

---

## Next implementation roadmap

### P0: make current release artifacts auditable

1. Keep the checksum-based release verification path green.
2. Fail release promotion if expected target archives are missing.
3. Publish one stable verification document and keep it version-agnostic.

This document plus `scripts/verify-release-artifacts.sh` closes that P0 gap.

### P1: add supply-chain metadata

1. Generate a CycloneDX SBOM in the release workflow.
2. Publish provenance metadata for each tagged release.
3. Document how operators validate both the checksum and provenance layers.

This is the minimum bar to move from "developer-grade release assets" to "enterprise-reviewable release assets."

### P2: add enterprise delivery formats

1. Publish an OCI image for daemon deployments.
2. Publish an air-gapped bundle containing:
   - binary
   - model assets
   - checksums
   - SBOM
   - provenance
   - example configs

This is the point where CodeLens becomes materially easier to operate in locked-down environments.

---

## Related references

- [Architecture overview](architecture.md)
- [Platform setup](platform-setup.md)
- [ADR-0002 enterprise productization](adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md)
- [v1.9.14 release notes](release-notes/v1.9.14.md)
