//! Phase P4 — prescriptive signals contract.
//!
//! `docs/plans/PLAN_extreme-efficiency.md` Pillar 3 promises that a
//! `mutation_ready=caution` verdict carries at least one actionable
//! blocker naming the specific file(s) driving the caution. Before P4,
//! the `impact_report` handler downgraded readiness to caution on high
//! blast radius but emitted an empty `blockers[]`, leaving the harness
//! to re-derive the offending files from `impact_rows` on its own. The
//! two tests below lock in the minimum contract:
//!
//! 1. `impact_report_caution_on_high_blast_radius_emits_blockers` —
//!    high-importer fixture produces at least one blocker string.
//! 2. `impact_report_blockers_reference_actual_dependent_file_paths` —
//!    blocker text mentions a specific dependent file from
//!    `direct_importers`, not a generic "large blast radius" phrase.

use super::*;
use serde_json::json;

fn write_fixture_with_many_importers(
    project: &codelens_engine::ProjectRoot,
    importer_count: usize,
) {
    // Core module with a single public symbol that many files import.
    fs::write(
        project.as_path().join("core.py"),
        "def shared_helper(value):\n    return value * 2\n",
    )
    .unwrap();
    // Generate `importer_count` sibling modules that all pull in
    // `core.shared_helper`. The import-graph analyzer treats each as a
    // direct importer of `core.py`, producing affected_files ≥ threshold
    // and tripping the existing caution branch in `report_verifier`.
    for idx in 0..importer_count {
        let name = format!("consumer_{idx:02}.py");
        let content = format!(
            "from core import shared_helper\n\n\
             def run_{idx:02}():\n    \
                 return shared_helper({idx})\n"
        );
        fs::write(project.as_path().join(name), content).unwrap();
    }
}

#[test]
fn impact_report_caution_on_high_blast_radius_emits_blockers() {
    let project = project_root();
    write_fixture_with_many_importers(&project, 10);

    let state = make_state(&project);
    let payload = call_tool(&state, "impact_report", json!({ "path": "core.py" }));
    assert_eq!(payload["success"], json!(true), "payload={payload}");

    let readiness = &payload["data"]["readiness"];
    let mutation_ready = readiness["mutation_ready"].as_str().unwrap_or("<missing>");

    let impacts = payload["data"]["top_findings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let affected_header = impacts.first().and_then(|v| v.as_str()).unwrap_or("<none>");

    // The fixture is designed so affected_files ≥ 8 (the caution
    // threshold inside `report_verifier::build_verifier_contract`).
    // If this precondition isn't met, the test is measuring the wrong
    // branch — fail loudly with diagnostic context.
    assert!(
        mutation_ready == "caution" || mutation_ready == "blocked",
        "expected caution/blocked readiness on 10-importer fixture; \
         got mutation_ready={mutation_ready}, affected_header={affected_header}, \
         readiness={readiness}"
    );

    let blockers = payload["data"]["blockers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !blockers.is_empty(),
        "caution readiness must surface at least one actionable blocker; \
         blockers={blockers:?}, readiness={readiness}"
    );
}

#[test]
fn impact_report_emits_command_hint_for_touched_crate() {
    // Phase P4-b contract: when the touched path lives under a
    // cargo crate (`crates/<name>/src/...`), the response must
    // surface an executable `command` hint keyed on that crate so
    // the harness can run verification without re-deriving the
    // crate name. We assert on `next_actions_detailed[*].command`
    // so old string-only consumers keep working unchanged.
    let project = project_root();
    // Lay out a miniature cargo-style layout inside the temp project
    // so the heuristic can extract the crate name from the touched
    // path. The impact_report handler only needs the path string;
    // actual cargo metadata is not required.
    fs::create_dir_all(project.as_path().join("crates/fake-crate/src")).unwrap();
    fs::write(
        project.as_path().join("crates/fake-crate/src/lib.rs"),
        "pub fn helper() -> u32 {\n    42\n}\n",
    )
    .unwrap();

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "impact_report",
        json!({ "path": "crates/fake-crate/src/lib.rs" }),
    );
    assert_eq!(payload["success"], json!(true), "payload={payload}");

    let detailed = payload["data"]["next_actions_detailed"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !detailed.is_empty(),
        "next_actions_detailed must be present; payload={payload}"
    );

    let commands: Vec<String> = detailed
        .iter()
        .filter_map(|entry| {
            entry
                .get("command")
                .and_then(|c| c.as_str())
                .map(ToOwned::to_owned)
        })
        .collect();
    assert!(
        commands
            .iter()
            .any(|cmd| cmd.contains("cargo test") && cmd.contains("fake-crate")),
        "expected a `cargo test -p fake-crate` command hint; got commands={commands:?}"
    );
}

#[test]
fn impact_report_blockers_reference_actual_dependent_file_paths() {
    let project = project_root();
    write_fixture_with_many_importers(&project, 10);

    let state = make_state(&project);
    let payload = call_tool(&state, "impact_report", json!({ "path": "core.py" }));
    assert_eq!(payload["success"], json!(true));

    let blockers = payload["data"]["blockers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !blockers.is_empty(),
        "precondition: blockers must be non-empty; payload={payload}"
    );

    // At least one blocker must name a specific dependent file path
    // (e.g. `consumer_03.py`) so the harness can jump straight to a
    // concrete read target instead of parsing impact_rows itself.
    let blocker_texts: Vec<String> = blockers
        .iter()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect();
    let mentions_specific_file = blocker_texts
        .iter()
        .any(|text| (0..10).any(|idx| text.contains(&format!("consumer_{idx:02}.py"))));
    assert!(
        mentions_specific_file,
        "at least one blocker must cite a specific dependent file path; \
         blocker_texts={blocker_texts:?}"
    );
}
