use serde::{Deserialize, Serialize};

use super::MemoryTier;

/// Parsed YAML frontmatter from a memory markdown file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MemoryFrontmatter {
    #[serde(default)]
    pub linked_symbols: Vec<String>,
    #[serde(default)]
    pub linked_files: Vec<String>,
    #[serde(default)]
    pub linked_analyses: Vec<String>,
}

/// Parse an optional YAML frontmatter block from memory content.
pub fn parse_frontmatter(content: &str) -> Option<MemoryFrontmatter> {
    if !content.starts_with("---") {
        return None;
    }
    let after_first = &content[3..];
    let end_marker = after_first.find("\n---")?;
    let yaml_text = &after_first[..end_marker];
    let mut fm = MemoryFrontmatter::default();
    let mut current_list: Option<&str> = None;
    for line in yaml_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("linked_symbols:") {
            current_list = Some("symbols");
            fm.linked_symbols = parse_yaml_list(value);
        } else if let Some(value) = line.strip_prefix("linked_files:") {
            current_list = Some("files");
            fm.linked_files = parse_yaml_list(value);
        } else if let Some(value) = line.strip_prefix("linked_analyses:") {
            current_list = Some("analyses");
            fm.linked_analyses = parse_yaml_list(value);
        } else if line.starts_with("- ") {
            let item = line.strip_prefix("- ").unwrap().trim().trim_matches('"');
            if !item.is_empty() {
                match current_list {
                    Some("symbols") => fm.linked_symbols.push(item.to_string()),
                    Some("files") => fm.linked_files.push(item.to_string()),
                    Some("analyses") => fm.linked_analyses.push(item.to_string()),
                    _ => {}
                }
            }
        }
    }
    if fm.linked_symbols.is_empty() && fm.linked_files.is_empty() && fm.linked_analyses.is_empty() {
        return None;
    }
    Some(fm)
}

fn parse_yaml_list(value: &str) -> Vec<String> {
    let v = value.trim();
    if v.starts_with('[') && v.ends_with(']') {
        let inner = &v[1..v.len() - 1];
        inner
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    } else if v.is_empty() {
        Vec::new()
    } else {
        vec![v.trim_matches('"').to_string()]
    }
}

/// Strip frontmatter from content, returning just the body text.
pub fn strip_frontmatter(content: &str) -> &str {
    if !content.starts_with("---") {
        return content;
    }
    let after_first = &content[3..];
    if let Some(end_offset) = after_first.find("\n---") {
        let body_start = end_offset + 4;
        let body = &content[3 + body_start..];
        body.trim_start()
    } else {
        content
    }
}

/// Metadata returned alongside memory content for rich responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryMetadata {
    pub tier: MemoryTier,
    pub stale: bool,
    pub last_modified_secs: Option<u64>,
    pub linked_symbols: Vec<String>,
    pub linked_files: Vec<String>,
    pub linked_analyses: Vec<String>,
}
