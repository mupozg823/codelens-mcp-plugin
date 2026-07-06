pub fn is_test_only_symbol(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> bool {
    let fp = &sym.file_path;

    if fp.contains("/tests/") || fp.ends_with("_tests.rs") {
        return true;
    }
    if fp.contains("/__tests__/") || fp.contains("\\__tests__\\") {
        return true;
    }
    if fp.ends_with("_test.py") || fp.ends_with("_test.go") {
        return true;
    }
    if fp.ends_with(".test.ts")
        || fp.ends_with(".test.tsx")
        || fp.ends_with(".test.js")
        || fp.ends_with(".test.jsx")
        || fp.ends_with(".spec.ts")
        || fp.ends_with(".spec.js")
    {
        return true;
    }
    if fp.contains("/src/test/") {
        return true;
    }
    if fp.ends_with("Test.java") || fp.ends_with("Tests.java") {
        return true;
    }
    if fp.ends_with("_test.rb") || fp.contains("/spec/") {
        return true;
    }

    if sym.name_path.starts_with("tests::")
        || sym.name_path.contains("::tests::")
        || sym.name_path.starts_with("test::")
        || sym.name_path.contains("::test::")
    {
        return true;
    }

    let Some(source) = source else {
        return false;
    };

    let start = usize::try_from(sym.start_byte.max(0))
        .unwrap_or(0)
        .min(source.len());
    let window_start = start.saturating_sub(2048);
    let attrs = String::from_utf8_lossy(&source.as_bytes()[window_start..start]);
    if attrs.contains("#[test]")
        || attrs.contains("#[tokio::test]")
        || attrs.contains("#[cfg(test)]")
        || attrs.contains("#[cfg(all(test")
    {
        return true;
    }

    if fp.ends_with(".py") {
        if sym.name.starts_with("test_") {
            return true;
        }
        if sym.kind == "class" && sym.name.starts_with("Test") {
            return true;
        }
    }

    if fp.ends_with(".go") && sym.name.starts_with("Test") && sym.kind == "function" {
        return true;
    }

    if fp.ends_with(".java") || fp.ends_with(".kt") {
        let before = &source[..start];
        let window = if before.len() > 200 {
            &before[before.len() - 200..]
        } else {
            before
        };
        if window.contains("@Test")
            || window.contains("@ParameterizedTest")
            || window.contains("@RepeatedTest")
        {
            return true;
        }
    }

    false
}
