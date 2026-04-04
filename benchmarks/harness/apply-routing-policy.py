#!/usr/bin/env python3
"""Apply exported CodeLens routing policy to global AGENTS and repo override snippets."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from datetime import datetime
from pathlib import Path

import agent_registry as agents

SCRIPT_DIR = Path(__file__).resolve().parent
BENCH_DIR = SCRIPT_DIR.parent
DEFAULT_CODEX_POLICY = Path(agents.get_agent("codex")["canonical_policy_json"])
DEFAULT_CLAUDE_POLICY = Path(agents.get_agent("claude")["canonical_policy_json"])
DEFAULT_AGENTS = Path(agents.get_agent("codex")["global_instruction_path"])
DEFAULT_CLAUDE = Path(agents.get_agent("claude")["global_instruction_path"])
DEFAULT_OVERRIDE_DIR = agents.DEFAULT_OVERRIDE_DIR
DEFAULT_REPO_CONFIG = BENCH_DIR / "harness-eval-config.json"
BEGIN_MARKER = "<!-- CODELENS_ROUTING_POLICY:BEGIN -->"
END_MARKER = "<!-- CODELENS_ROUTING_POLICY:END -->"
REPO_BEGIN_MARKER = "<!-- CODELENS_REPO_ROUTING_POLICY:BEGIN -->"
REPO_END_MARKER = "<!-- CODELENS_REPO_ROUTING_POLICY:END -->"
CLAUDE_BEGIN_MARKER = "<!-- CODELENS_CLAUDE_ROUTING_POLICY:BEGIN -->"
CLAUDE_END_MARKER = "<!-- CODELENS_CLAUDE_ROUTING_POLICY:END -->"
REPO_CLAUDE_BEGIN_MARKER = "<!-- CODELENS_REPO_CLAUDE_ROUTING_POLICY:BEGIN -->"
REPO_CLAUDE_END_MARKER = "<!-- CODELENS_REPO_CLAUDE_ROUTING_POLICY:END -->"


def load_policy(path: Path):
    return json.loads(path.read_text())


def render_policy_section(policy):
    agent = policy.get("agent") or "codex"
    agent_cfg = agents.get_agent(agent)
    lines = [
        BEGIN_MARKER,
        "## CodeLens Routing Policy",
        "",
        f"_Generated from `{policy.get('source_report_path', policy.get('source_report', 'unknown'))}` on {policy.get('generated_at', datetime.now().isoformat(timespec='seconds'))}_",
        "",
        f"_Policy target: `{agent}`_",
        "_Derived from the authoritative policy JSON for this agent. This markdown is non-authoritative and must not be used as policy input._",
        "",
        "Global rules:",
    ]
    for rule in policy.get("global_rules", []):
        lines.append(f"- `{rule['task_kind']}` → `{rule['recommended_policy']}`")
        lines.append(f"  {rule.get('explanation', '')}")

    overrides = policy.get("repo_overrides", [])
    if overrides:
        lines.extend(["", "Repo-specific exceptions:"])
        for override in overrides:
            lines.append(
                f"- `{override['repo_label']} / {override['task_kind']}` → `{override['recommended_policy']}`"
            )
            lines.append(f"  {override.get('explanation', '')}")

    lines.extend(
        [
            "",
            "Operational guidance:",
            "- reviewer/impact/refactor preflight tasks: native first step is allowed, then escalate to CodeLens workflow tools once the task clearly spans multiple files or risk boundaries.",
            "- simple local lookup/edit: native `rg/read/test` remains the default path.",
            "- do not open full CodeLens tool surface unless the task has clearly crossed the routing threshold.",
            f"- codex global defaults still come from `{agent_cfg['global_instruction_label']}`.",
            END_MARKER,
            "",
        ]
    )
    return "\n".join(lines)


def render_claude_policy_section(policy):
    agent = policy.get("agent") or "claude"
    agent_cfg = agents.get_agent(agent)
    lines = [
        CLAUDE_BEGIN_MARKER,
        "## CodeLens Routing Policy",
        "",
        f"_Generated from `{policy.get('source_report_path', policy.get('source_report', 'unknown'))}` on {policy.get('generated_at', datetime.now().isoformat(timespec='seconds'))}_",
        "",
        f"_Policy target: `{agent}`_",
        "_Derived from the authoritative policy JSON for this agent. This markdown is non-authoritative and must not be used as policy input._",
        "",
        "Global rules:",
    ]
    for rule in policy.get("global_rules", []):
        lines.append(f"- `{rule['task_kind']}` → `{rule['recommended_policy']}`")
        lines.append(f"  {rule.get('explanation', '')}")
    overrides = policy.get("repo_overrides", [])
    if overrides:
        lines.extend(["", "Repo-specific exceptions:"])
        for override in overrides:
            lines.append(
                f"- `{override['repo_label']} / {override['task_kind']}` → `{override['recommended_policy']}`"
            )
            lines.append(f"  {override.get('explanation', '')}")
    lines.extend(
        [
            "",
            "Claude harness guidance:",
            "- for multi-file review or refactor preflight, prefer CodeLens workflow tools after the first concrete local step.",
            "- for simple local lookup/edit, stay on native read/grep unless the task broadens.",
            "- for code exploration subagents, do not use Explore; use CodeLens-aware agents or explicit CodeLens instructions.",
            f"- claude global defaults still come from `{agent_cfg['global_instruction_label']}`.",
            CLAUDE_END_MARKER,
            "",
        ]
    )
    return "\n".join(lines)


def apply_marked_section(
    existing: str, new_section: str, begin_marker: str, end_marker: str
):
    if begin_marker in existing and end_marker in existing:
        start = existing.index(begin_marker)
        end = existing.index(end_marker) + len(end_marker)
        updated = existing[:start].rstrip() + "\n\n" + new_section + existing[end:]
        return updated
    trimmed = existing.rstrip()
    if trimmed:
        return trimmed + "\n\n" + new_section
    return new_section


def render_override_snippet(repo_label, repo_id, overrides, agent: str):
    lines = [
        f"# CodeLens Override: {repo_label}",
        "",
        f"- Policy target: `{agent}`",
        f"- Repo id: `{repo_id}`",
        "- Derived from authoritative policy JSON; reference only.",
        "",
        "Task-specific overrides:",
    ]
    if overrides:
        for override in overrides:
            lines.extend(
                [
                    f"- `{override['task_kind']}` → `{override['recommended_policy']}`",
                    f"  Confidence: `{override.get('confidence', 'unknown')}`",
                    f"  {override.get('explanation', '')}",
                ]
            )
    else:
        lines.append("- no repo-specific overrides; follow the global routing policy.")
    lines.extend(["", "Suggested AGENTS insertion:"])
    if overrides:
        for override in overrides:
            lines.extend(
                [
                    f"- `{override['task_kind']}`: `{override['recommended_policy']}`",
                    f"  {override.get('explanation', '')}",
                ]
            )
    else:
        lines.append("- no repo-specific insertion needed")
    lines.append("")
    return "\n".join(lines)


def load_repo_map(path: Path):
    import harness_eval_common as common

    config = json.loads(path.read_text())
    return {
        common.normalize_repo_id(repo): repo
        for repo in config.get("representative_repos", [])
    }


def render_repo_policy_section(policy, repo_label, repo_id, overrides):
    lines = [
        REPO_BEGIN_MARKER,
        "## CodeLens Repo Routing Policy",
        "",
        (
            f"_Generated from `{policy.get('source_report_path', policy.get('source_report', 'unknown'))}` "
            f"on {policy.get('generated_at', datetime.now().isoformat(timespec='seconds'))} for `{repo_id}`_"
        ),
        "",
        "_Derived from the authoritative Codex policy JSON. This repo section is non-authoritative._",
        "",
        "Repo-specific routing rules:",
    ]
    if overrides:
        for override in overrides:
            lines.extend(
                [
                    f"- `{override['task_kind']}` → `{override['recommended_policy']}`",
                    f"  {override.get('explanation', '')}",
                ]
            )
    else:
        lines.append(
            "- no repo-specific exceptions; follow the global CodeLens routing policy."
        )
    lines.extend(
        [
            "",
            "Operational guidance:",
            "- prefer the global CodeLens routing policy unless a repo-specific rule above is more restrictive.",
            "- keep simple point lookups on native rg/read/test when the repo rule says native is preferred.",
            "- use verifier-first CodeLens workflow for refactor/impact tasks only when the routing threshold is crossed.",
            REPO_END_MARKER,
            "",
        ]
    )
    return "\n".join(lines)


def render_repo_claude_policy_section(policy, repo_label, repo_id, overrides):
    lines = [
        REPO_CLAUDE_BEGIN_MARKER,
        "## CodeLens Repo Routing Policy",
        "",
        (
            f"_Generated from `{policy.get('source_report_path', policy.get('source_report', 'unknown'))}` "
            f"on {policy.get('generated_at', datetime.now().isoformat(timespec='seconds'))} for `{repo_id}`_"
        ),
        "",
        "_Derived from the authoritative Claude policy JSON. This repo section is non-authoritative._",
        "",
        "Repo-specific routing rules:",
    ]
    if overrides:
        for override in overrides:
            lines.extend(
                [
                    f"- `{override['task_kind']}` → `{override['recommended_policy']}`",
                    f"  {override.get('explanation', '')}",
                ]
            )
    else:
        lines.append(
            "- no repo-specific exceptions; follow the global CodeLens routing policy."
        )
    lines.extend(
        [
            "",
            "Claude harness guidance:",
            "- on complex tasks, use the repo and global CLAUDE instructions before selecting a harness pattern.",
            "- keep simple point lookups native when the policy says native is preferred.",
            "- use CodeLens-aware exploration for multi-file or reviewer-heavy work.",
            REPO_CLAUDE_END_MARKER,
            "",
        ]
    )
    return "\n".join(lines)


def render_bootstrap_agents(repo):
    lines = [
        f"# {repo['label']}",
        "",
        "## Codex Harness Bootstrap",
        "",
        "This AGENTS.md was bootstrap-generated to attach minimal verification defaults and repo-local CodeLens routing guidance.",
        "The authoritative routing rules remain in policy JSON; this file is derived guidance only.",
        "Global defaults still come from `~/.codex/AGENTS.md`.",
        "",
        "## Verification",
        "",
        "Before finishing, run:",
    ]
    for command in repo.get("verify_commands", []):
        lines.append(f"- `{command}`")
    lines.extend(
        [
            "",
            "## Stack",
            "",
            f"- `{repo.get('stack', 'unknown')}`",
            "",
            "## Local Guidance",
            "",
            "- Keep diffs minimal and prefer existing modules over new wrappers.",
            "- Use native `rg/read/test` for trivial point lookups.",
            "- Escalate to CodeLens workflow tools only when the task becomes multi-file, reviewer-heavy, or refactor-sensitive.",
            "",
        ]
    )
    return "\n".join(lines)


def render_bootstrap_claude(repo):
    lines = [
        f"# {repo['label']}",
        "",
        "## Codex/Claude Harness Bootstrap",
        "",
        "This CLAUDE.md was bootstrap-generated to attach minimal verification defaults and repo-local CodeLens routing guidance.",
        "The authoritative routing rules remain in policy JSON; this file is derived guidance only.",
        "Global defaults still come from `~/.claude/CLAUDE.md`.",
        "",
        "## Verification",
        "",
        "Before finishing, run:",
    ]
    for command in repo.get("verify_commands", []):
        lines.append(f"- `{command}`")
    lines.extend(
        [
            "",
            "## Local Guidance",
            "",
            "- Prefer CodeLens workflow tools for multi-file impact analysis, reviewer tasks, and refactor preflight.",
            "- Keep simple point lookups on native read/grep when the task is already local and unambiguous.",
            "- For complex tasks, choose a harness pattern after reading global and local CLAUDE instructions.",
            "",
        ]
    )
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", default=str(DEFAULT_CODEX_POLICY))
    parser.add_argument("--claude-policy", default=str(DEFAULT_CLAUDE_POLICY))
    parser.add_argument("--agents-file", default=str(DEFAULT_AGENTS))
    parser.add_argument("--claude-file", default=str(DEFAULT_CLAUDE))
    parser.add_argument("--override-dir", default=str(DEFAULT_OVERRIDE_DIR))
    parser.add_argument("--repo-config", default=str(DEFAULT_REPO_CONFIG))
    parser.add_argument("--bootstrap-missing-agents", action="store_true")
    parser.add_argument("--bootstrap-missing-claude", action="store_true")
    args = parser.parse_args()

    policy_path = Path(args.policy).expanduser()
    claude_policy_path = Path(args.claude_policy).expanduser()
    agents_path = Path(args.agents_file).expanduser()
    claude_path = Path(args.claude_file).expanduser()
    override_dir = Path(args.override_dir).expanduser()
    repo_config_path = Path(args.repo_config).expanduser()

    policy = load_policy(policy_path)
    claude_policy = load_policy(claude_policy_path)
    repo_map = load_repo_map(repo_config_path)
    agents_text = agents_path.read_text() if agents_path.exists() else ""
    section = render_policy_section(policy)
    updated_agents = apply_marked_section(
        agents_text, section, BEGIN_MARKER, END_MARKER
    )
    agents_path.parent.mkdir(parents=True, exist_ok=True)
    agents_path.write_text(updated_agents)

    claude_text = claude_path.read_text() if claude_path.exists() else ""
    claude_section = render_claude_policy_section(claude_policy)
    updated_claude = apply_marked_section(
        claude_text, claude_section, CLAUDE_BEGIN_MARKER, CLAUDE_END_MARKER
    )
    claude_path.parent.mkdir(parents=True, exist_ok=True)
    claude_path.write_text(updated_claude)

    override_dir.mkdir(parents=True, exist_ok=True)
    grouped_overrides = defaultdict(list)
    for override in policy.get("repo_overrides", []):
        grouped_overrides[override["repo_id"]].append(override)
    grouped_claude_overrides = defaultdict(list)
    for override in claude_policy.get("repo_overrides", []):
        grouped_claude_overrides[override["repo_id"]].append(override)

    override_paths = []
    claude_override_paths = []
    repo_agents_paths = []
    bootstrapped_repo_agents = []
    repo_claude_paths = []
    bootstrapped_repo_claude = []
    skipped_repo_overrides = []
    repo_ids = set(grouped_overrides.keys()) | set(grouped_claude_overrides.keys())
    if args.bootstrap_missing_agents or args.bootstrap_missing_claude:
        repo_ids = set(repo_map.keys())
    for repo_id in sorted(repo_ids):
        overrides = grouped_overrides.get(repo_id, [])
        claude_overrides = grouped_claude_overrides.get(repo_id, [])
        path = (
            override_dir
            / f"{repo_id}-{agents.get_agent('codex')['override_suffix']}.md"
        )
        claude_override_path = (
            override_dir
            / f"{repo_id}-{agents.get_agent('claude')['override_suffix']}.md"
        )
        repo = repo_map.get(repo_id)
        if not repo:
            skipped_repo_overrides.append(
                {
                    "repo_id": repo_id,
                    "reason": f"repo id missing from {repo_config_path}",
                }
            )
            continue

        repo_label = overrides[0]["repo_label"] if overrides else repo["label"]
        path.write_text(
            render_override_snippet(repo_label, repo_id, overrides, "codex")
        )
        override_paths.append(str(path))
        claude_override_path.write_text(
            render_override_snippet(
                (
                    claude_overrides[0]["repo_label"]
                    if claude_overrides
                    else repo["label"]
                ),
                repo_id,
                claude_overrides,
                "claude",
            )
        )
        claude_override_paths.append(str(claude_override_path))

        repo_agents_path = Path(repo["path"]) / str(
            agents.get_agent("codex")["repo_instruction_name"]
        )
        if not repo_agents_path.exists():
            if not args.bootstrap_missing_agents:
                skipped_repo_overrides.append(
                    {
                        "repo_id": repo_id,
                        "reason": f"{repo_agents_path} does not exist",
                    }
                )
                continue
            repo_agents_path.write_text(render_bootstrap_agents(repo))
            bootstrapped_repo_agents.append(str(repo_agents_path))

        repo_agents_text = repo_agents_path.read_text()
        repo_section = render_repo_policy_section(
            policy,
            repo_label,
            repo_id,
            overrides,
        )
        repo_agents_path.write_text(
            apply_marked_section(
                repo_agents_text,
                repo_section,
                REPO_BEGIN_MARKER,
                REPO_END_MARKER,
            )
        )
        repo_agents_paths.append(str(repo_agents_path))

        repo_claude_path = Path(repo["path"]) / str(
            agents.get_agent("claude")["repo_instruction_name"]
        )
        if not repo_claude_path.exists():
            if not args.bootstrap_missing_claude:
                skipped_repo_overrides.append(
                    {
                        "repo_id": repo_id,
                        "reason": f"{repo_claude_path} does not exist",
                    }
                )
            else:
                repo_claude_path.write_text(render_bootstrap_claude(repo))
                bootstrapped_repo_claude.append(str(repo_claude_path))

        if repo_claude_path.exists():
            repo_claude_text = repo_claude_path.read_text()
            repo_claude_section = render_repo_claude_policy_section(
                claude_policy,
                (
                    claude_overrides[0]["repo_label"]
                    if claude_overrides
                    else repo["label"]
                ),
                repo_id,
                claude_overrides,
            )
            repo_claude_path.write_text(
                apply_marked_section(
                    repo_claude_text,
                    repo_claude_section,
                    REPO_CLAUDE_BEGIN_MARKER,
                    REPO_CLAUDE_END_MARKER,
                )
            )
            repo_claude_paths.append(str(repo_claude_path))

    print(
        json.dumps(
            {
                "codex_policy": str(policy_path),
                "claude_policy": str(claude_policy_path),
                "agents_file": str(agents_path),
                "claude_file": str(claude_path),
                "override_dir": str(override_dir),
                "override_files": override_paths,
                "claude_override_files": claude_override_paths,
                "repo_agents_files": repo_agents_paths,
                "bootstrapped_repo_agents": bootstrapped_repo_agents,
                "repo_claude_files": repo_claude_paths,
                "bootstrapped_repo_claude": bootstrapped_repo_claude,
                "skipped_repo_overrides": skipped_repo_overrides,
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
