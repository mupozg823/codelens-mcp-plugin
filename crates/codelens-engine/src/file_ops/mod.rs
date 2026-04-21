mod reader;
mod support;
#[cfg(test)]
mod tests;
mod text_refs;
mod types;
mod writer;

pub use reader::{find_files, list_dir, read_file, search_for_pattern, search_for_pattern_smart};
pub use text_refs::{extract_word_at_position, find_referencing_symbols_via_text};
pub use types::{
    DirectoryEntry, EnclosingSymbol, FileMatch, FileReadResult, PatternMatch, SmartPatternMatch,
    TextReference, TextRefsReport,
};
pub use writer::{
    create_text_file, delete_lines, insert_after_symbol, insert_at_line, insert_before_symbol,
    replace_content, replace_lines, replace_symbol_body,
};

use support::{
    FlatSymbol, compile_glob, find_enclosing_symbol, flatten_to_ranges, to_directory_entry,
};
