#!/usr/bin/env python3
"""Resolve CodeLens routing policy into a task bootstrap brief."""

from __future__ import annotations

import argparse
import json
from datetime import datetime
from pathlib import Path

import agent_registry as agents
import harness_eval_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
BENCH_DIR = SCRIPT_DIR.parent
DEFAULT_POLICY = Path(agents.get_agent("codex")["canonical_policy_json"])
DEFAULT_CLAUDE_POLICY = Path(agents.get_agent("claude")["canonical_policy_json"])
DEFAULT_REPO_CONFIG = BENCH_DIR / "harness-eval-config.json"
DEFAULT_OUTPUT_DIR = Path(agents.get_agent("codex")["bootstrap_output_dir"])
DEFAULT_CLAUDE_OUTPUT_DIR = Path(agents.get_agent("claude")["bootstrap_output_dir"])


POLICY_RUNTIME = {
    "prefer_routed_codelens": {
        "route_mode": "deferred_workflow_first",
        "use_codelens": "required",
        "native_first": False,
        "deferred_loading": True,
        "summary": "Start with deferred CodeLens workflow tools immediately.",
    },
    "prefer_codelens_after_bootstrap": {
        "route_mode": "native_then_deferred_workflow",
        "use_codelens": "recommended",
        "native_first": True,
        "deferred_loading": True,
        "summary": "Use one native local step first, then switch to deferred CodeLens workflow tools once the task crosses file or risk boundaries.",
    },
    "prefer_naive_codelens": {
        "route_mode": "direct_composite",
        "use_codelens": "recommended",
        "native_first": False,
        "deferred_loading": False,
        "summary": "A direct CodeLens composite call is worthwhile without full deferred bootstrap.",
    },
    "prefer_native_baseline": {
        "route_mode": "native_first_optional_codelens",
        "use_codelens": "optional",
        "native_first": True,
        "deferred_loading": False,
        "summary": "Stay on native rg/read/test by default and escalate to CodeLens only if the task broadens.",
    },
    "avoid_codelens_for_simple_local_lookup": {
        "route_mode": "native_only",
        "use_codelens": "avoid",
        "native_first": True,
        "deferred_loading": False,
        "summary": "Do not bootstrap CodeLens for point lookups or already-local single-file edits.",
    },
    "native_or_naive_both_ok_but_default_native": {
        "route_mode": "native_default_naive_optional",
        "use_codelens": "optional",
        "native_first": True,
        "deferred_loading": False,
        "summary": "Native is the default; a one-shot CodeLens call is acceptable if it avoids manual search churn.",
    },
}

TASK_ENTRYPOINTS = {
    "onboarding/planning": [
        "analyze_change_request",
        "verify_change_readiness",
    ],
    "impact/reviewer": [
        "impact_report",
        "analyze_change_request",
        "get_analysis_section",
    ],
    "refactor preflight": [
        "verify_change_readiness",
        "safe_rename_report",
        "unresolved_reference_check",
        "refactor_safety_report",
    ],
    "simple local lookup/edit": [
        "search_for_pattern",
        "summarize_file",
    ],
}

PLATFORM_DEFAULTS = {
    agent: {
        "global_instructions": str(cfg["global_instruction_path"]),
        "repo_instructions_name": cfg["repo_instruction_name"],
        "default_policy": Path(cfg["canonical_policy_json"]),
        "default_output_dir": Path(cfg["bootstrap_output_dir"]),
    }
    for agent, cfg in agents.AGENT_REGISTRY.items()
}


def load_json(path: Path):
    return common.load_json(path)


def find_repo(repo_map, repo_path: Path):
    repo_path = repo_path.resolve()
    for repo in repo_map.values():
        if Path(repo["path"]).resolve() == repo_path:
            return repo
    return {
        "id": repo_path.name,
        "label": repo_path.name,
        "path": str(repo_path),
        "stack": "unknown",
        "verify_commands": [],
    }


def choose_rule(policy, repo_id: str, task_kind: str):
    for override in policy.get("repo_overrides", []):
        if override["repo_id"] == repo_id and override["task_kind"] == task_kind:
            return {
                "source": "repo_override",
                "recommended_policy": override["recommended_policy"],
                "confidence": override.get("confidence", "medium"),
                "explanation": override.get("explanation", ""),
            }
    for rule in policy.get("global_rules", []):
        if rule["task_kind"] == task_kind:
            return {
                "source": "global_rule",
                "recommended_policy": rule["recommended_policy"],
                "confidence": "high" if rule.get("consensus") == "unanimous" else "medium",
                "explanation": rule.get("explanation", ""),
            }
    return {
        "source": "fallback",
        "recommended_policy": "prefer_native_baseline",
        "confidence": "low",
        "explanation": "No evaluated routing policy matched this task kind; stay native first.",
}


def ordered_entrypoints(task_kind: str, scenario: dict | None):
    entries = list(TASK_ENTRYPOINTS.get(task_kind, []))
    if not scenario:
        return entries
    ordered = []
    seen = set()
    for item in [scenario.get("primary_entrypoint"), *scenario.get("secondary_entrypoints", [])]:
        if item and item not in seen:
            ordered.append(item)
            seen.add(item)
    for item in entries:
        if item not in seen:
            ordered.append(item)
    return ordered


def native_boundary_hint(task_kind: str):
    hints = {
        "impact/reviewer": "one changed-file list or one narrow path confirmation",
        "onboarding/planning": "one module or path confirmation tied to the requested change",
        "refactor preflight": "one symbol/file boundary confirmation before the verifier call",
        "simple local lookup/edit": "one direct file or symbol lookup",
    }
    return hints.get(task_kind, "one narrow local boundary check")


def render_result_budget(result_budget: dict):
    if not result_budget:
        return ""
    return ", ".join(f"{key} <= {value}" for key, value in result_budget.items())


def build_first_actions(task_kind: str, runtime: dict, scenario: dict | None = None):
    actions = []
    evaluation_mode = (scenario or {}).get("task_mode", "")
    workflow_budget = (scenario or {}).get("workflow_budget", {})
    result_budget = (scenario or {}).get("result_budget", {})
    max_native_steps = workflow_budget.get("max_native_steps")
    max_workflow_calls = workflow_budget.get("max_workflow_calls")
    max_evidence_expansions = workflow_budget.get("max_evidence_expansions")
    if runtime["native_first"]:
        if evaluation_mode in {"read-only-eval", "bounded-local-eval"} and max_native_steps:
            actions.append(
                f"Use at most {max_native_steps} native boundary check before any workflow call; keep it to {native_boundary_hint(task_kind)}."
            )
            actions.append(
                "If that boundary check uses `rg`, exclude docs/build/generated noise by default "
                "(`--glob '!node_modules' --glob '!.next' --glob '!coverage' --glob '!dist' "
                "--glob '!docs/**' --glob '!*.tsbuildinfo'`) unless the task explicitly targets those paths."
            )
        else:
            actions.append("Start with native `rg/read/test` to confirm the first concrete file or failure boundary.")
    if runtime["use_codelens"] in {"recommended", "required"}:
        if runtime["deferred_loading"]:
            if evaluation_mode in {"read-only-eval", "bounded-local-eval"}:
                actions.append("Open a deferred CodeLens session, keep the workflow surface small, and do not request full tool exposure.")
            else:
                actions.append("Open a deferred CodeLens session and keep the default surface small; do not request full tool exposure.")
        entrypoints = ordered_entrypoints(task_kind, scenario)
        if entrypoints:
            actions.append(
                "CodeLens entrypoints: "
                + ", ".join(f"`{tool}`" for tool in entrypoints[:3])
                + "."
            )
        if evaluation_mode in {"read-only-eval", "bounded-local-eval"}:
            budget_bits = []
            if max_workflow_calls is not None:
                budget_bits.append(f"{max_workflow_calls} workflow report(s)")
            if max_evidence_expansions is not None:
                budget_bits.append(f"{max_evidence_expansions} evidence expansion(s)")
            if budget_bits:
                actions.append("Budget: at most " + " and ".join(budget_bits) + " before concluding.")
            actions.append("Use evidence/section expansion only if the first workflow result is insufficient; avoid primitive tier unless the workflow path fails.")
            budget_summary = render_result_budget(result_budget)
            if budget_summary:
                actions.append(f"Keep the answer bounded: {budget_summary}.")
            if scenario and scenario.get("stop_rule"):
                actions.append(scenario["stop_rule"])
        else:
            actions.append("Use evidence/section expansion only after the first workflow result; avoid primitive tier unless the workflow result is insufficient.")
    elif runtime["use_codelens"] == "optional":
        entrypoints = TASK_ENTRYPOINTS.get(task_kind, [])
        if entrypoints:
            actions.append(
                "If native search becomes noisy, a one-shot CodeLens call is acceptable: "
                + ", ".join(f"`{tool}`" for tool in entrypoints[:2])
                + "."
            )
    if task_kind == "refactor preflight":
        actions.append("Before mutation, require verifier-first workflow and inspect blockers/readiness before any write step.")
    return actions


def build_platform_actions(task_kind: str, platform: str, runtime: dict):
    platform_cfg = agents.get_agent(platform)
    if platform == "claude":
        actions = [
            f"Read the repo-local `{platform_cfg['repo_instruction_name']}` and global `{platform_cfg['global_instruction_label']}` before choosing a harness path.",
        ]
        if runtime["use_codelens"] in {"recommended", "required"}:
            actions.append("Prefer CodeLens-aware exploration; do not use Explore agents for code tasks.")
        if task_kind in {"impact/reviewer", "refactor preflight", "onboarding/planning"}:
            actions.append("If the task grows beyond a local edit, select a Claude harness pattern (`tool-centric`, `workflow`, or `agent-loop`) explicitly.")
        return actions
    return [
        f"Follow repo-local `{platform_cfg['repo_instruction_name']}` and global `{platform_cfg['global_instruction_label']}` before starting the task.",
    ]


def build_brief(repo: dict, task_kind: str, task_text: str, policy: dict, resolved_rule: dict, platform: str = "codex"):
    runtime = POLICY_RUNTIME[resolved_rule["recommended_policy"]]
    platform_cfg = PLATFORM_DEFAULTS[platform]
    repo_agents = Path(repo["path"]) / platform_cfg["repo_instructions_name"]
    first_actions = build_first_actions(task_kind, runtime)
    first_actions.extend(build_platform_actions(task_kind, platform, runtime))
    return {
        "schema_version": "codelens-task-bootstrap-v1",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "platform": platform,
        "repo_id": repo["id"],
        "repo_label": repo["label"],
        "repo_path": repo["path"],
        "repo_stack": repo.get("stack", "unknown"),
        "repo_instruction_path": str(repo_agents),
        "repo_instruction_exists": repo_agents.exists(),
        "global_instruction_path": platform_cfg["global_instructions"],
        "task_kind": task_kind,
        "task": task_text,
        "policy_source": resolved_rule["source"],
        "recommended_policy": resolved_rule["recommended_policy"],
        "confidence": resolved_rule["confidence"],
        "explanation": resolved_rule["explanation"],
        "route_mode": runtime["route_mode"],
        "use_codelens": runtime["use_codelens"],
        "native_first": runtime["native_first"],
        "deferred_loading": runtime["deferred_loading"],
        "summary": runtime["summary"],
        "preferred_entrypoints": TASK_ENTRYPOINTS.get(task_kind, []),
        "first_actions": first_actions,
        "verify_commands": repo.get("verify_commands", []),
        "policy_path": str(platform_cfg["default_policy"]),
        "source_report_path": policy.get("source_report_path"),
    }


def apply_scenario_to_brief(brief: dict, scenario: dict | None, platform: str = "codex"):
    if not scenario:
        return brief
    brief["scenario_id"] = scenario["scenario_id"]
    brief["scenario_goal"] = scenario.get("goal")
    brief["evaluation_mode"] = scenario.get("task_mode")
    brief["verification_mode"] = scenario.get("verification_mode")
    brief["workflow_budget"] = scenario.get("workflow_budget", {})
    brief["result_budget"] = scenario.get("result_budget", {})
    brief["stop_rule"] = scenario.get("stop_rule", "")
    brief["agent_hints"] = scenario.get("agent_hints", {})
    brief["preferred_entrypoints"] = ordered_entrypoints(brief["task_kind"], scenario)
    runtime = POLICY_RUNTIME[brief["recommended_policy"]]
    first_actions = build_first_actions(brief["task_kind"], runtime, scenario)
    first_actions.extend(build_platform_actions(brief["task_kind"], platform, runtime))
    brief["first_actions"] = first_actions
    return brief


def render_markdown(brief: dict) -> str:
    lines = []
    a = lines.append
    a(f"# CodeLens Task Bootstrap: {brief['repo_label']} / {brief['task_kind']}")
    a("")
    a("| Field | Value |")
    a("|---|---|")
    a(f"| Platform | {brief.get('platform', 'codex')} |")
    a(f"| Repo | {brief['repo_path']} |")
    a(f"| Task kind | {brief['task_kind']} |")
    a(f"| Policy | {brief['recommended_policy']} |")
    a(f"| Source | {brief['policy_source']} |")
    a(f"| Confidence | {brief['confidence']} |")
    a(f"| Route mode | {brief['route_mode']} |")
    a(f"| Use CodeLens | {brief['use_codelens']} |")
    a(f"| Native first | {brief['native_first']} |")
    a(f"| Deferred loading | {brief['deferred_loading']} |")
    a(f"| Repo instructions | {brief['repo_instruction_path']} |")
    a(f"| Global instructions | {brief['global_instruction_path']} |")
    a("")
    if brief.get("task"):
        a("## Task")
        a("")
        a(brief["task"])
        a("")
    a("## Summary")
    a("")
    a(brief["summary"])
    a("")
    a("## First Actions")
    a("")
    for item in brief["first_actions"]:
        a(f"- {item}")
    a("")
    a("## Verification")
    a("")
    if brief["verify_commands"]:
        for command in brief["verify_commands"]:
            a(f"- `{command}`")
    else:
        a("- no repo-specific verify commands registered")
    a("")
    a("## Policy Reason")
    a("")
    a(brief["explanation"] or "No explicit explanation available.")
    a("")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", required=True)
    parser.add_argument("--task-kind", required=True)
    parser.add_argument("--task", default="")
    parser.add_argument("--platform", choices=list(agents.agent_names()), default="codex")
    parser.add_argument("--policy", default="")
    parser.add_argument("--repo-config", default=str(DEFAULT_REPO_CONFIG))
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    args = parser.parse_args()

    repo_path = Path(args.repo).expanduser().resolve()
    platform_cfg = PLATFORM_DEFAULTS[args.platform]
    policy_path = Path(args.policy).expanduser() if args.policy else platform_cfg["default_policy"]
    repo_config_path = Path(args.repo_config).expanduser()
    policy = common.load_json(policy_path)
    repo_config = common.load_json(repo_config_path)
    repo_map = {repo["id"]: repo for repo in repo_config.get("representative_repos", [])}
    repo = find_repo(repo_map, repo_path)
    resolved_rule = choose_rule(policy, repo["id"], args.task_kind)
    brief = build_brief(repo, args.task_kind, args.task, policy, resolved_rule, platform=args.platform)

    output_dir = platform_cfg["default_output_dir"]
    output_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y-%m-%d")
    base = f"{stamp}-{common.slugify(repo['id'])}-{common.slugify(args.task_kind)}-{args.platform}"
    output_json = Path(args.output_json).expanduser() if args.output_json else output_dir / f"{base}.json"
    output_md = Path(args.output_md).expanduser() if args.output_md else output_dir / f"{base}.md"
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_json.write_text(json.dumps(brief, ensure_ascii=False, indent=2) + "\n")
    output_md.write_text(render_markdown(brief))

    print(
        json.dumps(
            {
                "output_json": str(output_json),
                "output_markdown": str(output_md),
                "repo_id": repo["id"],
                "platform": args.platform,
                "task_kind": args.task_kind,
                "recommended_policy": brief["recommended_policy"],
                "route_mode": brief["route_mode"],
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
