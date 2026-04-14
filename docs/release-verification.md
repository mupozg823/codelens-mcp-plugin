# CodeLens MCP — Release Verification and Packaging Status

This document is the operational reference for what the current release pipeline is configured to produce, how to verify a published release, and what still needs to land before CodeLens can claim supply-chain-grade packaging.

It is intentionally split into:

- **current verified state**
- **current gaps**
- **next implementation roadmap**

That keeps public packaging claims grounded in what the repository actually ships today.

---

## Configured release outputs

The tag-driven GitHub release workflow in [`.github/workflows/release.yml`](../.github/workflows/release.yml) currently builds and publishes three release artifacts:

| Target | Archive | Payload |
| ------ | ------- | ------- |
| `darwin-arm64` | `codelens-mcp-darwin-arm64.tar.gz` | `codelens-mcp` |
| `linux-x86_64` | `codelens-mcp-linux-x86_64.tar.gz` | `codelens-mcp` |
| `windows-x86_64` | `codelens-mcp-windows-x86_64.zip` | `codelens-mcp.exe` |

The workflow is also configured to publish:

- Optional: `checksums-sha256.txt` plus sidecars (`.sig`, `.pem`, `.sigstore.json`) if you publish the supplemental checksum layer.
- `release-manifest.json`
- `sigstore-trusted-root.jsonl`
- `*.sigstore.json` Sigstore bundle sidecars for release payloads
- `*.sig` keyless blob signatures for release payloads
- `*.pem` Fulcio-issued signing certificates for release payloads
- `codelens-mcp-<target>.cdx.json` per-target CycloneDX SBOM files
- a Linux OCI image to GHCR from the released `linux-x86_64` archive payload
- `codelens-mcp-airgap-linux-x86_64.tar.gz` self-contained offline bundle

The workflow is also configured to generate GitHub artifact attestations for each packaged archive:

- one provenance attestation
- one SBOM attestation bound to the corresponding archive

and then uses the release manifest checksums to update the Homebrew tap formula.

### What is currently true in repository configuration

- release archives are built in CI from tagged source
- SHA-256 checksums are computed from the authoritative release manifest
- the checksum manifest itself is separately signed and shipped with its own certificate
- Sigstore bundle files are mirrored as plain release assets for each signable payload and for the checksum manifest
- a Sigstore `trusted_root.jsonl` snapshot is mirrored as a plain release asset for offline bootstrap
- `release-manifest.json` is marked as the authoritative inventory for release payloads
- keyless blob signatures and signing certificates are published for each archive, SBOM, air-gapped bundle, and `release-manifest.json`
- per-target CycloneDX SBOMs are generated in the release workflow
- provenance and SBOM attestations are generated in the release workflow
- an OCI image is built from the released Linux binary and pushed to GHCR
- an air-gapped Linux bundle is assembled from the released Linux binary plus bundled model assets
- a machine-readable `release-manifest.json` is generated from the checksum set before publication
- the publish job verifies the assembled release bundle locally before creating the GitHub release
- a reusable GitHub attestation verification script exists for both provenance and SBOM policy checks
- Homebrew is derived from the release manifest rather than from a separate manual path
- release notes can be generated from GitHub plus repository-maintained notes under [`docs/release-notes`](release-notes)

### What is **not** currently true

- no fully pinned TUF mirror policy is shipped beyond the release-scoped `trusted_root.jsonl` snapshot

The remaining items are roadmap gaps, not shipped capabilities.

---

## How to verify a published release

### 1. Download a release bundle

Download the published archives, `release-manifest.json`, `sigstore-trusted-root.jsonl`, matching `*.sig`, `*.pem`, and `*.sigstore.json` sidecars, and per-target SBOM files from a tagged GitHub release into a single directory. `checksums-sha256.txt` and its sidecars remain supported as a supplemental layer when present.

### 2. Run the local verification script

```bash
scripts/verify-release-artifacts.sh ./release-bundle \
  --require-bundles \
  --verify-bundles-with-cosign
```

By default the script requires all current release targets:

- `darwin-arm64`
- `linux-x86_64`
- `windows-x86_64`

It verifies:

1. `release-manifest.json` is structurally valid and acts as the authoritative inventory for release payloads
2. every payload named in `release-manifest.json` exists and matches the SHA-256 recorded in the manifest
3. if `checksums-sha256.txt` is present, it is treated as a supplemental integrity manifest and must cover every authoritative manifest payload with matching hashes
4. each tarball contains exactly one `codelens-mcp` binary
5. each zip contains exactly one `codelens-mcp.exe`
6. each `*.cdx.json` file is valid JSON and declares a CycloneDX SBOM for `codelens-mcp`
7. each `codelens-mcp-airgap-*.tar.gz` bundle contains the binary, bundled model assets, examples, manifest, and internally valid checksums
8. each signable release payload has a non-empty `.sig` sidecar and a parseable `.pem` certificate sidecar
9. each signable release payload has a `*.sigstore.json` bundle sidecar
10. if `checksums-sha256.txt` is present, it has its own `.sig`, `.pem`, and `.sigstore.json` sidecars
11. when `--verify-bundles-with-cosign` is enabled, each mirrored bundle also passes `cosign verify-blob` against the GitHub Actions signer identity without requiring a live Rekor lookup

### 3. Verify only a subset of targets when needed

```bash
scripts/verify-release-artifacts.sh ./release-bundle \
  --require-bundles \
  --require-targets darwin-arm64,linux-x86_64
```

This is useful for partial mirrors or internal staging environments.

Manifest-only verification is also valid:

```bash
scripts/verify-release-artifacts.sh ./release-bundle --require-bundles
```

That path uses `release-manifest.json` as the sole inventory source and skips supplemental checksum checks when the checksum file is absent.

### 4. Verify GitHub attestations with the repository script

For provenance and SBOM verification against GitHub attestations:

```bash
scripts/verify-github-attestations.sh ./release-bundle
```

This verifies, for every packaged target archive:

- the default SLSA provenance attestation
- the SBOM attestation using the configured CycloneDX predicate type
- the expected signer workflow identity
- the release-bundled `sigstore-trusted-root.jsonl` snapshot automatically, when present

To download bundle JSONL files for later offline verification:

```bash
scripts/verify-github-attestations.sh ./release-bundle \
  --download-bundles-dir ./attestation-bundles
```

To re-verify later without GitHub API access:

```bash
scripts/verify-github-attestations.sh ./release-bundle \
  --offline-bundles-dir ./attestation-bundles
```

If you need to override the bundled trusted root snapshot, pass:

```bash
scripts/verify-github-attestations.sh ./release-bundle \
  --custom-trusted-root ./trusted_root.jsonl
```

### 5. Verify one provenance attestation manually

If you need a single manual provenance check instead of the repository script:

```bash
gh attestation verify ./codelens-mcp-linux-x86_64.tar.gz \
  --repo mupozg823/codelens-mcp-plugin \
  --signer-workflow mupozg823/codelens-mcp-plugin/.github/workflows/release.yml
```

### 6. Verify a keyless Sigstore bundle

Example for the Linux archive:

```bash
cosign verify-blob codelens-mcp-linux-x86_64.tar.gz \
  --bundle codelens-mcp-linux-x86_64.tar.gz.sigstore.json \
  --certificate-identity-regexp "https://github.com/mupozg823/codelens-mcp-plugin/.github/workflows/release.yml@refs/tags/.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com"
```

Use the same pattern for:

- Optional: `checksums-sha256.txt`
- `codelens-mcp-<target>.cdx.json`
- `codelens-mcp-airgap-linux-x86_64.tar.gz`
- `release-manifest.json`

### 7. Verify a legacy `.sig` / `.pem` sidecar pair when needed

Example for the Linux archive:

```bash
cosign verify-blob codelens-mcp-linux-x86_64.tar.gz \
  --signature codelens-mcp-linux-x86_64.tar.gz.sig \
  --certificate codelens-mcp-linux-x86_64.tar.gz.pem \
  --certificate-identity-regexp "https://github.com/mupozg823/codelens-mcp-plugin/.github/workflows/release.yml@refs/tags/.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com"
```

### 8. Pull the published OCI image

Once a new tag has gone through the release workflow, the container image is published to:

```text
ghcr.io/mupozg823/codelens-mcp-plugin:<tag>
```

Example:

```bash
docker pull ghcr.io/mupozg823/codelens-mcp-plugin:1.9.14
```

### 9. Use the air-gapped bundle

The release workflow also publishes a Linux offline bundle:

```text
codelens-mcp-airgap-linux-x86_64.tar.gz
```

It contains:

- the `linux-x86_64` `codelens-mcp` binary
- `models/codesearch` semantic model assets
- the matching CycloneDX SBOM under `sbom/`
- internal checksums
- MCP and daemon examples

Once extracted, the binary can run in place and will resolve `./models/codesearch` automatically.

---

## Rust crate publish workflow

CodeLens publishes Rust crates from the workspace in dependency order:

1. `codelens-engine`
2. `codelens-mcp`
3. `codelens-tui`

This ordering is not optional. Both `codelens-mcp` and `codelens-tui` pin
`codelens-engine` to the exact workspace version, so a standalone:

```bash
cargo publish --dry-run -p codelens-mcp
```

will fail until the matching `codelens-engine` version is already visible on
crates.io.

Use the workspace publish helper instead:

```bash
scripts/publish-crates-workspace.sh --allow-dirty
```

Operational behavior:

- `codelens-engine` runs a full `cargo publish --dry-run`
- downstream crates fall back to `cargo check --locked` until the new engine
  version has propagated to crates.io
- real publish uses the same order and requires `--execute`

Examples:

```bash
# full workspace dry-run
scripts/publish-crates-workspace.sh --allow-dirty

# real crates.io publish in workspace order
scripts/publish-crates-workspace.sh --execute

# publish only after earlier workspace dependencies are already published
scripts/publish-crates-workspace.sh --execute --package codelens-mcp --skip-existing
```

For package-page quality, the crates.io page for `codelens-mcp` is sourced from
[`crates/codelens-mcp/README.md`](../crates/codelens-mcp/README.md), while the
crate metadata comes from [`crates/codelens-mcp/Cargo.toml`](../crates/codelens-mcp/Cargo.toml).

---

## Current configured attestation model

Each matrix build is configured to create:

- one provenance attestation for the packaged release archive
- one SBOM attestation using the generated CycloneDX JSON for that same archive

The release assets themselves now include the archives, SBOM files, an authoritative `release-manifest.json`, a supplemental checksum manifest, legacy `.sig` / `.pem` pairs, and mirrored `*.sigstore.json` bundle evidence. GitHub attestation bundles remain stored through GitHub's attestation API rather than mirrored as plain release assets.

The release bundle also includes `sigstore-trusted-root.jsonl`, exported from `gh attestation trusted-root`, so offline attestation verification can bootstrap without contacting the live TUF mirror.

The OCI image is built from the released `linux-x86_64` binary artifact rather than from a second Rust compilation path. That keeps the container packaging path aligned with the release archive it represents.

---

## Release packaging status by enterprise gate

| Gate | Current state | Status |
| ---- | ------------- | ------ |
| Reproducible tagged binary builds | GitHub Actions release workflow | Partial |
| Published checksums | `checksums-sha256.txt` | Optional |
| Signed checksum manifest | `checksums-sha256.txt.sig` + `.pem` | Optional |
| Published Sigstore bundles | `*.sigstore.json` | Present |
| Published trusted root snapshot | `sigstore-trusted-root.jsonl` | Present |
| Authoritative release inventory | `release-manifest.json` | Present |
| Published keyless signatures | `*.sig` + `*.pem` sidecars | Present |
| Published per-target CycloneDX SBOMs | release workflow configured | Present |
| GitHub provenance attestation | `actions/attest@v4` in release workflow | Present |
| GitHub SBOM attestation | `actions/attest@v4` in release workflow | Present |
| OCI image publishing | GHCR via `docker/build-push-action` | Present |
| Air-gapped bundle | Linux offline tarball via `build-airgap-bundle.sh` | Present |
| Local artifact verification path | `scripts/verify-release-artifacts.sh` | Present |
| Local GitHub attestation verification path | `scripts/verify-github-attestations.sh` | Present |
| Homebrew derivation from published assets | Implemented in release workflow | Present |
| Signature verification path | Cosign keyless blob verification for release payloads | Present |

Interpretation:

- CodeLens already has a workable release path for standard developer installs.
- CodeLens does **not** yet meet a strict enterprise supply-chain bar.
- The missing pieces are packaging/provenance work, not more retrieval features.

---

## Next implementation roadmap

### P0: make current release artifacts auditable

1. Keep the checksum-based release verification path green.
2. Keep the GitHub attestation path green on every tagged release.
3. Fail release promotion if expected target archives or SBOM files are missing.
4. Publish one stable verification document and keep it version-agnostic.

This document plus `scripts/verify-release-artifacts.sh` closes that P0 gap.

### P1: harden signature portability

1. Decide whether the release-scoped `sigstore-trusted-root.jsonl` snapshot is sufficient or whether a separately pinned TUF mirror policy should also be shipped.
2. Decide whether `checksums-sha256.txt` should remain a published convenience layer long-term now that the signed release manifest is treated as authoritative for payload inventory.
3. Keep the SBOM predicate type and signer-workflow policy pinned in documentation and scripts as the attestation surface evolves.

This is the minimum bar to move from "developer-grade release assets" to "enterprise-reviewable signed release assets."

### P2: add enterprise delivery formats

1. Publish an air-gapped bundle containing:
   - additional target variants beyond Linux x86_64
   - explicit provenance export for fully offline verification

2. Add an explicit verification playbook for registry-hosted OCI provenance/SBOM attestations.

This is the point where CodeLens becomes materially easier to operate in locked-down environments.

---

## Related references

- [Architecture overview](architecture.md)
- [Platform setup](platform-setup.md)
- [ADR-0002 enterprise productization](adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md)
- [v1.9.14 release notes](release-notes/v1.9.14.md)
