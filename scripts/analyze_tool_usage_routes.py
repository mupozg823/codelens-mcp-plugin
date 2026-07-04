from __future__ import annotations

from typing import Final

HEALTH_TOOLS: Final = {
    "activate_project",
    "get_current_config",
    "prepare_harness_session",
    "set_preset",
    "set_profile",
}
WORKFLOW_TOOLS: Final = {
    "cleanup_duplicate_logic",
    "explore_codebase",
    "impact_report",
    "plan_safe_refactor",
    "review_architecture",
    "review_changes",
    "start_analysis_job",
    "trace_request_path",
    "verify_change_readiness",
}
USER_CLARIFICATION_TOOLS: Final = {
    "AskUserQuestion",
    "functions.request_user_input",
    "request_user_input",
}
NATIVE_TOOLS: Final = {
    "Bash",
    "Edit",
    "Read",
    "Write",
    "apply_patch",
    "exec_command",
    "functions.apply_patch",
    "functions.exec_command",
    "functions.write_stdin",
    "multi_tool_use.parallel",
    "view_image",
    "write_stdin",
}
DYNAMIC_WORKFLOW_TOOLS: Final = {
    "Task",
    "TaskCreate",
    "TaskUpdate",
    "TodoWrite",
}
SERENA_PREFIXES: Final = ("mcp__serena", "serena.")


def is_serena_tool(tool: str) -> bool:
    return tool.startswith(SERENA_PREFIXES)


def missed_route_label(
    next_codelens_tools: list[str],
    next_external_tools: list[str],
) -> str:
    if any(tool in USER_CLARIFICATION_TOOLS for tool in next_external_tools):
        return "user_clarification"
    if any(is_serena_tool(tool) for tool in next_external_tools):
        return "serena_fallback"
    if any(tool in DYNAMIC_WORKFLOW_TOOLS for tool in next_external_tools):
        return "dynamic_workflow"
    if any(tool in NATIVE_TOOLS for tool in next_external_tools):
        return "native_fallback"
    if not next_codelens_tools:
        return "no_codelens_followup"
    first_tool = next_codelens_tools[0]
    if first_tool in HEALTH_TOOLS:
        return "rebootstrap_or_health_check"
    if any(tool in WORKFLOW_TOOLS for tool in next_codelens_tools):
        return "workflow_alternative"
    return "other_codelens_followup"
