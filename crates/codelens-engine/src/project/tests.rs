mod exclusions;
mod language;
mod root_detect;
mod workspace;

use std::fs;

pub(super) fn tempfile_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let (td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-core-project-");
    fs::create_dir_all(&dir).expect("create tempdir");
    (td, dir)
}
