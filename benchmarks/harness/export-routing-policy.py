#!/usr/bin/env python3
"""Export machine-readable CodeLens harness routing policy from evaluation reports."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

import harness_eval_common as common


DEFAULT_REPORT = (
    Path.home() / ".codex" / "harness" / "reports" / "2026-04-03-codelens-eval-cross-repo-release.json"
)
DEFAULT_OUTPUT_DIR = Path.home() / ".codex" / "harness" / "policies"
CANONICAL_JSON = DEFAULT_OUTPUT_DIR / "codelens-routing-policy.json"
CANONICAL_MD = DEFAULT_OUTPUT_DIR / "codelens-routing-policy.md"
CLAUDE_OUTPUT_DIR = Path.home() / ".claude" / "harness" / "policies"
CLAUDE_CANONICAL_JSON = CLAUDE_OUTPUT_DIR / "codelens-routing-policy.json"
CLAUDE_CANONICAL_MD = CLAUDE_OUTPUT_DIR / "codelens-routing-policy.md"


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


def build_policy(report):
    task_groups = defaultdict(list)
    repo_overrides = []
    for item in report.get("task_summaries", []):
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
        "schema_version": "codelens-routing-policy-v1",
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
    a("| Field | Value |")
    a("|---|---|")
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
    policy = build_policy(report)

    DEFAULT_OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y-%m-%d")
    base_name = f"{stamp}-{common.slugify(args.label)}"
    output_json = Path(args.output_json).expanduser() if args.output_json else DEFAULT_OUTPUT_DIR / f"{base_name}.json"
    output_md = Path(args.output_md).expanduser() if args.output_md else DEFAULT_OUTPUT_DIR / f"{base_name}.md"
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    json_text = json.dumps(policy, ensure_ascii=False, indent=2) + "\n"
    md_text = render_markdown(policy)
    output_json.write_text(json_text)
    output_md.write_text(md_text)
    if not args.skip_canonical_write:
        CANONICAL_JSON.write_text(json_text)
        CANONICAL_MD.write_text(md_text)
        if not args.skip_claude_mirror:
            CLAUDE_OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
            CLAUDE_CANONICAL_JSON.write_text(json_text)
            CLAUDE_CANONICAL_MD.write_text(md_text)

    print(
        json.dumps(
            {
                "policy_json": str(output_json),
                "policy_markdown": str(output_md),
                "canonical_json": None if args.skip_canonical_write else str(CANONICAL_JSON),
                "canonical_markdown": None if args.skip_canonical_write else str(CANONICAL_MD),
                "claude_canonical_json": (
                    None if args.skip_canonical_write or args.skip_claude_mirror else str(CLAUDE_CANONICAL_JSON)
                ),
                "claude_canonical_markdown": (
                    None if args.skip_canonical_write or args.skip_claude_mirror else str(CLAUDE_CANONICAL_MD)
                ),
                "global_rules": len(policy["global_rules"]),
                "repo_overrides": len(policy["repo_overrides"]),
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
