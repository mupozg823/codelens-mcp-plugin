# Host Capability Adapter Spec — Skills, Plugin Manifests, Trigger Ladder

Companion to ADR-0015/0016 and `runtime-convergence-execution-plan.md` (E5.1/E6.2).
Defines how one
common workflow source compiles into host-native skill/plugin packages for Claude Code
and Codex, replacing the current non-standard `trigger:`/`tools:` frontmatter
(`skills/*/SKILL.md` at HEAD reference alias-tier tools and a `trigger` key neither host
honors).

## 1. Common workflow source (single source of truth)

One file per skill under `skills-src/` (staged drafts: `docs/design/workflow-skills/`):

```yaml
id: review                     # stable id
title: CodeLens Review
risk: read                     # read | analyze | mutate  → gates implicit invocation
implicit: true                 # false ⇒ explicit invocation only on every host
description: >                 # the ONLY implicit-matching surface on both hosts
  …clear scope and boundaries…
tools:                         # CORE-20 names only (CI-audited, ADR-0016 §3)
  - prepare_harness_session
  - get_changed_files
  - review
steps: |                       # host-neutral workflow body (markdown)
  …
```

Compilation is deterministic (`scripts/gen-host-adapters.py`, joins the ToolCatalog):
referencing a non-CORE-20 tool fails the build — this is the E2.5/E5.1 drift gate.

## 2. Claude Code package

```
.claude-plugin/plugin.json          # name, version, skills index
skills/<id>/SKILL.md                # generated
```

Generated `SKILL.md` frontmatter:

```yaml
---
name: codelens-<id>
description: "<description>. Use when …; not for …"   # implicit matching lives here
allowed-tools: mcp__codelens__<tool>, …               # per-turn PREAPPROVAL, not a cap
---
```

Rules: no `trigger:` key (not part of the contract); `allowed-tools` lists exactly the
source `tools` (preapproval semantics — the model may still use others subject to normal
permissions); skills with `implicit: false` (safe-refactor) get a description that
starts with "Only on explicit request" and are additionally invoked as
`/codelens:<id>`. Mutation-risk skills never appear in `suggested`/implicit paths.

## 3. Codex package

```
.codex-plugin/plugin.json           # namespace manifest (name, version, author)
components/<id>/SKILL.md            # name + description first; body lazy-loaded
components/<id>/agents/openai.yaml  # policy + dependencies
```

Codex reads name/description/path first and opens `SKILL.md` on selection (progressive
disclosure), so `description` carries all implicit-matching signal. Generated
`agents/openai.yaml`:

```yaml
policy:
  allow_implicit_invocation: <implicit>   # false for safe-refactor
dependencies:
  tools:
    - mcp: codelens/<tool>
```

Because Codex has no native tool search, the Codex adapter pins the static CORE-20
surface (no CodeLens-side deferred loading; ADR-0016 §5) and relies on skills for
anything deeper.

## 4. The three workflow skills

| id | risk | implicit | tools (CORE-20 subset) |
|---|---|---|---|
| `explore-impact` | read | yes | prepare_harness_session, overview, search, graph, get_ranked_context, find_symbol |
| `review` | analyze | yes | prepare_harness_session, get_changed_files, review, diagnose, graph, find_referencing_symbols |
| `safe-refactor` | mutate-adjacent | **no** | prepare_harness_session, plan_safe_refactor, verify_change_readiness, diagnose, graph |

`safe-refactor` performs no mutation itself in Q3 (mutation tools are out of the public
surface until the Q2'27 transaction ADR); it produces the preflight evidence the host's
native edit path consumes.

## 5. Trigger ladder (five stages, concrete)

| Stage | Mechanism | Runtime-convergence implementation |
|---|---|---|
| 1 | MCP `initialize` | ≤ 15-line server card: role ("code evidence & analysis data plane"), how to bind (`prepare_harness_session(project=…)`), nothing else — no routing policy |
| 2 | tool names/schemas | CORE-20 with complete input/output schemas + read-only/idempotent/destructive annotations (ADR-0016 §6); hosts with tool search discover the alias tier themselves |
| 3 | skill `description` | implicit invocation for `explore-impact`, `review` only |
| 4 | explicit call | `/codelens:review`, `/codelens:safe-refactor` (Claude); `$safe-refactor` prompt or skill mention (Codex) |
| 5 | server transaction gate | deterministic checks regardless of what the model asked: principal (transport-derived, ADR-0018), snapshot/diff binding, target-path allowlist, `verify_change_readiness` state |

Stages 1–4 are advisory (host/model may ignore); stage 5 is the only enforcement layer.

## 6. Retirement of current assets

- `skills/{analyze,code-review,onboard}` → regenerated from common source (`review`,
  `explore-impact`); `onboard` folds into `explore-impact`.
- `agents/codelens-explorer.md` → regenerated: model-unpinned, read-only, CORE-20 verbs
  only (E6.2).
- `hooks/hooks.json` → out of default install (E6.1); optional extra package.
