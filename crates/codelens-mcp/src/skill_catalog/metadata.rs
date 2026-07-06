use std::io::Read;
use std::path::Path;

pub(super) const METADATA_READ_LIMIT: u64 = 8192;

pub(super) struct SkillMetadata {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) mtime_epoch_secs: u64,
    pub(super) content_hash: String,
}

pub(super) fn read_skill_metadata(path: &Path) -> Option<SkillMetadata> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(METADATA_READ_LIMIT)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mtime_epoch_secs = std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    Some(SkillMetadata {
        name: frontmatter_value(&text, "name")
            .or_else(|| first_markdown_heading(&text))
            .unwrap_or_else(|| {
                path.parent()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown-skill".to_owned())
            }),
        description: frontmatter_value(&text, "description").unwrap_or_default(),
        mtime_epoch_secs,
        content_hash: fnv1a64_hex(&bytes),
    })
}

fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let mut lines = text.lines();
    (lines.next()? == "---").then_some(())?;
    let prefix = format!("{key}:");
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

fn first_markdown_heading(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|heading| !heading.is_empty())
        .map(ToOwned::to_owned)
}

fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
