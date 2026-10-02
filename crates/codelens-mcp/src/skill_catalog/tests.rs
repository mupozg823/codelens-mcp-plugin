use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "codelens-skill-catalog-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn codex_skill_catalog_counts_skill_files_without_loading_bodies() {
    let root = temp_dir("counts");
    let codex_skill = root.join(".codex/skills/rust/SKILL.md");
    let plugin_skill = root.join(".codex/plugins/cache/plugin/skills/frontend/SKILL.md");
    std::fs::create_dir_all(codex_skill.parent().unwrap()).unwrap();
    std::fs::create_dir_all(plugin_skill.parent().unwrap()).unwrap();
    std::fs::write(&codex_skill, "# Rust\n\nlarge body intentionally ignored").unwrap();
    std::fs::write(
        &plugin_skill,
        "# Frontend\n\nlarge body intentionally ignored",
    )
    .unwrap();

    let roots = discover_codex_skill_roots_from_home(&root);
    let catalog = codex_skill_catalog_for_roots(&roots, 8);

    assert_eq!(catalog["total_skill_count"], json!(2));
    assert_eq!(
        catalog["scan_policy"],
        json!("metadata path scan only; SKILL.md bodies are not loaded by this resource")
    );
    assert!(catalog["roots"].as_array().unwrap().iter().any(|root| {
        root["path"].as_str().unwrap().ends_with(".codex/skills") && root["skill_count"] == json!(1)
    }));
    assert!(
        serde_json::to_string(&catalog)
            .unwrap()
            .contains(".codex/plugins/cache/plugin/skills/frontend/SKILL.md")
    );
    assert!(
        serde_json::to_string(&catalog)
            .unwrap()
            .contains("content_hash")
    );
}

#[test]
fn codex_skill_binding_contract_points_codex_at_runtime_catalog() {
    let contract = codex_skill_binding_contract();

    assert_eq!(contract["target_host"], json!("codex"));
    assert_eq!(
        contract["resource_uri"],
        json!(CODEX_SKILL_CATALOG_RESOURCE_URI)
    );
    assert!(
        contract["codex_native_targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target == "AGENTS.md")
    );
}

#[test]
fn codex_prepare_skill_hints_stay_compact_and_point_to_catalog() {
    let hints = codex_prepare_skill_hints(None, None);

    assert_eq!(hints["target_host"], json!("codex"));
    assert_eq!(
        hints["catalog_resource"],
        json!(CODEX_SKILL_CATALOG_RESOURCE_URI)
    );
    assert_eq!(hints["selection_limit"], json!(3));
    assert!(hints["roots"].is_array());
}

#[test]
fn codex_prepare_skill_hints_accepts_host_observed_roots() {
    let root = temp_dir("host-roots");
    let skill = root.join("team/skills/rust/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        &skill,
        r#"---
name: rust-host-skill
description: Use for Rust MCP embedding and CodeLens harness work.
---
"#,
    )
    .unwrap();

    let hints = codex_prepare_skill_hints_for_roots(
        Some("러스트 MCP embedding 문제"),
        Some("crates/codelens-mcp/src/main.rs"),
        std::slice::from_ref(&root),
    );

    assert_eq!(hints["total_skill_count"], json!(1));
    assert!(
        hints["candidate_skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|skill| skill["name"] == "rust-host-skill")
    );
}

#[test]
fn codex_skill_recommendations_rank_metadata_matches() {
    let root = temp_dir("recommend");
    let rust_skill = root.join(".codex/skills/rust/SKILL.md");
    let frontend_skill = root.join(".codex/skills/frontend/SKILL.md");
    std::fs::create_dir_all(rust_skill.parent().unwrap()).unwrap();
    std::fs::create_dir_all(frontend_skill.parent().unwrap()).unwrap();
    std::fs::write(
        &rust_skill,
        r#"---
name: rust-codelens
description: Use for Rust MCP semantic embedding recovery and cargo tests.
---
"#,
    )
    .unwrap();
    std::fs::write(
        &frontend_skill,
        r#"---
name: frontend
description: Use for React UI work.
---
"#,
    )
    .unwrap();

    let roots = discover_codex_skill_roots_from_home(&root);
    let candidates = recommend::recommend_codex_skills_for_roots(
        &roots,
        Some("러스트 MCP semantic embedding 문제"),
        Some("crates/codelens-mcp/src/main.rs"),
        3,
    );

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["name"], json!("rust-codelens"));
    assert!(candidates[0]["content_hash"].is_string());
    assert!(
        candidates[0]["matched_terms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|term| term == "rust")
    );
}

#[test]
fn skill_recommendations_ignore_common_words_and_installation_root_names() {
    let root = temp_dir("codex-root-noise");
    let android = root.join("android-performance/SKILL.md");
    let codelens = root.join("codelens/SKILL.md");
    for path in [&android, &codelens] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    std::fs::write(
        android,
        "---\nname: android-performance\ndescription: Use for Android performance and profiling.\n---\n",
    )
    .unwrap();
    std::fs::write(
        codelens,
        "---\nname: codelens\ndescription: CodeLens structural analysis.\n---\n",
    )
    .unwrap();

    let candidates = recommend::recommend_codex_skills_for_roots(
        std::slice::from_ref(&root),
        Some("CodeLens for Codex and Claude"),
        None,
        3,
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["name"], "codelens");
    assert_eq!(candidates[0]["matched_terms"], json!(["codelens"]));
    let korean = recommend::recommend_codex_skills_for_roots(
        std::slice::from_ref(&root),
        Some("코드렌즈를 코덱스와 클로드에 적용"),
        None,
        3,
    );
    assert_eq!(korean.len(), 1);
    assert_eq!(korean[0]["name"], "codelens");
    assert!(
        recommend::recommend_codex_skills_for_roots(&[root], Some("and for the with"), None, 3)
            .is_empty()
    );
}

#[test]
fn skill_shortlist_deduplicates_identical_copies_without_hiding_distinct_skills() {
    let root = temp_dir("duplicates");
    let user_root = root.join("z-user-skills");
    let cache_root = root.join("a-plugin-cache");
    let metadata = "---\nname: rust-codelens\ndescription: Rust CodeLens analysis.\n---\n";
    for (base, name, body) in [
        (&user_root, "primary", metadata),
        (&cache_root, "old-release", metadata),
        (&cache_root, "new-release", metadata),
        (
            &cache_root,
            "different",
            "---\nname: rust-tests\ndescription: Rust tests.\n---\n",
        ),
    ] {
        let path = base.join(name).join("SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let roots = [user_root.clone(), cache_root];
    let candidates = recommend::recommend_codex_skills_for_roots(&roots, Some("Rust"), None, 3);
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        candidates[0]["source_root"],
        user_root.to_string_lossy().as_ref()
    );
    assert_eq!(candidates[1]["name"], "rust-tests");
    assert_eq!(
        codex_skill_catalog_for_roots(&roots, 8)["total_skill_count"],
        4
    );
    // Alphabetical names must not outrank the user's root at equal relevance.
    let alphabetical = roots[1].join("alphabetical/SKILL.md");
    std::fs::create_dir_all(alphabetical.parent().unwrap()).unwrap();
    std::fs::write(
        alphabetical,
        "---\nname: aaa-rust\ndescription: Rust tests.\n---\n",
    )
    .unwrap();
    let first = recommend::recommend_codex_skills_for_roots(&roots, Some("Rust"), None, 1);
    assert_eq!(
        first[0]["source_root"],
        user_root.to_string_lossy().as_ref()
    );
}

#[test]
fn skill_shortlist_preserves_long_bodies_with_identical_metadata_prefixes() {
    let root = temp_dir("long-bodies");
    let prefix = format!(
        "---\nname: rust-review\ndescription: Rust review.\n---\n{}",
        "x".repeat(metadata::METADATA_READ_LIMIT as usize)
    );
    for (name, suffix) in [("first", "rule A"), ("second", "rule B")] {
        let path = root.join(name).join("SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("{prefix}{suffix}")).unwrap();
    }
    let candidates = recommend::recommend_codex_skills_for_roots(&[root], Some("Rust"), None, 3);
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0]["content_hash"], candidates[1]["content_hash"]);
    assert_ne!(candidates[0]["path"], candidates[1]["path"]);
}
