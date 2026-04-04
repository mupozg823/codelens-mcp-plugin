#!/usr/bin/env python3
"""Export machine-readable CodeLens harness routing policy from evaluation reports."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

import agent_registry as agents
import harness_eval_common as common


DEFAULT_REPORT = (
    Path.home() / ".codex" / "harness" / "reports" / "2026-04-03-codelens-eval-cross-repo-release.json"
)
DEFAULT_OUTPUT_DIR = agents.SHARED_POLICY_DIR
SHARED_CANONICAL_JSON = agents.SHARED_POLICY["canonical_policy_json"]
SHARED_CANONICAL_MD = agents.SHARED_POLICY["canonical_policy_markdown"]


POLICY_EXPLANATIONS = {
    "prefer_routed_codelens": "Start with deferred workflow tools and expand evidence or primitive tiers only if needed.",
    "prefer_codelens_after_bootstrap": "Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy.",
    "prefer_naive_codelens": "A direct composite call is worthwhile, but routed session bootstrap is not required.",
    "prefer_native_baseline": "Stay on native rg/read/test by default and escalate to CodeLens only if the task broadens.",
    "avoid_codelens_for_simple_local_lookup": "Do not bootstrap CodeLens for point lookups or already-local single-file edits.",
    "native_or_naive_both_ok_but_default_native": "Native is the default, but an opportunistic direct CodeLens call is acceptable if it avoids extra manual search.",
}


def load_report(path: Path):
    return json.loads(path.read_text())


def with_agent_suffix(path: Path, agent: str):
    return path.with_name(f"{path.stem}-{agent}{path.suffix}")


def choose_global_policy(items):
    counter = Counter(item["recommended_policy"] for item in items)
    policy, votes = counter.most_common(1)[0]
    unanimous = votes == len(items)
    return {
        "recommended_policy": policy,
        "consensus": "unanimous" if unanimous else "majority",
        "repo_count": len(items),
        "vote_count": votes,
    }


def build_policy(report, task_summaries, *, policy_scope: str, agent: str | None = None):
    task_groups = defaultdict(list)
    repo_overrides = []
    for item in task_summaries:
        task_groups[item["task_kind"]].append(item)

    global_rules = []
    for task_kind, items in sorted(task_groups.items()):
        summary = choose_global_policy(items)
        rule = {
            "task_kind": task_kind,
            "recommended_policy": summary["recommended_policy"],
            "consensus": summary["consensus"],
            "repo_count": summary["repo_count"],
            "vote_count": summary["vote_count"],
            "explanation": POLICY_EXPLANATIONS.get(summary["recommended_policy"], ""),
        }
        global_rules.append(rule)

        for item in items:
            if item["recommended_policy"] != summary["recommended_policy"]:
                repo_overrides.append(
                    {
                        "repo_id": item["repo_id"],
                        "repo_label": item["repo_label"],
                        "task_kind": task_kind,
                        "recommended_policy": item["recommended_policy"],
                        "confidence": item.get("confidence", "unknown"),
                        "explanation": POLICY_EXPLANATIONS.get(item["recommended_policy"], ""),
                    }
                )

    return {
        "schema_version": "codelens-routing-policy-v2",
        "policy_scope": policy_scope,
        "agent": agent,
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "source_report": report.get("source_report") or report.get("generated_at"),
        "source_report_path": report.get("_source_path"),
        "binary": report.get("binary"),
        "global_rules": global_rules,
        "repo_overrides": repo_overrides,
    }


def render_markdown(policy):
    lines = []
    a = lines.append
    a("# CodeLens Harness Routing Policy")
    a("")
    scope_label = policy.get("policy_scope", "shared")
    agent = policy.get("agent")
    a("| Field | Value |")
    a("|---|---|")
    a(f"| Scope | {scope_label} |")
    a(f"| Agent | {agent or 'shared'} |")
    a(f"| Source report | {policy.get('source_report_path') or policy.get('source_report')} |")
    a(f"| Binary | {policy.get('binary', 'unknown')} |")
    a(f"| Generated at | {policy.get('generated_at')} |")
    a("")
    a("## Global Rules")
    a("")
    a("| Task Kind | Policy | Consensus | Explanation |")
    a("|---|---|---|---|")
    for rule in policy["global_rules"]:
        a(
            f"| {rule['task_kind']} | {rule['recommended_policy']} | {rule['consensus']} ({rule['vote_count']}/{rule['repo_count']}) | {rule['explanation']} |"
        )
    a("")
    a("## Repo Overrides")
    a("")
    if policy["repo_overrides"]:
        a("| Repo | Task Kind | Policy | Confidence | Explanation |")
        a("|---|---|---|---|---|")
        for override in policy["repo_overrides"]:
            a(
                f"| {override['repo_label']} | {override['task_kind']} | {override['recommended_policy']} | {override['confidence']} | {override['explanation']} |"
            )
    else:
        a("- no repo-specific overrides")
    a("")
    a("## Suggested AGENTS Snippet")
    a("")
    for rule in policy["global_rules"]:
        a(f"- `{rule['task_kind']}`: `{rule['recommended_policy']}`")
        a(f"  {rule['explanation']}")
    if policy["repo_overrides"]:
        a("")
        a("Repo-specific exceptions:")
        for override in policy["repo_overrides"]:
            a(
                f"- `{override['repo_label']} / {override['task_kind']}`: `{override['recommended_policy']}`"
            )
            a(f"  {override['explanation']}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", default=str(DEFAULT_REPORT))
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    parser.add_argument("--label", default="codelens-routing-policy")
    parser.add_argument("--skip-canonical-write", action="store_true")
    parser.add_argument("--skip-claude-mirror", action="store_true")
    args = parser.parse_args()

    report_path = Path(args.input).expanduser()
    report = load_report(report_path)
    report["_source_path"] = str(report_path)
    shared_policy = build_policy(
        report,
        report.get("task_summaries", []),
        policy_scope="shared",
    )
    agent_task_summaries = report.get("agent_task_summaries") or {}
    agent_policies = {
        agent: build_policy(
            report,
            agent_task_summaries.get(agent) or report.get("task_summaries", []),
            policy_scope="agent",
            agent=agent,
        )
        for agent in agents.agent_names()
    }

    DEFAULT_OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y-%m-%d")
    base_name = f"{stamp}-{common.slugify(args.label)}"
    output_json = Path(args.output_json).expanduser() if args.output_json else DEFAULT_OUTPUT_DIR / f"{base_name}.json"
    output_md = Path(args.output_md).expanduser() if args.output_md else DEFAULT_OUTPUT_DIR / f"{base_name}.md"
    codex_output_json = with_agent_suffix(output_json, "codex")
    codex_output_md = with_agent_suffix(output_md, "codex")
    claude_output_json = with_agent_suffix(output_json, "claude")
    claude_output_md = with_agent_suffix(output_md, "claude")
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    shared_json_text = json.dumps(shared_policy, ensure_ascii=False, indent=2) + "\n"
    shared_md_text = render_markdown(shared_policy)
    output_json.write_text(shared_json_text)
    output_md.write_text(shared_md_text)
    agent_outputs = {}
    for agent, policy in agent_policies.items():
        agent_cfg = agents.get_agent(agent)
        agent_output_json = with_agent_suffix(output_json, agent)
        agent_output_md = with_agent_suffix(output_md, agent)
        agent_json_text = json.dumps(policy, ensure_ascii=False, indent=2) + "\n"
        agent_md_text = render_markdown(policy)
        agent_output_json.write_text(agent_json_text)
        agent_output_md.write_text(agent_md_text)
        agent_outputs[agent] = {
            "policy_json": str(agent_output_json),
            "policy_markdown": str(agent_output_md),
        }
        if not args.skip_canonical_write:
            if agent == "claude" and args.skip_claude_mirror:
                continue
            Path(agent_cfg["policy_output_dir"]).mkdir(parents=True, exist_ok=True)
            Path(agent_cfg["canonical_policy_json"]).write_text(agent_json_text)
            Path(agent_cfg["canonical_policy_markdown"]).write_text(agent_md_text)
    if not args.skip_canonical_write:
        SHARED_CANONICAL_JSON.write_text(shared_json_text)
        SHARED_CANONICAL_MD.write_text(shared_md_text)

    payload = {
        "policy_json": str(output_json),
        "policy_markdown": str(output_md),
        "shared_policy_json": str(output_json),
        "shared_policy_markdown": str(output_md),
        "shared_canonical_json": None if args.skip_canonical_write else str(SHARED_CANONICAL_JSON),
        "shared_canonical_markdown": None if args.skip_canonical_write else str(SHARED_CANONICAL_MD),
        "agent_policy_outputs": agent_outputs,
        "agent_canonical_outputs": {
            agent: {
                "policy_json": None
                if args.skip_canonical_write or (agent == "claude" and args.skip_claude_mirror)
                else str(agents.get_agent(agent)["canonical_policy_json"]),
                "policy_markdown": None
                if args.skip_canonical_write or (agent == "claude" and args.skip_claude_mirror)
                else str(agents.get_agent(agent)["canonical_policy_markdown"]),
            }
            for agent in agents.agent_names()
        },
        "global_rules": len(shared_policy["global_rules"]),
        "repo_overrides": len(shared_policy["repo_overrides"]),
    }
    for agent, paths in agent_outputs.items():
        payload[f"{agent}_policy_json"] = paths["policy_json"]
        payload[f"{agent}_policy_markdown"] = paths["policy_markdown"]
        payload[f"{agent}_canonical_json"] = payload["agent_canonical_outputs"][agent]["policy_json"]
        payload[f"{agent}_canonical_markdown"] = payload["agent_canonical_outputs"][agent]["policy_markdown"]
    if "codex" in agent_outputs:
        payload["canonical_json"] = payload["codex_canonical_json"]
        payload["canonical_markdown"] = payload["codex_canonical_markdown"]
    print(json.dumps(payload, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
