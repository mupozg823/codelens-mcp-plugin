use super::{AppState, ToolResult, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tools::semantic_edit::{
    SemanticEditBackendSelection, SemanticTransactionContractInput, semantic_transaction_contract,
};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub(crate) fn refactor_with_local_adapter(
    state: &AppState,
    arguments: &Value,
    backend: SemanticEditBackendSelection,
    tool: &'static str,
    operation: &'static str,
) -> ToolResult {
    let dry_run = arguments
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    invoke_workspace_edit_adapter(state, arguments, backend, tool, operation, dry_run)
}

pub(crate) fn rename_with_local_adapter(
    state: &AppState,
    arguments: &Value,
    backend: SemanticEditBackendSelection,
) -> ToolResult {
    let dry_run = arguments
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    invoke_workspace_edit_adapter(
        state,
        arguments,
        backend,
        "rename_symbol",
        "rename",
        dry_run,
    )
}

fn invoke_workspace_edit_adapter(
    state: &AppState,
    arguments: &Value,
    backend: SemanticEditBackendSelection,
    tool: &'static str,
    operation: &'static str,
    dry_run: bool,
) -> ToolResult {
    let backend_name = backend.adapter_name().ok_or_else(|| {
        CodeLensError::Validation("IDE adapter requested for non-adapter backend".into())
    })?;
    let config = AdapterConfig::from_env(backend_name)?;
    let response = run_adapter(
        &config,
        state,
        arguments,
        backend_name,
        tool,
        operation,
        dry_run,
    )?;
    let workspace_edit = response
        .get("workspace_edit")
        .or_else(|| response.get("workspaceEdit"))
        .ok_or_else(|| {
            CodeLensError::Validation(format!(
                "unsupported_semantic_refactor: {backend_name} adapter returned no inspectable WorkspaceEdit"
            ))
        })?;
    let transaction = codelens_engine::lsp::workspace_edit_transaction_from_value(
        &state.project(),
        workspace_edit,
    )
    .map_err(|error| CodeLensError::LspError(format!("{backend_name} adapter: {error}")))?;
    let edit_files = transaction
        .edits
        .iter()
        .map(|edit| edit.file_path.clone())
        .collect::<Vec<_>>();
    let authority_backend = authority_backend_name(backend_name);
    let transaction_contract = semantic_transaction_contract(SemanticTransactionContractInput {
        state,
        backend_id: &authority_backend,
        operation,
        target_symbol: arguments
            .get("symbol_name")
            .or_else(|| arguments.get("name"))
            .and_then(Value::as_str),
        file_paths: &edit_files,
        dry_run,
        modified_files: transaction.modified_files,
        edit_count: transaction.edit_count,
        resource_ops: json!(transaction.resource_ops),
        rollback_available: transaction.rollback_available,
        workspace_edit: serde_json::to_value(&transaction)
            .unwrap_or_else(|_| json!({"serialization_error": true})),
        apply_status: if dry_run { "preview_only" } else { "applied" },
        references_checked: false,
        conflicts: json!([]),
    });
    if !dry_run {
        codelens_engine::lsp::apply_workspace_edit_transaction(&state.project(), &transaction)
            .map_err(|error| CodeLensError::LspError(format!("{backend_name} adapter: {error}")))?;
    }

    Ok((
        json!({
            "success": true,
            "backend": "semantic_edit_backend",
            "semantic_edit_backend": backend_name,
            "authority": "workspace_edit",
            "authority_backend": authority_backend,
            "can_preview": true,
            "can_apply": true,
            "support": "authoritative_apply",
            "blocker_reason": null,
            "operation": operation,
            "edit_authority": {
                "kind": "authoritative_local_adapter",
                "backend": backend_name,
                "operation": operation,
                "methods": ["local_adapter_workspace_edit"],
                "embedding_used": false,
                "search_used": false
            },
            "transaction": {
                "dry_run": dry_run,
                "modified_files": transaction.modified_files,
                "edit_count": transaction.edit_count,
                "resource_ops": transaction.resource_ops,
                "rollback_available": transaction.rollback_available,
                "contract": transaction_contract
            },
            "workspace_edit": transaction,
            "verification": {
                "pre_diagnostics": [],
                "post_diagnostics": [],
                "references_checked": false,
                "conflicts": []
            },
            "applied": !dry_run,
            "adapter": {
                "protocol": "codelens-semantic-adapter-v1",
                "command": config.command,
                "tool": tool
            },
            "message": response.get("message").cloned().unwrap_or_else(|| {
                json!(format!(
                    "{} {} adapter edit(s) in {} file(s)",
                    if dry_run { "Would apply" } else { "Applied" },
                    transaction.edit_count,
                    transaction.modified_files
                ))
            })
        }),
        success_meta(BackendKind::Lsp, 0.90),
    ))
}

fn authority_backend_name(backend_name: &str) -> String {
    match backend_name {
        "roslyn" => "roslyn-sidecar".to_owned(),
        "jetbrains" => "ide-adapter:jetbrains".to_owned(),
        other => format!("ide-adapter:{other}"),
    }
}

struct AdapterConfig {
    command: String,
    args: Vec<String>,
    timeout: Duration,
}

impl AdapterConfig {
    fn from_env(backend_name: &str) -> Result<Self, CodeLensError> {
        let prefix = match backend_name {
            "jetbrains" => "CODELENS_JETBRAINS_ADAPTER",
            "roslyn" => "CODELENS_ROSLYN_ADAPTER",
            _ => {
                return Err(CodeLensError::Validation(format!(
                    "unsupported IDE adapter backend: {backend_name}"
                )));
            }
        };
        let timeout = adapter_timeout(prefix);
        if let Ok(command) = std::env::var(format!("{prefix}_CMD")) {
            return Ok(Self {
                command,
                args: adapter_args(prefix),
                timeout,
            });
        }

        if let Some(config) = Self::discover_sidecar(backend_name, timeout) {
            return Ok(config);
        }

        Err(CodeLensError::Validation(format!(
            "unsupported_semantic_refactor: semantic_edit_backend={backend_name} requires {prefix}_CMD or a bundled sidecar under CODELENS_ADAPTERS_DIR/{backend_name}-workspace-service"
        )))
    }

    fn discover_sidecar(backend_name: &str, timeout: Duration) -> Option<Self> {
        if backend_name != "roslyn" {
            return None;
        }

        for root in adapter_roots() {
            let sidecar_dir = root.join("roslyn-workspace-service");
            let executable = sidecar_dir.join(if cfg!(windows) {
                "codelens-roslyn-workspace-service.exe"
            } else {
                "codelens-roslyn-workspace-service"
            });
            if executable.is_file() {
                return Some(Self {
                    command: executable.display().to_string(),
                    args: Vec::new(),
                    timeout,
                });
            }

            let dll = sidecar_dir.join("CodeLens.Roslyn.WorkspaceService.dll");
            if dll.is_file() {
                return Some(Self {
                    command: "dotnet".to_owned(),
                    args: vec![dll.display().to_string()],
                    timeout,
                });
            }
        }
        None
    }
}

fn adapter_args(prefix: &str) -> Vec<String> {
    std::env::var(format!("{prefix}_ARGS"))
        .ok()
        .map(|raw| raw.split_whitespace().map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

fn adapter_timeout(prefix: &str) -> Duration {
    let timeout = std::env::var(format!("{prefix}_TIMEOUT_MS"))
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(30_000)
        .clamp(100, 120_000);
    Duration::from_millis(timeout)
}

fn adapter_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(raw) = std::env::var("CODELENS_ADAPTERS_DIR") {
        push_unique_path(&mut roots, PathBuf::from(raw));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        push_unique_path(&mut roots, parent.join("adapters"));
    }
    roots
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| same_path(existing, &path)) {
        paths.push(path);
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || match (left.canonicalize(), right.canonicalize()) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
}

fn run_adapter(
    config: &AdapterConfig,
    state: &AppState,
    arguments: &Value,
    backend_name: &str,
    tool: &str,
    operation: &str,
    dry_run: bool,
) -> Result<Value, CodeLensError> {
    let request = json!({
        "schema_version": "codelens-semantic-adapter-request-v1",
        "backend": backend_name,
        "tool": tool,
        "operation": operation,
        "project_root": state.project().as_path().display().to_string(),
        "arguments": arguments,
        "dry_run": dry_run
    });

    let mut child = Command::new(&config.command)
        .args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CodeLensError::Validation(format!(
                "failed to spawn {backend_name} semantic adapter `{}`: {error}",
                config.command
            ))
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(
                serde_json::to_string(&request)
                    .map_err(|error| {
                        CodeLensError::Validation(format!(
                            "failed to encode semantic adapter request: {error}"
                        ))
                    })?
                    .as_bytes(),
            )
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(CodeLensError::Io)?;
    }

    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(CodeLensError::Io)? {
            let output = child.wait_with_output().map_err(CodeLensError::Io)?;
            if !status.success() {
                return Err(CodeLensError::Validation(format!(
                    "{backend_name} semantic adapter failed with status {status}: {}",
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            let payload: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
                CodeLensError::Validation(format!(
                    "{backend_name} semantic adapter returned invalid JSON: {error}"
                ))
            })?;
            if payload.get("success").and_then(Value::as_bool) == Some(false) {
                let message = payload
                    .get("error")
                    .or_else(|| payload.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("adapter reported failure");
                return Err(CodeLensError::Validation(format!(
                    "{backend_name} semantic adapter rejected operation: {message}"
                )));
            }
            return Ok(payload);
        }
        if started.elapsed() > config.timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CodeLensError::Timeout {
                operation: format!("{backend_name}_semantic_adapter"),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
