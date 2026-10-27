---
name: analysis-lite
description: Optional exact JS/TS declaration references and bounded reverse-callgraph evidence from an explicitly bound analysis-lite MCP session.
---

Use native file reads and text search for exact lookups and local edits. Use
this optional MCP surface when declaration references or a bounded impact query
would replace a wider scan. Tool availability does not require using it.

A stdio server binds each call on its own: an absolute target.file binds the
file's repository, anywhere; a relative target.file resolves under the launch
directory's repository (CLAUDE_PROJECT_DIR, else its working directory) and
returns project_binding_required when there is none, as in the home
directory. Only a marked, non-cloud repository binds.
freshness.runtime_role=reader means another session owns the index:
generation is null and pins are refused. An HTTP session must be bound to
the canonical workspace through the initialize project, endpoint URL project or
x-codelens-project header. Either way, verify the response project before
using evidence. Both tools are deferred: find them through host tool search.
This profile exposes only:

```json
{"name":"search","arguments":{"mode":"refs","target":{"file":"src/service.ts","name":"handle"},"max_results":20}}
{"name":"graph","arguments":{"mode":"impact","target":{"file":"src/service.ts","name":"handle"},"max_depth":3,"max_nodes":30,"max_edges":40}}
```

If the name is ambiguous, select one returned candidate using target.symbol_id
or target.start_byte/end_byte, the exact name-token UTF-8 byte range. These
offsets and response columns are not UTF-16 positions. Do not merge same-name
declarations across scopes. An indexed symbol ID requires matching source
content; a returned candidate ID identifies its exact source span.

Read status, scope, provenance, unresolved, errors, truncation and freshness.
Local semantic references and resolved graph evidence are combined; cross-file
non-call/type/import references remain incomplete. Class/interface/property
members and dynamic dispatch can be unsupported. A callgraph_edge may be a
callback; detail_unknown does not prove a direct call. Zero rows do not prove
unused. Stable index generation does not prove disk freshness.

Use the generation value to pin follow-up reads when needed. On a generation
conflict, retry once against current evidence; on repeated conflicts or
unsupported/error results, continue with native evidence and report the gap.
Narrow scope or caps on response_size_limit. Other modes, aliases, mutations
and bootstrap/control tools are rejected on this profile. No global activation
or second project writer is part of this skill.
