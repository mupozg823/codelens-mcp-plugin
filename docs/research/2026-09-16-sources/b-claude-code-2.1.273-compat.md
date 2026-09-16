# Claude Code 2.1.200→2.1.273 Compatibility Audit — CodeLens MCP Plugin Repo

Repo audited: `/Users/bagjaeseog/codelens-mcp-plugin`. Installed Claude Code: v2.1.273 (2026-09-16).
CHANGELOG source fetched raw (full 6849-line file, not summarized) from
`https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md`; the newest entry in
that file is `## 2.1.273` — no version newer than the installed one was published as of fetch time.

## 1) Executive summary

**One real, confirmed-broken finding, verified live against the installed 2.1.273 CLI, not just
docs inference**: `.claude-plugin/plugin.json`'s `"agents": "./agents/"` field **fails validation**
(`claude plugin validate .claude-plugin/plugin.json --strict` → `✘ agents: Invalid input`). Unlike
`skills`, which does accept a bare directory string, `agents` requires an explicit array of `.md`
file paths (confirmed by reproducing the failure and the fix in a scratch copy: `"agents":
["./agents/codelens-explorer.md"]` passes; `"agents": ["./agents/"]` still fails with
`agents.0: Invalid input`). This contradicts the third-party-summarized docs snippet that described
`agents` as accepting `string|array` — the live validator is the ground truth here, not the docs
summary.

**Task item (a) — the `_meta` always-load key — resolves compatible, confirmed against the raw
doc HTML (not a WebFetch summary).** Claude Code documents two distinct `alwaysLoad` mechanisms on
`/docs/en/mcp`: a server-level `.mcp.json` boolean, and — separately — "An MCP server can also
mark individual tools as always-loaded by including `"anthropic/alwaysLoad": true` in the tool's
`_meta` object, which has the same effect for that tool only." CodeLens's own
`crates/codelens-mcp/src/server/tools_list.rs:94-95` does exactly this:
`if tool_anthropic_always_load(tool.name) { meta["anthropic/alwaysLoad"] = Value::Bool(true); }`
— the key name, casing, and value type (`_meta["anthropic/alwaysLoad"]: bool`) are an **exact
match** to the current 2.1.273 contract. CodeLens separately uses `anthropic/maxResultSizeChars`
(`dispatch/response_support/envelope.rs:22`), also an exact match to the documented key. Both
`_meta` keys CodeLens emits are correct against the live doc, not just plausible-looking.

**Task item (a) sub-question — agent frontmatter array syntax for `tools:`/`disallowedTools:` —
also resolves compatible**, again verified live: `claude plugin validate agents --strict` on this
repo's `agents/` directory returns `✔ Validation passed`, confirming Claude Code 2.1.273 accepts
the YAML flow-array form used in `agents/codelens-explorer.md` even though the docs' examples only
show the comma-separated scalar form.

Beyond the one confirmed bug, the remaining issues are two missing-capability gaps (no
`fork`-source handling in the session probe hook; no explicit `host_capabilities` facts for Claude
Code even though nearly every capability flag CodeLens tracks has a confirmed native Claude Code
counterpart), one CLAUDE.md content gap (no mention of Claude Code's native `LSP` tool, which
structurally overlaps with CodeLens `diagnose`/`search(mode=defn|refs)`), and a handful of
dead/inert (but harmless — confirmed not even flagged under `--strict`) frontmatter fields in the
top-level skills. `.agents/plugins/marketplace.json` is **not a Claude Code file at all** — it is
consumed only by `scripts/validate-codex-plugin-manifest.py` for the Codex-plugin ecosystem, so it
is out of scope for this audit and should not be evaluated against Claude Code's marketplace
schema.

## 2) Changelog delta table (2.1.200 → 2.1.273)

CHANGELOG.md publishes no per-version dates, so the Date column is N/A throughout. Only entries
relevant to this repo's integration surface are listed; the full raw range is saved locally at
`/Users/bagjaeseog/.claude/jobs/2edd14dd/tmp/changelog-200-273.md` (2145 lines) for follow-up.

| Version | Date | Change | Impact on CodeLens |
|---|---|---|---|
| 2.1.273 | N/A | `alwaysLoad`-eligible remote MCP servers keep improving mid-connect usability; `_meta`/tool-search fixes continue | No action; CodeLens's own `_meta` usage (`anthropic/maxResultSizeChars`, `codelens/executionPolicy`) is unaffected |
| 2.1.271 | N/A | `alwaysLoad` MCP server "usable on the next turn without a tool-search round trip" (Foundry/AWS); tool search returns no match on bare tool name fixed | Confirms `alwaysLoad` is a real, maturing client-side `.mcp.json` field; CodeLens's `.mcp.json` template doesn't set it (P2) |
| 2.1.271 | N/A | Added `omitClaudeMd` to agent frontmatter | Not used by `agents/codelens-explorer.md`; no action needed, but available if a future read-only agent should skip CLAUDE.md |
| 2.1.269 | N/A | `alwaysLoad` MCP server usable mid-conversation without tool-search round trip (first-party sessions) | Same as above |
| 2.1.268 | N/A | `managedMcpServers` managed setting added; Bedrock/Vertex/Foundry sessions keep tool list byte-stable, late tools "load deferred instead of rewriting it" | Confirms deferred-tool mechanics are entirely host-side; CodeLens's own `deferredToolLoading` init flag is a parallel, server-side concept the host must opt into explicitly (see §3) |
| 2.1.267 | N/A | Mid-session MCP/plugin tools "now receive them as deferred definitions"; tool search matches by bare `mcp__server__tool` name | No CodeLens action; naming convention (`mcp__codelens__search` etc.) already matches the `mcp__server__tool` pattern Claude Code's tool search expects |
| 2.1.265 | N/A | MCP OAuth client-registration fixes; `http` servers that only speak legacy SSE now fall back automatically | `.mcp.json` here uses `"type": "http"` with no OAuth block — unaffected |
| 2.1.261 | N/A | VS Code MCP servers dialog gets Add/Remove UI | No action |
| 2.1.259 | N/A | `managedMcpServers` added (same shape as `.mcp.json`) | No action for this repo (not managed-settings distributed) |
| 2.1.257 | N/A | MCP OAuth/WebSocket error-message hardening | No action |
| 2.1.251 | N/A | Added `PreModelSwitch`/`PostModelSwitch` hook events; `SessionStart` resume hooks now receive staleness/re-cache cost | Repo has no `PreModelSwitch`/`PostModelSwitch` hooks; not required, but available (P2 idea, not pursued) |
| 2.1.246 | N/A | Hook error messages show resolved `${CLAUDE_PLUGIN_ROOT}` instead of literal string | No action; `hooks/codelens-first.py` command line already uses `"${CLAUDE_PLUGIN_ROOT}"` correctly |
| 2.1.238 | N/A | Skills with `context: fork` now background by default; MCP `headersHelper` sandboxing tightened | No CodeLens skill uses `context: fork` — no action |
| 2.1.233 | N/A | `claude plugin validate` checks bare `.claude/skills`, reports SKILL.md frontmatter parse failures | Repo's `skills/*/SKILL.md` frontmatter parses (YAML valid) but uses non-schema keys `trigger`/`tools` — see §4/§5 |
| 2.1.229 | N/A | Plugin marketplace `command` sources added (`mode: "link"`) | Not used; `.claude-plugin/marketplace.json`'s `"source": "./"` (string form) remains valid |
| 2.1.222 | N/A | `disable-model-invocation` refusal message improved | No repo skill sets `disable-model-invocation` |
| 2.1.221 | N/A | Plugins accept `"."` as a `skills` path | Not applicable; repo uses `"./skills/"` |
| 2.1.218 | N/A | Boolean skill/agent frontmatter fields accept `yes/no/on/off/1/0` in addition to `true/false` (requires v2.1.218+) | No boolean-typed keys are set in repo skill/agent frontmatter today; informational only |
| 2.1.217 | N/A | Brace-expansion in `SKILL.md`/`CLAUDE.md` `paths` frontmatter is now budget-bounded (was an OOM/stall risk) | No repo skill sets `paths:`; no exposure |
| 2.1.214 | N/A | **`SessionStart` hooks changed to report source `"fork"`** instead of `"resume"` when a session begins as a fork; also list_changed refresh failures stopped wiping the tool list on transient errors | `hooks/codelens-session-probe.sh` only special-cases `"source":"resume"`; forked sessions now report a **distinct** `"fork"` value the script does not check (see §5, P1) |
| 2.1.212 | N/A | MCP tool calls over 2 minutes auto-background (`CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS`) | Long-running CodeLens jobs (`start_analysis_job`) already use an async job pattern instead of a blocking call, so this is aligned, not a gap |
| 2.1.208 | N/A | LSP documents capped at 50-doc LRU; MCP stdio stderr capped at 64MB; async hook output GC'd | No action (infra-level fix on Claude Code's side) |
| 2.1.207 | N/A | `${user_config.*}` in shell-form plugin hook/monitor/`headersHelper` commands rejected (shell-injection fix); must use exec-form `args` array or `$CLAUDE_PLUGIN_OPTION_<KEY>` | `hooks/optional/codelens-first.hooks.json` already uses a plain `command` string with no `${user_config.*}` interpolation — unaffected |
| 2.1.203 | N/A | MCP `roots/list` now includes session's additional working directories, with `notifications/roots/list_changed` | No action; CodeLens doesn't currently branch on `roots/list` |
| 2.1.203 | N/A | Fixed LSP-only plugins being flagged for disuse when only diagnostics/navigation used | Direct evidence the native `LSP` tool is a first-class, actively maintained capability (see §3 LSP overlap) |
| 2.1.200 | N/A | (session-start baseline for this range) | — |

## 3) Contract facts with citations

**MCP / tool search / deferred loading**
- `.mcp.json` server-level field `alwaysLoad` (boolean): "the server is remote. A server with `alwaysLoad: true` connects at startup, and one without is deferred to on-demand." Applies to HTTP and SSE servers. [Source: https://code.claude.com/docs/en/mcp]
- Claude Code recognizes at least two MCP-tool-level `_meta` keys: `anthropic/maxResultSizeChars` (number, max 500,000, raises that tool's persist-to-disk truncation threshold) and **`anthropic/alwaysLoad`** (boolean). Exact quote from the doc's raw HTML (this text did not survive an initial WebFetch summarization pass and required a direct `curl` + grep to recover — the page is long enough that the summarizer truncated it): "The `alwaysLoad` field is available on all server types. An MCP server can also mark individual tools as always-loaded by including `"anthropic/alwaysLoad": true` in the tool's `_meta` object, which has the same effect for that tool only." Setting the server-level `alwaysLoad: true` additionally "makes startup wait for the server's tools, capped at the standard 5-second connect timeout." [Source: https://code.claude.com/docs/en/mcp, section "Exempt a server from deferral"]
- CodeLens sets exactly this per-tool key: `crates/codelens-mcp/src/server/tools_list.rs:94-95` — `if tool_anthropic_always_load(tool.name) { meta["anthropic/alwaysLoad"] = Value::Bool(true); }`. Key name, casing, and boolean type are an exact match to the current contract; this is the direct, confirmed answer to audit-request item (a).
- `MAX_MCP_OUTPUT_TOKENS` (env var): default 25,000 tokens; warning at 10,000 tokens (fixed). [Source: https://code.claude.com/docs/en/mcp]
- `MCP_TOOL_TIMEOUT` (env var, ms): per-tool wall-clock timeout, default ~28 hours unset; a per-server `.mcp.json` `timeout` field overrides it for that server. [Source: https://code.claude.com/docs/en/mcp]
- `MCP_TIMEOUT` (env var, ms): server startup/connect timeout. [Source: https://code.claude.com/docs/en/mcp]
- `ENABLE_TOOL_SEARCH=true` is documented on the env-vars page specifically for non-first-party `ANTHROPIC_BASE_URL` proxies: "When set to a non-first-party host, MCP tool search is disabled by default. Set `ENABLE_TOOL_SEARCH=true` if your proxy forwards `tool_reference` blocks." [Source: https://code.claude.com/docs/en/env-vars] `MAX_MCP_OUTPUT_TOKENS`, `MCP_TOOL_TIMEOUT`, `MCP_TIMEOUT` are documented on the MCP page but were **not found** on the env-vars page itself when searched directly — UNVERIFIED whether env-vars.md also lists them (they may live only on the /mcp page).
- `list_changed` notifications: Claude Code "automatically refreshes the available capabilities from that server" and, since v2.1.214, "keeps the server's previously discovered tools... until a later refresh succeeds" instead of wiping them on a transient error. [Source: https://code.claude.com/docs/en/mcp] — matches CHANGELOG 2.1.214.
- `initialize` capabilities and `hostCapabilities`/`host_capabilities`: CodeLens's own `HostCapabilities` struct (`native_tool_search`, `native_subagents`, `nested_subagents`, `native_worktrees`, `native_edit`, `mcp_tasks`, `dynamic_tool_list`, `workspace_binding`, `approval_or_elicitation`) is populated either from a `prepare_harness_session` tool-call argument or from an HTTP `initialize` params field named `hostCapabilities`/`host_capabilities` (`crates/codelens-mcp/src/host_capabilities.rs:20-30`). No evidence Claude Code itself sends a custom `hostCapabilities` object in its native MCP `initialize` request — this is a CodeLens-defined extension point that only activates if the calling agent/host explicitly reports it (e.g., via the `prepare_harness_session(host_capabilities=...)` argument, which the routing text in CLAUDE.md only gestures at generically). UNVERIFIED: whether any current host actually sends `hostCapabilities` at the JSON-RPC `initialize` layer.

**Hooks**
- Full current hook-event list (32 events) includes `PreModelSwitch`, `PostModelSwitch`, `DirectoryAdded`, `WorktreeCreate`/`WorktreeRemove`, `PreCompact`/`PostCompact`, `Elicitation`/`ElicitationResult`, none of which existed as concepts the repo's hooks reference by name. [Source: https://code.claude.com/docs/en/hooks]
- `SessionStart` `source` field has exactly five values: `"startup"`, `"resume"`, `"clear"`, `"compact"`, `"fork"`. [Source: https://code.claude.com/docs/en/hooks and https://code.claude.com/docs/en/hooks-guide, line: "`SessionStart` hooks get a `source` of `startup`, `resume`, `clear`, `compact`, or `fork`"]
- Hook JSON output contract fields confirmed current: `hookSpecificOutput`, `hookEventName`, `systemMessage`, `terminalSequence`, and inside `hookSpecificOutput`: `permissionDecision` (`allow`/`deny`/`block`), `permissionDecisionReason`, `additionalContext`, `updatedInput`, `continue`, `stopReason`. [Source: https://code.claude.com/docs/en/hooks] — `hooks/codelens-first.py`'s documented contract (`permissionDecision`, `permissionDecisionReason`, `additionalContext`, exit 0 always) matches this exactly.
- Exit-code semantics: exit 0 = read JSON for decisions, exit 2 = blocking error (only where the event supports blocking), other = non-blocking error, action proceeds. [Source: https://code.claude.com/docs/en/hooks] — matches `hooks/codelens-first.py`'s documented "exit: always 0 (JSON on stdout carries the decision; fail-open by design)."

**Plugins / marketplaces**
- `.claude-plugin/plugin.json` valid top-level fields: `name`, `displayName`, `version`, `description`, `author{name,email,url}`, `homepage`, `repository`, `license`, `keywords`, plus component paths `skills`, `commands`, `agents`, `hooks`, `mcpServers`, `lspServers`, each documented as `string|array|object`. [Source: https://code.claude.com/docs/en/plugins-reference] **This documented typing for `agents` is imprecise** — verified live against the installed 2.1.273 CLI (`claude plugin validate <path> --strict`) in a scratch copy of this repo's `.claude-plugin/plugin.json`: `"agents": "./agents/"` (directory string) → `✘ agents: Invalid input`; `"agents": ["./agents/"]` (array containing a directory) → `✘ agents.0: Invalid input`; `"agents": ["./agents/codelens-explorer.md"]` (array of explicit `.md` file paths) → `✔ Validation passed`. `skills: "./skills/"` (bare directory string, unchanged) validates fine in the same file, so the directory-string convenience applies to `skills` only, not `agents`. [Ground truth: `claude plugin validate`, installed Claude Code 2.1.273, run 2026-09-16]
- Separately, `claude plugin validate agents --strict` on this repo's real `agents/` directory (containing `agents/codelens-explorer.md` with YAML flow-array `tools:`/`disallowedTools:`) returns `✔ Validation passed`, confirming the array syntax there is accepted by the current CLI even though the sub-agents doc's examples only show the scalar comma-separated form. [Ground truth: `claude plugin validate`, installed Claude Code 2.1.273, run 2026-09-16]
- `.claude-plugin/marketplace.json` **requires** a top-level `owner` object; each `plugins[]` entry's `source` field accepts either a plain string path or an object with a `source` discriminator whose valid values are `github`, `url`, `git-subdir`, `npm`, `archive`, `command` — **`"local"` is not in this enumerated list.** [Source: https://code.claude.com/docs/en/plugin-marketplaces]
- Plugin skill invocation naming: for personal/project skills the directory name drives the slash command; **for plugin skills, "the `name` field controls the command segment after the plugin prefix."** [Source: https://code.claude.com/docs/en/skills] Bare-name invocation of a plugin skill works only "when a bare name matches exactly one plugin skill" (CHANGELOG 2.1.269).
- Unknown/unrecognized SKILL.md frontmatter keys are **not stated to fail validation inside Claude Code itself** — only external distribution (claude.ai upload / Skills API packaging) hard-fails on unrecognized keys with an explicit allow-list error. [Source: https://code.claude.com/docs/en/skills]

**Sub-agent frontmatter**
- Full current field list: `name`, `description`, `tools` (type: **comma-separated list**), `disallowedTools` (type: **comma-separated list**), `model`, `permissionMode`, `maxTurns`, `skills` (type: **list** — YAML array is the documented type here, unlike `tools`), `mcpServers`, `hooks`, `memory`, `background`, `omitClaudeMd`, `effort`, `isolation` (`worktree` only), `color`, `initialPrompt`, `experimental.cacheTtl`. [Source: https://code.claude.com/docs/en/sub-agents]
- Every documented example of `tools:`/`disallowedTools:` uses a bare comma-separated scalar string (`tools: Read, Glob, Grep`), never a YAML block/flow array. No example anywhere shows `tools: [a, b, c]` or a `tools:\n  - a\n  - b` block form. [Source: https://code.claude.com/docs/en/sub-agents] This is contrasted with `skills:`, whose own documented example is a genuine YAML list.

**Skills frontmatter**
- Full current field list: `name`, `description`, `when_to_use`, `argument-hint`, `arguments`, `disable-model-invocation`, `user-invocable`, `allowed-tools`, `disallowed-tools`, `model`, `effort`, `context` (`fork` only), `agent`, `background`, `hooks`, `paths`, `shell`, `metadata`, `license`, `compatibility`. **`trigger` and `tools` (unhyphenated) do not exist anywhere in this schema.** [Source: https://code.claude.com/docs/en/skills]

**Native LSP tool (overlap with CodeLens)**
- Claude Code ships a native tool literally named `LSP`: "Code intelligence via language servers: jump to definitions, find references, report type errors and warnings." [Source: https://code.claude.com/docs/en/tools-reference]
- It is off by default; enabling it requires installing a per-language "code intelligence plugin" (e.g. `/plugin install typescript-lsp@claude-plugins-official`) plus the language server binary on the developer's machine; it does not function in cloud sessions ("In cloud sessions, Claude Code doesn't start plugin language servers, so Claude doesn't get the LSP tool there"). [Source: https://code.claude.com/docs/en/large-codebases]
- Official guidance already frames this as a grep/read-reduction technique: "Code intelligence plugins connect Claude to a language server so it can jump to definitions, find references, and surface type errors directly instead of scanning the tree." [Source: https://code.claude.com/docs/en/large-codebases]

## 4) Repo file audit table

| File | Status | Field / line | Recommended change | Citation |
|---|---|---|---|---|
| `.mcp.json` | compatible | `type:"http"`, `url`, `headers.x-codelens-project` | None required. Optional: add `"alwaysLoad": true` to skip the tool-search round trip for this always-used dev daemon | §3 alwaysLoad fact |
| `.claude-plugin/plugin.json` | **broken** | `"agents": "./agents/"` (line: `"agents": "./agents/"`) | Change to `"agents": ["./agents/codelens-explorer.md"]` (array of explicit `.md` file paths). Verified live: current form fails `claude plugin validate .claude-plugin/plugin.json --strict` with `✘ agents: Invalid input`; the array-of-files form passes. All other fields (`name`, `displayName`, `version`, `author`, `homepage`, `repository`, `license`, `keywords`, `mcpServers`, `skills`) are compatible | §3 live-validated agents field fact |
| `.claude-plugin/marketplace.json` | compatible | `name`, `owner`, `plugins[0].source:"./"` | None. `owner` is present (required) and `source` uses the valid plain-string form | §3 marketplace.json schema |
| `.agents/plugins/marketplace.json` | **N/A — not a Claude Code artifact** | whole file | No change needed for Claude Code compat; it is read only by `scripts/validate-codex-plugin-manifest.py` for the Codex plugin ecosystem. Do not audit it against `.claude-plugin/marketplace.json`'s schema (different host, different schema; e.g. its `"source":{"source":"local",...}` uses a discriminator value that isn't valid in Claude Code's marketplace schema, but that's expected since it isn't one) | grep evidence: `scripts/validate-codex-plugin-manifest.py:14`, `scripts/test/codex_plugin_test_support.py:17` |
| `hooks/codelens-first.py` | compatible | JSON output contract (`permissionDecision`, `permissionDecisionReason`, `additionalContext`), exit-code-0-always | None required for the contract shape. Confirm `strict` mode's `deny` still maps correctly since `deny` remains a valid `permissionDecision` value | §3 hook output contract |
| `hooks/optional/codelens-first.hooks.json` | compatible | `matcher:"Grep"`/`"Bash"`, `hooks[].command`, `timeout:5` | None. No `${user_config.*}` shell-injection exposure (2.1.207 fix N/A here) | §3 plugin hook shell-injection note |
| `hooks/codelens-session-probe.sh` | **missing new capability** | `case "$HOOK_INPUT" in *'"source"'*'"resume"'*) exit 0 ;; esac` (only line handling `source`) | Add a `fork` case alongside `resume`: since 2.1.214, a forked session reports `"source":"fork"`, a value distinct from `"resume"` that this script never checks. If the recommended settings-level matcher (`startup|clear|compact`) is used verbatim, `fork` sessions never invoke this hook at all and no duplicate-injection risk exists; but the script's own comment calls the internal `resume` check "이중 방어" (defense-in-depth) for callers who register a broader matcher — that defense is incomplete without a matching `fork` branch | CHANGELOG 2.1.214 + §3 SessionStart source enum |
| `hooks/README.md` | compatible (documentation) | recommends matcher `"startup|clear|compact"` | Consider documenting the `fork` source value explicitly next to `resume` so a future broader-matcher registration doesn't silently miss it | §3 SessionStart source enum |
| `hooks/post-edit-diagnostics.sh`, `hooks/clang-linker.sh` | compatible (unregistered, opt-in only) | positional `$1` arg, not stdin JSON | Already documented in `hooks/README.md` as predating the stdin-JSON contract; no fix needed since neither ships a `hooks/optional/*.hooks.json` fragment | §3 hook input contract |
| `agents/codelens-explorer.md` | **compatible (verified live)** | `tools:` and `disallowedTools:` use YAML flow-sequence arrays (`tools:\n  [\n    mcp__codelens__...,\n    ...\n  ]`) | None required. The sub-agents doc's examples only show the comma-separated scalar form for `tools`/`disallowedTools`, which raised a real question — resolved by running `claude plugin validate agents --strict` against this repo's actual `agents/` directory, which returned `✔ Validation passed` on the installed 2.1.273 CLI. The array form is accepted in practice even though undocumented in examples | §3 live-validated tools/disallowedTools fact |
| `skills/analyze/SKILL.md` | uses non-schema fields (inert, not breaking) | frontmatter `trigger: "/codelens-analyze"`, `tools: [review, graph]` | `trigger` and `tools` (unhyphenated) do not exist in the current SKILL.md schema. Confirmed harmless, not just docs-inferred: `claude plugin validate skills --strict` on this repo's real `skills/` directory returns `✔ Validation passed` with no warning about either key, even under `--strict`. Because this is a **plugin** skill (reached via `plugin.json`'s `"skills":"./skills/"`), the invocable command is determined by design from the frontmatter `name:` field (`codelens-analyze`) — that is the documented mechanism for plugin skills, not a coincidence — so the `/codelens-analyze` usage text is correct today. `trigger`/`tools` still do nothing functionally and should be replaced with `allowed-tools:` if tool-restriction was the actual goal, or dropped as dead metadata | §3 SKILL.md schema + plugin skill naming, live-validated |
| `skills/code-review/SKILL.md` | same as above | `trigger: "/codelens-review"`, `tools: [get_changed_files, graph, ...]` | Same recommendation | same |
| `skills/onboard/SKILL.md` | same as above | `trigger: "/codelens-onboard"`, `tools: [activate_project, onboard_project, overview, get_ranked_context]` | Same recommendation | same |
| `plugins/codelens/skills/codelens/SKILL.md` | N/A — Codex-only | frontmatter `name`, `description` only (no `trigger`/`tools`) | Out of scope: sibling `.codex-plugin/plugin.json` and `agents/openai.yaml` confirm this bundle targets Codex, not Claude Code. Already schema-minimal (no non-standard keys), so no action even if it were in scope | directory evidence |
| `crates/codelens-mcp/src/server/tools_list.rs:94-95` + `tool_defs::tool_anthropic_always_load` (found via `rg -n 'always_load\|alwaysLoad\|tool_anthropic\|defer' crates/codelens-mcp/src` per the task's own suggested search) | **compatible — exact key match, confirmed against live doc HTML** | `meta["anthropic/alwaysLoad"] = Value::Bool(true)` | None. This is the correct, current, documented per-tool always-load mechanism (§3). No change needed | §3 alwaysLoad per-tool fact |
| `docs/platform-setup.md` | compatible, but describes a CodeLens-only extension as if broadly interoperable | "`prepare_harness_session` also accepts optional `host_context` and `task_overlay` hints" / "opt in during `initialize` with `{"deferredToolLoading": true}`" | Clarify that `deferredToolLoading` and `hostCapabilities` are CodeLens-side opt-in extensions that only take effect if the **calling agent or host explicitly sends them** (via a tool-call argument or a custom `initialize` param) — Claude Code does not autonomously send either today; its native ToolSearch operates independently of what a server's `tools/list` already trimmed | §3 host_capabilities fact |
| `CLAUDE.md` `CODELENS_HOST_ROUTING` block / `crates/codelens-mcp/src/surface_manifest/host_adapters/templates/claude_code.rs` | missing new capability | `bundle()` has no `host_capabilities` literal and the routing prose never mentions the native `LSP` tool | (1) Add a concrete `host_capabilities` example for Claude Code (`native_tool_search: true`, `mcp_tasks: true`, `native_worktrees: true`, `native_edit: true`, `approval_or_elicitation: true` — all confirmed native features per §3) so `prepare_harness_session` calls stop defaulting every flag to `false`. (2) Add one routing line distinguishing the native `LSP` tool (single-language, needs a locally installed language server, off by default, no cloud-session support) from CodeLens (cross-file architecture/impact/bounded-context, no local LSP install required) | §3 tools-reference LSP + host_capabilities |

## 5) Recommended changes, ranked

**P0 — confirmed broken, fix now**
1. `.claude-plugin/plugin.json`: change `"agents": "./agents/"` to `"agents": ["./agents/codelens-explorer.md"]`. Live-verified on the installed 2.1.273 CLI: the current directory-string form fails `claude plugin validate .claude-plugin/plugin.json --strict` with `✘ agents: Invalid input`; the array-of-explicit-file-paths form passes cleanly in a scratch reproduction. This is a read-only audit finding, not applied — the one-line fix above is what should land. Re-run `claude plugin validate .claude-plugin/plugin.json --strict` after editing to confirm `✔ Validation passed`.

**P1 — missing capability, cheap to add**
1. `hooks/codelens-session-probe.sh`: add a `fork` branch next to the existing `resume` branch in the `case "$HOOK_INPUT" in ...` guard, since `fork` is a distinct, documented `SessionStart` source value since v2.1.214 that the script's own "defense-in-depth" comment implies it should also cover.
2. `crates/codelens-mcp/src/surface_manifest/host_adapters/templates/claude_code.rs` (and the synced `CODELENS_HOST_ROUTING` block in `CLAUDE.md`/`AGENTS.md`): add a concrete Claude Code `host_capabilities` literal (native_tool_search, mcp_tasks, native_worktrees, native_edit, approval_or_elicitation all `true`) so agents following the routing block report accurate facts to `prepare_harness_session` instead of leaving every flag at the struct's `false` default.
3. Same template/CLAUDE.md block: add one sentence on the native `LSP` tool and when to prefer it over CodeLens (single-file/type-precise navigation with an installed language server) versus when to prefer CodeLens (cross-file impact/architecture, or when no language server is installed/available, e.g. cloud sessions).

**P2 — nice, low risk**
1. Drop or rename the dead `trigger`/`tools` frontmatter keys in `skills/analyze/SKILL.md`, `skills/code-review/SKILL.md`, `skills/onboard/SKILL.md` to `allowed-tools:` (hyphenated) if tool-restriction was the intent, since the current keys are schema-unrecognized and confirmed to do nothing (not even a `--strict` warning) inside Claude Code today.
2. Conditionally add `"alwaysLoad": true` to the `codelens` entry in `.mcp.json` to skip the tool-search round trip, **but only once the port drift is resolved first**: `.mcp.json` here points at `127.0.0.1:7736` (the dev daemon), while `docs/platform-setup.md` and `templates/claude_code.rs` both describe `7838` as canonical, and the separate root `mcp.json` describes `7837`. `alwaysLoad: true` makes Claude Code connect **at session startup**; on a port nothing is actually listening on, that turns today's lazy no-op into a startup connection failure (and, per the 2.1.273 changelog, an explicit "MCP server disconnects... reconnection gives up" notice). Fix the three-way port divergence first, or scope `alwaysLoad` to whichever `.mcp.json` genuinely matches the running daemon.
3. `docs/platform-setup.md`: add one clarifying sentence that `deferredToolLoading`/`hostCapabilities` initialize-time opt-ins require explicit host cooperation and are not something Claude Code sends unprompted.

## 6) UNVERIFIED items

- Whether `MAX_MCP_OUTPUT_TOKENS`, `MCP_TOOL_TIMEOUT`, `MCP_TIMEOUT`, `CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT`, `MCP_DISCOVERY_CACHE`, `MCP_PROTOCOL_NEGOTIATION`, `MCP_SDK_GENERATION` are also documented on `/docs/en/env-vars` (only confirmed present on `/docs/en/mcp`; a direct env-vars page search found only `ENABLE_TOOL_SEARCH`).
- Whether any current Claude Code build sends a `hostCapabilities`/`host_capabilities` object in its native MCP `initialize` request params (no changelog or doc evidence found either way; CodeLens's `from_initialize_params` code path exists but may never be exercised by a real Claude Code session — only the `prepare_harness_session` tool-argument path is confirmed reachable).
- `/docs/en/plugins`, `/docs/en/settings` (only partially fetched — precedence table, not the full settings-key reference), `/docs/en/memory`, `/docs/en/costs` were not deeply fetched in this pass; nothing in the repo's audited files depended on those specific pages beyond what `/docs/en/mcp`, `/docs/en/large-codebases`, and `/docs/en/skills`/`/docs/en/sub-agents` quotes already covered, and the two open questions those pages' examples raised (agent-frontmatter array syntax, plugin.json `agents` field shape) were both resolved by running the live `claude plugin validate` CLI directly (§3, §4) rather than left as documentation-only inference.
- Two remaining low-value items **not** re-run for time: whether `claude plugin validate` on the marketplace path also silently accepts `.agents/plugins/marketplace.json`'s non-Claude-Code shape if pointed at it directly (irrelevant since that file is a Codex artifact, not loaded by Claude Code at all); and whether `claude plugin eval` (added 2.1.269) would surface anything additional for this plugin — not run, out of scope for a compatibility audit.
