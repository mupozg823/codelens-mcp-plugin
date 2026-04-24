#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerCompatMode {
    Default,
    AnthropicRemote,
}

impl ServerCompatMode {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "anthropic-remote" | "anthropic_remote" | "anthropic" => Self::AnthropicRemote,
            _ => Self::Default,
        }
    }

    pub(crate) fn tools_only(self) -> bool {
        matches!(self, Self::AnthropicRemote)
    }
}
