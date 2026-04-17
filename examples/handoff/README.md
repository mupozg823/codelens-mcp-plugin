# Handoff Artifact Reference Adapter

Demonstrates that the [v1 handoff artifact schema](../../docs/schemas/handoff-artifact.v1.json)
is implementable from an external host using only stdio MCP and Python
stdlib — no MCP SDK, no pip dependency.

## Why this exists

The original criticism during the v1.9.41 session was that
`docs/schemas/handoff-artifact.v1.json`, `codelens://harness/spec`, and
`codelens://schemas/handoff-artifact/v1` were shipped without a single
external consumer. Schema validation existed, but nobody actually
produced or consumed the artifact shape. This directory is the first
external producer so the schema stops being speculative infra.

## What it does

`planner_brief_producer.py` spawns a `codelens-mcp` subprocess in
stdio mode with the `planner-readonly` profile, issues the MCP
initialize handshake, calls `analyze_change_request` for a user-given
task, and reshapes the response into a `PlannerBrief` conforming to
`schema_version: codelens-handoff-artifact-v1`, `kind: planner_brief`.
Output is printed as JSON to stdout or a file.

## Requirements

- A built `codelens-mcp` binary (`cargo build --release` at the
  workspace root produces `target/release/codelens-mcp`).
- Python 3.11+ (for `datetime.UTC`).

## Usage

```bash
python3 examples/handoff/planner_brief_producer.py \
    --binary target/release/codelens-mcp \
    --project . \
    --task "Split tools/query_analysis into intent + bridge modules" \
    --output planner-brief.json
```

`--output -` prints to stdout.

## Output shape

The script emits a JSON object conforming to the top-level
`PlannerBrief` path in the schema:

```json
{
  "schema_version": "codelens-handoff-artifact-v1",
  "kind": "planner_brief",
  "session_id": "planner-example-<uuid>",
  "producer": {
    "role": "planner-reviewer",
    "surface": "planner-readonly",
    "client_name": "planner_brief_producer.py",
    "client_version": "0.1.0"
  },
  "created_at": "2026-04-18T10:30:00+00:00",
  "payload": {
    "goal": "...",
    "rationale": "...",
    "ranked_context": [
      { "kind": "symbol", "reference": "path#symbol", "why": "..." }
    ],
    "target_paths": ["path1", "path2"],
    "acceptance": [
      { "id": "ac-1", "statement": "...", "verification": "cargo test ..." }
    ],
    "preflight": {
      "verify_change_readiness": {
        "status": "ready",
        "generated_at": "...",
        "blockers": [],
        "cautions": []
      }
    }
  }
}
```

## Validation

Validate the output against the schema with any JSON Schema validator:

```bash
# ajv (Node.js)
npx ajv validate -s docs/schemas/handoff-artifact.v1.json -d planner-brief.json

# jsonschema (Python)
pip install jsonschema
python3 -c "
import json, jsonschema
schema = json.load(open('docs/schemas/handoff-artifact.v1.json'))
brief  = json.load(open('planner-brief.json'))
jsonschema.validate(brief, schema)
print('valid')
"
```

The script intentionally does not bundle a validator so it can run
from stdlib only. A real production host should validate before
emitting.

## BuilderResult producer

`builder_result_producer.py` is the second leg of the chain. It
queries `audit_builder_session` for a completed builder session,
pulls `get_file_diagnostics` for each touched file, and emits a
`BuilderResult` conforming to the same schema.

```bash
python3 examples/handoff/builder_result_producer.py \
    --binary target/release/codelens-mcp \
    --project . \
    --session-id <builder-session-id> \
    --changed-file crates/codelens-mcp/src/tools/reports/eval_reports.rs \
    --tests-command "cargo test -p codelens-mcp --features http" \
    --tests-passed 316 \
    --tests-failed 0 \
    --planner-brief planner-brief.json \
    --output builder-result.json
```

`--planner-brief` is optional; when provided, its `session_id` becomes
`parent_artifact.session_id` so the two legs chain together as the
schema's `parent_artifact` field intends.

## ReviewerVerdict producer

`reviewer_verdict_producer.py` closes the planner → builder → reviewer
chain. It audits its own read-only session plus the reviewed builder
session, then derives a decision (`approve` / `request_changes` /
`block`) from the two audit statuses:

- both `pass` → `approve`
- reviewer `fail` → `block` (read-side contract break)
- builder `fail` → `block` (mutation gate break)
- any `warn` with findings → `request_changes`

```bash
python3 examples/handoff/reviewer_verdict_producer.py \
    --binary target/release/codelens-mcp \
    --project . \
    --reviewed-session-id <builder-session-id> \
    --builder-result builder-result.json \
    --output reviewer-verdict.json
```

`--builder-result` is optional; when provided, its `session_id`
becomes `parent_artifact.session_id` so the full chain links together.

All three external producers ship as stdlib-only Python and together
verify that schema v1 is end-to-end implementable from outside the
codelens-mcp crate boundary.

## Beyond v1

A consumer host inverts the flow: read a persisted artifact, dispatch
the next leg, produce the next artifact. Future host adapters would
add:

- A persistence layer (artifacts live today only as files the caller
  wrote manually; a richer host would store + fetch by id).
- JSON Schema validation before emit (this reference omits it for
  stdlib-only simplicity).
- Transport diversity: HTTP daemon client, Server-Sent Events, or
  future MCP Streamable HTTP.

## Related

- Schema: [`docs/schemas/handoff-artifact.v1.json`](../../docs/schemas/handoff-artifact.v1.json)
- Spec: [`docs/harness-spec.md`](../../docs/harness-spec.md)
- ADR: [`docs/adr/ADR-0005-harness-v2.md`](../../docs/adr/ADR-0005-harness-v2.md)
- Runtime resources: `codelens://harness/spec`, `codelens://schemas/handoff-artifact/v1`
