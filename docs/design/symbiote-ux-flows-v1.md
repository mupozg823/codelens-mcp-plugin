# [Archived] Symbiote MCP — UX / Agent Flow Specification v1

> Archived proposal: this rebrand flow is parked. Public product language stays CodeLens-first for now.

Status: draft for v2.0.0 release readiness
Date: 2026-04-18
Parent: [ADR-0007](../adr/ADR-0007-symbiote-rebrand.md)
Cutover planning: [Phase 3 rename execution plan](symbiote-phase3-rename-plan.md)
Runtime summary: `codelens://design/agent-experience`

This document specifies the end-to-end flows Symbiote MCP must support
so the rebrand is more than a rename. Every flow below is designed to
honor the symbiote metaphor (host retains identity + control, symbiote
attaches, together they form a superhuman capability neither has alone)
and to stay universal — any MCP-capable host should be able to follow
the flow with only standard MCP primitives.

## 0. Naming deployment strategy

`Symbiote` is the right **product metaphor** for the UX and flow model:
attach to a host, preserve host identity, add capabilities the host does
not have alone. But that does **not** mean the public primary product
name should flip immediately.

Current deployment policy:

- Keep **CodeLens MCP** as the public primary install/docs/binary name
  until trademark clearance is complete.
- Keep **Symbiote** as the transition codename, UX metaphor, and runtime
  alias family (`symbiote://`, `SYMBIOTE_*`) during the v1.9.x -> v2.0.0
  bridge.
- Treat a public primary-name cutover as **blocked pending clearance**,
  not as an unconditional release task.

Why this gate exists:

- bare `Symbiote` search results are still dominated by Marvel and the
  Linux rootkit story, which weakens first-run trust
- existing third-party `SYMBIOTE` registrations make a direct software
  rename materially riskier than the metaphor alone suggests

Design consequence: the **flows** can and should be symbiotic now, even
if the final public brand string stays `CodeLens MCP` longer than the
metaphor does.

## 1. Brand → UX principles

| Metaphor beat                    | UX translation                                                                                                 | Anti-pattern (don't do)                                                         |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Symbiote attaches to a host      | Single `attach` verb (install + configure + first call) is < 60 seconds                                        | Multi-step wizard, account creation, license keys                               |
| Host retains identity            | Host client never proxies through Symbiote for non-code operations                                             | Wrapping the host chat UI                                                       |
| Bidirectional benefit            | Every call returns compressed value to the host AND captures telemetry that improves future calls              | Silent data collection, unreciprocated feedback                                 |
| Superhuman emerges from the pair | Capabilities unavailable to host alone: mutation gate, canonical truth, handoff artifacts, cross-session audit | Duplicating what the host already does well (free-form chat, single-file edits) |
| Symbiote can detach cleanly      | `detach` verb removes all binaries/config/cache without residue                                                | Registry entries, system services, phone-home                                   |
| Symbiote evolves with the host   | Version gates are additive; a v1 host talking to a v2 symbiote still works                                     | Hard version pins, breaking schema migrations                                   |

## 2. User flow — first attachment (Time to first compressed answer)

Target: **< 60 seconds from zero to first `analyze_change_request`
response**. Universal — same flow for Claude Code, Codex CLI, Cursor,
Cline, Windsurf, or any MCP client that supports stdio or HTTP.

```text
[1] Choose install channel                        <-- 5s
       cargo install symbiote-mcp
       brew install symbiote-mcp
       curl -fsSL install.sh | bash
       docker run ghcr.io/.../symbiote-mcp

[2] Verify binary                                 <-- 2s
       symbiote-mcp --version
       symbiote-mcp . --cmd get_capabilities --args '{}'

[3] Attach to host                                <-- 10s
       Claude Code / Cursor / Cline / Windsurf:
         drop 3-line mcpServers JSON
       Codex CLI:
         symbiote-mcp attach codex
       CI / script:
         symbiote-mcp --transport http --port 7837 &

[4] First compressed call                         <-- 3s
       Agent issues analyze_change_request("...")
       Returns ranked files + risk + next actions in 1 tool call

[5] (optional) audit + export                     <-- 5s
       audit_builder_session → export_session_markdown
       signed-off artifact ready to paste into PR description
```

Acceptance test: an untrained user on a fresh machine reaches step [4]
in under 60 seconds end-to-end. Measure during every release.

## 2b. Product IA / interface surfaces

Even when the host owns the actual UI, Symbiote needs a stable
information architecture so every host renders the same product shape.

### Core surfaces

| Surface              | Primary user        | Minimum payload                                                                 |
| -------------------- | ------------------- | ------------------------------------------------------------------------------- |
| Attach               | human + host setup  | install channel, verify command, host config target, reachable MCP URL         |
| Session overview     | human + active agent| active profile, visible surface, daemon mode, index health, recent session id  |
| Task router          | active agent        | task phase, risk level, preferred executor, suggested next tools               |
| Audit timeline       | human + CI          | bootstrap evidence, verifier evidence, mutation evidence, audit verdict        |
| Handoff inspector    | planner/builder/reviewer | handoff artifact JSON, `delegate_to_codex_builder`, exported markdown     |
| Detach / migrate     | human + ops         | remove binary, remove config, preserve or delete runtime state                 |

### Minimal host-native rendering contract

- If a host has rich panels, render Session overview + Audit timeline.
- If a host is terminal-only, these surfaces can collapse to structured
  JSON resources and markdown exports.
- If a host is agent-only, the same surfaces must remain machine-readable
  through runtime resources and `suggested_next_calls`.

## 3. Agent flow — universal role lattice

Every host selects a role profile on attach. Symbiote's substrate
enforces what the role can and cannot do; the host chooses _which_
role, not _how_ the role is enforced.

```text
┌───────────────────────────────────────────────────────────────────────┐
│                        Symbiote Host Attach                           │
│                                                                       │
│   prepare_harness_session(profile=<role>)                             │
│          │                                                            │
│          ├── planner-readonly    → plan-phase tools visible           │
│          ├── builder-minimal     → build-phase + retrieval visible    │
│          ├── reviewer-graph      → review-phase + audit visible       │
│          ├── refactor-full       → build + preflight-gated visible    │
│          ├── evaluator-compact   → eval-phase + telemetry visible     │
│          ├── ci-audit            → review + eval, strict read-only    │
│          └── workflow-first      → phase-agnostic shortlist           │
│                                                                       │
│   preferred_phases  = [plan, review] | [build, review] | ...          │
│   preferred_executor hints route bulk work to codex-class executors   │
└───────────────────────────────────────────────────────────────────────┘
```

Universal means: any agent host that speaks MCP `tools/list` + params
resolves the correct role. Claude Code, Codex, Cursor, Cline, Windsurf,
and raw CI scripts all share the exact same bootstrap call.

## 4. Agent tool flow — selection, discovery, budget

Step by step. Every bullet is a single tool call unless marked
inline.

```text
[A] BOOTSTRAP
    1. prepare_harness_session(profile, detail=compact)
    2. tools/list(phase=<current phase>)            -- budgeted list
                                                       only the N tools
                                                       this phase needs
    3. (optional) tools/list(full=true)             -- full registry
                                                       if host insists

[B] DISCOVER — agent decides which tool to invoke
    • analyze_change_request(task=...)              -- 1 call, ranked
                                                       files + risk +
                                                       readiness
      OR
    • get_ranked_context(query=..., max_tokens=2k)  -- smart retrieval
                                                       when no specific
                                                       task known

[C] INVESTIGATE — only if analyze_change_request signals unclear scope
    • find_symbol(name=..., include_body=true)      -- exact symbol
    • find_referencing_symbols(file_path, symbol)   -- callers
    • get_symbols_overview(path)                    -- structural map
    • semantic_search(query)                        -- NL fallback
    Stop expanding as soon as top_findings answer the question.

[D] ACT
    • planner-phase  → plan_safe_refactor or issue-level brief
    • builder-phase  → verify_change_readiness → mutation tool
    • reviewer-phase → review_changes / impact_report
    • eval-phase     → start_analysis_job(kind=eval_session_audit)

[E] VERIFY
    • get_file_diagnostics(file_path)   -- post-edit type check
    • audit_builder_session|audit_planner_session

[F] HANDOFF (optional)
    • export_session_markdown           -- human-readable
    • planner_brief_producer.py         -- machine-readable
      or builder_result_producer.py
      or reviewer_verdict_producer.py
```

Budget discipline — every response carries `budget_hint` +
`suggested_next_tools`. Hosts that respect those are
capped in token usage by design; hosts that ignore them fall back to
doom-loop detection which replaces duplicate suggestions with
`start_analysis_job` escalation.

## 5. Agent tool routing — executor preference enforcement

ADR-0006 Layer 1 already ships `preferred_executor` metadata.
The universal flow is:

```text
tool_call(name)
   │
   ├── tool_defs::tool_preferred_executor(name)
   │        ├── Some("claude")        → host orchestrator runs it
   │        ├── Some("codex-builder") → host delegates to Codex CLI
   │        └── None                  → host free choice
   │
   └── host honors hint      (advisory in v1.9.x, enforced in v2.0+)
```

Universal means any MCP-capable host with access to both a reasoner
and a code executor can adopt the same routing table. Hosts that only
have one executor treat everything as `any`.

Counts as of v1.9.44: codex-builder 17 / claude 9 / any 83. These
counts ship in `docs/generated/surface-manifest.json` under
`tool_registry.preferred_executors`, so hosts can surface them in
their own UX (e.g., "17 of this repo's tools run faster through Codex
— attach it for best effect").

## 6. Agent reference flow — code navigation

The canonical code-navigation sequence for every supported language
family. Universal because every sequence is expressible as MCP tool
calls — no host-specific extension.

```text
USER / ORCHESTRATOR ASKS:  "What calls resolve_audit_session_view?"

[1] find_symbol(name="resolve_audit_session_view", include_body=false)
       → returns symbol location, signature

[2] find_referencing_symbols(
        file_path=<symbol's file>,
        symbol_name="resolve_audit_session_view",
        max_results=20
    )
       → returns ranked call sites with line/column

[3] (optional) get_impact_analysis(file_path=<symbol's file>, max_depth=2)
       → if caller asks "what breaks if I change it?"

[4] (optional) get_type_hierarchy(name=..., use_lsp=true)
       → if caller is looking at an interface / trait boundary
```

Fallback ladder (host-independent):

```
find_symbol (tree-sitter, exact) fails?
    → semantic_search (embeddings, natural language) fails?
        → get_ranked_context (query, multi-signal) fails?
            → host's built-in Grep tool
```

Every step returns bounded JSON; no host is ever forced to stream raw
file bytes unless `--full` is explicitly asked. This is what makes the
relationship symbiotic rather than parasitic — the host keeps control
of its token budget.

## 7. Agent harness flow — four modes end-to-end

### 7.1 Solo-local

```
stdio or single HTTP
  └── prepare_harness_session(profile="builder-minimal")
        ├── explore_codebase()
        ├── (iterate) find_symbol + get_file_diagnostics + mutation tools
        └── audit_builder_session()            ← warn if process skipped
```

One session can plan and edit. Mutation still requires
`verify_change_readiness` preflight; the symbiote refuses to let the
host burn its own work by skipping safety.

### 7.2 Planner-builder (primary multi-agent)

```
Planner session                             Builder session
 (7837 read-only daemon)                     (7838 mutation daemon)
─────────────────────                       ──────────────────────
prepare_harness_session                     prepare_harness_session
analyze_change_request                      ↓ (read planner_brief.json)
verify_change_readiness                     get_symbols_overview
register_agent_work                         get_file_diagnostics
claim_files  ─────────────────────────────→ verify_change_readiness
emit planner_brief.json ──────────────────→ (preflight match check)
                                            register_agent_work (inherits)
                                            mutation tool
                                            get_file_diagnostics (post)
                                            audit_builder_session
                                            emit builder_result.json
           ←── reviewer reads ──────────────
reviewer audits both, emits reviewer_verdict.json
```

The handoff is via JSON artifact (schema in
`docs/schemas/handoff-artifact.v1.json`). Any host can produce /
consume these artifacts — that is what keeps the flow universal.
Sample producers live under `examples/handoff/`.

### 7.3 Reviewer / CI-audit

```
CI runner
  └── prepare_harness_session(profile="ci-audit")
        ├── review_changes(changed_files=...)
        ├── audit_builder_session(session_id=<builder-id>)
        ├── audit_planner_session(session_id=<planner-id>)
        ├── (optional) start_analysis_job(kind="eval_session_audit")
        └── export_session_markdown → attach to PR
```

Read-only surface enforces "no mutations from CI", which is the
symbiote's explicit contract for this mode.

### 7.4 Batch analysis

```
async-capable runner
  └── start_analysis_job(kind=<impact/dead-code/refactor-safety/...>)
        ├── get_analysis_job(job_id)   ← poll
        └── get_analysis_section(analysis_id, section=...)
              ← incremental read, never full dump
```

Use when the wall-clock exceeds a single agent turn. Sections let the
host pull just the part its UI is rendering, preventing massive
payloads.

## 8. Error + recovery flow

Symbiote's contract: every failure response carries **structured
recovery hint** so the host can retry without prompt-engineering from
the error string.

```
tool_call(...)
   │
   └─ Err(e)
        ├── e.recovery_hint.action       ← "retry" / "escalate" / "abort"
        ├── e.recovery_hint.alternative  ← name of a fallback tool
        └── e.recovery_hint.after_ms     ← back-off if rate-limited
```

Examples of the advisory ladder:

| Failure                   | recovery_hint.action | alternative                         |
| ------------------------- | -------------------- | ----------------------------------- |
| Mutation preflight stale  | retry                | `verify_change_readiness` (refresh) |
| Namespace deferred-hidden | retry_with           | `tools/list` expanding namespace    |
| Rate-limited              | backoff              | —                                   |
| Semantic engine cold      | alternative          | `find_symbol` (tree-sitter)         |
| Schema validation         | abort                | —                                   |

Universal: the `recovery_hint` is plain JSON fields in the MCP
`ToolCallResponse`. Any MCP-literate host reads it; no vendor
extension needed.

## 9. Detachment flow

Symbiote must be detachable without residue. The verb in our docs:
`detach`.

```
[1] Stop daemons       (if any)
       killall symbiote-mcp
[2] Remove binary
       cargo uninstall symbiote-mcp
       brew uninstall symbiote-mcp
       rm ~/.local/bin/symbiote-mcp
[3] Remove host config
       edit .mcp.json / .cursor/mcp.json / AGENTS.md
[4] Remove runtime state (optional, recoverable)
       rm -rf .codelens/      ← v1.9.x
       rm -rf .symbiote/      ← v2.0.0+
```

The symbiote leaves no registry entries, no system services, no
phone-home. Clean detachment is a brand-trust feature, not an
afterthought.

## 10. Universal adoption contract

For Symbiote MCP to be truly universal across Claude Code, Codex
CLI, Cursor, Cline, Windsurf, and any future MCP host:

| Host assumption                               | How Symbiote honors it                                                                    |
| --------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Host may not support HTTP                     | stdio transport is always available with identical capabilities                           |
| Host may not support deferred loading         | Full tool surface returns in one `tools/list` if `deferredToolLoading: false`             |
| Host may not support `_meta` annotations      | `preferred_executor` is advisory; absence of consumption is not a failure                 |
| Host may not support MCP Tasks (SEP-1686)     | Long-running work uses `start_analysis_job` + poll instead; Tasks opt-in later            |
| Host does not offer a Codex-class executor    | `preferred_executor="codex-builder"` hints are ignored; all tools run in the orchestrator |
| Host is a CI runner, not a human-facing agent | `ci-audit` profile exists; audits are first-class artifacts                               |
| Host is a human in a terminal, no agent       | TUI crate (`symbiote-tui`) will grow; stdio one-shot mode works today                     |

No flow above requires a CodeLens-specific SDK, vendor login, cloud
service, or license key. If it needs anything beyond the binary + the
host's own MCP config, it is not in scope for v2.0.0.

## 11. Implementation checklist (v2.0.0 readiness)

Ordered by user-visible impact. Cross-referenced to commits where
already landed.

- [x] Canonical-truth manifest generation — `ece43c8`
- [x] Harness mode catalog — `5f00304` + ADR-0005
- [x] Handoff artifact v1 schema + 3 external producers — `57fa853`, `204a038`, `d5b65e5`
- [x] Phase-aware surface reduction — `1cf43ef`
- [x] `preferred_executor` routing metadata — `c26f92d` + ADR-0006
- [x] `symbiote://` URI alias + dual resource discovery emission + `env_compat` helper — `50f5dcf` + ADR-0007 Phase 2
- [x] `detach` command in `install.sh` and Homebrew formula, backed by `codelens-mcp detach <host>` / `detach --all` for machine-editable host config cleanup
- [x] `codelens-mcp attach <host>` subcommand generating host-native MCP config templates for Claude Code / Cursor / Cline / Windsurf / Codex CLI; `symbiote-mcp` binary alias lands with the Phase 3 rename
- [ ] Full rename pass (Phase 3): crate names, binary, repo, `.codelens/` → `.symbiote/`, docs
- [x] `docs/migrate-from-codelens.md` with line-by-line config diffs per host
- [ ] Announcement post + v2.0.0 release notes with rootkit disambiguation paragraph

## 12. Success criteria (post-v2.0.0)

- **Time-to-first-answer** p95 < 60 seconds on a fresh macOS or Linux
  machine, measured by CI against a scripted user.
- **Attach-detach cleanness**: after attach + N tool calls + detach,
  `find ~ -type d -name ".symbiote*"` returns nothing the user didn't
  explicitly keep.
- **Universal host parity**: the same 10-step integration test passes
  against Claude Code, Codex CLI, Cursor, Cline, and a raw Python MCP
  client. No host-specific branches in the test.
- **Adoption signal**: at least one external host runs the handoff
  artifact producers as part of its real workflow within 90 days of
  v2.0.0 GA. If not, re-evaluate whether the universality claim is
  justified.

## References

- [ADR-0005 Harness v2](../adr/ADR-0005-harness-v2.md)
- [ADR-0006 Agent routing enforcement](../adr/ADR-0006-agent-routing-enforcement.md)
- [ADR-0007 Symbiote rebrand](../adr/ADR-0007-symbiote-rebrand.md)
- [Harness modes](../harness-modes.md)
- [Handoff artifact v1 schema](../schemas/handoff-artifact.v1.json)
- Handoff producers: `examples/handoff/{planner_brief,builder_result,reviewer_verdict}_producer.py`
