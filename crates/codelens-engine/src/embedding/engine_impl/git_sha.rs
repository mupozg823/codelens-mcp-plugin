use crate::project::ProjectRoot;

pub(super) fn current_git_sha(project: &ProjectRoot) -> Option<String> {
    let git_path = project.as_path().join(".git");
    let git_dir = if git_path.is_dir() {
        git_path
    } else {
        let git_file = std::fs::read_to_string(&git_path).ok()?;
        let path = git_file.trim().strip_prefix("gitdir:")?.trim();
        let path = std::path::Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            project.as_path().join(path)
        }
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let Some(reference) = head.strip_prefix("ref:").map(str::trim) else {
        return valid_sha(head);
    };
    let ref_path = git_dir.join(reference);
    if let Ok(sha) = std::fs::read_to_string(ref_path)
        && let Some(valid) = valid_sha(sha.trim())
    {
        return Some(valid);
    }
    let packed_refs = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    packed_refs.lines().find_map(|line| {
        if line.starts_with('#') || line.starts_with('^') {
            return None;
        }
        let (sha, name) = line.split_once(' ')?;
        if name.trim() == reference {
            valid_sha(sha)
        } else {
            None
        }
    })
}

fn valid_sha(raw: &str) -> Option<String> {
    let candidate = raw.trim();
    if candidate.len() == 40 && candidate.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(candidate.to_owned())
    } else {
        None
    }
}
