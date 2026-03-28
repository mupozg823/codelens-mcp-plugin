pub mod composite;
pub mod filesystem;
pub mod graph;
pub mod lsp;
pub mod memory;
pub mod mutation;
pub mod session;
pub mod symbols;

use crate::protocol::ToolResponseMeta;
use crate::AppState;

/// Tool handler result type — every handler returns this.
pub type ToolResult = anyhow::Result<(serde_json::Value, ToolResponseMeta)>;

pub fn success_meta(backend_used: &str, confidence: f64) -> ToolResponseMeta {
    ToolResponseMeta {
        backend_used: backend_used.to_owned(),
        confidence,
        degraded_reason: None,
    }
}

pub fn required_string<'a>(value: &'a serde_json::Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: {key}"))
}

/// Parse LSP args from arguments, falling back to defaults for the given command.
pub fn parse_lsp_args(arguments: &serde_json::Value, command: &str) -> Vec<String> {
    arguments
        .get("args")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| default_lsp_args_for_command(command))
}

pub fn default_lsp_command_for_path(file_path: &str) -> Option<String> {
    match std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "py" => Some("pyright-langserver".to_owned()),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            Some("typescript-language-server".to_owned())
        }
        "rs" => Some("rust-analyzer".to_owned()),
        "cs" => Some("csharp-ls".to_owned()),
        "dart" => Some("dart".to_owned()),
        _ => None,
    }
}

pub fn default_lsp_args_for_command(command: &str) -> Vec<String> {
    match command {
        "pyright-langserver" => vec!["--stdio".to_owned()],
        "typescript-language-server" => vec!["--stdio".to_owned()],
        "dart" => vec!["language-server".to_owned(), "--protocol=lsp".to_owned()],
        _ => Vec::new(),
    }
}
