# Issue-localization baseline

- Generated: `2026-08-08T13:11:28+00:00`
- Repo HEAD: `6b8e1cb60ff1922f518532678dede0bbc5e30208`
- Daemon: `http://127.0.0.1:7838` (clientInfo.name=`claude-code`)
- Dataset: 16 queries (8 verbatim / 8 descriptive)

## Ground-truth rules

- member addition is attributed to the enclosing container and the file, never to a synthetic function
- import and top-level static/const edits are file-level only
- comment-only and doc-comment-only hunks are ignored
- commits whose gold patch is entirely file creation or deletion are excluded; created/deleted files inside a mixed commit are dropped from the gold set
- commits with any gold file absent from HEAD are excluded

**HEAD-state deviation.** The runner scores against the daemon index of the current HEAD, not the paper's pre-PR snapshot. Gold locations therefore refer to symbols as they exist after the fix landed.

## Scores

| surface | class | granularity | precision | recall | F1 | scored q |
| --- | --- | --- | --- | --- | --- | --- |
| analyze_change_request | all | files | 0.177 | 0.417 | 0.240 | 16 |
| analyze_change_request | all | containers | 0.000 | 0.000 | 0.000 | 8 |
| analyze_change_request | all | functions | 0.156 | 0.269 | 0.176 | 15 |
| analyze_change_request | verbatim | files | 0.208 | 0.458 | 0.271 | 8 |
| analyze_change_request | verbatim | containers | 0.000 | 0.000 | 0.000 | 3 |
| analyze_change_request | verbatim | functions | 0.238 | 0.524 | 0.309 | 7 |
| analyze_change_request | descriptive | files | 0.146 | 0.375 | 0.208 | 8 |
| analyze_change_request | descriptive | containers | 0.000 | 0.000 | 0.000 | 5 |
| analyze_change_request | descriptive | functions | 0.083 | 0.046 | 0.059 | 8 |
| search_ranked | all | files | 0.111 | 0.542 | 0.175 | 16 |
| search_ranked | all | containers | 0.000 | 0.000 | 0.000 | 8 |
| search_ranked | all | functions | 0.066 | 0.302 | 0.100 | 15 |
| search_ranked | verbatim | files | 0.111 | 0.500 | 0.170 | 8 |
| search_ranked | verbatim | containers | 0.000 | 0.000 | 0.000 | 3 |
| search_ranked | verbatim | functions | 0.092 | 0.571 | 0.154 | 7 |
| search_ranked | descriptive | files | 0.111 | 0.583 | 0.180 | 8 |
| search_ranked | descriptive | containers | 0.000 | 0.000 | 0.000 | 5 |
| search_ranked | descriptive | functions | 0.043 | 0.067 | 0.053 | 8 |

| surface | class | file hit@1 | file hit@3 | queries |
| --- | --- | --- | --- | --- |
| analyze_change_request | all | 0.062 | 0.500 | 16 |
| analyze_change_request | verbatim | 0.125 | 0.625 | 8 |
| analyze_change_request | descriptive | 0.000 | 0.375 | 8 |
| search_ranked | all | 0.062 | 0.562 | 16 |
| search_ranked | verbatim | 0.125 | 0.625 | 8 |
| search_ranked | descriptive | 0.000 | 0.500 | 8 |

## Per-query misses

### analyze_change_request

- `Q01` (verbatim, `00a6289a`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/tool_defs/presets.rs']; predicted top-3 ['crates/codelens-engine/src/symbols/writer.rs', 'crates/codelens-mcp/src/tools/admin/mod.rs']
- `Q03` (verbatim, `44edfce4`) — right module, wrong file. Missing ['crates/codelens-mcp/src/artifact_store.rs', 'crates/codelens-mcp/src/job_store.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/symbol_query/retrieval_scope.rs', 'crates/codelens-mcp/src/util.rs', 'crates/codelens-engine/src/embedding/duplicates.rs']
- `Q04` (verbatim, `da621489`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/artifact_store.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/reports/impact_reports/boundary.rs', 'crates/codelens-mcp/src/tool_defs/mod.rs', 'crates/codelens-mcp/src/tool_defs/build.rs']
- `Q06` (verbatim, `d3cb909f`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/tool_defs/generated/build_generated.rs', 'crates/codelens-mcp/src/tools/report_jobs.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/reports/impact_reports/refactor.rs', 'crates/codelens-mcp/src/tools/report_jobs/runners.rs', 'crates/codelens-mcp/src/tools/reports/impact_reports/boundary.rs']
- `Q07` (verbatim, `45b76f38`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/integration_tests/workflow/harness.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/session/metrics_config/health.rs', 'crates/codelens-mcp/src/tools/session/project_ops/prepare_harness/warnings/index.rs', 'crates/codelens-engine/src/memory/policy.rs']
- `Q09` (descriptive, `cf8e4e63`) — different subsystem entirely. Missing ['crates/codelens-mcp/src/agent_coordination.rs']; predicted top-3 ['crates/codelens-engine/src/db/mod.rs', 'crates/codelens-engine/src/symbols/mod.rs']
- `Q13` (descriptive, `1296b90f`) — right crate, wrong module. Missing ['crates/codelens-engine/src/project.rs']; predicted top-3 ['crates/codelens-engine/src/embedding/vec_store.rs', 'crates/codelens-engine/src/embedding/engine_impl/index.rs']
- `Q14` (descriptive, `ac7aed70`) — different subsystem entirely. Missing ['crates/codelens-mcp/src/job_store.rs']; predicted top-3 ['crates/codelens-engine/src/symbols/writer.rs', 'crates/codelens-engine/src/embedding/engine_impl/coverage.rs', 'crates/codelens-engine/src/embedding/engine_impl.rs']
- `Q15` (descriptive, `9ba6ae9a`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/integration_tests/workflow/session.rs']; predicted top-3 ['crates/codelens-engine/src/call_graph/tests.rs', 'crates/codelens-mcp/src/skill_catalog/tests.rs', 'crates/codelens-mcp/src/surface_manifest/tests.rs']
- `Q16` (descriptive, `b9e79e30`) — right module, wrong file. Missing ['crates/codelens-mcp/src/integration_tests/workflow/mod.rs', 'crates/codelens-mcp/src/tools/reports/impact_reports/mermaid.rs', 'crates/codelens-mcp/src/tools/reports/impact_reports/mod.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/reports/impact_reports/workspace_modules/build.rs', 'crates/codelens-mcp/src/tools/reports/impact_reports/workspace_modules.rs']

### search_ranked

- `Q01` (verbatim, `00a6289a`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/tool_defs/presets.rs']; predicted top-3 ['crates/codelens-engine/src/symbols/writer.rs', 'crates/codelens-mcp/src/tools/admin/mod.rs', 'crates/codelens-mcp/src/tools/session/surface_mutation.rs']
- `Q03` (verbatim, `44edfce4`) — right module, wrong file. Missing ['crates/codelens-mcp/src/artifact_store.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/symbol_query/retrieval_scope.rs', 'crates/codelens-mcp/src/util.rs', 'crates/codelens-mcp/src/job_store.rs']
- `Q04` (verbatim, `da621489`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/artifact_store.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/reports/impact_reports/boundary.rs', 'crates/codelens-mcp/src/tool_defs/mod.rs', 'crates/codelens-mcp/src/tool_defs/build.rs']
- `Q06` (verbatim, `d3cb909f`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/tool_defs/generated/build_generated.rs', 'crates/codelens-mcp/src/tools/report_jobs.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/reports/impact_reports/refactor.rs', 'crates/codelens-mcp/src/tools/report_jobs/runners.rs', 'crates/codelens-mcp/src/tools/reports/impact_reports/boundary.rs']
- `Q07` (verbatim, `45b76f38`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/integration_tests/workflow/harness.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/session/metrics_config/health.rs', 'crates/codelens-mcp/src/tools/session/project_ops/prepare_harness/warnings/index.rs', 'crates/codelens-engine/src/memory/policy.rs']
- `Q09` (descriptive, `cf8e4e63`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/agent_coordination.rs']; predicted top-3 ['crates/codelens-engine/src/db/mod.rs', 'crates/codelens-engine/src/symbols/mod.rs', 'crates/codelens-mcp/src/state/project_runtime_lease.rs']
- `Q13` (descriptive, `1296b90f`) — right crate, wrong module. Missing ['crates/codelens-engine/src/project.rs']; predicted top-3 ['crates/codelens-engine/src/embedding/vec_store.rs', 'crates/codelens-engine/src/embedding/engine_impl/index.rs', 'scripts/artifact_maintenance.py']
- `Q15` (descriptive, `9ba6ae9a`) — right crate, wrong module. Missing ['crates/codelens-mcp/src/integration_tests/workflow/session.rs']; predicted top-3 ['crates/codelens-engine/src/call_graph/tests.rs', 'crates/codelens-mcp/src/skill_catalog/tests.rs', 'crates/codelens-mcp/src/surface_manifest/tests.rs']
- `Q16` (descriptive, `b9e79e30`) — right module, wrong file. Missing ['crates/codelens-mcp/src/tools/reports/impact_reports/mermaid.rs']; predicted top-3 ['crates/codelens-mcp/src/tools/reports/impact_reports/workspace_modules/build.rs', 'crates/codelens-mcp/src/tools/reports/impact_reports/workspace_modules.rs', 'crates/codelens-mcp/src/integration_tests/workflow/mod.rs']

## Surface response shape

| surface | truncated q | stage-5 q | median parsed items |
| --- | --- | --- | --- |
| analyze_change_request | 0 | 0 | 9 |
| search_ranked | 0 | 0 | 18 |

