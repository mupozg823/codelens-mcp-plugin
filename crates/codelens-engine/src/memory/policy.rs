use std::path::Path;

use serde::Deserialize;

use super::paths::now_secs;

/// Policy controlling visibility and mutability of memory entries.
///
/// Stored as TOML at `.codelens/memories/.policy`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoryPolicy {
    /// Glob patterns for read-only entries.
    #[serde(default)]
    pub read_only: Vec<String>,
    /// Glob patterns for ignored entries.
    #[serde(default)]
    pub ignored: Vec<String>,
    /// Maximum age in days before an entry is considered stale.
    #[serde(default)]
    pub max_age_days: Option<u64>,
}

pub(crate) const POLICY_FILENAME: &str = "__policy__";
pub(crate) const POLICY_FILE_BASENAME: &str = ".policy";
pub(crate) const ARCHIVE_DIRNAME: &str = ".archive";

impl MemoryPolicy {
    /// Load policy from the `.policy` file inside the memories directory.
    pub fn load(memories_dir: &Path) -> Self {
        let policy_path = memories_dir.join(POLICY_FILE_BASENAME);
        if !policy_path.is_file() {
            return Self::default();
        }
        let content = match std::fs::read_to_string(&policy_path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        Self::parse(&content)
    }

    /// Parse a TOML policy file.
    pub(crate) fn parse(content: &str) -> Self {
        toml::from_str(content).unwrap_or_default()
    }

    pub fn is_read_only(&self, name: &str) -> bool {
        matches_any_pattern(name, &self.read_only)
    }

    pub fn is_ignored(&self, name: &str) -> bool {
        matches_any_pattern(name, &self.ignored)
    }

    pub fn is_stale(&self, _name: &str, modified_secs: u64) -> bool {
        let Some(max_days) = self.max_age_days else {
            return false;
        };
        let age_secs = now_secs().saturating_sub(modified_secs);
        age_secs > max_days.saturating_mul(86400)
    }
}

fn matches_any_pattern(name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| glob_match(pattern, name))
}

pub(crate) fn glob_match(pattern: &str, name: &str) -> bool {
    let pat_bytes = pattern.as_bytes();
    let name_bytes = name.as_bytes();
    let mut pi = 0usize;
    let mut ni = 0usize;
    let mut star_pi = usize::MAX;
    let mut star_ni = usize::MAX;

    while ni < name_bytes.len() {
        if pi < pat_bytes.len() {
            let pc = pat_bytes[pi];
            if pc == b'*' {
                star_pi = pi;
                star_ni = ni;
                pi += 1;
                continue;
            }
            if pc == b'?' || pc == name_bytes[ni] {
                pi += 1;
                ni += 1;
                continue;
            }
        }
        if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ni += 1;
            ni = star_ni;
            continue;
        }
        return false;
    }

    while pi < pat_bytes.len() && pat_bytes[pi] == b'*' {
        pi += 1;
    }
    pi == pat_bytes.len()
}
