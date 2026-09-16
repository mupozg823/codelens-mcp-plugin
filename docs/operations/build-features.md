# Build features and the verify matrix

> Moved from `CLAUDE.md` on 2026-09-16. `.github/workflows/ci.yml` is the authoritative gate; this page
> explains the feature flags and mirrors the commands CI runs.

## Feature flag matrix (build-time)

The default `cargo install codelens-mcp` build is `default = ["scip-backend"]` (set in `crates/codelens-mcp/Cargo.toml`; SCIP itself only activates when an `index.scip` exists in the project). Most other operational use needs explicit features:

| Feature        | When required                                                    | Symptom if missing                                                                                        |
| -------------- | ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `http`         | Any HTTP transport / daemon mode                                 | `Error: HTTP transport requires the http feature` at startup, port never binds                            |
| `semantic`     | `semantic_search`, `index_embeddings`, hybrid ranking            | Tools degrade to BM25-only; status reports `FeatureDisabled`                                              |
| `scip-backend` | SCIP precise navigation in `find_symbol`, `heuristic_body_slice` | `cargo clippy --no-default-features` flags `dead_code` on `#[cfg(feature = "scip-backend")]`-only callees |
| `coreml`       | macOS CoreML execution provider for ONNX                         | Falls back to CPU silently                                                                                |
| `otel`         | OpenTelemetry export                                             | No telemetry emitted                                                                                      |

**Daemon rule:** `~/Library/LaunchAgents/dev.codelens.mcp-{readonly,mutation}.plist` invokes `target/release/codelens-mcp --transport http …`. The release binary **must** be built with `--features http` or both daemons exit immediately. `cargo build --release` alone is insufficient.

## Build & verify (CI mirror)

```bash
# Default verify (matches local pre-push)
cargo check
cargo test -p codelens-engine
cargo test -p codelens-mcp --bin codelens-mcp        # NOT --lib (no lib target)

# Feature-matrix mirroring CI (.github/workflows/ci.yml)
cargo check --workspace --features http
cargo check --workspace --features otel
cargo check --workspace --features scip-backend
cargo check --workspace --no-default-features        # "semantic-off" gate
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features scip-backend -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo nextest run --workspace                        # CI uses nextest
cargo nextest run --workspace --features http
cargo nextest run --workspace --no-default-features
cargo test --doc --workspace
cargo fmt --all -- --check                           # Prefer running this; matches CI

# Single test
cargo test -p codelens-mcp --bin codelens-mcp <test_substring>
cargo test -p codelens-engine --lib <test_substring>

# Codegen drift gates (CI runs these too)
python3 scripts/regen-tool-defs.py --check           # tools.toml ↔ build_generated.rs
python3 scripts/surface-manifest.py --check          # generated doc blocks
python3 benchmarks/lint-datasets.py --project .      # benchmark dataset hygiene

# Release build for the local launchd daemons
cargo build --release --features http,semantic
bash scripts/install-http-daemons-launchd.sh . --load
```

`scripts/quality-gate.sh` and `scripts/mcp-doctor.sh . --strict` are convenience wrappers; CI is the authoritative pre-merge gate.
