//! Strict semantic coverage probe for host doctor/status.

#[path = "coverage_http_probe.rs"]
mod http_probe;
#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
#[path = "coverage_transport.rs"]
mod transport;

use http_probe::probe_http_coverage;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use transport::{Transport, parse_transport};

#[cfg(any(feature = "http", test))]
const RECOVERY_HINT: &str =
    "Run index_embeddings for this project, then rerun doctor/status --strict.";
#[cfg(any(feature = "http", test))]
const MODEL_ASSETS_HINT: &str = "Install CodeLens embedding model assets or set CODELENS_MODEL_DIR, then rerun doctor/status --strict.";
#[cfg(any(feature = "http", test))]
const MODEL_MISMATCH_HINT: &str = "Run index_embeddings for this project with the configured embedding model, then rerun doctor/status --strict.";
#[cfg(any(feature = "http", test))]
const RECREATE_INDEX_HINT: &str =
    "Recreate the derived embedding index, then rerun doctor/status --strict.";
#[cfg(any(feature = "http", test))]
const INSPECT_RUNTIME_HINT: &str =
    "Inspect embedding runtime and index metadata, then rerun doctor/status --strict.";
#[cfg(feature = "http")]
const HTTP_UNREACHABLE_HINT: &str = "Start the CodeLens HTTP daemon or repair the configured URL/headers, then rerun doctor/status --strict.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SemanticCoverage {
    checked: bool,
    ok: bool,
    status: String,
    detail: String,
    remediation: Option<String>,
    report: Option<Value>,
}

impl SemanticCoverage {
    fn skipped(status: &str, detail: impl Into<String>) -> Self {
        Self {
            checked: false,
            ok: false,
            status: status.to_owned(),
            detail: detail.into(),
            remediation: None,
            report: None,
        }
    }

    fn failed(status: &str, detail: impl Into<String>, remediation: impl Into<String>) -> Self {
        Self {
            checked: true,
            ok: false,
            status: status.to_owned(),
            detail: detail.into(),
            remediation: Some(remediation.into()),
            report: None,
        }
    }

    #[cfg(any(feature = "http", test))]
    fn from_report(report: Value) -> Self {
        let status = report
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let compiled = report.get("compiled").and_then(Value::as_bool);
        let model_assets_available = report
            .get("model_assets")
            .and_then(Value::as_object)
            .and_then(|assets| assets.get("available"))
            .and_then(Value::as_bool);
        let index = report.get("index").and_then(Value::as_object);
        let indexed_symbols = index
            .and_then(|fields| fields.get("indexed_symbols"))
            .and_then(Value::as_u64);
        let readiness_percent = index
            .and_then(|fields| fields.get("readiness_percent"))
            .and_then(Value::as_u64);
        let stale_files = index
            .and_then(|fields| fields.get("stale_files"))
            .and_then(Value::as_u64);
        let model_mismatch = index
            .and_then(|fields| fields.get("model_mismatch"))
            .and_then(Value::as_bool);
        let query_cache_entries = report
            .get("query_cache")
            .and_then(Value::as_object)
            .and_then(|cache| cache.get("entries"))
            .and_then(Value::as_u64);
        let remediation_action = report
            .get("remediation")
            .and_then(Value::as_object)
            .and_then(|remediation| remediation.get("action"))
            .and_then(Value::as_str)
            .or_else(|| report.get("recommended_action").and_then(Value::as_str))
            .unwrap_or("unknown");
        let last_index_sha = index
            .and_then(|fields| fields.get("last_index_sha"))
            .and_then(Value::as_str)
            .unwrap_or("null");
        let stale_reason = first_stale_reason(index);
        let detail = format!(
            "status={status}, compiled={}, model_assets.available={}, indexed_symbols={}, readiness_percent={}%, stale_files={}, stale_reason={}, model_mismatch={}, remediation.action={}, query_cache.entries={}, last_index_sha={last_index_sha}",
            display_bool(compiled),
            display_bool(model_assets_available),
            display_u64(indexed_symbols),
            display_u64(readiness_percent),
            display_u64(stale_files),
            stale_reason,
            display_bool(model_mismatch),
            remediation_action,
            display_u64(query_cache_entries),
        );
        let ok = status == "ready";
        let remediation = remediation_hint_for_action(remediation_action);
        Self {
            checked: true,
            ok,
            status,
            detail,
            remediation: (!ok).then(|| remediation.to_owned()),
            report: Some(report),
        }
    }

    pub(super) fn render_text(&self) -> String {
        let verdict = if self.ok {
            "OK"
        } else if self.checked {
            "FAIL"
        } else {
            "SKIP"
        };
        match &self.remediation {
            Some(remediation) => format!("{verdict} ({}) remediation: {remediation}", self.detail),
            None => format!("{verdict} ({})", self.detail),
        }
    }

    pub(super) fn to_json(&self) -> Value {
        json!({
            "checked": self.checked,
            "ok": self.ok,
            "status": self.status,
            "detail": self.detail,
            "remediation": self.remediation,
            "report": self.report,
        })
    }

    pub(super) fn strict_exit_issue(&self) -> Option<String> {
        if self.ok || self.status == "not_configured" {
            return None;
        }
        let mut issue = self.detail.clone();
        if let Some(remediation) = &self.remediation {
            issue.push_str("; remediation: ");
            issue.push_str(remediation);
        }
        Some(issue)
    }
}

#[cfg(any(feature = "http", test))]
fn first_stale_reason(index: Option<&serde_json::Map<String, Value>>) -> String {
    let Some(first_reason) = index
        .and_then(|fields| fields.get("stale_file_reasons"))
        .and_then(Value::as_array)
        .and_then(|reasons| reasons.first())
        .and_then(Value::as_object)
    else {
        return "none".to_owned();
    };
    let file_path = first_reason
        .get("file_path")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let reason = first_reason
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("{file_path}:{reason}")
}

#[cfg(any(feature = "http", test))]
fn remediation_hint_for_action(action: &str) -> &'static str {
    match action {
        "install_model_assets" => MODEL_ASSETS_HINT,
        "reindex_embeddings_for_model" => MODEL_MISMATCH_HINT,
        "recreate_embedding_index" => RECREATE_INDEX_HINT,
        "build_embedding_index" | "refresh_embedding_index" => RECOVERY_HINT,
        "none" => INSPECT_RUNTIME_HINT,
        _ => INSPECT_RUNTIME_HINT,
    }
}

pub(super) fn strict_semantic_coverage_enabled(args: &[String]) -> bool {
    args[2..].iter().any(|arg| arg == "--strict")
}

pub(super) fn coverage_for_inspected_files(files: &[Value], cwd: &Path) -> SemanticCoverage {
    let Some(file) = files.iter().find(|file| {
        let status = file
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let format = file
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or_default();
        status.starts_with("attached_") && matches!(format, "json" | "toml")
    }) else {
        return SemanticCoverage::skipped(
            "not_configured",
            "no attached machine-readable CodeLens config",
        );
    };

    let Some(path) = file.get("path").and_then(Value::as_str).map(PathBuf::from) else {
        return SemanticCoverage::failed(
            "invalid_config",
            "attached config entry is missing its path",
            "Repair host config, then rerun doctor/status --strict.",
        );
    };
    let format = file
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match parse_transport(&path, format) {
        Ok(Transport::Http { url, headers }) => probe_http_coverage(&url, &headers, cwd),
        Ok(Transport::Stdio { command }) => SemanticCoverage::skipped(
            "stdio_attach",
            format!("stdio attach `{command}`; semantic coverage is only probed for HTTP daemons"),
        ),
        Err(detail) => SemanticCoverage::failed(
            "invalid_config",
            detail,
            "Repair host config, then rerun doctor/status --strict.",
        ),
    }
}

#[cfg(any(feature = "http", test))]
fn display_bool(value: Option<bool>) -> String {
    value
        .map(|flag| flag.to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(any(feature = "http", test))]
fn display_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}
