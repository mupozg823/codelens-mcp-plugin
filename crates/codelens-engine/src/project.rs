mod discovery;
mod frameworks;
mod root;
#[cfg(test)]
mod tests;
mod workspace;

pub use discovery::{EXCLUDED_DIRS, collect_files, compute_dominant_language, is_excluded};
pub use frameworks::detect_frameworks;
pub use root::ProjectRoot;
pub use workspace::{WorkspacePackage, detect_workspace_packages};
