//! Single source of truth for supported language extensions.
//!
//! All extension-to-language dispatch tables across the codebase should derive
//! from this registry to prevent mismatches.

use std::path::Path;

/// Metadata for a supported file extension.
#[derive(Debug, Clone, Copy)]
pub struct ExtEntry {
    pub ext: &'static str,
    /// LSP textDocument/didOpen language identifier.
    pub language_id: &'static str,
    /// Whether import graph analysis is supported for this extension.
    pub supports_imports: bool,
}

/// Canonical table of all supported extensions.
/// Every extension that tree-sitter can parse should appear here.
static EXTENSIONS: &[ExtEntry] = &[
    // Python
    ExtEntry {
        ext: "py",
        language_id: "python",
        supports_imports: true,
    },
    // JavaScript
    ExtEntry {
        ext: "js",
        language_id: "javascript",
        supports_imports: true,
    },
    ExtEntry {
        ext: "mjs",
        language_id: "javascript",
        supports_imports: true,
    },
    ExtEntry {
        ext: "cjs",
        language_id: "javascript",
        supports_imports: true,
    },
    // TypeScript
    ExtEntry {
        ext: "ts",
        language_id: "typescript",
        supports_imports: true,
    },
    // TSX / JSX
    ExtEntry {
        ext: "tsx",
        language_id: "typescriptreact",
        supports_imports: true,
    },
    ExtEntry {
        ext: "jsx",
        language_id: "javascriptreact",
        supports_imports: true,
    },
    // Go
    ExtEntry {
        ext: "go",
        language_id: "go",
        supports_imports: true,
    },
    // Java
    ExtEntry {
        ext: "java",
        language_id: "java",
        supports_imports: true,
    },
    // Kotlin (including .kts scripts)
    ExtEntry {
        ext: "kt",
        language_id: "kotlin",
        supports_imports: true,
    },
    ExtEntry {
        ext: "kts",
        language_id: "kotlin",
        supports_imports: true,
    },
    // Rust
    ExtEntry {
        ext: "rs",
        language_id: "rust",
        supports_imports: true,
    },
    // C
    ExtEntry {
        ext: "c",
        language_id: "c",
        supports_imports: true,
    },
    ExtEntry {
        ext: "h",
        language_id: "c",
        supports_imports: true,
    },
    // C++
    ExtEntry {
        ext: "cpp",
        language_id: "cpp",
        supports_imports: true,
    },
    ExtEntry {
        ext: "cc",
        language_id: "cpp",
        supports_imports: true,
    },
    ExtEntry {
        ext: "cxx",
        language_id: "cpp",
        supports_imports: true,
    },
    ExtEntry {
        ext: "hpp",
        language_id: "cpp",
        supports_imports: true,
    },
    ExtEntry {
        ext: "hh",
        language_id: "cpp",
        supports_imports: true,
    },
    ExtEntry {
        ext: "hxx",
        language_id: "cpp",
        supports_imports: true,
    },
    // PHP
    ExtEntry {
        ext: "php",
        language_id: "php",
        supports_imports: true,
    },
    // Swift — no import extraction yet
    ExtEntry {
        ext: "swift",
        language_id: "swift",
        supports_imports: false,
    },
    // Scala — no import extraction yet
    ExtEntry {
        ext: "scala",
        language_id: "scala",
        supports_imports: false,
    },
    ExtEntry {
        ext: "sc",
        language_id: "scala",
        supports_imports: false,
    },
    // Ruby
    ExtEntry {
        ext: "rb",
        language_id: "ruby",
        supports_imports: true,
    },
    // C#
    ExtEntry {
        ext: "cs",
        language_id: "csharp",
        supports_imports: true,
    },
    // Dart
    ExtEntry {
        ext: "dart",
        language_id: "dart",
        supports_imports: true,
    },
];

/// Look up an extension entry by lowercase extension string.
pub fn for_extension(ext: &str) -> Option<&'static ExtEntry> {
    EXTENSIONS.iter().find(|e| e.ext == ext)
}

/// Whether tree-sitter symbol parsing is supported for this extension.
/// All registered extensions support symbols.
pub fn supports_symbols(ext: &str) -> bool {
    for_extension(ext).is_some()
}

/// Whether import graph analysis is supported for this extension.
pub fn supports_imports(ext: &str) -> bool {
    for_extension(ext).is_some_and(|e| e.supports_imports)
}

/// Whether import graph analysis is supported for a file path.
pub fn supports_imports_for_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| supports_imports(&ext.to_ascii_lowercase()))
}

/// Return the LSP language identifier for an extension.
pub fn language_id(ext: &str) -> Option<&'static str> {
    for_extension(ext).map(|e| e.language_id)
}

/// Return all extensions that support import analysis.
pub fn import_extensions() -> impl Iterator<Item = &'static str> {
    EXTENSIONS
        .iter()
        .filter(|e| e.supports_imports)
        .map(|e| e.ext)
}

/// Return all supported extensions.
pub fn all_extensions() -> impl Iterator<Item = &'static str> {
    EXTENSIONS.iter().map(|e| e.ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_extensions_unique() {
        let mut seen = std::collections::HashSet::new();
        for entry in EXTENSIONS {
            assert!(seen.insert(entry.ext), "duplicate extension: {}", entry.ext);
        }
    }

    #[test]
    fn kts_supports_imports() {
        assert!(
            supports_imports("kts"),
            "kts should support imports (Kotlin scripts)"
        );
    }

    #[test]
    fn swift_scala_no_imports() {
        assert!(!supports_imports("swift"));
        assert!(!supports_imports("scala"));
        assert!(!supports_imports("sc"));
    }

    #[test]
    fn hh_hxx_have_language_id() {
        assert_eq!(language_id("hh"), Some("cpp"));
        assert_eq!(language_id("hxx"), Some("cpp"));
    }

    #[test]
    fn jsx_tsx_distinct_language_ids() {
        assert_eq!(language_id("tsx"), Some("typescriptreact"));
        assert_eq!(language_id("jsx"), Some("javascriptreact"));
    }
}
