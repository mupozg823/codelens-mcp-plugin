# ADR-0012: Move semantic feature default-off on the crates.io install path

## Status

Accepted

## Date

2026-05-02

## Context

`crates/codelens-mcp/Cargo.toml` declared `default = ["semantic"]`, so a
user running

```
cargo install codelens-mcp
```

implicitly opted into `codelens-engine/semantic`, which pulls in
`fastembed`, `ort` (ONNX Runtime), and `sqlite-vec`. The semantic stack
needs an ~80 MB sidecar model directory to be useful; crates.io does
not ship the model (the model is gitignored under
`crates/codelens-engine/models/` and only included in GitHub Release
tarballs).

The README itself acknowledges the resulting cliff:

> Semantic search additionally needs a sidecar model directory (~80 MB
> ONNX) — GitHub Release tarballs bundle it automatically, but users
> installing via `cargo install codelens-mcp` must point
> `CODELENS_MODEL_DIR` at a separately-fetched model payload.

In practice this means:

- `cargo install` users pay the heavy ONNX/fastembed/ort dependency
  graph at compile time — even before they discover the model is
  missing.
- The first `semantic_search` call returns a `FeatureUnavailable`
  error pointing at `index_embeddings`, which itself fails because the
  model directory is empty. There is no way for a fresh user to reach
  a green semantic surface from `cargo install` alone.
- The bench-data figure in the README ("67-87% token saving") is
  measured with semantic on. With semantic off — which is what most
  cargo-install-only users actually run after the first failure — the
  retrieval surface degrades to BM25-only. That fact is not surfaced.

The 2026-05-02 architecture audit (ADR-0011 §Neutral / Deferred) flagged
this onboarding cliff as a separate decision. ADR-0012 makes that
decision.

### Existing release-channel feature policy

The four user-facing channels already have different feature policies,
and the inconsistency is part of the problem:

| Channel                                               | Feature flags today                                                                  |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `cargo install codelens-mcp` (crates.io)              | `default = ["semantic"]` — semantic on, but no model shipped                         |
| GitHub Release tarball (linux-x86_64, windows-x86_64) | `--features "http"` — semantic OFF                                                   |
| GitHub Release tarball (darwin-arm64)                 | `--features "http,coreml"` — semantic on (via `coreml` → `semantic`) + model bundled |
| `install.sh`                                          | downloads the GitHub Release tarball, inherits per-target feature set                |
| Homebrew tap                                          | external repo `mupozg823/tap`, not in this repo's scope                              |

The contradiction is concentrated on the crates.io path: it claims
semantic but cannot deliver it, while the GitHub Release linux/win
builds already ship semantic-off and that is _not_ a problem because
those users selected the "tarball" path knowingly.

## Decision

Flip the crates.io default to `[]`. Three sub-decisions follow.

### 1. `Cargo.toml [features].default = []`

`crates/codelens-mcp/Cargo.toml`:

```toml
[features]
default = []
semantic = ["codelens-engine/semantic"]
coreml = ["semantic", "codelens-engine/coreml"]
…
```

Effect:

- `cargo install codelens-mcp` no longer compiles `fastembed`, `ort`,
  `sqlite-vec`; the binary is materially smaller and faster to install.
- Existing semantic users opt in explicitly:
  `cargo install codelens-mcp --features semantic` (model dir still
  has to be supplied separately, as today).
- All `#[cfg(feature = "semantic")]` gates already exist in the
  codebase (PR #125 audit); the dispatcher already returns
  `FeatureUnavailable` with a hint when semantic tools are called and
  the feature is off. No additional dispatch changes are required.

### 2. Startup banner makes the trade-off explicit

When the binary boots without `feature = "semantic"`, the stderr banner
now says so once:

```
codelens-mcp 1.x.x (semantic off)
  Hybrid retrieval is disabled. To enable it:
    cargo install codelens-mcp --features semantic
    or download a GitHub Release tarball (semantic + model bundled).
  BM25, AST, and call-graph tools work with no extra setup.
```

This avoids a silent "semantic_search returns 0 results" failure mode.
The banner is suppressed in stdio mode so JSON-RPC stdout stays clean —
banner goes to stderr per the existing `init_tracing` policy.

### 3. README repositions the bench claim

The "67-87% saving" line is genuine but it is measured with semantic on
and the bundled model. The README is updated so the headline number is
qualified ("with `--features semantic`"), and the install section gets
two parallel quick-paths:

- "Quick install (BM25 + AST only)" → `cargo install codelens-mcp`
- "Quick install (full hybrid retrieval)" → `cargo install codelens-mcp --features semantic` + a one-line `CODELENS_MODEL_DIR` setup, _or_ the GitHub Release tarball.

### Out-of-scope sub-decisions (deferred)

The following were considered and **not** taken in this ADR; each gets
its own decision when telemetry from this change accumulates:

- **Auto-download the model on first semantic call.** Tempting, but it
  introduces an outbound supply-chain dependency the binary did not
  previously have. Defer until model hosting is decided.
- **Make `coreml` an additive layer rather than `coreml = ["semantic", …]`.**
  The current shape (coreml implies semantic) is fine for the macOS
  GitHub Release path, but if we ever ship a coreml-only-no-onnx
  variant we will need to revisit. Out of scope.
- **A `CODELENS_AUTO_FETCH_MODEL` opt-in.** Same supply-chain note.
- **Adjust `default-members` in the workspace `Cargo.toml`.** Workspace
  default-members affects `cargo build` from the repo root; that is
  developer-only ergonomics and unrelated to the install-path UX. No
  change.

## Consequences

### Positive

- Onboarding cliff removed: a fresh `cargo install codelens-mcp`
  succeeds, the binary boots, the banner explains what semantic adds,
  and BM25 / AST / call-graph / mutation tools all work without a
  model directory.
- Compile time and binary size on the crates.io path drop noticeably —
  `fastembed`, `ort`, `sqlite-vec` are heavy.
- Honest README: the headline bench number now carries the feature
  qualifier, so the marketing claim and the actual default install
  match.
- A new telemetry counter pair (`semantic_engaged_calls` /
  `semantic_disabled_calls`) gives Track 2/3 of the deferred-items
  roadmap real install-path data instead of a guess.

### Negative

- Existing users who run `cargo install --force codelens-mcp` after
  this change will silently lose semantic, because they will not pass
  `--features semantic`. Mitigations:
  - Bump the published version to `1.10.0` (minor — feature default is
    a behaviour change, not a SemVer-breaking API change, but it is
    non-trivial enough to deserve a minor bump).
  - CHANGELOG.md entry under `1.10.0` calling out the flag explicitly.
  - The startup banner makes the change visible on the first run.
- Two parallel quick-install paths in the README slightly raise reader
  load. Mitigation: one default path is shown first, the semantic path
  is collapsed into a "Need NL retrieval?" expand block.

### Neutral

- GitHub Release tarballs are unchanged. The linux/win builds were
  already semantic-off, the darwin-arm64 build is still semantic-on
  via the `coreml` feature, and the bundled model still ships.
- `install.sh` is unchanged — it consumes the Release tarball.
- The Homebrew tap (external repo) is unchanged in this PR; if its
  formula passes through `cargo install` it should be updated to
  `--features semantic` separately. Tracked as a follow-up note in
  the PR description, not in this ADR.
- The CI matrix already has a `--no-default-features` lane (PR #125);
  it stays as the canonical "semantic-off" coverage.

## Cross-reference

- ADR-0011 — control-plane sprawl resolution; this is the first
  follow-up from §Neutral / Deferred.
- ADR-0001 — runtime boundaries; the semantic backend is correctly
  isolated behind `codelens-engine/semantic`, so the flip is local to
  one Cargo.toml line plus banner/README.
- README — Quick Install section reorganisation lands in the same PR
  as this ADR.
