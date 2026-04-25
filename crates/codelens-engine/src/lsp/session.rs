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

use super::code_actions::{code_actions_from_response, select_code_action};
use super::parsers::{
    apply_workspace_edit_transaction, diagnostics_from_response, method_suffix_to_hierarchy,
    references_from_response, rename_plan_from_response, resolved_targets_from_response,
    type_hierarchy_node_from_item, type_hierarchy_to_map, utf16_character_for_byte_column,
    workspace_edit_transaction_from_edit, workspace_edit_transaction_from_response,
    workspace_symbols_from_response,
};
use super::protocol::{language_id_for_path, poll_readable, read_message, send_message};
use super::registry::resolve_lsp_binary;
use super::types::{
    LspCodeActionRefactorResult, LspCodeActionRequest, LspDiagnostic, LspDiagnosticRequest,
    LspReference, LspRenamePlan, LspRenamePlanRequest, LspRenameRequest, LspRequest,
    LspResolveTargetRequest, LspResolvedTarget, LspTypeHierarchyNode, LspTypeHierarchyRequest,
    LspWorkspaceSymbol, LspWorkspaceSymbolRequest,
};

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

/// Known-safe LSP server binaries. Commands not in this list are rejected.
pub(super) fn is_allowed_lsp_command(command: &str) -> bool {
    // Extract the binary name from the command path (e.g., "/usr/bin/pyright-langserver" → "pyright-langserver")
    let binary = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);

    ALLOWED_COMMANDS.contains(&binary)
}

pub(super) const ALLOWED_COMMANDS: &[&str] = &[
    // From LSP_RECIPES
    "pyright-langserver",
    "typescript-language-server",
    "rust-analyzer",
    "gopls",
    "jdtls",
    "kotlin-language-server",
    "clangd",
    "solargraph",
    "intelephense",
    "sourcekit-lsp",
    "csharp-ls",
    "dart",
    // Additional well-known LSP servers
    "metals",
    "lua-language-server",
    "terraform-ls",
    "yaml-language-server",
    // Test support: allow python3/python for mock LSP in tests
    "python3",
    "python",
];

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

    /// Replace the project root and close all existing sessions.
    pub fn reset(&self, project: ProjectRoot) -> Self {
        // Drop existing sessions so LSP processes are killed.
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        Self::new(project)
    }

    pub fn session_count(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len()
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

    pub fn get_diagnostics(&self, request: &LspDiagnosticRequest) -> Result<Vec<LspDiagnostic>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = ensure_session(
            &mut sessions,
            &self.project,
            &request.command,
            &request.args,
        )?;
        session.get_diagnostics(request)
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
}

impl LspSession {
    fn start(project: &ProjectRoot, command: &str, args: &[String]) -> Result<Self> {
        let command_path = resolve_lsp_binary(command).unwrap_or_else(|| command.into());
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

    fn find_references(&mut self, request: &LspRequest) -> Result<Vec<LspReference>> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/references",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character},
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

    fn resolve_symbol_target(
        &mut self,
        request: &LspResolveTargetRequest,
    ) -> Result<Vec<LspResolvedTarget>> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);
        let method = match request.target.as_str() {
            "declaration" => "textDocument/declaration",
            "definition" => "textDocument/definition",
            "implementation" => "textDocument/implementation",
            "type_definition" => "textDocument/typeDefinition",
            other => bail!("unsupported LSP target: {other}"),
        };

        let id = self.next_id();
        self.send_request(
            id,
            method,
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        resolved_targets_from_response(
            &self.project,
            response,
            &request.target,
            method,
            request.max_results,
        )
    }

    fn get_rename_plan(&mut self, request: &LspRenamePlanRequest) -> Result<LspRenamePlan> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/prepareRename",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character}
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

    fn rename_symbol(&mut self, request: &LspRenameRequest) -> Result<crate::rename::RenameResult> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);
        let _plan = self.get_rename_plan(&LspRenamePlanRequest {
            command: request.command.clone(),
            args: request.args.clone(),
            file_path: request.file_path.clone(),
            line: request.line,
            column: request.column,
            new_name: Some(request.new_name.clone()),
        })?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/rename",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character},
                "newName": request.new_name.clone(),
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let transaction = workspace_edit_transaction_from_response(&self.project, response)?;
        let edits = transaction.edits.clone();
        let modified_files = transaction.modified_files;
        let total_replacements = transaction.edit_count;

        if !request.dry_run {
            apply_workspace_edit_transaction(&self.project, &transaction)?;
        }

        Ok(crate::rename::RenameResult {
            success: true,
            message: format!(
                "{} {} LSP replacement(s) in {} file(s)",
                if request.dry_run {
                    "Would make"
                } else {
                    "Made"
                },
                total_replacements,
                modified_files
            ),
            modified_files,
            total_replacements,
            edits,
        })
    }

    fn code_action_refactor(
        &mut self,
        request: &LspCodeActionRequest,
    ) -> Result<LspCodeActionRefactorResult> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let start_character =
            utf16_character_for_byte_column(&source, request.start_line, request.start_column);
        let end_character =
            utf16_character_for_byte_column(&source, request.end_line, request.end_column);

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/codeAction",
            json!({
                "textDocument":{"uri":uri_string},
                "range":{
                    "start":{
                        "line":request.start_line.saturating_sub(1),
                        "character":start_character
                    },
                    "end":{
                        "line":request.end_line.saturating_sub(1),
                        "character":end_character
                    }
                },
                "context":{
                    "diagnostics":[],
                    "only": request.only
                }
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let actions = code_actions_from_response(response, &request.only)?;
        let action = select_code_action(&actions, request.action_id.as_deref())?;
        if let Some(reason) = &action.disabled_reason {
            bail!("unsupported_semantic_refactor: selected LSP codeAction is disabled: {reason}");
        }

        let (edit, resolved_via) = if let Some(edit) = action.edit.clone() {
            (edit, "textDocument/codeAction".to_owned())
        } else {
            let id = self.next_id();
            self.send_request(id, "codeAction/resolve", action.raw.clone())?;
            let response = self.read_response_for_id(id)?;
            let Some(result) = response.get("result") else {
                bail!("unsupported_semantic_refactor: codeAction/resolve returned no result");
            };
            if let Some(edit) = result.get("edit").cloned() {
                (edit, "codeAction/resolve".to_owned())
            } else if action.command.is_some() || result.get("command").is_some() {
                bail!(
                    "unsupported_semantic_refactor: LSP codeAction returned command without inspectable WorkspaceEdit"
                );
            } else {
                bail!("unsupported_semantic_refactor: LSP codeAction returned no WorkspaceEdit");
            }
        };

        let transaction = workspace_edit_transaction_from_edit(&self.project, &edit)?;
        if transaction.edit_count == 0 && transaction.resource_ops.is_empty() {
            bail!("unsupported_semantic_refactor: LSP codeAction WorkspaceEdit is empty");
        }
        if !request.dry_run {
            apply_workspace_edit_transaction(&self.project, &transaction)?;
        }

        Ok(LspCodeActionRefactorResult {
            success: true,
            message: format!(
                "{} {} LSP codeAction edit(s) in {} file(s)",
                if request.dry_run {
                    "Would apply"
                } else {
                    "Applied"
                },
                transaction.edit_count,
                transaction.modified_files
            ),
            operation: request.operation.clone(),
            action_title: action.title,
            action_kind: action.kind,
            resolved_via,
            applied: !request.dry_run,
            transaction,
        })
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
