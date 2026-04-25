use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use url::Url;

pub(super) fn lsp_uri_to_project_relative(project: &ProjectRoot, uri: &str) -> Result<String> {
    let absolute_path = Url::parse(uri)
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
        .with_context(|| format!("invalid LSP file uri: {uri}"))?;
    let canonical_path = canonicalize_lsp_path(absolute_path);
    let resolved_path = project.resolve(&canonical_path)?;
    Ok(project.to_relative(&resolved_path))
}

pub(super) fn canonicalize_lsp_path(path: std::path::PathBuf) -> std::path::PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name())
        && let Ok(parent) = parent.canonicalize()
    {
        return parent.join(file_name);
    }
    path
}
