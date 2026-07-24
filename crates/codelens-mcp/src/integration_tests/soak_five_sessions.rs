//! Quarter exit criterion #1 (`docs/design/runtime-convergence-execution-plan.md`):
//! "5 sessions · 5 worktrees share one daemon without active-runtime eviction or
//! cross-project leakage; a second same-project process is rejected before WAL
//! contention; newest content wins and mixed-generation responses are discarded."
//!
//! This is the deterministic in-process soak for that criterion: one `AppState`
//! (one daemon), one axum router, five HTTP sessions, five temp projects. Five
//! bound projects deliberately exceed `PROJECT_CONTEXT_CACHE_LIMIT` (4), which is
//! the whole point — ADR-0017 decision 5 says the limit bounds *idle* runtimes,
//! never live session scopes.
//!
//! Four assertion axes, none of them timing-dependent (every synchronization is a
//! task join):
//!
//! 1. **Zero cross-project leakage** — each session reads only its own project's
//!    unique symbol/file across every round.
//! 2. **Zero active-runtime eviction** — every one of the five runtimes still owns
//!    its writer lease, at the same durable lease generation, after the soak. A
//!    retired-and-rebuilt runtime would show a higher generation; a retired-only
//!    runtime would let the contender acquire the lease outright.
//! 3. **Second same-project writer rejected** — attempted *while the five sessions
//!    are mid-round*, and it must be a typed `ProjectWriterBusy` (-32010), never a
//!    silent read-only downgrade.
//! 4. **Mixed-generation responses discarded** — pages served inside one generation
//!    stitch back to the unpaged result; once new content advances the committed
//!    generation, the in-flight cursor and the stale snapshot pin are both refused
//!    with retryable `index_generation_changed` (-32011).
//!
//! Defect-predicate (RED) coverage, so no axis can pass vacuously:
//!
//! * axis 2 — `without_session_bindings_the_fifth_project_evicts_an_idle_runtime`
//!   reproduces the failure directly: identical five-project pressure without
//!   session bindings *does* retire the LRU runtime and release its writer lease.
//! * axis 3 — the same `try_new_minimal` probe returns `Ok` in that RED test
//!   (lease free) and typed `ProjectWriterBusy` here (lease live), so the
//!   rejection is a measured discriminator, not a constant.
//! * axis 4 — the identical paged request succeeds inside one generation and is
//!   refused only after the generation advances.
//! * axis 1 — no in-test bypass exists: every leak token *is* served to its owner
//!   session in the same round (positive control), so its absence elsewhere is a
//!   real negative. Reproducing an actual leak would mean removing the
//!   per-request project binding from production code, which is out of scope.

use crate::AppState;
use crate::error::CodeLensError;
use crate::server::transport_http::build_router;
use crate::tool_defs::ToolPreset;
use axum::http::Request;
use codelens_engine::ProjectRoot;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;

/// Sessions == projects == worktrees in the exit criterion.
const SESSIONS: usize = 5;
/// Concurrent read rounds per session. Constant so the soak stays inside the
/// CI time budget; the leak/eviction contract is per-round, not statistical.
const ROUNDS: usize = 4;

// ── fixtures ─────────────────────────────────────────────────────────

/// One temp project carrying symbols unique to its index. `soak_p{i}.py` and
/// `soak_symbol_p{i}` are the leak tokens: seeing either one in session `j`'s
/// response is cross-project leakage.
fn soak_project(index: usize) -> PathBuf {
    let dir = crate::test_helpers::fixtures::temp_project_dir(&format!("soak-p{index}"));
    std::fs::create_dir_all(dir.join(".codelens")).unwrap();
    std::fs::write(
        dir.join(".codelens/principals.toml"),
        "[default]\nrole = \"Refactor\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join(format!("soak_p{index}.py")),
        format!(
            "def soak_symbol_p{index}():\n    return {index}\n\n\
             def soak_alpha_p{index}():\n    return {index}\n\n\
             def soak_beta_p{index}():\n    return {index}\n"
        ),
    )
    .unwrap();
    dir
}

fn project_root(dir: &Path) -> ProjectRoot {
    ProjectRoot::new(dir.to_str().expect("utf-8 temp path")).expect("temp project root")
}

// ── transport helpers (local: http_tests' fixtures are private to that module) ──

async fn init_session(app: &axum::Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("initialize must mint a session id")
        .to_owned()
}

async fn call_tool(app: &axum::Router, sid: &str, id: u64, name: &str, arguments: Value) -> String {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", sid)
                .body(axum::body::Body::from(request))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn tool_payload(body: &str) -> Value {
    let value: Value = serde_json::from_str(body).expect("json-rpc body must be JSON");
    super::parse_tool_payload(value["result"]["content"][0]["text"].as_str().unwrap_or(""))
}

/// Handler payload underneath the response envelope. Programmatic reads carry
/// `batch` / `next_cursor` / `index_snapshot` in the `data` object; the envelope
/// keeps `success` / `error` / `retryable`.
fn tool_data(body: &str) -> Value {
    let payload = tool_payload(body);
    match payload.get("data") {
        Some(data) if data.is_object() => data.clone(),
        _ => payload,
    }
}

fn assert_transport_ok(body: &str, context: &str) {
    let value: Value = serde_json::from_str(body).expect("json-rpc body must be JSON");
    assert!(
        value.get("error").is_none(),
        "{context} must not fail at the JSON-RPC layer: {body}"
    );
}

/// `index_snapshot` is advertised on every programmatic read as `gen:<n>`.
fn snapshot_generation(payload: &Value, context: &str) -> u64 {
    let token = payload["index_snapshot"]
        .as_str()
        .unwrap_or_else(|| panic!("{context} must advertise index_snapshot: {payload}"));
    token
        .strip_prefix("gen:")
        .unwrap_or(token)
        .parse()
        .unwrap_or_else(|_| panic!("{context} advertised a non-numeric snapshot token: {token}"))
}

/// Both transports of the same typed contract: the tool layer surfaces
/// `index_generation_changed` as a retryable envelope, the JSON-RPC layer as
/// -32011. Either is acceptable; a served payload is not.
fn assert_index_generation_rejected(body: &str, context: &str) {
    let value: Value = serde_json::from_str(body).expect("json-rpc body must be JSON");
    if let Some(code) = value["error"]["code"].as_i64() {
        assert_eq!(code, -32011, "{context} must be typed -32011: {body}");
        return;
    }
    let payload = tool_payload(body);
    assert_eq!(
        payload["success"],
        json!(false),
        "{context} must not serve a payload: {body}"
    );
    assert!(
        payload["error"]
            .as_str()
            .is_some_and(|error| error.contains("index_generation_changed")),
        "{context} must reuse the index_generation_changed contract: {body}"
    );
    assert_eq!(
        payload["retryable"],
        json!(true),
        "{context} must be retryable: {body}"
    );
}

// ── axis 2 + 3 probe ─────────────────────────────────────────────────

/// Attempt a second writable runtime for `project` and return the *holder's*
/// durable lease generation.
///
/// Doubles as the liveness probe for axis 2: an evicted (retired) runtime would
/// have released its lease, so the attempt would succeed instead of being
/// rejected, and a retired-then-rebuilt runtime would report a higher holder
/// generation than the baseline.
fn second_writable_runtime_rejection(project: &Path) -> u64 {
    let root = project_root(project);
    match AppState::try_new_minimal(root, ToolPreset::Balanced) {
        Ok(_) => panic!(
            "a second writable runtime acquired the lease for `{}`: the live runtime was evicted and shut down",
            project.display()
        ),
        Err(error) => {
            let structured = error
                .downcast::<CodeLensError>()
                .expect("a rejected second writer must stay typed, not a bare anyhow error");
            match structured {
                CodeLensError::ProjectWriterBusy { holder, .. } => {
                    let holder = holder.expect("a busy lease must expose holder metadata");
                    let metadata: Value =
                        serde_json::from_str(&holder).expect("holder metadata must be JSON");
                    metadata["generation"]
                        .as_u64()
                        .expect("holder metadata must carry the durable lease generation")
                }
                other => panic!(
                    "the second writable runtime for `{}` must be rejected with project_writer_busy, got: {other}",
                    project.display()
                ),
            }
        }
    }
}

// ── axis 1 assertion ─────────────────────────────────────────────────

fn assert_reads_own_project_only(body: &str, index: usize, round: usize) {
    assert_transport_ok(body, &format!("session {index} round {round}"));
    let payload = tool_payload(body);
    assert_ne!(
        payload["success"],
        json!(false),
        "session {index} round {round} must not fail while five runtimes are live: {body}"
    );
    assert!(
        body.contains(&format!("soak_p{index}.py")),
        "session {index} must read its own project (round {round}): {body}"
    );
    for other in 0..SESSIONS {
        if other == index {
            continue;
        }
        assert!(
            !body.contains(&format!("soak_p{other}.py")),
            "session {index} leaked project {other}'s file (round {round}): {body}"
        );
        assert!(
            !body.contains(&format!("soak_symbol_p{other}")),
            "session {index} leaked project {other}'s symbol (round {round}): {body}"
        );
    }
}

// ── axis 4 ───────────────────────────────────────────────────────────

/// Newest content wins and no response may straddle two committed generations.
async fn assert_mixed_generation_is_discarded(
    app: &axum::Router,
    sid: &str,
    project: &Path,
    index: usize,
) {
    let names = json!([
        format!("soak_symbol_p{index}"),
        format!("soak_alpha_p{index}"),
        format!("soak_beta_p{index}"),
    ]);

    // Settle the index first so the paged control below reads one generation.
    let warmed = call_tool(app, sid, 900, "refresh_symbol_index", json!({})).await;
    assert_transport_ok(&warmed, "index warm-up");

    let unpaged_body = call_tool(
        app,
        sid,
        901,
        "find_symbol",
        json!({ "names": names, "include_body": false }),
    )
    .await;
    let unpaged = tool_data(&unpaged_body);
    let generation_before = snapshot_generation(&unpaged, "unpaged probe");
    let unpaged_items = unpaged["batch"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("unpaged batch array missing: {unpaged_body}"));
    assert_eq!(
        unpaged_items.len(),
        3,
        "the paged control needs a three-entry result to slice: {unpaged_body}"
    );

    // Control: two pages inside ONE generation are a pure slice of the whole.
    let page_one_body = call_tool(
        app,
        sid,
        902,
        "find_symbol",
        json!({ "names": names, "include_body": false, "page_size": 2 }),
    )
    .await;
    let page_one = tool_data(&page_one_body);
    let cursor = page_one["next_cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("a truncated page must advertise next_cursor: {page_one_body}"))
        .to_owned();
    let page_two_body = call_tool(
        app,
        sid,
        903,
        "find_symbol",
        json!({ "names": names, "include_body": false, "page_size": 2, "cursor": cursor }),
    )
    .await;
    let mut stitched = page_one["batch"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        stitched.len(),
        2,
        "the first page must carry exactly page_size entries: {page_one_body}"
    );
    stitched.extend(
        tool_data(&page_two_body)["batch"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    assert_eq!(
        stitched, unpaged_items,
        "pages served inside one generation must stitch back to the unpaged result: {page_two_body}"
    );

    // Newest content wins: new source advances the committed generation.
    std::fs::write(
        project.join(format!("soak_p{index}_extra.py")),
        format!("def soak_extra_p{index}():\n    return {index}\n"),
    )
    .unwrap();
    let refreshed = call_tool(app, sid, 904, "refresh_symbol_index", json!({})).await;
    assert_transport_ok(&refreshed, "post-write refresh");
    let newest_body = call_tool(
        app,
        sid,
        905,
        "find_symbol",
        json!({ "name": format!("soak_extra_p{index}"), "include_body": false }),
    )
    .await;
    let newest = tool_data(&newest_body);
    let generation_after = snapshot_generation(&newest, "post-refresh probe");
    assert!(
        generation_after > generation_before,
        "indexing new content must advance the committed generation: {generation_before} -> {generation_after} ({newest_body})"
    );
    assert!(
        newest_body.contains(&format!("soak_p{index}_extra.py")),
        "newest content must win: {newest_body}"
    );

    // Mixed generation: the in-flight cursor and the stale pin are both refused.
    let stale_cursor_body = call_tool(
        app,
        sid,
        906,
        "find_symbol",
        json!({ "names": names, "include_body": false, "page_size": 2, "cursor": cursor }),
    )
    .await;
    assert_index_generation_rejected(
        &stale_cursor_body,
        "a page continuation across a generation change",
    );

    let stale_pin_body = call_tool(
        app,
        sid,
        907,
        "find_symbol",
        json!({
            "name": format!("soak_symbol_p{index}"),
            "include_body": false,
            "snapshot": format!("gen:{generation_before}"),
        }),
    )
    .await;
    assert_index_generation_rejected(&stale_pin_body, "a read pinned to the retired generation");
}

// ── the soak ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn five_sessions_and_five_projects_share_one_daemon_without_eviction_or_leakage() {
    let started = std::time::Instant::now();
    let projects: Vec<PathBuf> = (0..SESSIONS).map(soak_project).collect();

    let default_dir = crate::test_helpers::fixtures::temp_project_dir("soak-default");
    std::fs::create_dir_all(default_dir.join(".codelens")).unwrap();
    std::fs::write(
        default_dir.join(".codelens/principals.toml"),
        "[default]\nrole = \"Refactor\"\n",
    )
    .unwrap();
    let state = Arc::new(
        AppState::new(project_root(&default_dir), ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(Arc::clone(&state));

    // Five sessions, each bound to its own project — five live runtimes behind a
    // four-entry idle cache.
    let mut sessions = Vec::with_capacity(SESSIONS);
    for (index, project) in projects.iter().enumerate() {
        let sid = init_session(&app).await;
        let body = call_tool(
            &app,
            &sid,
            2,
            "activate_project",
            json!({ "project": project.display().to_string() }),
        )
        .await;
        assert_transport_ok(&body, &format!("session {index} activate_project"));
        sessions.push(sid);
    }

    // Axis 2 baseline: every runtime is alive (its lease is held) right after the
    // fifth binding, i.e. the fifth insert evicted none of the four before it.
    let baseline: Vec<u64> = projects
        .iter()
        .map(|project| second_writable_runtime_rejection(project))
        .collect();

    for round in 0..ROUNDS {
        let mut reads = Vec::with_capacity(SESSIONS);
        for (index, sid) in sessions.iter().enumerate() {
            let app = app.clone();
            let sid = sid.clone();
            let id = 100 + (round * SESSIONS + index) as u64;
            reads.push(tokio::spawn(async move {
                call_tool(
                    &app,
                    &sid,
                    id,
                    "find_symbol",
                    json!({
                        "name": format!("soak_symbol_p{index}"),
                        "include_body": false,
                        "max_matches": 5,
                    }),
                )
                .await
            }));
        }

        // Axis 3, under concurrent load: a second writable runtime for a project
        // one of the in-flight sessions is reading right now.
        let contended_index = round % SESSIONS;
        let contended = projects[contended_index].clone();
        let contender =
            tokio::task::spawn_blocking(move || second_writable_runtime_rejection(&contended));

        let mut bodies = Vec::with_capacity(SESSIONS);
        for read in reads {
            bodies.push(read.await.expect("session read task must not panic"));
        }
        let holder_generation = contender.await.expect("contender task must not panic");
        assert_eq!(
            holder_generation, baseline[contended_index],
            "project {contended_index} was retired and rebuilt under load (round {round})"
        );

        for (index, body) in bodies.iter().enumerate() {
            assert_reads_own_project_only(body, index, round);
        }
    }

    // Axis 2 after the soak: same holder, same durable lease generation. A
    // shutdown-and-rebuild would have bumped it; a shutdown alone would have let
    // the probe acquire the lease.
    for (index, project) in projects.iter().enumerate() {
        assert_eq!(
            second_writable_runtime_rejection(project),
            baseline[index],
            "project {index} lost or rebuilt its runtime during the soak"
        );
    }

    // Axis 4, with all five runtimes still live.
    assert_mixed_generation_is_discarded(&app, &sessions[0], &projects[0], 0).await;

    // Session-scoped binding must never have touched the daemon-global override.
    assert!(
        !state.has_explicit_active_project(),
        "session-bound calls must leave the daemon default project untouched"
    );

    let elapsed = started.elapsed();
    eprintln!(
        "five-session soak: {SESSIONS} sessions x {ROUNDS} rounds in {:.2}s",
        elapsed.as_secs_f64()
    );
    assert!(
        elapsed < std::time::Duration::from_secs(60),
        "soak must stay inside the CI budget, took {elapsed:?}"
    );
}

/// Defect-predicate (RED) measurement for axis 2. Identical five-project
/// pressure, but no session owns a binding: the fifth activation evicts the
/// least-recently-used runtime and releases its writer lease, which is exactly
/// the failure the session-aware guard prevents above.
#[test]
fn without_session_bindings_the_fifth_project_evicts_an_idle_runtime() {
    let default_dir = crate::test_helpers::fixtures::temp_project_dir("soak-red-default");
    let state = AppState::new_minimal(project_root(&default_dir), ToolPreset::Balanced);

    let controls: Vec<PathBuf> = (0..SESSIONS)
        .map(|index| soak_project(90 + index))
        .collect();
    for control in &controls {
        state
            .switch_project(control.to_str().unwrap())
            .expect("daemon-global activation");
    }

    // Unprotected LRU runtime: retired, lease released, reacquirable.
    let reacquired = AppState::try_new_minimal(project_root(&controls[0]), ToolPreset::Balanced)
        .expect(
            "without a session binding the fifth activation must evict the first runtime and release its writer lease",
        );
    drop(reacquired);

    // The most recent activation is still protected and still holds its lease.
    let busy = match AppState::try_new_minimal(project_root(&controls[4]), ToolPreset::Balanced) {
        Ok(_) => panic!("the freshly activated runtime must keep its writer lease"),
        Err(error) => error,
    };
    assert!(
        matches!(
            busy.downcast::<CodeLensError>()
                .expect("busy error must stay typed"),
            CodeLensError::ProjectWriterBusy { .. }
        ),
        "an in-use runtime must reject a second writer",
    );
}
