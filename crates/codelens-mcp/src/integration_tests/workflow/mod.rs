use super::*;

/// Global mutex to serialise tests that temporarily mutate PATH so they don't
/// stomp each other when the test runner uses multiple threads.
pub(super) static PATH_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn prepend_path(dir: &std::path::Path, original_path: &str) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(original_path));
    std::env::join_paths(paths).expect("join PATH entries")
}

mod analysis;
mod capabilities;
mod change;
mod dispatch;
mod harness;
mod impact;
mod jobs;
mod misc;
mod onboard;
mod resources;
mod schema;
mod session;
mod symbol;
mod workflow;
