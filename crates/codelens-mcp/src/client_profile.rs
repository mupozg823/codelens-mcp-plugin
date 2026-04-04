#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientProfile {
    /// Claude Code — tighter budget, balanced preset excludes builtins
    Claude,
    /// OpenAI Codex CLI — larger budget, minimal preset (has own builtins)
    Codex,
    /// Unknown or generic MCP client
    Generic,
}

impl ClientProfile {
    /// Detect client from name string (from MCP clientInfo or env).
    pub(crate) fn detect(client_name: Option<&str>) -> Self {
        match client_name {
            Some(name) => {
                let lower = name.to_ascii_lowercase();
                if lower.contains("codex") {
                    Self::Codex
                } else if lower.contains("claude") {
                    Self::Claude
                } else {
                    Self::Generic
                }
            }
            None => {
                if std::env::var("CLAUDE_PROJECT_DIR").is_ok()
                    || std::env::var("CLAUDE_CODE_ENTRYPOINT").is_ok()
                {
                    Self::Claude
                } else if std::env::var("CODEX_SANDBOX_DIR").is_ok() {
                    Self::Codex
                } else {
                    Self::Generic
                }
            }
        }
    }

    pub(crate) fn default_budget(&self) -> usize {
        match self {
            Self::Codex => 6000,
            Self::Claude => 4000,
            Self::Generic => 4000,
        }
    }

    pub(crate) fn default_preset(&self) -> crate::tool_defs::ToolPreset {
        match self {
            Self::Codex => crate::tool_defs::ToolPreset::Minimal,
            Self::Claude => crate::tool_defs::ToolPreset::Balanced,
            Self::Generic => crate::tool_defs::ToolPreset::Balanced,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Generic => "generic",
        }
    }
}
