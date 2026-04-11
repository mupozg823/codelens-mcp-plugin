# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-engine/models/codesearch/model.onnx`
- Dataset size: 436
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.046 | 4% | 5% | 6% | 231.1 |
| get_ranked_context_no_semantic | 0.041 | 3% | 4% | 5% | 31.6 |
| get_ranked_context | 0.048 | 4% | 5% | 6% | 112.2 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 83 | 0.096 | 10% | 10% | 10% | 128.7 |
| semantic_search | natural_language | 299 | 0.037 | 3% | 4% | 5% | 261.4 |
| semantic_search | short_phrase | 54 | 0.021 | 2% | 2% | 2% | 220.5 |
| get_ranked_context_no_semantic | identifier | 83 | 0.096 | 10% | 10% | 10% | 22.9 |
| get_ranked_context_no_semantic | natural_language | 299 | 0.032 | 2% | 4% | 4% | 34.0 |
| get_ranked_context_no_semantic | short_phrase | 54 | 0.005 | 0% | 0% | 2% | 31.3 |
| get_ranked_context | identifier | 83 | 0.096 | 10% | 10% | 10% | 23.2 |
| get_ranked_context | natural_language | 299 | 0.038 | 3% | 4% | 5% | 133.7 |
| get_ranked_context | short_phrase | 54 | 0.028 | 2% | 4% | 4% | 129.8 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.007 |
| Acc@1 uplift | +1% |
| Acc@3 uplift | +1% |
| Acc@5 uplift | +0% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.005 | +1% | +0% | +0% |
| short_phrase | +0.023 | +2% | +4% | +2% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | rename a variable or function across the project | miss | rename (crates/codelens-engine/src/lib.rs) |
| semantic_search | find where a symbol is defined in a file | miss | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | apply text edits to multiple files | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| semantic_search | search code by natural language query | miss | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | inline a function and remove its definition | miss | inline_function (crates/codelens-engine/src/inline.rs) |
| semantic_search | move code to a different file | miss | dead_code_report (crates/codelens-mcp/src/tools/reports/impact_reports.rs) |
| semantic_search | change function parameters | miss | changed_files_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | find circular import dependencies | miss | find_circular_dependencies (crates/codelens-engine/src/circular.rs) |
| semantic_search | read input from stdin line by line | miss | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| semantic_search | parse source code into an AST | miss | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| semantic_search | build embedding vectors for all symbols | miss | build_embedding_text (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | find near-duplicate code in the codebase | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | categorize a function by its purpose | miss | natural_language_kind_prior_prefers_functions_over_types (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | get project structure and key files on first load | 8 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | watch filesystem for file changes | miss | FileWatcher (crates/codelens-engine/src/watcher.rs) |
| semantic_search | skip comments and string literals during search | miss | engine_search_returns_results (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | compute similarity between two vectors | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | rename_symbol | miss | rename_symbol (crates/codelens-engine/src/rename.rs) |
| semantic_search | cosine_similarity | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | find_circular_dependencies | miss | find_circular_dependencies (crates/codelens-engine/src/circular.rs) |
| semantic_search | how to build embedding text from a symbol | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | mutation gate preflight check before editing | 4 | MutationFailureKind (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | exclude directories from indexing | miss | from (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | record which files were recently accessed | miss | recent_file_paths (crates/codelens-mcp/src/state.rs) |
| semantic_search | EmbeddingEngine | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | find all functions that call a given function | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | filter out standard library noise from call graph | miss | is_noise_callee (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | resolve which file a called function belongs to | miss | summarize_file (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | CallEdge | miss | CallEdge (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | CallerEntry | miss | CallerEntry (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | detect communities and clusters in the codebase | miss | detect_communities (crates/codelens-engine/src/community.rs) |
| semantic_search | calculate modularity score for graph partitioning | miss | empty_graph_returns_empty (crates/codelens-engine/src/community.rs) |
| semantic_search | find common path prefix for files in a community | miss | common_path_prefix (crates/codelens-engine/src/community.rs) |
| semantic_search | measure density of internal edges in a cluster | miss | add_import_inserts_at_correct_position (crates/codelens-engine/src/auto_import.rs) |
| semantic_search | ArchitectureOverview | miss | ArchitectureOverview (crates/codelens-engine/src/community.rs) |
| semantic_search | register sqlite vector extension for similarity search | miss | search_dual (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | split camelCase or snake_case identifier into words | miss | split_camel_case (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | SemanticMatch | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | SqliteVecStore | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | validate that a new name is a valid identifier | miss | validate_identifier (crates/codelens-engine/src/rename.rs) |
| semantic_search | find all occurrences of a word in project files | miss | count_word_occurrences (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | collect rename edits scoped to a single file | miss | collect_file_scope_edits (crates/codelens-engine/src/rename.rs) |
| semantic_search | RenameEdit | miss | RenameEdit (crates/codelens-engine/src/rename.rs) |
| semantic_search | RenameScope | miss | RenameScope (crates/codelens-engine/src/rename.rs) |
| semantic_search | get overview of all symbols in a file | miss | get_symbols_overview (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | search for a symbol by name | miss | find_symbols_by_name (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | get project directory tree structure | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | SymbolIndex | miss | SymbolIndex (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| semantic_search | how does the embedding engine initialize the model | miss | configured_embedding_model_name (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | determine which language config to use for a file | miss | LanguageConfig (crates/codelens-engine/src/lang_config.rs) |
| semantic_search | select the most relevant symbols for a query | miss | select_solve_symbols (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | find similar code snippets using embeddings | 8 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | classify what kind of symbol this is | 4 | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | collect rename edits across the entire project | miss | collect_project_scope_edits (crates/codelens-engine/src/rename.rs) |
| semantic_search | upsert embedding vector for a symbol | miss | upsert (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | find all word matches in a single file | miss | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| semantic_search | get current timestamp in milliseconds | 8 | matches_scope (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | ProjectOverride | miss | project (crates/codelens-mcp/src/state.rs) |
| semantic_search | Community | miss | Community (crates/codelens-engine/src/community.rs) |
| semantic_search | RenameResult | miss | RenameResult (crates/codelens-engine/src/rename.rs) |
| semantic_search | CallLanguageConfig | miss | CallLanguageConfig (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | CalleeEntry | miss | CalleeEntry (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | handle user authentication with JWT tokens | miss | mermaid_handles_missing_file_field_gracefully (crates/codelens-mcp/src/tools/reports/impact_reports.rs) |
| semantic_search | create a custom React hook for fetching data | miss | expired (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | validate form input fields before submission | miss | configured_coreml_model_format_name (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | generate a signed URL for S3 file upload | miss | generation (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | middleware to protect API routes from unauthenticated access | miss | references_from_response (crates/codelens-engine/src/lsp/parsers.rs) |
| semantic_search | send a transactional email using a template | miss | ArtifactSpec (scripts/artifact_maintenance.py) |
| semantic_search | paginate database query results | miss | search_scored (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | refresh access token using refresh token | miss | refresh_file (crates/codelens-engine/src/symbols/writer.rs) |
| semantic_search | debounce a search input handler | miss | prefers_dispatch_entrypoint_over_handler_types (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | connect to PostgreSQL with connection pooling | miss | find_symbols_with_path (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | rate limit API requests per IP address | miss | percentile_95 (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | server-side render a Next.js page with user data | miss | next_id (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | upload image and resize to multiple dimensions | miss | mermaid_renders_upstream_and_downstream_edges (crates/codelens-mcp/src/tools/reports/impact_reports.rs) |
| semantic_search | handle Stripe webhook events | miss | telemetry_writer_appends_multiple_events_in_order (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | create a subscription checkout session | miss | active_session_count (crates/codelens-mcp/src/state.rs) |
| semantic_search | manage global state with Zustand store | miss | store_analysis_job_for_current_scope (crates/codelens-mcp/src/state.rs) |
| semantic_search | infinite scroll hook for loading more items | miss | store_can_fetch_single_embedding_without_loading_all (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | format currency values for display | miss | clone_for_worker (crates/codelens-mcp/src/state.rs) |
| semantic_search | generate a random slug from a title string | miss | build_graph_from_db (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | transform API response to camelCase | miss | build_error_response (crates/codelens-mcp/src/dispatch_response.rs) |
| semantic_search | parse and validate environment variables at startup | miss | parser (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | cache API responses in Redis | miss | extract_api_calls_returns_none_when_body_has_no_type_calls (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | handle file drag and drop in a React component | miss | find_similar_code_handler (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | update user profile in the database | miss | set_profile (crates/codelens-mcp/src/tools/session/metrics_config.rs) |
| semantic_search | generate OTP for two-factor authentication | miss | generate_import_statement (crates/codelens-engine/src/auto_import.rs) |
| semantic_search | SSR data fetching for static site generation | miss | is_static_method_ident (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | intercept Axios requests to add auth headers | miss | mcp_get_handler (crates/codelens-mcp/src/server/transport_http.rs) |
| semantic_search | export data to CSV file from a table | miss | build_graph_from_files (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | sort array of objects by a given key | miss | get_imports_of (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | toast notification system for user feedback | miss | clone_for_worker (crates/codelens-mcp/src/state.rs) |
| semantic_search | track user analytics events | miss | telemetry_writer_appends_multiple_events_in_order (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | deep merge two configuration objects | miss | short_phrase_merge_only_inserts_top_confident_semantic_hit (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | create a typed API route handler in Next.js | miss | find_misplaced_code_handler (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | hash a password with bcrypt | miss | store_can_fetch_single_embedding_without_loading_all (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | lazy load a React component | miss | store_can_fetch_single_embedding_without_loading_all (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | handle SSE streaming response from server | miss | slim_text_payload_for_async_handle (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | build a reusable modal dialog component | miss | build (crates/codelens-mcp/src/state.rs) |
| semantic_search | check user subscription plan and permissions | miss | rename_plan_from_response (crates/codelens-engine/src/lsp/parsers.rs) |
| semantic_search | implement optimistic UI updates in a React mutation | miss | MutationFailureKind (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | internationalize strings with i18n locale | miss | new_with_writer (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | ApiResponse | miss |  |
| semantic_search | UserProfile | miss | User (crates/codelens-engine/tests/fixtures/sample_project/src/models.ts) |
| semantic_search | AuthContext | miss | authority (crates/codelens-mcp/src/main.rs) |
| semantic_search | PaginationMeta | miss | passing_candidates (scripts/finetune/promotion_gate.py) |
| semantic_search | upload a file to cloud storage and return its URL | miss | record_and_snapshot (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | parse markdown content to HTML | miss | parse_tool_response (crates/codelens-mcp/src/integration_tests.rs) |
| semantic_search | map tRPC router endpoints to handlers | miss | copy_summarized_field (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | build Prisma query filters from request params | miss | build (crates/codelens-mcp/src/state.rs) |
| semantic_search | retry a failed API call with exponential backoff | miss | test_extract_call_args (crates/codelens-engine/src/inline.rs) |
| semantic_search | scroll restoration hook for page navigation | miss | configured_coreml_model_format_name (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | sanitize user-generated HTML content | miss | content_cache_key (crates/codelens-engine/src/auto_import.rs) |
| semantic_search | generate a unique ID for a new entity | miss | read_response_for_id (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | configure Next.js Image component with allowed domains | miss | next_id (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | StreamingTextResponse | miss | stat (scripts/finetune/finetune_distill.py) |
| semantic_search | SubscriptionPlan | miss | subset (scripts/finetune/finetune_distill.py) |
| semantic_search | create a serverless API route to handle POST requests | miss | find_similar_code_handler (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | build React Server Component for a data table | miss | build (crates/codelens-mcp/src/state.rs) |
| semantic_search | flatten a nested array structure | miss | flatten_symbols (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | theme switching between light and dark mode | miss | extract_comment_body_rejects_rust_attributes_and_shebangs (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | run database migrations on application startup | miss | text_only (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | handle OAuth callback and exchange code for tokens | miss | semantic_search_handler (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | transform image buffer to WebP format | miss | fmt (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | compute reading time estimate from article text | miss | timeout_secs (crates/codelens-mcp/src/server/session.rs) |
| semantic_search | create a feature flag check function | miss | create_text_file_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | log structured JSON errors to external service | miss | key (crates/codelens-mcp/src/preflight_store.rs) |
| semantic_search | validate and parse a UUID string | miss | parse_commits (crates/codelens-engine/src/coupling.rs) |
| semantic_search | RouteConfig | miss | routing (benchmarks/harness/harness_runner_common.py) |
| semantic_search | useLocalStorage | miss | uses (.github/workflows/release.yml) |
| semantic_search | buildMetadata | miss | build (crates/codelens-mcp/src/state.rs) |
| semantic_search | protect a route by checking user role | miss | all_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | batch insert multiple records into the database | miss | insert_batch (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | handle websocket connection lifecycle | miss | status_to_score (crates/codelens-mcp/src/tools/report_payload.rs) |
| semantic_search | get current authenticated user from session | miss | get (crates/codelens-mcp/src/server/session.rs) |
| semantic_search | compress and decompress JSON payload | miss | json_resource (crates/codelens-mcp/src/resources.rs) |
| semantic_search | register a service worker for offline support | miss | record_file_access_for_session (crates/codelens-mcp/src/state.rs) |
| semantic_search | run background job with a queue processor | miss | run_analysis_job_from_queue (crates/codelens-mcp/src/tools/report_jobs.rs) |
| semantic_search | create breadcrumb navigation from URL path | miss | from (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | calculate Stripe proration for plan upgrade | miss | project_scope_for_session (crates/codelens-mcp/src/state.rs) |
| semantic_search | fetch paginated list of posts with cursor | miss | find_symbols_with_path (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | CursorPaginationInput | miss |  |
| semantic_search | withErrorBoundary | miss | with (.github/workflows/release.yml) |
| semantic_search | decode a JWT token without verifying the signature | miss | sparse_query_tokens_drops_stopwords_and_short_tokens (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | group array items by a property value | miss | store_streams_embeddings_grouped_by_file (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | check if a date is within a valid range | miss | is_allowed_lsp_command (crates/codelens-engine/src/lsp/mod.rs) |
| semantic_search | define a FastAPI route with path and query parameters | miss | sparse_query_tokens_drops_stopwords_and_short_tokens (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | create a Pydantic model for request body validation | miss | analyze_change_request (crates/codelens-mcp/src/tools/reports/context_reports.rs) |
| semantic_search | run Celery background task asynchronously | miss | find_task (benchmarks/harness/session-pack.py) |
| semantic_search | connect to database using SQLAlchemy async session | miss | dir_stats (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | authenticate user and return JWT access token | miss | engine_search_returns_results (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | verify a JWT token and extract claims | miss | assert_extracts (crates/codelens-engine/src/lang_config.rs) |
| semantic_search | hash a password using bcrypt | miss | type_hierarchy_child_to_map (crates/codelens-engine/src/lsp/parsers.rs) |
| semantic_search | define a Django ORM model with relationships | miss | store_can_fetch_single_embedding_without_loading_all (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | serialize Django model to JSON response | miss | text_payload_for_response (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | run Django database migration for a new field | miss | new (crates/codelens-mcp/src/job_store.rs) |
| semantic_search | paginate a Django REST Framework queryset | miss | deferred_tools_list_can_restore_output_schema_explicitly (crates/codelens-mcp/src/integration_tests.rs) |
| semantic_search | send a webhook notification on model save | miss | send_request (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | write a pytest fixture for database session | miss | write_to_disk (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | upload a file in FastAPI using UploadFile | miss | CS_USING_RE (crates/codelens-engine/src/import_graph/parsers.rs) |
| semantic_search | parse CSV file into a list of dicts | miss | sparse_weighting_gated_off_by_default (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | apply rate limiting to a FastAPI endpoint | miss | validate_tool_access (crates/codelens-mcp/src/dispatch_access.rs) |
| semantic_search | connect to Redis cache and set a value | miss | engine_new_and_index (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | bulk insert records into PostgreSQL | miss | merged_string_set (crates/codelens-mcp/src/server/router.rs) |
| semantic_search | implement a repository pattern for database access | miss | project_scope_for_arguments (crates/codelens-mcp/src/state.rs) |
| semantic_search | create a custom exception handler in FastAPI | miss | prefers_dispatch_entrypoint_over_handler_types (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | load configuration from environment variables | miss | extract_types_from_node (crates/codelens-engine/src/type_hierarchy.rs) |
| semantic_search | implement a Python context manager for a transaction | miss | get_analysis_for_scope (crates/codelens-mcp/src/state.rs) |
| semantic_search | filter Django queryset by date range | miss | semantic_low_scores_filtered_out (crates/codelens-engine/src/search.rs) |
| semantic_search | define GraphQL mutation to create a user | miss | create_or_resume (crates/codelens-mcp/src/server/session.rs) |
| semantic_search | run a subprocess command and capture output | miss | default_lsp_args_for_command (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | decrypt a message using AES encryption | miss | error (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | load a machine learning model from a file | miss | embedding_model_assets_available (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | preprocess text for NLP pipeline | miss | slim_text_payload_for_async_handle (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | stream large file response without loading into memory | miss | write_memory (crates/codelens-engine/src/memory.rs) |
| semantic_search | register a FastAPI dependency injection provider | miss | build_coreml_execution_provider (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | configure CORS allowed origins in FastAPI | miss | engine_incremental_index (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | TokenData | miss | token (scripts/finetune/build_runtime_training_pipeline.py) |
| semantic_search | UserInDB | miss | User (crates/codelens-engine/tests/fixtures/sample_project/src/models.ts) |
| semantic_search | check if user has permission to access a resource | miss | text_resource (crates/codelens-mcp/src/resources.rs) |
| semantic_search | extract features from raw dataset for training | miss | extract_body_hint_returns_none_for_empty (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | write a pytest parametrize test for multiple inputs | miss | write_to_disk (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | handle async generator for streaming responses | miss | preflight_key_for_session (crates/codelens-mcp/src/state.rs) |
| semantic_search | retry a network request with exponential backoff | miss | context_request_client_profile (crates/codelens-mcp/src/resource_catalog.rs) |
| semantic_search | convert a dataclass to a dict recursively | miss | get_agent (benchmarks/harness/agent_registry.py) |
| semantic_search | import data from external API and normalize schema | miss | add_import_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | generate PDF report from HTML template | miss | from (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | validate and sanitize user input against a schema | miss | with_output_schema (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | BaseRepository | miss | base_result (scripts/finetune/compare_base_vs_v6.py) |
| semantic_search | CeleryConfig | miss |  |
| semantic_search | initialize SQLAlchemy ORM engine with connection string | miss | files_with_symbol_kinds (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | detect anomalies in time series data | miss | still_detects_project_root_before_home_directory (crates/codelens-engine/src/project.rs) |
| semantic_search | store and retrieve values from a Python LRU cache | miss | stats (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | implement Django custom management command | miss | start (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | test a FastAPI endpoint with the TestClient | miss | tests (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | create a custom Django admin action | miss | create_text_file_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | schedule a periodic task with Celery beat | miss | all_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | send a push notification to a mobile device | miss | send_notification (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | aggregate query results grouped by day | miss | results_sorted_by_score_descending (crates/codelens-engine/src/search.rs) |
| semantic_search | HealthCheck | miss | teacher (scripts/finetune/compress_to_3layer.py) |
| semantic_search | DatabaseError | miss | dataset (scripts/finetune/finetune_distill.py) |
| semantic_search | convert pandas DataFrame to a list of records | miss | list_queryable_projects (crates/codelens-mcp/src/tools/session/project_ops.rs) |
| semantic_search | read secrets from AWS Secrets Manager | miss | reset (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | encode a binary file as base64 string | miss | common_path_prefix (crates/codelens-engine/src/community.rs) |
| semantic_search | verify an HMAC signature on an incoming webhook | miss | list_analysis_summaries (crates/codelens-mcp/src/state.rs) |
| semantic_search | find the closest matching item using fuzzy search | miss | search_workspace_symbols (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | run data validation and return a list of errors | miss | record_and_snapshot (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | split a list into chunks of a given size | miss | sparse_weighting_gated_off_by_default (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | log a request and its response time as a middleware | miss | record_and_snapshot (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | compute TF-IDF score for document ranking | miss | semantic_adjusted_score_exposes_positive_prior_for_dispatch_entrypoint (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | create a dataclass for a configuration section | miss | sanitize_section_name (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | S3Client | miss |  |
| semantic_search | PushNotificationPayload | miss | push (.github/workflows/release.yml) |
| semantic_search | summarize a long text using an LLM | miss | store_analysis (crates/codelens-mcp/src/state.rs) |
| semantic_search | run a full-text search query on a database table | miss | onboarding (crates/codelens-mcp/src/tools/session/project_ops.rs) |
| semantic_search | track model training metrics per epoch | miss | error_count_tracked (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | handle HTTP request with context and timeout | miss | mcp_post_handler (crates/codelens-mcp/src/server/transport_http.rs) |
| semantic_search | connect to PostgreSQL database using pgx driver | miss | tools (crates/codelens-mcp/src/tool_defs/build.rs) |
| semantic_search | run a database transaction with rollback on error | miss | with_annotations (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | parse JSON request body into a struct | miss | parse (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | generate a JWT token for a user | miss | generation (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | validate a JWT token and return claims | miss | set_session_surface_and_budget (crates/codelens-mcp/src/state.rs) |
| semantic_search | implement middleware for request logging | miss | search_for_pattern (crates/codelens-engine/src/file_ops/reader.rs) |
| semantic_search | write a gRPC server handler for a service method | miss | classifies_write (crates/codelens-engine/src/scope_analysis.rs) |
| semantic_search | publish a message to a Kafka topic | miss | push (crates/codelens-mcp/src/recent_buffer.rs) |
| semantic_search | consume messages from a Kafka topic | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | implement a retry loop with exponential backoff | miss | with_path (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | parse command-line flags using the flag package | miss | parse_bool_flag (benchmarks/harness/session-eval.py) |
| semantic_search | gracefully shut down an HTTP server on signal | miss | server_card_handler (crates/codelens-mcp/src/server/transport_http.rs) |
| semantic_search | read configuration from YAML file | miss | build_graph_from_db (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | implement a worker pool for parallel processing | miss | get_analysis_for_scope (crates/codelens-mcp/src/state.rs) |
| semantic_search | write unit test with table-driven test cases | miss | file_scope_renames_within_symbol_body (crates/codelens-engine/src/rename.rs) |
| semantic_search | implement an LRU cache with expiration | miss | switch_project_reuses_cached_symbol_index_and_lsp_pool (crates/codelens-mcp/src/state.rs) |
| semantic_search | use sync.WaitGroup to wait for goroutines | miss | has_go_dependency (crates/codelens-engine/src/project.rs) |
| semantic_search | register routes on an HTTP mux with middleware | miss | with_pagerank_and_semantic (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | read a file line by line using bufio scanner | miss | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| semantic_search | encode a struct to JSON and write to response | miss | success_jsonrpc_response (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | connect to Redis and implement a simple set-get | miss | get_architecture_tool (crates/codelens-mcp/src/tools/graph.rs) |
| semantic_search | measure function execution latency with prometheus | miss | parse_function_parts (crates/codelens-engine/src/inline.rs) |
| semantic_search | Config | miss | config (benchmarks/harness/apply-routing-policy.py) |
| semantic_search | Server | miss | seen (scripts/finetune/build_runtime_training_pipeline.py) |
| semantic_search | Repository | miss | reports (scripts/finetune/promotion_gate.py) |
| semantic_search | implement OpenTelemetry trace span for a function | miss | record_file_access_for_session (crates/codelens-mcp/src/state.rs) |
| semantic_search | apply database migrations using goose | miss | apply_session_headers (crates/codelens-mcp/src/server/transport_http_support.rs) |
| semantic_search | marshal proto message to binary format | miss | configured_coreml_model_format_name (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | implement health check endpoint for Kubernetes | miss | infer_summary_recommended_checks (crates/codelens-mcp/src/resource_analysis.rs) |
| semantic_search | seed the database with initial test data | miss | registry_without_writer_is_noop_for_persistence (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | scan SQL rows into a slice of structs | miss | error (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | ErrNotFound | miss | error (benchmarks/harness/harness_runner_common.py) |
| semantic_search | handle an error by wrapping it with additional context | miss | error (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | validate a struct using validator tags | miss | annotate_ranked_context_provenance_marks_structural_and_semantic_entries (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | log a structured message with fields using zap | miss | all_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | create a mock implementation for an interface | miss | create_text_file_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | compute SHA256 hash of a string | miss | compute_file_sha256 (benchmarks/role-retrieval.py) |
| semantic_search | JobWorker | miss | job_store (crates/codelens-mcp/src/main.rs) |
| semantic_search | KafkaProducer | miss |  |
| semantic_search | serialize a struct to a map for dynamic querying | miss | get_project_structure (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | implement a circuit breaker for external service calls | miss | extract_calls_from_source (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | rate limit requests using a token bucket algorithm | miss | sparse_query_tokens_drops_stopwords_and_short_tokens (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | upload a file to GCS bucket | miss | get_callees_scoped_to_file (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | decode URL-encoded query parameters | miss | decode_embedding_bytes (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | define a Spring REST controller endpoint | miss | does_not_suggest_locally_defined (crates/codelens-engine/src/auto_import.rs) |
| semantic_search | create a Spring JPA repository interface | miss | expired (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | implement a Spring service layer with transaction | miss | semantic_adjusted_score_with_lower (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | configure Spring Security filter chain | miss | configured_embedding_text_cache_size (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | map a JPA entity to a database table | miss | validate_tool_access (crates/codelens-mcp/src/dispatch_access.rs) |
| semantic_search | convert entity to DTO for API response | miss | success_jsonrpc_response (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | handle global exceptions with Spring ControllerAdvice | miss | prefers_dispatch_entrypoint_over_handler_types (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | write a JUnit 5 test with mocked dependencies | miss | make_project_with_source (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | create a Spring Batch job for data processing | miss | create_text_file_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | publish a Spring application event and listen to it | miss | engine_new_and_index (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | configure Liquibase database changelog | miss | configured_embedding_model_name_defaults_to_codesearchnet (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | implement a JWT authentication filter | miss | filters_direct_test_symbols_from_embedding_index (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | create a Spring Cache configuration with Redis | miss | all_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | send an email with Spring Mail and Thymeleaf template | miss | extract_comment_body_rejects_rust_attributes_and_shebangs (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | validate request body with Bean Validation annotations | miss | extract_body_hint_skips_comments (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | UserServiceImpl | miss | UserService (crates/codelens-engine/tests/fixtures/sample_project/src/service.py) |
| semantic_search | JwtTokenProvider | miss |  |
| semantic_search | schedule a Spring task with a cron expression | miss | store_can_fetch_single_embedding_without_loading_all (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | implement an aspect for method-level logging | miss | is_static_method_ident (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | create a Spring WebClient for non-blocking HTTP calls | miss | create_initialize_session (crates/codelens-mcp/src/server/transport_http_support.rs) |
| semantic_search | write a custom Spring Health Indicator | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| semantic_search | PageableResponse | miss | parse (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | ApiException | miss |  |
| semantic_search | implement Flyway migration script to add a column | miss | column_precise_replacement (crates/codelens-engine/src/rename.rs) |
| semantic_search | publish a message to a RabbitMQ exchange | miss | push (crates/codelens-mcp/src/recent_buffer.rs) |
| semantic_search | consume messages from a RabbitMQ queue | miss | visible_axes_from_tools (crates/codelens-mcp/src/server/router.rs) |
| semantic_search | configure OpenAPI Swagger documentation for the API | miss | configured_embedding_model_name_defaults_to_codesearchnet (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | search records with dynamic JPA Specification | miss | with_path (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | RabbitMQConfig | miss | ratio (benchmarks/token-efficiency.py) |
| semantic_search | AuditListener | miss | audit_dir (crates/codelens-mcp/src/state.rs) |
| semantic_search | implement an async TCP server with Tokio | miss | find_symbols_with_path (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | deserialize JSON into a Rust struct with serde | miss | extracts_rust_brace_group_imports (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | define a custom error type with thiserror | miss | error (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | query a database row with sqlx and bind parameters | miss | engine_new_and_index (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | spawn a background task with Tokio handle | miss | prefers_dispatch_entrypoint_over_handler_types (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | implement the Display trait for a custom type | miss | build_type_map (crates/codelens-engine/src/type_hierarchy.rs) |
| semantic_search | write a property-based test with proptest | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | build a CLI argument parser with clap derive | miss | build (crates/codelens-mcp/src/state.rs) |
| semantic_search | implement an Axum route handler with state injection | miss | prefers_dispatch_entrypoint_over_handler_types (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | use a channel to communicate between threads | miss | recommended_embed_threads_caps_macos_style_load (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | create a connection pool with deadpool-postgres | miss | switch_project_reuses_cached_symbol_index_and_lsp_pool (crates/codelens-mcp/src/state.rs) |
| semantic_search | read and write a TOML configuration file | miss | TOML_QUERY (crates/codelens-engine/src/lang_config.rs) |
| semantic_search | implement a custom Serde deserializer for a field | miss | parse_lsp_args (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | verify HMAC-SHA256 signature in Rust | miss | extracts_rust_calls (crates/codelens-engine/src/call_graph.rs) |
| semantic_search | implement From trait for error type conversion | miss | error (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | run integration test against a real database | miss | test_extract_call_args_nested (crates/codelens-engine/src/inline.rs) |
| semantic_search | AppConfig | miss |  |
| semantic_search | DbPool | miss | db (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | use Arc and Mutex to share state across async tasks | miss | preset (crates/codelens-mcp/src/server/session.rs) |
| semantic_search | implement a trait for pluggable storage backends | miss | clone_for_worker (crates/codelens-mcp/src/state.rs) |
| semantic_search | parse command-line arguments and run subcommands | miss | parse_lsp_args (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | emit structured tracing spans with tracing crate | miss | find_symbols_with_path (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | check if two byte slices are equal in constant time | miss | all_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | AxumRouter | miss |  |
| semantic_search | implement retry logic with a backoff strategy | miss | key (crates/codelens-mcp/src/preflight_store.rs) |
| semantic_search | write a benchmark test with criterion | miss | BENCHMARK (scripts/finetune/compare_base_vs_v6.py) |
| semantic_search | TokenBucket | miss | token_budget (crates/codelens-mcp/src/state.rs) |
| semantic_search | handle a timeout on an async future | miss | build_handle_payload (crates/codelens-mcp/src/tools/report_payload.rs) |
| semantic_search | generate a random cryptographic nonce | miss | generate_import_statement (crates/codelens-engine/src/auto_import.rs) |
| semantic_search | flatten a nested Result and propagate errors | miss | is_nl_shaped_rejects_code_and_paths (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | define a Rails controller action to create a resource | miss | text_resource (crates/codelens-mcp/src/resources.rs) |
| semantic_search | write an ActiveRecord scope with a custom condition | miss | project_scope_renames_across_files (crates/codelens-engine/src/rename.rs) |
| semantic_search | add a before_action callback to authenticate requests | miss | query_has_action_verb (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | serialize an ActiveRecord model to JSON with JBuilder | miss | sparse_query_tokens_drops_stopwords_and_short_tokens (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | create a background job with Sidekiq | miss | create_or_resume_reuses_existing_session (crates/codelens-mcp/src/server/session.rs) |
| semantic_search | write a Rails database migration to add a column | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | validate presence and uniqueness of a model attribute | miss | set_session_surface_and_budget (crates/codelens-mcp/src/state.rs) |
| semantic_search | implement a custom Devise strategy for API token auth | miss | sparse_query_tokens_drops_stopwords_and_short_tokens (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | write an RSpec request spec for a POST endpoint | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | create a Rails service object for business logic | miss | analysis_parallelism_for_profile (crates/codelens-mcp/src/state.rs) |
| semantic_search | ApplicationRecord | miss | apply_scenario_to_brief (benchmarks/harness/task-bootstrap.py) |
| semantic_search | ApplicationMailer | miss |  |
| semantic_search | configure Rails routes with namespaced API endpoints | miss | configured_embedding_model_name_defaults_to_codesearchnet (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | use ActiveJob to enqueue a mailer in background | miss | insert_content_tool (crates/codelens-mcp/src/tools/mutation.rs) |
| semantic_search | configure ActiveStorage for file attachments | miss | default_lsp_command_for_path (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | define a Laravel route with controller action | miss | all_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | create a Laravel Eloquent model with relationships | miss | all_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | write a Laravel form request for input validation | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | dispatch a Laravel job to the queue | miss | prefers_dispatch_entrypoint_over_handler_types (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | create a Laravel migration to add an index | miss | find_similar_code_uses_index_and_excludes_target_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | implement a Laravel middleware to check API key | miss | infer_recommended_checks (crates/codelens-mcp/src/tools/report_payload.rs) |
| semantic_search | send an email with a Mailable and Blade template | miss | is_static_method_ident_accepts_pascal_and_rejects_snake (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | write a Laravel Observer to react to model events | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | create a Livewire component for real-time search | miss | create_text_file_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | define a Laravel event and listener pair | miss | telemetry_writer_appends_multiple_events_in_order (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | ApiResource | miss |  |
| semantic_search | ServiceProvider | miss | server (benchmarks/harness/harness_runner_common.py) |
| semantic_search | query database using Laravel Query Builder with joins | miss | semantic_query_splits_identifier_terms_without_alias_injection (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | write a PHPUnit feature test for a POST endpoint | miss | write_to_disk (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | cache a database query result with Laravel Cache | miss | cached_query (crates/codelens-engine/src/symbols/parser.rs) |
| semantic_search | JWT token generation | miss | pad_token_rows (scripts/finetune/finetune_distill.py) |
| semantic_search | form validation schema | miss | validation_path (scripts/finetune/build_runtime_training_pipeline.py) |
| semantic_search | file upload handler | miss | file_path (scripts/finetune/build_runtime_training_pipeline.py) |
| semantic_search | Redis cache client setup | miss | set_client_metadata (crates/codelens-mcp/src/server/session.rs) |
| semantic_search | database connection singleton | miss | dataset (scripts/finetune/finetune_distill.py) |
| semantic_search | password strength validation | miss | path (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | access token expiry check | miss | push_verifier_check (crates/codelens-mcp/src/tools/report_verifier.rs) |
| semantic_search | image compression utility | miss | compression_threshold_offset (crates/codelens-mcp/src/client_profile.rs) |
| semantic_search | Next.js error page component | miss | error (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | user session persistence hook | miss | session_snapshot (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | async database session factory | miss | slim_text_payload_for_async_handle (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | user model serialization | miss | uses (.github/workflows/release.yml) |
| semantic_search | background task queue | miss | queue_max_depth (benchmarks/token_efficiency_scenarios.py) |
| semantic_search | Pydantic settings config | miss | config (benchmarks/harness/tests/test_policy_integrity.py) |
| semantic_search | password reset token generation | miss | reset_clears_all (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | Django test factory for user | miss | preflight_key_for_session (crates/codelens-mcp/src/state.rs) |
| semantic_search | ML model prediction endpoint | miss | evaluate_model (scripts/finetune/compare_base_vs_v6.py) |
| semantic_search | CSV row parser utility | miss | parsers (crates/codelens-engine/src/lsp/mod.rs) |
| semantic_search | context deadline propagation | miss | content (scripts/finetune/finetune_distill.py) |
| semantic_search | HTTP JSON error response | miss | json_response (crates/codelens-mcp/src/server/transport_http_support.rs) |
| semantic_search | goroutine safe counter | miss | code_time (models/benchmark.py) |
| semantic_search | database row scanner helper | miss | chunk_from_row (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | OpenTelemetry span context | miss | ops (crates/codelens-engine/src/db/mod.rs) |
| semantic_search | JWT claims struct | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | Spring bean factory method | miss | is_static_method_ident_accepts_pascal_and_rejects_snake (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | JPA entity auditing | miss | audit_dir (crates/codelens-mcp/src/state.rs) |
| semantic_search | custom validator annotation | miss | codex_session_can_restore_tool_annotations_explicitly (crates/codelens-mcp/src/server/http_tests.rs) |
| semantic_search | REST API pagination response | miss | compact_response_payload (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | async trait implementation | miss | codex_session_prepare_harness_session_bootstraps_without_tools_list (crates/codelens-mcp/src/server/http_tests.rs) |
| semantic_search | config file loading | miss | load_config (benchmarks/harness/harness-eval.py) |
| semantic_search | error chain conversion | miss | message (benchmarks/harness/harness_runner_common.py) |
| semantic_search | connection pool initialization | miss | pick_rows (scripts/finetune/build_codex_dataset.py) |
| semantic_search | middleware stack builder | miss | builder_minimal_mutation_behavior_unchanged (crates/codelens-mcp/src/integration_tests.rs) |
| semantic_search | ActiveRecord association helpers | miss | activate (benchmarks/harness/harness_runner_common.py) |
| semantic_search | Rails concern for soft delete | miss | project_scope_for_session (crates/codelens-mcp/src/state.rs) |
| semantic_search | Sidekiq retry configuration | miss | RETRY_DELAYS_MS (crates/codelens-engine/src/watcher.rs) |
| semantic_search | Eloquent query scope | miss | analysis_jobs_follow_session_bound_project_scope (crates/codelens-mcp/src/server/http_tests.rs) |
| semantic_search | Laravel service container binding | miss | insert_symbol (crates/codelens-engine/src/symbols/parser.rs) |
| semantic_search | Blade component rendering | miss | mrr_component (benchmarks/external-retrieval.py) |
| semantic_search | SignedUploadUrlResult | miss | signature (scripts/finetune/build_runtime_training_pipeline.py) |
| semantic_search | CheckoutSessionParams | miss | checks (benchmarks/token-efficiency.py) |
| semantic_search | WebSocketMessage | miss |  |
| semantic_search | FeatureFlags | miss | features (crates/codelens-engine/Cargo.toml) |
| semantic_search | JobPayload | miss | job_path (crates/codelens-mcp/src/job_store.rs) |
| semantic_search | EmailTemplate | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | AnomalyResult | miss | AnalysisSource (crates/codelens-mcp/src/protocol.rs) |
| semantic_search | ImportRecord | miss | ImportRow (crates/codelens-engine/src/db/mod.rs) |
| semantic_search | WebhookEvent | miss |  |
| semantic_search | PaginatedResponse | miss | pairs (scripts/finetune/build_runtime_training_pipeline.py) |
| semantic_search | UserDTO | miss | User (crates/codelens-engine/tests/fixtures/sample_project/src/models.ts) |
| semantic_search | CircuitBreaker | miss | circular (crates/codelens-engine/src/lib.rs) |
| semantic_search | MetricsCollector | miss | metrics (crates/codelens-mcp/src/state.rs) |
| semantic_search | UserDto | miss | User (crates/codelens-engine/tests/fixtures/sample_project/src/models.ts) |
| semantic_search | OrderEntity | miss | ordered_entrypoints (benchmarks/harness/task-bootstrap.py) |
| semantic_search | HandlerError | miss | handle_request (crates/codelens-mcp/src/server/router.rs) |
| semantic_search | StorageConfig | miss | state (benchmarks/harness/harness_runner_common.py) |
| semantic_search | ApplicationJob | miss | apply_scenario_to_brief (benchmarks/harness/task-bootstrap.py) |
| semantic_search | BaseController | miss | base_score (scripts/finetune/promotion_gate.py) |
| get_ranked_context_no_semantic | rename a variable or function across the project | miss | rename_symbol (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | find where a symbol is defined in a file | miss | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | apply text edits to multiple files | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | search code by natural language query | miss | configured_embedding_model_name_defaults_to_codesearchnet (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | inline a function and remove its definition | miss | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | move code to a different file | miss | remove (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | change function parameters | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | find circular import dependencies | miss | find_circular_dependencies_tool (crates/codelens-mcp/src/tools/graph.rs) |
| get_ranked_context_no_semantic | parse source code into an AST | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find near-duplicate code in the codebase | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | build_non_code_ranges (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | compute similarity between two vectors | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | rename_symbol | miss | rename_symbol (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | cosine_similarity | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find_circular_dependencies | miss | find_circular_dependencies (crates/codelens-engine/src/circular.rs) |
| get_ranked_context_no_semantic | how to build embedding text from a symbol | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 7 | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | exclude directories from indexing | miss | EXCLUDED_DIRS (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | EmbeddingEngine | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find all functions that call a given function | miss | extract_api_calls_rejects_module_prefixed_free_functions (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | filter out standard library noise from call graph | miss | call_graph (crates/codelens-engine/src/lib.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | CallEdge | miss | CallEdge (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | CallerEntry | miss | CallerEntry (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | detect communities and clusters in the codebase | miss | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | calculate modularity score for graph partitioning | miss | calculate_modularity (crates/codelens-engine/src/community.rs) |
| get_ranked_context_no_semantic | find common path prefix for files in a community | miss | common_path_prefix (crates/codelens-engine/src/community.rs) |
| get_ranked_context_no_semantic | measure density of internal edges in a cluster | miss | resolve_call_edges (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | ArchitectureOverview | miss | ArchitectureOverview (crates/codelens-engine/src/community.rs) |
| get_ranked_context_no_semantic | register sqlite vector extension for similarity search | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | miss | _split_camel (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context_no_semantic | SemanticMatch | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | SqliteVecStore | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | validate that a new name is a valid identifier | miss | new (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | still_detects_project_root_before_home_directory (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | collect rename edits scoped to a single file | miss | rename_symbol (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | RenameEdit | miss | RenameEdit (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | RenameScope | miss | RenameScope (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | get overview of all symbols in a file | miss | get_symbols_overview (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | search for a symbol by name | miss | parse_symbol_id (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | get project directory tree structure | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | SymbolIndex | miss | SymbolIndex (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | rerank semantic search results by relevance | miss | engine_search_returns_results (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | ProjectRoot (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | build an error response with failure kind | 4 | is_protocol_error (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | how does the embedding engine initialize the model | miss | embedding_engine (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | doom loop detection and prevention | 4 | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | select the most relevant symbols for a query | miss | select_solve_symbols (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 16 | SymbolKind (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | check if a tool is a symbol-aware mutation | 9 | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | collect rename edits across the entire project | miss | rename_symbol (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find all word matches in a single file | miss | finds_references_in_single_file (crates/codelens-engine/src/scope_analysis.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | set_recent_preflight_timestamp_for_test (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | parse_tier_label (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | ProjectOverride | miss |  |
| get_ranked_context_no_semantic | Community | miss | Community (crates/codelens-engine/src/community.rs) |
| get_ranked_context_no_semantic | RenameResult | miss | RenameResult (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | CallLanguageConfig | miss | CallLanguageConfig (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | CalleeEntry | miss | CalleeEntry (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | handle user authentication with JWT tokens | miss | extract_handle_fields (crates/codelens-mcp/src/tools/report_utils.rs) |
| get_ranked_context_no_semantic | create a custom React hook for fetching data | miss | set_created_at_for_test (crates/codelens-mcp/src/artifact_store.rs) |
| get_ranked_context_no_semantic | validate form input fields before submission | miss | invalidate_fts (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | generate a signed URL for S3 file upload | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | middleware to protect API routes from unauthenticated access | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | send a transactional email using a template | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context_no_semantic | paginate database query results | miss | engine_search_returns_results (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | refresh access token using refresh token | miss | record_file_access_for_session (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | debounce a search input handler | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | connect to PostgreSQL with connection pooling | miss | with (.github/workflows/release.yml) |
| get_ranked_context_no_semantic | rate limit API requests per IP address | miss | migrate (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | server-side render a Next.js page with user data | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | upload image and resize to multiple dimensions | miss | multiple_tools_independent (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context_no_semantic | handle Stripe webhook events | miss | handle_request (crates/codelens-mcp/src/server/router.rs) |
| get_ranked_context_no_semantic | create a subscription checkout session | miss | create_and_get_session (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | manage global state with Zustand store | miss | does_not_promote_home_directory_from_global_codelens_marker (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | infinite scroll hook for loading more items | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | format currency values for display | miss | backend_kind_display_stable (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | generate a random slug from a title string | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | transform API response to camelCase | miss | ToolCallResponse (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | parse and validate environment variables at startup | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | cache API responses in Redis | miss | graph_cache (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | handle file drag and drop in a React component | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | update user profile in the database | miss | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | generate OTP for two-factor authentication | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | SSR data fetching for static site generation | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | intercept Axios requests to add auth headers | miss | ReadDb (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | export data to CSV file from a table | miss | backend_kind_display_stable (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | sort array of objects by a given key | miss | string_array_field (crates/codelens-mcp/src/session_context.rs) |
| get_ranked_context_no_semantic | toast notification system for user feedback | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | track user analytics events | miss | error_count_tracked (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context_no_semantic | deep merge two configuration objects | miss | merged_string_set (crates/codelens-mcp/src/server/router.rs) |
| get_ranked_context_no_semantic | create a typed API route handler in Next.js | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | hash a password with bcrypt | miss | content_hash (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | lazy load a React component | miss | build_session_metrics_payload (crates/codelens-mcp/src/session_metrics_payload.rs) |
| get_ranked_context_no_semantic | handle SSE streaming response from server | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | build a reusable modal dialog component | miss | find_reusable_analysis (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | check user subscription plan and permissions | miss | get_rename_plan (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context_no_semantic | implement optimistic UI updates in a React mutation | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | internationalize strings with i18n locale | miss | strings_from_array (crates/codelens-mcp/src/tools/report_utils.rs) |
| get_ranked_context_no_semantic | ApiResponse | miss |  |
| get_ranked_context_no_semantic | UserProfile | miss |  |
| get_ranked_context_no_semantic | AuthContext | miss |  |
| get_ranked_context_no_semantic | PaginationMeta | miss |  |
| get_ranked_context_no_semantic | upload a file to cloud storage and return its URL | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | parse markdown content to HTML | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | map tRPC router endpoints to handlers | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | build Prisma query filters from request params | miss | from_request (crates/codelens-mcp/src/resource_context.rs) |
| get_ranked_context_no_semantic | retry a failed API call with exponential backoff | miss | BACKOFF_MS (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | scroll restoration hook for page navigation | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | sanitize user-generated HTML content | miss | HTML_QUERY (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context_no_semantic | generate a unique ID for a new entity | miss | new (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | configure Next.js Image component with allowed domains | miss | configure_daemon_mode (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | StreamingTextResponse | miss |  |
| get_ranked_context_no_semantic | SubscriptionPlan | miss |  |
| get_ranked_context_no_semantic | create a serverless API route to handle POST requests | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | build React Server Component for a data table | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | flatten a nested array structure | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | theme switching between light and dark mode | miss | RecentPreflight (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | run database migrations on application startup | miss | MIGRATIONS (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | handle OAuth callback and exchange code for tokens | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | transform image buffer to WebP format | miss | configured_coreml_model_format_name (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | compute reading time estimate from article text | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | create a feature flag check function | miss | create (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | log structured JSON errors to external service | miss | from_json (crates/codelens-mcp/src/session_context.rs) |
| get_ranked_context_no_semantic | validate and parse a UUID string | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | RouteConfig | miss |  |
| get_ranked_context_no_semantic | useLocalStorage | miss |  |
| get_ranked_context_no_semantic | buildMetadata | miss |  |
| get_ranked_context_no_semantic | protect a route by checking user role | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | batch insert multiple records into the database | miss | insert_imports (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | handle websocket connection lifecycle | miss | JobLifecycle (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | get current authenticated user from session | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | compress and decompress JSON payload | miss | bounded_result_payload (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context_no_semantic | register a service worker for offline support | miss | clone_for_worker (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | run background job with a queue processor | miss | run_analysis_job_from_queue (crates/codelens-mcp/src/tools/report_jobs.rs) |
| get_ranked_context_no_semantic | create breadcrumb navigation from URL path | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | calculate Stripe proration for plan upgrade | miss | calculates_python_blast_radius (crates/codelens-engine/src/import_graph/mod.rs) |
| get_ranked_context_no_semantic | fetch paginated list of posts with cursor | miss | store_can_fetch_single_embedding_without_loading_all (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | CursorPaginationInput | miss |  |
| get_ranked_context_no_semantic | withErrorBoundary | miss |  |
| get_ranked_context_no_semantic | decode a JWT token without verifying the signature | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | group array items by a property value | miss | MAX_ARRAY_ITEMS (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context_no_semantic | check if a date is within a valid range | miss | invalidate_fts (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | define a FastAPI route with path and query parameters | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | create a Pydantic model for request body validation | miss | JsonRpcRequest (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | run Celery background task asynchronously | miss | run (benchmarks/_run_harness_wrapper.py) |
| get_ranked_context_no_semantic | connect to database using SQLAlchemy async session | miss | slim_text_payload_for_async_handle (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context_no_semantic | authenticate user and return JWT access token | miss | record_file_access (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | verify a JWT token and extract claims | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context_no_semantic | hash a password using bcrypt | miss | content_hash (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | define a Django ORM model with relationships | miss | does_not_suggest_locally_defined (crates/codelens-engine/src/auto_import.rs) |
| get_ranked_context_no_semantic | serialize Django model to JSON response | miss | json_response (crates/codelens-mcp/src/server/transport_http_support.rs) |
| get_ranked_context_no_semantic | run Django database migration for a new field | miss | new (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | paginate a Django REST Framework queryset | miss | detect_frameworks (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | send a webhook notification on model save | miss | JsonRpcNotification (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | write a pytest fixture for database session | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | upload a file in FastAPI using UploadFile | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | parse CSV file into a list of dicts | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | apply rate limiting to a FastAPI endpoint | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | connect to Redis cache and set a value | miss | tool_tier (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | bulk insert records into PostgreSQL | miss | insert_calls (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | implement a repository pattern for database access | miss | record_file_access_for_session (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | create a custom exception handler in FastAPI | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | load configuration from environment variables | miss | from_env (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context_no_semantic | implement a Python context manager for a transaction | miss | RankedContextEntry (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | filter Django queryset by date range | miss | find_symbol_range (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | define GraphQL mutation to create a user | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | run a subprocess command and capture output | miss | capture_name_to_kind (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | decrypt a message using AES encryption | miss | sqlite_message_suggests_recovery (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | load a machine learning model from a file | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | preprocess text for NLP pipeline | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | stream large file response without loading into memory | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | register a FastAPI dependency injection provider | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | configure CORS allowed origins in FastAPI | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | TokenData | miss |  |
| get_ranked_context_no_semantic | UserInDB | miss |  |
| get_ranked_context_no_semantic | check if user has permission to access a resource | miss | content_hash (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | extract features from raw dataset for training | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context_no_semantic | write a pytest parametrize test for multiple inputs | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | handle async generator for streaming responses | miss | slim_text_payload_for_async_handle (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context_no_semantic | retry a network request with exponential backoff | miss | JsonRpcRequest (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | convert a dataclass to a dict recursively | miss | convert_row (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context_no_semantic | import data from external API and normalize schema | miss | with_output_schema (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | generate PDF report from HTML template | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | validate and sanitize user input against a schema | miss | with_output_schema (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | BaseRepository | miss |  |
| get_ranked_context_no_semantic | CeleryConfig | miss |  |
| get_ranked_context_no_semantic | initialize SQLAlchemy ORM engine with connection string | miss | SymbolWithFile (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | detect anomalies in time series data | miss | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | store and retrieve values from a Python LRU cache | miss | ProjectContextCache (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | implement Django custom management command | miss | ALLOWED_COMMANDS (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context_no_semantic | test a FastAPI endpoint with the TestClient | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | create a custom Django admin action | miss | create (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | schedule a periodic task with Celery beat | miss | with (.github/workflows/build.yml) |
| get_ranked_context_no_semantic | send a push notification to a mobile device | miss | push_recent_tool (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | aggregate query results grouped by day | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | HealthCheck | miss |  |
| get_ranked_context_no_semantic | DatabaseError | miss |  |
| get_ranked_context_no_semantic | convert pandas DataFrame to a list of records | miss | list_dir_tool (crates/codelens-mcp/src/tools/filesystem.rs) |
| get_ranked_context_no_semantic | read secrets from AWS Secrets Manager | miss | reader (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | encode a binary file as base64 string | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | verify an HMAC signature on an incoming webhook | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | find the closest matching item using fuzzy search | miss | search_dual (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | run data validation and return a list of errors | miss | returns_dead_code_candidates (crates/codelens-engine/src/import_graph/mod.rs) |
| get_ranked_context_no_semantic | split a list into chunks of a given size | miss | optional_usize (crates/codelens-mcp/src/tool_runtime.rs) |
| get_ranked_context_no_semantic | log a request and its response time as a middleware | miss | JsonRpcRequest (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | compute TF-IDF score for document ranking | miss | ranking (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | create a dataclass for a configuration section | miss | get_analysis_section (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | S3Client | miss |  |
| get_ranked_context_no_semantic | PushNotificationPayload | miss |  |
| get_ranked_context_no_semantic | summarize a long text using an LLM | miss | ResourceRequestContext (crates/codelens-mcp/src/resource_context.rs) |
| get_ranked_context_no_semantic | run a full-text search query on a database table | miss | backend_kind_display_stable (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | track model training metrics per epoch | miss | metrics (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | handle HTTP request with context and timeout | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | connect to PostgreSQL database using pgx driver | miss | CS_USING_RE (crates/codelens-engine/src/import_graph/parsers.rs) |
| get_ranked_context_no_semantic | run a database transaction with rollback on error | miss | with_transaction (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | parse JSON request body into a struct | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | generate a JWT token for a user | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | validate a JWT token and return claims | miss | invalidate_fts (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | implement middleware for request logging | miss | JsonRpcRequest (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | write a gRPC server handler for a service method | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | publish a message to a Kafka topic | miss | sqlite_message_suggests_recovery (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | consume messages from a Kafka topic | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | implement a retry loop with exponential backoff | miss | BACKOFF_MS (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | parse command-line flags using the flag package | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | gracefully shut down an HTTP server on signal | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | read configuration from YAML file | miss | reader (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | implement a worker pool for parallel processing | miss | analysis_parallelism_for_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | write unit test with table-driven test cases | miss | backend_kind_display_stable (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | implement an LRU cache with expiration | miss | graph_cache (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | use sync.WaitGroup to wait for goroutines | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | register routes on an HTTP mux with middleware | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | read a file line by line using bufio scanner | miss | run_stdio (crates/codelens-mcp/src/server/transport_stdio.rs) |
| get_ranked_context_no_semantic | encode a struct to JSON and write to response | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | connect to Redis and implement a simple set-get | miss | tool_tier (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | measure function execution latency with prometheus | miss | execution_token_budget (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | Config | miss | config (benchmarks/role-retrieval.py) |
| get_ranked_context_no_semantic | Server | miss | server (crates/codelens-mcp/src/main.rs) |
| get_ranked_context_no_semantic | Repository | miss | UserRepository (crates/codelens-engine/tests/fixtures/sample_project/src/models.ts) |
| get_ranked_context_no_semantic | implement OpenTelemetry trace span for a function | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | apply database migrations using goose | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | marshal proto message to binary format | miss | is_protocol_error (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | implement health check endpoint for Kubernetes | miss | watcher_failure_health (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | seed the database with initial test data | miss | tests (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | scan SQL rows into a slice of structs | miss | sqlite_related_paths (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | ErrNotFound | miss |  |
| get_ranked_context_no_semantic | handle an error by wrapping it with additional context | miss | is_protocol_error (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | validate a struct using validator tags | miss | destructive (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | log a structured message with fields using zap | miss | sqlite_message_suggests_recovery (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | create a mock implementation for an interface | miss | create_and_get_session (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | compute SHA256 hash of a string | miss | content_hash (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | JobWorker | miss |  |
| get_ranked_context_no_semantic | KafkaProducer | miss |  |
| get_ranked_context_no_semantic | serialize a struct to a map for dynamic querying | miss | destructive (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | implement a circuit breaker for external service calls | miss | insert_calls (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | rate limit requests using a token bucket algorithm | miss | migrate (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | upload a file to GCS bucket | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | decode URL-encoded query parameters | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | define a Spring REST controller endpoint | miss | does_not_suggest_locally_defined (crates/codelens-engine/src/auto_import.rs) |
| get_ranked_context_no_semantic | create a Spring JPA repository interface | miss | create (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | implement a Spring service layer with transaction | miss | with_transaction (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | configure Spring Security filter chain | miss | configure_daemon_mode (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | map a JPA entity to a database table | miss | backend_kind_display_stable (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | convert entity to DTO for API response | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | handle global exceptions with Spring ControllerAdvice | miss | does_not_promote_home_directory_from_global_codelens_marker (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | write a JUnit 5 test with mocked dependencies | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | create a Spring Batch job for data processing | miss | AnalysisJob (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | publish a Spring application event and listen to it | miss | append_event (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context_no_semantic | configure Liquibase database changelog | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | implement a JWT authentication filter | miss | filtered (benchmarks/harness/test_policy_integrity.py) |
| get_ranked_context_no_semantic | create a Spring Cache configuration with Redis | miss | graph_cache (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | send an email with Spring Mail and Thymeleaf template | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context_no_semantic | validate request body with Bean Validation annotations | miss | with_annotations (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | UserServiceImpl | miss |  |
| get_ranked_context_no_semantic | JwtTokenProvider | miss |  |
| get_ranked_context_no_semantic | schedule a Spring task with a cron expression | miss | with (.github/workflows/build.yml) |
| get_ranked_context_no_semantic | implement an aspect for method-level logging | miss | set_effort_level (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | create a Spring WebClient for non-blocking HTTP calls | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | write a custom Spring Health Indicator | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | PageableResponse | miss |  |
| get_ranked_context_no_semantic | ApiException | miss |  |
| get_ranked_context_no_semantic | implement Flyway migration script to add a column | miss | ReadDb (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | publish a message to a RabbitMQ exchange | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | consume messages from a RabbitMQ queue | miss | record_recent_preflight_from_payload (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | configure OpenAPI Swagger documentation for the API | miss | configure_daemon_mode (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | search records with dynamic JPA Specification | miss | all_with_embeddings (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | RabbitMQConfig | miss |  |
| get_ranked_context_no_semantic | AuditListener | miss |  |
| get_ranked_context_no_semantic | implement an async TCP server with Tokio | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | deserialize JSON into a Rust struct with serde | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | define a custom error type with thiserror | miss | CodeLensError (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | query a database row with sqlx and bind parameters | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | spawn a background task with Tokio handle | miss | extract_handle_fields (crates/codelens-mcp/src/tools/report_utils.rs) |
| get_ranked_context_no_semantic | implement the Display trait for a custom type | miss | types (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | write a property-based test with proptest | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | build a CLI argument parser with clap derive | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | implement an Axum route handler with state injection | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | use a channel to communicate between threads | miss | recommended_embed_threads (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | create a connection pool with deadpool-postgres | miss | lsp_pool (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | read and write a TOML configuration file | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | implement a custom Serde deserializer for a field | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | verify HMAC-SHA256 signature in Rust | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | implement From trait for error type conversion | miss | CodeLensError (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | run integration test against a real database | miss | tests (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | AppConfig | miss |  |
| get_ranked_context_no_semantic | DbPool | miss |  |
| get_ranked_context_no_semantic | use Arc and Mutex to share state across async tasks | miss | AppState (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | implement a trait for pluggable storage backends | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | parse command-line arguments and run subcommands | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | emit structured tracing spans with tracing crate | miss | SymbolWithFile (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | check if two byte slices are equal in constant time | miss | for_each_file_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | AxumRouter | miss |  |
| get_ranked_context_no_semantic | implement retry logic with a backoff strategy | miss | BACKOFF_MS (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | write a benchmark test with criterion | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | TokenBucket | miss |  |
| get_ranked_context_no_semantic | handle a timeout on an async future | miss | session_timeout_seconds (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | generate a random cryptographic nonce | miss | generate_typescript_import (crates/codelens-engine/src/auto_import.rs) |
| get_ranked_context_no_semantic | flatten a nested Result and propagate errors | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | define a Rails controller action to create a resource | miss | create_and_get_session (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | write an ActiveRecord scope with a custom condition | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | add a before_action callback to authenticate requests | miss | ReadDb (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | serialize an ActiveRecord model to JSON with JBuilder | miss | estimate_serialized_tokens (crates/codelens-mcp/src/tool_defs/build.rs) |
| get_ranked_context_no_semantic | create a background job with Sidekiq | miss | AnalysisJob (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | write a Rails database migration to add a column | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | validate presence and uniqueness of a model attribute | miss | invalidate_fts (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | implement a custom Devise strategy for API token auth | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | write an RSpec request spec for a POST endpoint | miss | collect_candidate_files (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | create a Rails service object for business logic | miss | set_created_at_for_test (crates/codelens-mcp/src/artifact_store.rs) |
| get_ranked_context_no_semantic | ApplicationRecord | miss |  |
| get_ranked_context_no_semantic | ApplicationMailer | miss |  |
| get_ranked_context_no_semantic | configure Rails routes with namespaced API endpoints | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | use ActiveJob to enqueue a mailer in background | miss | enqueue_analysis_job (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | configure ActiveStorage for file attachments | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | define a Laravel route with controller action | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | create a Laravel Eloquent model with relationships | miss | create_or_resume (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | write a Laravel form request for input validation | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | dispatch a Laravel job to the queue | miss | AnalysisJob (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | create a Laravel migration to add an index | miss | ReadDb (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | implement a Laravel middleware to check API key | miss | extract_api_calls_inner (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | send an email with a Mailable and Blade template | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context_no_semantic | write a Laravel Observer to react to model events | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | create a Livewire component for real-time search | miss | search_dual (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | define a Laravel event and listener pair | miss | SseEvent (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | ApiResource | miss |  |
| get_ranked_context_no_semantic | ServiceProvider | miss |  |
| get_ranked_context_no_semantic | query database using Laravel Query Builder with joins | miss | all_with_embeddings (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | write a PHPUnit feature test for a POST endpoint | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | cache a database query result with Laravel Cache | miss | all_with_embeddings (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | JWT token generation | miss | generation (crates/codelens-engine/src/import_graph/mod.rs) |
| get_ranked_context_no_semantic | form validation schema | miss | output_schemas (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | file upload handler | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | Redis cache client setup | miss | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | database connection singleton | miss |  |
| get_ranked_context_no_semantic | password strength validation | miss | validation_path (scripts/finetune/build_runtime_training_pipeline.py) |
| get_ranked_context_no_semantic | access token expiry check | miss | record_file_access (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | image compression utility | miss | compression_threshold_offset (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context_no_semantic | Next.js error page component | miss | CodeLensError (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | user session persistence hook | miss | registry_without_writer_is_noop_for_persistence (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context_no_semantic | async database session factory | miss | slim_text_payload_for_async_handle (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context_no_semantic | user model serialization | miss | type_node_kind_serialization (crates/codelens-engine/src/type_hierarchy.rs) |
| get_ranked_context_no_semantic | background task queue | miss | AnalysisQueueState (crates/codelens-mcp/src/analysis_queue.rs) |
| get_ranked_context_no_semantic | Pydantic settings config | miss | LanguageConfig (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context_no_semantic | password reset token generation | miss | presets (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | Django test factory for user | miss | tests (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | ML model prediction endpoint | miss | model (scripts/finetune/train_v7_nl_augmented.py) |
| get_ranked_context_no_semantic | CSV row parser utility | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | context deadline propagation | miss | RankedContextEntry (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | HTTP JSON error response | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context_no_semantic | goroutine safe counter | miss | safe_rename_report (crates/codelens-mcp/src/tools/reports/verifier_reports.rs) |
| get_ranked_context_no_semantic | database row scanner helper | miss | prefers_stdio_entrypoint_over_generic_read_helpers (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | OpenTelemetry span context | miss | RankedContextEntry (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | JWT claims struct | miss | destructive (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | Spring bean factory method | miss | method (scripts/finetune/promotion_gate.py) |
| get_ranked_context_no_semantic | JPA entity auditing | miss | identity (scripts/finetune/compress_to_3layer.py) |
| get_ranked_context_no_semantic | custom validator annotation | miss | ToolAnnotations (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | REST API pagination response | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | async trait implementation | miss | rust_trait_impl (crates/codelens-engine/src/type_hierarchy.rs) |
| get_ranked_context_no_semantic | config file loading | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | error chain conversion | miss | CodeLensError (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | connection pool initialization | miss | lsp_pool (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | middleware stack builder | miss | BUILDER_MINIMAL_TOOLS (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context_no_semantic | ActiveRecord association helpers | miss | prefers_stdio_entrypoint_over_generic_read_helpers (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context_no_semantic | Rails concern for soft delete | miss | delete_file (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | Sidekiq retry configuration | miss | RETRY_DELAYS_MS (crates/codelens-engine/src/watcher.rs) |
| get_ranked_context_no_semantic | Eloquent query scope | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | Laravel service container binding | miss | UserService (crates/codelens-engine/tests/fixtures/sample_project/src/service.py) |
| get_ranked_context_no_semantic | Blade component rendering | miss | components (.github/workflows/ci.yml) |
| get_ranked_context_no_semantic | SignedUploadUrlResult | miss |  |
| get_ranked_context_no_semantic | CheckoutSessionParams | miss |  |
| get_ranked_context_no_semantic | WebSocketMessage | miss |  |
| get_ranked_context_no_semantic | FeatureFlags | miss |  |
| get_ranked_context_no_semantic | JobPayload | miss |  |
| get_ranked_context_no_semantic | EmailTemplate | miss |  |
| get_ranked_context_no_semantic | AnomalyResult | miss |  |
| get_ranked_context_no_semantic | ImportRecord | miss |  |
| get_ranked_context_no_semantic | WebhookEvent | miss |  |
| get_ranked_context_no_semantic | PaginatedResponse | miss |  |
| get_ranked_context_no_semantic | UserDTO | miss |  |
| get_ranked_context_no_semantic | CircuitBreaker | miss |  |
| get_ranked_context_no_semantic | MetricsCollector | miss |  |
| get_ranked_context_no_semantic | UserDto | miss |  |
| get_ranked_context_no_semantic | OrderEntity | miss |  |
| get_ranked_context_no_semantic | HandlerError | miss |  |
| get_ranked_context_no_semantic | StorageConfig | miss |  |
| get_ranked_context_no_semantic | ApplicationJob | miss |  |
| get_ranked_context_no_semantic | BaseController | miss |  |
| get_ranked_context | rename a variable or function across the project | miss | rename (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | find where a symbol is defined in a file | miss | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | apply text edits to multiple files | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | search code by natural language query | miss | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context | inline a function and remove its definition | miss | inline_function (crates/codelens-engine/src/inline.rs) |
| get_ranked_context | move code to a different file | miss | refactor_move_to_file (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context | change function parameters | miss | ParamSpec (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context | find circular import dependencies | miss | find_circular_dependencies (crates/codelens-engine/src/circular.rs) |
| get_ranked_context | read input from stdin line by line | 9 | read_line_at (crates/codelens-engine/src/file_ops/mod.rs) |
| get_ranked_context | parse source code into an AST | miss | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | find near-duplicate code in the codebase | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | categorize a function by its purpose | miss | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | get project structure and key files on first load | 10 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | watch filesystem for file changes | miss | FileWatcher (crates/codelens-engine/src/watcher.rs) |
| get_ranked_context | skip comments and string literals during search | miss | search (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | compute similarity between two vectors | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | rename_symbol | miss | rename_symbol (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | cosine_similarity | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | find_circular_dependencies | miss | find_circular_dependencies (crates/codelens-engine/src/circular.rs) |
| get_ranked_context | how to build embedding text from a symbol | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | mutation gate preflight check before editing | 4 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | exclude directories from indexing | miss | EXCLUDED_DIRS (crates/codelens-engine/src/project.rs) |
| get_ranked_context | EmbeddingEngine | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | find all functions that call a given function | miss | __call__ (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | filter out standard library noise from call graph | miss | is_noise_callee (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| get_ranked_context | CallEdge | miss | CallEdge (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context | CallerEntry | miss | CallerEntry (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context | detect communities and clusters in the codebase | miss | detect_communities (crates/codelens-engine/src/community.rs) |
| get_ranked_context | calculate modularity score for graph partitioning | miss | graph (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | find common path prefix for files in a community | miss | common_path_prefix (crates/codelens-engine/src/community.rs) |
| get_ranked_context | measure density of internal edges in a cluster | miss | community_density (crates/codelens-engine/src/community.rs) |
| get_ranked_context | ArchitectureOverview | miss | ArchitectureOverview (crates/codelens-engine/src/community.rs) |
| get_ranked_context | register sqlite vector extension for similarity search | miss | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | miss | split_identifier (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | SemanticMatch | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | SqliteVecStore | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | validate that a new name is a valid identifier | miss | validate_identifier (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | find all occurrences of a word in project files | miss | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | collect rename edits scoped to a single file | miss | collect_file_scope_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | RenameEdit | miss | RenameEdit (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | RenameScope | miss | RenameScope (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | get overview of all symbols in a file | miss | get_symbols_overview (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | search for a symbol by name | miss | find_symbols_by_name (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | get project directory tree structure | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | SymbolIndex | miss | SymbolIndex (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | build a success response with suggested next tools | 4 | suggest_next (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | how does the embedding engine initialize the model | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | determine which language config to use for a file | miss | lang_config (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | select the most relevant symbols for a query | miss | select_solve_symbols (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | find similar code snippets using embeddings | 16 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | classify what kind of symbol this is | 6 | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | collect rename edits across the entire project | miss | collect_project_scope_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | upsert embedding vector for a symbol | miss | upsert (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | find all word matches in a single file | miss | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | parse incoming MCP tool call JSON | miss | write_mcp_json (install.sh) |
| get_ranked_context | ProjectOverride | miss |  |
| get_ranked_context | Community | miss | Community (crates/codelens-engine/src/community.rs) |
| get_ranked_context | RenameResult | miss | RenameResult (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | CallLanguageConfig | miss | CallLanguageConfig (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context | CalleeEntry | miss | CalleeEntry (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context | handle user authentication with JWT tokens | miss | list_tokens (benchmarks/harness/session_overhead_common.py) |
| get_ranked_context | create a custom React hook for fetching data | miss | data (scripts/finetune/collect_training_data.py) |
| get_ranked_context | validate form input fields before submission | miss | validate_required_params (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context | generate a signed URL for S3 file upload | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | middleware to protect API routes from unauthenticated access | miss | RoutingHint (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | send a transactional email using a template | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context | paginate database query results | miss | results (scripts/finetune/train_v8_final.py) |
| get_ranked_context | refresh access token using refresh token | miss | refresh_file (crates/codelens-engine/src/symbols/writer.rs) |
| get_ranked_context | debounce a search input handler | miss | semantic_search_handler (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context | connect to PostgreSQL with connection pooling | miss | lsp_pool (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | rate limit API requests per IP address | miss | migrate (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | server-side render a Next.js page with user data | miss | next_id (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context | upload image and resize to multiple dimensions | miss | upsert_file (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | handle Stripe webhook events | miss | handle_request (crates/codelens-mcp/src/server/router.rs) |
| get_ranked_context | create a subscription checkout session | miss | create (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | manage global state with Zustand store | miss | vec_store (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | infinite scroll hook for loading more items | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context | format currency values for display | miss | backend_kind_display_stable (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | generate a random slug from a title string | miss | slug (benchmarks/harness/harness_eval_common.py) |
| get_ranked_context | transform API response to camelCase | miss | is_camelcase (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context | parse and validate environment variables at startup | miss | StartupProjectSource (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | cache API responses in Redis | miss | QUERY_CACHE (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | handle file drag and drop in a React component | miss | refactor_move_to_file (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context | update user profile in the database | miss | set_profile (crates/codelens-mcp/src/tools/session/metrics_config.rs) |
| get_ranked_context | generate OTP for two-factor authentication | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context | SSR data fetching for static site generation | miss | browser_or_ssr_sensitive (crates/codelens-mcp/src/tools/report_verifier.rs) |
| get_ranked_context | intercept Axios requests to add auth headers | miss | request_headers (benchmarks/benchmark_runtime_common.py) |
| get_ranked_context | export data to CSV file from a table | miss | from_env (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context | sort array of objects by a given key | miss | sorted_names (models/benchmark_full.py) |
| get_ranked_context | toast notification system for user feedback | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | track user analytics events | miss | error_count_tracked (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context | deep merge two configuration objects | miss | merged (scripts/finetune/build_runtime_training_pipeline.py) |
| get_ranked_context | create a typed API route handler in Next.js | miss | next_id (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context | hash a password with bcrypt | miss | content_hash (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | lazy load a React component | miss | deferred_loading_active (crates/codelens-mcp/src/resource_context.rs) |
| get_ranked_context | handle SSE streaming response from server | miss | sse_single_response (crates/codelens-mcp/src/server/transport_http_support.rs) |
| get_ranked_context | build a reusable modal dialog component | miss | build (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context | check user subscription plan and permissions | miss | get_rename_plan (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context | implement optimistic UI updates in a React mutation | miss | mutation (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | internationalize strings with i18n locale | miss | language_for_path (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | ApiResponse | miss |  |
| get_ranked_context | UserProfile | miss |  |
| get_ranked_context | AuthContext | miss |  |
| get_ranked_context | PaginationMeta | miss |  |
| get_ranked_context | upload a file to cloud storage and return its URL | miss | upsert_file (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | parse markdown content to HTML | miss | render_markdown (benchmarks/harness/export-routing-policy.py) |
| get_ranked_context | map tRPC router endpoints to handlers | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context | build Prisma query filters from request params | miss | query (scripts/finetune/generate_curated_1k.py) |
| get_ranked_context | retry a failed API call with exponential backoff | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | scroll restoration hook for page navigation | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context | sanitize user-generated HTML content | miss | HTML_QUERY (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | generate a unique ID for a new entity | miss | existing_identity (benchmarks/harness/harness_runner_common.py) |
| get_ranked_context | configure Next.js Image component with allowed domains | miss | configure_daemon_mode (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | StreamingTextResponse | miss |  |
| get_ranked_context | SubscriptionPlan | miss |  |
| get_ranked_context | create a serverless API route to handle POST requests | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context | build React Server Component for a data table | miss | dispatch_table (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | flatten a nested array structure | miss | flatten_symbols (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | theme switching between light and dark mode | miss | RecentPreflight (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context | run database migrations on application startup | miss | migrate (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | handle OAuth callback and exchange code for tokens | miss | with_max_response_tokens (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | transform image buffer to WebP format | miss | configured_coreml_model_format_name (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | compute reading time estimate from article text | miss | read_file_text (crates/codelens-engine/src/project.rs) |
| get_ranked_context | create a feature flag check function | miss | flag_takes_value (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | log structured JSON errors to external service | miss | JsonRpcError (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | validate and parse a UUID string | miss | parse_usize_env (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | RouteConfig | miss |  |
| get_ranked_context | useLocalStorage | miss |  |
| get_ranked_context | buildMetadata | miss |  |
| get_ranked_context | protect a route by checking user role | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context | batch insert multiple records into the database | miss | insert_batch (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | handle websocket connection lifecycle | miss | JobLifecycle (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context | get current authenticated user from session | miss | get (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | compress and decompress JSON payload | miss | compact_response_payload (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context | register a service worker for offline support | miss | clone_for_worker (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | run background job with a queue processor | miss | run_analysis_job_from_queue (crates/codelens-mcp/src/tools/report_jobs.rs) |
| get_ranked_context | create breadcrumb navigation from URL path | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context | calculate Stripe proration for plan upgrade | miss | calculates_python_blast_radius (crates/codelens-engine/src/import_graph/mod.rs) |
| get_ranked_context | fetch paginated list of posts with cursor | miss | store_can_fetch_single_embedding_without_loading_all (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | CursorPaginationInput | miss |  |
| get_ranked_context | withErrorBoundary | miss |  |
| get_ranked_context | decode a JWT token without verifying the signature | miss | extract_nl_tokens_inner (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | group array items by a property value | miss | grouped (benchmarks/harness/session-pack.py) |
| get_ranked_context | check if a date is within a valid range | miss | is_in_ranges (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | define a FastAPI route with path and query parameters | miss | RoutingHint (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | create a Pydantic model for request body validation | miss | body_lines (scripts/finetune/finetune_v2.py) |
| get_ranked_context | run Celery background task asynchronously | miss | tempdir (benchmarks/harness/codex-task-runner.py) |
| get_ranked_context | connect to database using SQLAlchemy async session | miss | session (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | authenticate user and return JWT access token | miss | GH_TOKEN (.github/workflows/release.yml) |
| get_ranked_context | verify a JWT token and extract claims | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context | hash a password using bcrypt | miss | content_hash (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | define a Django ORM model with relationships | miss | does_not_suggest_locally_defined (crates/codelens-engine/src/auto_import.rs) |
| get_ranked_context | serialize Django model to JSON response | miss | json_response (crates/codelens-mcp/src/server/transport_http_support.rs) |
| get_ranked_context | run Django database migration for a new field | miss | migrate (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | paginate a Django REST Framework queryset | miss | detect_frameworks (crates/codelens-engine/src/project.rs) |
| get_ranked_context | send a webhook notification on model save | miss | send_notification (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context | write a pytest fixture for database session | miss | session (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | upload a file in FastAPI using UploadFile | miss | upsert_file (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | parse CSV file into a list of dicts | miss | extract_imports_for_file (crates/codelens-engine/src/import_graph/parsers.rs) |
| get_ranked_context | apply rate limiting to a FastAPI endpoint | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | connect to Redis cache and set a value | miss | cached_query (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | bulk insert records into PostgreSQL | miss | insert (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | implement a repository pattern for database access | miss | db (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | create a custom exception handler in FastAPI | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | load configuration from environment variables | miss | load_config (benchmarks/harness/harness-eval.py) |
| get_ranked_context | implement a Python context manager for a transaction | miss | session_context (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | filter Django queryset by date range | miss | find_symbol_range (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | define GraphQL mutation to create a user | miss | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | run a subprocess command and capture output | miss | result (benchmarks/multi-repo-eval.py) |
| get_ranked_context | decrypt a message using AES encryption | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context | load a machine learning model from a file | miss | load_codesearch_model (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | preprocess text for NLP pipeline | miss | text (scripts/finetune/build_nl_augmentation.py) |
| get_ranked_context | stream large file response without loading into memory | miss | bounded_result_payload (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context | register a FastAPI dependency injection provider | miss | parse_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | configure CORS allowed origins in FastAPI | miss | cosine_sim (models/benchmark_full.py) |
| get_ranked_context | TokenData | miss |  |
| get_ranked_context | UserInDB | miss |  |
| get_ranked_context | check if user has permission to access a resource | miss | text_resource (crates/codelens-mcp/src/resources.rs) |
| get_ranked_context | extract features from raw dataset for training | miss | extract_func_name (scripts/finetune/build_runtime_training_pipeline.py) |
| get_ranked_context | write a pytest parametrize test for multiple inputs | miss | inputs (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | handle async generator for streaming responses | miss | slim_text_payload_for_async_handle (crates/codelens-mcp/src/dispatch_response_support.rs) |
| get_ranked_context | retry a network request with exponential backoff | miss | BACKOFF_MS (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | convert a dataclass to a dict recursively | miss | DiffSymbol (crates/codelens-engine/src/git.rs) |
| get_ranked_context | import data from external API and normalize schema | miss | add_import_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| get_ranked_context | generate PDF report from HTML template | miss | report_contract (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | validate and sanitize user input against a schema | miss | with_output_schema (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | BaseRepository | miss |  |
| get_ranked_context | CeleryConfig | miss |  |
| get_ranked_context | initialize SQLAlchemy ORM engine with connection string | miss | SymbolWithFile (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | detect anomalies in time series data | miss | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | store and retrieve values from a Python LRU cache | miss | cache_dir (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | implement Django custom management command | miss | ALLOWED_COMMANDS (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context | test a FastAPI endpoint with the TestClient | miss | tests (benchmarks/harness/test_policy_integrity.py) |
| get_ranked_context | create a custom Django admin action | miss | actions (benchmarks/harness/task-bootstrap.py) |
| get_ranked_context | schedule a periodic task with Celery beat | miss | task_text (benchmarks/harness/harness_runner_common.py) |
| get_ranked_context | send a push notification to a mobile device | miss | send_notification (crates/codelens-engine/src/lsp/session.rs) |
| get_ranked_context | aggregate query results grouped by day | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context | HealthCheck | miss |  |
| get_ranked_context | DatabaseError | miss |  |
| get_ranked_context | convert pandas DataFrame to a list of records | miss | convert_rows (scripts/finetune/fetch_cosqa_retrieval.py) |
| get_ranked_context | read secrets from AWS Secrets Manager | miss | reader (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | encode a binary file as base64 string | miss | decode_embedding_bytes (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | verify an HMAC signature on an incoming webhook | miss | signature (scripts/finetune/collect_training_data.py) |
| get_ranked_context | find the closest matching item using fuzzy search | miss | search_symbols_fuzzy (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | run data validation and return a list of errors | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | split a list into chunks of a given size | miss | chunk_from_row (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | log a request and its response time as a middleware | miss | main (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | compute TF-IDF score for document ranking | miss | file_pagerank_scores (crates/codelens-engine/src/import_graph/mod.rs) |
| get_ranked_context | create a dataclass for a configuration section | miss | get_analysis_section (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | S3Client | miss |  |
| get_ranked_context | PushNotificationPayload | miss |  |
| get_ranked_context | summarize a long text using an LLM | miss | list_summaries (crates/codelens-mcp/src/artifact_store.rs) |
| get_ranked_context | run a full-text search query on a database table | miss | search (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | track model training metrics per epoch | miss | metrics (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | handle HTTP request with context and timeout | miss | DEFAULT_HTTP_REQUEST_TIMEOUT_SECONDS (benchmarks/benchmark_runtime_common.py) |
| get_ranked_context | connect to PostgreSQL database using pgx driver | miss | db (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | run a database transaction with rollback on error | miss | is_recoverable_sqlite_error (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | parse JSON request body into a struct | miss | parse_output_json (benchmarks/benchmark_runtime_common.py) |
| get_ranked_context | generate a JWT token for a user | miss | tokens (scripts/finetune/build_repo_adversarial_dataset.py) |
| get_ranked_context | validate a JWT token and return claims | miss | validate_tool_access (crates/codelens-mcp/src/dispatch_access.rs) |
| get_ranked_context | implement middleware for request logging | miss | JsonRpcRequest (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | write a gRPC server handler for a service method | miss | server_card_handler (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context | publish a message to a Kafka topic | miss | name_to_topic (scripts/finetune/augment_dataset.py) |
| get_ranked_context | consume messages from a Kafka topic | miss | topic (scripts/finetune/augment_dataset.py) |
| get_ranked_context | implement a retry loop with exponential backoff | miss | RETRY_DELAYS_MS (crates/codelens-engine/src/watcher.rs) |
| get_ranked_context | parse command-line flags using the flag package | miss | parse_bool_flag (benchmarks/harness/session-eval.py) |
| get_ranked_context | gracefully shut down an HTTP server on signal | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context | read configuration from YAML file | miss | YAML_QUERY (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | implement a worker pool for parallel processing | miss | record_analysis_worker_pool (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context | write unit test with table-driven test cases | miss | write_to_disk (crates/codelens-mcp/src/job_store.rs) |
| get_ranked_context | implement an LRU cache with expiration | miss | expired (crates/codelens-mcp/src/artifact_store.rs) |
| get_ranked_context | use sync.WaitGroup to wait for goroutines | miss | EffortLevel (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context | register routes on an HTTP mux with middleware | miss | router (crates/codelens-mcp/src/server/mod.rs) |
| get_ranked_context | read a file line by line using bufio scanner | miss | read_line_at (crates/codelens-engine/src/file_ops/mod.rs) |
| get_ranked_context | encode a struct to JSON and write to response | miss | json_response (crates/codelens-mcp/src/server/transport_http_support.rs) |
| get_ranked_context | connect to Redis and implement a simple set-get | miss | set_env_if_unset (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | measure function execution latency with prometheus | miss | test_latency (models/benchmark_full.py) |
| get_ranked_context | Config | miss | config (benchmarks/role-retrieval.py) |
| get_ranked_context | Server | miss | server (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | Repository | miss | UserRepository (crates/codelens-engine/tests/fixtures/sample_project/src/models.ts) |
| get_ranked_context | implement OpenTelemetry trace span for a function | miss | event_trace_counts (benchmarks/harness/harness_runner_common.py) |
| get_ranked_context | apply database migrations using goose | miss | MIGRATIONS (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | marshal proto message to binary format | miss | is_protocol_error (crates/codelens-mcp/src/error.rs) |
| get_ranked_context | implement health check endpoint for Kubernetes | miss | watcher_failure_health (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | seed the database with initial test data | miss | db (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | scan SQL rows into a slice of structs | miss | sqlite_related_paths (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | ErrNotFound | miss |  |
| get_ranked_context | handle an error by wrapping it with additional context | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | validate a struct using validator tags | miss | validate_identifier (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | log a structured message with fields using zap | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context | create a mock implementation for an interface | miss | create_and_get_session (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | compute SHA256 hash of a string | miss | compute_file_sha256 (benchmarks/role-retrieval.py) |
| get_ranked_context | JobWorker | miss |  |
| get_ranked_context | KafkaProducer | miss |  |
| get_ranked_context | serialize a struct to a map for dynamic querying | miss | run_stdio (crates/codelens-mcp/src/server/transport_stdio.rs) |
| get_ranked_context | implement a circuit breaker for external service calls | miss | insert_calls (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | rate limit requests using a token bucket algorithm | miss | buckets (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | upload a file to GCS bucket | miss | bucket (scripts/finetune/build_runtime_training_pipeline.py) |
| get_ranked_context | decode URL-encoded query parameters | miss | query (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | define a Spring REST controller endpoint | miss | does_not_suggest_locally_defined (crates/codelens-engine/src/auto_import.rs) |
| get_ranked_context | create a Spring JPA repository interface | miss | create (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | implement a Spring service layer with transaction | miss | with_transaction (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | configure Spring Security filter chain | miss | configure_daemon_mode (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | map a JPA entity to a database table | miss | backend_kind_display_stable (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | convert entity to DTO for API response | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | handle global exceptions with Spring ControllerAdvice | miss | does_not_promote_home_directory_from_global_codelens_marker (crates/codelens-engine/src/project.rs) |
| get_ranked_context | write a JUnit 5 test with mocked dependencies | miss | is_test_only_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | create a Spring Batch job for data processing | miss | batches (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | publish a Spring application event and listen to it | miss | FileEvent (crates/codelens-engine/src/vfs.rs) |
| get_ranked_context | configure Liquibase database changelog | miss | changed_files (scripts/quality-gate.sh) |
| get_ranked_context | implement a JWT authentication filter | miss | filtered (benchmarks/harness/test_policy_integrity.py) |
| get_ranked_context | create a Spring Cache configuration with Redis | miss | QUERY_CACHE (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | send an email with Spring Mail and Thymeleaf template | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context | validate request body with Bean Validation annotations | miss | validate_required_params (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context | UserServiceImpl | miss |  |
| get_ranked_context | JwtTokenProvider | miss |  |
| get_ranked_context | schedule a Spring task with a cron expression | miss | with (.github/workflows/build.yml) |
| get_ranked_context | implement an aspect for method-level logging | miss | set_effort_level (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | create a Spring WebClient for non-blocking HTTP calls | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context | write a custom Spring Health Indicator | miss | watcher_failure_health (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | PageableResponse | miss |  |
| get_ranked_context | ApiException | miss |  |
| get_ranked_context | implement Flyway migration script to add a column | miss | migrate (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | publish a message to a RabbitMQ exchange | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context | consume messages from a RabbitMQ queue | miss | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context | configure OpenAPI Swagger documentation for the API | miss | configure_daemon_mode (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | search records with dynamic JPA Specification | miss | all_with_embeddings (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context | RabbitMQConfig | miss |  |
| get_ranked_context | AuditListener | miss |  |
| get_ranked_context | implement an async TCP server with Tokio | miss | run_http (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context | deserialize JSON into a Rust struct with serde | miss | json_response (crates/codelens-mcp/src/server/transport_http_support.rs) |
| get_ranked_context | define a custom error type with thiserror | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | query a database row with sqlx and bind parameters | miss | db (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | spawn a background task with Tokio handle | miss | base (benchmarks/harness/codex-task-runner.py) |
| get_ranked_context | implement the Display trait for a custom type | miss | by_type (benchmarks/embedding-quality.py) |
| get_ranked_context | write a property-based test with proptest | miss | is_test_only_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | build a CLI argument parser with clap derive | miss | parser (benchmarks/harness/task-bootstrap.py) |
| get_ranked_context | implement an Axum route handler with state injection | miss | server_card_handler (crates/codelens-mcp/src/server/transport_http.rs) |
| get_ranked_context | use a channel to communicate between threads | miss | configured_embedding_threads (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | create a connection pool with deadpool-postgres | miss | pool (scripts/finetune/build_codex_dataset.py) |
| get_ranked_context | read and write a TOML configuration file | miss | TOML_QUERY (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | implement a custom Serde deserializer for a field | miss | str_field (crates/codelens-mcp/src/session_context.rs) |
| get_ranked_context | verify HMAC-SHA256 signature in Rust | miss | compute_file_sha256 (scripts/finetune/promotion_gate.py) |
| get_ranked_context | implement From trait for error type conversion | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | run integration test against a real database | miss | db (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | AppConfig | miss |  |
| get_ranked_context | DbPool | miss |  |
| get_ranked_context | use Arc and Mutex to share state across async tasks | miss | AnalysisQueueState (crates/codelens-mcp/src/analysis_queue.rs) |
| get_ranked_context | implement a trait for pluggable storage backends | miss | BackendKind (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | parse command-line arguments and run subcommands | miss | parse_lsp_args (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | emit structured tracing spans with tracing crate | miss | SymbolWithFile (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | check if two byte slices are equal in constant time | miss | for_each_file_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | AxumRouter | miss |  |
| get_ranked_context | implement retry logic with a backoff strategy | miss | index_files_with_retry (crates/codelens-engine/src/watcher.rs) |
| get_ranked_context | write a benchmark test with criterion | miss | bench (benchmarks/bench.sh) |
| get_ranked_context | TokenBucket | miss |  |
| get_ranked_context | handle a timeout on an async future | miss | timeout (benchmarks/token_efficiency_scenarios.py) |
| get_ranked_context | generate a random cryptographic nonce | miss | generate_typescript_import (crates/codelens-engine/src/auto_import.rs) |
| get_ranked_context | flatten a nested Result and propagate errors | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | define a Rails controller action to create a resource | miss | text_resource (crates/codelens-mcp/src/resources.rs) |
| get_ranked_context | write an ActiveRecord scope with a custom condition | miss | active_project_context (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | add a before_action callback to authenticate requests | miss | before (benchmarks/harness/refresh-routing-policy.py) |
| get_ranked_context | serialize an ActiveRecord model to JSON with JBuilder | miss | deferred_loading_active (crates/codelens-mcp/src/resource_context.rs) |
| get_ranked_context | create a background job with Sidekiq | miss | AnalysisJob (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context | write a Rails database migration to add a column | miss | migrate (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | validate presence and uniqueness of a model attribute | miss | validate_identifier (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | implement a custom Devise strategy for API token auth | miss | tokens (scripts/finetune/build_repo_adversarial_dataset.py) |
| get_ranked_context | write an RSpec request spec for a POST endpoint | miss | from_request (crates/codelens-mcp/src/resource_context.rs) |
| get_ranked_context | create a Rails service object for business logic | miss | set_created_at_for_test (crates/codelens-mcp/src/artifact_store.rs) |
| get_ranked_context | ApplicationRecord | miss |  |
| get_ranked_context | ApplicationMailer | miss |  |
| get_ranked_context | configure Rails routes with namespaced API endpoints | miss | dispatch_tool (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context | use ActiveJob to enqueue a mailer in background | miss | JobLifecycle (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context | configure ActiveStorage for file attachments | miss | AnalysisArtifactStore (crates/codelens-mcp/src/artifact_store.rs) |
| get_ranked_context | define a Laravel route with controller action | miss | router (crates/codelens-mcp/src/server/mod.rs) |
| get_ranked_context | create a Laravel Eloquent model with relationships | miss | create_or_resume (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | write a Laravel form request for input validation | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | dispatch a Laravel job to the queue | miss | run_analysis_job_from_queue (crates/codelens-mcp/src/tools/report_jobs.rs) |
| get_ranked_context | create a Laravel migration to add an index | miss | index_files (crates/codelens-engine/src/symbols/writer.rs) |
| get_ranked_context | implement a Laravel middleware to check API key | miss | extract_api_calls_inner (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | send an email with a Mailable and Blade template | miss | send_message (crates/codelens-engine/src/lsp/protocol.rs) |
| get_ranked_context | write a Laravel Observer to react to model events | miss | partition_events (crates/codelens-engine/src/vfs.rs) |
| get_ranked_context | create a Livewire component for real-time search | miss | search (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | define a Laravel event and listener pair | miss | normalize_events (crates/codelens-engine/src/vfs.rs) |
| get_ranked_context | ApiResource | miss |  |
| get_ranked_context | ServiceProvider | miss |  |
| get_ranked_context | query database using Laravel Query Builder with joins | miss | PHP_QUERY (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | write a PHPUnit feature test for a POST endpoint | miss | writer (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | cache a database query result with Laravel Cache | miss | cached_query (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | JWT token generation | miss | tokens (scripts/finetune/build_repo_adversarial_dataset.py) |
| get_ranked_context | form validation schema | miss | validation_path (scripts/finetune/build_runtime_training_pipeline.py) |
| get_ranked_context | file upload handler | miss | upsert_file (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | Redis cache client setup | miss | get_callees_cached (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | database connection singleton | miss |  |
| get_ranked_context | password strength validation | miss | validation_path (scripts/finetune/build_runtime_training_pipeline.py) |
| get_ranked_context | access token expiry check | miss | is_expired (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | image compression utility | miss | compression_threshold_offset (crates/codelens-mcp/src/client_profile.rs) |
| get_ranked_context | Next.js error page component | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | user session persistence hook | miss | session (crates/codelens-engine/src/lsp/mod.rs) |
| get_ranked_context | async database session factory | miss | session (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | user model serialization | miss | model (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | background task queue | miss | queue (benchmarks/render-summary.py) |
| get_ranked_context | Pydantic settings config | miss | PYTHON_QUERY (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | password reset token generation | miss | presets (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context | Django test factory for user | miss | tests (benchmarks/harness/test_policy_integrity.py) |
| get_ranked_context | ML model prediction endpoint | miss | model (scripts/finetune/train_v8_final.py) |
| get_ranked_context | CSV row parser utility | miss | parse_row_positive (scripts/finetune/contamination_audit.py) |
| get_ranked_context | context deadline propagation | miss | RankedContextEntry (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context | HTTP JSON error response | miss | json_response (crates/codelens-mcp/src/server/transport_http_support.rs) |
| get_ranked_context | goroutine safe counter | miss | safe_rename_report (crates/codelens-mcp/src/tools/reports/verifier_reports.rs) |
| get_ranked_context | database row scanner helper | miss | row (scripts/finetune/build_repo_adversarial_dataset.py) |
| get_ranked_context | OpenTelemetry span context | miss | RankedContextEntry (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context | JWT claims struct | miss | destructive (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | Spring bean factory method | miss | method (scripts/finetune/promotion_gate.py) |
| get_ranked_context | JPA entity auditing | miss | identity (scripts/finetune/compress_to_3layer.py) |
| get_ranked_context | custom validator annotation | miss | validation_pairs (scripts/finetune/build_runtime_training_pipeline.py) |
| get_ranked_context | REST API pagination response | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | async trait implementation | miss | rust_trait_impl (crates/codelens-engine/src/type_hierarchy.rs) |
| get_ranked_context | config file loading | miss | load_config (benchmarks/harness/harness-eval.py) |
| get_ranked_context | error chain conversion | miss | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | connection pool initialization | miss | pooled (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | middleware stack builder | miss | builder_minimal_mutation_behavior_unchanged (crates/codelens-mcp/src/integration_tests.rs) |
| get_ranked_context | ActiveRecord association helpers | miss | prefers_stdio_entrypoint_over_generic_read_helpers (crates/codelens-mcp/src/dispatch.rs) |
| get_ranked_context | Rails concern for soft delete | miss | delete_file (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | Sidekiq retry configuration | miss | RETRY_DELAYS_MS (crates/codelens-engine/src/watcher.rs) |
| get_ranked_context | Eloquent query scope | miss | embeddings_for_files (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context | Laravel service container binding | miss | UserService (crates/codelens-engine/tests/fixtures/sample_project/src/service.py) |
| get_ranked_context | Blade component rendering | miss | components (.github/workflows/ci.yml) |
| get_ranked_context | SignedUploadUrlResult | miss |  |
| get_ranked_context | CheckoutSessionParams | miss |  |
| get_ranked_context | WebSocketMessage | miss |  |
| get_ranked_context | FeatureFlags | miss |  |
| get_ranked_context | JobPayload | miss |  |
| get_ranked_context | EmailTemplate | miss |  |
| get_ranked_context | AnomalyResult | miss |  |
| get_ranked_context | ImportRecord | miss |  |
| get_ranked_context | WebhookEvent | miss |  |
| get_ranked_context | PaginatedResponse | miss |  |
| get_ranked_context | UserDTO | miss |  |
| get_ranked_context | CircuitBreaker | miss |  |
| get_ranked_context | MetricsCollector | miss |  |
| get_ranked_context | UserDto | miss |  |
| get_ranked_context | OrderEntity | miss |  |
| get_ranked_context | HandlerError | miss |  |
| get_ranked_context | StorageConfig | miss |  |
| get_ranked_context | ApplicationJob | miss |  |
| get_ranked_context | BaseController | miss |  |

