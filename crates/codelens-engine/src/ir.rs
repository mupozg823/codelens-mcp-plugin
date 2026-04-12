//! Canonical semantic IR types for downstream consumers.
//!
//! This module provides a unified set of types that represent the semantic
//! structure of a codebase — relationships between symbols, call graph edges,
//! impact analysis nodes, and structured edit plans.
//!
//! # Re-exports
//!
//! Core types from other engine modules are re-exported here so that consumers
//! can import everything from a single location:
//!
//! ```rust
//! use codelens_engine::ir::{SymbolInfo, Relation, ImpactNode, EditPlan};
//! ```

use serde::Serialize;

// Re-exports of existing types from other engine modules.
pub use crate::circular::CircularDependency;
pub use crate::git::ChangedFile;
pub use crate::lsp::types::LspDiagnostic;
pub use crate::rename::RenameEdit;
pub use crate::search::SearchResult;
pub use crate::symbols::{RankedContextEntry, SymbolInfo, SymbolKind};

// ---------------------------------------------------------------------------
// Relation graph types
// ---------------------------------------------------------------------------

/// A directed relationship between two symbols or files.
#[derive(Debug, Clone, Serialize)]
pub struct Relation {
    /// Source symbol ID or file path.
    pub source: String,
    /// Target symbol ID or file path.
    pub target: String,
    pub kind: RelationKind,
    /// File where the relation was observed, if applicable.
    pub file_path: Option<String>,
    /// Line number where the relation was observed, if applicable.
    pub line: Option<usize>,
}

/// The kind of directed relationship between two symbols or files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RelationKind {
    /// Function calls function.
    Calls,
    /// Reverse of `Calls`.
    CalledBy,
    /// File imports file.
    Imports,
    /// Reverse of `Imports`.
    ImportedBy,
    /// Class extends class.
    Inherits,
    /// Class implements interface.
    Implements,
    /// Symbol references symbol.
    References,
    /// File or module contains symbol.
    Contains,
}

// ---------------------------------------------------------------------------
// Call graph edge
// ---------------------------------------------------------------------------

/// A call graph edge with optional metadata.
///
/// Note: the engine's lower-level [`crate::call_graph::CallEdge`] carries
/// confidence and resolution strategy fields.  This IR type is the
/// schema-facing, minimal form used in output payloads.
#[derive(Debug, Clone, Serialize)]
pub struct IrCallEdge {
    /// Caller symbol name or ID.
    pub caller: String,
    /// Callee symbol name or ID.
    pub callee: String,
    pub caller_file: String,
    pub callee_file: Option<String>,
    pub line: usize,
}

// ---------------------------------------------------------------------------
// Impact analysis graph
// ---------------------------------------------------------------------------

/// A node in an impact analysis graph.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactNode {
    pub file_path: String,
    /// Symbol name within the file, if the node represents a symbol.
    pub symbol: Option<String>,
    /// Distance from the change origin (0 = directly changed).
    pub depth: usize,
    pub impact_kind: ImpactKind,
    /// Count of symbols affected within this file.
    pub affected_symbols: usize,
}

/// How a file or symbol is affected by a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ImpactKind {
    /// Directly changed.
    Direct,
    /// Calls something that changed.
    Caller,
    /// Imports something that changed.
    Importer,
    /// Inherits or implements something that changed.
    TypeChild,
    /// Indirectly affected (transitive dependency).
    Transitive,
}

// ---------------------------------------------------------------------------
// Structured edit plan
// ---------------------------------------------------------------------------

/// A structured edit plan for multi-file changes.
#[derive(Debug, Clone, Serialize)]
pub struct EditPlan {
    pub description: String,
    pub edits: Vec<EditAction>,
}

/// A single edit action within an [`EditPlan`].
#[derive(Debug, Clone, Serialize)]
pub struct EditAction {
    pub file_path: String,
    pub kind: EditActionKind,
    /// Target line for `Insert` and `Replace` actions.
    pub line: Option<usize>,
    /// Original text to replace (used for `Replace` and `Delete`).
    pub old_text: Option<String>,
    /// Replacement or inserted text.
    pub new_text: String,
}

/// The kind of edit performed by an [`EditAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EditActionKind {
    Insert,
    Replace,
    Delete,
    /// Create a new file.
    Create,
}
