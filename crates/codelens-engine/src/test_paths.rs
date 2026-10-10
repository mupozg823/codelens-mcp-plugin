//! Whether a path names test code, by the conventions of common languages.

/// `file_path` is matched on `/`-delimited segments, so pass a path that
/// starts with `/` (prefix a repo-relative path) to catch a top-level `tests/`.
pub fn is_probable_test_path(file_path: &str) -> bool {
    file_path.contains("/tests/")
        || file_path.contains("/__tests__/")
        || file_path.contains("/src/test/")
        || file_path.ends_with("_test.py")
        || file_path.ends_with("_test.go")
        || file_path.ends_with("_test.rs")
        || file_path.ends_with("_tests.rs")
        || file_path.ends_with(".test.ts")
        || file_path.ends_with(".test.tsx")
        || file_path.ends_with(".test.js")
        || file_path.ends_with(".spec.ts")
        || file_path.ends_with(".spec.js")
        || file_path.ends_with("Test.java")
        || file_path.ends_with("Tests.java")
        || file_path.ends_with("_test.rb")
}
