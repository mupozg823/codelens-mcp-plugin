from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ANALYZER = REPO_ROOT / "scripts" / "analyze-tool-usage.py"


def run_rollout_analyzer(rollout_path: Path) -> dict:
    proc = subprocess.run(
        [
            sys.executable,
            str(ANALYZER),
            "--codex-rollout-path",
            str(rollout_path),
            "--format",
            "json",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert proc.returncode == 0, (
        f"rollout analyzer should pass: stdout={proc.stdout} stderr={proc.stderr}"
    )
    return json.loads(proc.stdout)


def write_jsonl(path: Path, events: list[dict]) -> None:
    path.write_text(
        "\n".join(json.dumps(event) for event in events) + "\n",
        encoding="utf-8",
    )


def agent_message(message: str) -> dict:
    return {
        "type": "event_msg",
        "payload": {"type": "agent_message", "message": message},
    }


def codelens_call(tool: str, args: str) -> dict:
    return agent_message(
        f"[external_agent_tool_call: mcp__codelens__{tool}]\n"
        f"input: {args}\n"
        "[/external_agent_tool_call]"
    )


def external_tool_call(tool: str, args: str) -> dict:
    return agent_message(
        f"[external_agent_tool_call: {tool}]\n"
        f"input: {args}\n"
        "[/external_agent_tool_call]"
    )


def codelens_result(payload: str) -> dict:
    return agent_message(
        "[external_agent_tool_result]\n"
        f"{payload}\n"
        "[/external_agent_tool_result]"
    )


def external_tool_result(payload: str) -> dict:
    return agent_message(
        "[external_agent_tool_result]\n"
        f"{payload}\n"
        "[/external_agent_tool_result]"
    )


def codex_function_call(
    name: str,
    arguments: str,
    call_id: str = "call-codex",
) -> dict:
    return {
        "type": "response_item",
        "payload": {
            "type": "function_call",
            "name": name,
            "arguments": arguments,
            "call_id": call_id,
        },
    }


def codex_function_output(output: str, call_id: str = "call-codex") -> dict:
    return {
        "type": "response_item",
        "payload": {
            "type": "function_call_output",
            "call_id": call_id,
            "output": output,
        },
    }
