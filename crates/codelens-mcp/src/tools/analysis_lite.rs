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
    if let Some((project, file)) = source_only_target(state, args, selector_args)? {
        return run_query(state, args, target, query, &file, name_span, Some(project));
    }
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
        || run_query(state, args, target, query, &file, name_span, None),
    )
}

/// Run the query against the request's bound runtime, or, for `source_only`,
/// against that project's sources with an empty in-memory index: nothing is
/// built, leased or written, and like a reader it reports no generation.
fn run_query(
    state: &AppState,
    args: &Value,
    target: &str,
    query: AnalysisQuery,
    file: &str,
    name_span: Option<ByteSpan>,
    source_only: Option<ProjectRoot>,
) -> ToolResult {
    let selector_args = &args["target"];
    let source_only = source_only.map(|project| {
        let index = Arc::new(codelens_engine::SymbolIndex::new_memory(project.clone()));
        (project, index)
    });
    // A reader cannot see the writer process's mutation counter: its committed
    // generation is a constant, so it is reported unknown and pins are refused
    // rather than every pin passing against that constant.
    let (project, index, reader) = match &source_only {
        Some((project, index)) => (project.clone(), Arc::clone(index), true),
        None => (
            state.project(),
            state.symbol_index(),
            state.project_runtime_is_reader(),
        ),
    };
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
        &project,
        &index,
        query,
        &selector,
        args.get("path").and_then(Value::as_str),
        limits,
    );
    // The query's source graph was just dropped on this thread.
    crate::memory_pressure::return_freed_memory();
    #[cfg(test)]
    AFTER_QUERY.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(state);
        }
    });
    // Read while the request binding is held; the error envelope is built
    // after it is released.
    let runtime_role = if source_only.is_some() {
        json!({"runtime_role": "source_only"})
    } else {
        state.project_runtime_role_evidence()
    };
    if evidence.status == EvidenceStatus::Error {
        return Err(CodeLensError::AnalysisEvidence {
            evidence: Box::new(evidence),
            runtime_role: Box::new(runtime_role),
        });
    }
    let partial = evidence.status != EvidenceStatus::Ok;
    let mut payload =
        serde_json::to_value(evidence).map_err(|e| CodeLensError::Internal(e.into()))?;
    if source_only.is_some() {
        payload["freshness"]["watcher_running"] = Value::Null;
        payload["freshness"]["watcher_start_error"] = Value::Null;
    } else {
        payload["freshness"]["watcher_running"] =
            json!(state.watcher_stats().map(|stats| stats.running));
        payload["freshness"]["watcher_start_error"] = json!(state.watcher_error());
    }
    apply_runtime_role(&mut payload, &runtime_role);
    // This read preserves DB errors as errors, rather than the legacy health
    // helper's unwrap_or_default() zero-failure interpretation.
    payload["freshness"]["index_failure_summary"] = if source_only.is_some() {
        json!({"status": "no_index"})
    } else {
        match index.db().index_failure_summary(900) {
            Ok(summary) => {
                json!({"status":"observed", "recent_failures":summary.recent_failures, "total_failures":summary.total_failures, "persistent_failures":summary.persistent_failures})
            }
            Err(error) => json!({"status":"error", "error":error.to_string()}),
        }
    };
    compact_rows(&mut payload, query);
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

/// Drop what every row repeats, losslessly: a refs row's `target` when it is
/// the response `target`; an impact edge's `source`/`target` identity, written
/// as its `symbol_id` string when it equals that entry of `results.nodes`; and
/// each provenance `source_hash` that equals the one hash of its row's file,
/// hoisted once into `snapshot.source_hashes`. A file seen with two hashes
/// keeps them inline.
fn compact_rows(payload: &mut Value, query: AnalysisQuery) {
    let target = payload["target"].clone();
    if query == AnalysisQuery::Impact {
        let nodes = payload["results"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|node| Some((node["symbol_id"].as_str()?.to_owned(), node.clone())))
            .collect::<std::collections::HashMap<_, _>>();
        for edge in payload["results"]["edges"]
            .as_array_mut()
            .into_iter()
            .flatten()
        {
            for end in ["source", "target"] {
                let id = edge[end]["symbol_id"].as_str().map(ToOwned::to_owned);
                if let Some(id) = id.filter(|id| nodes.get(id) == Some(&edge[end])) {
                    edge[end] = Value::String(id);
                }
            }
        }
    }
    let mut hashes: std::collections::BTreeMap<String, Option<String>> = Default::default();
    for pointer in ["/results/references", "/results/edges"] {
        for row in payload
            .pointer(pointer)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(file) = row["location"]["file"].as_str() else {
                continue;
            };
            for hash in row["provenance"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry["source_hash"].as_str())
            {
                let slot = hashes
                    .entry(file.to_owned())
                    .or_insert_with(|| Some(hash.to_owned()));
                if slot.as_deref() != Some(hash) {
                    *slot = None;
                }
            }
        }
    }
    for pointer in ["/results/references", "/results/edges"] {
        let Some(rows) = payload.pointer_mut(pointer).and_then(Value::as_array_mut) else {
            continue;
        };
        for row in rows {
            if query == AnalysisQuery::References && !target.is_null() && row["target"] == target {
                row.as_object_mut().map(|row| row.remove("target"));
            }
            let file = row["location"]["file"].as_str().map(ToOwned::to_owned);
            let single = file.and_then(|file| hashes.get(&file).cloned().flatten());
            let Some(single) = single else {
                continue;
            };
            for entry in row["provenance"].as_array_mut().into_iter().flatten() {
                if entry["source_hash"].as_str() == Some(single.as_str()) {
                    entry
                        .as_object_mut()
                        .map(|entry| entry.remove("source_hash"));
                }
            }
        }
    }
    let hoisted = hashes
        .into_iter()
        .filter_map(|(file, hash)| hash.map(|hash| (file, Value::String(hash))))
        .collect::<serde_json::Map<_, _>>();
    if !hoisted.is_empty() && payload["snapshot"].is_object() {
        payload["snapshot"]["source_hashes"] = Value::Object(hoisted);
    }
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
            // An endpoint is a node `symbol_id` string or a full identity.
            let id = |end: &'_ Value| -> Value {
                if end.is_string() {
                    end.clone()
                } else {
                    end["symbol_id"].clone()
                }
            };
            let reached = kept_rows
                .iter()
                .flat_map(|edge| [id(&edge["source"]), id(&edge["target"])])
                .chain([root.clone()])
                .collect::<Vec<_>>();
            let kept_nodes = nodes
                .iter()
                .filter(|node| reached.contains(&node["symbol_id"]))
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
    let role = &runtime_role["runtime_role"];
    if (role == "reader" || role == "source_only") && payload["snapshot"].is_object() {
        payload["snapshot"]["generation"] = Value::Null;
    }
}

/// The project and root-relative file for a call that can skip the project
/// runtime: analysis-lite stdio with source-only reads on, no `generation`
/// pin and no indexed `symbol_id`, and no runtime already built for that
/// repository. Root derivation and its refusals are the binding's own.
fn source_only_target(
    state: &AppState,
    args: &Value,
    selector_args: &Value,
) -> Result<Option<(ProjectRoot, String)>, CodeLensError> {
    if !state.analysis_lite_source_only()
        || args.get("generation").is_some()
        || selector_args.get("symbol_id").is_some()
    {
        return Ok(None);
    }
    let Some(per_call) = state.analysis_lite_per_call_binding() else {
        return Ok(None);
    };
    let raw = selector_args["file"]
        .as_str()
        .expect("schema-validated file");
    let unbound = |reason: String| CodeLensError::AnalysisLiteLaunchUnbound {
        file: raw.to_owned(),
        reason,
    };
    let path = Path::new(raw);
    let (root, absolute) = if path.is_absolute() {
        let file = canonical_file(path);
        let root = derive_call_root(&file, per_call.launch_root.as_ref()).map_err(unbound)?;
        (root, Some(file))
    } else {
        let Some(root) = per_call.launch_root else {
            return Err(unbound(
                "a relative path has no launch project to resolve against".to_owned(),
            ));
        };
        (root, None)
    };
    let (project, built) = state
        .analysis_lite_source_scope(&root.as_path().to_string_lossy())
        .map_err(|error| match error.downcast::<CodeLensError>() {
            Ok(typed) => typed,
            Err(error) => unbound(format!("{error:#}")),
        })?;
    if built {
        return Ok(None);
    }
    let file = match absolute {
        Some(file) => root_relative(&project, &file)
            .ok_or_else(|| unbound("the file is not inside its derived root".to_owned()))?,
        None => raw.to_owned(),
    };
    Ok(Some((project, file)))
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
        let root = derive_call_root(&file, per_call.launch_root.as_ref()).map_err(unbound)?;
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
///
/// The nearest marker is not always the repository. A `package.json` of a
/// Node tool inside a larger checkout (`scripts/browserctl/package.json`)
/// stopped the walk there, so the call scanned that one folder and every
/// caller elsewhere in the repository was out of scope — and a relative and
/// an absolute spelling of one file bound different projects. The launch root
/// wins when it holds the file; otherwise the enclosing git checkout does, as
/// long as its discovery would walk the file (a gitignored scratch folder keeps
/// its own marker, or the wider scan would skip the very files it holds).
fn derive_call_root(file: &Path, launch_root: Option<&ProjectRoot>) -> Result<ProjectRoot, String> {
    if let Some(launch) = launch_root.filter(|launch| file.starts_with(launch.as_path())) {
        return Ok(launch.clone());
    }
    let directory = file
        .parent()
        .ok_or_else(|| "the path has no parent directory".to_owned())?;
    let nearest =
        ProjectRoot::new_requiring_marker(directory).map_err(|error| format!("{error:#}"))?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let root = enclosing_checkout(nearest.as_path(), file, home.as_deref())
        .and_then(|checkout| ProjectRoot::new_requiring_marker(checkout).ok())
        .unwrap_or(nearest);
    if let Some(synced) = cloud_synced_root(root.as_path(), home.as_deref()) {
        return Err(format!(
            "its root `{}` is inside the cloud-synced folder `{}`; CodeLens does not create index state there",
            root.as_path().display(),
            synced.display()
        ));
    }
    Ok(root)
}

/// The git checkout (`.git` directory or file: repository, worktree or
/// submodule) above a nested marker root `nearest`, when that checkout's
/// discovery walks `file`. `$HOME` is never a checkout here.
fn enclosing_checkout(nearest: &Path, file: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let home = home.map(|home| home.canonicalize().unwrap_or_else(|_| home.to_path_buf()));
    let checkout = nearest
        .ancestors()
        .take_while(|dir| Some(*dir) != home.as_deref())
        .find(|dir| dir.join(".git").exists())?;
    if checkout == nearest {
        return None;
    }
    let walked = !codelens_engine::project::is_excluded_within(checkout, file)
        && !codelens_engine::project::GitignoreFilter::new(checkout).is_ignored(file);
    walked.then(|| checkout.to_path_buf())
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
                Some(file) if file.is_absolute() => {
                    derive_call_root(&canonical_file(file), per_call.launch_root.as_ref()).ok()
                }
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
    use super::{
        AnalysisQuery, clip_to_wire_bound, cloud_synced_ancestor, compact_rows, wire_bytes,
    };
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
    fn compaction_hoists_one_hash_per_file_and_keeps_conflicts_and_edge_targets_inline() {
        let target = json!({"symbol_id": "a.ts#t", "name": "t"});
        let other = json!({"symbol_id": "b.ts#u", "name": "u"});
        let row = |file: &str, hash: &str, target: &Value| {
            json!({
                "location": {"file": file, "line": 1},
                "target": target,
                "provenance": [{"backend": "oxc_semantic", "source_hash": hash}],
            })
        };
        let mut refs = json!({
            "target": target,
            "snapshot": {"generation": 1},
            "results": {"references": [
                row("a.ts", "h1", &target),
                row("a.ts", "h1", &target),
                row("b.ts", "h2", &target),
                row("b.ts", "h3", &target),
            ], "edges": []},
        });
        compact_rows(&mut refs, AnalysisQuery::References);
        let rows = refs["results"]["references"].as_array().unwrap();
        assert!(rows.iter().all(|row| row.get("target").is_none()), "{refs}");
        assert_eq!(refs["snapshot"]["source_hashes"], json!({"a.ts": "h1"}));
        assert!(rows[0]["provenance"][0].get("source_hash").is_none());
        // b.ts was seen with two hashes: both stay on their rows.
        assert_eq!(rows[2]["provenance"][0]["source_hash"], "h2");
        assert_eq!(rows[3]["provenance"][0]["source_hash"], "h3");

        // Impact endpoints equal to a node become that node's id; one that
        // differs from its node (or has none) stays a full identity.
        let stale = json!({"symbol_id": "a.ts#t", "name": "renamed"});
        let mut edge = row("b.ts", "h2", &other);
        edge["source"] = json!({"symbol_id": "c.ts#module", "name": "<module>"});
        let mut edge2 = row("b.ts", "h2", &stale);
        edge2["source"] = other.clone();
        let mut impact = json!({
            "target": target,
            "snapshot": {"generation": 1},
            "results": {"references": [], "nodes": [target, other], "edges": [edge, edge2]},
        });
        compact_rows(&mut impact, AnalysisQuery::Impact);
        let edges = &impact["results"]["edges"];
        assert_eq!(edges[0]["target"], "b.ts#u");
        assert_eq!(edges[0]["source"]["name"], "<module>");
        assert_eq!(edges[1]["source"], "b.ts#u");
        assert_eq!(edges[1]["target"], stale);
        assert_eq!(impact["snapshot"]["source_hashes"], json!({"b.ts": "h2"}));
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
