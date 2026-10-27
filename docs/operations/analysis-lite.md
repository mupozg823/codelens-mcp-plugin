# Analysis-lite candidate operation

This optional read-only profile adds two JS/TS evidence capabilities to the
existing runtime: search with mode=refs and graph with mode=impact. Other
profiles retain their tool registry and behavior. Analysis-lite rejects direct
legacy tools, private candidate targets, other modes, control tools and content
mutations. Discovery and rejection apply to actual dispatch, not just listing.

The candidate has no adoption or performance finding yet. Protocol tests and
model-free host probes are separate from the later approved model pilot. The
global CodeLens-off configuration and retired launchd setup stay unchanged.

## Everyday registration (stdio, on demand)

For day-to-day use the profile runs as a stdio server that the host starts per
session, wrapped in a lazy stdio proxy so idle sessions start nothing. This is
the registration shape; the foreground HTTP runtime below stays the pilot and
multi-host shape. Registering is a per-user decision taken from pilot evidence.

- Binding. A stdio server binds once at launch: CLI path, then
  CLAUDE_PROJECT_DIR, then MCP_PROJECT_DIR, then the working directory.
  Claude Code 2.1.311 starts the MCP child with CLAUDE_PROJECT_DIR and the
  working directory both set to the session directory (measured 2026-10-27),
  so each session binds its own checkout and a worktree session binds the
  worktree. No header, URL project or initialize project is involved.
- Context cost. On this surface neither tool carries anthropic/alwaysLoad,
  even though search and graph share names with the CORE-10 facades. Both are
  deferred behind host tool search via anthropic/searchHint; the 5882-byte
  two-tool listing is not loaded until a search selects it.
- Lazy start. The proxy records the handshake once and replays it; the server
  process starts on the first tools/call. Measured: an idle session started 0
  backends, a session that called graph started 1, in its own directory. The
  initialize instructions name no path, so one recording is correct for every
  project that runs the same command line.
- Binding. Every analysis-lite stdio server starts on an empty per-process
  placeholder under ~/.codelens/analysis-lite-unbound/ and indexes nothing at
  launch, wherever it was launched. Each call binds only that request. An
  absolute target.file binds to the file's repository root, in any checkout,
  whether or not it lies under the launch directory. A relative target.file
  resolves under the launch directory's repository. Either root must carry a
  real project marker (a .codelens directory alone does not count), must not
  be $HOME or /, and must not sit inside a macOS File Provider (cloud-synced)
  folder (a folder whose provider tag cannot be read counts as synced); otherwise the call returns project_binding_required before anything
  is written. A session launched in $HOME, /, or a folder without a marker
  therefore has no launch project, and its relative target.files return the
  same error.
- Parallel sessions on one repository. The first server takes the project
  writer lease. A later analysis-lite server's call finds it busy and binds the
  repository as an explicit reader: no lease, watcher, refresh or writes, and the index opened
  read-only. Every response, including status=error evidence, carries
  freshness.runtime_role and, for a reader, writer_holder with alive (whether
  the holder pid still exists). If the writer has not created its index yet,
  that call returns a retryable index_not_ready and the next call retries the
  bind. A reader reports snapshot.generation as null and refuses
  generation pins, because the writer's mutation counter is in the writer's
  process. A reader is not promoted when the writer exits; its index stops
  following edits until its session restarts. Other profiles keep rejecting a
  second writer with project_writer_busy.

Install the binary from a merged commit to a versioned path and register that
path. Do not register a checkout's target/ directory: any build in that
checkout, including uncommitted work, replaces the server.

Claude Code (user scope; substitute the installed binary path):

```sh
claude mcp add-json codelens-lite -s user '{"type":"stdio","command":"'"$HOME"'/.local/bin/lazy-mcp","args":["--name","codelens-lite","--key-path","/path/to/codelens-mcp","--","/path/to/codelens-mcp","--profile","analysis-lite","--daemon-mode","read-only"]}'
claude mcp remove codelens-lite -s user   # way back
```

The `--key-path` argument makes a rebuilt binary invalidate the recorded
handshake. The binary needs no `http` feature for stdio. `codex exec` 0.164
starts a stdio MCP child in the thread's working directory without
CLAUDE_PROJECT_DIR (measured 2026-10-27), so the working-directory fallback
binds it the same way. Codex desktop-app threads have not been measured.

## One foreground runtime

Build an optimized task-owned binary, then run a single foreground HTTP process:

```sh
CARGO_BUILD_JOBS=1 cargo build -p codelens-mcp --release --no-default-features --features http
target/release/codelens-mcp /absolute/task-owned/project --transport http --listen 127.0.0.1 --port 17839 --profile analysis-lite --daemon-mode read-only
```

Choose a free task-owned port and record the actual PID, executable hash, source
HEAD and feature flags. Stop that foreground PID with Ctrl-C/SIGTERM after the
evaluation. Do not use launchd, autostart, a broker or a second stdio writer.
Existing single-writer leases and ProjectContext sharing remain active.

Both host sessions connect to the same process and explicitly bind the same
canonical project. Bind a different source checkout/worktree by its own
canonical root; its context and index are separate. Unbound analysis-lite reads
fail. An invalid explicit binding cannot fall back to the daemon's project.
Initialize instructions name only the two capabilities, binding and coverage
limits; they do not ask for an unavailable bootstrap tool.

## Temporary host adapters

The endpoint and headers below are a per-run configuration fragment. Keep normal
user authentication, model and effort settings. Do not install these globally.
Substitute the actual local port and canonical source root for each run.

Codex supports Streamable HTTP URLs and static http_headers. Use its existing
per-command config override mechanism for this fragment, without changing the
user config file. See the [official Codex MCP contract](https://developers.openai.com/codex/mcp).

```toml
[mcp_servers.codelens_analysis_lite]
url = "http://127.0.0.1:17839/mcp"
enabled = true
enabled_tools = ["search", "graph"]
http_headers = { "x-codelens-project" = "/absolute/task-owned/project", "x-codelens-profile" = "analysis-lite" }
```

Claude Code supports a task-owned JSON file passed with --mcp-config. Its
--strict-mcp-config option limits the run to the supplied MCP servers. Follow
the [official Claude Code MCP contract](https://code.claude.com/docs/en/mcp)
and check installed CLI help before running a probe or pilot.

```json
{"mcpServers":{"codelens_analysis_lite":{"type":"http","url":"http://127.0.0.1:17839/mcp","headers":{"x-codelens-project":"/absolute/task-owned/project","x-codelens-profile":"analysis-lite"}}}}
```

The target selector uses a nested object because the Claude API does not accept
root-level schema unions; its native host may otherwise rewrite or omit that
tool. The canonical anyOf stays inside properties.target. Server validation
still enforces the advertised selector combinations. Current official host docs
were checked on 2026-10-26; CLI discovery does not establish Desktop discovery.

## Inputs and outputs

```json
{"name":"search","arguments":{"mode":"refs","target":{"file":"src/service.ts","name":"handle","start_byte":142,"end_byte":148},"max_results":20}}
{"name":"graph","arguments":{"mode":"impact","target":{"file":"src/service.ts","symbol_id":"<exact returned identity>"},"max_depth":3,"max_nodes":30,"max_edges":40}}
```

Names alone must select one declaration, including same-name members that this
candidate cannot resolve. Ambiguity returns partial selection_candidates and
requires a choice. The token offsets are zero-based UTF-8 bytes with an
exclusive end; response lines and UTF-8 byte columns are one-based. They are
not TypeScript/LSP UTF-16 positions. Returned candidate IDs include exact token
spans. Legacy indexed IDs are accepted only when the indexed and disk source
hashes match, and the indexed selection identifies one source declaration.

The optional path narrows caller graph evidence. Local references to the
selected declaration are always included. Current caps/defaults are generated
from crates/codelens-mcp/tools.toml: references 1–200/default 100, depth
1–8/default 3, nodes 1–100/default 40, edges 1–200/default 100, source files
1–10000/default 2000. The source cap includes the target and is explicit when
truncated. Imports outside a narrow caller path may still need source reads.

Evidence includes schema_version, capability, canonical project/target,
snapshot generation and source basis, scope/backends, status, locations and
identities, provenance, limits, truncation, unresolved, freshness and errors.
Unknown totals stay null. Partial, unsupported and error are distinct. Actual
engine failures return an MCP error envelope with typed evidence, never a
successful affected-count zero. Rejections without source evidence retain null
target/snapshot rather than fabricating them.

The generation argument pins the committed symbol-index generation for a
follow-up read. Existing target-role/access checks and the generation fence
remain in the query path. A read crossing an index commit is rerun once;
repeated conflicts surface as retryable generation errors. Source hashes are
checked separately during the query. Watcher start/failure telemetry is
informational, and this read query performs no index reconciliation. Neither
generation stability nor a running watcher proves subsequent disk freshness.

## Coverage and limits

Supported evidence combines OXC-resolved local reads/writes with source-located
callgraph edges resolved to exact declarations. Named ESM imports, aliases and
named reexports use verified source tokens plus the existing project module
resolver. Duplicate source locations merge backend provenance. Different
callsites on one line remain distinct. Same-name nested declarations and
shadowed import aliases remain separate.

The extractor also captures callback arguments. All such edges are honestly
callgraph_edge/detail_unknown; direct-call detail is not inferred. Reverse
impact is bounded traversal over this extracted graph, with depth/node/edge
truncation and unresolved evidence. It is not a whole-program impact proof.

Cross-file general non-call, import and type references are incomplete.
Class/interface/property members, namespace member access, this-based dynamic
dispatch and star exports are unsupported or unresolved in this candidate.
Known member targets return unsupported with exact source token identity;
member uses never become edges to a same-name free function. Unsupported
language/source evidence remains explicit. Zero rows never prove unused.

Both MCP text and structuredContent preserve the same typed evidence. The
legacy lossy summary/compact sampler does not process candidate locations,
identities or provenance. Compact evidence above 18000 UTF-8 JSON bytes is
rejected with response_size_limit; narrow path/max_results or graph caps and
retry. No sampled zero is returned. This byte guard is a wire bound, not an
actual provider token measurement.

## Verification receipts

The implementation ledger is analysis-lite-progress.md. The local evidence
index is .omo/evidence/analysis-lite-core.md; raw artifacts are in the ignored
.codex/analysis-lite-evidence/core/ directory. It records RED/GREEN logs,
test/build/lint commands and exit codes, real socket request/response receipts,
native-host probes and the final optimized binary/source identity. Host-model
runs, Desktop discovery, performance and adoption are not inferred from these
functional checks; the later pilot owns those results.
