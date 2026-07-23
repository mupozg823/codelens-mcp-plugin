---
id: review
title: Repository Review
risk: analyze
implicit: true
description: >
  Review a change set (working diff, branch, or PR) for impact, correctness
  signals, and architecture fit using indexed evidence: changed files, reference
  and call-path verification, diagnostics, and boundary/dead-code checks. Use
  when a review names multiple files or asks "what does this change affect".
  Not for style-only nits on a single file.
tools:
  - prepare_harness_session
  - get_changed_files
  - review
  - diagnose
  - graph
  - find_referencing_symbols
---

# CodeLens Review

1. **Bind + scope.** `prepare_harness_session`, then `get_changed_files` to fix the
   review scope; pin the snapshot so later calls are deterministic.
2. **Impact pass.** `graph` (impact/diff-refs) on the changed set — enumerate
   affected callers before reading bodies; verify externally visible symbols with
   `find_referencing_symbols`.
3. **Quality pass.** `review` (changes; architecture/boundary/dead/dupes modes as
   relevant) and `diagnose` (file/unresolved) on touched files only.
4. **Verdict format.** Findings ranked by severity, each with file:line, the
   failing scenario, and the evidence call that produced it. State what was NOT
   checked (untouched modes) — no silent coverage claims.
