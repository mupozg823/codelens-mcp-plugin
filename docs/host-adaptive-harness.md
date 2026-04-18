# CodeLens MCP — Host-Adaptive Harness Architecture

> Grounded architecture for using CodeLens across different agent hosts without repeating the same routing mistakes in every repository.

This document exists because memory-only routing does not scale. If the
decision to use or skip CodeLens lives only in one agent session, the
same inefficiencies reappear in the next repository. The fix is to move
that logic into a durable architecture contract: one shared substrate,
multiple host adapters.

Portable runtime summary: `codelens://harness/host-adapters`

Portable UX / flow summary: `codelens://design/agent-experience`

Concrete per-host bundles:

- `codelens://host-adapters/claude-code`
- `codelens://host-adapters/codex`
- `codelens://host-adapters/cursor`
- `codelens://host-adapters/cline`

## Root Cause

- Memory-only policy is not portable. A good routing decision in one session is lost in the next project, next host, or next team member.
- Host capability differences are real. Claude Code, Codex, Cursor, Cline, and OpenHands do not expose the same primitives for subagents, worktrees, background execution, rules, or MCP governance.
- One-size-fits-all harnessing creates opposite failures at once: too much CodeLens overhead on trivial local edits, and too little CodeLens discipline on multi-file review/refactor flows.
- Over-engineering hides the real issue. Adding new routers, lanes, or agent chatter without eval-backed signal increases complexity faster than it increases quality.

## External Evidence

### Research

- [SWE-agent](https://arxiv.org/abs/2405.15793) argues that interface design matters materially: its custom agent-computer interface improved repository navigation, file editing, and execution effectiveness.
- [AutoCodeRover](https://arxiv.org/abs/2404.05427) shows that structured program representations and code search can beat flatter file-only retrieval, with lower cost on SWE-bench-lite.
- [Agentless](https://arxiv.org/abs/2407.01489) is the main warning against harness bloat: a simple localization → repair → validation pipeline outperformed more elaborate open-source agents on SWE-bench Lite.
- [Survey on Evaluation of LLM-based Agents](https://arxiv.org/abs/2503.16416) highlights the current evaluation gap: cost-efficiency, safety, robustness, and fine-grained measurement still lag behind headline benchmark numbers.
- [SWE-Skills-Bench](https://arxiv.org/abs/2603.15401) shows that skills are not automatically useful. Most injected skills in the benchmark did not improve pass rate, and some degraded it because the guidance mismatched the actual repo context.

### Official host architecture signals

- Anthropic’s [Managed Agents](https://www.anthropic.com/engineering/managed-agents) architecture separates `session`, `harness`, and `sandbox`, and explicitly warns that harness assumptions go stale as models improve.
- Anthropic’s [Claude Code subagents docs](https://code.claude.com/docs/en/sub-agents) and [MCP docs](https://code.claude.com/docs/en/mcp) show that Claude Code is strongest when policy, isolation, and tool scope are explicit.
- Anthropic’s March 24, 2026 webinar deck, [Claude Code Advanced Patterns](https://resources.anthropic.com/hubfs/Claude%20Code%20Advanced%20Patterns_%20Subagents%2C%20MCP%2C%20and%20Scaling%20to%20Real%20Codebases.pdf), pushes the same direction: CLAUDE.md, hooks, subagents, and parallelization are the durable primitives.
- OpenAI’s [Codex GA announcement](https://openai.com/index/codex-now-generally-available/) reports real internal adoption and measurable PR throughput improvement, but the product shape stays execution-focused rather than planner-heavy.
- OpenAI’s [Codex app announcement](https://openai.com/index/introducing-the-codex-app/) emphasizes reusable skills and background automations, which is consistent with Codex being strongest as a builder/executor with sharable repo policy.
- OpenAI’s [Docs MCP guide](https://developers.openai.com/learn/docs-mcp) explicitly recommends putting MCP expectations into `AGENTS.md`, which supports compiling policy into host-native artifacts rather than hoping the agent “remembers.”
- Cursor’s official docs describe [rules](https://docs.cursor.com/en/context), [background agents](https://docs.cursor.com/en/background-agents), and [custom modes](https://docs.cursor.com/en/chat/agent). That is a host with strong editor-local routing and separate remote execution posture, not just “another Codex.”

### OSS adoption snapshot

GitHub stars are not proof of architectural quality, but they are useful as a demand signal. Snapshot taken on 2026-04-18:

- [openai/codex](https://github.com/openai/codex): 75.3k
- [OpenHands/OpenHands](https://github.com/OpenHands/OpenHands): 71.4k
- [cline/cline](https://github.com/cline/cline): 60.4k
- [microsoft/autogen](https://github.com/microsoft/autogen): 57.2k
- [Aider-AI/aider](https://github.com/Aider-AI/aider): 43.5k
- [continuedev/continue](https://github.com/continuedev/continue): 32.6k
- [SWE-agent/SWE-agent](https://github.com/SWE-agent/SWE-agent): 19.0k
- [SWE-agent/mini-swe-agent](https://github.com/SWE-agent/mini-swe-agent): 3.9k

The signal in that list is not “everyone should look the same.” The real pattern is that the successful systems specialize:

- Codex: worktrees, skills, shared repo policy, execution.
- OpenHands: SDK + CLI + GUI + cloud, with clear separation between engine and product surfaces.
- Cline: human-in-the-loop IDE control, approvals, checkpoints, browser loop.
- Aider: minimal terminal loop with strong codebase mapping and git/test ergonomics.
- Continue: source-controlled CI checks and repo-native policy.
- mini-SWE-agent: radical simplification of the agent scaffold to keep the language model, not the harness, in the center.

## Architectural Conclusion

CodeLens should not become a universal orchestrator. It should become the shared substrate that host-specific adapters compile into.

### What should stay global

- Session bootstrap and health summary
- Role/profile-scoped surfaces
- Deferred tool loading
- Verifier and rename preflight
- Session-scoped audits
- Durable analysis jobs and section handles
- Portable handoff schema and runtime resources

### What should stay host-specific

- Subagent semantics
- Worktree lifecycle and merge workflow
- UI approvals and background execution posture
- Prompting style and agent personality
- Team-level rules files and IDE-mode configuration

## Recommended Reference Architecture

### Layer 1. Durable substrate

CodeLens owns:

- `prepare_harness_session`
- profiles and deferred loading
- `verify_change_readiness` and rename-aware preflight
- `audit_builder_session` / `audit_planner_session`
- `start_analysis_job` / `get_analysis_job` / `get_analysis_section`
- `codelens://surface/manifest`
- `codelens://harness/modes`
- `codelens://harness/spec`
- `codelens://schemas/handoff-artifact/v1`

### Layer 2. Host adapter

The host adapter decides:

- when the task is trivial enough to stay native
- when CodeLens bootstrap is worth paying for
- which profile and harness mode to use
- which host-native config file should carry the policy

### Layer 3. Policy compiler

The same logical policy should compile into different artifacts:

| Host | Native artifacts |
| --- | --- |
| Claude Code | `CLAUDE.md`, `.mcp.json`, `managed-mcp.json`, subagent definitions |
| Codex | `AGENTS.md`, `~/.codex/config.toml`, repo skills |
| Cursor | `.cursor/rules`, `AGENTS.md`, `.cursor/mcp.json`, `environment.json` |
| Cline | `mcp_servers.json`, `.clinerules`, repo instructions |

The runtime form of this compiler is now host-scoped resource bundles.
Each `codelens://host-adapters/{host}` resource includes:

- recommended harness modes
- recommended CodeLens profiles
- routing defaults by task class
- host-native config targets
- copy-ready template snippets

The same policy is also emitted at tool granularity through runtime metadata:

- `tools/list` exposes `_meta["codelens/preferredExecutor"]` per tool
- `tools/call` echoes `_meta["codelens/preferredExecutor"]` on the call result
- current labels are `codex-builder`, `claude`, and `any`
- `suggested_next_tools` / `suggested_next_calls` may prepend the synthetic host action `delegate_to_codex_builder` when the next step crosses into a builder-heavy lane or a builder-heavy tool is being retried in a loop
- that synthetic action is advisory, not callable on the server; hosts should read its `delegate_tool`, optional `delegate_arguments`, `carry_forward`, and `briefing` payload to launch a builder session without reshaping context

### Layer 4. Eval and governance

Only keep lanes that create new signal:

- session audit aggregation is real signal
- synthetic tool-selection grading without labels is not
- duplicate retrieval metrics on top of an existing CI gate are not

## Host-by-Host Guidance

### Claude Code

- Best role: planner/reviewer.
- Best CodeLens mode: `planner-builder` or `reviewer-gate`.
- Why: Claude Code has the richest primitives for constrained subagents, scoped MCP, hooks, and explicit policy.
- Use CodeLens for: bootstrap, architecture review, preflight, planner-session audit, and artifact handoff.
- Do not use CodeLens as a substitute for Claude’s own subagent isolation or hook system.

### Codex

- Best role: builder/refactor executor.
- Best CodeLens mode: `planner-builder`, with `builder-minimal` as the default mutation surface and `refactor-full` only after preflight.
- Why: Codex already has strong worktree, skill, MCP, and automation ergonomics.
- Use CodeLens for: bounded mutation gating, builder-session audit, and CI-facing analysis artifacts.
- Do not force Claude-style planner choreography into Codex when a direct executor loop is enough.

### Cursor

- Best role: editor-local adaptive assistant with optional background execution.
- Best CodeLens mode: `solo-local`, `reviewer-gate`, or `batch-analysis`.
- Why: Cursor’s rules and modes are effectively a prompt/router layer, and background agents have a different trust boundary from foreground editing.
- Use CodeLens for: review-heavy work, background audit jobs, and narrow MCP surfaces.
- Do not expose the whole CodeLens registry in every Cursor mode.

### Cline

- Best role: interactive debugging and explicit-approval foreground execution.
- Best CodeLens mode: `solo-local` or `planner-builder`.
- Why: Cline already provides a strong human-in-the-loop checkpoint loop.
- Use CodeLens for: reviewer-heavy exploration and session audit when the work must cross sessions.
- Do not treat Cline as a headless CI substrate.

## Dynamic Adaptation: What “Adaptive” Actually Means

Dynamic adaptation should not mean “let the model guess.” It should mean deterministic routing over a small set of observable inputs:

- host identity
- interactive vs background execution
- task phase: lookup, plan, review, build, eval
- scope: single-file vs multi-file
- mutation risk
- need for durable handoff or audit evidence

Given those inputs, the system should output:

- harness mode
- CodeLens profile
- whether native-first or CodeLens-first is preferred
- whether handoff artifacts are required
- whether async analysis jobs should replace direct report expansion

## Product Direction for CodeLens

The practical path is:

1. Publish portable adapter policy as a runtime resource.
2. Keep the shared substrate small and measurable.
3. Compile policy into host-native artifacts instead of relying on memory.
4. Add new lanes or host behaviors only when they have a benchmark or merge-gating reason to exist.

That is the difference between a reusable harness substrate and another overfit repo-local workflow.
