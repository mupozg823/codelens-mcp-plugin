# Benchmark Archive

Scripts in this directory are **no longer part of the active CI or quality-gate pipeline**.
They were moved here from `benchmarks/` during Phase 1 cleanup (2026-04-14) per ADR-005.

## Contents

| File                       | Type                                                           | Original Purpose                                | Reason Archived                                                                                        |
| -------------------------- | -------------------------------------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `coverage-gap-queue.py`    | 7-line wrapper → `benchmarks/harness/coverage-gap-queue.py`    | Enqueue coverage-gap analysis jobs              | Not referenced by `.github/workflows/*.yml`, `scripts/quality-gate.sh`, or any active benchmark runner |
| `coverage-gap-runner.py`   | 7-line wrapper → `benchmarks/harness/coverage-gap-runner.py`   | Execute queued coverage-gap jobs                | Same as above                                                                                          |
| `prune-session-entries.py` | 7-line wrapper → `benchmarks/harness/prune-session-entries.py` | One-shot cleanup of old harness session entries | Ad-hoc maintenance tool, not part of regular pipeline                                                  |

## Restoration

If an archived script needs to be reinstated:

1. `git mv benchmarks/archive/<name>.py benchmarks/<name>.py`
2. Verify `_run_harness_wrapper.py` still exists in `benchmarks/` (it does as of this commit)
3. Confirm the underlying implementation still exists in `benchmarks/harness/<name>.py`

## Related

- ADR-005: Benchmark Artifact & Bundled Model Externalization
- `benchmarks/harness/` — the actual implementations (unchanged)
- `benchmarks/_run_harness_wrapper.py` — the shim these wrappers delegated to
