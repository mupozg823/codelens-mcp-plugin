//! I5.2 — cross-host skill first-step matrix.
//!
//! `docs/design/runtime-convergence-execution-plan.md` §E5 requires a
//! "first-step failure count 0 for every packaged skill × default profile"
//! across the claude-code / codex / generic host contexts on the CORE-20
//! default surface.
//!
//! Every packaged skill under `skills/` names, in its `## Workflow` section, the
//! tool an agent has to call first. This module runs exactly that call through
//! the real `tools/call` path (router → dispatch → access gates) once per host
//! context, with the session envelope each host actually gets:
//!
//! | host context | client profile | default preset | deferred loading |
//! | ------------ | -------------- | -------------- | ---------------- |
//! | claude-code  | Claude         | Balanced       | on               |
//! | codex        | Codex          | Minimal        | on               |
//! | generic      | Generic        | Balanced       | off              |
//!
//! The assertion is scoped to *availability*: the ADR-0016 surface-listing gate,
//! the deferred-loading namespace/tier gates, the experimental gate, the
//! read-only / daemon-mode / trusted-client gates, the role gate, and the
//! unknown-tool path — every one of which runs in `dispatch::access` *before*
//! the handler. Domain outcomes are deliberately not asserted: the temp fixture
//! has no git history and no populated index, so "no commits" is a legitimate
//! answer that still proves the cell is reachable.

use super::{parse_tool_response, project_root};
use crate::client_profile::ClientProfile;
use crate::server::router::handle_request;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// One packaged skill under `skills/`.
struct PackagedSkill {
    /// Directory name under `skills/`.
    dir: &'static str,
    /// Frontmatter `name:` value.
    name: &'static str,
    /// The tool the `## Workflow` section tells the agent to call first.
    /// Cross-checked against the shipped SKILL.md by
    /// `packaged_skill_first_steps_match_skill_docs`.
    first_tool: &'static str,
    /// Arguments for that first call.
    first_args: fn(&Path) -> Value,
}

fn analyze_first_args(_project: &Path) -> Value {
    json!({ "mode": "architecture" })
}

fn review_first_args(_project: &Path) -> Value {
    json!({ "ref": "HEAD" })
}

fn onboard_first_args(project: &Path) -> Value {
    json!({ "project": project.to_string_lossy() })
}

const PACKAGED_SKILLS: &[PackagedSkill] = &[
    PackagedSkill {
        dir: "analyze",
        name: "codelens-analyze",
        first_tool: "review",
        first_args: analyze_first_args,
    },
    PackagedSkill {
        dir: "code-review",
        name: "codelens-review",
        first_tool: "get_changed_files",
        first_args: review_first_args,
    },
    PackagedSkill {
        dir: "onboard",
        name: "codelens-onboard",
        first_tool: "activate_project",
        first_args: onboard_first_args,
    },
];

/// Host contexts under test. The client name is what an MCP `initialize` would
/// report; `ClientProfile::detect_request` derives the profile (and therefore
/// the default preset + deferred-loading default) from the pair.
struct HostContextCell {
    host_context: &'static str,
    client_name: &'static str,
}

const HOST_CONTEXTS: &[HostContextCell] = &[
    HostContextCell {
        host_context: "claude-code",
        client_name: "claude-code",
    },
    HostContextCell {
        host_context: "codex",
        client_name: "codex-cli",
    },
    // Unrecognized host string → `ClientProfile::Generic`, the envelope every
    // non-Claude / non-Codex MCP client lands in.
    HostContextCell {
        host_context: "generic",
        client_name: "generic-mcp-client",
    },
];

/// Error fragments emitted by the pre-handler availability gates
/// (`dispatch/access.rs`, `tools/mod.rs` unknown-tool path, verb facade
/// registration check). Any of these in a first-step response is an I5.2
/// failure; anything else is a domain outcome and out of scope here.
const AVAILABILITY_DENIALS: &[&str] = &[
    "not available in active surface",
    "hidden by deferred loading",
    "requires experimental feature",
    "blocked in read-only surface",
    "blocked by daemon mode",
    "requires a trusted HTTP client",
    "Unknown tool",
    "is not registered in this build",
    "Permission denied",
];

/// Repository root — two levels up from `crates/codelens-mcp`.
fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crate is nested two levels under the repo root")
        .to_path_buf()
}

fn skill_doc(skill: &PackagedSkill) -> String {
    let path = repo_root().join("skills").join(skill.dir).join("SKILL.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The first `` Call `tool` `` reference inside the `## Workflow` section.
fn documented_first_tool(skill_md: &str) -> Option<String> {
    let workflow = skill_md.split("## Workflow").nth(1)?;
    let re = regex::Regex::new(r"Call `([a-z][a-z0-9_]*)`").expect("static regex");
    re.captures(workflow).map(|caps| caps[1].to_owned())
}

fn documented_skill_name(skill_md: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?m)^name:\s*(\S+)\s*$").expect("static regex");
    re.captures(skill_md).map(|caps| caps[1].to_owned())
}

/// Run one matrix cell: the skill's first tool call, on a fresh project, under
/// the session envelope of `host`.
fn run_first_step(host: &HostContextCell, skill: &PackagedSkill) -> Value {
    let profile = ClientProfile::detect_request(Some(host.client_name), Some(host.host_context));
    let project = project_root();
    let state = crate::AppState::new_minimal(project.clone(), profile.default_preset());

    let mut arguments = (skill.first_args)(project.as_path())
        .as_object()
        .cloned()
        .expect("first-step arguments must be a JSON object");
    arguments.insert(
        "_session_id".to_owned(),
        json!(format!("i52-{}-{}", host.host_context, skill.dir)),
    );
    arguments.insert("_session_client_name".to_owned(), json!(host.client_name));
    arguments.insert("_session_host_context".to_owned(), json!(host.host_context));
    // A first step is by definition the session's first call: no namespace or
    // tier has been expanded yet, and no `full:true` listing has happened.
    arguments.insert(
        "_session_deferred_tool_loading".to_owned(),
        json!(profile.default_deferred_tool_loading().unwrap_or(false)),
    );
    arguments.insert("_session_loaded_namespaces".to_owned(), json!([]));
    arguments.insert("_session_loaded_tiers".to_owned(), json!([]));
    arguments.insert("_session_full_tool_exposure".to_owned(), json!(false));

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": skill.first_tool,
                "arguments": Value::Object(arguments),
            })),
        },
    )
    .expect("tools/call must return a response");

    let raw = serde_json::to_value(&response).expect("serialize response");
    let mut payload = parse_tool_response(&response);
    // A protocol-level rejection (unknown tool) never reaches the tool payload.
    if let Some(error) = raw.get("error").and_then(|error| error.get("message")) {
        payload = json!({ "success": false, "error": error });
    }
    payload
}

fn availability_denial(payload: &Value) -> Option<String> {
    if payload["success"] != json!(false) {
        return None;
    }
    let error = payload["error"].as_str().unwrap_or("");
    AVAILABILITY_DENIALS
        .iter()
        .find(|marker| error.contains(*marker))
        .map(|_| error.to_owned())
}

/// I5.2 — the matrix itself. Every (host context × packaged skill) cell must
/// clear the availability gates on that host's default surface.
#[test]
fn packaged_skill_first_steps_are_available_on_every_host_default_surface() {
    let mut failures = Vec::new();
    let mut cells = 0usize;

    for host in HOST_CONTEXTS {
        for skill in PACKAGED_SKILLS {
            cells += 1;
            let payload = run_first_step(host, skill);
            if let Some(error) = availability_denial(&payload) {
                let profile =
                    ClientProfile::detect_request(Some(host.client_name), Some(host.host_context));
                failures.push(format!(
                    "[{host} × {skill} → {tool}] surface={surface:?} profile={profile} \
                     deferred={deferred}: {error}",
                    host = host.host_context,
                    skill = skill.name,
                    tool = skill.first_tool,
                    surface = profile.default_preset(),
                    profile = profile.as_str(),
                    deferred = profile.default_deferred_tool_loading().unwrap_or(false),
                ));
            }
        }
    }

    assert_eq!(
        cells,
        HOST_CONTEXTS.len() * PACKAGED_SKILLS.len(),
        "the matrix must cover every host × skill cell"
    );
    assert!(
        failures.is_empty(),
        "I5.2: packaged-skill first-step availability failures ({}/{cells} cells):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// The `first_tool` constants above are only trustworthy while they agree with
/// the shipped SKILL.md files — otherwise the matrix would verify a tool no
/// agent is actually told to call.
#[test]
fn packaged_skill_first_steps_match_skill_docs() {
    for skill in PACKAGED_SKILLS {
        let text = skill_doc(skill);
        assert_eq!(
            documented_skill_name(&text).as_deref(),
            Some(skill.name),
            "skills/{}/SKILL.md frontmatter name drifted from the matrix constant",
            skill.dir
        );
        assert_eq!(
            documented_first_tool(&text).as_deref(),
            Some(skill.first_tool),
            "skills/{}/SKILL.md workflow step 1 drifted from the matrix constant",
            skill.dir
        );
    }
}

/// The matrix is only complete while it enumerates every packaged skill.
#[test]
fn every_packaged_skill_is_covered_by_the_matrix() {
    let skills_dir = repo_root().join("skills");
    let mut on_disk: Vec<String> = std::fs::read_dir(&skills_dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", skills_dir.display()))
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();

    let mut covered: Vec<String> = PACKAGED_SKILLS
        .iter()
        .map(|skill| skill.dir.to_owned())
        .collect();
    covered.sort();

    assert!(
        !on_disk.is_empty(),
        "no packaged skills discovered under {}",
        skills_dir.display()
    );
    assert_eq!(
        on_disk, covered,
        "packaged skills on disk and the I5.2 matrix roster disagree"
    );
}
