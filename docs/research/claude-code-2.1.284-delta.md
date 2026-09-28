# Claude Code 2.1.277–2.1.284 delta for CodeLens (2026-09-29)

Source: `https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md`
(read 2026-09-29; the file lists 2.1.277, 2.1.278, 2.1.280–2.1.284 above the
2.1.273 audit in `landscape-deep-research-2026-09-16.md`; 2.1.274–276 and
2.1.279 are not in it). Installed locally: 2.1.284. Only items that touch an
MCP server or a plugin are listed.

| Version | Change (quoted) | Implication for CodeLens |
|---|---|---|
| 2.1.284 | "Fixed MCP tool calls in a resumed session failing with 'No such tool available' while their server was still connecting; the call now waits up to 10 seconds for the server" | None required; a slow daemon start inside 10 s no longer loses the first call. |
| 2.1.284 | "Added `/mcp reconnect all` …" | Operator recovery after a daemon redeploy; worth naming in `docs/operations/http-daemon.md`. |
| 2.1.283 | "Fixed MCP progress notifications being discarded once a long-running tool call moved to the background" | CodeLens emits no `notifications/progress` today. The slow tools (`review(mode=changes)`, first `graph(mode=callers)` on a large repo, first bind) could report progress now that the host keeps it. Backlog. |
| 2.1.283 | "Fixed stdio MCP servers being left running when the session ended while they were still starting" | Affects the stdio install path only. |
| 2.1.282 | "Changed MCP resource lists … to skip MCP Apps UI resources" | None; CodeLens serves no UI resources. |
| 2.1.281 | "Fixed mcp_tool hooks on blocking events … being skipped while their MCP server was still connecting" | Hooks keyed on CodeLens tools now run during a slow connect. |
| 2.1.281 | "Fixed the same MCP server being connected twice when a plugin or claude.ai connector and a configured server spell its URL differently" | The plugin manifest starts stdio `codelens-mcp` while user config points at HTTP `:7838`; these are different servers, not a double connection. Not installed as a plugin on this machine. |
| 2.1.277 | "Added AGENTS.md support: in a project with no CLAUDE.md, Claude Code reads AGENTS.md instead" | This repo ships both; the Codex twin `AGENTS.md` is now also the Claude fallback for repos that run `attach codex` only. |

Follow-ups: progress notifications for long tools (above); mention
`/mcp reconnect all` in the redeploy runbook.
