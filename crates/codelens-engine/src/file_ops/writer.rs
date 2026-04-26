//! Raw single-file write primitives.
//!
//! **Internal API — bypass-the-substrate warning.** Every function in
//! this module performs an unconditional disk write through
//! `apply_full_write_with_evidence`. None of them enforce
//! ADR-0009 role gates, write audit rows, or invalidate engine
//! caches. That contract lives in `codelens-mcp`'s
//! `dispatch::session::apply_post_mutation`.
//!
//! Consumers must call these primitives only via `codelens-mcp`
//! dispatch (HTTP / stdio MCP, or in-process `dispatch_tool`) —
//! direct calls from third-party crates silently bypass the
//! principals.toml configuration, the audit log, and downstream
//! cache invalidation. See the crate-level docs in `lib.rs`.

use crate::edit_transaction::{apply_full_write_with_evidence, ApplyEvidence};
use crate::project::ProjectRoot;
use anyhow::{bail, Context, Result};
use regex::Regex;
use std::fs;

pub fn create_text_file(
    project: &ProjectRoot,
    relative_path: &str,
    content: &str,
    overwrite: bool,
) -> Result<ApplyEvidence> {
    let resolved = project.resolve(relative_path)?;
    if !overwrite && resolved.exists() {
        bail!("file already exists: {}", resolved.display());
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directories for {}", resolved.display()))?;
    }
    let evidence = match apply_full_write_with_evidence(project, relative_path, content) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok(evidence)
}

pub fn delete_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
) -> Result<(String, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if start_line < 1 || start_line > total + 1 {
        bail!(
            "start_line {} out of range (file has {} lines)",
            start_line,
            total
        );
    }
    if end_line < start_line || end_line > total + 1 {
        bail!("end_line {} out of range", end_line);
    }
    // Convert from 1-indexed inclusive-start/exclusive-end to 0-indexed
    let from = start_line - 1;
    let to = (end_line - 1).min(lines.len());
    lines.drain(from..to);
    let result = lines.join("\n");
    // Preserve trailing newline if original had one
    let result = if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok((result, evidence))
}

pub fn insert_at_line(
    project: &ProjectRoot,
    relative_path: &str,
    line: usize,
    content_to_insert: &str,
) -> Result<(String, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if line < 1 || line > total + 1 {
        bail!("line {} out of range (file has {} lines)", line, total);
    }
    let insert_pos = line - 1;
    let new_lines: Vec<&str> = content_to_insert.lines().collect();
    for (i, new_line) in new_lines.iter().enumerate() {
        lines.insert(insert_pos + i, new_line);
    }
    let result = lines.join("\n");
    let result = if content.ends_with('\n') || content_to_insert.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok((result, evidence))
}

pub fn replace_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
    new_content: &str,
) -> Result<(String, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if start_line < 1 || start_line > total + 1 {
        bail!(
            "start_line {} out of range (file has {} lines)",
            start_line,
            total
        );
    }
    if end_line < start_line || end_line > total + 1 {
        bail!("end_line {} out of range", end_line);
    }
    let from = start_line - 1;
    let to = (end_line - 1).min(lines.len());
    lines.drain(from..to);
    let replacement: Vec<&str> = new_content.lines().collect();
    for (i, rep_line) in replacement.iter().enumerate() {
        lines.insert(from + i, rep_line);
    }
    let result = lines.join("\n");
    let result = if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok((result, evidence))
}

pub fn replace_content(
    project: &ProjectRoot,
    relative_path: &str,
    old_text: &str,
    new_text: &str,
    regex_mode: bool,
) -> Result<(String, usize, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let (result, count) = if regex_mode {
        let re = Regex::new(old_text).with_context(|| format!("invalid regex: {old_text}"))?;
        let mut count = 0usize;
        let replaced = re
            .replace_all(&content, |_caps: &regex::Captures| {
                count += 1;
                new_text
            })
            .into_owned();
        (replaced, count)
    } else {
        let count = content.matches(old_text).count();
        let replaced = content.replace(old_text, new_text);
        (replaced, count)
    };
    let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok((result, count, evidence))
}

pub fn replace_symbol_body(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    new_body: &str,
) -> Result<(String, ApplyEvidence)> {
    let (start_byte, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut buffer = Vec::with_capacity(bytes.len());
    buffer.extend_from_slice(&bytes[..start_byte]);
    buffer.extend_from_slice(new_body.as_bytes());
    buffer.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(buffer).with_context(|| "result is not valid UTF-8 after replacement")?;
    let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok((result, evidence))
}

pub fn insert_before_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<(String, ApplyEvidence)> {
    let (start_byte, _) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut buffer = Vec::with_capacity(bytes.len() + content_to_insert.len());
    buffer.extend_from_slice(&bytes[..start_byte]);
    buffer.extend_from_slice(content_to_insert.as_bytes());
    buffer.extend_from_slice(&bytes[start_byte..]);
    let result =
        String::from_utf8(buffer).with_context(|| "result is not valid UTF-8 after insertion")?;
    let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok((result, evidence))
}

pub fn insert_after_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<(String, ApplyEvidence)> {
    let (_, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut buffer = Vec::with_capacity(bytes.len() + content_to_insert.len());
    buffer.extend_from_slice(&bytes[..end_byte]);
    buffer.extend_from_slice(content_to_insert.as_bytes());
    buffer.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(buffer).with_context(|| "result is not valid UTF-8 after insertion")?;
    let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
        Ok(ev) => ev,
        Err(crate::edit_transaction::ApplyError::ApplyFailed {
            source: _,
            evidence,
        }) => {
            // Hybrid: status=RolledBack signals fail-closed; mcp tool handler
            // translates this to Ok response with apply_status="rolled_back" +
            // error_message synthesised from rollback_report[].reason.
            evidence
        }
        Err(other) => return Err(anyhow::Error::msg(other.to_string())),
    };
    Ok((result, evidence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_transaction::ApplyStatus;
    use crate::project::ProjectRoot;

    fn make_project(dir: &std::path::Path) -> ProjectRoot {
        ProjectRoot::new(dir.to_str().unwrap()).unwrap()
    }

    fn temp_dir_with_name(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-writer-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replace_lines_evidence_post_apply_hash_matches_disk() {
        use sha2::{Digest, Sha256};

        let dir = temp_dir_with_name("evidence");
        let project = make_project(&dir);
        std::fs::write(dir.join("doc.txt"), "line1\nline2\nline3\n").unwrap();

        let (content, evidence) = replace_lines(&project, "doc.txt", 2, 3, "REPLACED\n").unwrap();
        assert!(content.contains("REPLACED"));
        assert_eq!(evidence.status, ApplyStatus::Applied);
        assert_eq!(evidence.modified_files, 1);
        assert_eq!(evidence.edit_count, 1);

        let on_disk = std::fs::read(dir.join("doc.txt")).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&on_disk);
        let mut hex = String::with_capacity(64);
        for byte in hasher.finalize() {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
        }
        let evidence_hash = &evidence.file_hashes_after["doc.txt"].sha256;
        assert_eq!(
            evidence_hash, &hex,
            "evidence post-apply hash must match disk content"
        );
    }
}
