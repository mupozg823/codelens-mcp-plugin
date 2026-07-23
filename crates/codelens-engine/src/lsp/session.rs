use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

use super::commands::{LspLaunchPolicy, ValidatedLspInvocation, validate_lsp_invocation};
use super::protocol::{language_id_for_path, poll_readable, read_message, send_message};
use super::types::{
    LspCodeActionRefactorPlan, LspCodeActionRefactorResult, LspCodeActionRequest, LspDiagnostic,
    LspDiagnosticRequest, LspReference, LspRenamePlan, LspRenamePlanRequest, LspRenameRequest,
    LspRequest, LspResolveTargetRequest, LspResolvedTarget, LspTypeHierarchyRequest,
    LspWorkspaceEditTransaction, LspWorkspaceSymbol, LspWorkspaceSymbolRequest,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionKey {
    recipe_binary: &'static str,
    executable: PathBuf,
    args: Vec<String>,
}

impl SessionKey {
    fn new(invocation: &ValidatedLspInvocation) -> Self {
        Self {
            recipe_binary: invocation.recipe_binary(),
            executable: invocation.executable().to_owned(),
            args: invocation.args().to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
struct OpenDocumentState {
    version: i32,
    text: String,
}

pub struct LspSessionPool {
    project: ProjectRoot,
    launch_policy: LspLaunchPolicy,
    sessions: std::sync::Mutex<HashMap<SessionKey, LspSession>>,
}

pub(super) struct LspSession {
    pub(super) project: ProjectRoot,
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_request_id: u64,
    documents: HashMap<String, OpenDocumentState>,
    #[allow(dead_code)] // retained for future stderr diagnostics
    stderr_buffer: std::sync::Arc<std::sync::Mutex<String>>,
    /// Server-reported readiness (P1.1): `Some(true)` once the server has
    /// signalled a quiescent state (rust-analyzer `experimental/serverStatus`
    /// with `quiescent: true`), `Some(false)` while it reports active
    /// indexing, `None` for servers that never emit a readiness signal.
    /// Consumed by warm-routing/confidence calibration — a warm session is
    /// not necessarily a *quiescent* one.
    server_quiescent: Option<bool>,
}

fn ensure_validated_session<'a>(
    sessions: &'a mut HashMap<SessionKey, LspSession>,
    project: &ProjectRoot,
    invocation: ValidatedLspInvocation,
) -> Result<&'a mut LspSession> {
    let key = SessionKey::new(&invocation);

    // Check for dead sessions: if the child process has exited, remove the stale entry.
    if let Some(session) = sessions.get_mut(&key) {
        match session.child.try_wait() {
            Ok(Some(_status)) => {
                // Process exited — remove stale session so we start fresh below.
                sessions.remove(&key);
            }
            Ok(None) => {} // Still running — will return it via Occupied below.
            Err(_) => {
                sessions.remove(&key);
            }
        }
    }

    match sessions.entry(key) {
        std::collections::hash_map::Entry::Occupied(e) => Ok(e.into_mut()),
        std::collections::hash_map::Entry::Vacant(e) => {
            let session = LspSession::start(project, &invocation)?;
            Ok(e.insert(session))
        }
    }
}

fn is_retriable_lsp_transport_error(err: &anyhow::Error) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    [
        "unexpected eof",
        "broken pipe",
        "connection reset",
        "connection aborted",
        "transport endpoint is not connected",
        "os error 32",
        "os error 54",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

impl LspSessionPool {
    pub fn new(project: ProjectRoot) -> Self {
        Self {
            project,
            launch_policy: LspLaunchPolicy::from_environment(),
            sessions: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Add an executable to this host-controlled LSP trust set.
    ///
    /// This is an embedding/configuration API, not a request parameter. Hosts
    /// must never forward caller-controlled paths here. Registration verifies
    /// the recipe name, canonicalizes the executable, and clears existing
    /// sessions before the new mapping can be used.
    pub fn register_trusted_lsp_binary(
        &self,
        command: &str,
        executable: impl AsRef<Path>,
    ) -> Result<PathBuf> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let canonical = self
            .launch_policy
            .register_trusted_binary(command, executable.as_ref())?;
        sessions.clear();
        Ok(canonical)
    }

    /// Return the canonical executable preconfigured for a recipe.
    pub fn trusted_lsp_binary(&self, command: &str) -> Option<PathBuf> {
        self.launch_policy.trusted_binary(command)
    }

    fn validate_invocation(
        &self,
        command: &str,
        args: &[String],
    ) -> Result<ValidatedLspInvocation> {
        validate_lsp_invocation(&self.launch_policy, command, args)
    }

    fn ensure_session<'a>(
        &self,
        sessions: &'a mut HashMap<SessionKey, LspSession>,
        command: &str,
        args: &[String],
    ) -> Result<&'a mut LspSession> {
        let invocation = self.validate_invocation(command, args)?;
        ensure_validated_session(sessions, &self.project, invocation)
    }

    /// Replace the project root and close all existing sessions.
    pub fn reset(&self, project: ProjectRoot) -> Self {
        // Drop existing sessions so LSP processes are killed.
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        Self {
            project,
            launch_policy: self.launch_policy.clone(),
            sessions: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Shutdown all active LSP sessions in this pool by dropping them.
    pub fn shutdown(&self) {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    pub fn session_count(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len()
    }

    /// Non-spawning warmth probe for the latency-sensitive default reference
    /// path. Returns `true` only when a live LSP session for `command`+`args`
    /// is already resident (child process still running). It **never spawns**
    /// a server — a cold or absent language returns `false` and leaves the
    /// pool untouched, so callers can gate precise LSP routing on warmth
    /// without risking a 2-30s cold start. Stale (exited) sessions are reaped
    /// as a side effect, mirroring `ensure_session`.
    pub fn has_warm_session(&self, command: &str, args: &[String]) -> bool {
        let Ok(invocation) = self.validate_invocation(command, args) else {
            return false;
        };
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let key = SessionKey::new(&invocation);
        match sessions.get_mut(&key) {
            Some(session) => match session.child.try_wait() {
                Ok(None) => true,
                _ => {
                    sessions.remove(&key);
                    false
                }
            },
            None => false,
        }
    }

    /// Spawn-and-initialize a session so later default-path calls find it
    /// warm (P1.3 pre-warm pool). Idempotent: an already-live session is a
    /// no-op. Runs the full spawn+initialize handshake, so callers should
    /// invoke this from a background thread — never on a bind/request hot
    /// path. Recipe and executable trust is enforced by `ensure_session`.
    pub fn prewarm_session(&self, command: &str, args: &[String]) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        self.ensure_session(&mut sessions, command, args)
            .map(|_| ())
    }

    /// Readiness of a warm session (P1.1): outer `None` = no live session for
    /// this server; `Some(None)` = live but the server never emitted a
    /// readiness signal (unknown — do NOT assume ready); `Some(Some(q))` =
    /// the server's latest `experimental/serverStatus` quiescence state.
    /// Warm ≠ quiescent: confidence calibration must treat `Some(Some(false))`
    /// (still indexing) as degraded evidence.
    pub fn warm_session_quiescence(&self, command: &str, args: &[String]) -> Option<Option<bool>> {
        let invocation = self.validate_invocation(command, args).ok()?;
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let key = SessionKey::new(&invocation);
        match sessions.get_mut(&key) {
            Some(session) => match session.child.try_wait() {
                Ok(None) => Some(session.server_quiescent()),
                _ => {
                    sessions.remove(&key);
                    None
                }
            },
            None => None,
        }
    }

    pub fn find_referencing_symbols(&self, request: &LspRequest) -> Result<Vec<LspReference>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.find_references(request)
    }

    /// Same as [`find_referencing_symbols`], but also reports whether *this*
    /// call had to spawn the LSP server (a cold start). `false` means an
    /// already-resident warm session was reused. The warmth check and the
    /// spawn decision happen under the same lock, so the flag is race-free for
    /// this call: a caller that gated on an earlier `has_warm_session` probe
    /// can use it to detect the rare TOCTOU case where the server died between
    /// the probe and this request and had to be respawned mid-flight. Routing
    /// is unchanged — the flag only lets the caller describe what happened.
    pub fn find_referencing_symbols_tracking_spawn(
        &self,
        request: &LspRequest,
    ) -> Result<(Vec<LspReference>, bool)> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let invocation = self.validate_invocation(&request.command, &request.args)?;
        let key = SessionKey::new(&invocation);
        // A live child for this key means `ensure_session` reuses it (no
        // spawn); its absence or a dead child means it will spawn below.
        let was_warm = sessions
            .get_mut(&key)
            .map(|session| matches!(session.child.try_wait(), Ok(None)))
            .unwrap_or(false);
        let session = ensure_validated_session(&mut sessions, &self.project, invocation)?;
        let references = session.find_references(request)?;
        Ok((references, !was_warm))
    }

    pub fn get_diagnostics(&self, request: &LspDiagnosticRequest) -> Result<Vec<LspDiagnostic>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let result = {
            let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
            session.get_diagnostics(request)
        };

        match result {
            Ok(diagnostics) => Ok(diagnostics),
            Err(err) if is_retriable_lsp_transport_error(&err) => {
                let invocation = self.validate_invocation(&request.command, &request.args)?;
                let key = SessionKey::new(&invocation);
                sessions.remove(&key);
                let session = ensure_validated_session(&mut sessions, &self.project, invocation)?;
                session
                    .get_diagnostics(request)
                    .with_context(|| "retried diagnostics after stale LSP transport")
            }
            Err(err) => Err(err),
        }
    }

    pub fn search_workspace_symbols(
        &self,
        request: &LspWorkspaceSymbolRequest,
    ) -> Result<Vec<LspWorkspaceSymbol>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.search_workspace_symbols(request)
    }

    pub fn get_type_hierarchy(
        &self,
        request: &LspTypeHierarchyRequest,
    ) -> Result<HashMap<String, Value>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.get_type_hierarchy(request)
    }

    pub fn resolve_symbol_target(
        &self,
        request: &LspResolveTargetRequest,
    ) -> Result<Vec<LspResolvedTarget>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.resolve_symbol_target(request)
    }

    pub fn get_rename_plan(&self, request: &LspRenamePlanRequest) -> Result<LspRenamePlan> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.get_rename_plan(request)
    }

    pub fn rename_symbol(&self, request: &LspRenameRequest) -> Result<crate::rename::RenameResult> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.rename_symbol(request)
    }

    pub fn rename_symbol_transaction(
        &self,
        request: &LspRenameRequest,
    ) -> Result<LspWorkspaceEditTransaction> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.rename_symbol_transaction(request)
    }

    pub fn code_action_refactor(
        &self,
        request: &LspCodeActionRequest,
    ) -> Result<LspCodeActionRefactorResult> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.code_action_refactor(request)
    }

    pub fn code_action_refactor_plan(
        &self,
        request: &LspCodeActionRequest,
    ) -> Result<LspCodeActionRefactorPlan> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.ensure_session(&mut sessions, &request.command, &request.args)?;
        session.code_action_refactor_plan(request)
    }
}

impl LspSession {
    fn start(project: &ProjectRoot, invocation: &ValidatedLspInvocation) -> Result<Self> {
        let mut child = Command::new(invocation.executable())
            .args(invocation.args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn trusted LSP server {}",
                    invocation.executable().display()
                )
            })?;

        let stdin = child.stdin.take().context("failed to open LSP stdin")?;
        let stdout = child.stdout.take().context("failed to open LSP stdout")?;

        // Capture stderr in a background thread (bounded 4KB ring buffer).
        let stderr_buffer = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        if let Some(stderr) = child.stderr.take() {
            let buf = std::sync::Arc::clone(&stderr_buffer);
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Ok(mut b) = buf.lock() {
                        if b.len() > 4096 {
                            let drain_to = b.len() - 2048;
                            b.drain(..drain_to);
                        }
                        b.push_str(&line);
                    }
                    line.clear();
                }
            });
        }

        let mut session = Self {
            project: project.clone(),
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_request_id: 1,
            documents: HashMap::new(),
            stderr_buffer,
            server_quiescent: None,
        };
        session.initialize(invocation.recipe_binary())?;
        if let Some(grace) = configured_startup_grace() {
            thread::sleep(grace);
        }
        Ok(session)
    }

    fn initialize(&mut self, command: &str) -> Result<()> {
        let id = self.next_id();
        let root_uri = Url::from_directory_path(self.project.as_path())
            .ok()
            .map(|url| url.to_string());
        let workspace_name = self
            .project
            .as_path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace")
            .to_owned();
        self.send_request(
            id,
            "initialize",
            initialize_params(
                root_uri,
                &workspace_name,
                initialization_options_for_command(command),
            ),
        )?;
        let _ = self.read_response_for_id(id)?;
        self.send_notification("initialized", json!({}))?;
        Ok(())
    }

    pub(super) fn prepare_document(&mut self, absolute_path: &Path) -> Result<(String, String)> {
        let uri = Url::from_file_path(absolute_path).map_err(|_| {
            anyhow::anyhow!("failed to build file uri for {}", absolute_path.display())
        })?;
        let uri_string = uri.to_string();
        let source = std::fs::read_to_string(absolute_path)
            .with_context(|| format!("failed to read {}", absolute_path.display()))?;
        let language_id = language_id_for_path(absolute_path)?;
        self.sync_document(&uri_string, language_id, &source)?;
        Ok((uri_string, source))
    }

    pub(super) fn sync_document(
        &mut self,
        uri: &str,
        language_id: &str,
        source: &str,
    ) -> Result<()> {
        if let Some(state) = self.documents.get(uri)
            && state.text == source
        {
            return Ok(());
        }

        if let Some(state) = self.documents.get_mut(uri) {
            state.version += 1;
            state.text = source.to_owned();
            let version = state.version;
            return self.send_notification(
                "textDocument/didChange",
                json!({
                    "textDocument":{"uri":uri,"version":version},
                    "contentChanges":[{"text":source}]
                }),
            );
        }

        self.documents.insert(
            uri.to_owned(),
            OpenDocumentState {
                version: 1,
                text: source.to_owned(),
            },
        );
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument":{
                    "uri":uri,
                    "languageId":language_id,
                    "version":1,
                    "text":source
                }
            }),
        )
    }

    pub(super) fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    pub(super) fn send_request(&mut self, id: u64, method: &str, params: Value) -> Result<()> {
        send_message(
            &mut self.stdin,
            &json!({
                "jsonrpc":"2.0",
                "id":id,
                "method":method,
                "params":params
            }),
        )
    }

    fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        send_message(
            &mut self.stdin,
            &json!({
                "jsonrpc":"2.0",
                "method":method,
                "params":params
            }),
        )
    }

    pub(super) fn read_response_for_id(&mut self, expected_id: u64) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut discarded = 0u32;
        const MAX_DISCARDED: u32 = 500;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "LSP response timeout: no response for request id {expected_id} within 30s \
                     ({discarded} unrelated messages discarded)"
                );
            }
            if discarded >= MAX_DISCARDED {
                bail!(
                    "LSP response loop: discarded {MAX_DISCARDED} messages without finding id {expected_id}"
                );
            }

            // Poll the pipe before blocking read — prevents infinite hang
            if !poll_readable(self.reader.get_ref(), remaining.min(Duration::from_secs(5))) {
                continue; // no data yet, re-check deadline
            }

            let message = read_message(&mut self.reader)?;
            let method = message.get("method").and_then(Value::as_str);

            // P1.1: server→client REQUEST (both `id` and `method` present).
            // Historically these were discarded, which violates the protocol —
            // a server blocked on `workspace/configuration` either stalls or
            // falls back to defaults nondeterministically. Answer instead of
            // discarding; unknown methods get a spec-correct MethodNotFound.
            if let Some(method) = method {
                if let Some(request_id) = message.get("id").filter(|id| !id.is_null()) {
                    let request_id = request_id.clone();
                    let reply = server_request_reply_payload(method, message.get("params"));
                    self.answer_server_request(&request_id, reply)?;
                } else {
                    // Server notification: harvest readiness signals before
                    // dropping. Counted against MAX_DISCARDED so a
                    // notification-flooding server still trips the breaker.
                    self.observe_server_notification(method, message.get("params"));
                    discarded += 1;
                }
                continue;
            }

            let matches_id = message
                .get("id")
                .and_then(Value::as_u64)
                .map(|id| id == expected_id)
                .unwrap_or(false);
            if matches_id {
                if let Some(error) = message.get("error") {
                    let code = error.get("code").and_then(Value::as_i64).unwrap_or(-1);
                    let error_message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown LSP error");
                    bail!("LSP request failed ({code}): {error_message}");
                }
                return Ok(message);
            }
            discarded += 1;
        }
    }

    /// Send the prepared reply for a server→client request.
    fn answer_server_request(
        &mut self,
        request_id: &Value,
        reply: std::result::Result<Value, Value>,
    ) -> Result<()> {
        let body = match reply {
            Ok(result) => json!({"jsonrpc":"2.0","id":request_id,"result":result}),
            Err(error) => json!({"jsonrpc":"2.0","id":request_id,"error":error}),
        };
        send_message(&mut self.stdin, &body)
    }

    /// Harvest readiness state from server notifications (P1.1).
    fn observe_server_notification(&mut self, method: &str, params: Option<&Value>) {
        if method == "experimental/serverStatus"
            && let Some(quiescent) = params
                .and_then(|params| params.get("quiescent"))
                .and_then(Value::as_bool)
        {
            self.server_quiescent = Some(quiescent);
        }
    }

    /// True once the server has reported a quiescent (fully indexed) state.
    /// `None` when the server never emitted a readiness signal — callers must
    /// treat that as "unknown", not "ready".
    pub(super) fn server_quiescent(&self) -> Option<bool> {
        self.server_quiescent
    }

    fn shutdown(&mut self) -> Result<()> {
        let id = self.next_id();
        self.send_request(id, "shutdown", Value::Null)?;
        let _ = self.read_response_for_id(id)?;
        self.send_notification("exit", Value::Null)
    }
}

/// Pure decision table for server→client requests (P1.1): what to reply.
/// `Ok` carries the `result` payload, `Err` carries the `error` payload.
///
/// - `workspace/configuration` — one `null` per requested item = "use your
///   defaults". Deterministic, and unblocks servers that wait on the reply.
/// - `client/registerCapability` / `client/unregisterCapability` /
///   `window/workDoneProgress/create` — plain acknowledgement (`null`).
/// - `workspace/applyEdit` — REFUSED (`applied: false`): the read path must
///   never let a server mutate the workspace behind the caller's back; every
///   CodeLens mutation flows through the verifier-gated edit transaction.
/// - anything else — spec-correct `MethodNotFound` (-32601) instead of
///   silence, so the server can degrade deterministically.
fn server_request_reply_payload(
    method: &str,
    params: Option<&Value>,
) -> std::result::Result<Value, Value> {
    match method {
        "workspace/configuration" => {
            let item_count = params
                .and_then(|params| params.get("items"))
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            Ok(Value::Array(vec![Value::Null; item_count]))
        }
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create" => Ok(Value::Null),
        "workspace/applyEdit" => Ok(json!({
            "applied": false,
            "failureReason": "codelens read sessions do not accept server-initiated edits"
        })),
        _ => Err(json!({
            "code": -32601,
            "message": format!("method not supported by codelens LSP client: {method}")
        })),
    }
}

/// Server-specific `initializationOptions` table (P1.1c).
///
/// Extension policy: officially documented options only, and the minimum
/// set per server — an unknown or unlisted server MUST get `None`, which
/// sends `initialize` without an `initializationOptions` field at all.
/// Matching is on the binary file name (mirroring `is_allowed_lsp_command`)
/// so path-qualified commands hit the same entry.
fn initialization_options_for_command(command: &str) -> Option<Value> {
    let binary = Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);
    match binary {
        // rust-analyzer (documented option): this client only drives
        // references/navigation, never reads save-time diagnostics, so
        // `cargo check` on save is pure daemon CPU cost.
        "rust-analyzer" => Some(json!({"checkOnSave": false})),
        _ => None,
    }
}

/// Build the `initialize` request params. The capabilities payload is
/// invariant across servers; `initialization_options` is attached as the
/// `initializationOptions` field only when `Some` — a `None` entry must
/// produce the exact pre-P1.1c params shape (no empty/null field).
fn initialize_params(
    root_uri: Option<String>,
    workspace_name: &str,
    initialization_options: Option<Value>,
) -> Value {
    let mut params = json!({
        "processId":null,
        "rootUri": root_uri.clone(),
        "capabilities":{
            "workspace":{
                "workspaceEdit":{
                    "documentChanges":true,
                    "resourceOperations":["create","rename","delete"],
                    "failureHandling":"textOnlyTransactional"
                },
                "symbol":{"dynamicRegistration":false}
            },
            "textDocument":{
                "declaration":{"dynamicRegistration":false},
                "definition":{"dynamicRegistration":false},
                "implementation":{"dynamicRegistration":false},
                "typeDefinition":{"dynamicRegistration":false},
                "references":{"dynamicRegistration":false},
                "rename":{"dynamicRegistration":false,"prepareSupport":true},
                "diagnostic":{"dynamicRegistration":false},
                "typeHierarchy":{"dynamicRegistration":false},
                "codeAction":{
                    "dynamicRegistration":false,
                    "codeActionLiteralSupport":{
                        "codeActionKind":{
                            "valueSet":["quickfix","refactor","refactor.extract","refactor.inline","refactor.rewrite"]
                        }
                    },
                    "resolveSupport":{"properties":["edit","command"]}
                }
            },
            // P1.1: rust-analyzer only emits `experimental/serverStatus`
            // (the quiescence/readiness signal) when the client
            // advertises support for it. Servers that don't know the
            // extension ignore it.
            "experimental":{"serverStatusNotification":true}
        },
        "workspaceFolders":[
            {
                "uri": root_uri,
                "name": workspace_name
            }
        ]
    });
    if let Some(options) = initialization_options {
        params["initializationOptions"] = options;
    }
    params
}

fn configured_startup_grace() -> Option<Duration> {
    let millis = std::env::var("CODELENS_LSP_STARTUP_GRACE_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
        .min(10_000);
    (millis > 0).then(|| Duration::from_millis(millis))
}

impl Drop for LspSession {
    fn drop(&mut self) {
        let _ = self.shutdown();
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_status)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod warm_probe_tests {
    use super::*;

    fn temp_project() -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-warmprobe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        ProjectRoot::new(dir.to_str().unwrap()).unwrap()
    }

    #[test]
    fn has_warm_session_is_false_and_non_spawning_when_cold() {
        let pool = LspSessionPool::new(temp_project());
        // A cold language must probe `false` — no session was ever started.
        assert!(!pool.has_warm_session("pyright-langserver", &["--stdio".to_owned()]));
        // ...and the probe must not have spawned anything: the pool stays
        // empty, preserving the default path's no-cold-start latency contract.
        assert_eq!(pool.session_count(), 0);
    }

    #[test]
    fn warm_session_quiescence_is_none_and_non_spawning_when_cold() {
        let pool = LspSessionPool::new(temp_project());
        assert_eq!(
            pool.warm_session_quiescence("pyright-langserver", &["--stdio".to_owned()]),
            None,
            "no live session must report outer None (not 'unknown readiness')"
        );
        assert_eq!(pool.session_count(), 0, "readiness probe must not spawn");
    }
}

#[cfg(test)]
mod initialization_options_tests {
    use super::*;

    #[test]
    fn rust_analyzer_disables_check_on_save() {
        assert_eq!(
            initialization_options_for_command("rust-analyzer"),
            Some(json!({"checkOnSave": false}))
        );
    }

    #[test]
    fn path_qualified_rust_analyzer_hits_the_same_entry() {
        // Matching mirrors `is_allowed_lsp_command`: basename, not raw string.
        assert_eq!(
            initialization_options_for_command("/opt/homebrew/bin/rust-analyzer"),
            Some(json!({"checkOnSave": false}))
        );
    }

    #[test]
    fn unknown_servers_get_none() {
        for command in [
            "pyright-langserver",
            "typescript-language-server",
            "gopls",
            "clangd",
            "not-an-lsp",
        ] {
            assert_eq!(
                initialization_options_for_command(command),
                None,
                "{command} must not receive initializationOptions"
            );
        }
    }

    #[test]
    fn initialize_params_omits_options_field_when_none() {
        let params = initialize_params(Some("file:///tmp/proj/".to_owned()), "proj", None);
        assert!(
            params.get("initializationOptions").is_none(),
            "None must omit the field entirely (no null/empty placeholder)"
        );
    }

    #[test]
    fn initialize_params_attaches_options_when_some() {
        let options = json!({"checkOnSave": false});
        let params = initialize_params(
            Some("file:///tmp/proj/".to_owned()),
            "proj",
            Some(options.clone()),
        );
        assert_eq!(params.get("initializationOptions"), Some(&options));
    }

    #[test]
    fn options_do_not_alter_the_rest_of_the_params() {
        // Capabilities/rootUri/workspaceFolders must be invariant across
        // servers — the table only ever adds the one extra field.
        let root_uri = Some("file:///tmp/proj/".to_owned());
        let without = initialize_params(root_uri.clone(), "proj", None);
        let mut with = initialize_params(root_uri, "proj", Some(json!({"checkOnSave": false})));
        assert!(
            with.as_object_mut()
                .expect("params is an object")
                .remove("initializationOptions")
                .is_some()
        );
        assert_eq!(with, without);
    }
}

#[cfg(test)]
mod server_request_reply_tests {
    use super::*;

    #[test]
    fn workspace_configuration_returns_one_null_per_item() {
        // "Use your defaults" — deterministic and unblocks servers that
        // wait on the reply (the pre-P1.1 discard path stalled them).
        let params = json!({"items": [{"section": "rust-analyzer"}, {"section": "python"}]});
        let reply = server_request_reply_payload("workspace/configuration", Some(&params));
        assert_eq!(reply, Ok(json!([null, null])));
    }

    #[test]
    fn workspace_configuration_with_no_items_returns_empty_array() {
        let reply = server_request_reply_payload("workspace/configuration", None);
        assert_eq!(reply, Ok(json!([])));
    }

    #[test]
    fn capability_registration_and_progress_create_are_acknowledged() {
        for method in [
            "client/registerCapability",
            "client/unregisterCapability",
            "window/workDoneProgress/create",
        ] {
            assert_eq!(
                server_request_reply_payload(method, None),
                Ok(Value::Null),
                "{method} must be acknowledged, not discarded"
            );
        }
    }

    #[test]
    fn server_initiated_apply_edit_is_refused() {
        // Read sessions must never let a server mutate the workspace behind
        // the caller's back — mutations flow through the verifier-gated
        // edit transaction only.
        let reply = server_request_reply_payload("workspace/applyEdit", Some(&json!({"edit": {}})))
            .expect("applyEdit is answered, not errored");
        assert_eq!(reply.get("applied"), Some(&json!(false)));
    }

    #[test]
    fn unknown_server_request_gets_method_not_found() {
        let err = server_request_reply_payload("window/showMessageRequest", None)
            .expect_err("unknown requests must be rejected explicitly");
        assert_eq!(err.get("code"), Some(&json!(-32601)));
    }

    /// Live proof that the quiescence signal is actually received (P1.1):
    /// spawns a real rust-analyzer against a tiny fixture crate, issues
    /// reference requests (whose read loops harvest `experimental/serverStatus`
    /// notifications), and asserts the pool eventually reports
    /// `Some(Some(true))`. Requires rust-analyzer on PATH and several seconds
    /// of indexing — run manually:
    /// `cargo test -p codelens-engine --lib quiescence_signal -- --ignored`
    #[test]
    #[ignore = "spawns a live rust-analyzer; run manually"]
    fn quiescence_signal_is_harvested_from_live_rust_analyzer() {
        use crate::lsp::types::LspRequest;

        let dir = std::env::temp_dir().join(format!(
            "codelens-quiescence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"quiescence_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn target() -> u32 { 41 }\npub fn caller() -> u32 { target() + 1 }\n",
        )
        .unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        let pool = LspSessionPool::new(project);

        let request = LspRequest {
            command: "rust-analyzer".to_owned(),
            args: Vec::new(),
            file_path: "src/lib.rs".to_owned(),
            line: 1,
            column: 8,
            max_results: 10,
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let mut quiescence = None;
        while std::time::Instant::now() < deadline {
            // Each request's read loop drains pending server notifications,
            // harvesting the latest serverStatus before returning.
            let _ = pool.find_referencing_symbols(&request);
            quiescence = pool.warm_session_quiescence("rust-analyzer", &[]);
            if quiescence == Some(Some(true)) {
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
        assert_eq!(
            quiescence,
            Some(Some(true)),
            "rust-analyzer must report quiescent=true within 60s (signal harvested)"
        );
    }
}
