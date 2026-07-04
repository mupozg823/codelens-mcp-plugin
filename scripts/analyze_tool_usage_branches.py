from __future__ import annotations

from typing import Final

BRANCHES: Final = ("claude", "codex")

CLAUDE_TOOLS: Final = {
    "AskUserQuestion",
    "Bash",
    "Edit",
    "Glob",
    "Grep",
    "LS",
    "Read",
    "Task",
    "TaskCreate",
    "TaskUpdate",
    "TodoWrite",
    "ToolSearch",
    "Workflow",
    "Write",
}
CODEX_TOOLS: Final = {
    "apply_patch",
    "exec_command",
    "get_goal",
    "list_mcp_resources",
    "multi_tool_use.parallel",
    "read_mcp_resource",
    "tool_search_tool",
    "update_goal",
    "update_plan",
    "view_image",
    "write_stdin",
}
CLAUDE_PREFIXES: Final = (
    "mcp__serena",
    "mcp__plugin_",
    "serena.",
)
CODEX_PREFIXES: Final = (
    "codex_app.",
    "functions.",
    "image_gen.",
    "multi_tool_use.",
    "tool_search.",
    "web.",
)


def tool_branch(tool: str) -> str:
    if tool in CLAUDE_TOOLS or tool.startswith(CLAUDE_PREFIXES):
        return "claude"
    if tool in CODEX_TOOLS or tool.startswith(CODEX_PREFIXES):
        return "codex"
    return "unknown"


def branch_counts(event: dict) -> dict[str, int]:
    raw = event.get("next_branch_counts")
    if not isinstance(raw, dict):
        return {branch: 0 for branch in BRANCHES}
    return {
        branch: value if isinstance(value, int) else 0
        for branch, value in raw.items()
        if branch in BRANCHES
    } | {
        branch: 0
        for branch in BRANCHES
        if branch not in raw
    }


def agent_branch(event: dict) -> str:
    counts = branch_counts(event)
    active = [branch for branch in BRANCHES if counts.get(branch, 0) > 0]
    if len(active) > 1:
        return "mixed"
    if active:
        return active[0]
    return "unknown"
