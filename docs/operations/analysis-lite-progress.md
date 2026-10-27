# Analysis-lite implementation ledger

## Accepted review fixes — caller identity and policy eligibility (complete; scoped review approved)

The parent accepted independent findings R1 and R2 at
`48e31f0dcfa8f552f75387ab6256ec5f781413ca` and assigned this worker the Rust caller
fix plus `harness-eval.py`, `harness_eval_common.py` and the aggregation tests.
Frozen tasks, gold and benchmark manifests remain unchanged. Evidence for this
pass is `.codex/analysis-lite-evidence/core/review-fix/`.

R1 RED reproduced false reverse propagation through an enclosing same-name
function for seven unsupported class/object member forms, including computed
and string names and property arrows. The located extractor now preserves the
actual AST caller boundary and exact declaration/name-token spans. Candidate
source identity requires an exact OXC match; unsupported members retain the
callsite with unavailable source and stop traversal. Nested same-name functions,
function expressions, variable arrows and anonymous callback containment in a
named lexical outer remain covered. Candidate GREEN: 15 engine and 8 MCP/HTTP
tests passed.

R2 RED reproduced activation recommendations through the actual CLI report
path despite zero qualifying sessions, plus promotion of a failed candidate.
Summaries retain every raw row and measured/unknown/failed acceptance and verify
counts. Unknown observations make the comparison inconclusive; observed failures
remain measured. Candidates with failed acceptance or verification cannot be
promoted, while a measured failed baseline remains a valid comparator. Shared
and agent summaries use the same eligibility check. All 61 related Python checks
passed (the previous 55 plus six scenario regressions). No new pilot adoption
threshold or global policy write was introduced.

The previous release session 95809 disappeared: both parent and owner receive
Unknown process id, no cargo/rustc process remains, and no saved exit code exists.
Its last log line is tempfile compilation. Completion is unconfirmed, no process
was killed, and the old log/source receipt remain intact. The full no-default
HTTP workspace suite passed: engine 463 passed/3 ignored, MCP 1280 passed/4
ignored, 9 integration tests and 1 doctest passed. Clippy with -D warnings passed
both no-default+http and default+http,semantic; canonical codegen drift,
formatting, Python compilation and benchmark invocation lint passed. Broad
Python lint retains the same seven baseline diagnostics and introduces none.
The subsequent optimized build succeeded from clean source commit
`23021fff0019b75532ac772ee9e1bf8ad8ea2edd`, no-default+http, jobs=1. Its executable
SHA-256 is `56ca90aedba109188d35931f80c9a82692f1c5cff61f24b2384044ad96205d2b`.
`core/release-binary.json` records build/socket exits 0 and owned-process cleanup.
`.omo/evidence/review-fix-review.md` records CLEAR/APPROVE, R1/R2 resolved and no
new critical regression in that scoped fix. The earlier 95809 uncertainty remains
historical evidence; it is not the successful build receipt. Model/pilot runs
remain zero; performance and activation remain unmeasured.

## Dispatch 3 — typed engine and strict candidate profile (implemented and reviewed)

Owner: one SOL implementation worker, isolated worktree
`/Users/bagjaeseog/.codex/worktrees/codelens-analysis-lite-20261026` on
`codex/analysis-lite`, starting at `30644142007a5c01885c98ea44d0a0de3c58bbf6`.
Parent accepted the dispatch-2 six-task freeze bundle
`b6c5d2c3023bed67fac86847cda60b92fb9d1fc0d0b1f4bc82ec9d3be9d47923`.
No model runs are authorized in this dispatch. Benchmark/task/gold files stay
outside this ownership. The current OMO launcher has a missing runtime target;
no active attempt directory could be read, so evidence is recorded under
`.omo/evidence/analysis-lite-core.md` with raw artifacts in repository-ignored
`.codex/analysis-lite-evidence/core/` and the exact launcher error retained.

Implementation seam: candidate-only exact OXC declaration identity and located
call-graph evidence; a surface-aware dispatch branch immediately after legacy
verb resolution, before target validation and the existing generation fence.
Existing profiles and legacy resolver behavior remain separate. A canonical
`tools.toml` candidate contract must drive the two schemas, validation and
fingerprint. Target selection and evidence errors must never become zero usage
or zero impact. Validation starts with the exact-declaration and two-descriptor
RED scenarios, then engine and actual HTTP scenarios under one cargo build job.

Implemented in this dispatch: exact OXC UTF-8 declaration/reference spans;
source-located graph evidence with exact lexical/import-export identities;
unconditional local/external merge and provenance deduplication; bounded
reverse traversal preserving multiple same-line callsites; typed status,
snapshot/source basis, errors, unknown totals and explicit unsupported member
targets. Class/interface/property dispatch is deliberately unsupported, with
structural source-token selection and no same-name free-function attribution.

The two generated descriptors use nested target selectors to avoid Claude API
root-level schema unions. `tools.toml` drives schemas, private target metadata,
runtime validation, defaults and the 18000-byte wire guard. Candidate dispatch
does not inherit registered hidden-alias callability. Existing session/role/
access and symbol-generation fencing remain. Candidate HTTP reads require a
valid explicit project binding; bad headers cannot fall back to the daemon
project. Initialize guidance is short and candidate-specific. Both response
channels preserve full typed evidence; oversized evidence is an explicit error
instead of the legacy lossy sampler's three-row preview.

Observed RED artifacts include the original 4-vs-2 declaration merge, missing
profile, lexical-function selection despite same-name class members, and three
HTTP failures: broad initialize instructions, invalid-header fallback, and
9 references silently summarized to 3. Their corrections have targeted GREEN
receipts in `candidate-2.log` and `candidate-3.log`. Later coverage added indexed
symbol IDs and independently exposed the legacy lookup's LIMIT 0 semantics;
the candidate now requests two rows to detect identity ambiguity. Final nested
selector coverage includes valid-schema direct-private-target rejection, not
just a missing-field failure. The full HTTP/no-default workspace suite passed
with build jobs=1 and test threads=2: engine unit 461 passed/3 ignored, MCP unit
1280 passed/4 ignored, engine integration 9 passed and doctest 1 passed. A
post-lint candidate rerun passed all 13 engine and 8 MCP/HTTP scenarios. Clippy
passed both no-default+http and default+http,semantic with -D warnings. Codegen
drift enforcement, its 16 regression checks, six manifest contracts, docs
surface, formatting and whitespace checks passed. The canonical surface
manifest was regenerated; old profile counts and old tool registries stayed
unchanged, with the two-tool candidate added as a fourth optional profile.

The optional project skill is `.agents/skills/analysis-lite/SKILL.md`; operation
and temporary host-adapter contracts are in `analysis-lite.md`. Installed CLI
help/version receipts show Codex 0.164.0 and Claude Code 2.1.310. Actual socket
receipts prove two explicitly bound sessions sharing one project, a separate
real Git worktree binding, exact two-mode schema/fingerprint, bounded impact,
direct mode/alias/mutation denial and typed binding/generation failures.

Native host checks used the debug test binary, not a performance binary:
Codex app-server initialized an ephemeral thread, listed the two tools and
received three typed references through direct mcpServer/tool/call, without a
turn/start request. It inherited the ordinary host's other configured MCP
status entries; no other tool was called. Claude's variadic --mcp-config
consumed positional mcp/get arguments in the first health-command attempt
(failure retained). A control-only initialize/mcp_status session then connected
and discovered exactly two tools. Its own debug log confirms protocolEra=legacy
and negotiatedProtocolVersion=2025-11-25. The server also correctly falls back
from a requested unsupported 2026-07-28 version; full support for that version
is not claimed. Claude model-level response consumption and Desktop discovery
remain unmeasured for the later pilot. All owned probe processes stopped.

Raw host assertions are `socket/host-assertions.json` beneath the evidence
directory. The optimized build and release HTTP receipt subsequently completed
at clean source `23021fff`, with the exact build/hash and scoped review receipts
above. Earlier native-host checks still describe the debug binary and must not
be represented as model-level optimized-binary consumption. No model request has
been made. No benchmark/task/
gold file, global setting or launchd entry was edited, and no shared process
was killed. Model-pilot and adoption work remain with the parent/later runner.

## Dispatch 1 — harness accounting

Scope: codex-task-runner.py, session-eval.py, one optional metrics helper, direct
regression tests, approved plan and this ledger. No Rust build or model run.

Preflight: isolated branch `codex/analysis-lite`, clean checkout at
`4ffd5d901fe449b3a1972fc6d036d077d42a65c3`. Codex CLI reports 0.164.0;
`exec --help` confirms normal user config/auth and per-command `-c` overrides.
No authentication file was read. Read complete target files (runner 624 lines,
session evaluator 289); existing runner exceeds 500 lines before this change.

Discovered accounting defects: server tools/list estimates were exposed as
provider input; remaining server tokens as provider output; process telemetry
automatically supplied task quality and success. Existing downstream
`harness-eval.py:298` also coerces missing tokens to zero. That consumer is outside
this dispatch and must be repaired before pilot aggregation/adoption. Existing
historical entry values cannot be reclassified as actual usage without raw host
evidence. Legacy v1 field names remain available with explicit estimate metadata;
new normalization uses a separate accounting version.

Implemented: normal runner execution inherits current host configuration and
authentication; no isolated home is created. Explicit model/effort overrides are
preserved and recorded, inherited values are recorded as unconfirmed instead of
invented. Legacy auth-copy helper returns before filesystem access unless an
explicit legacy caller enables it. New provider normalization retains raw JSON,
source and declared cache semantics; inclusive input is never charged cache twice,
exclusive input requires measured cache components, malformed counts remain errors.
Actual input/output stay null without provider evidence. Server estimates remain
separate; `bootstrap_tokens` and snapshot token fields retain explicit legacy
estimate semantics. Task quality/success are never inferred from server telemetry;
the historical weighted metric is now `process_kpi_score`.

Evidence directory (local, intentionally untracked):
`.codex/analysis-lite-evidence/dispatch1/`. RED: original 12-test run reproduced 8
failures and 5 missing-contract errors (including subtests). Final GREEN:
13 regression tests, 12 existing policy tests, and 17 standalone policy checks
pass. `build.log` records successful Python compilation; `benchmark-lint.log`
records repository benchmark lint success; `whitespace.log` records clean diff
whitespace. Exact commands and exit codes are in `checks.json`.

The broad host-configured ruff check returns 1 for 10 inherited diagnostics
(11 in the original target files). Baseline comparison in `lint.json` confirms
zero introduced diagnostics, and its comparison gate returns 0. These include
existing shebang/mode, timezone, broad-except and duplicate `captured_at` issues;
they were not expanded into this task. There is no repository Python lint config.
The directly applicable repository benchmark lint returns 0.

Remaining boundaries: this normalization accepts one provider usage record;
run-level event collection/turn aggregation and confirmation of provider-effective
model/effort still require later work. Historical v1 entries cannot be trusted as
actual token measurements. The legacy auto-report consumer and policy refresh
flow must not be used for pilot decisions until unknown propagation and global
write isolation are repaired. No global policy/configuration or auth file changed;
no model call, Rust build or HTTP process was run. All later stages remain pending.

## Dispatch 2 — aggregation and task freeze

Scope: harness-eval.py and harness_eval_common.py, direct aggregation tests,
six-task data, selection metadata and this ledger. Existing harness-eval.py was
854 lines; it was read completely, in consecutive sections, before editing.

Implemented actual-only provider totals with measured/unknown denominators;
missing/partial/error usage and historical/synthetic estimates cannot become
zero-cost observations. Explicit legacy provider measurements require both
`token_source=provider` and `provider_usage_source`; ambiguous historical values
remain recoverable in raw entries and separately classified historical estimates.
Legacy telemetry quality is unknown unless an explicit quality source exists.
Modern unknown acceptance/verification cannot qualify for policy input, and
distinct run_id values preserve repetitions instead of deduplicating them away.
Failed/unknown rows remain in report summaries. Candidate-only or incomplete
comparisons are inconclusive; native fallback is not an adoption finding.

RED→GREEN: 13 aggregation contracts plus all 13 dispatch-1 tests pass. Relevant
existing policy checks, Python compilation, repository benchmark lint, diff/scope
checks and broad-ruff baseline comparison are recorded in
`.codex/analysis-lite-evidence/dispatch2/checks.json` and sibling logs. No broad
inherited lint cleanup was added.

Selected/frozen six tasks: three everyday (two reads, one real historical repair)
and three hard (same-name/local+external, alias/re-export, bounded impact).
`benchmarks/analysis-lite/tasks-v1.json` includes exact Git trees, independent gold,
acceptance rules and source hashes. The companion research document freezes
name-token/UTF-8 span semantics and labels E3/H2/H3 honestly as source-derived,
with field failures unconfirmed. This metadata is covered by the freeze bundle.
25 Git source/config object hashes and identifier byte slices are checked; H1's
TypeScript UTF-16→UTF-8 offset conversion is explicitly asserted. Parent acceptance
of final `freeze.json` is required before the first of the maximum 48 model runs.

Remaining: pilot event collection/aggregation, effective host setting receipts,
global-write isolation, independent grader implementation, source copies and E2
npm dependency/test setup. Engine/profile/HTTP/host work and actual pilot remain
pending. No model run, original repository edit, credential access, source
checkout, HTTP process or global policy/configuration write occurred.

## Dispatch 4A — CLI event accounting adapter

Implemented `analysis_lite_events.collect_cli_events(host, stdout, stderr=...,
returncode=..., fresh_session=...)` as a pure parser. It preserves raw stdout,
stderr, decoded events, malformed lines, terminal usage, tool/retry and model
receipts. It executes no commands, reads no files/authentication/configuration,
and cannot override model/effort. Tests are explicitly synthetic protocol fixtures;
they are not actual provider measurements.

[Claude's current usage contract](https://code.claude.com/docs/en/agent-sdk/cost-tracking)
distinguishes cumulative whole-tree modelUsage from main-loop usage and assistant
output placeholders. The adapter selects the latest modelUsage map, sums models
once, maps separate cache components, and retains the original map. Complete
current-run counts require fresh-session setup, successful termination, no reset,
no malformed gaps and nondecreasing cumulative counters. Missing/failed/partial
evidence stays unknown while observed scoped counters remain recoverable.
[Claude's CLI stream contract](https://code.claude.com/docs/en/headless) provides
parent_tool_use_id and visible retry events; retry count is not assumed exhaustive.

[Codex's exec contract](https://learn.chatgpt.com/docs/non-interactive-mode)
establishes JSONL events. The generated installed 0.164.0 app-server schema has
separate total/last counters and cacheWriteInputTokens; it is not an exec event
schema. Upstream renamed the processor to event_processor_with_jsonl_output.rs.
Its immutable official Git blob
`73e0c9d911c82aae67c44e6b1a4ce54c7092b6be` maps total counters into turn.completed
and filters some collaboration events. The 0.164.0 public tag was unavailable,
so this upstream source is not proof of the installed binary's descendant scope.
The adapter preserves observed root counters/cache writes, rejects duplicate or
conflicting terminals, and leaves multi-turn/descendant completeness unconfirmed.
It does not add cumulative snapshots. Future version-matched coverage receipts
can support promotion by replaying saved raw logs without new model requests.

Only an observed main assistant model becomes an effective-model receipt;
configured aliases stay separate, and unobserved effort remains unknown.
Paired visible tool IDs and visible retry events are reported with their scope.
No expected setting or server estimate supplies a provider counter.

4A1 stopped at the builder's edit boundary after final source inspection found
one valid Codex error envelope misclassified as malformed: its message is a
string, unlike assistant/user message objects. The parent assigned a bounded
4A2 repair to this same worker. Its new regression first failed for the observed
misclassification; the minimal fix restricts message-object validation to the
assistant/user event kinds. This retains valid error receipts while preserving
the existing semantic-malformation checks. Final helper verification covers 20
synthetic cases; exact exit codes and RED/GREEN logs are retained beside 4A1.

Evidence is ignored under `.codex/analysis-lite-evidence/dispatch4a/`: official
semantics metadata and the pinned upstream source, RED/GREEN logs, exact commands,
exits and diff. New regressions cover duplicates, cache writes/reads, multiple
turns, reset, cumulative regression, children, failures, malformed data, missing
usage, tool pairs, retries, final text and unknown settings. Frozen task files
remain unchanged. Snapshot/grader/driver and all 48 model runs remain separate;
model turns are still zero. Future raw pilot logs belong in ignored evidence and
a separate durable non-cloud path, not in the PR with private source/auth/settings.

## Dispatch 4B — source snapshots, independent grader and protocol freeze

Created source-only Git-object templates under
`/Users/bagjaeseog/codelens-analysis-lite-eval/2026-10-26/snapshots/` without
overwriting existing folders. `analysis_lite_snapshots.prepare_snapshot` retains
JS/TS and configuration/test source plus applicable AGENTS/CLAUDE instructions;
it excludes every symlink/gitlink, history, hidden state, auth/env files, docs,
old work records and non-source assets. Returned control-only inventories record
each selected/excluded path, immutable Git blob hash, size and reason, plus source
SHA-256. All 25 frozen source/config hashes are mandatory. Template sizes:
Tuanbo 30,635,850 bytes/2,894 files; Winnation current 20,129,560/2,941; replay
12,834,060/2,231. The excluded documentation/assets limit the evaluation context;
the protocol records that limitation rather than claiming complete-repository QA.

Frozen replay package/lock were copied to task-owned setup. One `npm ci` with
ignore-scripts/no-audit/no-fund, task cache and empty task npmrc files succeeded
in 6.6365 seconds; original node_modules/configuration were not used. Setup,
source inventories and raw logs remain in the durable control directory and
ignored `.codex/analysis-lite-evidence/dispatch4b/`.

`grade_answer` compares exact claimed declaration, call/noncall and bounded impact
sets while preserving raw/unstructured/null answers for manual review. Mechanical
pass never supplies task quality; independent SOL semantic/rubric review is still
required. `run_oracle` executes literal and 100,001 decimal cases on actual source.
Post-agent `grade_edit` checks the allowed two-file diff and frozen package/lock,
replays the actual new test on a grader-only base copy, runs final GREEN and the
independent oracle, and preserves receipts. Dependencies are physical/APFS-cloned
copies within each execution folder; external links are rejected and copied
setup caches excluded. The dependency source/delivery method/time are explicit;
model-run delivery remains the later driver's responsibility.

Eight new helper tests pass with real task-local TypeScript/Vitest. A separate
grader-only smoke used the historical repaired formatter plus a synthetic test:
base RED exit 1, final GREEN exit 0, six literals and 100,001 oracle cases passed.
Its full grading wall was 45.2744 seconds, including dependency copy/check work;
it is not a model result. No new test/gold was written into a model template.

`benchmarks/analysis-lite/protocol-v1.json` freezes common source-only instructions,
non-leaking response shape, seeded twelve-block/four-run schedule, explicit
canonical bindings, timeouts, unknown cold/warm state, raw wall/setup scopes,
startup half-allocation to B's two hosts and independent per-host adoption gates.
A has no candidate process; each B block shares one foreground process whose
default project is empty control, then stops its own PID before A. The protocol
and source-filter hashes must be accepted before model execution. All frozen
task/gold/selection hashes remain unchanged; actual model runs remain zero.

Remaining seam: 4C owns foreground lifecycle, run dependency copies, host adapters,
48 CLI executions, receipt collection, independent semantic review and reporting.
This dispatch starts no server/model and does not implement that driver. No
original source, credential, global host setting or production service changed.

### Parent-approved 4B2 repair before any model run

Protocol revision 2 preserves normal task-native subagents under existing user/
host routing rules; it prohibits extra evaluation conversations, probes and paid
fallbacks rather than changing the native workflow. Child activity/access and
whole-tree usage scope must be retained. Forty-eight scheduled CLI trials are
distinct from permitted task-internal delegation.

The source filter now also preserves safe tracked `.gitignore`, `.ignore` and
`.codelensignore` plus `.mts`/`.cts`. New templates and inventories are under
`snapshots-v2/` and `control/filter-v2/`; first-generation receipts/templates are
retained and retired without overwrite. V2 sizes: Tuanbo 30,639,434 bytes/2,903
files; Winnation current 20,130,690/2,943; replay 12,835,092/2,233. All 25 required
source hashes remain unchanged. The future candidate's default project is the
dedicated source-free `control/filter-v2/server-empty`, never the control parent
that holds grader-only source and receipts.

Both npm tests and Node oracle now use `run_owned`: each starts a new process
group; timeout sends TERM/KILL only to that group and reaps its leader. Receipts
retain PID/PGID, cleanup signals, timeout state, text stdout/stderr and base64 of
their original bytes, making partial/error output JSON-serializable and recoverable.
The new synthetic child-timeout regression confirms child termination while an
unrelated sentinel remains alive. The real Node-oracle timeout receipt records
owned PID/PGID 67659, exit -15, timed_out=true, leader_reaped=true and result=null.
It is a grader fixture, not a model execution.

Targeted 4B2 verification reruns only the changed filter, own-child timeout, Node
oracle and post-run E2 paths; unchanged read-grader cases are not repeated. New
freeze/check receipts use `freeze-v2.json`/`checks-v2.json` alongside preserved V1
evidence. Model runs and server starts remain zero; frozen task/selection inputs
are unchanged. Foreground lifecycle and actual evaluation remain with 4C.
