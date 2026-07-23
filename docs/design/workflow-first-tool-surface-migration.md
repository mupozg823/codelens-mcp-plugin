# Workflow-First Tool Surface Migration — 100 → 20 (ADR-0016)

Disposition of every registered tool at HEAD `91794a0f` (100 entries, `tools.toml`).

Legend — **CORE-10**: always loaded · **CORE-20**: static default surface ·
**alias→X**: hidden alias for one compatibility release, then removed on telemetry ·
**profile**: visible only in the named profile · **generic**: generic-client
compatibility profile only · **admin/CLI**: moves out of MCP surface ·
**remove**: deleted after deprecation telemetry satisfies the removal gate.

| Tool | Cat | Disposition |
|---|---|---|
| prepare_harness_session | session | **CORE-10** (bind + health + capabilities only; surface/effort policy moves out per ADR-0015) |
| search | symbol | **CORE-10** |
| overview | symbol | **CORE-10** |
| graph | analysis | **CORE-10** |
| diagnose | lsp | **CORE-10** |
| review | workflow_first | **CORE-10** |
| plan_safe_refactor | workflow_first | **CORE-10** |
| verify_change_readiness | composite | **CORE-10** |
| get_changed_files | analysis | **CORE-10** |
| get_current_config | file_io | **CORE-10** |
| find_symbol | symbol | **CORE-20** (PTC-friendly precise fetch) |
| get_ranked_context | symbol | **CORE-20** (evidence pack; array/cursor/snapshot per ADR-0016 §6) |
| find_referencing_symbols | lsp | **CORE-20** |
| semantic_search | semantic | **CORE-20** (only when semantic feature active) |
| refresh_symbol_index | symbol | **CORE-20** |
| get_watch_status | session | **CORE-20** (freshness evidence) |
| start_analysis_job | composite | **CORE-20** (MCP Tasks adapter later, ADR pending Q1'27) |
| get_analysis_job | composite | **CORE-20** |
| cancel_analysis_job | composite | **CORE-20** |
| get_analysis_section | composite | **CORE-20** |
| get_symbols_overview | symbol | alias→overview |
| bm25_symbol_search | symbol | alias→search |
| search_symbols_fuzzy | symbol | alias→search |
| search_workspace_symbols | lsp | alias→search |
| resolve_symbol_target | lsp | alias→search |
| find_declaration | lsp | alias→search |
| find_implementations | lsp | alias→graph |
| get_type_hierarchy | lsp | alias→graph(types) |
| get_callers | analysis | alias→graph(callers) |
| get_callees | analysis | alias→graph(callees) |
| find_scoped_references | analysis | alias→graph(refs) |
| trace_request_path | workflow_first | alias→graph(trace) |
| impact_report | composite | alias→graph(impact) |
| diff_aware_references | composite | alias→graph(diff-refs) |
| get_file_diagnostics | lsp | alias→diagnose(file) |
| get_diagnostics_for_symbol | lsp | alias→diagnose(symbol) |
| unresolved_reference_check | composite | alias→diagnose(unresolved) |
| diagnose_issues | workflow_first | alias→diagnose(issues) |
| review_architecture | workflow_first | alias→review(architecture) |
| review_changes | workflow_first | alias→review(changes) |
| module_boundary_report | composite | alias→review(boundary) |
| dead_code_report | composite | alias→review(dead) |
| cleanup_duplicate_logic | workflow_first | alias→review(dupes) |
| find_code_duplicates | semantic | alias→review(dupes) |
| find_similar_code | semantic | alias→review(similar) |
| find_misplaced_code | semantic | alias→review(misplaced) |
| mermaid_module_graph | composite | alias→review(architecture, include_diagram) |
| safe_rename_report | composite | alias→plan_safe_refactor |
| plan_symbol_rename | lsp | alias→plan_safe_refactor |
| refactor_safety_report | composite | alias→plan_safe_refactor |
| analyze_change_request | composite | alias→review(changes)+plan_safe_refactor |
| explore_codebase | workflow_first | alias; workflow moves to `explore-impact` skill |
| onboard_project | composite | alias→overview+review composite; skill-driven |
| analyze | analysis | alias→review/diagnose (redundant umbrella) |
| find_tests | file_io | alias→search(tests lane) |
| list_analysis_jobs | composite | alias→get_analysis_job(list) |
| list_analysis_artifacts | composite | alias→get_analysis_job(artifacts) |
| activate_project | session | alias→prepare_harness_session |
| get_capabilities | session | alias→prepare_harness_session (returned inline) |
| get_complexity | symbol | profile: reviewer-graph |
| get_symbol_importance | analysis | profile: reviewer-graph |
| classify_symbol | semantic | profile: reviewer-graph |
| audit_builder_session | session | profile: ci-audit |
| audit_planner_session | session | profile: ci-audit |
| audit_log_query | session | profile: ci-audit |
| audit_tool_surface_consistency | session | profile: ci-audit (also CI script) |
| find_phantom_modules | session | profile: ci-audit |
| find_redundant_definitions | session | profile: ci-audit |
| find_over_visible_apis | session | profile: ci-audit |
| add_queryable_project | session | experimental gate (secondary-projects), unlisted |
| remove_queryable_project | session | experimental gate, unlisted |
| query_project | session | experimental gate, unlisted |
| list_queryable_projects | session | experimental gate, unlisted |
| set_preset | session | mgmt, unlisted (host adapters only) |
| set_profile | session | mgmt, unlisted |
| read_file | file_io | generic profile only (host-native elsewhere) |
| list_dir | file_io | generic profile only |
| find_file | file_io | generic profile only |
| find_annotations | file_io | remove (host grep covers) |
| get_lsp_recipe | lsp | remove |
| get_tool_metrics | session | admin/CLI |
| prune_index_failures | session | admin/CLI |
| export_session_markdown | session | admin/CLI |
| embedding_coverage_report | semantic | admin/CLI |
| index_embeddings | semantic | admin/CLI (auto-managed by prepare/jobs) |
| orchestrate_change | composite | **remove** — hosts own orchestration (ADR-0015) |
| register_agent_work | session | **remove** — ADR-0018 §3 (deprecation telemetry first) |
| list_active_agents | session | **remove** — ADR-0018 §3 |
| claim_files | session | **remove** — ADR-0018 §3 |
| release_files | session | **remove** — ADR-0018 §3 |
| audit_memory_consistency | session | **remove** (memory family) |
| list_memories | memory | **remove** (approved 2026-07-21; agent memory belongs to hosts — Serena-absorption remnant) |
| read_memory | memory | **remove** (approved 2026-07-21) |
| write_memory | memory | **remove** (approved 2026-07-21) |
| delete_memory | memory | **remove** (approved 2026-07-21) |
| rename_memory | memory | **remove** (approved 2026-07-21) |
| archive_memory | memory | **remove** (approved 2026-07-21) |
| restore_memory | memory | **remove** (approved 2026-07-21) |
| list_archived | memory | **remove** (approved 2026-07-21) |
| read_policy | memory | **remove** (approved 2026-07-21) |

Counts (sum = 100): CORE-20 = 20 (of which CORE-10 = 10) · alias = 39 ·
profile-gated = 10 · experimental/mgmt/generic = 9 · admin/CLI = 5 ·
remove = 17, including the 9 memory-family entries (removal **approved 2026-07-21**). Mutation tools (`rename_symbol` family) are not in
`tools.toml`'s public set at this HEAD and enter only via the Q2'27 transaction ADR.
