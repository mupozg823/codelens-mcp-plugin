# ADR-0004: Multi-Agent Concurrency Primitives (Bounded-Evidence Only)

- Status: Accepted (MVP implemented 2026-04-15, primitives 1 + 2 live)
- Date: 2026-04-15

## Context

CodeLens is already consumed by more than one agent at a time in practice:
Claude Code, Codex CLI, Cursor, editor MCP integrations, and automation
scripts all attach to the same project and its symbol index concurrently.

A recent session produced a concrete, reproducible pain point: a Claude
Code session was preparing PR #74 on the main repository checkout at the
same time a Codex session was editing the same working tree. Every
`git add -A && git commit` the Claude session made silently swept in the
Codex edits, causing the `docs/architecture.md` schema-count guard to
fail repeatedly (68 → 69 → 73 → 81 within a single PR cycle). No tool in
the ecosystem surfaced the fact that a second agent had touched the same
files; the collision was only visible after CI failed.

Git already coordinates humans at the commit level. It does not
coordinate agents at the **intent** level (which files an agent is about
to touch, which symbols it has claimed, which edits overlap before they
reach disk). Existing MCP servers (Serena, continue.dev, aider-mcp,
etc.) all assume a single agent per project. This is an empty space.

CodeLens already has most of the building blocks for a coordination
layer:

| Existing building block                               | Reusable for                                                                           |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `mutation_gate` + `verify_change_readiness` preflight | record + check advisory file leases                                                    |
| `prepare_harness_session` + logical session IDs       | stable agent identity across tool calls                                                |
| `memory` (project-scoped) + `codelens://` resources   | low-friction broadcast channel                                                         |
| `analysis_handle` + async `start_analysis_job`        | wrap an edit intent in a handle that other agents can observe                          |
| `diff_aware_references` + `impact_report`             | compare two pending edit plans at the symbol-graph level before either hits disk       |
| `canonical_tool_name` + telemetry normalization       | already routes multiple aliases onto one bucket — extends naturally to cross-agent IDs |

## Decision

Extend CodeLens with a minimal set of **coordination primitives**, under
one strict scope rule:

> CodeLens exposes coordination **evidence**. It does not enforce
> coordination **policy**.

The agent (or the host orchestrator around the agent) decides what to do
with the evidence. CodeLens never refuses to let an agent edit, never
takes a write lock, never blocks a tool call on another agent's state.
Every primitive returns data; behaviour stays in the host. This keeps the
existing contract in `docs/adr/ADR-0001` and the CLAUDE.md guidance
("`server_role: supporting_mcp`, `orchestration_owner: host`") intact.

### Primitives (ordered from simplest to widest)

1. **`agent_work_registry`** — session-scoped presence record.
   - `register_agent_work({ session_id, agent_name, branch, worktree, intent, ttl_secs })` — agent advertises what it is doing
   - `list_active_agents()` — other sessions see the currently registered agents, their heartbeats, and their stated intent
   - Backing store: existing project memory + a short-TTL table in the session store. No new DB.

2. **`claim_files` / `release_files`** — advisory file lease.
   - `claim_files({ session_id, paths: [...], reason, ttl_secs })` — a soft reservation, not an OS lock
   - `release_files({ session_id, paths: [...] })` — explicit release; TTL eventually releases anyway
   - `verify_change_readiness` gains a **new evidence field** `overlapping_claims: [{ session_id, agent_name, paths, reason }]`. The readiness score can drop to `caution` when there is a conflicting claim from a different session. **It does not block**. The agent decides.

3. **`agent_activity` resource** — `codelens://activity/current`.
   - Read-only bounded listing of the last N tool calls per active session, with symbol/file scope where available.
   - Consumers: orchestrators, dashboards, a coordinating "supervisor" agent.

4. **`reconcile_diff_intent`** — symbol-graph comparison of two pending edits.
   - Inputs: two intent descriptors (for example, `{ paths, symbols, rough_change_kind }`), which come from either the registry or explicit tool arguments.
   - Output: a report keyed off `diff_aware_references` and `impact_report` that lists which symbols would be touched by both intents, and the blast radius where they interact. Bounded, evidence only.

### MVP scope (this ADR)

Primitives **1** and **2** are the MVP. They are enough to prevent a
repeat of the session this ADR was born in: the Claude Code session
would have seen a live claim on `docs/architecture.md`, the preflight
readiness would have been `caution`, and the human user would have had
the information needed to coordinate the two agents.

Primitives **3** and **4** are recorded here so the design is coherent,
but they land in a follow-up ADR when 1 and 2 have a real consumer.

### Operating policy (established with the MVP)

Running more than one **mutation-enabled** agent against the same
working tree is not the supported configuration. The first safety
boundary is `worktree` / `branch` separation — give each mutation agent
its own. CodeLens coordination primitives do not replace that boundary;
they provide pre-commit intent evidence so hosts and humans can spot
a conflict **before** Git sees it. Reviewer / planner / read-only agents
may share a tree freely. The shared HTTP daemon is the recommended
carrier for cross-agent coordination state.

## Consequences

### Positive

- Fills a real gap nobody else is filling. Serena, aider, continue.dev
  all assume one agent per project.
- Leverages existing CodeLens surface (mutation gate, session store,
  analysis handles, memory), so the implementation cost is low.
- Strengthens the product story without violating "미소비 추상화 금지":
  every primitive has a concrete first consumer (the host's preflight
  loop, other agents' readiness signals).
- Makes CodeLens the natural integration point when a team runs Claude
  Code + Codex + Cursor on the same repo simultaneously.

### Negative / risks

- Increases API surface. Must resist expansion beyond the four primitives
  above unless a new consumer appears.
- Session identity is advisory. A malicious or buggy agent can lie about
  its `session_id` or skip registration. CodeLens explicitly does not
  promise enforcement — only visibility. The host (or human) arbitrates.
- Registry and claim records live for a bounded TTL and then expire.
  Agents that crash mid-edit will eventually drop out of the registry.
  This is simpler than liveness detection but must be documented.

### Non-goals

- **Write locks.** CodeLens never prevents a write. Claims are advisory.
- **Text-level conflict resolution.** Git already does that post-commit.
  CodeLens operates pre-commit on the symbol graph.
- **Cross-project coordination.** One project scope per session as today.
- **Agent authentication.** Session IDs identify a session, not a
  trust-level. Any auth/authz stays in the host.

## Implementation sketch (MVP only)

- `crates/codelens-mcp/src/agent_coordination.rs` (new, thin): in-memory
  `AgentWorkRegistry { entries: Vec<AgentWorkEntry> }` keyed by
  `session_id`. TTL-pruned on every read. Persisted lazily into the
  project memory so a restart does not erase claims older than a few
  minutes.
- `crates/codelens-mcp/src/mutation_gate.rs`: extend
  `MutationGateFailure` / readiness payload to carry `overlapping_claims`.
  Downgrade readiness from `ready` → `caution` when any claim from a
  different session covers a file in the preflight path set. **Never
  emit `blocked` on this signal alone.**
- `crates/codelens-mcp/src/tool_defs/build.rs`: register four new tools
  (`register_agent_work`, `list_active_agents`, `claim_files`,
  `release_files`). All read-only or mutating with
  `audit_category: "coordination"`.
- Integration tests:
  - two logical sessions register, second session sees the first
  - claim on `docs/foo.md` causes the second session's
    `verify_change_readiness` to report `caution` with overlap
    evidence
  - TTL expiry releases the claim without an explicit call
  - the first session's `release_files` immediately restores `ready`

## Verification plan

- All three CI platforms green (Ubuntu / macOS / Windows).
- `cargo test -p codelens-mcp coordination` covers the four integration
  scenarios above.
- Manual: run two `codelens-mcp` sessions against the same project,
  confirm `list_active_agents` on one sees the other.
- Benchmark gate: zero impact on `benchmarks/token-efficiency.py` (the
  primitives only run when explicitly called).

## Related work and prior ADRs

- `docs/adr/ADR-0001` — host keeps orchestration ownership. This ADR is
  consistent: CodeLens is still a supporting MCP, still evidence-only.
- `docs/adr/ADR-0003` — registry-derived guidance and thin workflow
  entrypoints. The four new primitives follow the same "registry as
  single source of truth" pattern.
- Prior art: file-locking in multi-user editors (Google Docs
  "now editing" indicator), Git's `fetch --dry-run` (evidence over
  enforcement), LSP server coordination in monorepo IDEs.

## Open questions

- Should `claim_files` accept a symbol path (`name_path`) as well as a
  file path? Symbols are more precise but require tree-sitter resolution
  before every claim — MVP keeps file paths only.
- How should `list_active_agents` expose agents that have deregistered
  but whose claims have not yet TTL-expired? Current sketch: show them
  with `status: "departed"` until the claim expires.
- Should there be a global rate limit on registrations to prevent a
  misbehaving agent from polluting the registry? MVP: per-session rate
  limit only, same as other mutating tools.

## Revisit conditions

- Revisit when MVP lands and at least one host (Claude Code or Codex)
  consumes `overlapping_claims` in its preflight UX.
- Revisit primitives 3 and 4 if the MVP is adopted and users ask for
  symbol-graph conflict preview explicitly.
- Revisit the entire ADR if Git, LSP, or an upstream MCP spec adds a
  native coordination mechanism that supersedes this.
