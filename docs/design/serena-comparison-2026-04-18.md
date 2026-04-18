# Serena Comparison — Upper-Compatible Architecture Direction

Date: 2026-04-18

## Scope

This note compares the current CodeLens MCP architecture against the current
Serena codebase and extracts the parts worth adopting without regressing the
existing CodeLens harness model.

Local inspection basis:

- Serena repository cloned to `/tmp/serena-oraios`
- Inspected Serena commit: `37d40d6659fabc3b1a297ba21f28cd373e9502c1`
- Official upstream: `https://github.com/oraios/serena`
- MCP Registry snapshot on 2026-04-18 showed Serena at `23,098` stars

CodeLens comparison basis:

- `crates/codelens-mcp/src/surface_manifest.rs`
- `crates/codelens-mcp/src/tool_defs/presets.rs`
- `crates/codelens-mcp/src/dispatch/response.rs`
- `crates/codelens-mcp/src/tools/session/project_ops.rs`

## Executive Judgment

Serena is stronger than CodeLens in three areas:

1. declarative context and mode composition
2. pluggable semantic backend strategy
3. packaged user-facing operator experience

CodeLens is stronger than Serena in four areas:

1. role-scoped harness surfaces and bounded tool exposure
2. mutation preflight and verifier-gated editing discipline
3. session-scoped audit, export, and aggregate evaluation
4. host-adaptive delegation contracts and explicit planner-builder separation

Therefore, the correct direction is not "copy Serena" and not "ignore Serena".
The correct direction is:

- keep CodeLens as the substrate and harness contract owner
- absorb Serena's declarative composition model
- absorb Serena's backend abstraction model
- selectively absorb Serena's operator UX
- avoid Serena's monolithic "agent + prompt + tool + server + dashboard" coupling

## Serena Architecture Summary

## 1. Serena is an agent toolkit first, MCP server second

The core Serena server builds MCP tools out of agent-owned tool instances.
The main server object is effectively a factory around a long-lived
`SerenaAgent`, not just a thin transport layer.

Evidence:

- `src/serena/mcp.py`
- `src/serena/agent.py`
- `src/serena/tools/tools_base.py`

Implication:

- Serena couples tool exposure, prompt shaping, project activation, mode
  switching, and backend selection inside one Python runtime object graph.

## 2. Serena uses declarative contexts and modes as first-class configuration

Serena has:

- one active context
- multiple active modes
- tool inclusion / exclusion rules
- prompt fragments loaded from YAML

Contexts are host-specific and may exclude overlapping tools. Modes are
task-specific and can be combined dynamically.

Evidence:

- `src/serena/config/context_mode.py`
- `src/serena/resources/config/contexts/*.yml`
- `src/serena/resources/config/modes/*.yml`

Examples:

- `claude-code.yml` excludes file reads, shell, and primitive file tools
- `codex.yml` excludes overlapping shell and file tools
- `planning.yml` excludes editing tools
- `editing.yml` adds editing-specific guidance

Implication:

- Serena's best architectural idea is not any one tool.
- It is the compiler layer that turns host + task shape into a bounded toolset
  and prompt contract.

## 3. Serena has a real multi-backend semantic layer

Serena supports:

- LSP-backed analysis via `solidlsp`
- JetBrains-backed analysis/editing via a Serena plugin

This is exposed as a single `LanguageBackend` abstraction and selected by
configuration.

Evidence:

- `src/serena/config/serena_config.py`
- `src/serena/ls_manager.py`
- `src/serena/code_editor.py`
- `src/serena/tools/jetbrains_tools.py`

Implication:

- Serena separates semantic capability intent from the concrete engine better
  than CodeLens currently does.

## 4. Serena has explicit project activation and memory as product concepts

Projects are registered and activated. Memory is split into:

- project-local memories
- global memories

This gives Serena a persistent cross-session knowledge surface independent of
the host.

Evidence:

- `src/serena/tools/config_tools.py`
- `src/serena/project.py`
- `src/serena/config/serena_config.py`

Implication:

- Serena treats "long-lived agent memory per project" as product infrastructure,
  not only a host responsibility.

## 5. Serena includes an operator-facing dashboard

Serena ships:

- a dashboard
- GUI log viewing
- analytics objects
- client setup helpers

Evidence:

- `src/serena/dashboard.py`
- `src/serena/analytics.py`
- `src/serena/config/client_setup.py`
- `src/serena/tools/config_tools.py`

Implication:

- Serena is ahead on productization and operator visibility.

## CodeLens Architecture Summary

## 1. CodeLens is substrate-first

CodeLens already treats itself as durable shared infrastructure rather than as
a monolithic in-process agent object.

Evidence:

- `crates/codelens-mcp/src/surface_manifest.rs`

The manifest explicitly defines:

- shared substrate responsibilities
- host adapter resources
- delegation contract
- telemetry contract
- role flow and harness flow

## 2. CodeLens is stronger on role-scoped harnessing

CodeLens has explicit profiles such as:

- `planner-readonly`
- `builder-minimal`
- `reviewer-graph`
- `refactor-full`
- `ci-audit`

Evidence:

- `crates/codelens-mcp/src/tool_defs/presets.rs`

This is stronger than Serena's current general contexts/modes in one key way:

- CodeLens profiles are already aligned to multi-agent harness roles rather
  than only interaction styles.

## 3. CodeLens is stronger on mutation control and audit

CodeLens already has:

- verifier-first mutation gating
- builder/planner session audits
- per-session markdown export
- aggregate eval lane

Evidence:

- `crates/codelens-mcp/src/tools/session/project_ops.rs`
- `crates/codelens-mcp/src/surface_manifest.rs`

This is a stricter and more production-ready control plane than Serena's
"editing mode + optional read-only project" model.

## 4. CodeLens is stronger on cross-host delegation contracts

CodeLens has explicit synthetic delegation:

- `delegate_to_codex_builder`
- `handoff_id`
- replay rules
- cross-session telemetry correlation

Evidence:

- `crates/codelens-mcp/src/dispatch/response.rs`
- `crates/codelens-mcp/src/surface_manifest.rs`

Serena currently focuses more on "give the agent IDE-grade tools" than on
explicit planner-builder-reviewer orchestration.

## Delta Matrix

| Area | Serena | CodeLens | Judgment |
| --- | --- | --- | --- |
| Tool semantics | strong symbol-first tooling | strong symbol + harness workflow tooling | CodeLens should keep its workflow layer |
| Host adaptation | YAML contexts | host adapter manifest/resources | merge the two ideas |
| Task adaptation | composable modes | fixed role profiles | add overlays to CodeLens |
| Backend abstraction | LSP and JetBrains backends | Rust engine + some LSP surfaces, less formal backend abstraction | Serena is ahead |
| Memory | built-in global + project memory | session audit + repo memory patterns, less productized registry | Serena is ahead |
| Audit/eval | lighter analytics/dashboard | stronger session audit and eval lanes | CodeLens is ahead |
| Multi-agent orchestration | weaker explicit orchestration | strong planner-builder-reviewer contract | CodeLens is ahead |
| Product setup UX | client setup helpers and dashboard | attach/detach templates, but less operational GUI | mixed |

## What CodeLens Should Adopt

## Adopt 1. Context overlays on top of profiles

CodeLens should not replace profiles with Serena-style modes. It should add a
second, orthogonal layer:

- role profile answers "what lane is this agent in?"
- context overlay answers "what host/runtime envelope am I in?"
- optional task overlay answers "planning, editing, one-shot, onboarding?"

Recommended model:

- `profile`: planner-readonly / builder-minimal / reviewer-graph / refactor-full
- `host_context`: claude-code / codex / cursor / desktop / api-agent
- `task_overlay`: planning / editing / one-shot / onboarding / interactive

This would be strictly more expressive than both systems:

- more harness-native than Serena
- more composable than current CodeLens profiles alone

## Adopt 2. Formal backend adapter interface

CodeLens should introduce a backend abstraction roughly like:

- `semantic_backend`
  - `rust_index`
  - `lsp`
  - `scip`
  - future `ide_bridge`

Key rule:

- workflow and audit contracts stay above the backend line
- retrieval/edit operations compile down to backend capabilities

This would let CodeLens gain Serena's flexibility without inheriting Serena's
runtime coupling.

## Adopt 3. Managed project registry as optional operator feature

Serena's project activation model is useful, but CodeLens should keep it
optional.

Recommended addition:

- optional project registry
- optional global/project memory registry
- optional `activate_project` UX for multi-repo operators

Do not make this the default runtime assumption for harness use.

Reason:

- CodeLens is already strong in explicit project-path and session-scoped
  harnessing.
- forcing registry-first workflows would regress the current clean substrate
  model.

## Adopt 4. Operator plane / dashboard

A small operator-facing surface is worth adding:

- live sessions
- current profile per session
- audit failures and warnings
- delegate handoff correlation counts
- index health
- analysis job queue

This should be built as an operator plane over existing telemetry and resources,
not as a new orchestration core.

## What CodeLens Should Not Copy

## Avoid 1. Agent/server monolith

Do not collapse CodeLens into a single in-process "agent owns tools owns prompt
owns server" model.

Why:

- CodeLens's main advantage is substrate neutrality.
- Monolithic agent state would make host adaptation and evaluation harder.

## Avoid 2. Prompt-driven safety instead of runtime gates

Serena relies more heavily on:

- context prompts
- mode prompts
- tool inclusion/exclusion

CodeLens should keep runtime-enforced rules:

- mutation readiness
- profile surface gating
- deferred loading gates
- audit lanes

Prompts are useful, but they should remain compiled hints, not the final safety
boundary.

## Avoid 3. Overloading memory as the main orchestration bus

Serena's memory model is useful for continuity, but CodeLens already has a
better direction for orchestration:

- explicit handoff schema
- session audit
- exportable markdown and JSON artifacts
- runtime resources

Memory should support recall, not replace artifacts.

## Proposed Upper-Compatible Target Architecture

## Layer 1. Substrate kernel

Owns:

- session state
- tool execution
- mutation gate
- telemetry
- audit/eval lanes
- artifact export
- handoff schema

Already mostly present in CodeLens.

## Layer 2. Semantic backend adapters

Owns:

- symbol lookup
- references
- type hierarchy
- rename/move/edit primitives
- diagnostics integration

Backends:

- Rust engine
- LSP bridge
- SCIP bridge
- future IDE bridge

This is the main Serena pattern to adopt.

## Layer 3. Surface compiler

Compiles:

- profile
- host context
- task overlay
- backend capability map

Into:

- visible tool set
- tool descriptions
- suggested entrypoints
- host-native config templates
- runtime warnings

This merges Serena's contexts/modes with CodeLens's host adapter manifest.

## Layer 4. Host adapter contract

Owns:

- attach/detach templates
- host-native rules files
- synthetic delegation actions
- replay guarantees
- session closeout instructions

CodeLens is already ahead here. Keep this direction.

## Layer 5. Operator plane

Owns:

- dashboards
- daemon-wide aggregate audit snapshots
- queue views
- index health
- correlation visibility

This is the Serena productization pattern worth adopting.

## Priority Roadmap

## P1. Add context overlays without disturbing profiles

Deliver:

- `host_context` + `task_overlay` on top of current profiles
- surface compiler merges profile + overlay rules
- preserve current profile names and behavior by default

Expected benefit:

- Serena-grade configurability without losing CodeLens harness semantics

## P2. Introduce semantic backend abstraction

Deliver:

- explicit backend trait/interface
- map current Rust index and existing LSP/scip paths behind it
- capability reporting by backend

Expected benefit:

- future JetBrains or other IDE bridges become possible without redesigning the
  harness layer

## P3. Add optional managed project registry and memory registry

Deliver:

- optional project registration
- optional global/project memory namespace
- activation UX for operators using many repos

Expected benefit:

- absorbs Serena's strongest continuity feature without forcing it on all users

## P4. Add operator dashboard

Deliver:

- telemetry and audit viewer
- active session summary
- analysis queue + health
- delegate handoff correlation monitor

Expected benefit:

- absorbs Serena's strongest product UX feature

## Bottom Line

Serena is not a better version of CodeLens. It is a better-configured symbolic
agent toolkit with stronger backend abstraction and better product packaging.

CodeLens is not a worse version of Serena. It is a stronger harness substrate
with better role separation, safer mutation control, and a more explicit audit
and delegation model.

The winning direction is:

- **Serena's composition model**
- plus **CodeLens's harness contract**
- under **CodeLens's stricter runtime gates**

That combination would produce a genuine upper-compatible architecture rather
than a rename or a feature copy.
