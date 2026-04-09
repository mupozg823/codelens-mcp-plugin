# Evaluation Contract

## Local Stop-Hook Gate

- `cargo check`
- `cargo test -p codelens-core`
- `cargo test -p codelens-mcp`

## Local Extended Gate

- `cargo test -p codelens-mcp --features http`
- `cargo clippy -- -W clippy::all`

## CI Parity Additions

- `cargo build --release --no-default-features`
- `cargo build --release`
- `cargo build --release --features http`

## Build Workflow Gate

- `cargo test -p codelens-core`
- `cargo test -p codelens-mcp -- --skip returns_lsp_diagnostics --skip returns_workspace_symbols --skip returns_rename_plan`
- `cargo build --release`

## CI-Only Benchmark Gates

- `python3 benchmarks/token-efficiency.py ... --check`
- `python3 benchmarks/embedding-quality.py ...`
- `python3 benchmarks/external-retrieval.py ...`
- `python3 benchmarks/role-retrieval.py ...`
- `python3 scripts/finetune/contamination_audit.py ...`
- `python3 scripts/finetune/promotion_gate.py ...` for any candidate embedding model

## Benchmark Interpretation

- Compare like-for-like build profiles only.
- Separate warm and cold measurements.
- Record p50 and p95 where applicable.
- Do not attribute release-vs-debug differences to refactor wins.
- Internal training validation is not a promotion gate for embedding models.
- Candidate embedding models must be compared against the currently deployed runtime model on fresh product retrieval benchmarks.
- Promotion is fail-closed:
  - real-session harness evidence is mandatory
  - exact-label external retrieval evidence is mandatory
  - role/adversarial retrieval evidence is mandatory
  - contamination audit must pass
- Promotion harness evidence should be produced through `benchmarks/harness/real-session-evidence.py`, not by trusting stale synthetic-only harness summaries.
- If real-session evidence is insufficient, promotion must emit a coverage-gap queue and scenario pack for the missing captures.
- Synthetic-only harness summaries are useful for smoke checks, but they are not sufficient evidence for paper claims or deployment decisions.

## Codex Harness Operating Defaults

- Harness runs should assume non-interactive execution by default.
- Multi-step tasks should keep explicit plan or checklist state rather than relying on implicit progress.
- When verification is available, the harness should encourage a build -> verify -> fix loop before completion.
- Completion output should include the requested work, evidence, verification actually run, and any remaining gaps.
- Point lookups should stay on the native path; multi-file reviewer/planning/refactor tasks should only escalate to CodeLens after the first local boundary check.
- Prompt or bootstrap changes are only acceptable if they preserve the fail-closed evaluation rules above.

## Flake Policy

- A single green run is not enough for historically flaky HTTP tests.
- Re-run the HTTP suite multiple times before calling stability resolved.
- If a failure appears only under parallel load, classify it as harness stability risk until disproven.

## Notes

- Local stop hooks should approximate CI without making every save or stop path unreasonably slow.
- Benchmarks remain CI-oriented unless a task explicitly targets benchmark work.
- Formatting is handled separately by edit-time tooling; stop hooks should not fail on checks CI does not enforce.
