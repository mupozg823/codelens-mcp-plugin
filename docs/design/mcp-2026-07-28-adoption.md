# Adopting MCP 2026-07-28

Status: **not adopted**. The HTTP daemon and stdio server still speak
2025-11-25 / 2025-06-18 / 2025-03-26, and two tests pin the behaviour that
keeps today's dual-era clients working (see [Guardrails](#guardrails)).
Turning 2026-07-28 on waits for a decision on how a project binding survives
without sessions ([Options](#binding-options)).

Measured 2026-10-10 against Claude Code 2.1.296 and the published
specification (`modelcontextprotocol.io/specification/2026-07-28`).

## What the new version changes for this server

- `initialize`, `notifications/initialized`, protocol sessions and
  `Mcp-Session-Id` are gone. Every request carries its version and client
  identity in `params._meta` (`io.modelcontextprotocol/protocolVersion`,
  `…/clientCapabilities`, optionally `…/clientInfo`).
- "Servers MUST NOT rely on prior requests over the same connection to
  establish context." Nothing a tool call sets may reach the next call.
- `server/discover` is mandatory: `resultType`, `supportedVersions`,
  `capabilities`, `ttlMs`, `cacheScope`, server identity in
  `_meta["io.modelcontextprotocol/serverInfo"]`, optional `instructions`.
- Every result carries `resultType` (`"complete"` here).
- HTTP POSTs carry `MCP-Protocol-Version`, `Mcp-Method` and, for
  `tools/call` / `resources/read` / `prompts/get`, `Mcp-Name` (Base64
  sentinel for non-ASCII). A mismatch with the body is `-32020`, an
  unsupported version `-32022` with `data: {supported, requested}`, both
  HTTP 400. An unknown method is HTTP 404 with `-32601`.
- GET streams and DELETE go away; change notifications move to a
  `subscriptions/listen` POST.
- A server MAY serve both eras on one endpoint: `initialize` selects the
  legacy era for that session, per-request `_meta` the modern one.

## What Claude Code 2.1.296 actually does

Measured through a logging proxy that answered `server/discover` itself and
forwarded the rest to the daemon:

1. It opens with `server/discover` (`MCP-Protocol-Version: 2026-07-28`,
   `Mcp-Method: server/discover`). On today's plain-text 400 it falls back to
   `initialize`, and everything works as before.
2. When discover succeeds, it never sends `initialize` or a session id. The
   only per-request identity is the three `_meta` keys above: there is no
   stable value a server could key state on.
3. **Without `resultType` on results it drops every tool silently.** It
   fetched `tools/list` four times, registered none of the 15 tools, and the
   session reported the server as having no tools. No error was shown.
   Adding `resultType: "complete"` alone made all of them load.
4. **A binding does not survive.** From `$HOME`,
   `prepare_harness_session(project=~/panda-alert-skin)` answered
   `panda-alert-skin`; the next `get_current_config` answered the daemon's
   default project. Each sessionless request gets its own request-scoped
   session (#428), so this is the spec-mandated behaviour, not a bug to patch.

## Session state that becomes per-request

| State | Set by | Effect when it no longer persists |
|---|---|---|
| Project binding | `prepare_harness_session`, `activate_project` | Later calls read the header/URL project or the daemon default, silently |
| Tool surface / preset | `set_profile`, `set_preset` | Every call gets the daemon's startup surface |
| Token budget | `prepare_harness_session`, `activate_project`, `set_profile`/`set_preset` | Every call gets the default budget |
| Deferred-loading state | `tools/list` and tier loads | Recomputed on every request |
| Recent tools and files | every call | Ranking hints and the low-level-chain warning lose their history |
| Doom-loop counter | every call | Repeated-call detection stops working |
| GET SSE stream | `GET /mcp` | `tools/list_changed` needs `subscriptions/listen` instead |
| Session journal (#421) | explicit bindings | Nothing left to journal |

The server's `instructions` string also turns misleading: it tells the model
to call `prepare_harness_session` first, which would bind nothing past that
call. A modern-era `instructions` has to say how binding works there.

## Who would be affected

Session journal, 60 sessions over 0.6 days (journal deployed the same day):
49 Claude Code, 6 doc-drift health checks, 4 Codex, 1 probe. Five carried an
explicit binding. Three of those were Codex sessions binding a worktree
(`.codex-worktrees/…`, `.codex/worktrees/…`). Codex has no launch-directory
expansion in its MCP config, so `prepare` is its only way to reach a
worktree. One was a Claude Code session whose project header already named
the same repo. The sample is small. It still shows that the clients losing
the most are the ones without host-side binding.

Host-side binding already covers most Claude Code sessions. 55 project
`.mcp.json` files send `x-codelens-project`, and the user-scope entry sends
`?project=${PWD}` (#432). Sessions launched from `$HOME`, or by parents
without `PWD`, have neither.

## Binding options

| Rank | Option | Works without sessions | Cost and risk |
|---|---|---|---|
| 1 | **Host configuration** (`?project=` / `x-codelens-project`) as the only binding in the modern era | Yes, by construction | Already deployed for Claude Code; Codex needs a per-project or launch-directory entry; `$HOME` sessions stay on the default project |
| 2 | **Per-call `project` argument** on read tools, as an escape hatch | Yes | Every schema grows an argument; the model must repeat it on every call; it has to clear [ADR-0018](../adr/ADR-0018-session-identity-and-coordination-hardening.md) decision 1 (identity from the transport, not arguments) for the same spoofing reasons, so it can only select among roots the principal may already open |
| 3 | **Server-issued handle** returned by `prepare` and passed back on each call | Yes | Same model dependence as option 2, less explicit, and server-side state the spec tells us not to rely on |
| — | Keep sessions in the modern era | No | Violates the MUST NOT above |

Recommendation: adopt option 1 as the modern-era contract. Add option 2 only
if a client that cannot set a launch-directory binding (Codex today) moves to
2026-07-28. Keep serving the legacy era for as long as clients fall back to
it, because it is the only era in which `prepare` binds anything.

## Enabling checklist

1. `resultType: "complete"` on every modern-era result. Without it Claude
   Code loses every tool (above).
2. `server/discover` with the fields listed above; `ttlMs`/`cacheScope` on
   list results already exist (`tools_list.rs`).
3. Era routing: `initialize` → legacy session; per-request `_meta` → modern,
   request-scoped (#428 already builds that session from headers and
   `_meta` `clientInfo`).
4. Header validation (`Mcp-Method`, `Mcp-Name` with Base64 sentinel,
   version equal to `_meta`), `-32020`/`-32022` bodies, 404 for unknown
   methods on the modern path only.
5. A modern-era `instructions` string and `prepare_harness_session` reply
   that say a binding lasts one request.
6. Replace the two guardrail tests below with tests of the modern path, and
   re-run the proxy measurement end to end.

## Guardrails

- `server::http_tests::protocol_version_tests::a_modern_discover_request_gets_a_400_the_client_falls_back_from`.
  This pins the HTTP fallback. The spec lets a dual-era client fall back to
  `initialize` only when the 400 body "is empty or is not a recognized modern
  JSON-RPC error". Making that 400 a spec-shaped `-32022` before the modern
  path exists would make clients retry instead of falling back, and the
  server would disappear.
- `tests::protocol_tools_list::server_discover_stays_an_unknown_method_until_2026_07_28_lands`.
  This pins the same behaviour on stdio, where any error other than a
  recognized modern one means "legacy server".
