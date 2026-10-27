//! Adapter for the generated analysis-lite contract and typed engine evidence.

use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::{BackendKind, Freshness};
use crate::tool_defs::analysis_lite::limit;
use crate::tool_defs::tool::ToolHandler;
use crate::tool_runtime::{ToolResult, success_meta};
use codelens_engine::ProjectRoot;
use codelens_engine::analysis_lite::{
    AnalysisQuery, DeclarationSelector, EvidenceLimits, EvidenceStatus,
};
use codelens_engine::oxc_analysis::ByteSpan;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
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
    // Held for the whole call: every read below (index, generation fence,
    // project, watcher, failure summary) must see the same bound project.
    let (_binding, file) = bind_target_file(
        state,
        selector_args["file"]
            .as_str()
            .expect("schema-validated file"),
    )?;
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
        file,
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

/// Resolve `target.file` under this server's binding, returning the
/// root-relative path the engine selects on.
///
/// - Launch-unbound (stdio started from `$HOME` or `/`): an absolute file binds
///   this request only, through the per-thread [`RequestProjectGuard`], to the
///   repository root derived from the file's directory. Root derivation and the
///   home guard refuse `$HOME`; a relative file has no project to resolve in.
/// - Launch-bound stdio: an absolute file must lie inside the bound root; there
///   is no cross-project switch.
/// - HTTP keeps its explicit session binding and passes the file through.
///
/// [`RequestProjectGuard`]: crate::state::RequestProjectGuard
fn bind_target_file(
    state: &AppState,
    raw: &str,
) -> Result<(Option<crate::state::RequestProjectGuard>, String), CodeLensError> {
    let path = Path::new(raw);
    if state.analysis_lite_launch_unbound() {
        let unbound = |reason: String| CodeLensError::AnalysisLiteLaunchUnbound {
            file: raw.to_owned(),
            reason,
        };
        if !path.is_absolute() {
            return Err(unbound(
                "a relative path has no launch project to resolve against".to_owned(),
            ));
        }
        let file = canonical_file(path);
        let directory = file
            .parent()
            .ok_or_else(|| unbound("the path has no parent directory".to_owned()))?;
        // A file location is not a project: require a real root marker, so
        // `~/Downloads/x.ts` never promotes (and writes `.codelens` into)
        // `~/Downloads`.
        let root = ProjectRoot::new_requiring_marker(directory)
            .map_err(|error| unbound(format!("{error:#}")))?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(synced) =
            cloud_synced_ancestor(root.as_path(), home.as_deref(), has_file_provider_domain)
        {
            return Err(unbound(format!(
                "its root `{}` is inside the cloud-synced folder `{}`; CodeLens does not create index state there",
                root.as_path().display(),
                synced.display()
            )));
        }
        let guard = state
            .bind_request_project_scope(&root.as_path().to_string_lossy())
            .map_err(|error| match error.downcast::<CodeLensError>() {
                Ok(typed) => typed,
                Err(error) => unbound(format!("{error:#}")),
            })?;
        let relative = root_relative(&state.project(), &file)
            .ok_or_else(|| unbound("the file is not inside its derived root".to_owned()))?;
        return Ok((Some(guard), relative));
    }
    if path.is_absolute()
        && state.transport_mode() == crate::runtime_types::RuntimeTransportMode::Stdio
    {
        let project = state.project();
        let relative = root_relative(&project, &canonical_file(path)).ok_or_else(|| {
            CodeLensError::Validation(format!(
                "analysis-lite target.file `{raw}` is outside the launch-bound project `{}`; this server does not switch projects. Pass a path inside that root, or use a server launched in the other repository.",
                project.as_path().display()
            ))
        })?;
        return Ok((None, relative));
    }
    Ok((None, raw.to_owned()))
}

/// The first directory from `root` up to (not including) `home` that belongs to
/// a cloud-sync provider. Binding a root there would write a multi-MB SQLite
/// index + WAL that the provider then uploads.
fn cloud_synced_ancestor(
    root: &Path,
    home: Option<&Path>,
    is_synced: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let home = home.map(|home| home.canonicalize().unwrap_or_else(|_| home.to_path_buf()));
    root.ancestors()
        .take_while(|dir| Some(*dir) != home.as_deref())
        .find(|dir| is_synced(dir))
        .map(Path::to_path_buf)
}

/// macOS File Provider domains (iCloud Desktop & Documents, `~/Library/
/// CloudStorage/*`, `~/Library/Mobile Documents`) tag their folders with this
/// extended attribute.
#[cfg(target_os = "macos")]
fn has_file_provider_domain(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: both pointers are valid NUL-terminated strings for the call; a
    // null value buffer with size 0 only queries the attribute's length.
    let size = unsafe {
        libc::getxattr(
            path.as_ptr(),
            c"com.apple.file-provider-domain-id".as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            0,
        )
    };
    size >= 0
}

#[cfg(not(target_os = "macos"))]
fn has_file_provider_domain(_dir: &Path) -> bool {
    false
}

/// Canonicalize the file, or its parent when the file itself is missing, so
/// `/var` and `/private/var` style aliases compare equal to a canonical root.
fn canonical_file(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        match (
            path.parent().and_then(|parent| parent.canonicalize().ok()),
            path.file_name(),
        ) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => path.to_path_buf(),
        }
    })
}

fn root_relative(project: &ProjectRoot, file: &Path) -> Option<String> {
    let resolved = project.resolve(file).ok()?;
    let relative = resolved.strip_prefix(project.as_path()).ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_string_lossy().replace('\\', "/"))
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
    let project = if state.analysis_lite_launch_unbound() {
        // The default context is a placeholder, not a project; a rejected
        // call has no bound repository to report.
        None
    } else if session.is_local() {
        Some(state.current_project_scope())
    } else if session.project_binding_is_explicit() {
        session
            .project_path
            .as_deref()
            .and_then(|path| ProjectRoot::new(path).ok())
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

#[cfg(test)]
mod tests {
    use super::cloud_synced_ancestor;
    use std::path::Path;

    #[test]
    fn cloud_synced_walk_stops_below_home_and_reports_the_tagged_dir() {
        let home = Path::new("/home-x");
        let root = Path::new("/home-x/Documents/work/repo");
        let tagged = |dir: &Path| dir == Path::new("/home-x/Documents");
        assert_eq!(
            cloud_synced_ancestor(root, Some(home), tagged).as_deref(),
            Some(Path::new("/home-x/Documents"))
        );
        // `$HOME` itself is never consulted, even if it carried the tag.
        let home_tagged = |dir: &Path| dir == Path::new("/home-x");
        assert_eq!(cloud_synced_ancestor(root, Some(home), home_tagged), None);
        assert_eq!(
            cloud_synced_ancestor(Path::new("/home-x/dev/repo"), Some(home), tagged),
            None
        );
    }
}
