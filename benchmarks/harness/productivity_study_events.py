"""Normalize native agent event streams without treating missing metrics as zero."""

from __future__ import annotations

import json
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
from typing import TypeAlias, assert_never

from productivity_study_contract import Agent


JsonScalar: TypeAlias = str | int | float | bool | None
JsonValue: TypeAlias = JsonScalar | list["JsonValue"] | dict[str, "JsonValue"]
JsonObject: TypeAlias = dict[str, JsonValue]


class MeasurementStatus(StrEnum):
    AVAILABLE = "available"
    UNAVAILABLE = "unavailable"


@dataclass(frozen=True, slots=True)
class AgentUsage:
    status: MeasurementStatus
    input_tokens: int | None
    cached_tokens: int | None
    output_tokens: int | None
    total_tokens: int | None


@dataclass(frozen=True, slots=True)
class AgentActivity:
    turns: int
    tool_calls: int
    codelens_calls: int
    file_write_events: int
    revisited_write_paths: int
    test_commands: int
    failed_test_commands: int


@dataclass(frozen=True, slots=True)
class AgentTelemetry:
    usage: AgentUsage
    activity: AgentActivity


def parse_agent_stream(agent: Agent, text: str) -> AgentTelemetry:
    records = tuple(parsed_records(text))
    match agent:
        case Agent.CODEX:
            return parse_codex(records)
        case Agent.CLAUDE:
            return parse_claude(records)
        case _ as unreachable:
            assert_never(unreachable)


def extract_final_response(agent: Agent, text: str) -> str | None:
    records = tuple(parsed_records(text))
    match agent:
        case Agent.CODEX:
            for record in reversed(records):
                item = object_field(record, "item")
                if item is not None and string_field(item, "type") == "agent_message":
                    return string_field(item, "text")
        case Agent.CLAUDE:
            for record in reversed(records):
                if string_field(record, "type") == "result":
                    return string_field(record, "result")
    return None


def parsed_records(text: str) -> list[JsonObject]:
    records: list[JsonObject] = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        try:
            decoded = json.loads(stripped)
        except json.JSONDecodeError:
            continue
        if isinstance(decoded, dict) and all(isinstance(key, str) for key in decoded):
            records.append(decoded)
    return records


def object_field(record: JsonObject, key: str) -> JsonObject | None:
    value = record.get(key)
    return value if isinstance(value, dict) else None


def list_field(record: JsonObject, key: str) -> list[JsonValue]:
    value = record.get(key)
    return value if isinstance(value, list) else []


def int_field(record: JsonObject, key: str) -> int | None:
    value = record.get(key)
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def string_field(record: JsonObject, key: str) -> str | None:
    value = record.get(key)
    return value if isinstance(value, str) else None


def parse_codex(records: tuple[JsonObject, ...]) -> AgentTelemetry:
    write_paths: Counter[str] = Counter()
    tool_calls = 0
    codelens_calls = 0
    test_commands = 0
    failed_test_commands = 0
    turns = 0
    usage: AgentUsage | None = None

    for record in records:
        item = object_field(record, "item")
        if item is not None:
            item_type = string_field(item, "type")
            if item_type == "mcp_tool_call":
                tool_calls += 1
                if string_field(item, "server") == "codelens":
                    codelens_calls += 1
            if item_type == "file_change":
                for change in list_field(item, "changes"):
                    if isinstance(change, dict):
                        path = string_field(change, "path")
                        if path is not None:
                            write_paths[path] += 1
            if item_type == "command_execution":
                command = string_field(item, "command")
                if command is not None and is_test_command(command):
                    test_commands += 1
                    if int_field(item, "exit_code") not in {None, 0}:
                        failed_test_commands += 1
        if string_field(record, "type") == "turn.completed":
            turns += 1
            candidate = usage_from_record(record)
            if candidate is not None:
                usage = candidate

    return telemetry_from_parts(
        usage,
        turns,
        tool_calls,
        codelens_calls,
        write_paths,
        test_commands,
        failed_test_commands,
    )


def parse_claude(records: tuple[JsonObject, ...]) -> AgentTelemetry:
    write_paths: Counter[str] = Counter()
    tool_calls = 0
    codelens_calls = 0
    test_commands = 0
    failed_test_commands = 0
    turns = 0
    usage: AgentUsage | None = None

    for record in records:
        if string_field(record, "type") == "assistant":
            message = object_field(record, "message")
            if message is not None:
                for content in list_field(message, "content"):
                    if not isinstance(content, dict):
                        continue
                    if string_field(content, "type") != "tool_use":
                        continue
                    tool_calls += 1
                    tool_name = string_field(content, "name") or ""
                    if "codelens" in tool_name.casefold():
                        codelens_calls += 1
                    input_data = object_field(content, "input")
                    if input_data is not None:
                        path = string_field(input_data, "file_path")
                        if tool_name in {"Edit", "Write", "NotebookEdit"} and path:
                            write_paths[path] += 1
                        command = string_field(input_data, "command")
                        if command is not None and is_test_command(command):
                            test_commands += 1
        if string_field(record, "type") == "result":
            turns += 1
            candidate = usage_from_record(record)
            if candidate is not None:
                usage = candidate
            if record.get("is_error") is True:
                failed_test_commands += test_commands

    return telemetry_from_parts(
        usage,
        turns,
        tool_calls,
        codelens_calls,
        write_paths,
        test_commands,
        failed_test_commands,
    )


def usage_from_record(record: JsonObject) -> AgentUsage | None:
    raw_usage = object_field(record, "usage")
    if raw_usage is None:
        return None
    input_tokens = int_field(raw_usage, "input_tokens")
    cached_tokens = int_field(raw_usage, "cached_input_tokens")
    if cached_tokens is None:
        cached_tokens = int_field(raw_usage, "cache_read_input_tokens")
    output_tokens = int_field(raw_usage, "output_tokens")
    total_tokens = int_field(raw_usage, "total_tokens")
    if total_tokens is None and input_tokens is not None and output_tokens is not None:
        total_tokens = input_tokens + output_tokens
    if input_tokens is None or output_tokens is None or total_tokens is None:
        return None
    return AgentUsage(
        status=MeasurementStatus.AVAILABLE,
        input_tokens=input_tokens,
        cached_tokens=cached_tokens,
        output_tokens=output_tokens,
        total_tokens=total_tokens,
    )


def telemetry_from_parts(
    usage: AgentUsage | None,
    turns: int,
    tool_calls: int,
    codelens_calls: int,
    write_paths: Counter[str],
    test_commands: int,
    failed_test_commands: int,
) -> AgentTelemetry:
    return AgentTelemetry(
        usage=usage
        or AgentUsage(
            status=MeasurementStatus.UNAVAILABLE,
            input_tokens=None,
            cached_tokens=None,
            output_tokens=None,
            total_tokens=None,
        ),
        activity=AgentActivity(
            turns=turns,
            tool_calls=tool_calls,
            codelens_calls=codelens_calls,
            file_write_events=sum(write_paths.values()),
            revisited_write_paths=sum(count - 1 for count in write_paths.values()),
            test_commands=test_commands,
            failed_test_commands=failed_test_commands,
        ),
    )


def is_test_command(command: str) -> bool:
    normalized = command.casefold()
    return " test" in normalized or normalized.startswith(("pytest", "cargo test"))
