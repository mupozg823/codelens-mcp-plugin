use std::path::Path;

const MARKUP_CONFIG_EXTENSIONS: &[&str] = &[".css", ".html", ".htm", ".yaml", ".yml"];
const MARKUP_CONFIG_QUERY_TERMS: &[&str] = &[
    "css",
    "stylesheet",
    "styles",
    "html",
    "markup",
    "dom",
    "yaml",
    "yml",
    "workflow",
    "workflows",
    "config",
    "configuration",
];

pub(crate) fn normalize_path_scope(project_root: &Path, raw_scope: Option<&str>) -> Option<String> {
    let raw = raw_scope?.trim();
    if raw.is_empty() {
        return None;
    }

    let raw_normalized = raw.replace('\\', "/");
    let project_normalized = project_root.to_string_lossy().replace('\\', "/");
    let stripped = if Path::new(raw).is_absolute() {
        raw_normalized
            .strip_prefix(&project_normalized)
            .unwrap_or(&raw_normalized)
    } else {
        &raw_normalized
    };
    let trimmed = stripped
        .trim_start_matches('/')
        .trim_start_matches("./")
        .trim_end_matches('/');

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub(crate) fn file_matches_scope(file_path: &str, path_scope: Option<&str>) -> bool {
    let Some(scope) = path_scope else {
        return true;
    };
    let file = normalize_relative_path(file_path);
    let scope = normalize_relative_path(scope);

    file == scope
        || file
            .strip_prefix(&scope)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn markup_config_penalty_multiplier(query: &str, file_path: &str) -> f64 {
    if !is_markup_or_config_path(file_path) || query_explicitly_targets_markup_or_config(query) {
        return 1.0;
    }
    0.55
}

fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_owned()
}

fn is_markup_or_config_path(file_path: &str) -> bool {
    let file_path = file_path.to_ascii_lowercase();
    MARKUP_CONFIG_EXTENSIONS
        .iter()
        .any(|extension| file_path.ends_with(extension))
}

fn query_explicitly_targets_markup_or_config(query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    if MARKUP_CONFIG_EXTENSIONS
        .iter()
        .any(|extension| query.contains(extension))
    {
        return true;
    }

    query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(|token| MARKUP_CONFIG_QUERY_TERMS.contains(&token))
}

#[cfg(test)]
mod tests {
    use super::{file_matches_scope, markup_config_penalty_multiplier, normalize_path_scope};
    use std::path::Path;

    #[test]
    fn normalizes_relative_and_absolute_scopes() {
        let root = Path::new("/workspace/project");

        assert_eq!(
            normalize_path_scope(root, Some("./src/services/")),
            Some("src/services".to_owned())
        );
        assert_eq!(
            normalize_path_scope(root, Some("/workspace/project/src/services")),
            Some("src/services".to_owned())
        );
        assert_eq!(normalize_path_scope(root, Some("  ")), None);
        assert_eq!(normalize_path_scope(root, None), None);
    }

    #[test]
    fn matches_exact_scope_and_children_only() {
        assert!(file_matches_scope(
            "tuanbo-controller/src/services/license.ts",
            Some("tuanbo-controller/src/services")
        ));
        assert!(file_matches_scope(
            "tuanbo-controller/src/services",
            Some("tuanbo-controller/src/services")
        ));
        assert!(!file_matches_scope(
            "tuanbo-controller/src/service-worker.ts",
            Some("tuanbo-controller/src/services")
        ));
        assert!(file_matches_scope("anything.rs", None));
    }

    #[test]
    fn penalizes_markup_config_only_when_query_does_not_target_it() {
        assert!(
            markup_config_penalty_multiplier(
                "block resolume advanced actions unless pro license feature enabled",
                "tuanbo-controller/src/styles/popup.css"
            ) < 1.0
        );
        assert_eq!(
            markup_config_penalty_multiplier(
                "css advanced section layout",
                "tuanbo-controller/src/styles/popup.css"
            ),
            1.0
        );
        assert_eq!(
            markup_config_penalty_multiplier(
                "block resolume advanced actions",
                "tuanbo-controller/src/services/license.ts"
            ),
            1.0
        );
    }
}
