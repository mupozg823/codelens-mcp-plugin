# Phase 1 G7 — Single-File Mutation Substrate Migration

**Status**: approved
**Date**: 2026-04-25
**Branch**: `feat/phase1-g7-fullfile-substrate` (stacked on `feat/phase1-g4-workspace-edit-transaction` → main after PR #83)
**Predecessor**: Phase 1 G4 (PR #83) — `WorkspaceEditTransaction` LSP substrate
**Successor**: Phase 1 G7b — `move_symbol` 2-file atomic transaction (separate PR)

## Background

Senior review (B+ search / C+ refactor platform) flagged that the Phase 0 envelope advertising on 11 mutation primitives is a _promise_ — agents are told `authority="syntax"` / `can_apply=true` / `edit_authority.kind="raw_fs"`, but the engine still performs raw `std::fs::write` with no hash evidence, no rollback report, no TOCTOU verification. Phase 1 G4 built the substrate (`WorkspaceEditTransaction::apply_with_evidence`) for LSP `WorkspaceEdit` flows; G7 reuses that substrate to make the Phase 0 envelope honest for the single-file mutation primitives.

## 1. Scope

### In scope (this PR)

9 callsites of raw `fs::write` in engine production code:

- `crates/codelens-engine/src/file_ops/writer.rs` — 8 functions:
  1. `create_text_file`
  2. `delete_lines`
  3. `insert_at_line`
  4. `replace_lines`
  5. `replace_content`
  6. `replace_symbol_body`
  7. `insert_before_symbol`
  8. `insert_after_symbol`
- `crates/codelens-engine/src/auto_import.rs::add_import` — 1 function

Each callsite is the final `fs::write(&resolved, &result)` line that commits the transformed in-memory content to disk.

### Out of scope (deferred)

- `crates/codelens-engine/src/move_symbol.rs:172,194` — 2-file atomic mutation (source rewrite + target write). Different transaction semantics (cross-file rollback). Tracked as G7b in a follow-up PR.
- `crates/codelens-engine/src/rename.rs:455` — already in the LSP rename path that G4 substrate covers via `WorkspaceEditTransaction`.
- Test setup `fs::write` calls (mock fixtures, integration test scaffolding).
- `WorkspaceEditTransaction` domain object itself (G4 substrate is unchanged in shape).

## 2. Components

### 2.1 Engine: `edit_transaction` substrate extension

Add **one** new free function to `crates/codelens-engine/src/edit_transaction.rs`:

```rust
pub fn apply_full_write_with_evidence(
    project: &ProjectRoot,
    relative_path: &str,
    new_content: &str,
) -> Result<ApplyEvidence, ApplyError>
```

Behaviour:

- Phase 1 — capture: read existing file, compute sha256 + length, store raw bytes as in-memory backup, populate `file_hashes_before`. If file does not exist (e.g., `create_text_file` against new path), `file_hashes_before` contains no entry for that path and backup is empty.
- Phase 2 — verify: re-read same file, recompute sha256, compare with capture. Mismatch → `Err(ApplyError::PreApplyHashMismatch{path, expected, actual})`. Skip if Phase 1 found no existing file.
- Phase 3 — apply: `fs::write(resolved, new_content)`. On `Err`, restore backup (if any), record `RollbackEntry{file_path, restored, reason}`, then return `Err(ApplyError::ApplyFailed{source, evidence})` with `evidence.status = RolledBack`.
- Phase 4 — post-hash: read written file, compute sha256, populate `file_hashes_after`. Return `Ok(ApplyEvidence{status: Applied, file_hashes_before, file_hashes_after, rollback_report: [], modified_files: 1, edit_count: 1})`.

**No new types**. Reuses G4's `ApplyEvidence`, `ApplyStatus`, `RollbackEntry`, `FileHash`, `ApplyError` verbatim.

If G4's `WorkspaceEditTransaction::capture_pre_apply` / `verify_pre_apply` helpers can be extracted into free functions (`capture_pre_apply_for_paths(project, &[path])`, `verify_pre_apply_for_paths(...)`) without changing G4 behaviour, prefer that to keep both substrates DRY. If extraction is invasive, duplicate the small single-file logic in `apply_full_write_with_evidence` rather than perturbing G4.

### 2.2 Engine: 9 mutation primitive signatures

All 9 functions change return type to expose `ApplyEvidence`:

| Function               | Before                    | After                                    |
| ---------------------- | ------------------------- | ---------------------------------------- |
| `create_text_file`     | `Result<()>`              | `Result<ApplyEvidence>`                  |
| `delete_lines`         | `Result<String>`          | `Result<(String, ApplyEvidence)>`        |
| `insert_at_line`       | `Result<String>`          | `Result<(String, ApplyEvidence)>`        |
| `replace_lines`        | `Result<String>`          | `Result<(String, ApplyEvidence)>`        |
| `replace_content`      | `Result<(String, usize)>` | `Result<(String, usize, ApplyEvidence)>` |
| `replace_symbol_body`  | `Result<String>`          | `Result<(String, ApplyEvidence)>`        |
| `insert_before_symbol` | `Result<String>`          | `Result<(String, ApplyEvidence)>`        |
| `insert_after_symbol`  | `Result<String>`          | `Result<(String, ApplyEvidence)>`        |
| `add_import`           | `Result<String>`          | `Result<(String, ApplyEvidence)>`        |

Implementation pattern: keep the existing in-memory transform unchanged; replace only the final `fs::write(&resolved, &result)?` line with `let evidence = apply_full_write_with_evidence(project, relative_path, &result)?;` then return the tuple.

### 2.3 MCP: 9 tool handler updates

`crates/codelens-mcp/src/tools/mutation.rs` — 9 handlers (`create_text_file_tool`, `delete_lines_tool`, `insert_at_line_tool`, `replace_lines_tool`, `replace_content_tool`, `replace_symbol_body_tool`, `insert_before_symbol_tool`, `insert_after_symbol_tool`, `add_import_tool`):

Each handler unpacks `(content, evidence)` and merges 6 evidence fields into the existing envelope:

```json
{
  "content": "...",
  "authority": "syntax",
  "can_preview": true,
  "can_apply": true,
  "edit_authority": { "kind": "raw_fs", "operation": "<op>", "validator": null },
  "file_hashes_before": { "<path>": { "sha256": "...", "length": N } },
  "file_hashes_after":  { "<path>": { "sha256": "...", "length": M } },
  "apply_status": "applied",
  "rollback_report": [],
  "modified_files": 1,
  "edit_count": 1
}
```

The envelope merge mirrors G4 `safe_delete_apply` — same 6 evidence keys, same naming conventions.

## 3. Data Flow

### 3.1 Happy path (e.g., `replace_lines`)

```
mcp::replace_lines_tool
  → engine::replace_lines(project, path, start, end, new_content)
       → fs::read existing                        # transform input
       → in-memory line splice
       → apply_full_write_with_evidence(project, path, &result)
              → Phase 1 capture (read + sha256 + backup)
              → Phase 2 verify (re-read + compare)
              → Phase 3 fs::write
              → Phase 4 post-hash
              → Ok(ApplyEvidence{status: Applied, ...})
       → Ok((result, evidence))
  → handler merges evidence into envelope JSON
  → success_meta(BackendKind::Filesystem, 0.7)
```

### 3.2 Pre-existing primitive read

writer.rs functions already read the file once for in-memory transform. The substrate then reads it twice more (capture + verify) — three reads per mutation total. This cost is accepted (Phase 0 confidence 0.7 mutations are not perf-critical). An optimised API that accepts pre-read content is out of scope for G7; if needed later, add a variant `apply_full_write_with_known_pre_state(project, path, pre_content, new_content)` that skips Phase 1 read.

### 3.3 No-op (new_content == existing)

substrate still calls `fs::write` (mtime touched). `file_hashes_before == file_hashes_after`. `status = Applied`. Mutation primitives do not pre-detect no-op; the substrate trusts the caller's transform. This matches G4 behaviour.

### 3.4 Rollback path

```
[Phase 3 fs::write → Err]
  → restore backup
       └─ success → RollbackEntry{path, restored: true, reason: None}
       └─ failure → RollbackEntry{path, restored: false, reason: Some(io_err.to_string())}
  → Phase 4 post-hash on actual disk state
       file_hashes_after reflects truth (== before if restore succeeded; new garbage if restore failed)
  → Err(ApplyError::ApplyFailed { source, evidence{ status: RolledBack, rollback_report: [entry], file_hashes_before, file_hashes_after } })

[mcp handler]
  match err {
      ApplyError::ApplyFailed { evidence, source } => {
          // E5 Hybrid: Ok response, apply_status field signals fail-closed
          Ok(merge_envelope(json!({
              "apply_status": "rolled_back",
              "error_message": source.to_string(),
              "rollback_report": evidence.rollback_report,
              "file_hashes_before": evidence.file_hashes_before,
              "file_hashes_after": evidence.file_hashes_after,
          }), op_name), success_meta(...))
      }
      // PreReadFailed / PreApplyHashMismatch → Err
      other => Err(CodeLensError::Validation(other.to_string())),
  }
```

## 4. Error Handling

7-row matrix (single-file, so multi-file E-rows from G4 are dropped):

| #   | Phase                                            | Cause                                                                                                      | substrate return                                                                                          | mcp response                                                                                                           |
| --- | ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| E1  | Phase 1 capture                                  | path resolve fails / file unreadable (and not a `create_text_file` against a new path)                     | `Err(ApplyError::PreReadFailed{path, source})`                                                            | `Err(CodeLensError::Validation)`                                                                                       |
| E2  | Phase 2 verify                                   | file changed between capture and verify (sha256 drift)                                                     | `Err(ApplyError::PreApplyHashMismatch{path, expected, actual})`                                           | `Err(CodeLensError::Validation)` — caller must retry                                                                   |
| E3  | Phase 3 apply OK                                 | —                                                                                                          | `Ok(ApplyEvidence{status: Applied, ...})`                                                                 | Ok response, `apply_status="applied"`                                                                                  |
| E4  | Phase 3 apply Err, rollback OK                   | fs::write fails (IO/perm), backup restore succeeds                                                         | `Err(ApplyError::ApplyFailed{source, evidence{status: RolledBack, rollback_report: [{restored: true}]}})` | **Ok** response, `apply_status="rolled_back"`, `error_message`, `rollback_report` (Hybrid E4)                          |
| E5  | Phase 3 apply Err, rollback Err                  | fs::write fails AND backup restore fails                                                                   | same as E4 but `rollback_report[].restored=false`                                                         | **Ok** response, `apply_status="rolled_back"`, `rollback_report` shows `restored=false` (caller detects partial state) |
| E6  | Pre-substrate (primitive's own read/transform)   | first read fails / line out of range / regex invalid / target symbol not found / overwrite=false collision | `Err(anyhow::Error)` from primitive (substrate not entered)                                               | `Err(CodeLensError)` — no evidence                                                                                     |
| E7  | Pre-substrate (transform produces invalid UTF-8) | byte-splice in `replace_symbol_body` etc. yields non-UTF-8 bytes                                           | `Err(anyhow::Error)` from primitive                                                                       | `Err(CodeLensError)`                                                                                                   |

### Hybrid policy scope

Substrate-level Ok-conversion only applies to `ApplyError::ApplyFailed` (E4/E5). `PreReadFailed` and `PreApplyHashMismatch` are user-input / concurrent-state problems and surface as `Err`. Pre-substrate errors (E6/E7) are unchanged from current behaviour.

## 5. Testing

### 5.1 Engine substrate unit tests (6 cases)

In `crates/codelens-engine/src/edit_transaction.rs` `#[cfg(test)] mod tests`:

| #   | Test                                                          | Asserts                                                                                                                                                         |
| --- | ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T1  | `apply_full_write_happy_returns_evidence`                     | `status=Applied`, `file_hashes_before/after` populated with correct sha256, `rollback_report=[]`, `modified_files=1`, `edit_count=1`                            |
| T2  | `apply_full_write_pre_read_failed_on_unreadable_path`         | nonexistent parent dir → `Err(PreReadFailed)` (note: `create_text_file` against new path is OK; this targets a true unreadable case)                            |
| T3  | `apply_full_write_toctou_mismatch_via_inject_corruption`      | use `inject_pre_apply_corruption` test helper between capture and verify → `Err(PreApplyHashMismatch)`                                                          |
| T4  | `apply_full_write_rollback_on_write_failure` (`#[cfg(unix)]`) | chmod parent dir 0555 to force `fs::write` Err → `Err(ApplyFailed{evidence{status: RolledBack, rollback_report[0].restored=true}})`, post-hash matches pre-hash |
| T5  | `apply_full_write_hash_determinism`                           | apply same content twice (separate transactions) → identical sha256 in evidence both runs                                                                       |
| T6  | `apply_full_write_no_op_same_content`                         | `new_content == existing` → `status=Applied`, `before == after` hashes, single `Applied` entry                                                                  |

`inject_pre_apply_corruption` is the existing G4 `#[cfg(test)]` helper; reuse without modification.

### 5.2 Engine primitive unit test updates

Each of the 9 primitives has at least one happy-path unit test today. Update minimally:

- Destructure new return shape `(content, evidence)` (or `(content, count, evidence)` for `replace_content`).
- Add: `assert_eq!(evidence.status, ApplyStatus::Applied)`, `assert_eq!(evidence.modified_files, 1)`, `assert_eq!(evidence.edit_count, 1)`, evidence has 1 entry in `file_hashes_after`.

Add **1** new test as a caller-site evidence integration check (not a substrate redundancy):

- `replace_lines_evidence_post_apply_hash_matches_disk` — call `replace_lines`, then read disk and recompute sha256, assert it equals `evidence.file_hashes_after[path].sha256`. Confirms the caller is forwarding evidence correctly, not just substrate-internal correctness.

### 5.3 MCP integration tests (5 cases)

In `crates/codelens-mcp/src/integration_tests/` (or a sibling module to `semantic_refactor.rs`):

| #   | Test                                                             | Asserts                                                                                                                                                   |
| --- | ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1  | `replace_lines_tool_response_includes_evidence`                  | response JSON has all 6 evidence keys, `apply_status="applied"`, `modified_files=1`, `edit_count=1`, `rollback_report=[]`                                 |
| M2  | `create_text_file_tool_response_includes_evidence`               | new file: `file_hashes_before` may have no entry for path or length=0 (whichever T1 establishes), `file_hashes_after` populated, `apply_status="applied"` |
| M3  | `add_import_tool_response_includes_evidence`                     | auto_import path produces same 6 evidence keys                                                                                                            |
| M4  | `replace_lines_tool_e2_toctou_returns_validation_err`            | injected mid-write corruption → `CodeLensError::Validation` (Err result), evidence not surfaced to user                                                   |
| M5  | `replace_lines_tool_e4_rollback_response_shape` (`#[cfg(unix)]`) | chmod-driven apply fail → Ok response with `apply_status="rolled_back"`, `error_message` populated, `rollback_report[0].restored=true` — Hybrid contract  |

### 5.4 Surface manifest / matrix regression

Phase 0 G6 contract A and B must remain green. G7 changes the _content_ of mutation primitive responses (adds 6 evidence fields) but does not change `authority` / `can_apply` / `edit_authority` / confidence values. Verify:

```bash
python3 scripts/surface-manifest.py --check-operation-matrix < <(cargo run -p codelens-mcp --quiet -- --print-operation-matrix)
python3 scripts/test/test-surface-manifest-contracts.py
```

Both must PASS with no diff in matrix output for any of the 9 primitives.

### 5.5 Test gates summary

```bash
cargo test -p codelens-engine                        # substrate + primitive tests
cargo test -p codelens-mcp                           # default (M1-M5)
cargo test -p codelens-mcp --features http           # http feature regression
cargo clippy -- -W clippy::all                       # 0 NEW warnings
python3 scripts/surface-manifest.py --check-operation-matrix
python3 scripts/test/test-surface-manifest-contracts.py
```

## 6. Acceptance Criteria

| #    | AC                                                                                                                                                                                                                                     | Evaluation                                                                                                                 |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| AC-1 | `apply_full_write_with_evidence` exists in `engine::edit_transaction` with signature `(project: &ProjectRoot, relative_path: &str, new_content: &str) -> Result<ApplyEvidence, ApplyError>`                                            | grep / `find_symbol`                                                                                                       |
| AC-2 | Substrate unit tests T1-T6 (6 cases) all PASS, 0 fail                                                                                                                                                                                  | `cargo test -p codelens-engine apply_full_write`                                                                           |
| AC-3 | All 9 primitives expose `ApplyEvidence` in return type. Raw `fs::write` calls in `crates/codelens-engine/src/file_ops/writer.rs` and `crates/codelens-engine/src/auto_import.rs` (production code only) = 0                            | `rg 'fs::write\b' crates/codelens-engine/src/file_ops/writer.rs crates/codelens-engine/src/auto_import.rs` returns 0 lines |
| AC-4 | MCP 9 tool handlers' Ok responses include the 6 evidence keys (`file_hashes_before`/`file_hashes_after`/`apply_status`/`rollback_report`/`modified_files`/`edit_count`) with correct values                                            | M1-M3 PASS                                                                                                                 |
| AC-5 | E2 TOCTOU yields `Err(CodeLensError::Validation)`; evidence not surfaced                                                                                                                                                               | M4 PASS                                                                                                                    |
| AC-6 | E4 rollback yields Ok response with `apply_status="rolled_back"`, `error_message` set, `rollback_report[].restored=true`. Hybrid contract honoured                                                                                     | M5 PASS (`#[cfg(unix)]` gated)                                                                                             |
| AC-7 | Phase 0 surface-manifest contract A+B green; 9 primitives' envelope authority/can_apply/edit_authority/confidence values unchanged                                                                                                     | `--check-operation-matrix` + contract fixtures PASS, snapshot diff = 0                                                     |
| AC-8 | `move_symbol.rs` callsites unchanged. G4 `WorkspaceEditTransaction` domain object (struct fields, `apply_with_evidence` signature) unchanged. New free function `apply_full_write_with_evidence` may be added to `edit_transaction.rs` | `git diff main..HEAD` review for these files                                                                               |
| AC-9 | All test gates green: engine + mcp default + mcp http, 0 fail. 0 NEW clippy warnings                                                                                                                                                   | full gate run from §5.5                                                                                                    |

Scoring follows the Phase 0 / G4 pattern: orchestrator collects evidence and synthesises directly, or dispatches an `evaluator(opus)` once at PR-merge time.

## 7. Next Steps

1. After this spec is approved: invoke `superpowers:writing-plans` skill for the implementation plan (target: `docs/superpowers/plans/2026-04-25-codelens-phase1-g7-fullfile-substrate.md`).
2. Plan decomposes into TDD tasks (substrate-first → primitive migration → MCP envelope merge → final verification).
3. Branch `feat/phase1-g7-fullfile-substrate` is stacked on `feat/phase1-g4-workspace-edit-transaction` until PR #83 merges, then rebases onto `main`.
4. Follow-up PRs (out of this G7 scope):
   - **G7b**: `move_symbol.rs` 2-file atomic substrate (separate spec — different transaction semantics).
   - **G5**: runtime capability probing (LSP `initialize.capabilities` validation against static operation_matrix).
