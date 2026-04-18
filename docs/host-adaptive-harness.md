# CodeLens MCP — Host-Adaptive Harness Architecture

> Grounded architecture for using CodeLens across different agent hosts without repeating the same routing mistakes in every repository.

This document exists because memory-only routing does not scale. If the
decision to use or skip CodeLens lives only in one agent session, the
same inefficiencies reappear in the next repository. The fix is to move
that logic into a durable architecture contract: one shared substrate,
multiple host adapters.

Portable runtime summary: `codelens://harness/host-adapters`

Compatibility alias for consumers that want one resolved host contract:
`codelens://harness/host` with a `host` parameter such as `{"host":"claude-code"}`.

Portable UX / flow summary: `codelens://design/agent-experience`

Concrete per-host bundles:

- `codelens://host-adapters/claude-code`
- `codelens://host-adapters/codex`
- `codelens://host-adapters/cursor`
- `codelens://host-adapters/cline`
- `codelens://host-adapters/windsurf`

<!-- SURFACE_MANIFEST_HOST_ADAPTER_SUMMARY:BEGIN -->
## Generated Host Runtime Snapshot

Generated from the canonical surface manifest. Runtime resources remain the authoritative source when the doc and live server differ.

### `claude-code`

- Resource: `codelens://host-adapters/claude-code`
- Best fit: planner and reviewer orchestration with isolated research and explicit policy control
- Recommended modes: `solo-local`, `planner-builder`, `reviewer-gate`
- Preferred profiles: `planner-readonly`, `reviewer-graph`
- Default compiled overlay: profile=`planner-readonly`, task_overlay=`planning`
- Primary bootstrap sequence: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `explore_codebase` -> `review_architecture`
- Compiler targets: `CLAUDE.md`, `.mcp.json`, `managed-mcp.json`, `subagent definitions`

### `codex`

- Resource: `codelens://host-adapters/codex`
- Best fit: builder and refactor execution, parallel worktree-based implementation, and automation
- Recommended modes: `solo-local`, `planner-builder`, `batch-analysis`
- Preferred profiles: `builder-minimal`, `refactor-full`, `ci-audit`
- Default compiled overlay: profile=`builder-minimal`, task_overlay=`editing`
- Primary bootstrap sequence: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `get_file_diagnostics`
- Compiler targets: `AGENTS.md`, `~/.codex/config.toml`, `repo-local skill files`

### `cursor`

- Resource: `codelens://host-adapters/cursor`
- Best fit: editor-local iteration with scoped rules plus asynchronous remote execution when needed
- Recommended modes: `solo-local`, `reviewer-gate`, `batch-analysis`
- Preferred profiles: `planner-readonly`, `reviewer-graph`, `ci-audit`
- Default compiled overlay: profile=`reviewer-graph`, task_overlay=`review`
- Primary bootstrap sequence: `prepare_harness_session` -> `review_changes` -> `impact_report` -> `diff_aware_references` -> `audit_planner_session`
- Compiler targets: `.cursor/rules`, `AGENTS.md`, `.cursor/mcp.json`, `background-agent environment.json`

### `cline`

- Resource: `codelens://host-adapters/cline`
- Best fit: human-in-the-loop debugging and foreground execution with explicit approvals
- Recommended modes: `solo-local`, `planner-builder`
- Preferred profiles: `builder-minimal`, `reviewer-graph`
- Default compiled overlay: profile=`builder-minimal`, task_overlay=`editing`
- Primary bootstrap sequence: `prepare_harness_session` -> `get_file_diagnostics` -> `verify_change_readiness` -> `trace_request_path` -> `plan_safe_refactor`
- Compiler targets: `mcp_servers.json`, `.clinerules`, `repo instructions`

### `windsurf`

- Resource: `codelens://host-adapters/windsurf`
- Best fit: editor-local implementation with a hard MCP tool cap and bounded foreground agent flows
- Recommended modes: `solo-local`, `reviewer-gate`
- Preferred profiles: `builder-minimal`, `planner-readonly`
- Default compiled overlay: profile=`builder-minimal`, task_overlay=`editing`
- Primary bootstrap sequence: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `get_file_diagnostics`
- Compiler targets: `~/.codeium/windsurf/mcp_config.json`
<!-- SURFACE_MANIFEST_HOST_ADAPTER_SUMMARY:END -->

## Root Cause

- Memory-only policy is not portable. A good routing decision in one session is lost in the next project, next host, or next team member.
- Host capability differences are real. Claude Code, Codex, Cursor, Cline, Windsurf, and OpenHands do not expose the same primitives for subagents, worktrees, background execution, rules, or MCP governance.
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
| Windsurf | `~/.codeium/windsurf/mcp_config.json`, workspace rules or repo instructions |

The runtime form of this compiler is now host-scoped resource bundles.
Each `codelens://host-adapters/{host}` resource includes:

- recommended harness modes
- recommended CodeLens profiles
- compiled default task overlays and primary bootstrap sequence
- routing defaults by task class
- host-native config targets
- copy-ready template snippets

The same policy is also emitted at tool granularity through runtime metadata:

- `tools/list` exposes `_meta["codelens/preferredExecutor"]` per tool
- `tools/call` echoes `_meta["codelens/preferredExecutor"]` on the call result
- HTTP `initialize` advertises `capabilities.tools.listChanged = true`
- HTTP sessions emit `notifications/tools/list_changed` after runtime surface changes such as `set_profile` and `set_preset`
- current labels are `codex-builder`, `claude`, and `any`
- `suggested_next_tools` / `suggested_next_calls` may prepend the synthetic host action `delegate_to_codex_builder` when the next step crosses into a builder-heavy lane or a builder-heavy tool is being retried in a loop
- that synthetic action is advisory, not callable on the server; hosts should read its `handoff_id`, `delegate_tool`, optional `delegate_arguments`, `carry_forward`, and `briefing` payload to launch a builder session without reshaping context
- when `delegate_arguments` already exist, replay them verbatim for the first delegated builder call instead of reconstructing them from prose; preserve `handoff_id` unchanged so planner-side emission and builder-side execution remain correlatable across sessions

### Layer 4. Eval and governance

Only keep lanes that create new signal:

- session audit aggregation is real signal
- synthetic tool-selection grading without labels is not
- duplicate retrieval metrics on top of an existing CI gate are not

## Host-by-Host Guidance

<!-- SURFACE_MANIFEST_HOST_ADAPTER_GUIDANCE:BEGIN -->
Generated from the canonical surface manifest. Use this block as the default operator guidance when the prose below is stale.

### `claude-code`

- Best fit: planner and reviewer orchestration with isolated research and explicit policy control
- Recommended CodeLens modes: `solo-local`, `planner-builder`, `reviewer-gate`
- Preferred profiles: `planner-readonly`, `reviewer-graph`
- Native host primitives: `CLAUDE.md`, `subagents and agent teams`, `hooks`, `managed-mcp.json and .mcp.json`, `subagent-scoped MCP servers`
- Use CodeLens for: bootstrap and bounded architecture review; preflight before dispatching a builder; planner-session audit and handoff artifact production
- Avoid: defaulting to live bidirectional chat between planner and builder; exposing mutation-heavy surfaces to read-side sessions
- Routing defaults: `point_lookup=native-first`, `multi_file_review=codelens-after-first-local-step`, `builder_dispatch=planner-builder-handoff-required`, `long_running_eval=analysis-job-first`

### `codex`

- Best fit: builder and refactor execution, parallel worktree-based implementation, and automation
- Recommended CodeLens modes: `solo-local`, `planner-builder`, `batch-analysis`
- Preferred profiles: `builder-minimal`, `refactor-full`, `ci-audit`
- Native host primitives: `AGENTS.md`, `skills`, `worktrees`, `shared MCP config`, `CLI, app, and IDE continuity`
- Use CodeLens for: bounded mutation after verify_change_readiness; session-scoped builder audit; analysis jobs for CI-facing summaries
- Avoid: forcing CodeLens into trivial single-file lookups; copying Claude-specific subagent topology into Codex worktree flows
- Routing defaults: `point_lookup=native-first`, `multi_file_build=builder-minimal-after-bootstrap`, `rename_or_broad_refactor=refactor-full-after-preflight`, `ci_summary=analysis-job-first`

### `cursor`

- Best fit: editor-local iteration with scoped rules plus asynchronous remote execution when needed
- Recommended CodeLens modes: `solo-local`, `reviewer-gate`, `batch-analysis`
- Preferred profiles: `planner-readonly`, `reviewer-graph`, `ci-audit`
- Native host primitives: `.cursor/rules`, `AGENTS.md`, `custom modes`, `background agents`, `mcp.json`
- Use CodeLens for: architecture review and diff-aware signoff; analysis jobs for background-agent queues; minimal surface exposure through mode- or rule-specific routing
- Avoid: assuming foreground and background agents share the same trust boundary; shipping the full CodeLens surface into every mode
- Routing defaults: `foreground_lookup=native-first`, `foreground_review=codelens-after-first-local-step`, `background_queue=analysis-job-first`, `wide_surface=deferred-loading-required`

### `cline`

- Best fit: human-in-the-loop debugging and foreground execution with explicit approvals
- Recommended CodeLens modes: `solo-local`, `planner-builder`
- Preferred profiles: `builder-minimal`, `reviewer-graph`
- Native host primitives: `interactive permissioned terminal execution`, `browser loop`, `workspace checkpoints`, `MCP integrations`
- Use CodeLens for: review-heavy exploration before write passes; session audit and handoff artifacts when a change must cross sessions
- Avoid: treating Cline as a headless CI runner; relying on CodeLens where the foreground checkpoint loop already provides the needed safety
- Routing defaults: `foreground_debug=native-first-with-codelens-escalation`, `write_pass=builder-minimal-after-bootstrap`, `handoff=artifact-required`

### `windsurf`

- Best fit: editor-local implementation with a hard MCP tool cap and bounded foreground agent flows
- Recommended CodeLens modes: `solo-local`, `reviewer-gate`
- Preferred profiles: `builder-minimal`, `planner-readonly`
- Native host primitives: `global MCP config`, `foreground agent loop`, `workspace-local editing`, `100-tool cap across MCP servers`
- Use CodeLens for: bounded builder execution under a small visible surface; compressed planning when the task escapes single-file scope
- Avoid: attaching the full CodeLens surface alongside many other MCP servers; using reviewer-heavy profiles as the default editing surface
- Routing defaults: `foreground_lookup=native-first`, `multi_file_edit=builder-minimal-after-bootstrap`, `wide_surface=deferred-loading-required`, `tool_cap=keep-profile-bounded`
<!-- SURFACE_MANIFEST_HOST_ADAPTER_GUIDANCE:END -->

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
