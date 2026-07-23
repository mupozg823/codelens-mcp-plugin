# Release distribution

CodeLens ships on six public distribution paths. Each release triggers the
`.github/workflows/release.yml` workflow on tag push (`v*`), which
builds three-OS binaries, publishes a GitHub Release, pushes an OCI
image to GHCR, and optionally syncs a Homebrew tap formula plus
crates.io publishes. The optional channels guard on secrets being set
so a repo with no secrets configured still produces a clean green
release.

**See also**: [`docs/release-verification.md`](release-verification.md)
for the auditor-side playbook — how to verify a published release from
the outside (Sigstore signatures, GitHub artifact attestations, SBOM
comparison, GHCR image pull + digest check). This file covers what
operators do to produce a release; that file covers what consumers do
to validate it.

## Channel inventory

| Channel             | Format                         | Enabled by                    | Registry/URL                                              |
| ------------------- | ------------------------------ | ----------------------------- | --------------------------------------------------------- |
| GitHub Release      | `tar.gz` + SBOM + Sigstore sig | always                        | https://github.com/mupozg823/codelens-mcp-plugin/releases |
| GitHub installer    | `install.sh` binary bootstrap  | always                        | https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh |
| GHCR OCI image      | Docker image                   | always                        | `ghcr.io/mupozg823/codelens-mcp-plugin:<tag>`             |
| crates.io           | `cargo install`                | `CARGO_REGISTRY_TOKEN` secret | https://crates.io/crates/codelens-mcp                     |
| Homebrew tap        | `brew install`                 | `TAP_GITHUB_TOKEN` secret     | https://github.com/mupozg823/homebrew-tap                 |
| Source              | git tag                        | always                        | https://github.com/mupozg823/codelens-mcp-plugin          |

## Public product line

Use this canonical line for GitHub repository metadata, plugin marketplace
cards, release summaries, and deployment pages:

> Rust MCP code intelligence for coding agents — live indexes, bounded hybrid
> retrieval, host-neutral workflows, single-writer sessions, and mutation gates.

### Localized deployment-page copy

These descriptions are localized text only. They do not imply region-specific
binaries, region-specific support, or different release artifacts.

| Locale / market | Short deployment-page description |
| --------------- | --------------------------------- |
| `en-US` / United States | Rust MCP code intelligence for coding agents — live indexes, bounded hybrid retrieval, host-neutral workflows, single-writer sessions, and mutation gates. |
| `ko-KR` / Korea | CodeLens는 코딩 에이전트를 위한 Rust MCP 코드 인텔리전스입니다. 실시간 인덱스, 범위가 제한된 하이브리드 검색, 호스트 중립 워크플로, 단일 라이터 세션, 변이 게이트를 제공합니다. |
| `ja-JP` / Japan | CodeLens はコーディングエージェント向けの Rust MCP コードインテリジェンスです。ライブインデックス、範囲を限定したハイブリッド検索、ホスト中立のワークフロー、シングルライターセッション、ミューテーションゲートを提供します。 |
| `zh-Hans` / China | CodeLens 是面向编码代理的 Rust MCP 代码智能工具，提供实时索引、范围受控的混合检索、主机无关工作流、单写入者会话和变更门控。 |
| `es-ES` / Spain | CodeLens ofrece inteligencia de código MCP en Rust para agentes de programación, con índices activos, recuperación híbrida acotada, flujos de trabajo independientes del host, sesiones de escritor único y puertas de mutación. |
| `de-DE` / Germany | CodeLens bietet Rust-MCP-Code-Intelligence für Coding-Agenten — mit Live-Indizes, begrenzter hybrider Suche, hostneutralen Workflows, Single-Writer-Sitzungen und Mutation Gates. |
| `fr-FR` / France | CodeLens fournit une intelligence de code MCP en Rust pour les agents de codage, avec des index actifs, une recherche hybride bornée, des workflows indépendants de l'hôte, des sessions à rédacteur unique et des barrières de mutation. |

Localized install callouts should point to the same channel matrix:

- Rust users: `cargo install codelens-mcp`
- prebuilt binary users: `curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash`
- macOS/Linux package users: `brew install mupozg823/tap/codelens-mcp`
- container users: `docker pull ghcr.io/mupozg823/codelens-mcp-plugin:<tag>`

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
```

Order matters: `codelens-mcp` depends on
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
for c in codelens-engine codelens-mcp; do
  curl -s "https://crates.io/api/v1/crates/${c}" \
    | python3 -c "import sys,json; print('${c}:', json.load(sys.stdin)['crate']['newest_version'])"
done

# Homebrew tap
curl -fsSL "https://raw.githubusercontent.com/mupozg823/homebrew-tap/main/Formula/codelens-mcp.rb" \
  | grep '^  version '

# Public installer/Homebrew transcript plan and live metadata smoke
python3 scripts/public_release_channel_smoke.py --version "${VERSION}"
python3 scripts/public_release_channel_smoke.py --version "${VERSION}" --mode metadata
```

All four commands should report the new `VERSION`. If any reports an
older version, the workflow log for that job will show whether the
secret was missing (clean skip) or the step actually failed.

For the publish-evidence transcript, also run `--mode installer` on a disposable
machine or CI runner. That mode isolates `HOME` and `CODELENS_INSTALL_DIR`, then
reuses the clean quickstart smoke against the installed binary and model sidecar.
Use `--mode homebrew-info` to verify the tapped formula version without
installing into the user's Homebrew prefix.

## User install cheatsheet

| Platform                 | Command                                                                                         |
| ------------------------ | ----------------------------------------------------------------------------------------------- |
| macOS/Linux via Homebrew | `brew tap mupozg823/tap && brew install codelens-mcp`                                           |
| Cargo from crates.io     | `cargo install codelens-mcp`                                                                    |
| Docker                   | `docker pull ghcr.io/mupozg823/codelens-mcp-plugin:<tag>`                                       |
| Binary download          | `gh release download <tag> -R mupozg823/codelens-mcp-plugin`                                    |
| Cargo from git           | `cargo install --git https://github.com/mupozg823/codelens-mcp-plugin --tag <tag> codelens-mcp` |

`cargo install codelens-mcp` installs the lean BM25 + AST + call-graph
binary by default. For semantic retrieval from crates.io, install with
`--features semantic` and point `CODELENS_MODEL_DIR` at a model sidecar
from a release archive. GitHub Release, installer, and Homebrew channels
are the release-tarball-equivalent paths that bundle or stage the
CodeSearchNet model payload. The Homebrew formula installs the `models/`
directory into the package prefix, and the clean quickstart smoke covers that
Cellar-style layout with `scripts/smoke-clean-quickstart.py --homebrew-layout`.
