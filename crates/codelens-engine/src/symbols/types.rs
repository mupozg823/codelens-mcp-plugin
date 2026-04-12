use crate::db::IndexDb;
use serde::{Deserialize, Serialize};

/// Structural ownership category derived from file path.
/// Used by the ranker to disambiguate same-name symbols across
/// crate boundaries without hardcoding specific symbol names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SymbolProvenance {
    /// Core engine implementation (codelens-engine/src/)
    #[default]
    EngineCore,
    /// MCP tool handler (codelens-mcp/src/tools/)
    McpTool,
    /// MCP dispatch/protocol layer (codelens-mcp/src/dispatch/, protocol.rs)
    McpInfra,
    /// TUI surface layer (codelens-tui/src/)
    TuiSurface,
    /// Test code (**/tests/, *_tests.rs)
    Test,
    /// Benchmark/script code (benchmarks/, scripts/)
    Benchmark,
}

impl SymbolProvenance {
    /// Derive provenance from a relative file path.
    pub fn from_path(path: &str) -> Self {
        if path.contains("/tests/")
            || path.contains("/tests.")
            || path.ends_with("_tests.rs")
            || path.contains("/integration_tests/")
        {
            return Self::Test;
        }
        if path.starts_with("benchmarks/")
            || path.starts_with("scripts/")
            || path.starts_with("models/")
        {
            return Self::Benchmark;
        }
        if path.contains("codelens-tui/") {
            return Self::TuiSurface;
        }
        if path.contains("codelens-mcp/src/tools/") {
            return Self::McpTool;
        }
        if path.contains("codelens-mcp/") {
            return Self::McpInfra;
        }
        // Default: engine core or any other source
        Self::EngineCore
    }

    /// Ranking penalty/boost for "implementation" queries.
    /// Positive = prefer, negative = demote.
    pub fn impl_query_prior(self) -> f64 {
        match self {
            Self::EngineCore => 6.0,
            Self::McpTool => -4.0,
            Self::McpInfra => -2.0,
            Self::TuiSurface => -8.0,
            Self::Test => -12.0,
            Self::Benchmark => -14.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    File,
    Class,
    Interface,
    Enum,
    Module,
    Method,
    Function,
    Property,
    Variable,
    TypeAlias,
    Unknown,
}

impl SymbolKind {
    pub fn as_label(&self) -> &'static str {
        match self {
            SymbolKind::File => "file",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            SymbolKind::Module => "module",
            SymbolKind::Method => "method",
            SymbolKind::Function => "function",
            SymbolKind::Property => "property",
            SymbolKind::Variable => "variable",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Unknown => "unknown",
        }
    }

    pub fn from_str_label(s: &str) -> SymbolKind {
        match s {
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "enum" => SymbolKind::Enum,
            "module" => SymbolKind::Module,
            "method" => SymbolKind::Method,
            "function" => SymbolKind::Function,
            "property" => SymbolKind::Property,
            "variable" => SymbolKind::Variable,
            "type_alias" => SymbolKind::TypeAlias,
            _ => SymbolKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub signature: String,
    pub name_path: String,
    pub id: String,
    /// Structural ownership derived from file path. Not stored in DB —
    /// computed on construction. Used by ranker for disambiguation.
    #[serde(skip)]
    pub provenance: SymbolProvenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SymbolInfo>,
    /// Byte offsets for batch body extraction (not serialized to API output).
    /// u32 saves 8 bytes per symbol vs usize; sufficient for files up to 4GB.
    #[serde(skip)]
    pub start_byte: u32,
    #[serde(skip)]
    pub end_byte: u32,
}

/// Construct a stable symbol ID: `{file_path}#{kind}:{name_path}`
///
/// Uses `String::with_capacity` to allocate the exact final size in
/// one shot, avoiding the internal reallocation that `format!()` may
/// do when it starts from an empty buffer and grows.
pub fn make_symbol_id(file_path: &str, kind: &SymbolKind, name_path: &str) -> String {
    let label = kind.as_label();
    let mut id = String::with_capacity(file_path.len() + 1 + label.len() + 1 + name_path.len());
    id.push_str(file_path);
    id.push('#');
    id.push_str(label);
    id.push(':');
    id.push_str(name_path);
    id
}

/// Parse a stable symbol ID. Returns `(file_path, kind_label, name_path)` or `None`.
pub fn parse_symbol_id(input: &str) -> Option<(&str, &str, &str)> {
    let hash_pos = input.find('#')?;
    let after_hash = &input[hash_pos + 1..];
    let colon_pos = after_hash.find(':')?;
    let file_path = &input[..hash_pos];
    let kind = &after_hash[..colon_pos];
    let name_path = &after_hash[colon_pos + 1..];
    if file_path.is_empty() || kind.is_empty() || name_path.is_empty() {
        return None;
    }
    Some((file_path, kind, name_path))
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    pub indexed_files: usize,
    pub supported_files: usize,
    pub stale_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedContextEntry {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub relevance_score: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedContextResult {
    pub query: String,
    pub symbols: Vec<RankedContextEntry>,
    pub count: usize,
    pub token_budget: usize,
    pub chars_used: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ParsedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub start_byte: u32,
    pub end_byte: u32,
    pub signature: String,
    pub body: Option<String>,
    pub name_path: String,
    pub children: Vec<ParsedSymbol>,
}

/// Read-only DB access — either an owned read-only connection or a borrowed writer guard.
pub(crate) enum ReadDb<'a> {
    Owned(IndexDb),
    Writer(std::sync::MutexGuard<'a, IndexDb>),
}

/// Intermediate result of analyzing a single file.
/// Decouples parse phase from DB write phase, enabling:
/// - Parallel parse (rayon) → sequential DB commit
/// - Failure tracking without losing previously indexed data
/// - Future: async pipeline stages
pub(crate) struct AnalyzedFile {
    pub relative_path: String,
    pub mtime: i64,
    pub content_hash: String,
    pub size_bytes: i64,
    pub language_ext: String,
    pub symbols: Vec<ParsedSymbol>,
    pub imports: Vec<crate::db::NewImport>,
    pub calls: Vec<crate::db::NewCall>,
}

impl std::ops::Deref for ReadDb<'_> {
    type Target = IndexDb;
    fn deref(&self) -> &IndexDb {
        match self {
            ReadDb::Owned(db) => db,
            ReadDb::Writer(guard) => guard,
        }
    }
}
