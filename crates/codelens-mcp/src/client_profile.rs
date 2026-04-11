/// Effort level controls response depth and compression aggressiveness.
/// Claude Code v2.1.94 changed the default from Medium to High for most users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum EffortLevel {
    Low,
    Medium,
    High,
}

#[allow(dead_code)]
impl EffortLevel {
    /// Detect from `CODELENS_EFFORT_LEVEL` env var. Default: High (matching Claude Code v2.1.94).
    pub(crate) fn detect() -> Self {
        match std::env::var("CODELENS_EFFORT_LEVEL").ok().as_deref() {
            Some("low") => Self::Low,
            Some("medium") => Self::Medium,
            _ => Self::High,
        }
    }

    /// Multiplier applied to base token budget.
    pub(crate) fn budget_multiplier(&self) -> f64 {
        match self {
            Self::Low => 0.6,
            Self::Medium => 1.0,
            Self::High => 1.3,
        }
    }

    /// Offset for the 5-stage compression thresholds (percentage points).
    /// Positive values delay compression (higher effort = more context).
    pub(crate) fn compression_threshold_offset(&self) -> i32 {
        match self {
            Self::Low => -10,
            Self::Medium => 0,
            Self::High => 10,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

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

    pub(crate) fn default_deferred_tool_loading(&self) -> Option<bool> {
        match self {
            Self::Codex => Some(true),
            Self::Claude => Some(false),
            Self::Generic => None,
        }
    }

    pub(crate) fn default_preset(&self) -> crate::tool_defs::ToolPreset {
        match self {
            Self::Codex => crate::tool_defs::ToolPreset::Minimal,
            Self::Claude => crate::tool_defs::ToolPreset::Balanced,
            Self::Generic => crate::tool_defs::ToolPreset::Balanced,
        }
    }

    pub(crate) fn default_tool_contract_mode(&self) -> &'static str {
        match self {
            Self::Codex => "lean",
            Self::Claude | Self::Generic => "full",
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
