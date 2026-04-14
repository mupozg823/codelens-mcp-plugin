use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
}

fn read_repo_file(relative_path: &str) -> String {
    fs::read_to_string(repo_root().join(relative_path))
        .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"))
}

fn is_semver_token(token: &str) -> bool {
    let mut parts = token.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !major.is_empty()
        && !minor.is_empty()
        && !patch.is_empty()
        && major.chars().all(|ch| ch.is_ascii_digit())
        && minor.chars().all(|ch| ch.is_ascii_digit())
        && patch.chars().all(|ch| ch.is_ascii_digit())
}

fn line_contains_backticked_semver(line: &str) -> bool {
    line.split('`')
        .enumerate()
        .any(|(index, segment)| index % 2 == 1 && is_semver_token(segment.trim()))
}

#[test]
fn architecture_snapshot_docs_match_runtime_registry() {
    let readme = read_repo_file("README.md");
    let architecture = read_repo_file("docs/architecture.md");
    let version = crate::build_info::BUILD_VERSION;
    let tools = crate::tool_defs::tools();
    let tool_count = tools.len();
    let schema_count = tools
        .iter()
        .filter(|tool| tool.output_schema.is_some())
        .count();
    let release_notes = repo_root().join(format!("docs/release-notes/v{version}.md"));

    assert!(
        release_notes.exists(),
        "expected current release notes file to exist: {}",
        release_notes.display()
    );
    assert!(
        readme.contains(&format!(
            "Latest release notes: [v{version}](docs/release-notes/v{version}.md)"
        )),
        "README.md should point to the current release notes for v{version}"
    );
    assert!(
        architecture.contains(&format!("- Workspace version: `{version}`")),
        "docs/architecture.md should report the current workspace version"
    );
    assert!(
        architecture.contains(&format!(
            "- Registered tool definitions in source: `{tool_count}` `Tool::new(...)` entries"
        )),
        "docs/architecture.md should report the current tool count"
    );
    assert!(
        architecture.contains(&format!(
            "- Tool output schemas in source: `{schema_count} / {tool_count}`"
        )),
        "docs/architecture.md should report the current schema/tool ratio"
    );
    assert!(
        architecture.contains(&format!(
            "- Current release notes: [docs/release-notes/v{version}.md](release-notes/v{version}.md)"
        )),
        "docs/architecture.md should link the current release notes"
    );
    assert!(
        architecture.contains(&format!(
            "- **{schema_count} of {tool_count} tools** declare a JSON output schema in the current source tree"
        )),
        "docs/architecture.md should keep the schema summary in sync"
    );
    assert!(
        architecture.contains(&format!(
            "✅ Tool Output Schemas ({schema_count}/{tool_count} tools)"
        )),
        "docs/architecture.md should keep the protocol-stack schema count in sync"
    );
}

#[test]
fn public_docs_only_expose_canonical_workflow_entrypoints() {
    let readme = read_repo_file("README.md");
    let platform_setup = read_repo_file("docs/platform-setup.md");

    for workflow in [
        "explore_codebase",
        "trace_request_path",
        "review_architecture",
        "plan_safe_refactor",
        "review_changes",
        "diagnose_issues",
        "cleanup_duplicate_logic",
    ] {
        assert!(
            readme.contains(&format!("`{workflow}`")),
            "README.md should expose canonical workflow `{workflow}`"
        );
    }

    for deprecated_alias in [
        "audit_security_context",
        "analyze_change_impact",
        "assess_change_readiness",
    ] {
        assert!(
            !readme.contains(deprecated_alias),
            "README.md should not expose deprecated workflow alias `{deprecated_alias}`"
        );
        assert!(
            !platform_setup.contains(deprecated_alias),
            "docs/platform-setup.md should not expose deprecated workflow alias `{deprecated_alias}`"
        );
    }
}

#[test]
fn platform_setup_avoids_version_pinned_runtime_shape_claims() {
    let platform_setup = read_repo_file("docs/platform-setup.md");

    for line in platform_setup
        .lines()
        .filter(|line| line.contains("runtime shape"))
    {
        assert!(
            !line_contains_backticked_semver(line),
            "docs/platform-setup.md should not pin a version inside runtime-shape guidance: {line}"
        );
    }
}
