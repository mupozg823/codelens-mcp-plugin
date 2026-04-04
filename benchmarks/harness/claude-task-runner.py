#!/usr/bin/env python3
"""Create or execute a Claude task prompt from CodeLens routing policy."""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime
from pathlib import Path

import harness_runner_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
BOOTSTRAP_SCRIPT = SCRIPT_DIR / "task-bootstrap.py"
SESSION_EVAL_SCRIPT = SCRIPT_DIR / "session-eval.py"
HARNESS_EVAL_SCRIPT = SCRIPT_DIR / "harness-eval.py"
REFRESH_POLICY_SCRIPT = SCRIPT_DIR / "refresh-routing-policy.py"
DEFAULT_PROMPT_DIR = Path.home() / ".claude" / "harness" / "bootstrap" / "prompts"
DEFAULT_RUN_DIR = Path.home() / ".claude" / "harness" / "runs"
DEFAULT_WORKSPACE_ALIAS_DIR = Path.home() / ".codex" / "harness" / "workspaces"
DEFAULT_MCP_URL = "http://127.0.0.1:7837/mcp"


def load_bootstrap_module():
    return common.load_module(BOOTSTRAP_SCRIPT, "task_bootstrap_module")


def load_session_eval_module():
    return common.load_module(SESSION_EVAL_SCRIPT, "session_eval_module")


def resolved_permission_mode(requested_mode: str, brief: dict) -> str:
    if requested_mode:
        return requested_mode
    if brief.get("evaluation_mode") in {"read-only-eval", "bounded-local-eval"}:
        return "plan"
    return "acceptEdits"


def resolved_timeout_seconds(requested_timeout: int, brief: dict) -> int:
    if requested_timeout > 0:
        return requested_timeout
    hints = brief.get("agent_hints", {})
    if isinstance(hints, dict) and hints.get("claude_timeout_seconds"):
        return int(hints["claude_timeout_seconds"])
    if brief.get("evaluation_mode") in {"read-only-eval", "bounded-local-eval"}:
        return 90
    return 0


def resolved_effort(requested_effort: str, brief: dict) -> str:
    if requested_effort:
        return requested_effort
    hints = brief.get("agent_hints", {})
    if isinstance(hints, dict) and hints.get("claude_effort"):
        return str(hints["claude_effort"])
    mode = brief.get("evaluation_mode")
    if mode == "bounded-local-eval":
        return "low"
    if mode == "read-only-eval":
        return "medium"
    return ""


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenario-file", default="")
    parser.add_argument("--scenario-id", default="")
    parser.add_argument("--repo", default="")
    parser.add_argument("--task-kind", default="")
    parser.add_argument("--task", default="")
    parser.add_argument("--task-file", default="")
    parser.add_argument("--policy", default="")
    parser.add_argument("--repo-config", default="")
    parser.add_argument("--prompt-file", default="")
    parser.add_argument("--bootstrap-json", default="")
    parser.add_argument("--bootstrap-md", default="")
    parser.add_argument("--model", default="")
    parser.add_argument("--effort", default="")
    parser.add_argument("--permission-mode", default="")
    parser.add_argument("--append-system-prompt", default="")
    parser.add_argument("--agent", default="claude")
    parser.add_argument("--mode", default="")
    parser.add_argument("--mcp-url", default=DEFAULT_MCP_URL)
    parser.add_argument("--timeout-seconds", type=int, default=0)
    parser.add_argument("--capture-eval", action="store_true")
    parser.add_argument("--run-dir", default="")
    parser.add_argument("--session-entry-json", default="")
    parser.add_argument("--session-entry-md", default="")
    parser.add_argument("--harness-eval-json", default="")
    parser.add_argument("--harness-eval-md", default="")
    parser.add_argument("--acceptance-passed", default="")
    parser.add_argument("--verify-passed", default="")
    parser.add_argument("--quality-score", default="")
    parser.add_argument("--notes", default="")
    parser.add_argument("--exec", action="store_true")
    parser.add_argument("--output-last-message", default="")
    args = parser.parse_args()

    bootstrap = load_bootstrap_module()
    session_eval = load_session_eval_module()
    scenario = session_eval.load_scenario(args.scenario_file, args.scenario_id)
    if scenario:
        if not args.repo:
            args.repo = scenario["repo_path"]
        if not args.task_kind:
            args.task_kind = scenario["task_kind"]
        if not args.mode:
            args.mode = scenario["mode"]
        if not args.notes:
            args.notes = f"captured from scenario {scenario['scenario_id']}"
        if not args.task and not args.task_file:
            args.task = scenario["prompt"]

    if not args.repo or not args.task_kind:
        raise SystemExit("--repo and --task-kind are required unless provided by --scenario-file")

    task_text = common.load_task_text(args)
    repo_path = Path(args.repo).expanduser().resolve()
    policy_path = Path(args.policy).expanduser() if args.policy else bootstrap.PLATFORM_DEFAULTS["claude"]["default_policy"]
    repo_config_path = Path(args.repo_config).expanduser() if args.repo_config else bootstrap.DEFAULT_REPO_CONFIG
    policy = bootstrap.load_json(policy_path)
    repo_config = bootstrap.load_json(repo_config_path)
    repo_map = {repo["id"]: repo for repo in repo_config.get("representative_repos", [])}
    repo = bootstrap.find_repo(repo_map, repo_path)
    execution_repo_path, workspace_alias = common.resolve_execution_repo_path(
        repo_path,
        repo.get("id", ""),
        DEFAULT_WORKSPACE_ALIAS_DIR,
    )
    resolved_rule = bootstrap.choose_rule(policy, repo["id"], args.task_kind)
    brief = bootstrap.build_brief(repo, args.task_kind, task_text, policy, resolved_rule, platform="claude")
    if scenario:
        brief = bootstrap.apply_scenario_to_brief(brief, scenario, platform="claude")

    permission_mode = resolved_permission_mode(args.permission_mode, brief)
    timeout_seconds = resolved_timeout_seconds(args.timeout_seconds, brief)
    effort = resolved_effort(args.effort, brief)

    stamp = datetime.now().strftime("%Y-%m-%d")
    base = f"{stamp}-{common.slugify(repo['id'])}-{common.slugify(args.task_kind)}-claude"
    run_dir = (
        Path(args.run_dir).expanduser()
        if args.run_dir
        else DEFAULT_RUN_DIR / f"{datetime.now().strftime('%Y%m%d-%H%M%S')}-{common.slugify(repo['id'])}-{common.slugify(args.task_kind)}"
    )
    bootstrap_json = Path(args.bootstrap_json).expanduser() if args.bootstrap_json else bootstrap.PLATFORM_DEFAULTS["claude"]["default_output_dir"] / f"{base}.json"
    bootstrap_md = Path(args.bootstrap_md).expanduser() if args.bootstrap_md else bootstrap.PLATFORM_DEFAULTS["claude"]["default_output_dir"] / f"{base}.md"
    prompt_file = Path(args.prompt_file).expanduser() if args.prompt_file else DEFAULT_PROMPT_DIR / f"{base}.md"

    run_dir.mkdir(parents=True, exist_ok=True)
    bootstrap_json.parent.mkdir(parents=True, exist_ok=True)
    bootstrap_md.parent.mkdir(parents=True, exist_ok=True)
    prompt_file.parent.mkdir(parents=True, exist_ok=True)

    bootstrap_json.write_text(json.dumps(brief, ensure_ascii=False, indent=2) + "\n")
    bootstrap_md.write_text(bootstrap.render_markdown(brief))
    prompt = common.render_prompt(brief, "~/.claude/CLAUDE.md")
    prompt_file.write_text(prompt)

    result = {
        "repo": str(repo_path),
        "execution_repo": str(execution_repo_path),
        "task_kind": args.task_kind,
        "scenario_id": scenario.get("scenario_id") if scenario else None,
        "recommended_policy": brief["recommended_policy"],
        "route_mode": brief["route_mode"],
        "bootstrap_json": str(bootstrap_json),
        "bootstrap_markdown": str(bootstrap_md),
        "prompt_file": str(prompt_file),
        "run_dir": str(run_dir),
    }
    if workspace_alias:
        result["workspace_alias"] = workspace_alias

    claude_cmd = ["claude", "-p"]
    if args.model:
        claude_cmd.extend(["--model", args.model])
    if effort:
        claude_cmd.extend(["--effort", effort])
    if permission_mode:
        claude_cmd.extend(["--permission-mode", permission_mode])
    if args.append_system_prompt:
        claude_cmd.extend(["--append-system-prompt", args.append_system_prompt])
    claude_cmd.append(prompt)
    result["claude_command"] = claude_cmd
    result["permission_mode"] = permission_mode
    result["timeout_seconds"] = timeout_seconds
    result["effort"] = effort

    last_message_file = Path(args.output_last_message).expanduser() if args.output_last_message else run_dir / "last-message.md"
    result["last_message_file"] = str(last_message_file)

    before_metrics = None
    before_metrics_file = run_dir / "metrics-before.json"
    after_metrics_file = run_dir / "metrics-after.json"
    delta_metrics_file = run_dir / "metrics-delta.json"
    if args.capture_eval:
        before_metrics = common.capture_metrics_snapshot(args.mcp_url, request_id=9201)
        before_metrics_file.write_text(json.dumps(before_metrics, ensure_ascii=False, indent=2) + "\n")
        result["metrics_before_file"] = str(before_metrics_file)

    if not args.exec:
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return

    try:
        proc = subprocess.run(
            claude_cmd,
            cwd=execution_repo_path,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout_seconds if timeout_seconds > 0 else None,
        )
    except subprocess.TimeoutExpired as exc:
        if exc.stdout:
            last_message_file.write_text(exc.stdout)
        timeout_file = run_dir / "claude-timeout.log"
        timeout_file.write_text(
            f"Timed out after {timeout_seconds} seconds.\n\nSTDOUT:\n{exc.stdout or ''}\n\nSTDERR:\n{exc.stderr or ''}"
        )
        result["timeout_file"] = str(timeout_file)
        result["timed_out"] = True
        raise SystemExit(124)

    last_message_file.write_text(proc.stdout)
    if proc.returncode != 0:
        error_file = run_dir / "claude-stderr.log"
        error_file.write_text(proc.stderr)
        result["stderr_file"] = str(error_file)
        raise SystemExit(proc.returncode)

    if args.capture_eval and before_metrics is not None:
        after_metrics = common.capture_metrics_snapshot(args.mcp_url, request_id=9202)
        after_metrics_file.write_text(json.dumps(after_metrics, ensure_ascii=False, indent=2) + "\n")
        result["metrics_after_file"] = str(after_metrics_file)
        delta_payload = common.build_metrics_delta(session_eval, before_metrics, after_metrics)
        delta_metrics_file.write_text(json.dumps(delta_payload, ensure_ascii=False, indent=2) + "\n")
        result["metrics_delta_file"] = str(delta_metrics_file)

        entry_args = common.build_entry_args(
            repo_path=repo_path,
            repo=repo,
            scenario=scenario,
            task_kind=args.task_kind,
            mode=args.mode or common.infer_mode_from_policy(brief["recommended_policy"]),
            agent=args.agent,
            session_eval=session_eval,
            acceptance_passed=args.acceptance_passed,
            verify_passed=args.verify_passed,
            quality_score=args.quality_score,
            recommended_policy=brief["recommended_policy"],
            notes=args.notes,
        )
        session_entry = session_eval.build_entry(entry_args, delta_payload)
        session_entry["repo_label"] = repo.get("label", session_entry.get("repo_label", repo["id"]))
        artifact_paths = common.write_session_entry_artifacts(
            session_eval=session_eval,
            session_entry=session_entry,
            run_dir=run_dir,
            session_entry_json_path=args.session_entry_json,
            session_entry_md_path=args.session_entry_md,
            archive_suffix="-claude",
            repo_id=repo["id"],
            task_kind=args.task_kind,
            mode=entry_args.mode,
        )
        result.update({k: v for k, v in artifact_paths.items() if not k.endswith("_path")})

        harness_eval_json = Path(args.harness_eval_json).expanduser() if args.harness_eval_json else run_dir / "harness-eval.json"
        harness_eval_md = Path(args.harness_eval_md).expanduser() if args.harness_eval_md else run_dir / "harness-eval.md"
        result["harness_eval_result"] = common.run_harness_eval(
            HARNESS_EVAL_SCRIPT,
            repo_path=repo_path,
            archive_entry_json=artifact_paths["archive_entry_json_path"],
            output_json=harness_eval_json,
            output_md=harness_eval_md,
            label=f"live-claude-{repo['id']}-{common.slugify(args.task_kind)}",
            base_report_path=policy.get("source_report_path", ""),
        )
        result["harness_eval_json"] = str(harness_eval_json)
        result["harness_eval_markdown"] = str(harness_eval_md)

        refresh_result_file = run_dir / "routing-policy-refresh.json"
        result["routing_policy_refresh"] = common.run_refresh(
            REFRESH_POLICY_SCRIPT,
            label=f"post-claude-session-{repo['id']}-{common.slugify(args.task_kind)}",
            output_json=refresh_result_file,
        )
        result["routing_policy_refresh_json"] = str(refresh_result_file)

    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
