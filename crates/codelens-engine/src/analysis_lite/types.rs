use crate::oxc_analysis::ByteSpan;
use serde::Serialize;

pub const SCHEMA_VERSION: &str = "analysis-lite/v1";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Ok,
    Partial,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceError {
    pub code: &'static str,
    pub backend: &'static str,
    pub file: Option<String>,
    pub message: String,
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for EvidenceError {}

#[derive(Debug, Clone)]
pub struct DeclarationSelector {
    pub file: String,
    pub name: Option<String>,
    pub symbol_id: Option<String>,
    pub name_span: Option<ByteSpan>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeclarationIdentity {
    pub symbol_id: String,
    pub file: String,
    pub name: String,
    pub name_span: Option<ByteSpan>,
    pub declaration_span: Option<ByteSpan>,
    pub kind: &'static str,
}

impl DeclarationIdentity {
    /// Reuses the traversal identity seam with an exact declaration path.
    pub fn call_target(&self) -> crate::call_graph::types::CallTargetIdentity {
        crate::call_graph::types::CallTargetIdentity {
            canonical_name: self.name.clone(),
            resolved_file: Some(self.file.clone()),
            declaration_path: Some(self.symbol_id.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct EvidenceLocation {
    pub file: String,
    pub span: ByteSpan,
    pub line: usize,
    pub column: usize,
    pub offset_encoding: &'static str,
    /// The source line holding `span`, trimmed and capped at
    /// [`LINE_TEXT_MAX_BYTES`]. Set on reference and edge rows so a host can
    /// judge a site without another file read; unset on unresolved examples.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_text: Option<String>,
}

/// Byte cap of [`EvidenceLocation::line_text`]; a longer line ends in `…`.
pub const LINE_TEXT_MAX_BYTES: usize = 160;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Provenance {
    pub backend: &'static str,
    pub relation_kind: &'static str,
    pub detail: &'static str,
    pub source_hash: String,
    pub resolution: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReferenceEvidence {
    pub location: EvidenceLocation,
    pub target: DeclarationIdentity,
    pub source: Option<DeclarationIdentity>,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EdgeEvidence {
    pub source: Option<DeclarationIdentity>,
    pub target: DeclarationIdentity,
    pub location: EvidenceLocation,
    pub relation_kind: &'static str,
    pub detail: &'static str,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedEvidence {
    pub location: Option<EvidenceLocation>,
    pub name: String,
    pub reason: String,
    pub backend: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedSummary {
    pub scope: &'static str,
    pub observed_count: usize,
    pub examples: Vec<UnresolvedEvidence>,
    pub examples_truncated: bool,
    pub total: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceLimits {
    pub max_results: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_files: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EvidenceTruncation {
    pub results: bool,
    pub depth: bool,
    pub nodes: bool,
    pub edges: bool,
    pub files: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceSnapshot {
    pub generation: u64,
    pub source_basis: &'static str,
    pub source_bundle_hash: Option<String>,
    pub files_read: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceScope {
    pub language: &'static str,
    pub caller_path: Option<String>,
    pub backends: Vec<&'static str>,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceFreshness {
    pub disk: &'static str,
    pub generation_is_disk_freshness: bool,
    pub reconciliation: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EvidenceResults {
    pub references: Vec<ReferenceEvidence>,
    pub nodes: Vec<DeclarationIdentity>,
    pub edges: Vec<EdgeEvidence>,
    pub selection_candidates: Vec<DeclarationIdentity>,
    pub total: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisEvidence {
    pub schema_version: &'static str,
    pub capability: &'static str,
    pub project: String,
    pub target: Option<DeclarationIdentity>,
    pub snapshot: EvidenceSnapshot,
    pub scope: EvidenceScope,
    pub status: EvidenceStatus,
    pub results: EvidenceResults,
    pub limits: EvidenceLimits,
    pub truncation: EvidenceTruncation,
    pub unresolved: UnresolvedSummary,
    pub freshness: SourceFreshness,
    pub errors: Vec<EvidenceError>,
}

impl std::fmt::Display for AnalysisEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisQuery {
    References,
    Impact,
}
