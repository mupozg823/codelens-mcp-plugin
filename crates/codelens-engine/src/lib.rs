pub mod auto_import;
pub mod call_graph;
pub mod circular;
pub mod community;
pub mod coupling;
pub mod db;
#[cfg(feature = "semantic")]
pub mod embedding;
pub mod embedding_store;
pub mod file_ops;
pub mod git;
pub mod import_graph;
pub(crate) mod lang_config;
pub mod lang_registry;
pub mod lsp;
pub mod memory;
pub mod project;
pub mod rename;
pub mod scope_analysis;
pub mod search;
pub mod symbols;
pub mod type_hierarchy;
pub mod vfs;
pub mod watcher;

pub use auto_import::{
    ImportSuggestion, MissingImportAnalysis, add_import, analyze_missing_imports,
};
pub use call_graph::{CallEdge, CalleeEntry, CallerEntry, extract_calls, get_callees, get_callers};
pub use circular::{CircularDependency, find_circular_dependencies};
pub use coupling::{CouplingEntry, get_change_coupling};
pub use db::{
    DirStats, IndexDb, NewCall, NewImport, NewSymbol, SymbolWithFile, content_hash, index_db_path,
};
pub use file_ops::{
    DirectoryEntry, EnclosingSymbol, FileMatch, FileReadResult, PatternMatch, SmartPatternMatch,
    TextReference, create_text_file, delete_lines, extract_word_at_position, find_files,
    find_referencing_symbols_via_text, insert_after_symbol, insert_at_line, insert_before_symbol,
    list_dir, read_file, replace_content, replace_lines, replace_symbol_body, search_for_pattern,
    search_for_pattern_smart,
};
pub use git::{ChangedFile, DiffSymbol, DiffSymbolEntry, get_changed_files, get_diff_symbols};
pub use import_graph::{
    BlastRadiusEntry, DeadCodeEntry, DeadCodeEntryV2, GraphCache, ImportanceEntry, ImporterEntry,
    extract_imports_for_file, find_dead_code, find_dead_code_v2, get_blast_radius, get_importance,
    get_importers, resolve_module_for_file, supports_import_graph,
};
pub use lsp::{
    LSP_RECIPES, LspDiagnostic, LspDiagnosticRequest, LspRecipe, LspReference, LspRenamePlan,
    LspRenamePlanRequest, LspRequest, LspSessionPool, LspStatus, LspTypeHierarchyRequest,
    LspWorkspaceSymbol, LspWorkspaceSymbolRequest, check_lsp_status, default_lsp_args_for_command,
    default_lsp_command_for_extension, default_lsp_command_for_path,
    find_referencing_symbols_via_lsp, get_diagnostics_via_lsp, get_lsp_recipe,
    get_rename_plan_via_lsp, get_type_hierarchy_via_lsp, lsp_binary_exists,
    search_workspace_symbols_via_lsp,
};
pub use project::{
    ProjectRoot, WorkspacePackage, compute_dominant_language, detect_frameworks,
    detect_workspace_packages,
};
pub use rename::{
    RenameEdit, RenameResult, RenameScope, apply_edits, find_all_word_matches, rename_symbol,
};
pub mod change_signature;
pub mod inline;
pub mod ir;
pub mod move_symbol;
pub mod oxc_analysis;
#[cfg(feature = "semantic")]
pub use embedding::{
    EmbeddingEngine, EmbeddingIndexInfo, EmbeddingRuntimeInfo, SemanticMatch,
    configured_embedding_model_name, configured_embedding_runtime_info,
    configured_embedding_runtime_preference, configured_embedding_threads,
    embedding_model_assets_available,
};
pub use scope_analysis::{
    ReferenceKind, ScopedReference, find_scoped_references, find_scoped_references_in_file,
};
pub use search::{SearchResult, search_symbols_hybrid, search_symbols_hybrid_with_semantic};
pub use symbols::{
    IndexStats, RankedContextEntry, RankedContextResult, SymbolIndex, SymbolInfo, SymbolKind,
    SymbolProvenance,
    find_symbol, find_symbol_range, get_symbols_overview, make_symbol_id, parse_symbol_id,
    sparse_coverage_bonus_from_fields, sparse_max_bonus, sparse_threshold,
    sparse_weighting_enabled,
};
pub use type_hierarchy::{TypeHierarchyResult, TypeNode, get_type_hierarchy_native};
pub use watcher::{FileWatcher, WatcherStats};
// Semantic IR — new types only; existing types are already re-exported above.
pub use ir::{
    EditAction, EditActionKind, EditPlan, ImpactKind, ImpactNode, IrCallEdge, Relation,
    RelationKind, RetrievalConfig, RetrievalStage, RetrievalWeights,
};
