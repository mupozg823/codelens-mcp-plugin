pub mod auto_import;
pub mod call_graph;
pub mod circular;
pub mod coupling;
pub mod db;
pub mod file_ops;
pub mod git;
pub mod import_graph;
pub mod lsp;
pub mod project;
pub mod rename;
pub mod scope_analysis;
pub mod search;
pub mod symbols;
pub mod type_hierarchy;
pub mod watcher;

pub use call_graph::{CallEdge, CalleeEntry, CallerEntry, extract_calls, get_callees, get_callers};
pub use circular::{CircularDependency, find_circular_dependencies};
pub use coupling::{CouplingEntry, get_change_coupling};
pub use db::{IndexDb, NewCall, NewImport, NewSymbol, content_hash, index_db_path};
pub use file_ops::{
    DirectoryEntry, FileMatch, FileReadResult, PatternMatch, TextReference, create_text_file,
    delete_lines, extract_word_at_position, find_files, find_referencing_symbols_via_text,
    insert_after_symbol, insert_at_line, insert_before_symbol, list_dir, read_file,
    replace_content, replace_lines, replace_symbol_body, search_for_pattern,
    search_for_pattern_smart, SmartPatternMatch, EnclosingSymbol,
};
pub use git::{ChangedFile, DiffSymbol, DiffSymbolEntry, get_changed_files, get_diff_symbols};
pub use import_graph::{
    BlastRadiusEntry, DeadCodeEntry, DeadCodeEntryV2, GraphCache, ImportanceEntry, ImporterEntry,
    extract_imports_for_file, find_dead_code, find_dead_code_v2, get_blast_radius, get_importance,
    get_importers, resolve_module_for_file, supports_import_graph,
};
pub use lsp::{
    LspDiagnostic, LspDiagnosticRequest, LspRecipe, LspReference, LspRenamePlan,
    LspRenamePlanRequest, LspRequest, LspSessionPool, LspStatus, LspTypeHierarchyRequest,
    LspWorkspaceSymbol, LspWorkspaceSymbolRequest, LSP_RECIPES, check_lsp_status,
    find_referencing_symbols_via_lsp, get_diagnostics_via_lsp, get_lsp_recipe,
    get_rename_plan_via_lsp, get_type_hierarchy_via_lsp, search_workspace_symbols_via_lsp,
};
pub use auto_import::{ImportSuggestion, MissingImportAnalysis, add_import, analyze_missing_imports};
pub use project::ProjectRoot;
pub use rename::{RenameResult, RenameScope, rename_symbol};
pub use scope_analysis::{ScopedReference, ReferenceKind, find_scoped_references, find_scoped_references_in_file};
pub use type_hierarchy::{TypeHierarchyResult, TypeNode, get_type_hierarchy_native};
pub use search::{SearchResult, search_symbols_hybrid};
pub use watcher::{FileWatcher, WatcherStats};
pub use symbols::{
    IndexStats, RankedContextEntry, RankedContextResult, SymbolIndex, SymbolInfo, SymbolKind,
    find_symbol, find_symbol_range, get_symbols_overview, make_symbol_id, parse_symbol_id,
};
