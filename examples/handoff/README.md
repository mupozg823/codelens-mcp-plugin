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

## Extending to BuilderResult and ReviewerVerdict

This adapter only emits `planner_brief`. The same pattern applies to
`builder_result` and `reviewer_verdict`:

- Subclass `StdioClient` to call `audit_builder_session` or
  `audit_planner_session` instead of `analyze_change_request`.
- Map the audit output's `status`, `findings`, and `session_summary`
  into the `BuilderResult.audit` or `ReviewerVerdict.audit` objects
  defined in the schema.

A consumer host would invert the flow: read a persisted artifact,
dispatch the next leg, produce the next artifact.

## Related

- Schema: [`docs/schemas/handoff-artifact.v1.json`](../../docs/schemas/handoff-artifact.v1.json)
- Spec: [`docs/harness-spec.md`](../../docs/harness-spec.md)
- ADR: [`docs/adr/ADR-0005-harness-v2.md`](../../docs/adr/ADR-0005-harness-v2.md)
- Runtime resources: `codelens://harness/spec`, `codelens://schemas/handoff-artifact/v1`
