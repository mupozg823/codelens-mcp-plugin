# ADR-0006: Agent routing enforcement — server-side `preferred_executor` metadata

- Status: Accepted
- Date: 2026-04-18
- Supersedes: none (extends ADR-0005 §5 "harness v2")

## Context

CLAUDE.md Global asserts "Codex: implementation / Claude: orchestration, review".
The 2026-04-18 session demonstrated this policy is systematically violated:
18 of 18 commits in that session were executed by a Claude orchestrator
doing implementation directly. Root cause: the policy exists only as
prose. No mechanism binds the model's subagent selection. Memory-based
self-correction is per-project — other users adopting CodeLens inherit
the policy violation, not the learning.

External research (2025-2026) confirms the gap:

- MCP tool annotations are explicitly **hints, not enforcement**; the
  [MCP blog post on tool annotations](https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations/)
  states enforcement must live at host/client layer.
- [`agentcop`](https://github.com/trusthandoff/agentcop) ships production
  runtime gates (`AgentHierarchy` raises `PermissionError` on unauthorized
  handoffs) across LangGraph/AutoGen/CrewAI/LlamaIndex.
- [MCP SEP-1686 Tasks](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/1686)
  accepts a task primitive enabling call-now/fetch-later semantics — the
  protocol-level solution, still propagating to clients.
- [FrugalGPT (arXiv:2305.05176)](https://arxiv.org/abs/2305.05176) measures
  up to 98% cost reduction with cascade routing; the empirical case for
  Claude→Codex routing is not speculative.
- [OpenHands SDK (arXiv:2511.03690)](https://arxiv.org/html/2511.03690v1)
  risk-tags every tool and gates execution by threshold — the dispatch-layer
  pattern we adapt here.

## Decision

Add `preferred_executor` routing metadata to the CodeLens manifest and
emit it as a dispatch-time annotation. Policy moves from CLAUDE.md prose
to code shipped with the MCP server. Every user adopting CodeLens
inherits the routing signal without configuration.

### Three layered mechanisms (adopt in order)

**Layer 1 — Per-tool `preferred_executor` metadata** (this ADR)

A new classifier `tool_preferred_executor(name) -> Option<&'static str>`
returns one of:

- `"codex-builder"` — bulk mutation, multi-file refactor, pure relocation.
  Examples: `rename_symbol`, `replace_symbol_body`, `replace`,
  `delete_lines`, `refactor_*`, `create_text_file`, `insert_content`.
- `"claude"` — orchestration, synthesis, design. Examples:
  `analyze_change_request`, `plan_safe_refactor`, `review_architecture`,
  `trace_request_path`.
- `None` — phase-agnostic, either executor is fine. Examples: `read_file`,
  `find_symbol`, `get_symbols_overview`, audit tools, session
  coordination.

Exposed in:

- per-tool entries in `docs/generated/surface-manifest.json`
- tool annotations `_meta["codelens/preferredExecutor"]` in `tools/list`
  and `tools/call` responses

Clients can read the annotation and route accordingly. The server never
blocks — v1 is an **advisory** signal following MCP's
hints-not-enforcement framing.

**Layer 2 — Dispatch-time envelope hints** (follow-up commit)

Extend the existing doom-loop detector (`dispatch/envelope.rs` +
`dispatch/table.rs`) so that when a main session calls a
`codex-builder`-preferred tool N times consecutively, the response's
`suggested_next_tools` includes `delegate_to_codex_builder` with a
pre-formatted briefing scaffold. The briefing points at the planner's
earlier `analyze_change_request` output so the handoff is zero-reshape.

**Layer 3 — SEP-1686 Task primitive** (deferred, upstream-blocked)

When Claude Code and Codex CLI both support MCP Tasks, bulk refactor
workflows (`plan_safe_refactor`, `refactor_*`) become Tasks. The main
orchestrator submits; only a worker client with the same task_id can
complete. Protocol-level enforcement, cleanest separation. Deferred
until SEP-1686 lands in the hosts we support.

## Alternatives considered and rejected

| Alternative                            | Why rejected                                                                                        |
| -------------------------------------- | --------------------------------------------------------------------------------------------------- |
| "Just put it in CLAUDE.md"             | Already tried. 18/18 violation rate in the 2026-04-18 session.                                      |
| Per-repo `.claude/routing-policy.json` | Doesn't propagate to other CodeLens users; same prose-based failure mode.                           |
| New `delegate` tool                    | Would grow the 109-tool cap we just defended in ADR-0005.                                           |
| Hard-block in dispatch                 | Violates MCP's hints-not-enforcement convention; clients should retain override. Re-evaluate in v2. |
| Client-side wrapper                    | We don't ship a client. Users bring Claude Code / Codex / Cursor.                                   |
| agentcop embedded                      | Python-only, doesn't fit a Rust MCP server.                                                         |

## Consequences

### Positive

- Policy propagates with the binary. Every user of CodeLens v1.9.45+
  receives the routing signal, solving the "memory-only, per-project"
  limitation the user flagged on 2026-04-18.
- Reuses the `_meta` envelope we already own (`anthropic/maxResultSizeChars`
  is the established precedent).
- Auditable: `preferred_executor` counts land in
  `docs/generated/surface-manifest.json` and the CI drift gate catches
  inconsistent reclassification.
- Empirical backing (FrugalGPT 98% cost reduction) gives the hint real
  numbers, not prose assertions.

### Negative

- Layer 1 is advisory. A stubborn host still free to ignore it. Layer 2
  (envelope hints) provides soft pressure; Layer 3 (SEP-1686) provides
  hard pressure. Neither is in this ADR.
- Classifier is hand-maintained. Every new tool requires a routing
  decision; reviewers must check the classification in code review.
- Wrong classification is worse than no classification — a tool marked
  `claude` that's actually cheap-to-execute wastes Claude tokens. Start
  with conservative defaults (`None`) and only tag where the answer is
  clear.

## Execution plan

1. This commit — ADR + Layer 1 metadata
2. Next commit — surface_manifest exposure + integration test
3. Follow-up — Layer 2 envelope hint integration with doom-loop detector
4. When SEP-1686 lands in Claude Code + Codex CLI — Layer 3 migration

## References

- [agentcop](https://github.com/trusthandoff/agentcop) — runtime hierarchy enforcement
- [MCP SEP-1686 Tasks](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/1686)
- [MCP Issue #1284 static metadata](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/1284)
- [MCP tool-annotations blog 2026-03-16](https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations/)
- [Anthropic Engineering — Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)
- [langgraph-supervisor](https://pypi.org/project/langgraph-supervisor/)
- [OpenHands Software Agent SDK](https://arxiv.org/html/2511.03690v1)
- [FrugalGPT (arXiv:2305.05176)](https://arxiv.org/abs/2305.05176)
- [RouterBench (arXiv:2403.12031)](https://arxiv.org/html/2403.12031v1)
- [xRouter (arXiv:2510.08439)](https://arxiv.org/html/2510.08439v1)
- Internal: `feedback_claude_codex_routing.md` (per-project memory, 2026-04-18)
- Internal: `feedback_self_dogfooding.md` (CodeLens routing, 2026-04-18)
