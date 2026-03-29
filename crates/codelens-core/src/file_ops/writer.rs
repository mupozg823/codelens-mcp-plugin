use crate::project::ProjectRoot;
use anyhow::{bail, Context, Result};
use regex::Regex;
use std::fs;

pub fn create_text_file(
    project: &ProjectRoot,
    relative_path: &str,
    content: &str,
    overwrite: bool,
) -> Result<()> {
    let resolved = project.resolve(relative_path)?;
    if !overwrite && resolved.exists() {
        bail!("file already exists: {}", resolved.display());
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directories for {}", resolved.display()))?;
    }
    fs::write(&resolved, content).with_context(|| format!("failed to write {}", resolved.display()))
}

pub fn delete_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
) -> Result<String> {
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
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn insert_at_line(
    project: &ProjectRoot,
    relative_path: &str,
    line: usize,
    content_to_insert: &str,
) -> Result<String> {
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
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn replace_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
    new_content: &str,
) -> Result<String> {
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
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn replace_content(
    project: &ProjectRoot,
    relative_path: &str,
    old_text: &str,
    new_text: &str,
    regex_mode: bool,
) -> Result<(String, usize)> {
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
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok((result, count))
}

pub fn replace_symbol_body(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    new_body: &str,
) -> Result<String> {
    let (start_byte, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    result.extend_from_slice(&bytes[..start_byte]);
    result.extend_from_slice(new_body.as_bytes());
    result.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(result).with_context(|| "result is not valid UTF-8 after replacement")?;
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn insert_before_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<String> {
    let (start_byte, _) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() + content_to_insert.len());
    result.extend_from_slice(&bytes[..start_byte]);
    result.extend_from_slice(content_to_insert.as_bytes());
    result.extend_from_slice(&bytes[start_byte..]);
    let result =
        String::from_utf8(result).with_context(|| "result is not valid UTF-8 after insertion")?;
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn insert_after_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<String> {
    let (_, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() + content_to_insert.len());
    result.extend_from_slice(&bytes[..end_byte]);
    result.extend_from_slice(content_to_insert.as_bytes());
    result.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(result).with_context(|| "result is not valid UTF-8 after insertion")?;
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}
