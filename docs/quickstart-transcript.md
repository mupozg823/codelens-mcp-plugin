# Clean Quickstart Transcript

This transcript is a repeatable local source-build smoke for the user-facing
flow:

```text
install -> doctor/status -> index -> coverage -> retrieve
```

Replay the source-build prefix path with:

```bash
python3 scripts/smoke-clean-quickstart.py \
  --binary target/debug/codelens-mcp \
  --model-root crates/codelens-engine/models
```

Replay a release archive with:

```bash
python3 scripts/smoke-clean-quickstart.py \
  --archive codelens-mcp-linux-x86_64.tar.gz
```

The source-build transcript proves that the current checkout can run from an
isolated prefix, isolated `HOME`, and isolated project without relying on this
repository's local Codex/Claude settings. The release workflow also runs the
same smoke directly from packaged archives on native runners.

## Captured Run

| Field | Value |
| --- | --- |
| Captured date | 2026-07-06 |
| Binary source | `target/debug/codelens-mcp` copied into `/tmp/codelens-clean-quickstart.oET2yA/prefix/bin/` |
| Model source | `crates/codelens-engine/models/codesearch` copied into `/tmp/codelens-clean-quickstart.oET2yA/prefix/models/` |
| Runtime env | `HOME=/tmp/codelens-clean-quickstart.oET2yA/home`, `CODELENS_LOG=error`; `CODELENS_MODEL_DIR` intentionally unset |
| Fixture project | `/tmp/codelens-clean-quickstart.oET2yA/project` with one Rust library file |

The local binary reported:

```text
codelens-mcp 1.13.34 (git 149e3a5, dirty true, built 2026-07-05T17:22:25Z)
```

`dirty true` is expected for this local WIP run. Tagged release archives should
report a clean build.

## Install

The clean prefix was created by copying the built binary and model sidecar into
a temp install layout:

```bash
root=$(mktemp -d /tmp/codelens-clean-quickstart.XXXXXX)
mkdir -p "$root/prefix/bin" "$root/prefix/models" "$root/home/.codex" "$root/project/src"
cp target/debug/codelens-mcp "$root/prefix/bin/codelens-mcp"
chmod +x "$root/prefix/bin/codelens-mcp"
cp -R crates/codelens-engine/models/codesearch "$root/prefix/models/"
```

The fixture project contained this source file:

```rust
/// Adds two values for the clean quickstart transcript.
pub fn add_values(left: i32, right: i32) -> i32 {
    left + right
}
```

The isolated Codex config pointed at the temp binary by stdio:

```toml
[mcp_servers.codelens]
command = "/tmp/codelens-clean-quickstart.oET2yA/prefix/bin/codelens-mcp"
args = ["/tmp/codelens-clean-quickstart.oET2yA/project"]
```

## Doctor / Status

Command:

```bash
HOME="$root/home" \
CODELENS_LOG=error \
"$root/prefix/bin/codelens-mcp" status codex --json
```

Observed status summary:

```text
host=codex
~/.codex/config.toml status=attached_customized
project AGENTS.md status=missing
strict_semantic_coverage=false
```

Interpretation: the isolated host config was detected from the temp `HOME`.
The missing project `AGENTS.md` is acceptable for this runtime smoke because
the goal is binary install, host config detection, index, and retrieval.

## Pre-Index Capability

Command:

```bash
HOME="$root/home" \
CODELENS_LOG=error \
"$root/prefix/bin/codelens-mcp" . --cmd get_capabilities --args '{}'
```

Observed capability summary before indexing:

```text
semantic_search_status=index_missing
recommended_action=run_index_embeddings
embedding_indexed=false
embedding_indexed_symbols=0
indexed_files=2
supported_files=2
```

Interpretation: the clean prefix could load the model sidecar without
`CODELENS_MODEL_DIR`, but correctly refused to claim semantic retrieval until
the project was indexed.

## Index

Command:

```bash
HOME="$root/home" \
CODELENS_LOG=error \
"$root/prefix/bin/codelens-mcp" . --cmd index_embeddings
```

Observed index summary:

```text
success=true
backend_used=semantic
indexed_symbols=3
query_cache.enabled=true
query_cache.entries=0
status=ok
```

Run indexing and coverage probes sequentially. Running a coverage probe at the
same time as the first clean index build can hit the expected SQLite writer
lock.

## Coverage

Command:

```bash
HOME="$root/home" \
CODELENS_LOG=error \
"$root/prefix/bin/codelens-mcp" . --cmd embedding_coverage_report
```

Observed coverage summary:

```text
compiled=true
model_assets.available=true
status=ready
indexed_symbols=3
indexed_files=2
readiness_percent=100
stale_files=0
model_mismatch=false
recommended_action=none
remediation.action=none
```

Interpretation: after indexing, the semantic operational report contains the
model, schema, file freshness, query cache, and remediation fields needed for a
single-turn operator decision.

## Retrieve

Command:

```bash
HOME="$root/home" \
CODELENS_LOG=error \
"$root/prefix/bin/codelens-mcp" . \
  --cmd get_ranked_context \
  --args '{"query":"function that adds two values","max_tokens":1200,"depth":1,"include_body":false}'
```

Observed retrieval summary:

```text
success=true
token_estimate=698
retrieval.semantic_enabled=true
retrieval.semantic_used_in_core=true
retrieval.sparse_used_in_core=true
retrieval.preferred_lane=hybrid_semantic_sparse
retrieval.query_type=natural_language
symbols[0].name=add_values
symbols[0].file=src/lib.rs
symbols[0].relevance_score=100
symbols[0].provenance.source=semantic_boosted
```

Interpretation: a natural-language query found the intended Rust function as
rank 1 using hybrid semantic+sparse retrieval within a bounded token budget.

## Release Replay Gap

This transcript used a source-built debug binary copied into a temp prefix.
Before claiming full public release quickstart readiness, repeat the same
sequence from every published release archive or installer output and verify
that:

1. the binary reports a clean build,
2. the release model sidecar resolves without `CODELENS_MODEL_DIR`, unless the
   installer channel explicitly owns an equivalent environment setup,
3. `status`/`doctor`, `index_embeddings`, `embedding_coverage_report`, and
   `get_ranked_context` pass with the same evidence shape.

The release workflow now runs the same smoke against packaged release archives
via `scripts/smoke-clean-quickstart.py --archive`. Keep this transcript as
human-readable evidence, and treat the workflow gates as the repeatable release
replay.
