# Release distribution

CodeLens ships on five channels. Each release triggers the
`.github/workflows/release.yml` workflow on tag push (`v*`), which
builds three-OS binaries, publishes a GitHub Release, pushes an OCI
image to GHCR, and optionally syncs a Homebrew tap formula plus
crates.io publishes. The optional channels guard on secrets being set
so a repo with no secrets configured still produces a clean green
release.

## Channel inventory

| Channel        | Format                         | Enabled by                    | Registry/URL                                              |
| -------------- | ------------------------------ | ----------------------------- | --------------------------------------------------------- |
| GitHub Release | `tar.gz` + SBOM + Sigstore sig | always                        | https://github.com/mupozg823/codelens-mcp-plugin/releases |
| GHCR OCI image | Docker image                   | always                        | `ghcr.io/mupozg823/codelens-mcp-plugin:<tag>`             |
| crates.io      | `cargo install`                | `CARGO_REGISTRY_TOKEN` secret | https://crates.io/crates/codelens-mcp                     |
| Homebrew tap   | `brew install`                 | `TAP_GITHUB_TOKEN` secret     | https://github.com/mupozg823/homebrew-tap                 |
| Source         | git tag                        | always                        | https://github.com/mupozg823/codelens-mcp-plugin          |

## One-time operator setup

Two repo secrets make the optional channels fire automatically on tag
push. Both jobs cleanly skip when their secret is absent — a missing
token should not fail the workflow.

### `CARGO_REGISTRY_TOKEN` — crates.io

1. Visit https://crates.io/settings/tokens.
2. Create a token scoped to `publish-new` and `publish-update` on the
   three `codelens-*` crates (or account-wide during initial setup).
3. In the GitHub repository, go to Settings → Secrets and variables →
   Actions → New repository secret. Name: `CARGO_REGISTRY_TOKEN`.
   Value: the token string.

Verify with `gh secret list -R mupozg823/codelens-mcp-plugin`.

### `TAP_GITHUB_TOKEN` — Homebrew tap

The Homebrew tap lives in a separate public repo
(`mupozg823/homebrew-tap`). Releasing pushes a new `codelens-mcp.rb`
formula into `Formula/` on that repo's `main` branch.

1. Generate a fine-grained Personal Access Token.
   - Repository access: `mupozg823/homebrew-tap` only
   - Permissions: `Contents: Read and write`, `Metadata: Read-only`
2. Store it as the `TAP_GITHUB_TOKEN` secret on the source repo using
   the same Actions secrets panel.

## Release flow (tag push)

```text
git tag -a vX.Y.Z -m "…"
git push origin vX.Y.Z
```

This triggers the jobs in `.github/workflows/release.yml`:

```
build (linux/darwin/windows)
        └── publish (GitHub Release + GHCR image)
                    ├── update-homebrew      ← needs TAP_GITHUB_TOKEN
                    └── publish-crates-io    ← needs CARGO_REGISTRY_TOKEN
```

`update-homebrew` and `publish-crates-io` run in parallel after the
GitHub Release exists and the tag is visible to the registry CDN.

## Manual fallback

If automation is unavailable (secret missing during a release),
publish by hand from a clean working tree at the tagged commit.

### crates.io

```bash
git checkout vX.Y.Z
cargo publish -p codelens-engine   # wait for "Published" line
cargo publish -p codelens-mcp
cargo publish -p codelens-tui
```

Order matters: `codelens-mcp` and `codelens-tui` depend on
`codelens-engine` by exact version, so the engine must reach the index
first. Each `cargo publish` internally waits for index propagation
before returning.

### Homebrew tap

```bash
git clone https://github.com/mupozg823/homebrew-tap
cd homebrew-tap
# Regenerate Formula/codelens-mcp.rb with the new checksums.
curl -fsSL "https://github.com/mupozg823/codelens-mcp-plugin/releases/download/vX.Y.Z/checksums-sha256.txt" \
  -o /tmp/checksums.txt
SHA_DARWIN=$(awk '/codelens-mcp-darwin-arm64.tar.gz$/ {print $1}' /tmp/checksums.txt)
SHA_LINUX=$(awk '/codelens-mcp-linux-x86_64.tar.gz$/ {print $1}' /tmp/checksums.txt)
sed -e "s/RELEASE_SHA256_DARWIN_ARM64/${SHA_DARWIN}/" \
    -e "s/RELEASE_SHA256_LINUX_X86_64/${SHA_LINUX}/" \
    -e 's/version ".*"/version "X.Y.Z"/' \
    ../codelens-mcp-plugin/Formula/codelens-mcp.rb > Formula/codelens-mcp.rb
git add Formula/codelens-mcp.rb
git commit -m "codelens-mcp X.Y.Z"
git push
```

## Post-release verification

Run these checks after any release to confirm every channel reached
the intended version.

```bash
VERSION=X.Y.Z

# GitHub Release
gh release view "v${VERSION}" -R mupozg823/codelens-mcp-plugin \
  --json tagName,name,assets -q '.assets | length'

# GHCR
curl -fsSL -o /dev/null -w '%{http_code}\n' \
  "https://ghcr.io/v2/mupozg823/codelens-mcp-plugin/manifests/v${VERSION}"

# crates.io
for c in codelens-engine codelens-mcp codelens-tui; do
  curl -s "https://crates.io/api/v1/crates/${c}" \
    | python3 -c "import sys,json; print('${c}:', json.load(sys.stdin)['crate']['newest_version'])"
done

# Homebrew tap
curl -fsSL "https://raw.githubusercontent.com/mupozg823/homebrew-tap/main/Formula/codelens-mcp.rb" \
  | grep '^  version '
```

All four commands should report the new `VERSION`. If any reports an
older version, the workflow log for that job will show whether the
secret was missing (clean skip) or the step actually failed.

## User install cheatsheet

| Platform                 | Command                                                                                         |
| ------------------------ | ----------------------------------------------------------------------------------------------- |
| macOS/Linux via Homebrew | `brew tap mupozg823/tap && brew install codelens-mcp`                                           |
| Cargo from crates.io     | `cargo install codelens-mcp`                                                                    |
| Docker                   | `docker pull ghcr.io/mupozg823/codelens-mcp-plugin:<tag>`                                       |
| Binary download          | `gh release download <tag> -R mupozg823/codelens-mcp-plugin`                                    |
| Cargo from git           | `cargo install --git https://github.com/mupozg823/codelens-mcp-plugin --tag <tag> codelens-mcp` |

`cargo install codelens-mcp` bundles the CodeSearchNet semantic model
via the default `semantic` feature; expect the first build to take
several minutes for `fastembed` and `ort`. For the lighter tree-sitter
only build, use `cargo install codelens-mcp --no-default-features`.
