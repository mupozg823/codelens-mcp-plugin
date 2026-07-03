use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

use super::commands::is_allowed_lsp_command;
use super::protocol::{language_id_for_path, poll_readable, read_message, send_message};
use super::registry::resolve_lsp_binary_with_hint;
use super::types::{
    LspCodeActionRefactorPlan, LspCodeActionRefactorResult, LspCodeActionRequest, LspDiagnostic,
    LspDiagnosticRequest, LspReference, LspRenamePlan, LspRenamePlanRequest, LspRenameRequest,
    LspRequest, LspResolveTargetRequest, LspResolvedTarget, LspTypeHierarchyRequest,
    LspWorkspaceEditTransaction, LspWorkspaceSymbol, LspWorkspaceSymbolRequest,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionKey {
    command: String,
    args: Vec<String>,
}

impl SessionKey {
    fn new(command: &str, args: &[String]) -> Self {
        Self {
            command: command.to_owned(),
            args: args.to_owned(),
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
}

fn ensure_session<'a>(
    sessions: &'a mut HashMap<SessionKey, LspSession>,
    project: &ProjectRoot,
    command: &str,
    args: &[String],
) -> Result<&'a mut LspSession> {
    if !is_allowed_lsp_command(command) {
        bail!(
            "Blocked: '{command}' is not a known LSP server. Only whitelisted LSP binaries are allowed."
        );
    }

    let key = SessionKey::new(command, args);

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
            let session = LspSession::start(project, command, args)?;
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
            sessions: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Replace the project root and close all existing sessions.
    pub fn reset(&self, project: ProjectRoot) -> Self {
        // Drop existing sessions so LSP processes are killed.
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        Self::new(project)
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
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let key = SessionKey::new(command, args);
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

    pub fn find_referencing_symbols(&self, request: &LspRequest) -> Result<Vec<LspReference>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
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
        let key = SessionKey::new(&request.command, &request.args);
        // A live child for this key means `ensure_session` reuses it (no
        // spawn); its absence or a dead child means it will spawn below.
        let was_warm = sessions
            .get_mut(&key)
            .map(|session| matches!(session.child.try_wait(), Ok(None)))
            .unwrap_or(false);
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        let references = session.find_references(request)?;
        Ok((references, !was_warm))
    }

    pub fn get_diagnostics(&self, request: &LspDiagnosticRequest) -> Result<Vec<LspDiagnostic>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let result = {
            let session = ensure_session(
                &mut sessions,
                &self.project,
                &request.command,
                &request.args,
            )?;
            session.get_diagnostics(request)
        };

        match result {
            Ok(diagnostics) => Ok(diagnostics),
            Err(err) if is_retriable_lsp_transport_error(&err) => {
                let key = SessionKey::new(&request.command, &request.args);
                sessions.remove(&key);
                let session = ensure_session(
                    &mut sessions,
                    &self.project,
                    &request.command,
                    &request.args,
                )?;
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
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.search_workspace_symbols(request)
    }

    pub fn get_type_hierarchy(
        &self,
        request: &LspTypeHierarchyRequest,
    ) -> Result<HashMap<String, Value>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.get_type_hierarchy(request)
    }

    pub fn resolve_symbol_target(
        &self,
        request: &LspResolveTargetRequest,
    ) -> Result<Vec<LspResolvedTarget>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.resolve_symbol_target(request)
    }

    pub fn get_rename_plan(&self, request: &LspRenamePlanRequest) -> Result<LspRenamePlan> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.get_rename_plan(request)
    }

    pub fn rename_symbol(&self, request: &LspRenameRequest) -> Result<crate::rename::RenameResult> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.rename_symbol(request)
    }

    pub fn rename_symbol_transaction(
        &self,
        request: &LspRenameRequest,
    ) -> Result<LspWorkspaceEditTransaction> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.rename_symbol_transaction(request)
    }

    pub fn code_action_refactor(
        &self,
        request: &LspCodeActionRequest,
    ) -> Result<LspCodeActionRefactorResult> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.code_action_refactor(request)
    }

    pub fn code_action_refactor_plan(
        &self,
        request: &LspCodeActionRequest,
    ) -> Result<LspCodeActionRefactorPlan> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.code_action_refactor_plan(request)
    }
}

impl LspSession {
    fn start(project: &ProjectRoot, command: &str, args: &[String]) -> Result<Self> {
        let command_path = resolve_lsp_binary_with_hint(command, Some(project.as_path()))
            .unwrap_or_else(|| command.into());
        let mut child = Command::new(&command_path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn LSP server {}", command_path.display()))?;

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
        };
        session.initialize()?;
        if let Some(grace) = configured_startup_grace() {
            thread::sleep(grace);
        }
        Ok(session)
    }

    fn initialize(&mut self) -> Result<()> {
        let id = self.next_id();
        let root_uri = Url::from_directory_path(self.project.as_path())
            .ok()
            .map(|url| url.to_string());
        self.send_request(
            id,
            "initialize",
            json!({
                "processId":null,
                "rootUri": root_uri,
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
                    }
                },
                "workspaceFolders":[
                    {
                        "uri": Url::from_directory_path(self.project.as_path()).ok().map(|url| url.to_string()),
                        "name": self.project.as_path().file_name().and_then(|n| n.to_str()).unwrap_or("workspace")
                    }
                ]
            }),
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

    fn shutdown(&mut self) -> Result<()> {
        let id = self.next_id();
        self.send_request(id, "shutdown", Value::Null)?;
        let _ = self.read_response_for_id(id)?;
        self.send_notification("exit", Value::Null)
    }
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
}
