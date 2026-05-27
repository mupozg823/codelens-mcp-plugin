//! Project and global memory management.
//!
//! Memory is organised in two tiers:
//! - Project: `<project>/.codelens/memories/`
//! - Global: `$HOME/.codelens/memories/`
//!
//! The public API is intentionally flat (`codelens_engine::memory::*`) while
//! the implementation is split by responsibility to keep policy, path
//! resolution, persistence, archive, audit, and frontmatter logic independent.

mod archive;
mod audit;
mod frontmatter;
mod paths;
mod policy;
mod store;

pub use archive::{
    archive_memory, archive_memory_rec, list_archived, restore_archived, restore_archived_rec,
};
pub use audit::{AuditRecorder, MemoryAuditEvent, NullRecorder};
pub use frontmatter::{MemoryFrontmatter, MemoryMetadata, parse_frontmatter, strip_frontmatter};
pub use paths::{
    MemoryLocation, MemoryTier, global_memory_dir, resolve_memory_path, resolve_memory_tier,
};
pub use policy::MemoryPolicy;
pub use store::{
    delete_memory, delete_memory_tiered, delete_memory_tiered_rec, list_all_memory_names,
    list_memory_names, list_memory_names_with_policy, read_memory, read_memory_from_tier,
    read_memory_with_metadata, read_policy, rename_memory, write_memory, write_memory_tiered,
    write_memory_tiered_rec,
};

#[cfg(test)]
mod tests;
