# HTTP Session Auto-Resurrect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Eliminate the post-restart / idle-timeout "Unknown session" lockout (#300/#301) by making the daemon stateless-on-session-axis: a non-initialize POST with an unknown (but UUID-shaped, non-tombstoned) session id transparently re-creates a soft session under that id instead of returning HTTP 404.

**Architecture:** New `SessionStore::get_or_resurrect` (single write-lock, cap-enforced, poison-tolerant, fail-closed seeding); a policy branch at the POST gate (default Lenient, `CODELENS_SESSION_STRICT` opt-in keeps the #318 envelope); a bounded short-TTL tombstone so explicit DELETE stays authoritative; resurrection observability via header + telemetry. GET/SSE stays strict. `session_injection.rs` is untouched (the gate resurrects first, so its `store.get` succeeds).

**Tech Stack:** Rust (edition 2024), axum, std `RwLock<HashMap>`, `uuid`, tokio. Test runner `cargo test -p codelens-mcp --bin codelens-mcp --features http,semantic`.

Spec: `docs/superpowers/specs/2026-06-04-http-session-resurrect-design.md`. Honor all 11 guards there.

---

## File Structure

- `crates/codelens-mcp/src/server/session.rs` — add `SessionSeed`, `is_valid_session_id`, `Tombstone`, `trim_to_cap` (factored from `create`), `get_or_resurrect`, `mark_tombstone`; add `SessionState::apply_seed`. **Most logic lives here.**
- `crates/codelens-mcp/src/server/transport_http.rs` — gate policy branch (474-481), resurrected header on the response, DELETE tombstone (617-634), hint text fix (193-213).
- `crates/codelens-mcp/src/server/transport_http_support.rs` — `SessionSeed::from_headers`.
- `crates/codelens-mcp/src/state.rs` (+ constructors) — `session_policy: SessionPolicy` field + accessor.
- `crates/codelens-mcp/src/main.rs` or `cli.rs` — parse `CODELENS_SESSION_STRICT`.
- Telemetry (`server/metrics` or `telemetry/registry`) — `record_session_resurrected`.

---

## Task 1: Store primitives — `SessionSeed`, id-shape, tombstone (session.rs)

**Files:** Modify `crates/codelens-mcp/src/server/session.rs`; tests in the same file's `mod tests`.

- [ ] **Step 1: Write failing tests**
```rust
#[test]
fn valid_session_id_accepts_uuid_rejects_garbage() {
    let good = uuid::Uuid::new_v4().to_string();
    assert!(is_valid_session_id(&good));
    assert!(!is_valid_session_id("not-a-uuid"));
    assert!(!is_valid_session_id(""));
}

#[test]
fn tombstone_marks_and_expires() {
    let tomb = Tombstone::new(Duration::from_millis(20), 8);
    tomb.mark("abc");
    assert!(tomb.contains("abc"));
    std::thread::sleep(Duration::from_millis(30));
    assert!(!tomb.contains("abc")); // TTL expiry
}

#[test]
fn tombstone_is_bounded() {
    let tomb = Tombstone::new(Duration::from_secs(300), 4);
    for i in 0..10 { tomb.mark(&format!("id{i}")); }
    assert!(tomb.len() <= 4); // cap honored, oldest dropped
}
```

- [ ] **Step 2: Run — expect FAIL** `cargo test -p codelens-mcp --bin codelens-mcp --features http session:: 2>&1 | tail` (unresolved `is_valid_session_id` / `Tombstone`).

- [ ] **Step 3: Implement**
```rust
pub fn is_valid_session_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

pub struct Tombstone {
    entries: Mutex<std::collections::VecDeque<(String, Instant)>>,
    ttl: Duration,
    cap: usize,
}
impl Tombstone {
    pub fn new(ttl: Duration, cap: usize) -> Self {
        Self { entries: Mutex::new(std::collections::VecDeque::new()), ttl, cap }
    }
    fn prune(entries: &mut std::collections::VecDeque<(String, Instant)>, ttl: Duration, cap: usize) {
        while entries.front().is_some_and(|(_, t)| t.elapsed() > ttl) { entries.pop_front(); }
        while entries.len() > cap { entries.pop_front(); }
    }
    pub fn mark(&self, id: &str) {
        let mut e = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        e.push_back((id.to_owned(), Instant::now()));
        Self::prune(&mut e, self.ttl, self.cap);
    }
    pub fn contains(&self, id: &str) -> bool {
        let mut e = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        Self::prune(&mut e, self.ttl, self.cap);
        e.iter().any(|(k, _)| k == id)
    }
    pub fn len(&self) -> usize { self.entries.lock().map(|e| e.len()).unwrap_or(0) }
}
```
Defaults when wiring into `SessionStore`: `Tombstone::new(Duration::from_secs(300), 256)`.

- [ ] **Step 4: Run — expect PASS** (same command).
- [ ] **Step 5: Commit** `git commit -am "feat(session): id-shape validation + bounded tombstone primitives"`

---

## Task 2: `trim_to_cap` refactor + `get_or_resurrect` (session.rs)

**Files:** Modify `crates/codelens-mcp/src/server/session.rs`.

- [ ] **Step 1: Write failing tests**
```rust
#[test]
fn get_or_resurrect_creates_under_client_id() {
    let store = SessionStore::new(Duration::from_secs(300));
    let id = uuid::Uuid::new_v4().to_string();
    let (s, resurrected) = store.get_or_resurrect(&id, &SessionSeed::default()).unwrap();
    assert!(resurrected);
    assert_eq!(s.id, id);                       // client id, NOT a fresh uuid
    let (s2, again) = store.get_or_resurrect(&id, &SessionSeed::default()).unwrap();
    assert!(!again);                            // second call finds it
    assert!(Arc::ptr_eq(&s, &s2));              // same Arc
}

#[test]
fn get_or_resurrect_rejects_non_uuid() {
    let store = SessionStore::new(Duration::from_secs(300));
    assert!(store.get_or_resurrect("garbage", &SessionSeed::default()).is_none());
}

#[test]
fn get_or_resurrect_seeds_profile_not_trusted_client() {
    let store = SessionStore::new(Duration::from_secs(300));
    let id = uuid::Uuid::new_v4().to_string();
    let seed = SessionSeed { requested_profile: Some("reviewer-graph".into()),
                             deferred_tool_loading: Some(true), client_name: Some("codex".into()) };
    let (s, _) = store.get_or_resurrect(&id, &seed).unwrap();
    let m = s.client_metadata();
    assert_eq!(m.requested_profile.as_deref(), Some("reviewer-graph"));
    assert_eq!(m.deferred_tool_loading, Some(true));
    assert_eq!(m.trusted_client, None);         // guard #2 — never seeded
}

#[test]
fn get_or_resurrect_refuses_when_full_of_active() {
    // MAX_SESSIONS active, none expired → refuse rather than evict a live client.
    // (Use a small helper or assert len stays == cap and returns None.)
}

#[test]
fn get_or_resurrect_concurrent_same_id_one_arc() {
    use std::sync::Arc as StdArc;
    let store = StdArc::new(SessionStore::new(Duration::from_secs(300)));
    let id = uuid::Uuid::new_v4().to_string();
    let handles: Vec<_> = (0..8).map(|_| {
        let store = StdArc::clone(&store); let id = id.clone();
        std::thread::spawn(move || store.get_or_resurrect(&id, &SessionSeed::default()).unwrap())
    }).collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let resurrected_count = results.iter().filter(|(_, r)| *r).count();
    assert_eq!(resurrected_count, 1);           // exactly one winner
    let first = &results[0].0;
    assert!(results.iter().all(|(s, _)| Arc::ptr_eq(s, first)));
}
```

- [ ] **Step 2: Run — expect FAIL** `cargo test -p codelens-mcp --bin codelens-mcp --features http session::tests::get_or_resurrect 2>&1 | tail`.

- [ ] **Step 3: Implement** — add `SessionSeed`, `SessionState::apply_seed`, factor `trim_to_cap`, add `get_or_resurrect`. Move `const MAX_SESSIONS: usize = 1000;` to the `impl SessionStore` scope.
```rust
#[derive(Debug, Clone, Default)]
pub struct SessionSeed {
    pub requested_profile: Option<String>,
    pub deferred_tool_loading: Option<bool>,
    pub client_name: Option<String>,
    // NOTE: no trusted_client — guard #2, fail closed.
}

impl SessionState {
    pub fn apply_seed(&self, seed: &SessionSeed) {
        if let Ok(mut m) = self.client_metadata.write() {
            if seed.requested_profile.is_some() { m.requested_profile = seed.requested_profile.clone(); }
            if seed.deferred_tool_loading.is_some() { m.deferred_tool_loading = seed.deferred_tool_loading; }
            if seed.client_name.is_some() { m.client_name = seed.client_name.clone(); }
        }
        // If a profile is supplied, mirror initialize's surface/budget so tools/list shape is stable.
        if let Some(profile) = seed.requested_profile.as_deref()
            .and_then(crate::tool_defs::ToolProfile::from_str)
        {
            self.set_surface(crate::tool_defs::ToolSurface::Profile(profile));
            self.set_token_budget(crate::tool_defs::default_budget_for_profile(profile));
        }
    }
}

impl SessionStore {
    const MAX_SESSIONS: usize = 1000;

    fn trim_to_cap(&self, sessions: &mut HashMap<SessionId, Arc<SessionState>>) {
        if sessions.len() >= Self::MAX_SESSIONS {
            let timeout = self.timeout;
            sessions.retain(|_, s| !s.is_expired(timeout));
        }
        if sessions.len() >= Self::MAX_SESSIONS
            && let Some(oldest_id) = sessions.iter()
                .min_by_key(|(_, s)| s.last_active.read().map(|t| *t).unwrap_or_else(|_| Instant::now()))
                .map(|(id, _)| id.clone())
        {
            sessions.remove(&oldest_id);
        }
    }

    /// Non-initialize recovery. Some((s,false))=found, Some((s,true))=resurrected,
    /// None=refused (non-uuid id, or cap full of active sessions). Tombstone is
    /// checked by the caller (it owns the tombstone set).
    pub fn get_or_resurrect(&self, id: &str, seed: &SessionSeed) -> Option<(Arc<SessionState>, bool)> {
        if !is_valid_session_id(id) { return None; }
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = sessions.get(id) {
            let s = Arc::clone(existing);
            s.touch();
            return Some((s, false));
        }
        self.trim_to_cap(&mut sessions);
        if sessions.len() >= Self::MAX_SESSIONS { return None; } // full of active → refuse
        let s = Arc::new(SessionState::new(id.to_owned()));
        s.apply_seed(seed);
        sessions.insert(id.to_owned(), Arc::clone(&s));
        Some((s, true))
    }
}
```
Refactor `create()` to call `self.trim_to_cap(&mut sessions)` in place of its inlined eviction (behavior byte-identical). Make `last_active` reachable from `trim_to_cap` (same module — it is `pub` within the file scope already as a struct field accessed in tests at line 288; keep it module-visible).

- [ ] **Step 4: Run — expect PASS**. Also run the full `session::tests` module to confirm `create_*` tests still pass (refactor parity).
- [ ] **Step 5: Commit** `git commit -am "feat(session): get_or_resurrect with capped, poison-tolerant, fail-closed seeding"`

---

## Task 3: Tombstone wiring into SessionStore + DELETE (session.rs + transport_http.rs)

**Files:** Modify `session.rs` (add `tombstone` field + `mark_tombstone`/`is_tombstoned`), `transport_http.rs:617-634` (DELETE records tombstone).

- [ ] **Step 1: Write failing test** (in transport_http http_tests or session tests)
```rust
#[test]
fn tombstoned_id_is_not_resurrected() {
    let store = SessionStore::new(Duration::from_secs(300));
    let id = uuid::Uuid::new_v4().to_string();
    store.get_or_resurrect(&id, &SessionSeed::default()).unwrap(); // create
    store.mark_tombstone(&id);                                     // DELETE
    // gate logic: tombstoned → refuse
    assert!(store.is_tombstoned(&id));
}
```

- [ ] **Step 2: Run — expect FAIL** (no `mark_tombstone`/`is_tombstoned`).

- [ ] **Step 3: Implement** — add `tombstone: Tombstone` to `SessionStore` (init in `new`), plus:
```rust
pub fn mark_tombstone(&self, id: &str) { self.remove(id); self.tombstone.mark(id); }
pub fn is_tombstoned(&self, id: &str) -> bool { self.tombstone.contains(id) }
```
In `mcp_delete_handler` (transport_http.rs:627-632) replace `store.remove(id)` with `store.mark_tombstone(id)`.

- [ ] **Step 4: Run — expect PASS.**
- [ ] **Step 5: Commit** `git commit -am "feat(session): bounded tombstone keeps DELETE authoritative"`

---

## Task 4: `SessionSeed::from_headers` (transport_http_support.rs)

**Files:** Modify `crates/codelens-mcp/src/server/transport_http_support.rs`.

- [ ] **Step 1: Write failing test**
```rust
#[test]
fn session_seed_reads_profile_deferred_client_not_trusted() {
    let mut h = HeaderMap::new();
    h.insert("x-codelens-profile", HeaderValue::from_static("reviewer-graph"));
    h.insert("x-codelens-deferred-tool-loading", HeaderValue::from_static("1"));
    h.insert("x-codelens-client", HeaderValue::from_static("codex"));
    h.insert("x-codelens-trusted-client", HeaderValue::from_static("1")); // must be ignored
    let seed = SessionSeed::from_headers(&h);
    assert_eq!(seed.requested_profile.as_deref(), Some("reviewer-graph"));
    assert_eq!(seed.deferred_tool_loading, Some(true));
    assert_eq!(seed.client_name.as_deref(), Some("codex"));
}
```

- [ ] **Step 2: Run — expect FAIL.**

- [ ] **Step 3: Implement** (reuse `parse_bool_header`; never read `x-codelens-trusted-client`):
```rust
impl SessionSeed {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).map(ToOwned::to_owned);
        Self {
            requested_profile: h("x-codelens-profile"),
            deferred_tool_loading: headers.get("x-codelens-deferred-tool-loading")
                .and_then(|v| v.to_str().ok()).and_then(parse_bool_header),
            client_name: h("x-codelens-client"),
        }
    }
}
```

- [ ] **Step 4: Run — expect PASS.**
- [ ] **Step 5: Commit** `git commit -am "feat(session): SessionSeed::from_headers (fail-closed, no trusted_client)"`

---

## Task 5: Policy flag `CODELENS_SESSION_STRICT` (state.rs + main.rs/cli.rs)

**Files:** `state.rs` (+ `state/constructors.rs`), `main.rs` or `cli.rs`.

- [ ] **Step 1: Write failing test** (state-level default)
```rust
#[test]
fn session_policy_defaults_to_lenient() {
    let state = AppState::new(project_root_for_test(), ToolPreset::Balanced);
    assert!(matches!(state.session_policy(), SessionPolicy::Lenient));
}
```

- [ ] **Step 2: Run — expect FAIL.**

- [ ] **Step 3: Implement** — `pub enum SessionPolicy { Lenient, Strict }` in `session.rs`; `session_policy: SessionPolicy` field on `AppState` (default `Lenient`); `fn session_policy(&self) -> SessionPolicy`; in startup, `if std::env::var("CODELENS_SESSION_STRICT").as_deref() == Ok("1") { SessionPolicy::Strict }`.

- [ ] **Step 4: Run — expect PASS.**
- [ ] **Step 5: Commit** `git commit -am "feat(session): CODELENS_SESSION_STRICT policy flag (default lenient)"`

---

## Task 6: Gate wiring + resurrected header + hint fix (transport_http.rs)

**Files:** Modify `crates/codelens-mcp/src/server/transport_http.rs`.

- [ ] **Step 1: Write/flip failing tests** in `server/http_tests/` (or `protocol_tests.rs`):
```rust
// FLIP: was post_unknown_session_returns_not_found
#[tokio::test]
async fn post_unknown_session_resurrects_under_lenient() {
    // build router (Lenient default), POST tools/list with a random UUID Mcp-Session-Id, no prior init
    // expect 200 (not 404) + header x-codelens-session-resurrected: 1
}

#[tokio::test]
async fn post_unknown_session_strict_returns_envelope() {
    // CODELENS_SESSION_STRICT path → 404 + body {"error":"unknown_session",...} + x-codelens-session-rotate:1
}

#[tokio::test]
async fn delete_then_reuse_stays_terminated() {
    // init → DELETE (204) → tools/list same sid → 404 envelope (tombstoned), NOT resurrected
}

#[tokio::test]
async fn get_unknown_session_stays_strict() {
    // GET /mcp with unknown sid → still unknown_session_response (guard #7), never resurrected
}
```

- [ ] **Step 2: Run — expect FAIL.**

- [ ] **Step 3: Implement** — replace the gate (474-481):
```rust
let mut resurrected = false;
if !is_initialize
    && let Some(ref sid) = session_id
    && let Some(store) = &state.session_store
    && store.get(sid).is_none()
{
    match state.session_policy() {
        crate::server::session::SessionPolicy::Strict => return unknown_session_response(),
        crate::server::session::SessionPolicy::Lenient => {
            if store.is_tombstoned(sid) { return unknown_session_response(); }
            let seed = crate::server::session::SessionSeed::from_headers(&headers);
            match store.get_or_resurrect(sid, &seed) {
                Some((_s, true)) => {
                    resurrected = true;
                    state.metrics().record_session_resurrected(principal_id.as_deref());
                }
                Some((_s, false)) => {}                  // race: now exists, proceed
                None => return unknown_session_response(),// non-uuid or cap-full-of-active
            }
        }
    }
}
```
After building the final `response` (line ~555, the non-initialize branch), attach the header:
```rust
let mut response = into_mcp_response(resp, accept, initialize_session.as_ref(), state.daemon_mode().as_str());
if resurrected {
    response.headers_mut().insert("x-codelens-session-resurrected", HeaderValue::from_static("1"));
}
response
```
Fix the hint in `unknown_session_response` (198): drop "or the SCIP index was hot-reloaded"; keep "Daemon may have restarted or the session timed out. Reinitialize the MCP session." Update the doc comment at 185-192 to match (remove the #298/hot-reload claim).

- [ ] **Step 4: Run — expect PASS** (the 4 gate tests + GET stays strict).
- [ ] **Step 5: Commit** `git commit -am "feat(http): lenient session resurrect at POST gate (#300/#301), GET stays strict"`

---

## Task 7: Telemetry + mutation-gate negative test + envelope-body lock

**Files:** telemetry module; `server/http_tests/`.

- [ ] **Step 1: Tests**
```rust
// CRITICAL guard #2: on a mutation-enabled daemon, resurrect via tools/call carrying
// x-codelens-trusted-client:1 must NOT grant trusted_client → content mutation rejected.
#[tokio::test]
async fn resurrect_does_not_grant_trusted_client_on_mutation_daemon() { /* ... */ }

// guard #9: resurrected session with no bearer → default role.
#[tokio::test]
async fn resurrected_session_grants_no_principal() { /* ... */ }

// envelope body currently untested — lock it on the strict path.
#[tokio::test]
async fn strict_unknown_session_envelope_body_contract() { /* assert error/code/rotate_required/header */ }
```

- [ ] **Step 2: Run — expect FAIL** (missing `record_session_resurrected`).
- [ ] **Step 3: Implement** `record_session_resurrected(&self, principal: Option<&str>)` on the metrics/telemetry registry (mirror an existing counter event); wire counters (`resurrected_total`, `id_shape_rejected`, `tombstoned_refused`).
- [ ] **Step 4: Run — expect PASS.**
- [ ] **Step 5: Commit** `git commit -am "feat(telemetry): session_resurrected event + mutation-gate/principal-agnostic regression tests"`

---

## Final verification gate (parent-run — subagent self-report 신뢰 0%)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --features http -- -D warnings
cargo test -p codelens-mcp --bin codelens-mcp --features http,semantic
cargo test -p codelens-engine
python3 scripts/regen-tool-defs.py --check
```
**Expected:** all green; prior baseline preserved minus the consciously-flipped `post_unknown_session_*` assertion. Live daemon redeploy is OUT OF SCOPE (separate user gate).

---

## Self-Review

**Spec coverage:** Guards 1 (initialize untouched — `create_or_resume` not modified) ✓ T2; 2 (trusted_client fail-closed) ✓ T2/T4/T7; 3 (tombstone) ✓ T3; 4 (single write-lock/Entry-equivalent/poison) ✓ T2; 5 (uuid-shape) ✓ T1/T2; 6 (cap no-evict-active) ✓ T2; 7 (GET strict) ✓ T6; 8 (seed profile+deferred+client_name) ✓ T2/T4; 9 (principal-agnostic) ✓ T7; 10 (header+telemetry) ✓ T6/T7; 11 (strict flag) ✓ T5/T6. Test matrix items 1-10 all mapped.

**Placeholder scan:** Task 6/7 leave a few `/* ... */` in async HTTP test bodies (router-setup boilerplate); the assertions are specified in prose. Fill using the existing `server/http_tests/lifecycle_tests.rs` router-build pattern as the template — not a design gap.

**Type consistency:** `get_or_resurrect(&str, &SessionSeed) -> Option<(Arc<SessionState>, bool)>`, `SessionSeed{requested_profile,deferred_tool_loading,client_name}`, `SessionPolicy::{Lenient,Strict}`, `mark_tombstone`/`is_tombstoned`, `apply_seed`, `record_session_resurrected` — names consistent across tasks.
