---
id: safe-refactor
title: Safe Refactor Readiness
risk: mutate-adjacent
implicit: false
description: >
  Only on explicit request. Produce a verified preflight for a rename/move/
  restructure: full reference set, readiness verdict, and a snapshot-bound
  evidence capsule that the host's native edit path executes against. Performs
  no mutation itself.
tools:
  - prepare_harness_session
  - plan_safe_refactor
  - verify_change_readiness
  - diagnose
  - graph
---

# Safe Refactor Preflight

1. **Bind + freshness gate.** `prepare_harness_session`; refuse to proceed on a
   stale index — refresh first. Record the snapshot/generation for the capsule.
2. **Plan.** `plan_safe_refactor` for the target symbol(s); cross-check the
   reference set with `graph` (refs/impact) — disagreement between the two is a
   blocker, not a footnote.
3. **Readiness.** `verify_change_readiness` on every target file. `blocked` ⇒ stop
   and report blockers; `caution` ⇒ proceed only with a post-edit `diagnose` step
   in the plan.
4. **Hand off, don't edit.** Emit the preflight capsule: snapshot id, target set,
   complete reference list (file:line), readiness verdicts, and the post-edit
   verification commands. The host performs edits with its native tools; re-run
   `diagnose` afterward and compare against the capsule.
