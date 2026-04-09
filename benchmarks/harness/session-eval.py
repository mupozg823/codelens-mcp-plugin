#!/usr/bin/env python3
"""Normalize a real Codex/Claude session into a harness-eval entry."""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path

import harness_eval_common as common

DEFAULT_REPORT_DIR = Path.home() / ".codex" / "harness" / "reports" / "session-entries"


def parse_bool_flag(value: str | None):
    if value is None:
        return None
    lowered = value.strip().lower()
    if lowered in {"true", "yes", "1", "pass", "passed"}:
        return True
    if lowered in {"false", "no", "0", "fail", "failed"}:
        return False
    if lowered in {"unknown", "n/a", "na", "null", ""}:
        return None
    raise argparse.ArgumentTypeError(f"unsupported boolean-ish value: {value}")


def load_json(path: str | None):
    if path:
        return common.load_json(Path(path))
    return json.load(sys.stdin)


def load_scenario(path: str | None, scenario_id: str | None):
    if not path:
        return None
    payload = json.loads(Path(path).expanduser().read_text())
    scenarios = payload.get("scenarios") or []
    if not scenario_id:
        if len(scenarios) != 1:
            raise SystemExit("scenario file has multiple scenarios; pass --scenario-id")
        return scenarios[0]
    for scenario in scenarios:
        if scenario.get("scenario_id") == scenario_id:
            return scenario
    raise SystemExit(f"scenario id not found: {scenario_id}")


def unwrap_metrics_payload(raw):
    if not isinstance(raw, dict):
        return {}
    if "session" in raw and "derived_kpis" in raw:
        return raw
    if isinstance(raw.get("data"), dict):
        return unwrap_metrics_payload(raw["data"])
    if isinstance(raw.get("result"), dict):
        result = raw["result"]
        if isinstance(result.get("structuredContent"), dict):
            return unwrap_metrics_payload(result["structuredContent"])
        contents = result.get("content") or []
        if contents:
            text = contents[0].get("text")
            if isinstance(text, str):
                try:
                    return unwrap_metrics_payload(json.loads(text))
                except Exception:
                    pass
    return raw


def build_entry(args, payload):
    session = payload.get("session") or {}
    derived = payload.get("derived_kpis") or {}
    metrics_capture_skipped = bool(payload.get("metrics_capture_skipped"))
    metrics_capture_reason = payload.get("metrics_capture_reason")
    repo_path = str(Path(args.repo).expanduser())
    repo_id = getattr(args, "repo_id", "") or Path(repo_path).name or "repo"
    repo_label = getattr(args, "repo_label", "") or repo_id
    bootstrap_raw = session.get("tools_list_tokens")
    total_raw = session.get("total_tokens")
    bootstrap_tokens = int(bootstrap_raw) if bootstrap_raw is not None else None
    total_tokens = int(total_raw) if total_raw is not None else None
    token_out = (
        max(total_tokens - bootstrap_tokens, 0)
        if total_tokens is not None and bootstrap_tokens is not None
        else None
    )
    notes = []
    if args.notes:
        notes.append(args.notes.strip())
    if metrics_capture_skipped and metrics_capture_reason:
        notes.append(f"metrics capture skipped: {metrics_capture_reason}")
    if float(derived.get("verifier_contract_present_rate") or 0.0) > 0:
        notes.append("verifier contract observed in session telemetry")
    if float(derived.get("recommended_check_followthrough_rate") or 0.0) > 0:
        notes.append("recommended checks were followed through in-session")
    if float(derived.get("handle_reuse_rate") or 0.0) > 0:
        notes.append("analysis/evidence handles were reused")
    if int(session.get("mutation_preflight_checked_count") or 0) > 0:
        notes.append("mutation preflight gate was exercised")
    if int(session.get("deferred_namespace_expansion_count") or 0) > 0:
        notes.append("deferred loading expansion occurred")
    last_message_file = getattr(args, "last_message_file", "") or ""
    last_message_text = ""
    if last_message_file:
        path = Path(last_message_file).expanduser()
        if path.exists():
            last_message_text = path.read_text(encoding="utf-8", errors="replace")
    completion_contract = common.analyze_completion_contract(last_message_text)
    if completion_contract.get("score") is not None:
        notes.append(
            "completion contract "
            f"{sum(1 for hit in completion_contract['section_hits'].values() if hit)}/4 sections detected"
        )
    if completion_contract.get("asked_for_user_input"):
        notes.append("asked for user input during nominally non-interactive run")

    success = args.success
    if success is None:
        success = int(session.get("error_count") or 0) == 0

    entry_draft = {
        "schema_version": "codelens-harness-eval-entry-v1",
        "source_kind": "real-session",
        "captured_at": getattr(args, "captured_at", None),
        "scenario_id": getattr(args, "scenario_id", None),
        "repo": repo_path,
        "repo_id": repo_id,
        "repo_label": repo_label,
        "task_kind": args.task_kind,
        "mode": args.mode,
        "agent": args.agent,
        "success": success,
        "acceptance_passed": args.acceptance_passed,
        "verify_passed": args.verify_passed,
        "quality_score": args.quality_score,
        "token_in": bootstrap_tokens,
        "token_out": token_out,
        "bootstrap_tokens": bootstrap_tokens,
        "tool_calls": int(session["total_calls"]) if session.get("total_calls") is not None else None,
        "low_level_chain_count": int(
            session.get("repeated_low_level_chain_count") or 0
        ) if session.get("repeated_low_level_chain_count") is not None else None,
        "elapsed_ms": session.get("total_ms"),
        "notes": " | ".join(notes),
        "recommended_policy": args.recommended_policy or "pending",
        "verifier_used": float(derived.get("verifier_contract_present_rate") or 0.0)
        > 0,
        "evidence_reuse_rate": float(derived.get("handle_reuse_rate") or 0.0),
        "recommended_check_followthrough_rate": float(
            derived.get("recommended_check_followthrough_rate") or 0.0
        ),
        "composite_ratio": float(derived.get("composite_ratio") or 0.0),
        "last_message_file": str(Path(last_message_file).expanduser()) if last_message_file else None,
        "completion_contract_score": completion_contract.get("score"),
        "completion_contract_passed": completion_contract.get("passed"),
        "asked_for_user_input": completion_contract.get("asked_for_user_input"),
        "completion_contract_sections": completion_contract.get("section_hits"),
        "metrics_snapshot": {
            "total_tokens": total_tokens,
            "tools_list_tokens": bootstrap_tokens,
            "error_count": int(session.get("error_count") or 0),
            "quality_contract_present_rate": float(
                derived.get("quality_contract_present_rate") or 0.0
            ),
            "verifier_contract_present_rate": float(
                derived.get("verifier_contract_present_rate") or 0.0
            ),
            "verifier_followthrough_rate": float(
                derived.get("verifier_followthrough_rate") or 0.0
            ),
        },
        "metrics_capture_skipped": metrics_capture_skipped,
        "metrics_capture_reason": metrics_capture_reason,
        "captured_at": datetime.now().isoformat(timespec="seconds"),
    }

    # Auto-compute quality_score from metrics when not manually set
    if entry_draft["quality_score"] is None:
        entry_draft["quality_score"] = common.compute_quality_score(entry_draft)

    return entry_draft


def render_markdown(entry):
    lines = [
        f"# Session Eval Entry: {entry['repo']} / {entry['task_kind']}",
        "",
        "| Field | Value |",
        "|---|---|",
        f"| Mode | {entry['mode']} |",
        f"| Agent | {entry['agent']} |",
        f"| Success | {entry['success']} |",
        f"| Acceptance passed | {entry['acceptance_passed']} |",
        f"| Verify passed | {entry['verify_passed']} |",
        f"| Quality score | {entry['quality_score']} |",
        f"| Completion contract score | {entry.get('completion_contract_score')} |",
        f"| Completion contract passed | {entry.get('completion_contract_passed')} |",
        f"| Asked for user input | {entry.get('asked_for_user_input')} |",
        f"| Token in | {entry['token_in']} |",
        f"| Token out | {entry['token_out']} |",
        f"| Bootstrap tokens | {entry['bootstrap_tokens']} |",
        f"| Tool calls | {entry['tool_calls']} |",
        f"| Low-level chain count | {entry['low_level_chain_count']} |",
        f"| Elapsed ms | {entry['elapsed_ms']} |",
        f"| Recommended policy | {entry['recommended_policy']} |",
        "",
        "## Notes",
        "",
        entry["notes"] or "(none)",
        "",
    ]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--input",
        default="",
        help="Path to raw get_tool_metrics JSON. Reads stdin when omitted.",
    )
    parser.add_argument("--scenario-file", default="")
    parser.add_argument("--scenario-id", default="")
    parser.add_argument("--repo", default="")
    parser.add_argument("--repo-id", default="")
    parser.add_argument("--repo-label", default="")
    parser.add_argument("--task-kind", default="")
    parser.add_argument(
        "--mode", default="", choices=["", "baseline", "naive-on", "routed-on"]
    )
    parser.add_argument("--agent", default="codex")
    parser.add_argument("--acceptance-passed", type=parse_bool_flag, default=None)
    parser.add_argument("--verify-passed", type=parse_bool_flag, default=None)
    parser.add_argument("--success", type=parse_bool_flag, default=None)
    parser.add_argument("--quality-score", type=float, default=None)
    parser.add_argument("--recommended-policy", default="")
    parser.add_argument("--notes", default="")
    parser.add_argument("--last-message-file", default="")
    parser.add_argument("--output-json", default="")
    parser.add_argument("--output-md", default="")
    args = parser.parse_args()

    scenario = load_scenario(args.scenario_file, args.scenario_id)
    if scenario:
        if not args.repo:
            args.repo = scenario["repo_path"]
        if not args.repo_id:
            args.repo_id = scenario.get("repo_id", "")
        if not args.repo_label:
            args.repo_label = scenario.get("repo_label", "")
        if not args.task_kind:
            args.task_kind = scenario["task_kind"]
        if not args.mode:
            args.mode = scenario["mode"]
        if not args.notes:
            args.notes = f"captured from scenario {scenario['scenario_id']}"
        if not args.recommended_policy:
            args.recommended_policy = "pending"
    if not args.repo or not args.task_kind or not args.mode:
        raise SystemExit(
            "--repo, --task-kind, and --mode are required unless provided by --scenario-file"
        )

    payload = unwrap_metrics_payload(load_json(args.input))
    entry = build_entry(args, payload)

    report_dir = DEFAULT_REPORT_DIR
    report_dir.mkdir(parents=True, exist_ok=True)
    if args.output_json:
        output_json = Path(args.output_json).expanduser()
    else:
        output_json = report_dir / (
            f"{datetime.now().strftime('%Y%m%d-%H%M%S')}-"
            f"{common.slugify(Path(args.repo).name)}-"
            f"{common.slugify(args.task_kind)}-"
            f"{common.slugify(args.mode)}.json"
        )
    output_json.parent.mkdir(parents=True, exist_ok=True)
    output_json.write_text(json.dumps(entry, ensure_ascii=False, indent=2) + "\n")

    markdown = render_markdown(entry)
    if args.output_md:
        output_md = Path(args.output_md).expanduser()
        output_md.parent.mkdir(parents=True, exist_ok=True)
        output_md.write_text(markdown + "\n")

    print(
        json.dumps(
            {"entry": entry, "output_json": str(output_json)},
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
