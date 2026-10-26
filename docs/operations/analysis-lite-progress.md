# Analysis-lite implementation ledger

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
