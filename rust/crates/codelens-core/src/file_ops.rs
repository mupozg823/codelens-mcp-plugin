use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use globset::{Glob, GlobMatcher};
use regex::Regex;
use serde::Serialize;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".idea",
    ".gradle",
    "build",
    "dist",
    "out",
    "node_modules",
    "__pycache__",
    "target",
    ".next",
    ".venv",
];

#[derive(Debug, Clone, Serialize)]
pub struct FileReadResult {
    pub file_path: String,
    pub total_lines: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub entry_type: String,
    pub path: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileMatch {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternMatch {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub matched_text: String,
    pub line_content: String,
}

pub fn read_file(
    project: &ProjectRoot,
    path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<FileReadResult> {
    let resolved = project.resolve(path)?;
    if !resolved.is_file() {
        bail!("not a file: {}", resolved.display());
    }

    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start = start_line.unwrap_or(0).min(total_lines);
    let end = end_line.unwrap_or(total_lines).clamp(start, total_lines);

    Ok(FileReadResult {
        file_path: project.to_relative(&resolved),
        total_lines,
        content: lines[start..end].join("\n"),
    })
}

pub fn list_dir(project: &ProjectRoot, path: &str, recursive: bool) -> Result<Vec<DirectoryEntry>> {
    let resolved = project.resolve(path)?;
    if !resolved.is_dir() {
        bail!("not a directory: {}", resolved.display());
    }

    let mut entries = Vec::new();
    if recursive {
        for entry in WalkDir::new(&resolved)
            .min_depth(1)
            .into_iter()
            .filter_entry(|entry| !is_excluded(entry.path()))
        {
            let entry = entry?;
            entries.push(to_directory_entry(project, entry.path())?);
        }
    } else {
        for entry in fs::read_dir(&resolved)? {
            let entry = entry?;
            entries.push(to_directory_entry(project, &entry.path())?);
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

pub fn find_files(
    project: &ProjectRoot,
    wildcard_pattern: &str,
    relative_dir: Option<&str>,
) -> Result<Vec<FileMatch>> {
    let base = match relative_dir {
        Some(path) => project.resolve(path)?,
        None => project.as_path().to_path_buf(),
    };
    if !base.is_dir() {
        bail!("not a directory: {}", base.display());
    }

    let matcher = compile_glob(wildcard_pattern)?;
    let mut matches = Vec::new();

    for entry in WalkDir::new(&base)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if entry.file_type().is_file() && matcher.is_match(entry.file_name()) {
            matches.push(FileMatch {
                path: project.to_relative(entry.path()),
            });
        }
    }

    matches.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(matches)
}

pub fn search_for_pattern(
    project: &ProjectRoot,
    pattern: &str,
    file_glob: Option<&str>,
    max_results: usize,
) -> Result<Vec<PatternMatch>> {
    let regex = Regex::new(pattern).with_context(|| format!("invalid regex: {pattern}"))?;
    let matcher = match file_glob {
        Some(glob) => Some(compile_glob(glob)?),
        None => None,
    };

    let mut results = Vec::new();
    for entry in WalkDir::new(project.as_path())
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(matcher) = &matcher {
            if !matcher.is_match(entry.file_name()) {
                continue;
            }
        }

        let content = match fs::read_to_string(entry.path()) {
            Ok(content) => content,
            Err(_) => continue,
        };

        for (index, line) in content.lines().enumerate() {
            if results.len() >= max_results {
                return Ok(results);
            }
            if let Some(found) = regex.find(line) {
                results.push(PatternMatch {
                    file_path: project.to_relative(entry.path()),
                    line: index + 1,
                    column: found.start() + 1,
                    matched_text: found.as_str().to_owned(),
                    line_content: line.trim().to_owned(),
                });
            }
        }
    }

    Ok(results)
}

fn to_directory_entry(project: &ProjectRoot, path: &Path) -> Result<DirectoryEntry> {
    let metadata = fs::metadata(path)?;
    Ok(DirectoryEntry {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        entry_type: if metadata.is_dir() {
            "directory".to_owned()
        } else {
            "file".to_owned()
        },
        path: project.to_relative(path),
        size: if metadata.is_file() {
            Some(metadata.len())
        } else {
            None
        },
    })
}

fn compile_glob(pattern: &str) -> Result<GlobMatcher> {
    Glob::new(pattern)
        .with_context(|| format!("invalid glob: {pattern}"))
        .map(|glob| glob.compile_matcher())
}

fn is_excluded(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        EXCLUDED_DIRS.contains(&value.as_ref())
    })
}

#[cfg(test)]
mod tests {
    use super::{find_files, list_dir, read_file, search_for_pattern};
    use crate::ProjectRoot;
    use std::fs;

    #[test]
    fn reads_partial_file() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = read_file(&project, "src/main.py", Some(1), Some(3)).expect("read file");
        assert_eq!(result.total_lines, 4);
        assert_eq!(
            result.content,
            "def greet(name):\n    return f\"Hello {name}\""
        );
    }

    #[test]
    fn lists_nested_dir() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = list_dir(&project, ".", true).expect("list dir");
        assert!(result.iter().any(|entry| entry.path == "src/main.py"));
    }

    #[test]
    fn finds_files_by_glob() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = find_files(&project, "*.py", Some("src")).expect("find files");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "src/main.py");
    }

    #[test]
    fn searches_text_pattern() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "greet", Some("*.py"), 10).expect("search");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].file_path, "src/main.py");
    }

    fn fixture_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-core-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).expect("create src dir");
        fs::write(
            dir.join("src/main.py"),
            "class Service:\ndef greet(name):\n    return f\"Hello {name}\"\nprint(greet(\"A\"))\n",
        )
        .expect("write fixture");
        dir
    }
}
