# Usability cycle, 2026-10-10

Baseline for PR #413. Every number here was measured before the change, so the
post-deploy re-measurement has something to compare against. Telemetry rows are
`recording_origin = runtime` from `.codelens/telemetry/tool_usage.jsonl` on the
shared :7838 daemon (binary 212f33b, deployed 2026-10-03).

## 1. Telemetry, 2026-09-26 to 2026-10-10 (232 sessions, 2,786 calls)

| tool / mode | calls | failed | p50 ms | p90 ms | max ms | total s |
|---|---:|---:|---:|---:|---:|---:|
| prepare_harness_session | 553 | 19 | 1,409 | 30,002 | 10,128,817 | 49,670 |
| review / changes | 774 | 74 | 1,099 | 18,394 | 271,798 | 5,032 |
| diagnose / file | 989 | 249 | 137 | 5,863 | 69,570 | 1,461 |
| search / refs | 142 | 36 | 1,891 | 12,973 | 360,309 | 1,382 |
| graph / callers | 49 | 1 | 1,005 | 15,030 | 50,734 | 284 |
| search / symbol | 62 | 2 | 12 | 227 | 731 | 4 |

Since the 2026-10-03 deploy only (84 sessions, 1,449 calls): prepare p90
14,321 ms (13 calls over 30 s, max 734 s), diagnose/file 110 of 682 failed,
review/changes 52 of 513 failed, search/refs 3 of 11 failed.

- Clients: `codex-mcp-client` 1,458 rows, no `client_name` 1,206, `grok-shell-codelens` 78, `claude-code` 43.
- 73 of 232 sessions called `prepare_harness_session` and nothing else.
- `diagnose/file` failures by extension: `.js` 59 of 59, `.mjs` 36 of 38, `.ts` 58 of 152, `.tsx` 47 of 131, and every CSS/JSON/Markdown/PNG call.
- Failed rows carried no reason, so each cause below was reproduced by hand.

Re-measure with the same filter after redeploy; `error_kind` now explains the failures.

## 2. Causes found

1. **TypeScript 7.** The global `typescript` is 7.0.2 (the Go port), which ships no `lib/tsserver.js`. typescript-language-server 6.0.1 needs one, so every project without its own TypeScript ≤ 6 failed at `initialize`. `tsc --lsp --stdio` (serverInfo `typescript-go`) initializes in ~0.05 s and supports pull diagnostics.
2. **"No language server" was an error**, not a structured not-applicable result.
3. **The verifier counted errors as clean.** `review(mode=changes)` on drawboard: "No diagnostics reported for 4 touched file(s)", `diagnostics_ready: ready`, while all three checked files had failed and the fourth was never checked.
4. **Gitignored files in the index.** drawboard: 1,219 indexed files, 676 under `.archive/` and 76 under `public/`, both gitignored. `search(mode=refs, symbol_name=renderBoard)` without `path` failed: "declared in 53 files".
5. **Upstream Smoke** failed every night since at least 2026-07-20 (last success 2026-07-08). The 07-20 run failed at the fixture matrix, the 08-15 and 10-09 runs at the self retrieval gate (`--method-workers 4` against one project → `project_writer_busy`).

## 3. Gitignored share of every local index (before)

`git check-ignore --stdin` over `files.relative_path` of each `.codelens/index/symbols.db`
(alias paths such as `~/dev/*` point at the same repository and are listed once):

| repository | gitignored / indexed | share | index size |
|---|---:|---:|---:|
| drawboard | 777 / 1,219 | 64% | 166 MB |
| winnation-sites-unified | 428 / 836 | 51% | 9 MB |
| SignatureStudio | 1,821 / 6,942 | 26% | 73 MB |
| shadow-partner | 133 / 832 | 16% | 304 MB |
| resolume-loop-studio | 38 / 239 | 16% | 7 MB |
| drawboard-notice-fit-20261007 | 75 / 521 | 14% | 20 MB |
| tuanbo-broadcast-suite | 59 / 2,867 | 2% | 44 MB |
| all 23 git projects | 6,913 / 40,825 | 16.9% | |

## 4. Claude transcripts, 2026-09-26 to 2026-10-10

1,563 transcript files (125 main sessions, 988 subagent transcripts).

- CodeLens was 1.0% of code exploration (366 non-prepare calls against 35,695 native searches); in main sessions 13 calls against 17,242.
- 61 of 190 transcripts that bound CodeLens never made a second CodeLens call.
- 23 `prepare_harness_session` calls hit the 60 s client timeout, 20 of them on worktree, alias or tmp paths.
- `codelens-explorer` was dispatched 0 times out of 804 agent dispatches; the routing rules contradicted each other.
- The builder's mandatory `diagnose` gate failed 17 of 19 times (TypeScript start failure, no language server, or a worktree path outside the bound checkout).
- The server attributes only 43 calls to `claude-code` while transcripts show 586: client attribution drops (section 5), it is not low usage alone.

The host rules were reconciled in the same cycle (`~/.claude` commit b4241e9).

## 5. Left open

- **serde-json retrieval.** With the smoke path unblocked, the upstream matrix fails on a real quality gap: for "serialize typed value into json string", `semantic_search` returns `from_borrowed`, `from_iter`, `from_string`, `json_expect_expr_comma`; the expected `serialize_bool` is not in the top 3. Reproduced locally (binary 212f33b, MiniLM-L12-CodeSearchNet-INT8, serde-json `dc8003a`, 74 files, 1,767 embedded symbols): every natural-language score sits at 0.16–0.21, the serialization entry points (`to_string`, `to_value`) are not in the top 8, and "serialize a bool into json" ranks `deserialize_bool` first. The hybrid lane is fine on the same index: `get_ranked_context` ranks `serialize_bool` first for "serialize a bool into json" and `serialize_newtype_struct` first for the smoke query. Ruled out: generic query bridges (`CODELENS_GENERIC_BRIDGES_OFF=1`) and automatic embedding hints (`CODELENS_EMBED_HINT_AUTO=0`), both byte-identical results. The expectation landed 2026-07-06 and the 07-08 smoke passed, so the pure embedding lane regressed after that; the run artifact has expired. Next step: bisect `semantic_search` on this repo between 07-08 and now with release builds. `search(mode=semantic)` had 0 calls in two weeks, so agents are not hitting it today.
- **Client attribution.** 39 sessions have no `client_name` on any row and 26 mix named and unnamed rows. Session metadata is injected only while the session is in the store; a session dropped and resurrected without `initialize` loses its client name.
- **Bind latency.** Re-measure after the gitignore cleanup before designing a bounded or asynchronous bind; prepare p90 was 14 s after the 10-03 deploy.
- **LSP pre-warm** picks servers by extension only (it is pure by design), so on a TypeScript-7-only machine `auto` pre-warm would start typescript-language-server and fail. Pre-warm is `off` in the deployed plist.
- **`get_changed_files` on an unbound session** fails with "not a git repository: ~/.codelens/daemon-default" (9 calls); the message does not say to bind first.

## 6. After the redeploy (c802d6c, 2026-10-10)

Both daemons redeployed (`:7838`, `:7736`; rollback binaries `codelens-mcp-http.rollback-212f33b`, `codelens-mcp-http-dev.rollback-be9e0ab`), `daemon-stale-check` in sync.

| check on drawboard | before | after |
|---|---|---|
| indexed files | 1,219 | 442 (first bind ran the one-time cleanup refresh; prepare 4.9 s) |
| gitignored rows / `.archive/` rows | 777 / 676 | 0 / 0, `discovery_signature = gitignore=on` |
| `search(mode=refs, symbol_name=renderBoard)` without `path` | error: declared in 53 files | 16 references, path inferred `src/web/draw-board-app.js` |
| `diagnose` `.mjs` / `.js` / `.css` | LSP init error / LSP init error / error | `checked:true` 0 / `checked:true` 27 TypeScript hints / `checked:false` + reason |
| `review(mode=changes)` diagnostic verdict | "No diagnostics reported for 4 touched file(s)" | "No blocking diagnostics in 3 checked file(s) … (1 not checked (cap 3))" |

Found during acceptance and fixed in the follow-up: `index_freshness` called this freshly verified index `stale` (31 h) because it measured the age of the last write, not whether the watcher keeps the index current.

Claude Code's user MCP entry now sends `x-codelens-client: claude-code`, so a session resurrected after a daemon restart (which the in-memory identity cache cannot cover) keeps its attribution; verified with a resurrected probe session (`client_name: claude-code` in telemetry).

## 7. The long bind stalls: a consent dialog nobody saw

Pre-warming the largest project after the redeploy (SignatureStudio, `~/Downloads`) took 238 s. The phase split in the daemon log put 234.8 s inside `IndexDb::open`, all of it in `Connection::open` (`connect_ms=234,819`); the discovery cleanup refresh itself took about 3 s (6,942 → 5,124 files). `~/drawboard` bound in 4.9 s at the same time.

After the next redeploy (845e8d3) the same bind did not finish for more than ten minutes. A `sample` of the daemon showed the build thread inside `open()` (`IndexDb::open` → `sqlite3BtreeOpen` → `unixOpen` → `__open`), the system log showed `sandboxd` sending "request approval" for the daemon at 06:42:14, and the screen held the macOS dialog "‘codelens-mcp-http’이(가) 다운로드 폴더의 파일에 접근하려고 합니다." with "허용 안 함" / "허용" (read through the accessibility tree; nothing was clicked).

Mechanism: `scripts/redeploy-daemons.sh` re-signs the binary ad hoc (`codesign --force --sign -`), so every redeploy is a new code identity to TCC. The first open in a TCC-protected folder (`~/Downloads`, `~/Documents`, `~/Desktop`) by this launchd agent asks for consent, and the open blocks until the dialog is answered. The first incident released at 06:05:02, when launchd also recorded `sandboxd` going inactive and respawning; an earlier draft of this record read that as a hung `sandboxd`, which the second incident rules out. The 2026-09-28/29 stalls recorded as unexplained (`seed_ms` 1–2 h, several builds released within seconds of each other) followed redeploys too and fit the same mechanism, though no dialog was observed then.

CodeLens cannot answer the dialog. PR #415 keeps requests from waiting on it: builds run in the background and a request answers within `CODELENS_BIND_BUDGET_SECS`.

After the cleanup, 3,354 of SignatureStudio's 5,124 indexed files were still under `.codex-worktrees/<name>`, linked worktrees that duplicate the tree; #415 also stops indexing those.

Durable fixes, both the user's call: sign the daemon with a stable identity (a self-signed code-signing certificate in the login keychain) so a consent survives rebuilds, or keep active repositories outside the protected folders. The user chose the stable identity (§9).

## 8. serde-json `semantic_search`: an unstable single-query check, not a lane regression

- The pure semantic lane is not regressed overall: in the CI self-retrieval run of this cycle (112 queries on this repository) `semantic_search` scored MRR 0.672 / Acc@1 0.56, against the historical baseline 0.502 / 44%; hybrid `get_ranked_context` scored 0.748.
- The embedding model is unchanged since April (`model.onnx` pinned by SHA in CI). Embedding-directory commits since 07-08 touch indexing, cleanup and lints, not scoring.
- `semantic_search` merges lexical candidates at `(bm25 / 100) × 0.35`. On serde-json the embedding similarities for this query sit at 0.16–0.23, so injected lexical candidates and score ties decide the top three. The same index and query ranked `serialize_*` first with the release (CoreML) binary, `into_float`/`json_internal` first with a debug (ONNX CPU) build, and `from_borrowed`/`from_iter` first in CI. The upstream expectation (`serialize_bool` in the top three) passes or fails on that instability.
- The lexical lane failed silently (`unwrap_or_default`); it now reports `retrieval.lexical_lane` and a `degraded_reason` when it fails.
- 07-10 (`a3a96131`) changed `index_embeddings` to default to a background job, which is what broke the fixture smoke until the harness asked for `background: false`.

Next: check whether CoreML and ONNX embeddings rank the same on a fixed query set, and replace the single-query upstream expectation with a small query set per project.

**Correction (later the same day).** The backend explanation above is wrong. At the pinned revision, the release binary on CoreML, the same binary forced to CPU (`CODELENS_EMBED_PROVIDER=cpu`) and CI on Linux return the same eight results with the same scores to four decimals (`from_borrowed` 0.2063 first). The embedding lane contributed nothing. The smoke passes `path_hint: "src"`, and the scoped vector query filtered `file_path` by a range. sqlite-vec 0.1.9 fails that query with "Could not filter metadata fields" when the partition holds a path of exactly 12 bytes; serde-json has `src/error.rs`. `unwrap_or_default` turned the error into an empty lane. Without `path_hint` the same index ranked `serialize_*` methods first. After the fix the scoped lane is `ok`, but `serialize_bool` ranks 28th. The expectation pins one of about fifteen near-identical `serialize_*` siblings, and `to_string` ("Serialize the given data structure as a String of JSON") is not in the top 40, because its leading `///` doc never reaches the embedding text: 4.3% of serde-json's Rust symbols and 2.8% of this repository's carry `doc=present`. That is the next fix.


## 9. Session state across a restart, and the signing identity (a55ad0b, 2026-10-10)

Same steps before and after, on the dev daemon (`:7736`): initialize with client name `journal-probe`, `prepare_harness_session(project=~/panda-alert-skin)`, `get_current_config`, then `launchctl kickstart -k` the daemon and call `get_current_config` again under the same `Mcp-Session-Id` with no new initialize.

| | before the restart | after the restart | `client_name` in the post-restart telemetry row |
| --- | --- | --- | --- |
| ebc5c12 (no journal) | `~/panda-alert-skin` | `~/codelens-mcp-plugin` (the daemon default, no error) | null |
| a55ad0b (#421) | `~/panda-alert-skin` | `~/panda-alert-skin` (pid 50468 → 52547) | `journal-probe` |

- The entry is `~/.codelens/runtime/project-writers/sessions/<id>.json`, mode 0600: client name and version, host context, requested profile, project path and binding source (`explicit_tool`). It has no trust field.
- Sessions created by the pre-journal binary have no entry, so the redeploy that installed #421 still dropped their bindings once. Sessions created after it survive later restarts.
- Both daemons are now signed with the `native-orchestrator-helper` identity (designated requirement `identifier "dev.codelens.mcp-http" and certificate leaf = H"ab22b72e…"`). That identity sits in a dedicated keychain, which a reboot had locked: the first signing attempt waited on a keychain password dialog and was still waiting at 45 s. #422 bounds that wait. After unlocking, signing took 0.13 s and both redeploys completed in 3 min 22 s.
- Not measured yet: whether one folder consent under the new requirement survives the next redeploy. The screen was locked after the deploy, and no bind in a protected folder had run. The last ad-hoc stall before the switch held a SignatureStudio bind in `open()` for 501 s (`connect_ms=501,247`, 07:12).
