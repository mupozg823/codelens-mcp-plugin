//! Raw host-native adapter template routing.

mod claude_code;
mod cline;
mod codex;
mod cursor;
mod windsurf;

use super::overlays::augment_host_adapter_bundle;
use serde_json::Value;

pub(super) fn raw_host_adapter_bundle(host: &str) -> Option<Value> {
    let mut bundle = match host {
        "claude-code" => claude_code::bundle(),
        "codex" => codex::bundle(),
        "cursor" => cursor::bundle(),
        "cline" => cline::bundle(),
        "windsurf" => windsurf::bundle(),
        _ => return None,
    };

    augment_host_adapter_bundle(host, &mut bundle);
    Some(bundle)
}

#[cfg(test)]
mod routing_policy_budget_tests {
    //! E6.3 regression guard. The attach-generated routing policy is the one
    //! block CodeLens writes into a host's always-on instruction file, so it is
    //! charged against every turn of every session in that project. It must
    //! stay a short invariants-plus-verification contract, and it must not
    //! assign roles, lanes, or agent topology to the host (ADR-0015).

    use super::*;

    /// The three hosts whose routing policy lands in an always-on instruction
    /// file (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/`).
    const POLICY_TARGETS: &[(&str, &str)] = &[
        ("claude-code", "CLAUDE.md"),
        ("codex", "AGENTS.md"),
        ("cursor", ".cursor/rules/codelens-routing.mdc"),
    ];

    const MIN_LINES: usize = 40;
    const MAX_LINES: usize = 60;

    fn policy_template(host: &str, path: &str) -> String {
        let bundle = raw_host_adapter_bundle(host).expect("host adapter bundle");
        bundle["native_files"]
            .as_array()
            .expect("native_files array")
            .iter()
            .find(|file| file["path"] == path)
            .unwrap_or_else(|| panic!("host `{host}` has no native file for `{path}`"))["template"]
            .as_str()
            .unwrap_or_else(|| panic!("host `{host}` template for `{path}` is not text"))
            .to_owned()
    }

    #[test]
    fn routing_policy_blocks_stay_within_the_line_budget() {
        for (host, path) in POLICY_TARGETS {
            let template = policy_template(host, path);
            let lines = template.trim_end().lines().count();
            assert!(
                (MIN_LINES..=MAX_LINES).contains(&lines),
                "host `{host}` routing policy for `{path}` is {lines} lines, outside the {MIN_LINES}..={MAX_LINES} budget",
            );
        }
    }

    #[test]
    fn routing_policy_blocks_assign_no_roles_or_agent_topology() {
        // ADR-0015: the generated contract states invariants and verification
        // commands. Role/lane/topology wording belongs to the host, never to a
        // block CodeLens generates.
        const BANNED: &[&str] = &[
            "agent_role",
            "planner",
            "builder",
            "subagent",
            "reviewer",
            "read-oriented",
            "write-capable",
        ];

        for (host, path) in POLICY_TARGETS {
            let template = policy_template(host, path).to_lowercase();
            for banned in BANNED {
                assert!(
                    !template.contains(banned),
                    "host `{host}` routing policy for `{path}` still assigns roles (found `{banned}`)",
                );
            }
        }
    }

    #[test]
    fn routing_policy_blocks_share_one_invariant_source() {
        // Drift guard: three files, one contract. Each host may add its own
        // heading and verification commands, but the invariants themselves come
        // from a single constant.
        for (host, path) in POLICY_TARGETS {
            let template = policy_template(host, path);
            assert!(
                template.contains(super::super::overlays::HOST_ROUTING_INVARIANTS),
                "host `{host}` routing policy for `{path}` does not embed the shared invariants",
            );
            assert!(
                template.contains(&format!("codelens-mcp doctor {host}")),
                "host `{host}` routing policy for `{path}` omits its verification command",
            );
        }
    }
}
