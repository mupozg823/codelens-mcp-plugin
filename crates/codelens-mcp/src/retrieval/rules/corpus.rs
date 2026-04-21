use std::fs;
use std::path::{Path, PathBuf};

/// Which corpus slot a snippet was loaded from. Retrieval can use this
/// to weight global rules above memory entries, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSource {
    /// `~/.claude/CLAUDE.md` — applies to every project.
    Global,
    /// `<project_root>/CLAUDE.md` — repo-local policy.
    ProjectLocal,
    /// `~/.claude/projects/<slug>/memory/*.md` — auto-memory entries.
    Memory,
}

impl RuleSource {
    pub fn as_label(self) -> &'static str {
        match self {
            RuleSource::Global => "global",
            RuleSource::ProjectLocal => "project-local",
            RuleSource::Memory => "memory",
        }
    }
}

/// One retrievable rule chunk.
#[derive(Debug, Clone)]
pub struct RuleSnippet {
    pub source_file: PathBuf,
    pub source_kind: RuleSource,
    /// YAML `name:` field if present, for memory-file frontmatter.
    pub frontmatter_name: Option<String>,
    /// `## heading` text if the chunk is inside a section.
    pub section_title: Option<String>,
    /// Body text (without the `## heading` line or frontmatter).
    pub content: String,
    /// 1-based line where the section starts in the source file.
    pub line_start: usize,
}

/// Slug used by the Claude host for per-project state directories.
/// Matches the convention observed under `~/.claude/projects/`:
/// absolute path with `/` replaced by `-`.
pub fn project_slug(project_root: &Path) -> String {
    project_root.to_string_lossy().replace('/', "-")
}

/// Scan every source and return a flat list of snippets. Missing files
/// are silently skipped so projects without memory / without a local
/// `CLAUDE.md` still get the global rules.
pub fn load_rule_corpus(project_root: &Path) -> Vec<RuleSnippet> {
    let mut out = Vec::new();
    let home = home_dir();

    if let Some(home) = home.as_ref() {
        let global = home.join(".claude").join("CLAUDE.md");
        if let Ok(text) = fs::read_to_string(&global) {
            append_chunks(&mut out, &global, RuleSource::Global, &text);
        }
    }

    let project_claude = project_root.join("CLAUDE.md");
    if let Ok(text) = fs::read_to_string(&project_claude) {
        append_chunks(&mut out, &project_claude, RuleSource::ProjectLocal, &text);
    }

    if let Some(home) = home.as_ref() {
        let slug = project_slug(project_root);
        let memory_dir = home
            .join(".claude")
            .join("projects")
            .join(slug)
            .join("memory");
        if let Ok(entries) = fs::read_dir(&memory_dir) {
            let mut files: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().and_then(|e| e.to_str()) == Some("md")
                        && p.file_name().and_then(|n| n.to_str()) != Some("MEMORY.md")
                })
                .collect();
            files.sort();
            for path in files {
                if let Ok(text) = fs::read_to_string(&path) {
                    append_chunks(&mut out, &path, RuleSource::Memory, &text);
                }
            }
        }
    }

    out
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Split markdown text into snippets, one per top-level `## ` section,
/// and push them onto `out`. YAML frontmatter between the first and
/// second `---` line is consumed — its `name:` field is captured into
/// each resulting snippet when present.
fn append_chunks(out: &mut Vec<RuleSnippet>, path: &Path, kind: RuleSource, text: &str) {
    let (frontmatter_name, body_start_line, body) = extract_frontmatter(text);

    let mut current_title: Option<String> = None;
    let mut current_body = String::new();
    let mut current_start = body_start_line;
    let mut saw_any_section = false;

    for (offset, line) in body.lines().enumerate() {
        let line_num = body_start_line + offset;
        if let Some(title) = line.strip_prefix("## ") {
            flush(
                out,
                path,
                kind,
                &frontmatter_name,
                &current_title,
                &current_body,
                current_start,
            );
            saw_any_section = true;
            current_title = Some(title.trim().to_string());
            current_body.clear();
            current_start = line_num;
            continue;
        }
        current_body.push_str(line);
        current_body.push('\n');
    }

    if !saw_any_section {
        flush(
            out,
            path,
            kind,
            &frontmatter_name,
            &None,
            &current_body,
            body_start_line,
        );
    } else {
        flush(
            out,
            path,
            kind,
            &frontmatter_name,
            &current_title,
            &current_body,
            current_start,
        );
    }
}

fn flush(
    out: &mut Vec<RuleSnippet>,
    path: &Path,
    kind: RuleSource,
    frontmatter_name: &Option<String>,
    section_title: &Option<String>,
    body: &str,
    line_start: usize,
) {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return;
    }
    out.push(RuleSnippet {
        source_file: path.to_path_buf(),
        source_kind: kind,
        frontmatter_name: frontmatter_name.clone(),
        section_title: section_title.clone(),
        content: trimmed.to_string(),
        line_start,
    });
}

/// Parse YAML frontmatter (`---` delimited) at the top of a markdown
/// file. Returns `(name_field, first_body_line_number, remaining_body)`.
/// If no frontmatter is present, returns `(None, 1, text)`.
fn extract_frontmatter(text: &str) -> (Option<String>, usize, &str) {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (None, 1, text);
    };
    if first.trim() != "---" {
        return (None, 1, text);
    }

    let mut consumed = first.len();
    let mut name_field = None;
    let mut body_line = 2usize;
    for line in lines {
        consumed += line.len();
        body_line += 1;
        let stripped = line.trim_end_matches('\n');
        if stripped.trim() == "---" {
            let remaining = &text[consumed..];
            return (name_field, body_line, remaining);
        }
        if let Some(rest) = stripped.strip_prefix("name:") {
            name_field = Some(rest.trim().to_string());
        }
    }
    (None, 1, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_file_snippet_when_no_section_headers() {
        let mut out = Vec::new();
        append_chunks(
            &mut out,
            Path::new("/tmp/example.md"),
            RuleSource::Global,
            "# Title\n\nBody text without ## headers.\n",
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_kind, RuleSource::Global);
        assert!(out[0].content.contains("Body text without"));
        assert_eq!(out[0].section_title, None);
    }

    #[test]
    fn section_split_by_double_hash_headers() {
        let text =
            "# Intro\n\nPreamble text.\n\n## First\n\nAlpha body.\n\n## Second\n\nBeta body.\n";
        let mut out = Vec::new();
        append_chunks(&mut out, Path::new("/tmp/a.md"), RuleSource::Global, text);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].section_title, None);
        assert!(out[0].content.contains("Preamble"));
        assert_eq!(out[1].section_title.as_deref(), Some("First"));
        assert!(out[1].content.contains("Alpha body"));
        assert_eq!(out[2].section_title.as_deref(), Some("Second"));
        assert!(out[2].content.contains("Beta body"));
    }

    #[test]
    fn frontmatter_name_captured() {
        let text = "---\nname: feedback testing\ndescription: x\ntype: feedback\n---\n\nBody.\n";
        let mut out = Vec::new();
        append_chunks(&mut out, Path::new("/tmp/b.md"), RuleSource::Memory, text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].frontmatter_name.as_deref(), Some("feedback testing"));
        assert!(out[0].content.contains("Body"));
        assert!(!out[0].content.contains("name:"));
    }

    #[test]
    fn empty_section_is_skipped() {
        let text = "## A\n\n## B\n\nNot empty.\n";
        let mut out = Vec::new();
        append_chunks(&mut out, Path::new("/tmp/c.md"), RuleSource::Global, text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].section_title.as_deref(), Some("B"));
    }

    #[test]
    fn malformed_frontmatter_treated_as_body() {
        let text = "---\nname: still open\nno closing delimiter\n\nBody.\n";
        let mut out = Vec::new();
        append_chunks(&mut out, Path::new("/tmp/d.md"), RuleSource::Memory, text);
        assert_eq!(out.len(), 1);
        assert!(out[0].content.contains("---"));
        assert_eq!(out[0].frontmatter_name, None);
    }

    #[test]
    fn project_slug_converts_absolute_path() {
        let s = project_slug(Path::new("/Users/alice/project/repo"));
        assert_eq!(s, "-Users-alice-project-repo");
    }

    #[test]
    fn line_numbers_track_through_frontmatter() {
        let text = "---\nname: x\n---\n\n## First\n\nAlpha.\n\n## Second\n\nBeta.\n";
        let mut out = Vec::new();
        append_chunks(&mut out, Path::new("/tmp/e.md"), RuleSource::Memory, text);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].section_title.as_deref(), Some("First"));
        assert_eq!(out[0].line_start, 5);
        assert_eq!(out[1].section_title.as_deref(), Some("Second"));
        assert_eq!(out[1].line_start, 9);
    }

    #[test]
    fn source_kind_labels_are_stable() {
        assert_eq!(RuleSource::Global.as_label(), "global");
        assert_eq!(RuleSource::ProjectLocal.as_label(), "project-local");
        assert_eq!(RuleSource::Memory.as_label(), "memory");
    }
}
