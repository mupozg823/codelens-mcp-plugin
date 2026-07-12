//! Structured error types for CodeLens MCP tools.
//! Maps to JSON-RPC error codes for protocol-level error reporting.

use serde_json::{Value, json};

#[derive(Debug, thiserror::Error)]
pub enum CodeLensError {
    // ── Protocol errors (JSON-RPC level) ──────────────────────────────
    /// Missing or invalid tool parameter (JSON-RPC -32602).
    #[error("Missing required parameter: {0}")]
    MissingParam(String),

    /// Unknown tool name (JSON-RPC -32601).
    #[error("Unknown tool: {0}")]
    ToolNotFound(String),

    // ── User errors ───────────────────────────────────────────────────
    /// Resource (file, memory, symbol) not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Validation error — invalid range, path traversal, etc.
    #[error("Validation error: {0}")]
    Validation(String),

    /// #347: content mutation on a shared-daemon HTTP session with no
    /// explicit project binding — the write could land in the daemon's
    /// default project instead of the caller's repository.
    #[cfg(feature = "http")]
    #[error(
        "project_binding_required: tool `{tool}` is blocked because this HTTP session has no explicit project binding, so the mutation may target the wrong repository. Bind first via `prepare_harness_session` with project=<absolute workspace root>, `activate_project`, the `x-codelens-project` header, or initialize `params.project`. Operator override: CODELENS_ALLOW_UNBOUND_MUTATION=1."
    )]
    ProjectBindingRequired { tool: String },

    /// A bind/activation request targeted the process user's home directory
    /// as the project root. Indexing the entire home tree pins the daemon
    /// (a huge symbol index over thousands of files) and was the cause of
    /// `prepare_harness_session(project=$HOME)` client-timeout hangs. A repo
    /// *inside* home is unaffected — only the home root itself is refused.
    /// Escape hatch: `CODELENS_ALLOW_HOME_PROJECT=1`.
    #[error(
        "home_root_rejected: refusing to bind project root `{root}` because it is the process user's home directory; indexing the whole home tree is unsupported and pins the daemon. Bind the specific repo's absolute path instead. Operator override: CODELENS_ALLOW_HOME_PROJECT=1."
    )]
    HomeRootRejected { root: String },

    // ── Capability errors ─────────────────────────────────────────────
    /// Feature not available (e.g., semantic search without embeddings).
    #[cfg(feature = "semantic")]
    #[error("Feature unavailable: {0}")]
    FeatureUnavailable(String),

    /// Language not supported for the requested operation.
    #[error("Language '{language}' does not support '{feature}'")]
    #[allow(dead_code)]
    LanguageUnsupported { language: String, feature: String },

    /// LSP server not attached or not configured for this project.
    #[error("LSP not attached: {0}")]
    LspNotAttached(String),

    /// Symbol index not ready (initial indexing still in progress).
    #[error("Index not ready: {0}")]
    #[allow(dead_code)]
    IndexNotReady(String),

    // ── System errors ─────────────────────────────────────────────────
    /// LSP server unavailable or error.
    #[error("LSP error: {0}")]
    LspError(String),

    /// Operation timed out.
    #[error("Timeout: {operation} after {elapsed_ms}ms")]
    Timeout { operation: String, elapsed_ms: u64 },

    /// Session expired or invalid.
    #[error("Stale session: {0}")]
    #[allow(dead_code)]
    StaleSession(String),

    /// Resource limit exceeded (e.g., too many concurrent LSP sessions).
    #[error("Resource exhausted: {0}")]
    #[allow(dead_code)]
    ResourceExhausted(String),

    /// ADR-0009 §1: principal does not hold the role required by the
    /// tool. Surfaces as JSON-RPC -32008 (note: ADR named -32004 but
    /// that code is already taken by `IndexNotReady`).
    #[error(
        "Permission denied: principal '{principal}' (role={principal_role}) cannot call tool '{tool}' which requires role={required_role}"
    )]
    PermissionDenied {
        principal: String,
        principal_role: String,
        tool: String,
        required_role: String,
    },

    /// I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal/unexpected error.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl CodeLensError {
    /// Map to a JSON-RPC error code. Used by dispatch_tool for protocol-level errors.
    pub fn jsonrpc_code(&self) -> i64 {
        match self {
            // Protocol errors
            Self::MissingParam(_) => -32602,
            Self::ToolNotFound(_) => -32601,
            // User errors
            Self::NotFound(_) => -32000,
            Self::Validation(_) => -32003,
            #[cfg(feature = "http")]
            Self::ProjectBindingRequired { .. } => -32003,
            Self::HomeRootRejected { .. } => -32003,
            // Capability errors
            #[cfg(feature = "semantic")]
            Self::FeatureUnavailable(_) => -32002,
            Self::LanguageUnsupported { .. } => -32002,
            Self::LspNotAttached(_) => -32001,
            Self::IndexNotReady(_) => -32004,
            // System errors
            Self::LspError(_) => -32001,
            Self::Timeout { .. } => -32005,
            Self::StaleSession(_) => -32006,
            Self::ResourceExhausted(_) => -32007,
            Self::PermissionDenied { .. } => -32008,
            Self::Io(_) => -32603,
            Self::Internal(_) => -32603,
        }
    }

    /// Whether this is a protocol-level error (should be returned as JSON-RPC error).
    pub fn is_protocol_error(&self) -> bool {
        matches!(self, Self::ToolNotFound(_) | Self::MissingParam(_))
    }

    /// Structured recovery hint derived from the error variant.
    ///
    /// Agents can parse this field to select a fallback action without
    /// string-matching the error message. Returns `None` when no specific
    /// recovery path is known.
    pub fn recovery_hint(&self) -> Option<RecoveryHint> {
        match self {
            Self::MissingParam(field) => Some(RecoveryHint::RequireField {
                field: field.clone(),
            }),
            Self::ToolNotFound(_) => Some(RecoveryHint::FallbackTool {
                tool: "get_capabilities".to_owned(),
                reason: "list currently available tools and features".to_owned(),
            }),
            #[cfg(feature = "semantic")]
            Self::FeatureUnavailable(_) => Some(RecoveryHint::RequireFeature {
                feature: "semantic".to_owned(),
                install: "rebuild with `--features semantic` and call index_embeddings".to_owned(),
            }),
            Self::LspNotAttached(_) => Some(RecoveryHint::FallbackTool {
                tool: "find_symbol".to_owned(),
                reason: "tree-sitter index satisfies most symbol lookups without LSP".to_owned(),
            }),
            Self::IndexNotReady(_) => Some(RecoveryHint::RetryAfterSeconds { seconds: 5 }),
            #[cfg(feature = "http")]
            Self::ProjectBindingRequired { .. } => Some(RecoveryHint::FallbackTool {
                tool: "prepare_harness_session".to_owned(),
                reason: "bind this session to a workspace: pass project=<absolute workspace root> (or attach the x-codelens-project header), then retry the mutation".to_owned(),
            }),
            Self::HomeRootRejected { .. } => Some(RecoveryHint::FallbackTool {
                tool: "prepare_harness_session".to_owned(),
                reason: "bind the specific repo's absolute path; indexing the whole home tree is unsupported. Operator override: CODELENS_ALLOW_HOME_PROJECT=1".to_owned(),
            }),
            Self::Timeout { .. } => Some(RecoveryHint::FallbackTool {
                tool: "start_analysis_job".to_owned(),
                reason: "move heavy work to the durable job queue".to_owned(),
            }),
            Self::ResourceExhausted(_) => Some(RecoveryHint::RetryAfterSeconds { seconds: 10 }),
            Self::PermissionDenied { required_role, .. } => Some(RecoveryHint::RequireRole {
                required_role: required_role.clone(),
                verify_tool: "get_capabilities".to_owned(),
            }),
            _ => None,
        }
    }

    /// Build the `(message, data)` pair for a protocol-level JSON-RPC error.
    ///
    /// `called_tool` is the tool name from the request and `known_tools` the
    /// live dispatch surface. For an unknown tool this attaches
    /// "did you mean …" suggestions to both the message and the structured
    /// `data.recovery_hint`, closing the previously bare protocol-error string
    /// so agents can self-correct without re-parsing prose. `data` is `None`
    /// only when the variant carries no recovery signal.
    pub(crate) fn protocol_error_data(
        &self,
        called_tool: &str,
        known_tools: &[&str],
    ) -> (String, Option<Value>) {
        match self {
            Self::ToolNotFound(_) => {
                let suggestions = did_you_mean(called_tool, known_tools, MAX_DID_YOU_MEAN);
                let mut message = self.to_string();
                if !suggestions.is_empty() {
                    message.push_str(&format!(" Did you mean: {}?", suggestions.join(", ")));
                }
                let hint = Self::unknown_tool_recovery_hint(suggestions);
                (message, Some(json!({ "recovery_hint": hint })))
            }
            _ => {
                let data = self
                    .recovery_hint()
                    .map(|hint| json!({ "recovery_hint": hint }));
                (self.to_string(), data)
            }
        }
    }

    /// Enriched recovery hint for an unknown-tool error given the closest
    /// candidate names from the live dispatch surface. `did_you_mean` may be
    /// empty when nothing is close enough — the caller then still gets the
    /// `get_capabilities` discovery fallback.
    pub(crate) fn unknown_tool_recovery_hint(did_you_mean: Vec<String>) -> RecoveryHint {
        RecoveryHint::UnknownTool {
            did_you_mean,
            fallback_tool: "get_capabilities".to_owned(),
            fallback_reason: "list currently available tools and features".to_owned(),
        }
    }
}

/// Structured recovery hint — lets agents pick a fallback action without
/// parsing error strings. Emitted in the error response when the variant
/// has a clear recovery path.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoveryHint {
    /// Call this tool instead; it satisfies the same intent by another route.
    FallbackTool { tool: String, reason: String },
    /// Feature must be enabled via build flag or data setup before the call succeeds.
    #[cfg(feature = "semantic")]
    RequireFeature { feature: String, install: String },
    /// A required input field is missing — name it explicitly so the agent can supply it.
    RequireField { field: String },
    /// The operation can succeed if retried after a short wait.
    RetryAfterSeconds { seconds: u64 },
    /// The named tool does not exist. `did_you_mean` lists the closest known
    /// tool names (shared-token / edit-distance match), best first;
    /// `fallback_tool` is the discovery tool that enumerates the full surface
    /// when no close match fits.
    UnknownTool {
        did_you_mean: Vec<String>,
        fallback_tool: String,
        fallback_reason: String,
    },
    /// The principal lacks the role the tool requires — name the missing role
    /// and the tool that lists the caller's currently available surface.
    RequireRole {
        required_role: String,
        verify_tool: String,
    },
}

/// Maximum number of "did you mean" candidates surfaced on an unknown tool.
const MAX_DID_YOU_MEAN: usize = 3;

/// Maximum Levenshtein distance for a raw-typo match (task threshold: ≤ 3).
const MAX_EDIT_DISTANCE: usize = 3;

/// Split a tool name into its underscore-separated tokens (empty tokens dropped).
fn tokens(name: &str) -> Vec<&str> {
    name.split('_').filter(|t| !t.is_empty()).collect()
}

/// Levenshtein edit distance (iterative two-row). Kept as a small local impl
/// because the dependency tree carries no fuzzy-match helper and the task
/// forbids adding one.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Rank known tool names by similarity to `unknown` and return up to `limit`
/// "did you mean" candidates, best first.
///
/// A candidate qualifies when it shares at least one underscore token, is a
/// prefix/substring match, or lands within `MAX_EDIT_DISTANCE` edits. Ranking
/// weights each shared token by its inverse frequency across `known_tools`, so
/// a rare, distinctive token (e.g. `impact`) outranks ubiquitous ones
/// (`get`/`find`/`list`). This lets a token-far typo like `get_impact_analysis`
/// still surface `impact_report`, which plain edit distance would miss. A
/// prefix/substring bonus plus an edit-similarity term break ties and catch
/// pure typos that share no token (e.g. `find_symbl` → `find_symbol`).
pub(crate) fn did_you_mean(unknown: &str, known_tools: &[&str], limit: usize) -> Vec<String> {
    if unknown.is_empty() || known_tools.is_empty() || limit == 0 {
        return Vec::new();
    }
    let unknown_tokens = tokens(unknown);

    // Document frequency of each token across the known surface → IDF weight.
    let total = known_tools.len() as f64;
    let mut doc_freq: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for tool in known_tools {
        let mut seen = tokens(tool);
        seen.sort_unstable();
        seen.dedup();
        for tok in seen {
            *doc_freq.entry(tok).or_insert(0) += 1;
        }
    }
    let idf = |tok: &str| -> f64 {
        let df = doc_freq.get(tok).copied().unwrap_or(1).max(1) as f64;
        (total / df).ln().max(0.0)
    };

    let mut scored: Vec<(f64, usize, &str)> = Vec::new();
    for &candidate in known_tools {
        if candidate == unknown {
            continue; // an exact match is not a "did you mean"
        }
        let cand_tokens = tokens(candidate);
        // Reward the single most-distinctive shared token, then discount any
        // extras. A rare token (`impact`) must beat several generic ones
        // (`get` + `analysis`), otherwise `get_impact_analysis` would rank the
        // `get_analysis_*` tools above the intended `impact_report`.
        let mut shared_idfs: Vec<f64> = Vec::new();
        for &ut in &unknown_tokens {
            if cand_tokens.contains(&ut) {
                shared_idfs.push(idf(ut));
            }
        }
        let shared = shared_idfs.len();
        let token_score = if shared_idfs.is_empty() {
            0.0
        } else {
            let max = shared_idfs.iter().copied().fold(0.0f64, f64::max);
            let sum: f64 = shared_idfs.iter().sum();
            max + 0.3 * (sum - max)
        };
        let is_affix = candidate.starts_with(unknown)
            || unknown.starts_with(candidate)
            || candidate.contains(unknown)
            || unknown.contains(candidate);
        let dist = levenshtein(unknown, candidate);
        let close_typo = dist <= MAX_EDIT_DISTANCE;

        if shared == 0 && !is_affix && !close_typo {
            continue; // does not qualify under any signal
        }

        let max_len = unknown.chars().count().max(candidate.chars().count()) as f64;
        let edit_sim = if max_len > 0.0 {
            1.0 - (dist as f64 / max_len)
        } else {
            0.0
        };
        let affix_bonus = if is_affix { 3.0 } else { 0.0 };
        let score = token_score + affix_bonus + edit_sim;
        scored.push((score, dist, candidate));
    }

    // Highest score first; ties broken by smaller edit distance then name so
    // the ordering is deterministic.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(b.2))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, name)| name.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_code_mappings() {
        assert_eq!(
            CodeLensError::MissingParam("x".into()).jsonrpc_code(),
            -32602
        );
        assert_eq!(
            CodeLensError::ToolNotFound("y".into()).jsonrpc_code(),
            -32601
        );
        assert_eq!(CodeLensError::NotFound("z".into()).jsonrpc_code(), -32000);
        assert_eq!(
            CodeLensError::Validation("bad".into()).jsonrpc_code(),
            -32003
        );
        assert_eq!(
            CodeLensError::LanguageUnsupported {
                language: "rs".into(),
                feature: "rename".into(),
            }
            .jsonrpc_code(),
            -32002
        );
        assert_eq!(
            CodeLensError::LspNotAttached("".into()).jsonrpc_code(),
            -32001
        );
        assert_eq!(
            CodeLensError::IndexNotReady("".into()).jsonrpc_code(),
            -32004
        );
        assert_eq!(
            CodeLensError::Timeout {
                operation: "op".into(),
                elapsed_ms: 100,
            }
            .jsonrpc_code(),
            -32005
        );
        assert_eq!(
            CodeLensError::StaleSession("".into()).jsonrpc_code(),
            -32006
        );
        assert_eq!(
            CodeLensError::ResourceExhausted("".into()).jsonrpc_code(),
            -32007
        );
        assert_eq!(
            CodeLensError::PermissionDenied {
                principal: "p".into(),
                principal_role: "r".into(),
                tool: "t".into(),
                required_role: "R".into(),
            }
            .jsonrpc_code(),
            -32008
        );
        assert_eq!(
            CodeLensError::Io(std::io::Error::other("x")).jsonrpc_code(),
            -32603
        );
        assert_eq!(
            CodeLensError::Internal(anyhow::anyhow!("x")).jsonrpc_code(),
            -32603
        );
    }

    #[test]
    fn is_protocol_error_only_for_protocol_variants() {
        assert!(CodeLensError::MissingParam("x".into()).is_protocol_error());
        assert!(CodeLensError::ToolNotFound("y".into()).is_protocol_error());
        assert!(!CodeLensError::NotFound("z".into()).is_protocol_error());
        assert!(!CodeLensError::Validation("bad".into()).is_protocol_error());
    }

    #[test]
    fn recovery_hint_variants() {
        assert_eq!(
            CodeLensError::MissingParam("field_name".into()).recovery_hint(),
            Some(RecoveryHint::RequireField {
                field: "field_name".into()
            })
        );
        assert_eq!(
            CodeLensError::ToolNotFound("x".into()).recovery_hint(),
            Some(RecoveryHint::FallbackTool {
                tool: "get_capabilities".into(),
                reason: "list currently available tools and features".into(),
            })
        );
        assert_eq!(
            CodeLensError::LspNotAttached("x".into()).recovery_hint(),
            Some(RecoveryHint::FallbackTool {
                tool: "find_symbol".into(),
                reason: "tree-sitter index satisfies most symbol lookups without LSP".into(),
            })
        );
        assert_eq!(
            CodeLensError::IndexNotReady("x".into()).recovery_hint(),
            Some(RecoveryHint::RetryAfterSeconds { seconds: 5 })
        );
        assert_eq!(
            CodeLensError::ResourceExhausted("x".into()).recovery_hint(),
            Some(RecoveryHint::RetryAfterSeconds { seconds: 10 })
        );
        assert_eq!(
            CodeLensError::Timeout {
                operation: "op".into(),
                elapsed_ms: 100,
            }
            .recovery_hint(),
            Some(RecoveryHint::FallbackTool {
                tool: "start_analysis_job".into(),
                reason: "move heavy work to the durable job queue".into(),
            })
        );
        assert_eq!(CodeLensError::Validation("x".into()).recovery_hint(), None);
    }

    #[cfg(feature = "http")]
    #[test]
    fn project_binding_required_carries_code_and_recovery_hint() {
        let err = CodeLensError::ProjectBindingRequired {
            tool: "write_memory".into(),
        };
        assert_eq!(err.jsonrpc_code(), -32003);
        let message = err.to_string();
        assert!(
            message.starts_with("project_binding_required:"),
            "{message}"
        );
        assert!(message.contains("write_memory"), "{message}");
        assert!(
            message.contains("CODELENS_ALLOW_UNBOUND_MUTATION"),
            "{message}"
        );
        match err.recovery_hint() {
            Some(RecoveryHint::FallbackTool { tool, reason }) => {
                assert_eq!(tool, "prepare_harness_session");
                assert!(
                    reason.contains("project=<absolute workspace root>"),
                    "{reason}"
                );
                assert!(reason.contains("x-codelens-project"), "{reason}");
            }
            other => panic!("expected FallbackTool recovery hint, got {other:?}"),
        }
    }

    #[test]
    fn home_root_rejected_carries_code_and_recovery_hint() {
        let err = CodeLensError::HomeRootRejected {
            root: "/Users/dev".to_owned(),
        };
        assert_eq!(err.jsonrpc_code(), -32003);
        let message = err.to_string();
        assert!(message.starts_with("home_root_rejected:"), "{message}");
        assert!(message.contains("/Users/dev"), "{message}");
        assert!(message.contains("CODELENS_ALLOW_HOME_PROJECT"), "{message}");
        match err.recovery_hint() {
            Some(RecoveryHint::FallbackTool { tool, reason }) => {
                assert_eq!(tool, "prepare_harness_session");
                assert!(
                    reason.contains("bind the specific repo's absolute path"),
                    "{reason}"
                );
                assert!(
                    reason.contains("indexing the whole home tree is unsupported"),
                    "{reason}"
                );
            }
            other => panic!("expected FallbackTool recovery hint, got {other:?}"),
        }
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn recovery_hint_semantic_feature() {
        assert_eq!(
            CodeLensError::FeatureUnavailable("embed".into()).recovery_hint(),
            Some(RecoveryHint::RequireFeature {
                feature: "semantic".into(),
                install: "rebuild with `--features semantic` and call index_embeddings".into(),
            })
        );
    }

    /// Representative slice of the live dispatch surface (`tools/list`), used to
    /// exercise did-you-mean ranking against the real token distribution — the
    /// `get_*` / `find_*` noise that a naive shared-token count would rank above
    /// the intended target.
    const KNOWN_TOOLS: &[&str] = &[
        "get_current_config",
        "read_file",
        "list_dir",
        "find_file",
        "find_annotations",
        "find_tests",
        "get_symbols_overview",
        "find_symbol",
        "get_ranked_context",
        "bm25_symbol_search",
        "search_symbols_fuzzy",
        "get_complexity",
        "refresh_symbol_index",
        "find_referencing_symbols",
        "get_file_diagnostics",
        "search_workspace_symbols",
        "get_type_hierarchy",
        "resolve_symbol_target",
        "find_declaration",
        "find_implementations",
        "get_diagnostics_for_symbol",
        "plan_symbol_rename",
        "get_lsp_recipe",
        "get_changed_files",
        "get_callers",
        "get_callees",
        "find_scoped_references",
        "get_symbol_importance",
        "verify_change_readiness",
        "onboard_project",
        "analyze_change_request",
        "orchestrate_change",
        "module_boundary_report",
        "mermaid_module_graph",
        "safe_rename_report",
        "unresolved_reference_check",
        "dead_code_report",
        "impact_report",
        "refactor_safety_report",
        "diff_aware_references",
        "start_analysis_job",
        "get_analysis_job",
        "cancel_analysis_job",
        "list_analysis_jobs",
        "list_analysis_artifacts",
        "get_analysis_section",
        "explore_codebase",
        "trace_request_path",
        "review_architecture",
        "plan_safe_refactor",
        "cleanup_duplicate_logic",
        "review_changes",
        "diagnose_issues",
        "activate_project",
        "prepare_harness_session",
        "register_agent_work",
        "list_active_agents",
        "claim_files",
        "release_files",
        "get_watch_status",
        "prune_index_failures",
        "add_queryable_project",
        "remove_queryable_project",
        "query_project",
        "list_queryable_projects",
        "set_preset",
        "set_profile",
        "get_capabilities",
        "get_tool_metrics",
        "audit_builder_session",
        "audit_planner_session",
        "export_session_markdown",
        "audit_log_query",
        "audit_tool_surface_consistency",
        "find_phantom_modules",
        "find_redundant_definitions",
        "find_over_visible_apis",
        "find_misplaced_code",
        "find_similar_code",
        "find_code_duplicates",
        "classify_symbol",
        "audit_memory_consistency",
        "list_memories",
        "read_memory",
        "write_memory",
        "delete_memory",
        "rename_memory",
        "archive_memory",
        "restore_memory",
        "list_archived",
        "read_policy",
        "semantic_search",
        "embedding_coverage_report",
        "index_embeddings",
    ];

    #[test]
    fn levenshtein_basic_distances() {
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn did_you_mean_token_far_typo_surfaces_impact_report() {
        // Edit distance to `impact_report` is large, but the rare shared token
        // `impact` must still float it to the top over the many `get_*` tools.
        let candidates = did_you_mean("get_impact_analysis", KNOWN_TOOLS, 3);
        assert!(
            candidates.contains(&"impact_report".to_owned()),
            "expected impact_report among {candidates:?}"
        );
        assert_eq!(
            candidates.first().map(String::as_str),
            Some("impact_report"),
            "impact_report should rank first, got {candidates:?}"
        );
    }

    #[test]
    fn did_you_mean_plural_typo_surfaces_reference_tool() {
        let candidates = did_you_mean("find_references", KNOWN_TOOLS, 3);
        assert!(
            candidates.contains(&"find_scoped_references".to_owned()),
            "expected find_scoped_references among {candidates:?}"
        );
        // Every suggestion should be reference/find related, never noise.
        assert!(
            candidates
                .iter()
                .all(|c| c.contains("reference") || c.starts_with("find_")),
            "unexpected noise in {candidates:?}"
        );
    }

    #[test]
    fn did_you_mean_prefix_typo_surfaces_prepare_harness_session() {
        let candidates = did_you_mean("prepare_harness", KNOWN_TOOLS, 3);
        assert_eq!(
            candidates,
            vec!["prepare_harness_session".to_owned()],
            "only prepare_harness_session should qualify, got {candidates:?}"
        );
    }

    #[test]
    fn did_you_mean_no_candidate_for_gibberish() {
        assert!(did_you_mean("zzqqxx_nonsense_blob", KNOWN_TOOLS, 3).is_empty());
    }

    #[test]
    fn protocol_error_data_tool_not_found_carries_did_you_mean() {
        let err = CodeLensError::ToolNotFound("get_impact_analysis".to_owned());
        let (message, data) = err.protocol_error_data("get_impact_analysis", KNOWN_TOOLS);
        assert!(
            message.contains("Did you mean") && message.contains("impact_report"),
            "message should name the candidate: {message}"
        );
        let data = data.expect("ToolNotFound must carry structured data");
        let hint = &data["recovery_hint"];
        assert_eq!(hint["kind"], "unknown_tool");
        assert_eq!(hint["fallback_tool"], "get_capabilities");
        let did_you_mean: Vec<&str> = hint["did_you_mean"]
            .as_array()
            .expect("did_you_mean array")
            .iter()
            .map(|v| v.as_str().unwrap_or_default())
            .collect();
        assert!(
            did_you_mean.contains(&"impact_report"),
            "recovery_hint.did_you_mean should include impact_report: {did_you_mean:?}"
        );
    }

    #[test]
    fn protocol_error_data_missing_param_carries_require_field() {
        let err = CodeLensError::MissingParam("relative_path".to_owned());
        let (message, data) = err.protocol_error_data("find_symbol", KNOWN_TOOLS);
        assert!(!message.contains("Did you mean"), "{message}");
        let data = data.expect("MissingParam must carry structured data");
        assert_eq!(data["recovery_hint"]["kind"], "require_field");
        assert_eq!(data["recovery_hint"]["field"], "relative_path");
    }

    #[test]
    fn permission_denied_recovery_hint_names_required_role() {
        let err = CodeLensError::PermissionDenied {
            principal: "reviewer".into(),
            principal_role: "read-only".into(),
            tool: "rename_symbol".into(),
            required_role: "mutation-enabled".into(),
        };
        assert_eq!(
            err.recovery_hint(),
            Some(RecoveryHint::RequireRole {
                required_role: "mutation-enabled".into(),
                verify_tool: "get_capabilities".into(),
            })
        );
        // And it flows through the structured data path for non-protocol errors.
        let (_, data) = err.protocol_error_data("rename_symbol", KNOWN_TOOLS);
        let data = data.expect("PermissionDenied exposes a recovery hint");
        assert_eq!(data["recovery_hint"]["kind"], "require_role");
        assert_eq!(data["recovery_hint"]["required_role"], "mutation-enabled");
    }
}
