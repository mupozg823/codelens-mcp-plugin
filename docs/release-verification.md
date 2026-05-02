# CodeLens MCP — Release Verification and Packaging Status

This document is the operational reference for what the current release pipeline is configured to produce, how to verify a published release, and what still needs to land before CodeLens can claim supply-chain-grade packaging.

**See also**: [`docs/release-distribution.md`](release-distribution.md)
for the producer-side playbook — tag-push flow, manual fallback commands,
post-release verification script, user install cheatsheet. That file covers
what operators do to produce a release; this file covers what consumers do
to validate it.

It is intentionally split into:

- **current verified state**
- **current gaps**
- **next implementation roadmap**

That keeps public packaging claims grounded in what the repository actually ships today.

---

## Feature-flag matrix (build-time requirements)

ADR-0012 made the `semantic` feature opt-in on the cargo-install path. As of v1.10.1, the feature requirements for each runtime mode are:

| Use case                                         | Required cargo features    | Notes                                                                                                                         |
| ------------------------------------------------ | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `cargo install codelens-mcp` (stdio MCP)         | `default = []` (none)      | BM25 + AST + call-graph only. No HTTP, no embeddings.                                                                         |
| `cargo install codelens-mcp --features semantic` | `semantic`                 | Adds ONNX hybrid retrieval. Requires `CODELENS_MODEL_DIR` or a release tarball with bundled model.                            |
| HTTP daemon (any port)                           | `--features http`          | The HTTP transport entrypoint is gated. Daemons fail with `Error: HTTP/HTTPS transport requires the http feature` if missing. |
| HTTP daemon **with** semantic                    | `--features http,semantic` | Recommended for production daemons that serve hybrid retrieval.                                                               |
| OpenTelemetry export                             | `--features otel`          | Independent of HTTP/semantic.                                                                                                 |
| Audit feature flag (deprecated)                  | n/a                        | Removed in ADR-0011.                                                                                                          |

**Operational implication for `launchd` / `systemd` / Docker daemons**: the binary you launch from the unit/service file must have been built with `--features http` (and `semantic` if you want hybrid retrieval). The launchd `.plist` files in this repo (`.codelens/launchd/dev.codelens.mcp-readonly.plist`, `dev.codelens.mcp-mutation.plist`) point at `target/release/codelens-mcp`, so the build command for the daemon stack is:

```bash
cargo build --release --features http,semantic
```

If you only want the stdio-mode binary that `cargo install codelens-mcp` produces, no build flags are needed. If you want to run the HTTP daemon stack from a `cargo install`-style binary, the equivalent is:

```bash
cargo install codelens-mcp --features http,semantic
```

This was a v1.10.0 release-time discovery (the daemon failed to bind on first reboot until rebuilt with the right features). See [`docs/eval/v1.10.0-post-release-eval.md`](eval/v1.10.0-post-release-eval.md) (F5) for the full RCA.

---

## Configured release outputs

The tag-driven GitHub release workflow in [`.github/workflows/release.yml`](../.github/workflows/release.yml) currently builds and publishes three release artifacts:

| Target           | Archive                            | Payload            |
| ---------------- | ---------------------------------- | ------------------ |
| `darwin-arm64`   | `codelens-mcp-darwin-arm64.tar.gz` | `codelens-mcp`     |
| `linux-x86_64`   | `codelens-mcp-linux-x86_64.tar.gz` | `codelens-mcp`     |
| `windows-x86_64` | `codelens-mcp-windows-x86_64.zip`  | `codelens-mcp.exe` |

The workflow is also configured to publish:

- `checksums-sha256.txt`
- `release-manifest.json`
- `*.sig` keyless blob signatures for release payloads
- `*.pem` Fulcio-issued signing certificates for release payloads
- `codelens-mcp-<target>.cdx.json` per-target CycloneDX SBOM files
- a Linux OCI image to GHCR from the released `linux-x86_64` archive payload
- `codelens-mcp-airgap-linux-x86_64.tar.gz` when bundled model assets are staged in CI

The workflow is also configured to generate GitHub artifact attestations for each packaged archive:

- one provenance attestation
- one SBOM attestation bound to the corresponding archive

and then uses those release checksums to update the Homebrew tap formula.

### What is currently true in repository configuration

- release archives are built in CI from tagged source
- SHA-256 checksums are published alongside the assets
- keyless blob signatures and signing certificates are published for each archive, SBOM, any emitted air-gapped bundle, and `release-manifest.json`
- per-target CycloneDX SBOMs are generated in the release workflow
- provenance and SBOM attestations are generated in the release workflow
- an OCI image is built from the released Linux binary and pushed to GHCR
- a machine-readable `release-manifest.json` is generated from the checksum set before publication
- the publish job verifies the assembled release bundle locally before creating the GitHub release
- Homebrew is derived from the published release checksums rather than from a separate manual path
- release notes can be generated from GitHub plus repository-maintained notes under [`docs/release-notes`](release-notes)

### What is **not** currently true

- no Sigstore bundle export is mirrored as a plain release asset
- `checksums-sha256.txt` itself is not separately signed
- the repository checkout used in CI does not currently stage `model.onnx`, so the air-gapped bundle is skipped by default

The remaining items are roadmap gaps, not shipped capabilities.

---

## How to verify a published release

### 1. Download a release bundle

Download the published archives, `release-manifest.json`, matching `*.sig` and `*.pem` sidecars, per-target SBOM files, and `checksums-sha256.txt` from a tagged GitHub release into a single directory.

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
5. each `*.cdx.json` file is valid JSON and declares a CycloneDX SBOM for `codelens-mcp`
6. each emitted `codelens-mcp-airgap-*.tar.gz` bundle contains the binary, bundled model assets, examples, manifest, and internally valid checksums
7. `release-manifest.json` matches the checksum manifest and enumerates the published assets
8. each signable release payload has a non-empty `.sig` sidecar and a non-empty `.pem` certificate sidecar

### 3. Verify only a subset of targets when needed

```bash
scripts/verify-release-artifacts.sh ./release-bundle \
  --require-targets darwin-arm64,linux-x86_64
```

This is useful for partial mirrors or internal staging environments.

### 4. Verify provenance attestation from GitHub

For provenance verification against GitHub attestations:

```bash
gh attestation verify ./codelens-mcp-linux-x86_64.tar.gz \
  --repo mupozg823/codelens-mcp-plugin \
  --signer-workflow mupozg823/codelens-mcp-plugin/.github/workflows/release.yml
```

This verifies the default SLSA provenance predicate bound to the artifact.

### 5. Verify a keyless blob signature

Example for the Linux archive:

```bash
cosign verify-blob codelens-mcp-linux-x86_64.tar.gz \
  --signature codelens-mcp-linux-x86_64.tar.gz.sig \
  --certificate codelens-mcp-linux-x86_64.tar.gz.pem \
  --certificate-identity-regexp "https://github.com/mupozg823/codelens-mcp-plugin/.github/workflows/release.yml@refs/tags/.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com"
```

Use the same pattern for:

- `codelens-mcp-<target>.cdx.json`
- `codelens-mcp-airgap-linux-x86_64.tar.gz` when present
- `release-manifest.json`

### 6. Pull the published OCI image

Once a new tag has gone through the release workflow, the container image is published to:

```text
ghcr.io/mupozg823/codelens-mcp-plugin:<tag>
```

Example:

```bash
docker pull ghcr.io/mupozg823/codelens-mcp-plugin:1.9.30
```

### 7. Use the air-gapped bundle when present

When the bundled model payload is staged for a release, the workflow also publishes a Linux offline bundle:

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

## Current configured attestation model

Each matrix build is configured to create:

- one provenance attestation for the packaged release archive
- one SBOM attestation using the generated CycloneDX JSON for that same archive

The release assets themselves remain the archives, SBOM files, and checksum manifest. The attestation bundles are stored through GitHub's attestation API rather than mirrored as plain release assets.

The OCI image is built from the released `linux-x86_64` binary artifact rather than from a second Rust compilation path. That keeps the container packaging path aligned with the release archive it represents.

---

## Release packaging status by enterprise gate

| Gate                                      | Current state                                                                  | Status      |
| ----------------------------------------- | ------------------------------------------------------------------------------ | ----------- |
| Reproducible tagged binary builds         | GitHub Actions release workflow                                                | Partial     |
| Published checksums                       | `checksums-sha256.txt`                                                         | Present     |
| Published release inventory               | `release-manifest.json`                                                        | Present     |
| Published keyless signatures              | `*.sig` + `*.pem` sidecars                                                     | Present     |
| Published per-target CycloneDX SBOMs      | release workflow configured                                                    | Present     |
| GitHub provenance attestation             | `actions/attest@v4` in release workflow                                        | Present     |
| GitHub SBOM attestation                   | `actions/attest@v4` in release workflow                                        | Present     |
| OCI image publishing                      | GHCR via `docker/build-push-action`                                            | Present     |
| Air-gapped bundle                         | Linux offline tarball via `build-airgap-bundle.sh` when `model.onnx` is staged | Conditional |
| Local artifact verification path          | `scripts/verify-release-artifacts.sh`                                          | Present     |
| Homebrew derivation from published assets | Implemented in release workflow                                                | Present     |
| Signature verification path               | Cosign keyless blob verification for release payloads                          | Present     |

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

1. Export or mirror Sigstore bundles so transparency-log evidence is available without a live Rekor lookup.
2. Decide whether `checksums-sha256.txt` should also be signed or replaced operationally by the signed release manifest.
3. Document explicit verification policy for SBOM attestations, not just provenance attestations.

This is the minimum bar to move from "developer-grade release assets" to "enterprise-reviewable signed release assets."

### P2: add enterprise delivery formats

1. Restore a first-class air-gapped bundle path containing:
   - additional target variants beyond Linux x86_64
   - explicit provenance export for fully offline verification

2. Add an explicit verification playbook for registry-hosted OCI provenance/SBOM attestations.

This is the point where CodeLens becomes materially easier to operate in locked-down environments.

---

## Related references

- [Architecture overview](architecture.md)
- [Platform setup](platform-setup.md)
- [ADR-0002 enterprise productization](adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md)
- [GitHub Release v1.9.30](https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.30)
