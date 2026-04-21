#!/usr/bin/env python3
"""One-command release harness runner with checkpoint reuse and signoff."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

import harness_runner_common as common


SCRIPT_DIR = Path(__file__).resolve().parent
CODEX_RUNNER = SCRIPT_DIR / "codex-task-runner.py"
CLAUDE_RUNNER = SCRIPT_DIR / "claude-task-runner.py"
DEFAULT_RUN_DIR = Path.home() / ".codex" / "harness" / "release-runs"

USAGE_BLOCK_RE = re.compile(r"<usage(?P<attrs>[^>]*)>(?P<body>.*?)</usage>", re.IGNORECASE | re.DOTALL)
USAGE_ATTR_RE = re.compile(r"([a-zA-Z_][a-zA-Z0-9_-]*)=\"([^\"]*)\"")
KV_LINE_RE = re.compile(r"^\s*([A-Za-z0-9_ ./()-]+?)\s*:\s*(.+?)\s*$")

ROLE_STAGE_ORDER = [
    ("worker", "worker_scan"),
    ("orchestrator", "orchestrator"),
    ("evaluator", "evaluator"),
    ("independent_evaluator", "independent_signoff"),
]


@dataclass
class StageExecutionError(RuntimeError):
    stage_name: str
    stage_result: dict

    def __str__(self) -> str:
        return f"{self.stage_name} failed"


def load_manifest(path: Path) -> dict:
    payload = common.read_json_file(path)
    required = [
        "task",
        "artifact_path",
        "acceptance_criteria",
        "roles",
        "repair",
        "runner_defaults",
    ]
    missing = [key for key in required if key not in payload]
    if missing:
        raise SystemExit(f"release harness manifest missing required fields: {', '.join(missing)}")
    if not isinstance(payload["acceptance_criteria"], list) or not payload["acceptance_criteria"]:
        raise SystemExit("acceptance_criteria must be a non-empty list")
    if not isinstance(payload["roles"], dict):
        raise SystemExit("roles must be an object")
    if not isinstance(payload["repair"], dict):
        raise SystemExit("repair must be an object")
    if not isinstance(payload["runner_defaults"], dict):
        raise SystemExit("runner_defaults must be an object")

    roles = {}
    for key, value in payload["roles"].items():
        normalized = key.strip().lower().replace("-", "_").replace(" ", "_")
        roles[normalized] = value
    payload["roles"] = roles

    for required_role in ["orchestrator", "evaluator", "independent_evaluator"]:
        if required_role not in roles:
            raise SystemExit(f"roles.{required_role} is required")

    payload["repair"]["max_rounds"] = max(0, min(int(payload["repair"].get("max_rounds", 0)), 1))
    payload["runner_defaults"]["task_kind"] = str(
        payload["runner_defaults"].get("task_kind") or "release-harness"
    )
    payload["runner_defaults"]["mode"] = str(
        payload["runner_defaults"].get("mode") or "routed-on"
    )
    payload["runner_defaults"]["mcp_url"] = str(
        payload["runner_defaults"].get("mcp_url") or "http://127.0.0.1:7837/mcp"
    )
    repo = payload["runner_defaults"].get("repo")
    if not repo:
        raise SystemExit("runner_defaults.repo is required")
    return payload


def resolve_artifact_path(repo_path: Path, artifact_path: str) -> Path:
    path = Path(artifact_path).expanduser()
    if not path.is_absolute():
        path = repo_path / path
    return path.resolve()


def stage_dir(root: Path, stage_name: str) -> Path:
    return root / "stages" / stage_name


def prompt_path_for_stage(stage_root: Path) -> Path:
    return stage_root / "task.txt"


def result_path_for_stage(stage_root: Path) -> Path:
    return stage_root / "stage-result.json"


def role_config(manifest: dict, role_name: str) -> dict | None:
    value = manifest["roles"].get(role_name)
    if not isinstance(value, dict):
        return None
    if value.get("enabled", True) is False:
        return None
    return value


def runner_script_for(role: dict) -> Path:
    runner = str(role.get("runner") or "").strip().lower()
    if runner == "codex":
        return CODEX_RUNNER
    if runner == "claude":
        return CLAUDE_RUNNER
    raise SystemExit(f"unsupported role runner: {runner}")


def render_acceptance_criteria(criteria: list[str]) -> str:
    return "\n".join(f"- {item}" for item in criteria)


def read_text_if_exists(path: Path | None) -> str:
    if not path or not path.exists():
        return ""
    return path.read_text(encoding="utf-8")


def build_worker_prompt(manifest: dict, artifact_path: Path) -> str:
    return (
        f"You are the worker scan stage for a release harness run.\n\n"
        f"Task:\n{manifest['task']}\n\n"
        f"Artifact target:\n{artifact_path}\n\n"
        f"Acceptance criteria:\n{render_acceptance_criteria(manifest['acceptance_criteria'])}\n\n"
        "Read-only only. Do not edit files. Return a concise evidence summary with file paths, risks, and any missing context the orchestrator should use."
    )


def build_orchestrator_prompt(
    manifest: dict,
    artifact_path: Path,
    *,
    worker_summary_path: Path | None = None,
    repair_hints: list[str] | None = None,
) -> str:
    lines = [
        "You are the orchestrator stage for a release harness run.",
        "",
        f"Primary task:\n{manifest['task']}",
        "",
        f"Update this artifact in the repo:\n{artifact_path}",
        "",
        "Acceptance criteria:",
        render_acceptance_criteria(manifest["acceptance_criteria"]),
        "",
        "Make the smallest changes that satisfy the criteria. Finish by verifying only the directly relevant evidence.",
    ]
    if worker_summary_path and worker_summary_path.exists():
        lines.extend(
            [
                "",
                f"Worker evidence summary is available at:\n{worker_summary_path}",
            ]
        )
    if repair_hints:
        lines.extend(
            [
                "",
                "Repair hints from the previous evaluator pass:",
                "\n".join(f"- {hint}" for hint in repair_hints),
            ]
        )
    return "\n".join(lines)


def build_evaluator_prompt(
    manifest: dict,
    artifact_path: Path,
    *,
    usage_drift_path: Path | None = None,
    independent: bool = False,
) -> str:
    lines = [
        "You are the evaluator stage for a release harness run.",
        "Read-only only. Do not edit files.",
        "",
        f"Task under evaluation:\n{manifest['task']}",
        "",
        f"Artifact path:\n{artifact_path}",
        "",
        "Acceptance criteria:",
        render_acceptance_criteria(manifest["acceptance_criteria"]),
        "",
        "Return JSON only. No markdown fence. Use this exact shape:",
        '{"verdict":"PASS|FAIL","aggregate_score":1.0,"summary":"...","repair_hints":["..."],"issues":["..."]}',
    ]
    if independent:
        lines.extend(
            [
                "",
                "This is the independent signoff pass. Judge the artifact directly, not prior evaluator prose.",
            ]
        )
    if usage_drift_path:
        lines.extend(
            [
                "",
                f"Usage drift evidence for the run is available at:\n{usage_drift_path}",
            ]
        )
    return "\n".join(lines)


def build_stage_prompt(
    manifest: dict,
    role_name: str,
    artifact_path: Path,
    *,
    worker_summary_path: Path | None = None,
    repair_hints: list[str] | None = None,
    usage_drift_path: Path | None = None,
) -> str:
    if role_name == "worker":
        return build_worker_prompt(manifest, artifact_path)
    if role_name == "orchestrator":
        return build_orchestrator_prompt(
            manifest,
            artifact_path,
            worker_summary_path=worker_summary_path,
            repair_hints=repair_hints,
        )
    if role_name == "evaluator":
        return build_evaluator_prompt(manifest, artifact_path)
    if role_name == "independent_evaluator":
        return build_evaluator_prompt(
            manifest,
            artifact_path,
            usage_drift_path=usage_drift_path,
            independent=True,
        )
    raise SystemExit(f"unsupported role: {role_name}")


def build_child_command(
    manifest: dict,
    role_name: str,
    role: dict,
    *,
    repo_path: Path,
    stage_root: Path,
    prompt_file: Path,
    exec_requested: bool,
) -> list[str]:
    script = runner_script_for(role)
    runner = str(role.get("runner")).strip().lower()
    defaults = manifest["runner_defaults"]
    command = [
        "python3",
        str(script),
        "--repo",
        str(repo_path),
        "--task-kind",
        str(role.get("task_kind") or defaults["task_kind"]),
        "--task-file",
        str(prompt_file),
        "--run-dir",
        str(stage_root),
        "--mode",
        str(role.get("mode") or defaults["mode"]),
        "--agent",
        str(role.get("agent") or role_name),
        "--mcp-url",
        str(role.get("mcp_url") or defaults["mcp_url"]),
        "--output-last-message",
        str(stage_root / "last-message.md"),
        "--capture-eval",
    ]
    if defaults.get("policy"):
        command.extend(["--policy", str(defaults["policy"])])
    if defaults.get("repo_config"):
        command.extend(["--repo-config", str(defaults["repo_config"])])

    if runner == "claude":
        permission_mode = role.get("permission_mode")
        if not permission_mode:
            permission_mode = "acceptEdits" if role_name == "orchestrator" else "plan"
        command.extend(["--permission-mode", str(permission_mode)])
        if role.get("model"):
            command.extend(["--model", str(role["model"])])
        if role.get("effort"):
            command.extend(["--effort", str(role["effort"])])
        if role.get("append_system_prompt"):
            command.extend(["--append-system-prompt", str(role["append_system_prompt"])])
        if role.get("timeout_seconds"):
            command.extend(["--timeout-seconds", str(int(role["timeout_seconds"]))])
    else:
        sandbox = role.get("sandbox")
        if not sandbox and role_name != "orchestrator":
            sandbox = "read-only"
        if role.get("model"):
            command.extend(["--model", str(role["model"])])
        if role.get("profile"):
            command.extend(["--profile", str(role["profile"])])
        if sandbox:
            command.extend(["--sandbox", str(sandbox)])
        if defaults.get("no_isolated_codex_home"):
            command.append("--no-isolated-codex-home")

    if exec_requested:
        command.append("--exec")
    return command


def collect_stage_artifacts(stage_root: Path) -> dict:
    candidates = {
        "child_run_manifest": stage_root / common.RUN_MANIFEST_FILENAME,
        "child_event_log": stage_root / common.RUN_EVENT_LOG_FILENAME,
        "last_message_file": stage_root / "last-message.md",
        "metrics_before_file": stage_root / "metrics-before.json",
        "metrics_after_file": stage_root / "metrics-after.json",
        "metrics_delta_file": stage_root / "metrics-delta.json",
        "session_entry_json": stage_root / "session-entry.json",
        "session_entry_markdown": stage_root / "session-entry.md",
        "harness_eval_json": stage_root / "harness-eval.json",
        "harness_eval_markdown": stage_root / "harness-eval.md",
        "runner_stdout": stage_root / "runner-stdout.log",
        "runner_stderr": stage_root / "runner-stderr.log",
        "stage_prompt": stage_root / "task.txt",
    }
    return {
        key: str(path)
        for key, path in candidates.items()
        if path.exists()
    }


def write_stage_result(stage_root: Path, payload: dict) -> Path:
    result_path = result_path_for_stage(stage_root)
    result_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return result_path


def load_existing_stage_result(stage_root: Path) -> dict | None:
    result_path = result_path_for_stage(stage_root)
    if not result_path.exists():
        return None
    return common.read_json_file(result_path)


def run_stage(
    manifest: dict,
    role_name: str,
    stage_name: str,
    *,
    repo_path: Path,
    run_dir: Path,
    prompt: str,
    exec_requested: bool,
) -> dict:
    stage_root = stage_dir(run_dir, stage_name)
    stage_root.mkdir(parents=True, exist_ok=True)
    prompt_file = prompt_path_for_stage(stage_root)
    prompt_file.write_text(prompt + "\n", encoding="utf-8")

    existing = load_existing_stage_result(stage_root)
    reusable_statuses = {"completed", "skipped"}
    if not exec_requested:
        reusable_statuses.add("planned")
    if existing and existing.get("status") in reusable_statuses:
        existing.setdefault("stage_dir", str(stage_root))
        existing["reused"] = True
        return existing

    role = role_config(manifest, role_name)
    if role is None:
        result = {
            "role": role_name,
            "stage": stage_name,
            "status": "skipped",
            "reason": "role_disabled",
            "stage_dir": str(stage_root),
            "artifacts": collect_stage_artifacts(stage_root),
            "reused": False,
        }
        result["stage_result_file"] = str(write_stage_result(stage_root, result))
        return result

    command = build_child_command(
        manifest,
        role_name,
        role,
        repo_path=repo_path,
        stage_root=stage_root,
        prompt_file=prompt_file,
        exec_requested=exec_requested,
    )
    result = {
        "role": role_name,
        "stage": stage_name,
        "runner": role.get("runner"),
        "command": command,
        "stage_dir": str(stage_root),
    }
    if not exec_requested:
        result["status"] = "planned"
        result["artifacts"] = collect_stage_artifacts(stage_root)
        result["reused"] = False
        result["stage_result_file"] = str(write_stage_result(stage_root, result))
        return result

    proc = subprocess.run(command, capture_output=True, text=True)
    stdout_path = stage_root / "runner-stdout.log"
    stderr_path = stage_root / "runner-stderr.log"
    stdout_path.write_text(proc.stdout or "", encoding="utf-8")
    stderr_path.write_text(proc.stderr or "", encoding="utf-8")
    result["returncode"] = proc.returncode
    result["stdout_file"] = str(stdout_path)
    result["stderr_file"] = str(stderr_path)
    result["status"] = "completed" if proc.returncode == 0 else "failed"
    result["reused"] = False
    result["artifacts"] = collect_stage_artifacts(stage_root)
    result["stage_result_file"] = str(write_stage_result(stage_root, result))
    if proc.returncode != 0:
        raise StageExecutionError(stage_name=stage_name, stage_result=result)
    return result


def parse_number(value: str) -> int | None:
    cleaned = value.strip().lower().replace(",", "")
    match = re.search(r"(-?\d+(?:\.\d+)?)([km]?)", cleaned)
    if not match:
        return None
    number = float(match.group(1))
    suffix = match.group(2)
    if suffix == "k":
        number *= 1000
    elif suffix == "m":
        number *= 1_000_000
    return int(round(number))


def parse_duration_ms(value: str) -> int | None:
    cleaned = value.strip().lower().replace(",", "")
    if re.fullmatch(r"\d+:\d{2}(?::\d{2})?", cleaned):
        parts = [int(part) for part in cleaned.split(":")]
        if len(parts) == 2:
            minutes, seconds = parts
            return (minutes * 60 + seconds) * 1000
        hours, minutes, seconds = parts
        return (hours * 3600 + minutes * 60 + seconds) * 1000
    match = re.search(r"(-?\d+(?:\.\d+)?)\s*(ms|msec|millisecond|milliseconds|s|sec|secs|second|seconds|m|min|mins|minute|minutes)\b", cleaned)
    if not match:
        numeric = parse_number(cleaned)
        return numeric
    value_num = float(match.group(1))
    unit = match.group(2)
    if unit in {"ms", "msec", "millisecond", "milliseconds"}:
        return int(round(value_num))
    if unit in {"m", "min", "mins", "minute", "minutes"}:
        return int(round(value_num * 60_000))
    return int(round(value_num * 1000))


def usage_metrics_from_mapping(mapping: dict[str, str]) -> dict:
    input_tokens = 0
    output_tokens = 0
    metrics = {"tokens": None, "elapsed_ms": None, "tool_calls": None}
    for key, raw in mapping.items():
        normalized = key.strip().lower().replace("-", "_").replace(" ", "_")
        if "input" in normalized and "token" in normalized:
            parsed = parse_number(raw)
            if parsed is not None:
                input_tokens += parsed
        elif "output" in normalized and "token" in normalized:
            parsed = parse_number(raw)
            if parsed is not None:
                output_tokens += parsed
        elif "token" in normalized and metrics["tokens"] is None:
            metrics["tokens"] = parse_number(raw)
        elif any(token in normalized for token in ["elapsed", "duration", "time"]):
            metrics["elapsed_ms"] = parse_duration_ms(raw)
        elif "tool" in normalized and ("call" in normalized or "count" in normalized):
            metrics["tool_calls"] = parse_number(raw)
    if metrics["tokens"] is None and (input_tokens or output_tokens):
        metrics["tokens"] = input_tokens + output_tokens
    return metrics


def parse_usage_blocks(text: str) -> list[dict]:
    blocks = []
    for match in USAGE_BLOCK_RE.finditer(text or ""):
        attrs = {key.lower(): value for key, value in USAGE_ATTR_RE.findall(match.group("attrs") or "")}
        body = match.group("body") or ""
        mapping = {}
        for line in body.splitlines():
            kv = KV_LINE_RE.match(line)
            if kv:
                mapping[kv.group(1)] = kv.group(2)
        metrics = usage_metrics_from_mapping({**attrs, **mapping})
        blocks.append(
            {
                "kind": attrs.get("kind", "actual").strip().lower(),
                "tokens": metrics["tokens"],
                "elapsed_ms": metrics["elapsed_ms"],
                "tool_calls": metrics["tool_calls"],
                "raw": match.group(0),
            }
        )
    return blocks


def extract_json_objects(text: str) -> list[dict]:
    objects = []
    start = None
    depth = 0
    for index, char in enumerate(text):
        if char == "{":
            if depth == 0:
                start = index
            depth += 1
        elif char == "}":
            if depth == 0:
                continue
            depth -= 1
            if depth == 0 and start is not None:
                chunk = text[start : index + 1]
                try:
                    payload = json.loads(chunk)
                except Exception:
                    continue
                if isinstance(payload, dict):
                    objects.append(payload)
    return objects


def normalize_verdict_payload(payload: dict | None) -> dict:
    payload = payload or {}
    verdict = str(payload.get("verdict") or "").strip().upper()
    if verdict not in {"PASS", "FAIL"}:
        verdict = ""
    hints = payload.get("repair_hints")
    issues = payload.get("issues")
    return {
        "verdict": verdict,
        "aggregate_score": payload.get("aggregate_score"),
        "summary": str(payload.get("summary") or "").strip(),
        "repair_hints": [str(item) for item in hints or [] if str(item).strip()],
        "issues": [str(item) for item in issues or [] if str(item).strip()],
        "parsed": bool(verdict),
    }


def parse_evaluator_result(last_message_path: Path) -> dict:
    text = read_text_if_exists(last_message_path)
    for payload in extract_json_objects(text):
        normalized = normalize_verdict_payload(payload)
        if normalized["parsed"]:
            return normalized
    return {
        "verdict": "",
        "aggregate_score": None,
        "summary": "",
        "repair_hints": [],
        "issues": [],
        "parsed": False,
        "error": "no evaluator verdict json found",
    }


def actual_usage_from_metrics(stage_root: Path) -> dict | None:
    metrics_path = stage_root / "metrics-delta.json"
    if not metrics_path.exists():
        return None
    payload = common.read_json_file(metrics_path)
    session = payload.get("session") or {}
    return {
        "tokens": int(session.get("total_tokens") or 0),
        "elapsed_ms": int(session.get("total_ms") or 0),
        "tool_calls": int(session.get("total_calls") or 0),
        "source": "metrics_delta",
    }


def compare_metric_pair(left: int | None, right: int | None, *, elapsed: bool = False) -> str:
    if left is None or right is None:
        return "missing"
    delta = abs(left - right)
    if elapsed:
        tolerance = max(1000, int(right * 0.05))
    else:
        tolerance = 0
    return "match" if delta <= tolerance else "mismatch"


def collect_stage_usage(stage_result: dict) -> dict:
    stage_root = Path(stage_result.get("stage_dir") or "")
    text_inputs = [
        stage_root / "last-message.md",
        stage_root / "runner-stdout.log",
        stage_root / "runner-stderr.log",
        stage_root / "claude-stderr.log",
    ]
    usage_blocks = []
    for path in text_inputs:
        usage_blocks.extend(parse_usage_blocks(read_text_if_exists(path)))

    actual_block = None
    self_report = None
    for block in usage_blocks:
        if block["kind"] in {"self", "self_report", "reported", "estimate"}:
            self_report = {
                "tokens": block["tokens"],
                "elapsed_ms": block["elapsed_ms"],
                "tool_calls": block["tool_calls"],
                "source": "usage_block",
            }
        else:
            actual_block = {
                "tokens": block["tokens"],
                "elapsed_ms": block["elapsed_ms"],
                "tool_calls": block["tool_calls"],
                "source": "usage_block",
            }

    metrics_usage = actual_usage_from_metrics(stage_root)
    actual_usage = actual_block or metrics_usage
    cross_check = {"status": "missing"}
    if actual_block and metrics_usage:
        field_status = {
            "tokens": compare_metric_pair(actual_block["tokens"], metrics_usage["tokens"]),
            "elapsed_ms": compare_metric_pair(
                actual_block["elapsed_ms"], metrics_usage["elapsed_ms"], elapsed=True
            ),
            "tool_calls": compare_metric_pair(actual_block["tool_calls"], metrics_usage["tool_calls"]),
        }
        cross_check = {
            "status": "match" if all(status in {"match", "missing"} for status in field_status.values()) else "mismatch",
            "fields": field_status,
        }

    drift = None
    if self_report and actual_usage:
        drift = {
            "tokens_delta": None
            if self_report["tokens"] is None or actual_usage["tokens"] is None
            else self_report["tokens"] - actual_usage["tokens"],
            "elapsed_ms_delta": None
            if self_report["elapsed_ms"] is None or actual_usage["elapsed_ms"] is None
            else self_report["elapsed_ms"] - actual_usage["elapsed_ms"],
            "tool_calls_delta": None
            if self_report["tool_calls"] is None or actual_usage["tool_calls"] is None
            else self_report["tool_calls"] - actual_usage["tool_calls"],
        }

    evidence_incomplete = bool(self_report and not actual_usage)
    return {
        "stage": stage_result.get("stage"),
        "role": stage_result.get("role"),
        "usage_blocks_found": len(usage_blocks),
        "actual_usage": actual_usage,
        "self_report": self_report,
        "drift": drift,
        "metrics_cross_check": cross_check,
        "evidence_incomplete": evidence_incomplete,
    }


def build_usage_drift_report(stage_results: list[dict]) -> dict:
    stages = [collect_stage_usage(stage_result) for stage_result in stage_results if stage_result]
    evidence_incomplete = any(stage["evidence_incomplete"] for stage in stages)
    cross_check_failed = any(
        (stage["metrics_cross_check"] or {}).get("status") == "mismatch"
        for stage in stages
    )
    return {
        "schema_version": "codelens-release-usage-drift-v1",
        "generated_at": common.now_iso(),
        "stages": stages,
        "evidence_incomplete": evidence_incomplete,
        "cross_check_failed": cross_check_failed,
        "release_blocking": evidence_incomplete or cross_check_failed,
    }


def render_usage_drift_markdown(report: dict) -> str:
    lines = [
        "# Usage Drift",
        "",
        f"- Release blocking: {report['release_blocking']}",
        f"- Evidence incomplete: {report['evidence_incomplete']}",
        f"- Metrics cross-check failed: {report['cross_check_failed']}",
        "",
        "| Stage | Actual usage | Self-report | Cross-check |",
        "|---|---|---|---|",
    ]
    for stage in report.get("stages", []):
        actual = stage.get("actual_usage") or {}
        self_report = stage.get("self_report") or {}
        actual_text = (
            f"tokens={actual.get('tokens', '-')}, elapsed_ms={actual.get('elapsed_ms', '-')}, tool_calls={actual.get('tool_calls', '-')}"
            if actual
            else "-"
        )
        self_text = (
            f"tokens={self_report.get('tokens', '-')}, elapsed_ms={self_report.get('elapsed_ms', '-')}, tool_calls={self_report.get('tool_calls', '-')}"
            if self_report
            else "-"
        )
        lines.append(
            f"| {stage.get('stage')} | {actual_text} | {self_text} | {(stage.get('metrics_cross_check') or {}).get('status', '-')} |"
        )
    lines.append("")
    return "\n".join(lines)


def build_signoff_payload(
    *,
    artifact_path: Path,
    evaluator_result: dict,
    independent_result: dict,
    usage_drift_report: dict,
    execution_completed: bool,
) -> dict:
    disagreement = False
    if evaluator_result.get("parsed") and independent_result.get("parsed"):
        disagreement = evaluator_result.get("verdict") != independent_result.get("verdict")
        if not disagreement:
            left = evaluator_result.get("aggregate_score")
            right = independent_result.get("aggregate_score")
            if left is not None and right is not None:
                disagreement = abs(float(left) - float(right)) > 1e-6
    verdict = independent_result.get("verdict")
    if not execution_completed:
        status = "planned"
    else:
        status = (
            "pass"
            if verdict == "PASS"
            and not disagreement
            and not usage_drift_report.get("release_blocking")
            else "fail"
        )
    return {
        "schema_version": "codelens-release-independent-signoff-v1",
        "generated_at": common.now_iso(),
        "artifact_path": str(artifact_path),
        "status": status,
        "independent_verdict": verdict,
        "independent_aggregate_score": independent_result.get("aggregate_score"),
        "independent_summary": independent_result.get("summary"),
        "self_grade_verdict": evaluator_result.get("verdict"),
        "self_grade_aggregate_score": evaluator_result.get("aggregate_score"),
        "disagreement": disagreement,
        "usage_drift_release_blocking": usage_drift_report.get("release_blocking"),
        "independent_parsed": independent_result.get("parsed", False),
    }


def render_signoff_markdown(signoff: dict) -> str:
    lines = [
        "# Independent Signoff",
        "",
        f"- Status: {signoff.get('status', '(missing)')}",
        f"- Independent verdict: {signoff.get('independent_verdict') or '(missing)'}",
        f"- Self-grade verdict: {signoff.get('self_grade_verdict') or '(missing)'}",
        f"- Disagreement: {signoff.get('disagreement', '(missing)')}",
        f"- Usage drift release blocking: {signoff.get('usage_drift_release_blocking', '(missing)')}",
        "",
    ]
    return "\n".join(lines)


def write_report_artifacts(run_dir: Path, *, usage_drift_report: dict, signoff: dict) -> dict:
    usage_json = run_dir / "usage-drift.json"
    usage_md = run_dir / "usage-drift.md"
    signoff_json = run_dir / "independent-signoff.json"
    signoff_md = run_dir / "independent-signoff.md"
    usage_json.write_text(json.dumps(usage_drift_report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    usage_md.write_text(render_usage_drift_markdown(usage_drift_report) + "\n", encoding="utf-8")
    signoff_json.write_text(json.dumps(signoff, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    signoff_md.write_text(render_signoff_markdown(signoff) + "\n", encoding="utf-8")
    return {
        "usage_drift_json": str(usage_json),
        "usage_drift_markdown": str(usage_md),
        "independent_signoff_json": str(signoff_json),
        "independent_signoff_markdown": str(signoff_md),
    }


def checkpoint_for_stage(
    manifest_path: Path,
    event_log_path: Path,
    stage_name: str,
    stage_result: dict,
) -> None:
    artifacts = {
        "stage_result_file": stage_result.get("stage_result_file", ""),
        **(stage_result.get("artifacts") or {}),
    }
    details = {
        "role": stage_result.get("role"),
        "runner": stage_result.get("runner"),
        "stage_dir": stage_result.get("stage_dir"),
        "returncode": stage_result.get("returncode"),
    }
    if stage_result.get("reused"):
        common.record_stage_reuse(
            manifest_path,
            event_log_path,
            stage_name,
            artifacts=artifacts,
            details=details,
        )
        return
    common.checkpoint_run_stage(
        manifest_path,
        event_log_path,
        stage_name,
        status=stage_result.get("status", "completed"),
        artifacts=artifacts,
        details=details,
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--run-dir", default="")
    parser.add_argument("--exec", action="store_true")
    args = parser.parse_args()

    manifest_path_input = Path(args.manifest).expanduser().resolve()
    manifest = load_manifest(manifest_path_input)
    repo_path = Path(manifest["runner_defaults"]["repo"]).expanduser().resolve()
    artifact_path = resolve_artifact_path(repo_path, manifest["artifact_path"])

    run_dir = (
        Path(args.run_dir).expanduser()
        if args.run_dir
        else DEFAULT_RUN_DIR / f"{common.slugify(repo_path.name)}-{common.slugify(artifact_path.name)}"
    )
    run_dir.mkdir(parents=True, exist_ok=True)
    copied_manifest = run_dir / "release-harness-manifest.json"
    copied_manifest.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    parent_manifest_path, event_log_path, _ = common.ensure_run_manifest(
        run_dir=run_dir,
        runner="release-harness-runner",
        agent="release-harness",
        repo_path=repo_path,
        execution_repo_path=repo_path,
        task_kind=manifest["runner_defaults"]["task_kind"],
        mode=manifest["runner_defaults"]["mode"],
        scenario_id=manifest.get("scenario_id"),
        recommended_policy="release-harness",
        route_mode=manifest["runner_defaults"]["mode"],
    )
    common.checkpoint_run_stage(
        parent_manifest_path,
        event_log_path,
        "preflight",
        status="completed",
        artifacts={"release_manifest": copied_manifest},
        details={
            "artifact_path": str(artifact_path),
            "repair_max_rounds": manifest["repair"]["max_rounds"],
        },
    )

    stage_results: dict[str, dict] = {}
    worker_summary_path = None
    try:
        worker_prompt = build_stage_prompt(manifest, "worker", artifact_path)
        worker_result = run_stage(
            manifest,
            "worker",
            "worker_scan",
            repo_path=repo_path,
            run_dir=run_dir,
            prompt=worker_prompt,
            exec_requested=args.exec,
        )
        checkpoint_for_stage(parent_manifest_path, event_log_path, "worker_scan", worker_result)
        stage_results["worker_scan"] = worker_result
        worker_summary_path = Path(worker_result["artifacts"]["last_message_file"]) if worker_result.get("artifacts", {}).get("last_message_file") else None

        orchestrator_prompt = build_stage_prompt(
            manifest,
            "orchestrator",
            artifact_path,
            worker_summary_path=worker_summary_path,
        )
        orchestrator_result = run_stage(
            manifest,
            "orchestrator",
            "orchestrator",
            repo_path=repo_path,
            run_dir=run_dir,
            prompt=orchestrator_prompt,
            exec_requested=args.exec,
        )
        checkpoint_for_stage(parent_manifest_path, event_log_path, "orchestrator", orchestrator_result)
        stage_results["orchestrator"] = orchestrator_result

        evaluator_prompt = build_stage_prompt(manifest, "evaluator", artifact_path)
        evaluator_result = run_stage(
            manifest,
            "evaluator",
            "evaluator",
            repo_path=repo_path,
            run_dir=run_dir,
            prompt=evaluator_prompt,
            exec_requested=args.exec,
        )
        checkpoint_for_stage(parent_manifest_path, event_log_path, "evaluator", evaluator_result)
        stage_results["evaluator"] = evaluator_result
        evaluator_verdict = parse_evaluator_result(stage_dir(run_dir, "evaluator") / "last-message.md")

        if (
            args.exec
            and evaluator_verdict.get("verdict") == "FAIL"
            and manifest["repair"]["max_rounds"] > 0
        ):
            repair_prompt = build_stage_prompt(
                manifest,
                "orchestrator",
                artifact_path,
                worker_summary_path=worker_summary_path,
                repair_hints=evaluator_verdict.get("repair_hints") or [],
            )
            repair_result = run_stage(
                manifest,
                "orchestrator",
                "orchestrator_repair",
                repo_path=repo_path,
                run_dir=run_dir,
                prompt=repair_prompt,
                exec_requested=True,
            )
            checkpoint_for_stage(
                parent_manifest_path,
                event_log_path,
                "orchestrator_repair",
                repair_result,
            )
            stage_results["orchestrator_repair"] = repair_result

            evaluator_retry_prompt = build_stage_prompt(manifest, "evaluator", artifact_path)
            evaluator_retry_result = run_stage(
                manifest,
                "evaluator",
                "evaluator_repair",
                repo_path=repo_path,
                run_dir=run_dir,
                prompt=evaluator_retry_prompt,
                exec_requested=True,
            )
            checkpoint_for_stage(
                parent_manifest_path,
                event_log_path,
                "evaluator_repair",
                evaluator_retry_result,
            )
            stage_results["evaluator_repair"] = evaluator_retry_result
            evaluator_verdict = parse_evaluator_result(
                stage_dir(run_dir, "evaluator_repair") / "last-message.md"
            )

        usage_drift_report = build_usage_drift_report(list(stage_results.values()))
        usage_report_paths = write_report_artifacts(
            run_dir,
            usage_drift_report=usage_drift_report,
            signoff={
                "schema_version": "codelens-release-independent-signoff-v1",
                "generated_at": common.now_iso(),
                "status": "pending",
                "artifact_path": str(artifact_path),
            },
        )
        common.checkpoint_run_stage(
            parent_manifest_path,
            event_log_path,
            "usage_drift",
            status="completed",
            artifacts={
                "usage_drift_json": usage_report_paths["usage_drift_json"],
                "usage_drift_markdown": usage_report_paths["usage_drift_markdown"],
            },
            details={
                "release_blocking": usage_drift_report["release_blocking"],
            },
        )

        independent_prompt = build_stage_prompt(
            manifest,
            "independent_evaluator",
            artifact_path,
            usage_drift_path=Path(usage_report_paths["usage_drift_markdown"]),
        )
        independent_result = run_stage(
            manifest,
            "independent_evaluator",
            "independent_signoff",
            repo_path=repo_path,
            run_dir=run_dir,
            prompt=independent_prompt,
            exec_requested=args.exec,
        )
        checkpoint_for_stage(
            parent_manifest_path,
            event_log_path,
            "independent_signoff",
            independent_result,
        )
        stage_results["independent_signoff"] = independent_result
        independent_verdict = parse_evaluator_result(
            stage_dir(run_dir, "independent_signoff") / "last-message.md"
        )

        signoff = build_signoff_payload(
            artifact_path=artifact_path,
            evaluator_result=evaluator_verdict,
            independent_result=independent_verdict,
            usage_drift_report=usage_drift_report,
            execution_completed=args.exec,
        )
        usage_report_paths = write_report_artifacts(
            run_dir,
            usage_drift_report=usage_drift_report,
            signoff=signoff,
        )
        common.checkpoint_run_stage(
            parent_manifest_path,
            event_log_path,
            "final_signoff",
            status="completed",
            artifacts=usage_report_paths,
            details={
                "status": signoff["status"],
                "independent_verdict": signoff["independent_verdict"],
                "disagreement": signoff["disagreement"],
            },
        )

        result = {
            "status": signoff["status"],
            "run_dir": str(run_dir),
            "run_manifest": str(parent_manifest_path),
            "artifact_path": str(artifact_path),
            "usage_drift": usage_drift_report,
            "independent_signoff": signoff,
            "artifacts": usage_report_paths,
            "stages": {
                key: {
                    "status": value.get("status"),
                    "stage_dir": value.get("stage_dir"),
                    "stage_result_file": value.get("stage_result_file"),
                    "artifacts": value.get("artifacts"),
                }
                for key, value in stage_results.items()
            },
        }
        print(json.dumps(result, ensure_ascii=False, indent=2))
    except StageExecutionError as exc:
        checkpoint_for_stage(parent_manifest_path, event_log_path, exc.stage_name, exc.stage_result)
        common.checkpoint_run_stage(
            parent_manifest_path,
            event_log_path,
            "final_signoff",
            status="failed",
            details={
                "failed_stage": exc.stage_name,
                "returncode": exc.stage_result.get("returncode"),
            },
        )
        print(
            json.dumps(
                {
                    "status": "failed",
                    "failed_stage": exc.stage_name,
                    "run_dir": str(run_dir),
                    "run_manifest": str(parent_manifest_path),
                    "stage_result": exc.stage_result,
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        raise SystemExit(1)


if __name__ == "__main__":
    main()
