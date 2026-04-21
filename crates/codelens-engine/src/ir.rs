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
//! use codelens_engine::ir::{
//!     SymbolInfo, Relation, ImpactNode, EditPlan,
//!     SearchCandidate, IntelligenceSource, CodeDiagnostic,
//! };
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

// ---------------------------------------------------------------------------
// Retrieval pipeline types
// ---------------------------------------------------------------------------

/// Describes a stage in the retrieval pipeline.
///
/// The full pipeline is: `Lexical → SymbolScore → DenseRetrieval → Rerank → GraphExpand`
///
/// Each stage can be enabled/disabled and contributes a weighted score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RetrievalStage {
    /// FTS5 / BM25 corpus search — file-level pre-filtering.
    Lexical,
    /// Symbol name/signature scoring — AST-aware matching.
    SymbolScore,
    /// Embedding-based dense retrieval — semantic similarity.
    DenseRetrieval,
    /// Multi-signal blending — text + pagerank + recency + semantic.
    Rerank,
    /// Graph expansion — callers, importers, type hierarchy of top results.
    GraphExpand,
}

/// Configuration for a retrieval pipeline run.
#[derive(Debug, Clone, Serialize)]
pub struct RetrievalConfig {
    /// Which stages are enabled.
    pub stages: Vec<RetrievalStage>,
    /// Maximum results to return.
    pub max_results: usize,
    /// Token budget for response.
    pub token_budget: usize,
    /// Whether to include symbol bodies.
    pub include_body: bool,
    /// Weight overrides per stage (default: equal weighting).
    pub weights: RetrievalWeights,
}

/// Weights for each retrieval signal in the rerank stage.
#[derive(Debug, Clone, Serialize)]
pub struct RetrievalWeights {
    pub text: f64,
    pub pagerank: f64,
    pub recency: f64,
    pub semantic: f64,
}

impl Default for RetrievalWeights {
    fn default() -> Self {
        Self {
            text: 0.40,
            pagerank: 0.20,
            recency: 0.10,
            semantic: 0.30,
        }
    }
}

// ---------------------------------------------------------------------------
// Intelligence source (fast / precise path)
// ---------------------------------------------------------------------------

/// The backend that produced a result.
///
/// Consumers use this to judge confidence: `TreeSitter` results are fast but
/// approximate; `Lsp` / `Scip` results are precise but require optional backends.
/// `Semantic` results come from the embedding model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IntelligenceSource {
    /// tree-sitter AST parse — always available, fast path.
    TreeSitter,
    /// LSP backend (opt-in) — precise type-aware results.
    Lsp,
    /// SCIP index import — precise, offline.
    Scip,
    /// Embedding-based semantic search.
    Semantic,
    /// Hybrid: multiple sources combined.
    Hybrid,
}

// ---------------------------------------------------------------------------
// Unified search candidate
// ---------------------------------------------------------------------------

/// A search result from any retrieval path. This is the substrate type that
/// downstream consumers (MCP response builders, workflow tools) should target.
///
/// Existing types (`SearchResult`, `ScoredChunk`, `RankedContextEntry`) are
/// gradually converging toward this shape. New code should prefer
/// `SearchCandidate` and convert from legacy types via `From` impls.
#[derive(Debug, Clone, Serialize)]
pub struct SearchCandidate {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line: usize,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub score: f64,
    pub source: IntelligenceSource,
}

impl From<crate::search::SearchResult> for SearchCandidate {
    fn from(r: crate::search::SearchResult) -> Self {
        Self {
            name: r.name,
            kind: r.kind,
            file_path: r.file,
            line: r.line,
            signature: r.signature,
            name_path: Some(r.name_path),
            body: None,
            score: r.score,
            source: IntelligenceSource::TreeSitter,
        }
    }
}

impl From<crate::embedding_store::ScoredChunk> for SearchCandidate {
    fn from(c: crate::embedding_store::ScoredChunk) -> Self {
        Self {
            name: c.symbol_name,
            kind: c.kind,
            file_path: c.file_path,
            line: c.line,
            signature: c.signature,
            name_path: Some(c.name_path),
            body: None,
            score: c.score,
            source: IntelligenceSource::Semantic,
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostic (unified)
// ---------------------------------------------------------------------------

/// A code diagnostic from any analysis backend.
#[derive(Debug, Clone, Serialize)]
pub struct CodeDiagnostic {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: IntelligenceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

// ---------------------------------------------------------------------------
// Retrieval config defaults
// ---------------------------------------------------------------------------

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            stages: vec![
                RetrievalStage::Lexical,
                RetrievalStage::SymbolScore,
                RetrievalStage::DenseRetrieval,
                RetrievalStage::Rerank,
            ],
            max_results: 20,
            token_budget: 4000,
            include_body: true,
            weights: RetrievalWeights::default(),
        }
    }
}
