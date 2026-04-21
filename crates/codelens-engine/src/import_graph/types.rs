use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlastRadiusEntry {
    pub file: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImporterEntry {
    pub file: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportanceEntry {
    pub file: String,
    pub score: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeadCodeEntry {
    pub file: String,
    pub symbol: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub(crate) imports: HashSet<String>,
    pub(crate) imported_by: HashSet<String>,
}
