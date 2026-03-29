use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

#[derive(Debug, Clone)]
pub struct LspRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub max_results: usize,
}

#[derive(Debug, Clone)]
pub struct LspDiagnosticRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub max_results: usize,
}

#[derive(Debug, Clone)]
pub struct LspWorkspaceSymbolRequest {
    pub command: String,
    pub args: Vec<String>,
    pub query: String,
    pub max_results: usize,
}

#[derive(Debug, Clone)]
pub struct LspTypeHierarchyRequest {
    pub command: String,
    pub args: Vec<String>,
    pub query: String,
    pub relative_path: Option<String>,
    pub hierarchy_type: String,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct LspRenamePlanRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub severity: Option<u8>,
    pub severity_label: Option<String>,
    pub code: Option<String>,
    pub source: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspWorkspaceSymbol {
    pub name: String,
    pub kind: Option<u32>,
    pub kind_label: Option<String>,
    pub container_name: Option<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspTypeHierarchyNode {
    pub name: String,
    pub fully_qualified_name: String,
    pub kind: String,
    pub members: HashMap<String, Vec<String>>,
    pub type_parameters: Vec<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supertypes: Vec<LspTypeHierarchyNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subtypes: Vec<LspTypeHierarchyNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRenamePlan {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub current_name: String,
    pub placeholder: Option<String>,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionKey {
    command: String,
    args: Vec<String>,
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

struct LspSession {
    project: ProjectRoot,
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_request_id: u64,
    documents: HashMap<String, OpenDocumentState>,
    #[allow(dead_code)] // retained for future stderr diagnostics
    stderr_buffer: std::sync::Arc<std::sync::Mutex<String>>,
}

pub fn find_referencing_symbols_via_lsp(
    project: &ProjectRoot,
    request: &LspRequest,
) -> Result<Vec<LspReference>> {
    let pool = LspSessionPool::new(project.clone());
    pool.find_referencing_symbols(request)
}

pub fn get_diagnostics_via_lsp(
    project: &ProjectRoot,
    request: &LspDiagnosticRequest,
) -> Result<Vec<LspDiagnostic>> {
    let pool = LspSessionPool::new(project.clone());
    pool.get_diagnostics(request)
}

pub fn search_workspace_symbols_via_lsp(
    project: &ProjectRoot,
    request: &LspWorkspaceSymbolRequest,
) -> Result<Vec<LspWorkspaceSymbol>> {
    let pool = LspSessionPool::new(project.clone());
    pool.search_workspace_symbols(request)
}

pub fn get_type_hierarchy_via_lsp(
    project: &ProjectRoot,
    request: &LspTypeHierarchyRequest,
) -> Result<HashMap<String, Value>> {
    let pool = LspSessionPool::new(project.clone());
    pool.get_type_hierarchy(request)
}

pub fn get_rename_plan_via_lsp(
    project: &ProjectRoot,
    request: &LspRenamePlanRequest,
) -> Result<LspRenamePlan> {
    let pool = LspSessionPool::new(project.clone());
    pool.get_rename_plan(request)
}

/// Known-safe LSP server binaries. Commands not in this list are rejected.
fn is_allowed_lsp_command(command: &str) -> bool {
    // Extract the binary name from the command path (e.g., "/usr/bin/pyright-langserver" → "pyright-langserver")
    let binary = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);

    let allowed: &[&str] = &[
        // From LSP_RECIPES
        "pyright-langserver", "typescript-language-server", "rust-analyzer",
        "gopls", "jdtls", "kotlin-language-server", "clangd",
        "solargraph", "intelephense", "sourcekit-lsp", "csharp-ls", "dart",
        // Additional well-known LSP servers
        "metals", "lua-language-server", "terraform-ls", "yaml-language-server",
        // Test support: allow python3/python for mock LSP in tests
        "python3", "python",
    ];
    allowed.iter().any(|&a| a == binary)
}

fn ensure_session<'a>(
    sessions: &'a mut HashMap<SessionKey, LspSession>,
    project: &ProjectRoot,
    command: &str,
    args: &[String],
) -> Result<&'a mut LspSession> {
    if !is_allowed_lsp_command(command) {
        bail!("Blocked: '{command}' is not a known LSP server. Only whitelisted LSP binaries are allowed.");
    }

    let key = SessionKey {
        command: command.to_owned(),
        args: args.to_owned(),
    };

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

impl LspSessionPool {
    pub fn new(project: ProjectRoot) -> Self {
        Self {
            project,
            sessions: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn session_count(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len()
    }

    pub fn find_referencing_symbols(&self, request: &LspRequest) -> Result<Vec<LspReference>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session =
            ensure_session(&mut sessions, &self.project, &request.command, &request.args)?;
        session.find_references(request)
    }

    pub fn get_diagnostics(
        &self,
        request: &LspDiagnosticRequest,
    ) -> Result<Vec<LspDiagnostic>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session =
            ensure_session(&mut sessions, &self.project, &request.command, &request.args)?;
        session.get_diagnostics(request)
    }

    pub fn search_workspace_symbols(
        &self,
        request: &LspWorkspaceSymbolRequest,
    ) -> Result<Vec<LspWorkspaceSymbol>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session =
            ensure_session(&mut sessions, &self.project, &request.command, &request.args)?;
        session.search_workspace_symbols(request)
    }

    pub fn get_type_hierarchy(
        &self,
        request: &LspTypeHierarchyRequest,
    ) -> Result<HashMap<String, Value>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session =
            ensure_session(&mut sessions, &self.project, &request.command, &request.args)?;
        session.get_type_hierarchy(request)
    }

    pub fn get_rename_plan(&self, request: &LspRenamePlanRequest) -> Result<LspRenamePlan> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session =
            ensure_session(&mut sessions, &self.project, &request.command, &request.args)?;
        session.get_rename_plan(request)
    }
}

impl LspSession {
    fn start(project: &ProjectRoot, command: &str, args: &[String]) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn LSP server {}", command))?;

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
                "capabilities":{},
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

    fn find_references(&mut self, request: &LspRequest) -> Result<Vec<LspReference>> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, _source) = self.prepare_document(&absolute_path)?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/references",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":request.column.saturating_sub(1)},
                "context":{"includeDeclaration":true}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        references_from_response(&self.project, response, request.max_results)
    }

    fn get_diagnostics(&mut self, request: &LspDiagnosticRequest) -> Result<Vec<LspDiagnostic>> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, _source) = self.prepare_document(&absolute_path)?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/diagnostic",
            json!({
                "textDocument":{"uri":uri_string}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        diagnostics_from_response(&self.project, response, request.max_results)
    }

    fn search_workspace_symbols(
        &mut self,
        request: &LspWorkspaceSymbolRequest,
    ) -> Result<Vec<LspWorkspaceSymbol>> {
        let id = self.next_id();
        self.send_request(
            id,
            "workspace/symbol",
            json!({
                "query": request.query
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        workspace_symbols_from_response(&self.project, response, request.max_results)
    }

    fn get_type_hierarchy(
        &mut self,
        request: &LspTypeHierarchyRequest,
    ) -> Result<HashMap<String, Value>> {
        let workspace_symbols = self.search_workspace_symbols(&LspWorkspaceSymbolRequest {
            command: request.command.clone(),
            args: request.args.clone(),
            query: request.query.clone(),
            max_results: 20,
        })?;
        let seed = workspace_symbols
            .into_iter()
            .find(|symbol| match &request.relative_path {
                Some(path) => &symbol.file_path == path,
                None => true,
            })
            .with_context(|| format!("No workspace symbol found for '{}'", request.query))?;

        let absolute_path = self.project.resolve(&seed.file_path)?;
        let (uri_string, _source) = self.prepare_document(&absolute_path)?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/prepareTypeHierarchy",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":seed.line.saturating_sub(1),"character":seed.column.saturating_sub(1)}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let items = response
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let root_item = items
            .into_iter()
            .next()
            .context("LSP prepareTypeHierarchy returned no items")?;

        let root = self.build_type_hierarchy_node(
            &root_item,
            request.depth,
            request.hierarchy_type.as_str(),
        )?;
        Ok(type_hierarchy_to_map(&root))
    }

    fn get_rename_plan(&mut self, request: &LspRenamePlanRequest) -> Result<LspRenamePlan> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/prepareRename",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":request.column.saturating_sub(1)}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        rename_plan_from_response(
            &self.project,
            &request.file_path,
            &source,
            response,
            request.new_name.clone(),
        )
    }

    fn build_type_hierarchy_node(
        &mut self,
        item: &Value,
        depth: usize,
        hierarchy_type: &str,
    ) -> Result<LspTypeHierarchyNode> {
        let mut node = type_hierarchy_node_from_item(item)?;

        if depth == 0 {
            return Ok(node);
        }

        let next_depth = depth.saturating_sub(1);
        if hierarchy_type == "super" || hierarchy_type == "both" {
            node.supertypes = self.fetch_type_hierarchy_branch(item, "supertypes", next_depth)?;
        }
        if hierarchy_type == "sub" || hierarchy_type == "both" {
            node.subtypes = self.fetch_type_hierarchy_branch(item, "subtypes", next_depth)?;
        }
        Ok(node)
    }

    fn fetch_type_hierarchy_branch(
        &mut self,
        item: &Value,
        method_suffix: &str,
        depth: usize,
    ) -> Result<Vec<LspTypeHierarchyNode>> {
        let id = self.next_id();
        self.send_request(
            id,
            &format!("typeHierarchy/{method_suffix}"),
            json!({
                "item": item
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let Some(items) = response.get("result").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };

        let mut nodes = Vec::new();
        for child in items {
            nodes.push(self.build_type_hierarchy_node(
                child,
                depth,
                method_suffix_to_hierarchy(method_suffix),
            )?);
        }
        Ok(nodes)
    }

    fn prepare_document(&mut self, absolute_path: &Path) -> Result<(String, String)> {
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

    fn sync_document(&mut self, uri: &str, language_id: &str, source: &str) -> Result<()> {
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

    fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    fn send_request(&mut self, id: u64, method: &str, params: Value) -> Result<()> {
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

    fn read_response_for_id(&mut self, expected_id: u64) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut discarded = 0u32;
        const MAX_DISCARDED: u32 = 500;

        loop {
            if Instant::now() > deadline {
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

fn references_from_response(
    project: &ProjectRoot,
    response: Value,
    max_results: usize,
) -> Result<Vec<LspReference>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    let Some(items) = result.as_array() else {
        return Ok(Vec::new());
    };

    let mut references = Vec::new();
    for item in items.iter().take(max_results) {
        let Some(uri) = item.get("uri").and_then(Value::as_str) else {
            continue;
        };
        let Ok(uri) = Url::parse(uri) else {
            continue;
        };
        let Ok(path) = uri.to_file_path() else {
            continue;
        };
        let Some(range) = item.get("range") else {
            continue;
        };
        let Some(start) = range.get("start") else {
            continue;
        };
        let Some(end) = range.get("end") else {
            continue;
        };
        references.push(LspReference {
            file_path: project.to_relative(path),
            line: start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            column: start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_line: end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_column: end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
        });
    }

    Ok(references)
}

fn diagnostics_from_response(
    project: &ProjectRoot,
    response: Value,
    max_results: usize,
) -> Result<Vec<LspDiagnostic>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };

    let Some(items) = result.get("items").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    let file_path = response
        .get("result")
        .and_then(|value| value.get("uri"))
        .and_then(Value::as_str)
        .and_then(|uri| Url::parse(uri).ok())
        .and_then(|uri| uri.to_file_path().ok())
        .map(|path| project.to_relative(path));

    let mut diagnostics = Vec::new();
    for item in items.iter().take(max_results) {
        let Some(range) = item.get("range") else {
            continue;
        };
        let Some(start) = range.get("start") else {
            continue;
        };
        let Some(end) = range.get("end") else {
            continue;
        };
        diagnostics.push(LspDiagnostic {
            file_path: file_path.clone().unwrap_or_default(),
            line: start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            column: start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_line: end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_column: end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            severity: item
                .get("severity")
                .and_then(Value::as_u64)
                .map(|value| value as u8),
            severity_label: item
                .get("severity")
                .and_then(Value::as_u64)
                .map(severity_label),
            code: item.get("code").map(code_to_string),
            source: item
                .get("source")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            message: item
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }

    Ok(diagnostics)
}

fn workspace_symbols_from_response(
    project: &ProjectRoot,
    response: Value,
    max_results: usize,
) -> Result<Vec<LspWorkspaceSymbol>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    let Some(items) = result.as_array() else {
        return Ok(Vec::new());
    };

    let mut symbols = Vec::new();
    for item in items.iter().take(max_results) {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some((file_path, line, column, end_line, end_column)) =
            workspace_symbol_location(project, item)
        else {
            continue;
        };
        symbols.push(LspWorkspaceSymbol {
            name: name.to_owned(),
            kind: item.get("kind").and_then(Value::as_u64).map(|v| v as u32),
            kind_label: item
                .get("kind")
                .and_then(Value::as_u64)
                .map(symbol_kind_label),
            container_name: item
                .get("containerName")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            file_path,
            line,
            column,
            end_line,
            end_column,
        });
    }

    Ok(symbols)
}

fn type_hierarchy_node_from_item(item: &Value) -> Result<LspTypeHierarchyNode> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .context("type hierarchy item missing name")?;
    let detail = item
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or(name)
        .to_owned();
    let kind = item
        .get("kind")
        .and_then(Value::as_u64)
        .map(symbol_kind_label)
        .unwrap_or_else(|| "unknown".to_owned());
    Ok(LspTypeHierarchyNode {
        name: name.to_owned(),
        fully_qualified_name: detail,
        kind,
        members: HashMap::from([
            ("methods".to_owned(), Vec::new()),
            ("fields".to_owned(), Vec::new()),
            ("properties".to_owned(), Vec::new()),
        ]),
        type_parameters: Vec::new(),
        supertypes: Vec::new(),
        subtypes: Vec::new(),
    })
}

fn type_hierarchy_to_map(node: &LspTypeHierarchyNode) -> HashMap<String, Value> {
    let mut result = HashMap::from([
        ("class_name".to_owned(), Value::String(node.name.clone())),
        (
            "fully_qualified_name".to_owned(),
            Value::String(node.fully_qualified_name.clone()),
        ),
        ("kind".to_owned(), Value::String(node.kind.clone())),
        (
            "members".to_owned(),
            serde_json::to_value(&node.members).unwrap_or_else(|_| json!({})),
        ),
        (
            "type_parameters".to_owned(),
            serde_json::to_value(&node.type_parameters).unwrap_or_else(|_| json!([])),
        ),
    ]);
    if !node.supertypes.is_empty() {
        result.insert(
            "supertypes".to_owned(),
            serde_json::to_value(
                node.supertypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    if !node.subtypes.is_empty() {
        result.insert(
            "subtypes".to_owned(),
            serde_json::to_value(
                node.subtypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    result
}

fn rename_plan_from_response(
    project: &ProjectRoot,
    request_file_path: &str,
    source: &str,
    response: Value,
    new_name: Option<String>,
) -> Result<LspRenamePlan> {
    let Some(result) = response.get("result") else {
        bail!("LSP prepareRename returned no result");
    };

    let (file_path, start, end, placeholder) = if let Some(range) = result.get("range") {
        let file_path = result
            .get("textDocument")
            .and_then(|value| value.get("uri"))
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok())
            .and_then(|uri| uri.to_file_path().ok())
            .map(|path| project.to_relative(path))
            .unwrap_or_else(|| request_file_path.to_owned());
        (
            file_path,
            range
                .get("start")
                .cloned()
                .context("prepareRename missing start")?,
            range
                .get("end")
                .cloned()
                .context("prepareRename missing end")?,
            result
                .get("placeholder")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        )
    } else {
        (
            request_file_path.to_owned(),
            result
                .get("start")
                .cloned()
                .context("prepareRename missing start")?,
            result
                .get("end")
                .cloned()
                .context("prepareRename missing end")?,
            None,
        )
    };

    let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let column = start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let end_line = end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let end_column = end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let current_name = placeholder
        .clone()
        .unwrap_or_else(|| extract_text_for_range(source, line, column, end_line, end_column));

    Ok(LspRenamePlan {
        file_path,
        line,
        column,
        end_line,
        end_column,
        current_name,
        placeholder,
        new_name,
    })
}

fn extract_text_for_range(
    source: &str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if line == 0 || end_line == 0 || line > lines.len() || end_line > lines.len() {
        return String::new();
    }
    if line == end_line {
        let text = lines[line - 1];
        let start = column.saturating_sub(1).min(text.len());
        let end = end_column.saturating_sub(1).min(text.len());
        return text.get(start..end).unwrap_or_default().to_owned();
    }

    let mut result = String::new();
    for index in line..=end_line {
        let text = lines[index - 1];
        let slice = if index == line {
            let start = column.saturating_sub(1).min(text.len());
            text.get(start..).unwrap_or_default()
        } else if index == end_line {
            let end = end_column.saturating_sub(1).min(text.len());
            text.get(..end).unwrap_or_default()
        } else {
            text
        };
        result.push_str(slice);
        if index != end_line {
            result.push('\n');
        }
    }
    result
}

fn type_hierarchy_child_to_map(node: &LspTypeHierarchyNode) -> HashMap<String, Value> {
    let mut result = HashMap::from([
        ("name".to_owned(), Value::String(node.name.clone())),
        (
            "qualified_name".to_owned(),
            Value::String(node.fully_qualified_name.clone()),
        ),
        ("kind".to_owned(), Value::String(node.kind.clone())),
    ]);
    if !node.supertypes.is_empty() {
        result.insert(
            "supertypes".to_owned(),
            serde_json::to_value(
                node.supertypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    if !node.subtypes.is_empty() {
        result.insert(
            "subtypes".to_owned(),
            serde_json::to_value(
                node.subtypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    result
}

fn method_suffix_to_hierarchy(method_suffix: &str) -> &str {
    match method_suffix {
        "supertypes" => "super",
        "subtypes" => "sub",
        _ => "both",
    }
}

fn workspace_symbol_location(
    project: &ProjectRoot,
    item: &Value,
) -> Option<(String, usize, usize, usize, usize)> {
    let location = item.get("location")?;

    if let Some(uri) = location.get("uri").and_then(Value::as_str) {
        let uri = Url::parse(uri).ok()?;
        let path = uri.to_file_path().ok()?;
        let range = location.get("range")?;
        let start = range.get("start")?;
        let end = range.get("end")?;
        return Some((
            project.to_relative(path),
            start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
        ));
    }

    if let Some(uri) = location
        .get("uri")
        .and_then(Value::as_str)
        .or_else(|| location.get("targetUri").and_then(Value::as_str))
    {
        let uri = Url::parse(uri).ok()?;
        let path = uri.to_file_path().ok()?;
        return Some((project.to_relative(path), 1, 1, 1, 1));
    }

    None
}

fn code_to_string(value: &Value) -> String {
    if let Some(code) = value.as_str() {
        return code.to_owned();
    }
    if let Some(code) = value.as_i64() {
        return code.to_string();
    }
    if let Some(code) = value.as_u64() {
        return code.to_string();
    }
    value.to_string()
}

fn severity_label(value: u64) -> String {
    match value {
        1 => "error",
        2 => "warning",
        3 => "information",
        4 => "hint",
        _ => "unknown",
    }
    .to_owned()
}

fn symbol_kind_label(value: u64) -> String {
    match value {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum_member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type_parameter",
        _ => "unknown",
    }
    .to_owned()
}

fn language_id_for_path(path: &Path) -> Result<&'static str> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    crate::lang_registry::language_id(&extension)
        .ok_or_else(|| anyhow::anyhow!("unsupported LSP language for extension: {extension}"))
}

fn send_message(writer: &mut impl Write, payload: &Value) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

fn read_message(reader: &mut BufReader<impl Read>) -> Result<Value> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        let bytes = reader.read_line(&mut header)?;
        if bytes == 0 {
            bail!("unexpected EOF while reading LSP headers");
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }

    let length = content_length.context("missing Content-Length header")?;
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).context("failed to decode LSP body")
}

#[derive(Debug, Clone, Serialize)]
pub struct LspRecipe {
    pub language: &'static str,
    pub extensions: &'static [&'static str],
    pub server_name: &'static str,
    pub install_command: &'static str,
    pub binary_name: &'static str,
    pub args: &'static [&'static str],
    pub package_manager: &'static str,
}

pub const LSP_RECIPES: &[LspRecipe] = &[
    LspRecipe {
        language: "python",
        extensions: &["py"],
        server_name: "pyright",
        install_command: "npm install -g pyright",
        binary_name: "pyright-langserver",
        args: &["--stdio"],
        package_manager: "npm",
    },
    LspRecipe {
        language: "typescript",
        extensions: &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
        server_name: "typescript-language-server",
        install_command: "npm install -g typescript-language-server typescript",
        binary_name: "typescript-language-server",
        args: &["--stdio"],
        package_manager: "npm",
    },
    LspRecipe {
        language: "rust",
        extensions: &["rs"],
        server_name: "rust-analyzer",
        install_command: "rustup component add rust-analyzer",
        binary_name: "rust-analyzer",
        args: &[],
        package_manager: "rustup",
    },
    LspRecipe {
        language: "go",
        extensions: &["go"],
        server_name: "gopls",
        install_command: "go install golang.org/x/tools/gopls@latest",
        binary_name: "gopls",
        args: &["serve"],
        package_manager: "go",
    },
    LspRecipe {
        language: "java",
        extensions: &["java"],
        server_name: "jdtls",
        install_command: "brew install jdtls",
        binary_name: "jdtls",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "kotlin",
        extensions: &["kt", "kts"],
        server_name: "kotlin-language-server",
        install_command: "brew install kotlin-language-server",
        binary_name: "kotlin-language-server",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "c_cpp",
        extensions: &["c", "h", "cpp", "cc", "cxx", "hpp"],
        server_name: "clangd",
        install_command: "brew install llvm",
        binary_name: "clangd",
        args: &[],
        package_manager: "brew",
    },
    LspRecipe {
        language: "ruby",
        extensions: &["rb"],
        server_name: "solargraph",
        install_command: "gem install solargraph",
        binary_name: "solargraph",
        args: &["stdio"],
        package_manager: "gem",
    },
    LspRecipe {
        language: "php",
        extensions: &["php"],
        server_name: "intelephense",
        install_command: "npm install -g intelephense",
        binary_name: "intelephense",
        args: &["--stdio"],
        package_manager: "npm",
    },
    LspRecipe {
        language: "swift",
        extensions: &["swift"],
        server_name: "sourcekit-lsp",
        install_command: "xcode-select --install",
        binary_name: "sourcekit-lsp",
        args: &[],
        package_manager: "xcode",
    },
    LspRecipe {
        language: "csharp",
        extensions: &["cs"],
        server_name: "omnisharp",
        install_command: "dotnet tool install -g csharp-ls",
        binary_name: "csharp-ls",
        args: &[],
        package_manager: "dotnet",
    },
    LspRecipe {
        language: "dart",
        extensions: &["dart"],
        server_name: "dart-language-server",
        install_command: "dart pub global activate dart_language_server",
        binary_name: "dart",
        args: &["language-server", "--protocol=lsp"],
        package_manager: "dart",
    },
];

/// Check which LSP servers are installed and which are missing.
pub fn check_lsp_status() -> Vec<LspStatus> {
    LSP_RECIPES
        .iter()
        .map(|recipe| {
            let installed = std::process::Command::new("which")
                .arg(recipe.binary_name)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            LspStatus {
                language: recipe.language,
                server_name: recipe.server_name,
                installed,
                install_command: recipe.install_command,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct LspStatus {
    pub language: &'static str,
    pub server_name: &'static str,
    pub installed: bool,
    pub install_command: &'static str,
}

/// Get the recipe for a file extension.
pub fn get_lsp_recipe(extension: &str) -> Option<&'static LspRecipe> {
    let ext = extension.to_ascii_lowercase();
    LSP_RECIPES
        .iter()
        .find(|r| r.extensions.contains(&ext.as_str()))
}

#[cfg(test)]
mod tests {
    use super::{
        LspDiagnosticRequest, LspRenamePlanRequest, LspRequest, LspSessionPool,
        LspTypeHierarchyRequest, LspWorkspaceSymbolRequest, find_referencing_symbols_via_lsp,
        get_diagnostics_via_lsp, get_rename_plan_via_lsp, get_type_hierarchy_via_lsp,
        search_workspace_symbols_via_lsp,
    };
    use crate::ProjectRoot;
    use serde_json::Value;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn reads_references_from_mock_lsp() {
        let dir = temp_dir("codelens-lsp-test");
        let project = ProjectRoot::new(&dir).expect("project");
        fs::write(dir.join("sample.py"), "def greet():\n    return 1\n").expect("write sample");
        let server_path = dir.join("mock_lsp.py");
        fs::write(&server_path, mock_server_script()).expect("write mock server");
        chmod_exec(&server_path);

        let refs = find_referencing_symbols_via_lsp(
            &project,
            &LspRequest {
                command: "python3".to_owned(),
                args: vec![
                    server_path.display().to_string(),
                    dir.join("count.txt").display().to_string(),
                ],
                file_path: "sample.py".to_owned(),
                line: 1,
                column: 5,
                max_results: 10,
            },
        )
        .expect("lsp references");

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].file_path, "sample.py");
        assert_eq!(refs[0].line, 1);
        assert_eq!(refs[0].column, 5);
    }

    #[test]
    fn reuses_pooled_session() {
        let dir = temp_dir("codelens-lsp-pool");
        let project = ProjectRoot::new(&dir).expect("project");
        fs::write(dir.join("sample.py"), "def greet():\n    return 1\n").expect("write sample");
        let server_path = dir.join("mock_lsp.py");
        let count_path = dir.join("count.txt");
        fs::write(&server_path, mock_server_script()).expect("write mock server");
        chmod_exec(&server_path);

        let pool = LspSessionPool::new(project.clone());
        let request = LspRequest {
            command: "python3".to_owned(),
            args: vec![
                server_path.display().to_string(),
                count_path.display().to_string(),
            ],
            file_path: "sample.py".to_owned(),
            line: 1,
            column: 5,
            max_results: 10,
        };

        let refs1 = pool.find_referencing_symbols(&request).expect("refs1");
        let refs2 = pool.find_referencing_symbols(&request).expect("refs2");
        assert_eq!(refs1.len(), 1);
        assert_eq!(refs2.len(), 1);
        assert_eq!(pool.session_count(), 1);

        drop(pool);

        let initialize_count = fs::read_to_string(&count_path)
            .expect("count file")
            .trim()
            .parse::<usize>()
            .expect("count");
        assert_eq!(initialize_count, 1);
    }

    #[test]
    fn reads_diagnostics_from_mock_lsp() {
        let dir = temp_dir("codelens-lsp-diagnostics");
        let project = ProjectRoot::new(&dir).expect("project");
        fs::write(dir.join("sample.py"), "def greet(:\n    return 1\n").expect("write sample");
        let server_path = dir.join("mock_lsp.py");
        fs::write(&server_path, mock_server_script()).expect("write mock server");
        chmod_exec(&server_path);

        let diagnostics = get_diagnostics_via_lsp(
            &project,
            &LspDiagnosticRequest {
                command: "python3".to_owned(),
                args: vec![server_path.display().to_string()],
                file_path: "sample.py".to_owned(),
                max_results: 10,
            },
        )
        .expect("lsp diagnostics");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].file_path, "sample.py");
        assert_eq!(diagnostics[0].severity_label.as_deref(), Some("error"));
        assert!(diagnostics[0].message.contains("syntax"));
    }

    #[test]
    fn reads_workspace_symbols_from_mock_lsp() {
        let dir = temp_dir("codelens-lsp-workspace-symbols");
        let project = ProjectRoot::new(&dir).expect("project");
        fs::write(dir.join("sample.py"), "class Service:\n    pass\n").expect("write sample");
        let server_path = dir.join("mock_lsp.py");
        fs::write(&server_path, mock_server_script()).expect("write mock server");
        chmod_exec(&server_path);

        let symbols = search_workspace_symbols_via_lsp(
            &project,
            &LspWorkspaceSymbolRequest {
                command: "python3".to_owned(),
                args: vec![
                    server_path.display().to_string(),
                    dir.join("sample.py").display().to_string(),
                ],
                query: "Service".to_owned(),
                max_results: 10,
            },
        )
        .expect("workspace symbols");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Service");
        assert_eq!(symbols[0].kind_label.as_deref(), Some("class"));
        assert_eq!(symbols[0].file_path, "sample.py");
    }

    #[test]
    fn reads_type_hierarchy_from_mock_lsp() {
        let dir = temp_dir("codelens-lsp-type-hierarchy");
        let project = ProjectRoot::new(&dir).expect("project");
        fs::write(dir.join("sample.py"), "class Service:\n    pass\n").expect("write sample");
        let server_path = dir.join("mock_lsp.py");
        fs::write(&server_path, mock_server_script()).expect("write mock server");
        chmod_exec(&server_path);

        let hierarchy = get_type_hierarchy_via_lsp(
            &project,
            &LspTypeHierarchyRequest {
                command: "python3".to_owned(),
                args: vec![
                    server_path.display().to_string(),
                    dir.join("sample.py").display().to_string(),
                ],
                query: "Service".to_owned(),
                relative_path: Some("sample.py".to_owned()),
                hierarchy_type: "both".to_owned(),
                depth: 1,
            },
        )
        .expect("type hierarchy");

        assert_eq!(
            hierarchy.get("class_name"),
            Some(&Value::String("Service".to_owned()))
        );
        assert_eq!(
            hierarchy.get("fully_qualified_name"),
            Some(&Value::String("sample.Service".to_owned()))
        );
        assert!(
            hierarchy
                .get("supertypes")
                .and_then(Value::as_array)
                .is_some_and(|items: &Vec<Value>| !items.is_empty())
        );
        assert!(
            hierarchy
                .get("subtypes")
                .and_then(Value::as_array)
                .is_some_and(|items: &Vec<Value>| !items.is_empty())
        );
    }

    #[test]
    fn reads_rename_plan_from_mock_lsp() {
        let dir = temp_dir("codelens-lsp-rename-plan");
        let project = ProjectRoot::new(&dir).expect("project");
        fs::write(dir.join("sample.py"), "class Service:\n    pass\n").expect("write sample");
        let server_path = dir.join("mock_lsp.py");
        fs::write(&server_path, mock_server_script()).expect("write mock server");
        chmod_exec(&server_path);

        let plan = get_rename_plan_via_lsp(
            &project,
            &LspRenamePlanRequest {
                command: "python3".to_owned(),
                args: vec![server_path.display().to_string()],
                file_path: "sample.py".to_owned(),
                line: 1,
                column: 8,
                new_name: Some("RenamedService".to_owned()),
            },
        )
        .expect("rename plan");

        assert_eq!(plan.file_path, "sample.py");
        assert_eq!(plan.current_name, "Service");
        assert_eq!(plan.placeholder.as_deref(), Some("Service"));
        assert_eq!(plan.new_name.as_deref(), Some("RenamedService"));
    }

    fn chmod_exec(path: &std::path::Path) {
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create dir");
        dir
    }

    fn mock_server_script() -> &'static str {
        r#"#!/usr/bin/env python3
import json
import sys
from pathlib import Path

count_file = Path(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].endswith(".txt") else None
symbol_path = Path(sys.argv[1]) if len(sys.argv) > 1 and not sys.argv[1].endswith(".txt") else None
if len(sys.argv) > 2:
    symbol_path = Path(sys.argv[2])
initialize_count = 0

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))

def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        initialize_count += 1
        if count_file:
            count_file.write_text(str(initialize_count))
        send({"jsonrpc":"2.0","id":message["id"],"result":{"capabilities":{"referencesProvider": True}}})
    elif method == "textDocument/references":
        uri = message["params"]["textDocument"]["uri"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "uri": uri,
                    "range": {
                        "start": {"line": 0, "character": 4},
                        "end": {"line": 0, "character": 9}
                    }
                }
            ]
        })
    elif method == "textDocument/diagnostic":
        uri = message["params"]["textDocument"]["uri"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":{
                "kind":"full",
                "uri": uri,
                "items":[
                    {
                        "range":{
                            "start":{"line":0,"character":10},
                            "end":{"line":0,"character":11}
                        },
                        "severity":1,
                        "code":"E999",
                        "source":"mock-lsp",
                        "message":"syntax error"
                    }
                ]
            }
        })
    elif method == "workspace/symbol":
        query = message["params"]["query"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name": query,
                    "kind": 5,
                    "containerName": "sample",
                    "location": {
                        "uri": "file://" + str(symbol_path.resolve() if symbol_path else (Path.cwd() / "sample.py").resolve()),
                        "range": {
                            "start": {"line": 0, "character": 6},
                            "end": {"line": 0, "character": 13}
                        }
                    }
                }
            ]
        })
    elif method == "textDocument/prepareTypeHierarchy":
        uri = message["params"]["textDocument"]["uri"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name":"Service",
                    "kind":5,
                    "detail":"sample.Service",
                    "uri": uri,
                    "range":{
                        "start":{"line":0,"character":6},
                        "end":{"line":0,"character":13}
                    },
                    "selectionRange":{
                        "start":{"line":0,"character":6},
                        "end":{"line":0,"character":13}
                    },
                    "data":{"name":"Service"}
                }
            ]
        })
    elif method == "typeHierarchy/supertypes":
        item = message["params"]["item"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name":"BaseService",
                    "kind":5,
                    "detail":"sample.BaseService",
                    "uri": item["uri"],
                    "range": item["range"],
                    "selectionRange": item["selectionRange"],
                    "data":{"name":"BaseService"}
                }
            ]
        })
    elif method == "typeHierarchy/subtypes":
        item = message["params"]["item"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name":"ServiceImpl",
                    "kind":5,
                    "detail":"sample.ServiceImpl",
                    "uri": item["uri"],
                    "range": item["range"],
                    "selectionRange": item["selectionRange"],
                    "data":{"name":"ServiceImpl"}
                }
            ]
        })
    elif method == "textDocument/prepareRename":
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":{
                "range":{
                    "start":{"line":0,"character":6},
                    "end":{"line":0,"character":13}
                },
                "placeholder":"Service"
            }
        })
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":message["id"],"result":None})
    elif method == "exit":
        break
"#
    }
}
