use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(super) fn home_dir_from_env() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot resolve host-native config paths")
}

pub(super) fn resolve_host_path(raw: &str, home: &Path, cwd: &Path) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        cwd.join(raw)
    }
}

pub(super) fn canonical_attach_host(host: &str) -> Option<&'static str> {
    match host.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" | "claudecode" => Some("claude-code"),
        "codex" => Some("codex"),
        "cursor" => Some("cursor"),
        "cline" => Some("cline"),
        "windsurf" | "codeium" => Some("windsurf"),
        _ => None,
    }
}

pub(super) fn supported_attach_hosts() -> &'static str {
    "claude-code, codex, cursor, cline, windsurf"
}
