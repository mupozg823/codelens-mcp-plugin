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
    // Cheap argument validation first: a malformed span never binds (or
    // builds) a project.
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
    // Held for the whole call: every read below (index, generation fence,
    // project, watcher, failure summary) must see the same bound project.
    let (_binding, file) = bind_target_file(
        state,
        selector_args["file"]
            .as_str()
            .expect("schema-validated file"),
    )?;
    // The end-of-request generation fence is captured here, under the
    // binding, so it watches the bound repository's index (dispatch skips its
    // own pre-binding capture for these handlers). A reader's generation is a
    // constant, so its fence never trips.
    crate::dispatch::run_fenced_read(
        || {
            Some(crate::dispatch::SymbolGenerationFence::new(
                state.symbol_index(),
                state.current_project_scope(),
            ))
        },
        || run_bound(state, args, target, query, &file, name_span),
    )
}

fn run_bound(
    state: &AppState,
    args: &Value,
    target: &str,
    query: AnalysisQuery,
    file: &str,
    name_span: Option<ByteSpan>,
) -> ToolResult {
    let selector_args = &args["target"];
    let index = state.symbol_index();
    // A reader cannot see the writer process's mutation counter: its committed
    // generation is a constant, so it is reported unknown and pins are refused
    // rather than every pin passing against that constant.
    let reader = state.project_runtime_is_reader();
    let generation = index.committed_generation();
    if let Some(expected) = args.get("generation").and_then(Value::as_u64) {
        if reader {
            return Err(CodeLensError::AnalysisLiteReaderGenerationPin {
                runtime_role: Box::new(state.project_runtime_role_evidence()),
            });
        }
        if expected != generation {
            return Err(CodeLensError::IndexGenerationChanged {
                project: state.current_project_scope(),
                before: expected,
                after: generation,
            });
        }
    }
    let selector = DeclarationSelector {
        file: file.to_owned(),
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
    #[cfg(test)]
    AFTER_QUERY.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(state);
        }
    });
    // Read while the request binding is held; the error envelope is built
    // after it is released.
    let runtime_role = state.project_runtime_role_evidence();
    if evidence.status == EvidenceStatus::Error {
        return Err(CodeLensError::AnalysisEvidence {
            evidence: Box::new(evidence),
            runtime_role: Box::new(runtime_role),
        });
    }
    let partial = evidence.status != EvidenceStatus::Ok;
    let mut payload =
        serde_json::to_value(evidence).map_err(|e| CodeLensError::Internal(e.into()))?;
    payload["freshness"]["watcher_running"] =
        json!(state.watcher_stats().map(|stats| stats.running));
    payload["freshness"]["watcher_start_error"] = json!(state.watcher_error());
    apply_runtime_role(&mut payload, &runtime_role);
    // This read preserves DB errors as errors, rather than the legacy health
    // helper's unwrap_or_default() zero-failure interpretation.
    payload["freshness"]["index_failure_summary"] = match index.db().index_failure_summary(900) {
        Ok(summary) => {
            json!({"status":"observed", "recent_failures":summary.recent_failures, "total_failures":summary.total_failures, "persistent_failures":summary.persistent_failures})
        }
        Err(error) => json!({"status":"error", "error":error.to_string()}),
    };
    let observed_bytes = wire_bytes(&payload)?;
    let limit_bytes = crate::tool_defs::analysis_lite::max_response_bytes();
    if observed_bytes > limit_bytes && !clip_to_wire_bound(&mut payload, query, limit_bytes)? {
        return Err(CodeLensError::AnalysisLiteResponseTooLarge {
            observed_bytes,
            limit_bytes,
            runtime_role: Box::new(runtime_role),
        });
    }
    let mut meta = success_meta(BackendKind::Hybrid, 0.0);
    meta.partial = partial;
    meta.freshness = Freshness::Indexed;
    meta.degraded_reason = Some("analysis-lite coverage and freshness are defined by the typed evidence; no completeness confidence is assigned".to_owned());
    Ok((payload, meta))
}

fn wire_bytes(payload: &Value) -> Result<usize, CodeLensError> {
    serde_json::to_vec(payload)
        .map(|bytes| bytes.len())
        .map_err(|error| CodeLensError::Internal(error.into()))
}

/// Fit evidence over the wire bound by dropping whole trailing rows, never by
/// rewriting one, and record exactly what was dropped under
/// `truncation.wire_bound`. Unresolved examples go first, then the query's own
/// rows: references, or impact edges together with the nodes only they
/// reached. Returns false when not even one row fits; the caller then keeps the
/// explicit rejection, so a clipped answer is never a sampled zero.
fn clip_to_wire_bound(
    payload: &mut Value,
    query: AnalysisQuery,
    limit: usize,
) -> Result<bool, CodeLensError> {
    let unclipped_bytes = wire_bytes(payload)?;
    let take = |value: &mut Value, pointer: &str| {
        value
            .pointer_mut(pointer)
            .map(Value::take)
            .and_then(|rows| match rows {
                Value::Array(rows) => Some(rows),
                _ => None,
            })
            .unwrap_or_default()
    };
    let examples = take(payload, "/unresolved/examples");
    let (rows_pointer, rows) = match query {
        AnalysisQuery::References => ("/results/references", take(payload, "/results/references")),
        AnalysisQuery::Impact => ("/results/edges", take(payload, "/results/edges")),
    };
    let nodes = take(payload, "/results/nodes");
    let root = payload["target"]["symbol_id"].clone();
    // `place` writes the first `examples_kept` examples and `rows_kept` rows
    // (plus, for impact, the nodes those rows reach) and the drop record.
    let place = |payload: &mut Value, examples_kept: usize, rows_kept: usize| {
        let kept_rows = rows[..rows_kept].to_vec();
        let mut nodes_omitted = 0;
        if query == AnalysisQuery::Impact {
            let reached = kept_rows
                .iter()
                .flat_map(|edge| [&edge["source"]["symbol_id"], &edge["target"]["symbol_id"]])
                .chain([&root])
                .collect::<Vec<_>>();
            let kept_nodes = nodes
                .iter()
                .filter(|node| reached.contains(&&node["symbol_id"]))
                .cloned()
                .collect::<Vec<_>>();
            nodes_omitted = nodes.len() - kept_nodes.len();
            payload["results"]["nodes"] = Value::Array(kept_nodes);
        } else {
            payload["results"]["nodes"] = Value::Array(nodes.clone());
        }
        payload["unresolved"]["examples"] = Value::Array(examples[..examples_kept].to_vec());
        if let Some(slot) = payload.pointer_mut(rows_pointer) {
            *slot = Value::Array(kept_rows);
        }
        let rows_omitted = rows.len() - rows_kept;
        let examples_omitted = examples.len() - examples_kept;
        if examples_omitted > 0 {
            payload["unresolved"]["examples_truncated"] = json!(true);
        }
        match query {
            AnalysisQuery::References => {
                if rows_omitted > 0 {
                    payload["truncation"]["results"] = json!(true);
                }
            }
            AnalysisQuery::Impact => {
                if rows_omitted > 0 {
                    payload["truncation"]["edges"] = json!(true);
                }
                if nodes_omitted > 0 {
                    payload["truncation"]["nodes"] = json!(true);
                }
            }
        }
        let row_key = rows_pointer.rsplit('/').next().unwrap_or("rows");
        payload["truncation"]["wire_bound"] = json!({
            "limit_bytes": limit,
            "unclipped_bytes": unclipped_bytes,
            format!("{row_key}_omitted"): rows_omitted,
            "nodes_omitted": nodes_omitted,
            "unresolved_examples_omitted": examples_omitted,
            "next": "narrow path or lower max_results / graph caps for the omitted rows",
        });
    };
    // Largest prefix that fits; the predicate is monotone in the kept count.
    let largest_fitting = |payload: &mut Value,
                           lo: usize,
                           hi: usize,
                           fits: &dyn Fn(&mut Value, usize) -> Result<bool, CodeLensError>|
     -> Result<Option<usize>, CodeLensError> {
        if !fits(payload, lo)? {
            return Ok(None);
        }
        let (mut lo, mut hi) = (lo, hi);
        while lo < hi {
            let mid = lo + (hi - lo).div_ceil(2);
            if fits(payload, mid)? {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        Ok(Some(lo))
    };
    let fits_examples = |payload: &mut Value, kept: usize| {
        place(payload, kept, rows.len());
        Ok(wire_bytes(payload)? <= limit)
    };
    let examples_kept = largest_fitting(payload, 0, examples.len(), &fits_examples)?;
    if examples_kept.is_none() && rows.is_empty() {
        return Ok(false);
    }
    let (examples_kept, rows_kept) = match examples_kept {
        Some(kept) => (kept, rows.len()),
        None => {
            let fits_rows = |payload: &mut Value, kept: usize| {
                place(payload, 0, kept);
                Ok(wire_bytes(payload)? <= limit)
            };
            match largest_fitting(payload, 1, rows.len(), &fits_rows)? {
                Some(kept) => (0, kept),
                None => return Ok(false),
            }
        }
    };
    place(payload, examples_kept, rows_kept);
    Ok(true)
}

#[cfg(test)]
pub(crate) type AfterQueryHook = Box<dyn Fn(&AppState)>;

#[cfg(test)]
thread_local! {
    /// Runs on the calling thread right after the engine query, under the
    /// request binding: lets a test commit to the bound index mid-request.
    pub(crate) static AFTER_QUERY: std::cell::RefCell<Option<AfterQueryHook>> =
        const { std::cell::RefCell::new(None) };
}

/// Stamp serialized evidence (success or `status=error`) with the runtime that
/// answered: `freshness.runtime_role` and, for a reader of another process's
/// writer lease, `freshness.writer_holder` — explicit, never a silent
/// downgrade. A reader cannot see the writer's mutation counter, so its
/// `snapshot.generation` is reported unknown.
pub(crate) fn apply_runtime_role(payload: &mut Value, runtime_role: &Value) {
    if let Value::Object(role) = runtime_role {
        for (key, value) in role {
            payload["freshness"][key] = value.clone();
        }
    }
    if runtime_role["runtime_role"] == "reader" && payload["snapshot"].is_object() {
        payload["snapshot"]["generation"] = Value::Null;
    }
}

/// Resolve `target.file` under this server's binding, returning the
/// root-relative path the engine selects on.
///
/// - Analysis-lite stdio (per-call binding; the default context is a
///   placeholder) has one rule. An absolute file binds this request only,
///   through the per-thread [`RequestProjectGuard`], to the repository root
///   derived from the file's directory — whether or not it lies under the
///   launch root. The root needs a real marker, is never `$HOME` and never sits
///   in a cloud-synced folder. A relative file resolves under the launch root;
///   without one it has no project to resolve in.
/// - Launch-bound stdio (`--cmd`/`--batch`): an absolute file must lie inside
///   the bound root; there is no cross-project switch.
/// - HTTP keeps its explicit session binding and passes the file through.
///
/// [`RequestProjectGuard`]: crate::state::RequestProjectGuard
fn bind_target_file(
    state: &AppState,
    raw: &str,
) -> Result<(Option<crate::state::RequestProjectGuard>, String), CodeLensError> {
    let path = Path::new(raw);
    if let Some(per_call) = state.analysis_lite_per_call_binding() {
        let unbound = |reason: String| CodeLensError::AnalysisLiteLaunchUnbound {
            file: raw.to_owned(),
            reason,
        };
        if !path.is_absolute() {
            let Some(root) = per_call.launch_root else {
                return Err(unbound(
                    "a relative path has no launch project to resolve against".to_owned(),
                ));
            };
            let guard = bind_call_project(state, &root, unbound)?;
            return Ok((Some(guard), raw.to_owned()));
        }
        let file = canonical_file(path);
        let root = derive_call_root(&file).map_err(unbound)?;
        let guard = bind_call_project(state, &root, unbound)?;
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

/// The repository an absolute file's call binds to: a real root marker above
/// the file (`~/Downloads/x.ts` never promotes, and writes `.codelens` into,
/// `~/Downloads`), outside every cloud-synced folder.
fn derive_call_root(file: &Path) -> Result<ProjectRoot, String> {
    let directory = file
        .parent()
        .ok_or_else(|| "the path has no parent directory".to_owned())?;
    let root =
        ProjectRoot::new_requiring_marker(directory).map_err(|error| format!("{error:#}"))?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(synced) = cloud_synced_root(root.as_path(), home.as_deref()) {
        return Err(format!(
            "its root `{}` is inside the cloud-synced folder `{}`; CodeLens does not create index state there",
            root.as_path().display(),
            synced.display()
        ));
    }
    Ok(root)
}

/// Bind this request to `root` (the home guard and the per-call build run
/// here); untyped failures become a binding error for `target.file`.
fn bind_call_project(
    state: &AppState,
    root: &ProjectRoot,
    unbound: impl Fn(String) -> CodeLensError,
) -> Result<crate::state::RequestProjectGuard, CodeLensError> {
    state
        .bind_request_project_scope(&root.as_path().to_string_lossy())
        .map_err(|error| match error.downcast::<CodeLensError>() {
            Ok(typed) => typed,
            Err(error) => unbound(format!("{error:#}")),
        })
}

/// [`cloud_synced_ancestor`] with the platform's File Provider check.
pub(crate) fn cloud_synced_root(root: &Path, home: Option<&Path>) -> Option<PathBuf> {
    cloud_synced_ancestor(root, home, has_file_provider_domain)
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
    // A path with an interior NUL cannot be checked: fail closed.
    let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return true;
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
    let errno = (size < 0)
        .then(|| std::io::Error::last_os_error().raw_os_error())
        .flatten();
    xattr_lookup_is_synced(size, errno)
}

/// Decision on one `getxattr` result. Fails closed: only a definite answer
/// clears a dir — `ENOATTR` (the attribute is absent) or `ENOTSUP` (the
/// filesystem has no extended attributes, e.g. exFAT, so it cannot host a
/// File Provider domain). Any other error (EACCES, EPERM, EIO, ...) means the
/// tag could not be checked, so the dir is treated as synced.
#[cfg(target_os = "macos")]
fn xattr_lookup_is_synced(size: isize, errno: Option<i32>) -> bool {
    size >= 0 || !matches!(errno, Some(libc::ENOATTR | libc::ENOTSUP))
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
    let project = if let Some(per_call) = state.analysis_lite_per_call_binding() {
        // The default context is a placeholder, not a project. A binding
        // refusal has no project; any later rejection names the repository
        // the call bound to.
        if matches!(error, CodeLensError::AnalysisLiteLaunchUnbound { .. }) {
            None
        } else {
            match args["target"]["file"].as_str().map(Path::new) {
                Some(file) if file.is_absolute() => derive_call_root(&canonical_file(file)).ok(),
                _ => per_call.launch_root,
            }
            .map(|root| root.as_path().display().to_string())
        }
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
    let mut payload = json!({
        "schema_version":"analysis-lite/v1", "capability":null, "project":project,
        "target":null,"snapshot":null,"scope":null,"status":"error",
        "results":{"references":[],"nodes":[],"edges":[],"selection_candidates":[],"total":null},
        "limits":null,"truncation":null,"unresolved":null,"freshness":null,
        "errors":[{"code":error.kind(),"backend":"runtime","message":error.to_string(),"retryable":error.is_retryable()}]
    });
    // A rejection raised after binding (a reader's generation pin, the size
    // limit, a writer's generation change) names the runtime that refused it;
    // the binding is gone by now, so the role travels in the error.
    if let Some(runtime_role) = error.analysis_lite_runtime_role() {
        apply_runtime_role(&mut payload, runtime_role);
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::{AnalysisQuery, clip_to_wire_bound, cloud_synced_ancestor, wire_bytes};
    use serde_json::{Value, json};
    use std::path::Path;

    fn impact_payload(edges: usize) -> Value {
        let node = |id: usize| json!({"symbol_id": format!("n{id}"), "pad": "x".repeat(200)});
        json!({
            "target": {"symbol_id": "n0"},
            "results": {
                "references": [],
                "nodes": (0..=edges).map(node).collect::<Vec<_>>(),
                "edges": (1..=edges).map(|i| json!({
                    "source": {"symbol_id": format!("n{i}")},
                    "target": {"symbol_id": "n0"},
                    "pad": "y".repeat(300),
                })).collect::<Vec<_>>(),
            },
            "truncation": {"edges": false, "nodes": false},
            "unresolved": {"examples": [{"pad": "z".repeat(400)}], "examples_truncated": false},
        })
    }

    #[test]
    fn clipping_drops_whole_trailing_edges_and_the_nodes_only_they_reached() {
        let mut payload = impact_payload(20);
        let limit = 6000;
        assert!(wire_bytes(&payload).unwrap() > limit);
        assert!(clip_to_wire_bound(&mut payload, AnalysisQuery::Impact, limit).unwrap());
        assert!(wire_bytes(&payload).unwrap() <= limit, "{payload}");
        let edges = payload["results"]["edges"].as_array().unwrap();
        let nodes = payload["results"]["nodes"].as_array().unwrap();
        assert!(!edges.is_empty());
        // Kept edges are the leading prefix, and every node is the root or an
        // endpoint of a kept edge.
        assert_eq!(edges[0]["source"]["symbol_id"], "n1");
        assert_eq!(nodes.len(), edges.len() + 1);
        let wire = &payload["truncation"]["wire_bound"];
        assert_eq!(wire["edges_omitted"], json!(20 - edges.len()));
        assert_eq!(wire["nodes_omitted"], json!(20 - edges.len()));
        assert_eq!(wire["unresolved_examples_omitted"], 1);
        assert_eq!(payload["truncation"]["edges"], true);
        assert_eq!(payload["unresolved"]["examples_truncated"], true);
    }

    #[test]
    fn clipping_refuses_when_not_one_row_fits_so_no_sampled_zero_is_returned() {
        let mut payload = impact_payload(3);
        assert!(!clip_to_wire_bound(&mut payload, AnalysisQuery::Impact, 200).unwrap());
        let mut empty = impact_payload(0);
        assert!(!clip_to_wire_bound(&mut empty, AnalysisQuery::Impact, 200).unwrap());
    }

    // A dir whose provider tag cannot be read (EACCES, EPERM, EIO, ...) may
    // still be synced. Only a definite "no such attribute" (ENOATTR) or "this
    // filesystem has no xattrs" (ENOTSUP: exFAT and friends cannot host a
    // File Provider domain) clears it.
    #[cfg(target_os = "macos")]
    #[test]
    fn file_provider_lookup_fails_closed_except_a_missing_attribute_or_xattr_support() {
        use super::xattr_lookup_is_synced;
        assert!(xattr_lookup_is_synced(4, None));
        assert!(xattr_lookup_is_synced(0, None));
        for errno in [libc::ENOATTR, libc::ENOTSUP] {
            assert!(!xattr_lookup_is_synced(-1, Some(errno)), "errno {errno}");
        }
        for errno in [
            libc::EACCES,
            libc::ENOENT,
            libc::EOPNOTSUPP,
            libc::EIO,
            libc::ELOOP,
            libc::EPERM,
        ] {
            assert!(xattr_lookup_is_synced(-1, Some(errno)), "errno {errno}");
        }
        assert!(xattr_lookup_is_synced(-1, None), "unknown errno");
        // An ordinary local dir still reads as not synced.
        assert!(!super::has_file_provider_domain(
            &std::env::temp_dir().canonicalize().unwrap()
        ));
    }

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
