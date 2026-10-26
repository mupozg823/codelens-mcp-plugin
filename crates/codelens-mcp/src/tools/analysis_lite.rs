//! Adapter for the generated analysis-lite contract and typed engine evidence.

use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::{BackendKind, Freshness};
use crate::tool_defs::analysis_lite::limit;
use crate::tool_defs::tool::ToolHandler;
use crate::tool_runtime::{ToolResult, success_meta};
use codelens_engine::analysis_lite::{
    AnalysisQuery, DeclarationSelector, EvidenceLimits, EvidenceStatus,
};
use codelens_engine::oxc_analysis::ByteSpan;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) fn handler(target: &str) -> Option<ToolHandler> {
    match target {
        "analysis_lite_refs" => Some(Arc::new(|state, args| {
            run(state, args, "analysis_lite_refs", AnalysisQuery::References)
        })),
        "analysis_lite_impact" => Some(Arc::new(|state, args| {
            run(state, args, "analysis_lite_impact", AnalysisQuery::Impact)
        })),
        _ => None,
    }
}

fn run(state: &AppState, args: &Value, target: &str, query: AnalysisQuery) -> ToolResult {
    let selector_args = &args["target"];
    let index = state.symbol_index();
    let generation = index.committed_generation();
    if let Some(expected) = args.get("generation").and_then(Value::as_u64)
        && expected != generation
    {
        return Err(CodeLensError::IndexGenerationChanged {
            project: state.current_project_scope(),
            before: expected,
            after: generation,
        });
    }
    let name_span = match (
        selector_args.get("start_byte"),
        selector_args.get("end_byte"),
    ) {
        (None, None) => None,
        (Some(start), Some(end)) => {
            let span = ByteSpan {
                start: start.as_u64().expect("schema-validated start") as usize,
                end: end.as_u64().expect("schema-validated end") as usize,
            };
            if span.start >= span.end {
                return Err(CodeLensError::Validation(
                    "analysis-lite name-token span must have start_byte < end_byte".to_owned(),
                ));
            }
            Some(span)
        }
        _ => {
            return Err(CodeLensError::Validation(
                "analysis-lite start_byte and end_byte must be supplied together".to_owned(),
            ));
        }
    };
    let selector = DeclarationSelector {
        file: selector_args["file"]
            .as_str()
            .expect("schema-validated file")
            .to_owned(),
        name: selector_args
            .get("name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        symbol_id: selector_args
            .get("symbol_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        name_span,
    };
    let limits = EvidenceLimits {
        // Unused limits retain a conservative nonzero engine value; only the
        // active query's advertised limits influence its result shape.
        max_results: if query == AnalysisQuery::References {
            limit(target, args, "max_results")
        } else {
            100
        },
        max_depth: if query == AnalysisQuery::Impact {
            limit(target, args, "max_depth")
        } else {
            3
        },
        max_nodes: if query == AnalysisQuery::Impact {
            limit(target, args, "max_nodes")
        } else {
            40
        },
        max_edges: if query == AnalysisQuery::Impact {
            limit(target, args, "max_edges")
        } else {
            100
        },
        max_files: limit(target, args, "max_files"),
    };
    let evidence = codelens_engine::analysis_lite::query(
        &state.project(),
        &index,
        query,
        &selector,
        args.get("path").and_then(Value::as_str),
        limits,
    );
    if evidence.status == EvidenceStatus::Error {
        return Err(CodeLensError::AnalysisEvidence(Box::new(evidence)));
    }
    let partial = evidence.status != EvidenceStatus::Ok;
    let mut payload =
        serde_json::to_value(evidence).map_err(|e| CodeLensError::Internal(e.into()))?;
    payload["freshness"]["watcher_running"] =
        json!(state.watcher_stats().map(|stats| stats.running));
    payload["freshness"]["watcher_start_error"] = json!(state.watcher_error());
    // This read preserves DB errors as errors, rather than the legacy health
    // helper's unwrap_or_default() zero-failure interpretation.
    payload["freshness"]["index_failure_summary"] = match index.db().index_failure_summary(900) {
        Ok(summary) => {
            json!({"status":"observed", "recent_failures":summary.recent_failures, "total_failures":summary.total_failures, "persistent_failures":summary.persistent_failures})
        }
        Err(error) => json!({"status":"error", "error":error.to_string()}),
    };
    let observed_bytes = serde_json::to_vec(&payload)
        .map_err(|error| CodeLensError::Internal(error.into()))?
        .len();
    let limit_bytes = crate::tool_defs::analysis_lite::max_response_bytes();
    if observed_bytes > limit_bytes {
        return Err(CodeLensError::AnalysisLiteResponseTooLarge {
            observed_bytes,
            limit_bytes,
        });
    }
    let mut meta = success_meta(BackendKind::Hybrid, 0.0);
    meta.partial = partial;
    meta.freshness = Freshness::Indexed;
    meta.degraded_reason = Some("analysis-lite coverage and freshness are defined by the typed evidence; no completeness confidence is assigned".to_owned());
    Ok((payload, meta))
}

/// Structured outputs are intentionally the same evidence shape on both
/// facades. Evidence contents come from the typed engine, never serializer
/// reconstruction of a missing identity, location, backend or count.
pub(crate) fn output_schema() -> Value {
    json!({
        "type":"object",
        "required":["schema_version","capability","project","target","snapshot","scope","status","results","limits","truncation","unresolved","freshness","errors"],
        "properties":{
            "schema_version":{"type":"string","const":"analysis-lite/v1"},
            "capability":{"type":["string","null"]},
            "project":{"type":["string","null"]},
            "target":{"type":["object","null"]},
            "snapshot":{"type":["object","null"]},
            "scope":{"type":["object","null"]},
            "status":{"type":"string","enum":["ok","partial","unsupported","error"]},
            "results":{"type":"object","properties":{
                "references":{"type":"array","items":{"type":"object"}},
                "nodes":{"type":"array","items":{"type":"object"}},
                "edges":{"type":"array","items":{"type":"object"}},
                "selection_candidates":{"type":"array","items":{"type":"object"}},
                "total":{"type":["integer","null"]}
            }},
            "limits":{"type":["object","null"]},
            "truncation":{"type":["object","null"]},
            "unresolved":{"type":["object","null"]},
            "freshness":{"type":["object","null"]},
            "errors":{"type":"array","items":{"type":"object"}}
        }
    })
}

/// A rejected call has no executed source snapshot or canonical declaration.
/// Keep those fields null, and retain the original typed runtime error.
pub(crate) fn rejected_payload(state: &AppState, args: &Value, error: &CodeLensError) -> Value {
    let session = crate::session_context::SessionRequestContext::from_json(args);
    let project = if session.is_local() {
        Some(state.current_project_scope())
    } else if session.project_binding_is_explicit() {
        session
            .project_path
            .as_deref()
            .and_then(|path| codelens_engine::ProjectRoot::new(path).ok())
            .map(|project| project.as_path().display().to_string())
    } else {
        None
    };
    json!({
        "schema_version":"analysis-lite/v1", "capability":null, "project":project,
        "target":null,"snapshot":null,"scope":null,"status":"error",
        "results":{"references":[],"nodes":[],"edges":[],"selection_candidates":[],"total":null},
        "limits":null,"truncation":null,"unresolved":null,"freshness":null,
        "errors":[{"code":error.kind(),"backend":"runtime","message":error.to_string(),"retryable":error.is_retryable()}]
    })
}
