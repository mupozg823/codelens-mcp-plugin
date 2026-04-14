# Main Worktree Split Plan

## Why This Exists

`main` is currently not in a push-safe state.

The worktree contains 82 changed paths spanning runtime, transport, state,
tests, release, benchmarks, datasets, scripts, docs, engine, and TUI.

That is too wide to ship as one commit stream.

The immediate problem is not "missing features".
The immediate problem is change governance.

## Current Shape

Observed dirty areas at the time of this audit:

- runtime: server = 4 files
- runtime: dispatch = 4 files
- runtime: state = 4 files
- runtime: tool definitions = 4 files
- runtime: tools/contracts = 15 files
- integration tests = 6 files
- engine = 3 files
- tui = 2 files
- docs = 13 files
- CI/release = 5 files
- benchmarks = 11 files
- scripts = 25 files
- datasets = 1 tracked path plus untracked assets
- other = 8 paths

Total:

- 82 tracked files in `git diff --shortstat`
- 105 dirty status entries in `git status --short`
- 7,559 insertions
- 16,172 deletions

This is not a single feature branch.

## Architecture Assessment

The CodeLens MCP server already has product-grade building blocks:

- startup and transport bootstrap in `crates/codelens-mcp/src/main.rs`
- runtime core in `crates/codelens-mcp/src/state.rs`
- request normalization and policy gate in `crates/codelens-mcp/src/dispatch/`
- HTTP/stdin transport layer in `crates/codelens-mcp/src/server/`
- tool surface and policy exposure in `crates/codelens-mcp/src/tool_defs/`

The direction is broadly correct:

- transport is separate from dispatch
- session/runtime concerns exist as explicit modules
- client profile and orchestration contract concepts already exist
- verification and reporting are part of the product, not bolted on

But productization is still blocked by workflow and governance issues:

- too many unrelated changes land together
- release and research changes are mixed with runtime changes
- backward compatibility is not isolated as a first-class stream
- release artifact validation is mixed into feature development

## Product Buckets

The current dirty worktree should be split into these buckets.

### Bucket 1: Runtime Contracts

Scope:

- `crates/codelens-mcp/src/dispatch/`
- `crates/codelens-mcp/src/server/`
- `crates/codelens-mcp/src/state.rs`
- `crates/codelens-mcp/src/state/`
- `crates/codelens-mcp/src/tool_defs/`
- `crates/codelens-mcp/src/tools/`
- `crates/codelens-mcp/src/protocol.rs`
- `crates/codelens-mcp/src/error.rs`
- `crates/codelens-mcp/src/client_profile.rs`
- `crates/codelens-mcp/src/harness_host.rs`
- `crates/codelens-mcp/src/main.rs`
- `crates/codelens-mcp/Cargo.toml`

Intent:

- transport/runtime contract behavior
- dispatch and response shape
- session/runtime state behavior
- orchestration contract and profile handling

Ship rule:

- no benchmarks
- no dataset changes
- no release automation changes

### Bucket 2: Runtime Verification

Scope:

- `crates/codelens-mcp/src/integration_tests/`
- `crates/codelens-mcp/src/server/http_tests.rs`
- any targeted runtime test helpers needed by Bucket 1

Intent:

- transport parity
- protocol compatibility
- readonly/mutation behavior
- HTTP contract verification

Ship rule:

- this bucket follows Bucket 1
- it should not also carry benchmark or release-system work

### Bucket 3: Engine / Retrieval / Quality

Scope:

- `crates/codelens-engine/`
- benchmark runtime code
- retrieval evaluation scripts
- dataset manifesting and linting
- dataset path helpers

Intent:

- embedding/retrieval quality
- benchmark harness accuracy
- training/eval pipeline quality gates

Ship rule:

- must not be mixed with transport/runtime contract changes

### Bucket 4: Release / Packaging / Operations

Scope:

- `.github/workflows/`
- `scripts/generate-release-manifest.py`
- `scripts/quality-gate.sh`
- `scripts/verify-release-artifacts.sh`
- `scripts/check-release-docs.py`
- `scripts/publish-crates-workspace.sh`
- `scripts/verify-github-attestations.sh`
- `scripts/sync-local-bin.sh`
- `Formula/codelens-mcp.rb`
- `CHANGELOG.md`
- release docs

Intent:

- supply chain
- release safety
- packaging
- artifact verification
- operational scripts

Ship rule:

- must be reviewed as release engineering, not mixed with runtime behavior

### Bucket 5: Documentation

Scope:

- `README.md`
- `crates/codelens-mcp/README.md`
- `docs/*.md`

Intent:

- architecture explanation
- platform setup
- release verification
- benchmark docs
- support policy

Ship rule:

- docs may reference other buckets, but should not hide runtime changes

### Bucket 6: TUI

Scope:

- `crates/codelens-tui/`

Intent:

- TUI feature or UX changes

Ship rule:

- keep isolated unless the runtime contract forces a TUI update

## Recommended Branch Strategy

Do not continue on dirty `main`.

Create branches in this order:

1. `codex/runtime-contract-hardening`
2. `codex/runtime-verification-matrix`
3. `codex/retrieval-quality-pipeline`
4. `codex/release-ops-hardening`
5. `codex/docs-productization`

If TUI changes are real and not incidental:

6. `codex/tui-alignment`

## Immediate Extraction Order

### First Extraction

Take only runtime contract files first.

Why:

- this is the product core
- this is where enterprise behavior is defined
- this is what clients depend on

Required review focus:

- dispatch safety
- response compatibility
- transport fallback behavior
- session/runtime correctness
- contract shape stability

### Second Extraction

Take only runtime verification files.

Why:

- feature and verification need separate review
- lets failures point to contract regressions instead of benchmark churn

### Third Extraction

Take release/ops hardening.

Why:

- release pipeline must be independently auditable
- mixing supply-chain work with runtime changes is operationally unsafe

### Fourth Extraction

Take benchmarks/datasets/retrieval changes.

Why:

- this is high-churn research/quality work
- it should not block or contaminate runtime releases

## Enterprise Product Gaps

Even after the split, product work is still required.

### 1. Contract Versioning

Needed:

- explicit protocol version for orchestration contract
- capability flags for optional behavior
- consumer fallback rules

### 2. Compatibility Matrix

Needed:

- stdio vs HTTP
- trusted vs untrusted mutation paths
- deferred loading enabled vs disabled
- profile/surface combinations
- upgrade compatibility between adjacent versions

### 3. Operational Observability

Needed:

- request latency by transport
- dispatch failure classes
- preflight reject rate
- deferred load miss rate
- fallback path counters
- startup/session markers

### 4. Release Discipline

Needed:

- artifact parity
- binary drift detection in CI
- changelog gating
- package verification
- support policy and rollback policy

### 5. Ownership Boundaries

CodeLens should remain:

- contract/evidence/optimization layer
- not a second orchestrator
- not a transport-specific policy snowball

## What To Avoid

- do not commit dirty `main` as one branch
- do not mix release engineering with retrieval pipeline changes
- do not mix runtime contract changes with dataset churn
- do not push from `main` while these buckets are still entangled

## Next Operator Actions

1. Freeze dirty `main` and do not add new unrelated changes.
2. Extract Bucket 1 into a clean runtime branch.
3. Validate only Bucket 1.
4. Extract Bucket 2 on top or separately, depending on conflict profile.
5. Extract Bucket 4 before any public release.
6. Extract Bucket 3 separately and keep it out of release-critical branches.

## Commit Guidance

For the runtime branch, use commits shaped like:

1. `feat(mcp): harden runtime transport and dispatch contracts`
2. `test(mcp): expand protocol and transport verification`

For the release branch:

1. `chore(release): harden packaging and verification pipeline`

For the retrieval branch:

1. `feat(engine): refine retrieval and benchmark pipeline`

This repo needs product governance before it needs more unbounded development.
