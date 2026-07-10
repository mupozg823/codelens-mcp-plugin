"""Measured agent subprocess execution for productivity studies."""

from __future__ import annotations

import subprocess
import time
from pathlib import Path
from typing import Callable

from productivity_study_agents import AgentInvocation, build_agent_command
from productivity_study_events import (
    AgentTelemetry,
    JsonObject,
    extract_final_response,
    parse_agent_stream,
)
from productivity_study_home import isolated_study_environment


def run_agent(
    invocation: AgentInvocation,
    candidate: Path,
    raw_path: Path,
    timeout_seconds: int,
    sample_resource: Callable[[], None] | None = None,
) -> JsonObject:
    """Run one measured agent after its isolated HOME has been prepared."""
    stdout_path = raw_path.with_name("agent.stdout")
    stderr_path = raw_path.with_name("agent.stderr")
    with isolated_study_environment(candidate) as environment:
        started = time.monotonic()
        with (
            stdout_path.open("w", encoding="utf-8") as stdout,
            stderr_path.open("w", encoding="utf-8") as stderr,
        ):
            process = subprocess.Popen(
                build_agent_command(invocation),
                cwd=candidate,
                env=environment,
                stdout=stdout,
                stderr=stderr,
                text=True,
            )
            deadline = started + timeout_seconds
            timed_out = False
            while process.poll() is None:
                if sample_resource is not None:
                    sample_resource()
                if time.monotonic() >= deadline:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)
                    timed_out = True
                    break
                time.sleep(0.25)
        stdout_text = stdout_path.read_text(encoding="utf-8")
        stderr_text = stderr_path.read_text(encoding="utf-8")
        stdout_path.unlink(missing_ok=True)
        stderr_path.unlink(missing_ok=True)
        invocation.claude_mcp_config.unlink(missing_ok=True)
        raw_text = f"{stdout_text}\n{stderr_text}"
        raw_path.write_text(raw_text, encoding="utf-8")
        telemetry = parse_agent_stream(invocation.agent, stdout_text)
        failure = (
            "agent timeout"
            if timed_out
            else (stderr_text[-500:].strip() if process.returncode else None)
        )
        result: JsonObject = {
            "agent_exit_code": 124 if timed_out else process.returncode,
            "wall_ms": int((time.monotonic() - started) * 1000),
            "agent_usage": usage_payload(telemetry),
            "agent_activity": activity_payload(telemetry),
            "mcp_metrics": not_used_mcp_metrics(),
            "response": extract_final_response(invocation.agent, stdout_text) or "",
            "failure_excerpt": failure,
        }
    return result


def usage_payload(telemetry: AgentTelemetry) -> JsonObject:
    usage = telemetry.usage
    return {
        "status": usage.status.value,
        "input_tokens": usage.input_tokens,
        "cached_tokens": usage.cached_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.total_tokens,
    }


def activity_payload(telemetry: AgentTelemetry) -> JsonObject:
    activity = telemetry.activity
    return {
        "turns": activity.turns,
        "tool_calls": activity.tool_calls,
        "codelens_calls": activity.codelens_calls,
        "file_write_events": activity.file_write_events,
        "revisited_write_paths": activity.revisited_write_paths,
        "test_commands": activity.test_commands,
        "failed_test_commands": activity.failed_test_commands,
    }


def not_used_mcp_metrics() -> JsonObject:
    return {
        "status": "not-used",
        "context_tokens": 0,
        "handle_reuse_count": 0,
        "duplicate_retrieval_count": 0,
        "external_fallback_count": 0,
        "tool_latency_p50_ms": None,
        "tool_latency_p95_ms": None,
        "daemon_cpu_ms": 0,
        "peak_rss_bytes": 0,
    }
