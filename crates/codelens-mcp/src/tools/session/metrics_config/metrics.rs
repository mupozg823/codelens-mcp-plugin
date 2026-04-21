use crate::AppState;
use crate::protocol::BackendKind;
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;
use std::collections::VecDeque;

use super::super::audit_common::{is_builder_surface, is_planner_surface};

/// Bucket latency samples into a compact histogram: <10ms, <50ms, <200ms, <1s, 1s+.
fn latency_histogram(samples: &VecDeque<u64>) -> serde_json::Value {
    let (mut lt10, mut lt50, mut lt200, mut lt1000, mut gte1000) = (0u32, 0, 0, 0, 0);
    for &ms in samples {
        match ms {
            0..=9 => lt10 += 1,
            10..=49 => lt50 += 1,
            50..=199 => lt200 += 1,
            200..=999 => lt1000 += 1,
            _ => gte1000 += 1,
        }
    }
    json!({"<10ms": lt10, "10-49ms": lt50, "50-199ms": lt200, "200-999ms": lt1000, "1s+": gte1000})
}

pub fn get_tool_metrics(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let requested_session_id = _arguments
        .get("session_id")
        .and_then(|value| value.as_str());
    let snapshot = requested_session_id
        .map(|session_id| state.metrics().snapshot_for_session(session_id))
        .unwrap_or_else(|| state.metrics().snapshot());
    let surfaces = requested_session_id
        .map(|session_id| state.metrics().surface_snapshot_for_session(session_id))
        .unwrap_or_else(|| state.metrics().surface_snapshot());
    let per_tool: Vec<serde_json::Value> = snapshot
        .into_iter()
        .map(|(name, m)| {
            json!({
                "tool": name,
                "calls": m.call_count,
                "success_count": m.success_count,
                "total_ms": m.total_ms,
                "max_ms": m.max_ms,
                "total_tokens": m.total_tokens,
                "avg_output_tokens": if m.call_count > 0 {
                    m.total_tokens / m.call_count as usize
                } else { 0 },
                "p95_latency_ms": crate::observability::telemetry::percentile_95(&m.latency_samples),
                "latency_histogram": latency_histogram(&m.latency_samples),
                "success_rate": if m.call_count > 0 {
                    m.success_count as f64 / m.call_count as f64
                } else { 0.0 },
                "error_rate": if m.call_count > 0 {
                    m.error_count as f64 / m.call_count as f64
                } else { 0.0 },
                "errors": m.error_count,
                "last_called": m.last_called_at,
            })
        })
        .collect();
    let count = per_tool.len();
    let per_surface = surfaces
        .into_iter()
        .map(|(surface, metrics)| {
            json!({
                "surface": surface,
                "calls": metrics.call_count,
                "success_count": metrics.success_count,
                "total_ms": metrics.total_ms,
                "total_tokens": metrics.total_tokens,
                "errors": metrics.error_count,
                "avg_tokens_per_call": if metrics.call_count > 0 {
                    metrics.total_tokens / metrics.call_count as usize
                } else { 0 },
                "p95_latency_ms": crate::observability::telemetry::percentile_95(&metrics.latency_samples),
                "surface_token_efficiency": if metrics.success_count > 0 {
                    metrics.total_tokens as f64 / metrics.success_count as f64
                } else { 0.0 }
            })
        })
        .collect::<Vec<_>>();
    let coordination_scope: Option<String> = requested_session_id.and_then(|session_id| {
        #[cfg(feature = "http")]
        {
            state.session_store.as_ref().and_then(|store| {
                store
                    .get(session_id)
                    .and_then(|session| session.client_metadata().project_path)
            })
        }
        #[cfg(not(feature = "http"))]
        {
            let _ = session_id;
            None
        }
    });
    let metrics_payload =
        build_session_metrics_payload(state, requested_session_id, coordination_scope.as_deref());
    Ok((
        json!({
            "tools": per_tool.clone(),
            "per_tool": per_tool,
            "count": count,
            "surfaces": per_surface.clone(),
            "per_surface": per_surface,
            "scope": if requested_session_id.is_some() { "session" } else { "global" },
            "session_id": requested_session_id,
            "session": metrics_payload.session,
            "derived_kpis": metrics_payload.derived_kpis
        }),
        success_meta(BackendKind::Telemetry, 1.0),
    ))
}

/// Export session telemetry as markdown — replaces collect-session.sh + Python.
pub fn export_session_markdown(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let session_name = arguments
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("session");
    let requested_session_id = arguments.get("session_id").and_then(|value| value.as_str());
    let snapshot = requested_session_id
        .map(|session_id| state.metrics().snapshot_for_session(session_id))
        .unwrap_or_else(|| state.metrics().snapshot());
    let session = requested_session_id
        .map(|session_id| state.metrics().session_snapshot_for(session_id))
        .unwrap_or_else(|| state.metrics().session_snapshot());

    let total_calls = session.total_calls.max(1);
    let mut tools: Vec<_> = snapshot.into_iter().collect();
    tools.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
    let count = tools.len();

    let mut md = String::with_capacity(2048);
    md.push_str(&format!("# Session Telemetry: {session_name}\n\n"));
    if let Some(session_id) = requested_session_id {
        md.push_str(&format!("Filtered session: `{session_id}`\n\n"));
    }
    md.push_str("## Summary\n\n| Metric | Value |\n|---|---|\n");
    md.push_str(&format!("| Total calls | {} |\n", total_calls));
    md.push_str(&format!("| Total time | {}ms |\n", session.total_ms));
    md.push_str(&format!(
        "| Avg per call | {}ms |\n",
        if total_calls > 0 {
            session.total_ms / total_calls
        } else {
            0
        }
    ));
    md.push_str(&format!("| Total tokens | {} |\n", session.total_tokens));
    md.push_str(&format!("| Errors | {} |\n", session.error_count));
    md.push_str(&format!(
        "| Analysis summary reads | {} |\n",
        session.analysis_summary_reads
    ));
    md.push_str(&format!(
        "| Analysis section reads | {} |\n",
        session.analysis_section_reads
    ));
    md.push_str(&format!("| Unique tools | {count} |\n\n"));

    md.push_str("## Tool Usage\n\n| Tool | Calls | Total(ms) | Avg(ms) | Max(ms) | Err |\n|---|---|---|---|---|---|\n");
    for (name, m) in &tools {
        let avg = if m.call_count > 0 {
            m.total_ms as f64 / m.call_count as f64
        } else {
            0.0
        };
        md.push_str(&format!(
            "| {} | {} | {} | {:.1} | {} | {} |\n",
            name, m.call_count, m.total_ms, avg, m.max_ms, m.error_count
        ));
    }

    md.push_str("\n## Distribution\n\n```\n");
    for (name, m) in tools.iter().take(5) {
        let pct = m.call_count as f64 / total_calls as f64 * 100.0;
        let bar = "#".repeat((pct / 2.0) as usize);
        md.push_str(&format!(
            "  {:<30} {:3} ({:5.1}%) {}\n",
            name, m.call_count, pct, bar
        ));
    }
    md.push_str("```\n\n");
    md.push_str(&format!(
        "Tokens/call: {}\n",
        if total_calls > 0 {
            session.total_tokens / total_calls as usize
        } else {
            0
        }
    ));

    if let Some(session_id) = requested_session_id {
        #[cfg(not(feature = "http"))]
        let _ = session_id;
        #[cfg(feature = "http")]
        let current_surface = state
            .session_store
            .as_ref()
            .and_then(|store| store.get(session_id))
            .map(|session| session.surface().as_label().to_owned())
            .or_else(|| session.timeline.last().map(|entry| entry.surface.clone()))
            .unwrap_or_else(|| state.surface().as_label().to_owned());
        #[cfg(not(feature = "http"))]
        let current_surface = session
            .timeline
            .last()
            .map(|entry| entry.surface.clone())
            .unwrap_or_else(|| state.surface().as_label().to_owned());

        let (audit_title, audit) = if is_builder_surface(&current_surface) {
            (
                "Builder Audit",
                super::super::builder_audit::build_builder_session_audit(state, arguments)?,
            )
        } else if is_planner_surface(&current_surface) {
            (
                "Planner Audit",
                super::super::planner_audit::build_planner_session_audit(state, arguments)?,
            )
        } else {
            let builder =
                super::super::builder_audit::build_builder_session_audit(state, arguments)?;
            if builder
                .get("status")
                .and_then(|value| value.as_str())
                .is_some_and(|status| status != "not_applicable")
            {
                ("Builder Audit", builder)
            } else {
                (
                    "Planner Audit",
                    super::super::planner_audit::build_planner_session_audit(state, arguments)?,
                )
            }
        };

        md.push_str(&format!("\n## {audit_title}\n\n"));
        md.push_str(&format!(
            "- Status: `{}`\n- Score: `{:.2}`\n",
            audit
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown"),
            audit
                .get("score")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0)
        ));
        if let Some(findings) = audit.get("findings").and_then(|value| value.as_array()) {
            if findings.is_empty() {
                md.push_str("- Findings: none\n");
            } else {
                md.push_str("- Findings:\n");
                for finding in findings {
                    let severity = finding
                        .get("severity")
                        .and_then(|value| value.as_str())
                        .unwrap_or("warn");
                    let summary = finding
                        .get("summary")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    md.push_str(&format!("  - [{}] {}\n", severity, summary));
                }
            }
        }
        if let Some(next_tools) = audit
            .get("recommended_next_tools")
            .and_then(|value| value.as_array())
            .filter(|items| !items.is_empty())
        {
            let tools = next_tools
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            md.push_str(&format!("- Recommended next tools: {}\n", tools));
        }
    }

    Ok((
        json!({
            "markdown": md,
            "session_name": session_name,
            "session_id": requested_session_id,
            "scope": if requested_session_id.is_some() { "session" } else { "global" },
            "tool_count": count,
            "total_calls": total_calls,
        }),
        success_meta(BackendKind::Telemetry, 1.0),
    ))
}
