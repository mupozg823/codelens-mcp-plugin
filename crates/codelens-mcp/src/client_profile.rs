/// Effort level controls response depth and compression aggressiveness.
/// Claude Code v2.1.94 changed the default from Medium to High for most users.
/// Phase O7 adds `XHigh` to track Opus 4.7's xhigh effort setting for
/// deep-reasoning sessions where the outer harness is willing to spend
/// more tokens per call in exchange for richer context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum EffortLevel {
    Low,
    Medium,
    High,
    XHigh,
}

#[allow(dead_code)]
impl EffortLevel {
    /// Detect from `CODELENS_EFFORT_LEVEL` env var. Default: High (matching Claude Code v2.1.94).
    /// Unknown values fall back to High so rollouts of new effort names
    /// never silently degrade to Low.
    pub(crate) fn detect() -> Self {
        match std::env::var("CODELENS_EFFORT_LEVEL").ok().as_deref() {
            Some("low") => Self::Low,
            Some("medium") => Self::Medium,
            Some("xhigh") => Self::XHigh,
            Some("high") => Self::High,
            _ => Self::High,
        }
    }

    /// Multiplier applied to base token budget.
    pub(crate) fn budget_multiplier(&self) -> f64 {
        match self {
            Self::Low => 0.6,
            Self::Medium => 1.0,
            Self::High => 1.3,
            Self::XHigh => 1.6,
        }
    }

    /// Offset for the 5-stage compression thresholds (percentage points).
    /// Positive values delay compression (higher effort = more context).
    pub(crate) fn compression_threshold_offset(&self) -> i32 {
        match self {
            Self::Low => -10,
            Self::Medium => 0,
            Self::High => 10,
            Self::XHigh => 20,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Phase O7 — xhigh effort level tier ──────────────────────────
    //
    // `docs/plans/PLAN_opus47-alignment.md` Tier B adds an `XHigh`
    // variant so CodeLens can take advantage of Opus 4.7's xhigh effort
    // setting when Claude Code is configured for deep reasoning. These
    // tests pin the variant's budget/compression relationship to High
    // and the env-var detection contract.

    #[test]
    fn xhigh_effort_level_raises_budget_multiplier() {
        assert!(
            EffortLevel::XHigh.budget_multiplier() > EffortLevel::High.budget_multiplier(),
            "xhigh multiplier {} must exceed high {}",
            EffortLevel::XHigh.budget_multiplier(),
            EffortLevel::High.budget_multiplier(),
        );
        // Strict ordering across the full tier so low < medium < high < xhigh.
        assert!(EffortLevel::Low.budget_multiplier() < EffortLevel::Medium.budget_multiplier());
        assert!(EffortLevel::Medium.budget_multiplier() < EffortLevel::High.budget_multiplier());
    }

    #[test]
    fn xhigh_compression_threshold_higher_than_high() {
        assert!(
            EffortLevel::XHigh.compression_threshold_offset()
                > EffortLevel::High.compression_threshold_offset(),
            "xhigh offset {} must exceed high {}",
            EffortLevel::XHigh.compression_threshold_offset(),
            EffortLevel::High.compression_threshold_offset(),
        );
    }

    #[test]
    fn unknown_effort_level_falls_back_to_high() {
        // Env vars are process-global; serialize tests that toggle them.
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        // SAFETY: single-threaded inside lock.
        unsafe { std::env::set_var("CODELENS_EFFORT_LEVEL", "ludicrous") };
        assert_eq!(
            EffortLevel::detect(),
            EffortLevel::High,
            "unknown effort level must fall back to High"
        );

        unsafe { std::env::set_var("CODELENS_EFFORT_LEVEL", "xhigh") };
        assert_eq!(EffortLevel::detect(), EffortLevel::XHigh);

        unsafe { std::env::remove_var("CODELENS_EFFORT_LEVEL") };
        assert_eq!(
            EffortLevel::detect(),
            EffortLevel::High,
            "missing env must default to High (Claude Code v2.1.94+)"
        );
    }
}
