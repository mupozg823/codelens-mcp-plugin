//! Project and global memory management.
//!
//! Memory is organised in two tiers:
//! - Project: `<project>/.codelens/memories/`
//! - Global: `$HOME/.codelens/memories/`
//!
//! The public API is intentionally flat (`codelens_engine::memory::*`) while
//! the implementation is split by responsibility to keep policy, path
//! resolution, persistence, archive, and frontmatter logic independent.

mod archive;
mod frontmatter;
mod paths;
mod policy;
mod store;

/// Current Unix time in seconds. Shared time util kept at module root so
/// neither `paths` nor `policy` depend on each other for it
/// (breaks the former paths↔policy import cycle).
pub(crate) fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub use archive::{archive_memory, list_archived, restore_archived};
pub use frontmatter::{MemoryFrontmatter, MemoryMetadata, parse_frontmatter, strip_frontmatter};
pub use paths::{
    MemoryLocation, MemoryTier, global_memory_dir, resolve_memory_path, resolve_memory_tier,
};
pub use policy::MemoryPolicy;
pub use store::{
    delete_memory, delete_memory_tiered, list_all_memory_names, list_memory_names,
    list_memory_names_with_policy, read_memory, read_memory_from_tier, read_memory_with_metadata,
    read_policy, rename_memory, write_memory, write_memory_tiered,
};

#[cfg(test)]
mod tests;
