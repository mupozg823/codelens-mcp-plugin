"""Ephemeral daemon and agent-process runtime for productivity studies."""

from __future__ import annotations

import json
import socket
import subprocess
import time
import urllib.request
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator

from productivity_study_agent_process import run_agent as run_agent
from productivity_study_binary_snapshot import bound_binary_snapshot
from productivity_study_home import isolated_study_environment, study_process_environment

@dataclass(frozen=True, slots=True)
class DaemonRuntime:
    url: str
    pid: int
    startup_ms: int
    health_session_id: str


@dataclass(frozen=True, slots=True)
class ProcessSample:
    cpu_ms: int
    rss_bytes: int


class DaemonResourceMonitor:
    def __init__(self, pid: int) -> None:
        self.pid = pid
        self.samples: list[ProcessSample] = []
        self.sample()

    def sample(self) -> None:
        observed = process_sample(self.pid)
        if observed is not None:
            self.samples.append(observed)

    def summary(self) -> dict[str, int | None]:
        self.sample()
        if len(self.samples) < 2:
            return {"daemon_cpu_ms": None, "peak_rss_bytes": None}
        return {
            "daemon_cpu_ms": self.samples[-1].cpu_ms - self.samples[0].cpu_ms,
            "peak_rss_bytes": max(sample.rss_bytes for sample in self.samples),
        }


@contextmanager
def dedicated_daemon(
    binary: Path,
    worktree: Path,
    telemetry_path: Path | None = None,
    *,
    expected_sha256: str,
) -> Iterator[DaemonRuntime]:
    overlays = (
        {"CODELENS_TELEMETRY_PATH": str(telemetry_path)}
        if telemetry_path is not None
        else None
    )
    with (
        bound_binary_snapshot(binary, expected_sha256) as bound_binary,
        isolated_study_environment(worktree, overlays) as environment,
    ):
        port = unused_local_port()
        command = build_daemon_command(bound_binary, worktree, port)
        started = time.monotonic()
        process = subprocess.Popen(
            command,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=environment,
        )
        url = f"http://127.0.0.1:{port}/mcp"
        try:
            health_session_id = open_mcp_session(url)
            yield DaemonRuntime(
                url,
                process.pid,
                int((time.monotonic() - started) * 1000),
                health_session_id,
            )
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def daemon_environment(telemetry_path: Path | None) -> dict[str, str]:
    overlays = (
        {"CODELENS_TELEMETRY_PATH": str(telemetry_path)}
        if telemetry_path is not None
        else None
    )
    return study_process_environment(overlays)


def build_daemon_command(binary: Path, worktree: Path, port: int) -> tuple[str, ...]:
    return (
        str(binary), str(worktree), "--preset", "full", "--transport", "http", "--listen", "127.0.0.1",
        "--port", str(port), "--auth", "off",
    )


def unused_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def process_sample(pid: int) -> ProcessSample | None:
    completed = subprocess.run(
        ["ps", "-p", str(pid), "-o", "rss=", "-o", "time="],
        env=study_process_environment(),
        check=False,
        capture_output=True,
        text=True,
    )
    fields = completed.stdout.split()
    if completed.returncode != 0 or len(fields) != 2 or not fields[0].isdigit():
        return None
    return ProcessSample(runtime_cpu_millis(fields[1]), int(fields[0]) * 1024)


def runtime_cpu_millis(value: str) -> int:
    days, separator, clock = value.partition("-")
    day_count = int(days) if separator else 0
    if not separator:
        clock = days
    parts = clock.split(":")
    if len(parts) == 2:
        hours, minutes = 0, int(parts[0])
    elif len(parts) == 3:
        hours, minutes = int(parts[0]), int(parts[1])
    else:
        raise ValueError(f"unsupported process CPU time: {value}")
    second_part, dot, fraction = parts[-1].partition(".")
    milliseconds = int((fraction + "000")[:3]) if dot else 0
    total_seconds = (day_count * 86_400) + (hours * 3_600) + (minutes * 60) + int(second_part)
    return (total_seconds * 1_000) + milliseconds


def open_mcp_session(url: str) -> str:
    for _ in range(30):
        try:
            _, session_id = mcp_request(
                url,
                "initialize",
                {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "productivity-study", "version": "1"},
                },
                None,
            )
            if session_id:
                return session_id
        except OSError:
            time.sleep(0.2)
    raise RuntimeError("isolated CodeLens daemon did not initialize")


def mcp_tool_call(url: str, session_id: str, name: str, arguments: dict[str, object]) -> dict:
    payload, _ = mcp_request(
        url,
        "tools/call",
        {"name": name, "arguments": arguments},
        session_id,
    )
    return payload


def metrics_snapshot(
    url: str, session_id: str, measured_session_id: str | None = None
) -> dict:
    arguments: dict[str, object] = {"compact": True}
    if measured_session_id is not None:
        arguments["session_id"] = measured_session_id
    payload = mcp_tool_call(url, session_id, "get_tool_metrics", arguments)
    result = payload.get("result")
    if not isinstance(result, dict):
        return {}
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        return unwrap_metrics_payload(structured)
    content = result.get("content")
    if not isinstance(content, list) or not content or not isinstance(content[0], dict):
        return {}
    text = content[0].get("text")
    if not isinstance(text, str):
        return {}
    decoded = json.loads(text)
    return unwrap_metrics_payload(decoded)


def unwrap_metrics_payload(payload: object) -> dict:
    if not isinstance(payload, dict):
        return {}
    data = payload.get("data")
    if isinstance(data, dict):
        return data
    return payload


def mcp_request(
    url: str, method: str, params: dict[str, object], session_id: str | None
) -> tuple[dict, str | None]:
    headers = {"content-type": "application/json"}
    if session_id:
        headers["mcp-session-id"] = session_id
    request = urllib.request.Request(
        url,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers=headers,
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        decoded = json.loads(response.read().decode())
        return decoded if isinstance(decoded, dict) else {}, response.headers.get("mcp-session-id")
