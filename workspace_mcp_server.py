#!/usr/bin/env python3
"""
Standalone workspace MCP server for CodeLens.

This server does not require a running JetBrains IDE. It exposes a Serena-like
tool surface over stdio and operates directly on the local workspace.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional

from codelens_workspace.config import JETBRAINS_ALIAS_TOOLS
from codelens_workspace.config import REQUIRED_ONBOARDING_MEMORIES
from codelens_workspace.config import SEARCHABLE_EXTENSIONS
from codelens_workspace.config import SERENA_BASELINE_TOOLS
from codelens_workspace.config import parse_args
from codelens_workspace.config import resolve_workspace_root
from codelens_workspace.file_memory_ops import (
    _list_memory_names as server_list_memory_names,
)
from codelens_workspace.file_memory_ops import _memory_path as server_memory_path
from codelens_workspace.file_memory_ops import _validate_range as server_validate_range
from codelens_workspace.file_memory_ops import (
    create_text_file as server_create_text_file,
)
from codelens_workspace.file_memory_ops import delete_lines as server_delete_lines
from codelens_workspace.file_memory_ops import delete_memory as server_delete_memory
from codelens_workspace.file_memory_ops import edit_memory as server_edit_memory
from codelens_workspace.file_memory_ops import find_file as server_find_file
from codelens_workspace.file_memory_ops import insert_at_line as server_insert_at_line
from codelens_workspace.file_memory_ops import list_dir as server_list_dir
from codelens_workspace.file_memory_ops import list_memories as server_list_memories
from codelens_workspace.file_memory_ops import read_file as server_read_file
from codelens_workspace.file_memory_ops import read_memory as server_read_memory
from codelens_workspace.file_memory_ops import rename_memory as server_rename_memory
from codelens_workspace.file_memory_ops import replace_content as server_replace_content
from codelens_workspace.file_memory_ops import replace_lines as server_replace_lines
from codelens_workspace.file_memory_ops import write_memory as server_write_memory
from codelens_workspace.protocol import error
from codelens_workspace.protocol import read_stdio_message
from codelens_workspace.protocol import success
from codelens_workspace.protocol import tool_result
from codelens_workspace.protocol import write_stdio_message

STATEMENT_PREFIXES = (
    "return ",
    "throw ",
    "new ",
    "if ",
    "for ",
    "while ",
    "switch ",
    "catch ",
    "when ",
)
CLASS_PATTERNS = [
    re.compile(
        r"^\s*(enum\s+class|annotation\s+class|class|interface|object)\s+([A-Za-z_][A-Za-z0-9_]*)\b"
    ),
    re.compile(
        r"^\s*(?:public|private|protected|internal|abstract|final|open|sealed|data|static|export|default|actual|expect|non-sealed)\s+"
        r"(class|interface|enum|record)\s+([A-Za-z_][A-Za-z0-9_]*)\b"
    ),
]
PACKAGE_PATTERN = re.compile(r"^\s*package\s+([A-Za-z_][\w.]*)")
EXTENDS_PATTERN = re.compile(r"\bextends\s+([A-Za-z_][\w.]*)")
IMPLEMENTS_PATTERN = re.compile(r"\bimplements\s+([A-Za-z_][\w.,\s]*)")
PRIMARY_PROPERTY_PATTERN = re.compile(r"(?:val|var)\s+([A-Za-z_][A-Za-z0-9_]*)")
FUNCTION_PATTERNS = [
    re.compile(
        r"^\s*(?:public|private|protected|internal|open|abstract|override|suspend|inline|operator|tailrec|external|infix|actual|expect|\s)*fun\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("
    ),
    re.compile(
        r"^\s*(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("
    ),
    re.compile(
        r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?\([^)]*\)\s*=>"
    ),
    re.compile(
        r"^\s*(?:public|private|protected|static|final|abstract|synchronized|native|default|async|\s)+"
        r"(?:<[^>]+>\s*)?(?:[A-Za-z_][\w<>\[\],.?]*\s+)+([A-Za-z_][A-Za-z0-9_]*)\s*\([^;=]*\)\s*\{?"
    ),
]
PROPERTY_PATTERNS = [
    re.compile(
        r"^\s*(?:public|private|protected|internal|override|lateinit|const|open|final|actual|expect|\s)*(val|var)\s+([A-Za-z_][A-Za-z0-9_]*)\b"
    ),
    re.compile(r"^\s*(?:export\s+)?(const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*="),
    re.compile(
        r"^\s*((?:public|private|protected|static|final|transient|volatile|\s)+(?:[A-Za-z_][\w<>\[\],.?]*\s+)+)"
        r"([A-Za-z_][A-Za-z0-9_]*)\s*(?:=|;)"
    ),
]
RESERVED_WORDS = {"if", "for", "while", "switch", "catch", "when"}


class ToolError(Exception):
    pass


# MCP Tool Annotations (2025-03-26 spec)
# readOnlyHint=True → auto-approved, no user confirmation
# destructiveHint=True → user confirmation required
ANNO_READ = {"readOnlyHint": True, "destructiveHint": False}
ANNO_WRITE = {"readOnlyHint": False, "destructiveHint": False}
ANNO_DESTRUCTIVE = {"readOnlyHint": False, "destructiveHint": True}
ANNO_NOOP = {"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True}


@dataclass
class ToolDefinition:
    name: str
    description: str
    input_schema: Dict[str, Any]
    handler: Callable[[Dict[str, Any]], Dict[str, Any]]
    annotations: Optional[Dict[str, Any]] = None
    title: Optional[str] = None


@dataclass
class ParsedSymbol:
    name: str
    name_path: str
    kind: str
    file_path: str
    line: int
    column: int
    signature: str
    start_line: int
    end_line: int
    body: Optional[str]
    children: List["ParsedSymbol"]

    def flatten(self) -> List["ParsedSymbol"]:
        items = [self]
        for child in self.children:
            items.extend(child.flatten())
        return items

    def to_map(self, depth: int) -> Dict[str, Any]:
        payload: Dict[str, Any] = {
            "name": self.name,
            "name_path": self.name_path,
            "kind": self.kind,
            "file": self.file_path,
            "line": self.line,
            "column": self.column,
            "signature": self.signature,
        }
        if self.body is not None:
            payload["body"] = self.body
        if depth > 1 and self.children:
            payload["children"] = [child.to_map(depth - 1) for child in self.children]
        return payload


class WorkspaceMcpServer:
    _tool_error = ToolError

    def __init__(self, workspace_root: Path, root_source: str = "argument"):
        self.workspace_root = workspace_root.resolve()
        self.root_source = root_source
        self.serena_dir = self.workspace_root / ".serena"
        self.memories_dir = self.serena_dir / "memories"
        raw_tools = self._build_tools()
        for t in raw_tools.values():
            if t.annotations is None:
                t.annotations = self._auto_annotate(t.name)
            if t.title is None:
                t.title = t.name.replace("_", " ").title()
        self.tools = raw_tools
        # LSP backend — lazy init, graceful fallback to regex
        self._lsp_manager = None
        self._multi_project = None
        try:
            from codelens_workspace.lsp_manager import MultiProjectLspManager

            self._multi_project = MultiProjectLspManager(str(self.workspace_root))
            self._lsp_manager = self._multi_project.primary
        except Exception:
            pass  # LSP unavailable, regex fallback only

    list_memories = server_list_memories
    read_memory = server_read_memory
    write_memory = server_write_memory
    edit_memory = server_edit_memory
    rename_memory = server_rename_memory
    delete_memory = server_delete_memory
    read_file = server_read_file
    list_dir = server_list_dir
    find_file = server_find_file
    create_text_file = server_create_text_file
    delete_lines = server_delete_lines
    insert_at_line = server_insert_at_line
    replace_lines = server_replace_lines
    replace_content = server_replace_content
    _memory_path = server_memory_path
    _list_memory_names = server_list_memory_names
    _validate_range = server_validate_range

    @staticmethod
    def _auto_annotate(name: str) -> Optional[Dict[str, Any]]:
        """Auto-assign MCP tool annotations based on tool name patterns."""
        READ_TOOLS = {
            "get_symbols_overview",
            "find_symbol",
            "find_referencing_symbols",
            "search_for_pattern",
            "get_type_hierarchy",
            "find_referencing_code_snippets",
            "read_file",
            "list_dir",
            "find_file",
            "list_directory_tree",
            "get_current_config",
            "get_project_modules",
            "get_open_files",
            "get_file_problems",
            "get_run_configurations",
            "get_project_dependencies",
            "get_repositories",
            "check_onboarding_performed",
            "initial_instructions",
            "list_memories",
            "read_memory",
            "list_queryable_projects",
            "query_project",
            "jet_brains_find_symbol",
            "jet_brains_find_referencing_symbols",
            "jet_brains_get_symbols_overview",
            "jet_brains_type_hierarchy",
            "activate_project",
        }
        NOOP_TOOLS = {
            "think_about_collected_information",
            "think_about_task_adherence",
            "think_about_whether_you_are_done",
            "prepare_for_new_conversation",
            "summarize_changes",
            "switch_modes",
            "open_dashboard",
            "onboarding",
        }
        DESTRUCTIVE_TOOLS = {
            "execute_shell_command",
            "execute_run_configuration",
            "delete_lines",
            "delete_memory",
            "remove_project",
        }
        if name in NOOP_TOOLS:
            return ANNO_NOOP
        if name in READ_TOOLS:
            return ANNO_READ
        if name in DESTRUCTIVE_TOOLS:
            return ANNO_DESTRUCTIVE
        return ANNO_WRITE  # default: write (create, replace, insert, rename, etc.)

    def _build_tools(self) -> Dict[str, ToolDefinition]:
        return {
            tool.name: tool
            for tool in [
                ToolDefinition(
                    "activate_project",
                    "Validate and activate the current workspace project.",
                    {
                        "type": "object",
                        "properties": {
                            "project": {
                                "type": "string",
                                "description": "Optional workspace name or absolute path to validate",
                            }
                        },
                    },
                    self.activate_project,
                ),
                ToolDefinition(
                    "get_current_config",
                    "Return current workspace backend configuration and available tools.",
                    {
                        "type": "object",
                        "properties": {
                            "include_tools": {"type": "boolean", "default": True}
                        },
                    },
                    self.get_current_config,
                ),
                ToolDefinition(
                    "check_onboarding_performed",
                    "Check whether the standard Serena onboarding memories exist.",
                    {
                        "type": "object",
                        "properties": {},
                    },
                    self.check_onboarding_performed,
                ),
                ToolDefinition(
                    "initial_instructions",
                    "Return initial instructions for the standalone workspace backend.",
                    {
                        "type": "object",
                        "properties": {},
                    },
                    self.initial_instructions,
                ),
                ToolDefinition(
                    "list_memories",
                    "List Serena-compatible project memories under .serena/memories.",
                    {
                        "type": "object",
                        "properties": {"topic": {"type": "string"}},
                    },
                    self.list_memories,
                ),
                ToolDefinition(
                    "read_memory",
                    "Read a Serena-compatible memory file.",
                    {
                        "type": "object",
                        "properties": {"memory_name": {"type": "string"}},
                        "required": ["memory_name"],
                    },
                    self.read_memory,
                ),
                ToolDefinition(
                    "write_memory",
                    "Write or overwrite a Serena-compatible memory.",
                    {
                        "type": "object",
                        "properties": {
                            "memory_name": {"type": "string"},
                            "content": {"type": "string"},
                            "max_chars": {"type": "integer", "minimum": 1},
                        },
                        "required": ["memory_name", "content"],
                    },
                    self.write_memory,
                ),
                ToolDefinition(
                    "edit_memory",
                    "Replace the contents of an existing Serena memory.",
                    {
                        "type": "object",
                        "properties": {
                            "memory_name": {"type": "string"},
                            "content": {"type": "string"},
                            "max_chars": {"type": "integer", "minimum": 1},
                        },
                        "required": ["memory_name", "content"],
                    },
                    self.edit_memory,
                ),
                ToolDefinition(
                    "rename_memory",
                    "Rename a Serena memory entry.",
                    {
                        "type": "object",
                        "properties": {
                            "old_name": {"type": "string"},
                            "new_name": {"type": "string"},
                        },
                        "required": ["old_name", "new_name"],
                    },
                    self.rename_memory,
                ),
                ToolDefinition(
                    "delete_memory",
                    "Delete a Serena-compatible memory file.",
                    {
                        "type": "object",
                        "properties": {"memory_name": {"type": "string"}},
                        "required": ["memory_name"],
                    },
                    self.delete_memory,
                ),
                ToolDefinition(
                    "get_symbols_overview",
                    "Return a structural overview of symbols in a file or directory.",
                    {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "depth": {"type": "integer", "default": 1},
                        },
                        "required": ["path"],
                    },
                    self.get_symbols_overview,
                ),
                ToolDefinition(
                    "find_symbol",
                    "Find a symbol by name within the workspace.",
                    {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "name_path": {"type": "string"},
                            "file_path": {"type": "string"},
                            "include_body": {"type": "boolean", "default": False},
                            "exact_match": {"type": "boolean", "default": True},
                        },
                    },
                    self.find_symbol,
                ),
                ToolDefinition(
                    "find_referencing_symbols",
                    "Find references to a symbol name in the workspace.",
                    {
                        "type": "object",
                        "properties": {
                            "symbol_name": {"type": "string"},
                            "name_path": {"type": "string"},
                            "file_path": {"type": "string"},
                            "max_results": {"type": "integer", "default": 50},
                        },
                    },
                    self.find_referencing_symbols,
                ),
                ToolDefinition(
                    "rename_symbol",
                    "Rename a symbol across the workspace or within a single file.",
                    {
                        "type": "object",
                        "properties": {
                            "symbol_name": {"type": "string"},
                            "name_path": {"type": "string"},
                            "file_path": {"type": "string"},
                            "new_name": {"type": "string"},
                            "scope": {
                                "type": "string",
                                "enum": ["file", "project"],
                                "default": "project",
                            },
                        },
                        "required": ["file_path", "new_name"],
                    },
                    self.rename_symbol,
                ),
                ToolDefinition(
                    "replace_symbol_body",
                    "Replace the declaration body/range for a symbol in a file.",
                    {
                        "type": "object",
                        "properties": {
                            "symbol_name": {"type": "string"},
                            "name_path": {"type": "string"},
                            "file_path": {"type": "string"},
                            "new_body": {"type": "string"},
                        },
                        "required": ["file_path", "new_body"],
                    },
                    self.replace_symbol_body,
                ),
                ToolDefinition(
                    "insert_after_symbol",
                    "Insert content after the declaration range for a symbol.",
                    {
                        "type": "object",
                        "properties": {
                            "symbol_name": {"type": "string"},
                            "name_path": {"type": "string"},
                            "file_path": {"type": "string"},
                            "content": {"type": "string"},
                        },
                        "required": ["file_path", "content"],
                    },
                    self.insert_after_symbol,
                ),
                ToolDefinition(
                    "insert_before_symbol",
                    "Insert content before the declaration range for a symbol.",
                    {
                        "type": "object",
                        "properties": {
                            "symbol_name": {"type": "string"},
                            "name_path": {"type": "string"},
                            "file_path": {"type": "string"},
                            "content": {"type": "string"},
                        },
                        "required": ["file_path", "content"],
                    },
                    self.insert_before_symbol,
                ),
                ToolDefinition(
                    "search_for_pattern",
                    "Search for a regex pattern across workspace files.",
                    {
                        "type": "object",
                        "properties": {
                            "pattern": {"type": "string"},
                            "substring_pattern": {
                                "type": "string",
                                "description": "Serena alias for pattern",
                            },
                            "file_glob": {"type": "string"},
                            "paths_include_glob": {"type": "string"},
                            "paths_exclude_glob": {"type": "string"},
                            "relative_path": {"type": "string"},
                            "max_results": {"type": "integer", "default": 50},
                            "context_lines": {"type": "integer", "default": 0},
                            "context_lines_before": {"type": "integer", "default": 0},
                            "context_lines_after": {"type": "integer", "default": 0},
                            "restrict_search_to_code_files": {
                                "type": "boolean",
                                "default": False,
                            },
                            "max_answer_chars": {"type": "integer", "default": -1},
                        },
                        "required": [],
                    },
                    self.search_for_pattern,
                ),
                ToolDefinition(
                    "get_type_hierarchy",
                    "Return degraded type hierarchy information for the workspace backend.",
                    {
                        "type": "object",
                        "properties": {"fully_qualified_name": {"type": "string"}},
                        "required": ["fully_qualified_name"],
                    },
                    self.get_type_hierarchy,
                ),
                ToolDefinition(
                    "think_about_collected_information",
                    "Reflect on collected information before proceeding. No side effects.",
                    {"type": "object", "properties": {}},
                    lambda args: {"status": "ok"},
                ),
                ToolDefinition(
                    "think_about_task_adherence",
                    "Reflect on whether current approach aligns with the task. No side effects.",
                    {"type": "object", "properties": {}},
                    lambda args: {"status": "ok"},
                ),
                ToolDefinition(
                    "think_about_whether_you_are_done",
                    "Evaluate whether the current task is complete. No side effects.",
                    {"type": "object", "properties": {}},
                    lambda args: {"status": "ok"},
                ),
                ToolDefinition(
                    "list_queryable_projects",
                    "List projects available for cross-project queries. In workspace mode, returns the current project only.",
                    {
                        "type": "object",
                        "properties": {
                            "symbol_access": {"type": "boolean", "default": True}
                        },
                    },
                    self.list_queryable_projects,
                ),
                ToolDefinition(
                    "query_project",
                    "Execute a read-only tool on the current project (workspace mode only supports single project).",
                    {
                        "type": "object",
                        "properties": {
                            "project_name": {"type": "string"},
                            "tool_name": {"type": "string"},
                            "tool_params_json": {"type": "string"},
                        },
                        "required": ["project_name", "tool_name", "tool_params_json"],
                    },
                    self.query_project,
                ),
                ToolDefinition(
                    "add_project",
                    "Register an additional project for cross-project queries.",
                    {
                        "type": "object",
                        "properties": {"project_path": {"type": "string"}},
                        "required": ["project_path"],
                    },
                    self.add_project,
                ),
                ToolDefinition(
                    "execute_shell_command",
                    "Execute a shell command in the workspace.",
                    {
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"},
                            "cwd": {"type": "string"},
                            "capture_stderr": {"type": "boolean", "default": True},
                        },
                        "required": ["command"],
                    },
                    self.execute_shell_command,
                ),
                ToolDefinition(
                    "onboarding",
                    "Perform initial project onboarding — analyze structure and create memories.",
                    {"type": "object", "properties": {}},
                    self.onboarding,
                ),
                ToolDefinition(
                    "prepare_for_new_conversation",
                    "Prepare for a new conversation session.",
                    {"type": "object", "properties": {}},
                    self.prepare_for_new_conversation,
                ),
                ToolDefinition(
                    "remove_project",
                    "Remove a project from the workspace configuration.",
                    {
                        "type": "object",
                        "properties": {"project_path": {"type": "string"}},
                        "required": ["project_path"],
                    },
                    self.remove_project_handler,
                ),
                ToolDefinition(
                    "summarize_changes",
                    "Summarize recent changes made during the session.",
                    {"type": "object", "properties": {}},
                    lambda args: {
                        "status": "ok",
                        "message": "Use git diff to review changes.",
                    },
                ),
                ToolDefinition(
                    "switch_modes",
                    "Switch operational modes (editing, planning, etc.).",
                    {
                        "type": "object",
                        "properties": {
                            "mode_names": {"type": "array", "items": {"type": "string"}}
                        },
                    },
                    lambda args: {
                        "status": "ok",
                        "active_modes": args.get("mode_names", []),
                    },
                ),
                ToolDefinition(
                    "open_dashboard",
                    "Open the CodeLens dashboard (not available in standalone mode).",
                    {"type": "object", "properties": {}},
                    lambda args: {
                        "status": "not_available",
                        "message": "Dashboard requires IDE plugin.",
                    },
                ),
                ToolDefinition(
                    "restart_language_server",
                    "Restart the language server for a specified language.",
                    {
                        "type": "object",
                        "properties": {"language": {"type": "string"}},
                        "required": ["language"],
                    },
                    self.restart_language_server,
                ),
                # JetBrains aliases — delegate to base tools in standalone mode
                ToolDefinition(
                    "jet_brains_find_symbol",
                    "Find symbol (JetBrains alias, uses LSP in standalone).",
                    {
                        "type": "object",
                        "properties": {
                            "name_path_pattern": {"type": "string"},
                            "relative_path": {"type": "string"},
                            "include_body": {"type": "boolean", "default": False},
                            "depth": {"type": "integer", "default": 0},
                        },
                    },
                    lambda args: self.find_symbol(
                        {
                            **args,
                            "name": args.get("name_path_pattern", ""),
                            "name_path": args.get("name_path_pattern", ""),
                            "file_path": args.get("relative_path"),
                            "include_body": args.get("include_body", False),
                        }
                    ),
                ),
                ToolDefinition(
                    "jet_brains_find_referencing_symbols",
                    "Find references (JetBrains alias).",
                    {
                        "type": "object",
                        "properties": {
                            "name_path": {"type": "string"},
                            "relative_path": {"type": "string"},
                        },
                        "required": ["name_path", "relative_path"],
                    },
                    lambda args: self.find_referencing_symbols(
                        {
                            **args,
                            "symbol_name": args.get("name_path", ""),
                            "file_path": args.get("relative_path"),
                        }
                    ),
                ),
                ToolDefinition(
                    "jet_brains_get_symbols_overview",
                    "Symbols overview (JetBrains alias).",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "depth": {"type": "integer", "default": 0},
                        },
                        "required": ["relative_path"],
                    },
                    lambda args: self.get_symbols_overview(
                        {**args, "path": args.get("relative_path", "")}
                    ),
                ),
                ToolDefinition(
                    "jet_brains_type_hierarchy",
                    "Type hierarchy (JetBrains alias).",
                    {
                        "type": "object",
                        "properties": {
                            "name_path": {"type": "string"},
                            "relative_path": {"type": "string"},
                            "hierarchy_type": {"type": "string", "default": "both"},
                            "depth": {"type": "integer", "default": 1},
                        },
                        "required": ["name_path", "relative_path"],
                    },
                    lambda args: self.get_type_hierarchy(
                        {**args, "fully_qualified_name": args.get("name_path", "")}
                    ),
                ),
                ToolDefinition(
                    "read_file",
                    "Read a file with an optional line range.",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "start_line": {"type": "integer"},
                            "end_line": {"type": "integer"},
                        },
                        "required": ["relative_path"],
                    },
                    self.read_file,
                ),
                ToolDefinition(
                    "list_dir",
                    "List directory contents.",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "recursive": {"type": "boolean", "default": False},
                        },
                        "required": ["relative_path"],
                    },
                    self.list_dir,
                ),
                ToolDefinition(
                    "find_file",
                    "Find files matching a wildcard pattern.",
                    {
                        "type": "object",
                        "properties": {
                            "wildcard_pattern": {"type": "string"},
                            "relative_dir": {"type": "string"},
                        },
                        "required": ["wildcard_pattern"],
                    },
                    self.find_file,
                ),
                ToolDefinition(
                    "create_text_file",
                    "Create or overwrite a text file within the workspace.",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "content": {"type": "string"},
                        },
                        "required": ["relative_path", "content"],
                    },
                    self.create_text_file,
                ),
                ToolDefinition(
                    "delete_lines",
                    "Delete a 1-based inclusive line range from a file.",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "start_line": {"type": "integer"},
                            "end_line": {"type": "integer"},
                        },
                        "required": ["relative_path", "start_line", "end_line"],
                    },
                    self.delete_lines,
                ),
                ToolDefinition(
                    "insert_at_line",
                    "Insert content at the given 1-based line.",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "line_number": {"type": "integer"},
                            "content": {"type": "string"},
                        },
                        "required": ["relative_path", "line_number", "content"],
                    },
                    self.insert_at_line,
                ),
                ToolDefinition(
                    "replace_lines",
                    "Replace a 1-based inclusive line range with new content.",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "start_line": {"type": "integer"},
                            "end_line": {"type": "integer"},
                            "content": {"type": "string"},
                        },
                        "required": [
                            "relative_path",
                            "start_line",
                            "end_line",
                            "content",
                        ],
                    },
                    self.replace_lines,
                ),
                ToolDefinition(
                    "replace_content",
                    "Replace file content using literal text or regex pattern.",
                    {
                        "type": "object",
                        "properties": {
                            "relative_path": {"type": "string"},
                            "needle": {
                                "type": "string",
                                "description": "String or regex to find",
                            },
                            "find": {
                                "type": "string",
                                "description": "Alias for needle",
                            },
                            "repl": {
                                "type": "string",
                                "description": "Replacement text",
                            },
                            "replace": {
                                "type": "string",
                                "description": "Alias for repl",
                            },
                            "mode": {
                                "type": "string",
                                "enum": ["literal", "regex"],
                                "default": "literal",
                            },
                            "allow_multiple_occurrences": {
                                "type": "boolean",
                                "default": False,
                            },
                            "first_only": {"type": "boolean", "default": False},
                        },
                        "required": ["relative_path"],
                    },
                    self.replace_content,
                ),
            ]
        }

    def run(self) -> None:
        try:
            while True:
                message = read_stdio_message()
                if message is None:
                    return
                response = self.handle_message(message)
                if response is not None:
                    write_stdio_message(response)
        finally:
            if self._multi_project:
                self._multi_project.shutdown_all()
            elif self._lsp_manager:
                self._lsp_manager.shutdown_all()

    def handle_message(self, message: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        method = message.get("method")
        message_id = message.get("id")
        params = message.get("params") or {}
        try:
            if method == "initialize":
                return {
                    "jsonrpc": "2.0",
                    "id": message_id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {"tools": {"listChanged": False}},
                        "serverInfo": {
                            "name": "codelens-workspace",
                            "version": "0.8.0",
                        },
                        "instructions": (
                            "CodeLens: PSI/LSP-powered code intelligence (45 tools). "
                            "Prefer symbolic tools over file reads: "
                            "get_symbols_overview for file structure, "
                            "find_symbol(include_body=true) for source code, "
                            "find_referencing_symbols for usage tracking, "
                            "search_for_pattern for regex search. "
                            "Use replace_symbol_body for atomic symbol edits. "
                            "Supports 50 language servers (Python, TypeScript, Go, Rust, Java, etc.) with LSP backend."
                        ),
                    },
                }
            if method == "notifications/initialized":
                return None
            if method == "tools/list":
                tool_list = []
                for t in self.tools.values():
                    entry: Dict[str, Any] = {
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema,
                    }
                    if t.title:
                        entry["title"] = t.title
                    if t.annotations:
                        entry["annotations"] = t.annotations
                    tool_list.append(entry)
                return {
                    "jsonrpc": "2.0",
                    "id": message_id,
                    "result": {"tools": tool_list},
                }
            if method == "tools/call":
                name = params.get("name")
                arguments = params.get("arguments") or {}
                tool = self.tools.get(name)
                if tool is None:
                    raise ToolError(f"Unknown tool: {name}")
                payload = success(tool.handler(arguments))
                return {
                    "jsonrpc": "2.0",
                    "id": message_id,
                    "result": tool_result(payload, is_error=False),
                }
            if method == "ping":
                return {"jsonrpc": "2.0", "id": message_id, "result": {}}
            if method == "exit":
                return None
            raise ToolError(f"Unsupported method: {method}")
        except Exception as exc:  # noqa: BLE001
            error_payload = error(str(exc))
            if method == "tools/call" and message_id is not None:
                return {
                    "jsonrpc": "2.0",
                    "id": message_id,
                    "result": tool_result(error_payload, is_error=True),
                }
            return {
                "jsonrpc": "2.0",
                "id": message_id,
                "error": {"code": -32000, "message": str(exc)},
            }

    def activate_project(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        requested = optional_string(arguments, "project")
        root_name = self.workspace_root.name
        root_str = str(self.workspace_root)
        if requested and requested not in {root_name, root_str}:
            raise ToolError(
                f"Requested project '{requested}' does not match the active workspace '{root_name}' at '{root_str}'"
            )
        return {
            "activated": True,
            "project_name": root_name,
            "project_base_path": root_str,
            "workspace_root_source": self.root_source,
            "requested_project": requested,
            "serena_project_dir": str(self.serena_dir),
            "serena_memories_dir": str(self.memories_dir),
            "memory_count": len(self._list_memory_names(None)),
            "backend_id": "workspace",
            "active_language_backend": "Workspace",
        }

    def get_current_config(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        include_tools = optional_bool(arguments, "include_tools", True)
        supported_profiles = self._supported_profiles()
        payload: Dict[str, Any] = {
            "project_name": self.workspace_root.name,
            "project_base_path": str(self.workspace_root),
            "compatible_context": "workspace",
            "transport": "stdio",
            "backend_id": "workspace",
            "active_language_backend": "Workspace",
            "workspace_root_source": self.root_source,
            "serena_project_dir": str(self.serena_dir),
            "serena_memories_dir": str(self.memories_dir),
            "serena_memories_present": self.memories_dir.is_dir(),
            "tool_count": len(self.tools),
            "recommended_profile": "codelens_workspace",
            "supported_profiles": supported_profiles,
        }
        if include_tools:
            payload["tools"] = list(self.tools)
        return payload

    def check_onboarding_performed(self, _: Dict[str, Any]) -> Dict[str, Any]:
        present = self._list_memory_names(None)
        missing = [name for name in REQUIRED_ONBOARDING_MEMORIES if name not in present]
        return {
            "onboarding_performed": not missing,
            "required_memories": REQUIRED_ONBOARDING_MEMORIES,
            "present_memories": present,
            "missing_memories": missing,
            "serena_project_dir": str(self.serena_dir),
            "serena_memories_dir": str(self.memories_dir),
            "serena_memories_present": self.memories_dir.is_dir(),
            "backend_id": "workspace",
            "active_language_backend": "Workspace",
        }

    def initial_instructions(self, _: Dict[str, Any]) -> Dict[str, Any]:
        return {
            "project_name": self.workspace_root.name,
            "project_base_path": str(self.workspace_root),
            "compatible_context": "workspace",
            "backend_id": "workspace",
            "active_language_backend": "Workspace",
            "workspace_root_source": self.root_source,
            "recommended_tools": [
                "activate_project",
                "get_current_config",
                "check_onboarding_performed",
                "list_memories",
                "read_memory",
                "write_memory",
                "get_symbols_overview",
                "find_symbol",
                "find_referencing_symbols",
                "search_for_pattern",
                "read_file",
                "list_dir",
                "find_file",
            ],
            "known_memories": self._list_memory_names(None),
            "instructions": [
                "The standalone workspace backend operates directly on local files and does not require IntelliJ IDEA.",
                "Use activate_project to validate the current workspace root before editing files or memories.",
                "Use get_current_config to inspect the active backend, transport, and registered tool list.",
                "Use check_onboarding_performed to verify the standard .serena onboarding memories.",
                "Use get_symbols_overview, find_symbol, and find_referencing_symbols for degraded symbol search when IDE PSI is unavailable.",
            ],
        }

    def get_symbols_overview(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        target = self._resolve_path(require_string(arguments, "path"))
        depth = optional_int(arguments, "depth", 1)
        # Try LSP first for single files
        if target.is_file():
            lsp_result = self._lsp_document_symbols(target)
            if lsp_result:
                symbols = [s.to_map(depth) for s in lsp_result]
                return {"symbols": symbols, "count": len(symbols), "backend": "lsp"}
        # Fallback to regex
        symbols = [
            symbol.to_map(depth)
            for symbol in self._collect_symbols(target, include_bodies=False)
        ]
        return (
            {"symbols": symbols, "count": len(symbols)}
            if symbols
            else {
                "symbols": [],
                "message": f"No symbols found in '{arguments['path']}'",
            }
        )

    def find_symbol(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        selector = optional_string(arguments, "name_path") or require_string(
            arguments, "name"
        )
        file_path = optional_string(arguments, "file_path")
        include_body = optional_bool(arguments, "include_body", False)
        exact_match = optional_bool(arguments, "exact_match", True)
        target = self._resolve_path(file_path) if file_path else self.workspace_root
        # Try LSP first for single files
        lsp_roots = None
        if target.is_file():
            lsp_roots = self._lsp_document_symbols(target, include_bodies=include_body)
        roots = (
            lsp_roots
            if lsp_roots
            else self._collect_symbols(target, include_bodies=include_body)
        )
        matcher = self._symbol_matcher(selector, exact_match)
        matches = [
            symbol.to_map(999)
            for root in roots
            for symbol in root.flatten()
            if matcher(symbol)
        ]
        return (
            {"symbols": matches, "count": len(matches)}
            if matches
            else {"symbols": [], "message": f"Symbol '{selector}' not found"}
        )

    def find_referencing_symbols(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        symbol_name = optional_string(arguments, "name_path") or require_string(
            arguments, "symbol_name"
        )
        file_path = optional_string(arguments, "file_path")
        max_results = optional_int(arguments, "max_results", 50)
        # Try LSP first
        lsp_refs = self._lsp_find_references(symbol_name, file_path, max_results)
        if lsp_refs is not None:
            return {"references": lsp_refs, "count": len(lsp_refs), "backend": "lsp"}
        resolved_definition = self._resolve_path(file_path) if file_path else None
        definition_symbols = (
            [
                s
                for root in self._collect_symbols(
                    resolved_definition, include_bodies=False
                )
                for s in root.flatten()
            ]
            if resolved_definition
            else []
        )
        target_symbol = (
            self._resolve_target_symbol(definition_symbols, symbol_name)
            if resolved_definition
            else None
        )
        if resolved_definition and target_symbol is None:
            return {
                "references": [],
                "message": f"No references found for '{symbol_name}'",
            }
        reference_name = (
            target_symbol.name
            if target_symbol
            else symbol_name.removeprefix("/").split("/")[-1]
        )
        pattern = re.compile(rf"\b{re.escape(reference_name)}\b")
        results: List[Dict[str, Any]] = []
        for file_path in self._candidate_files(self.workspace_root):
            if len(results) >= max_results:
                break
            lines = self._read_lines(file_path)
            if lines is None:
                continue
            symbols = [
                symbol
                for root in self._collect_symbols(file_path, include_bodies=False)
                for symbol in root.flatten()
            ]
            same_name_declarations = [
                symbol for symbol in symbols if symbol.name == reference_name
            ]
            same_file_reference_scope = None
            if (
                resolved_definition
                and file_path == resolved_definition
                and len(same_name_declarations) > 1
                and target_symbol is not None
            ):
                same_file_reference_scope = self._resolve_reference_scope(
                    definition_symbols, target_symbol
                )
            if (
                resolved_definition
                and file_path != resolved_definition
                and same_name_declarations
            ):
                continue
            for index, line in enumerate(lines):
                if len(results) >= max_results:
                    break
                line_number = index + 1
                if (
                    same_file_reference_scope
                    and line_number not in same_file_reference_scope
                ):
                    continue
                if self._declaration_name(line) == reference_name:
                    continue
                match = pattern.search(line)
                if not match:
                    continue
                if not self._is_code_occurrence(line, match.start()):
                    continue
                containing_symbol = next(
                    (
                        s.name
                        for s in sorted(
                            symbols, key=lambda item: item.end_line - item.start_line
                        )
                        if s.start_line <= line_number <= s.end_line
                    ),
                    file_path.name,
                )
                results.append(
                    {
                        "file": self._relative(file_path),
                        "line": line_number,
                        "column": match.start() + 1,
                        "containing_symbol": containing_symbol,
                        "context": line.strip(),
                        "is_write": bool(
                            re.search(
                                rf"\b{re.escape(reference_name)}\b\s*([+\-*/%]?=)", line
                            )
                        ),
                    }
                )
        return (
            {"references": results, "count": len(results)}
            if results
            else {
                "references": [],
                "message": f"No references found for '{symbol_name}'",
            }
        )

    def rename_symbol(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        symbol_name = optional_string(arguments, "name_path") or require_string(
            arguments, "symbol_name"
        )
        file_path = self._resolve_path(require_string(arguments, "file_path"))
        new_name = require_string(arguments, "new_name")
        scope = optional_string(arguments, "scope") or "project"
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", new_name):
            raise ToolError(f"Invalid target symbol name: {new_name}")

        declared_symbols = [
            s
            for root in self._collect_symbols(file_path, include_bodies=False)
            for s in root.flatten()
        ]
        target_symbol = self._resolve_target_symbol(declared_symbols, symbol_name)
        if target_symbol is None:
            raise ToolError(
                f"Symbol '{symbol_name}' not found in {self._relative(file_path)}"
            )

        pattern = re.compile(rf"\b{re.escape(target_symbol.name)}\b")
        if scope == "file":
            original_lines = file_path.read_text(encoding="utf-8").splitlines()
            target_lines = original_lines[
                target_symbol.start_line - 1 : target_symbol.end_line
            ]
            replacement_count = sum(len(pattern.findall(line)) for line in target_lines)
            if replacement_count == 0:
                raise ToolError(
                    f"Symbol '{symbol_name}' not found in {self._relative(file_path)}"
                )
            renamed_lines = [pattern.sub(new_name, line) for line in target_lines]
            updated_lines = (
                original_lines[: target_symbol.start_line - 1]
                + renamed_lines
                + original_lines[target_symbol.end_line :]
            )
            updated_content = "\n".join(updated_lines)
            file_path.write_text(updated_content, encoding="utf-8")
            return {
                "success": True,
                "message": f"Renamed '{symbol_name}' to '{new_name}' in 1 file(s)",
                "file": self._relative(file_path),
                "replacement_count": replacement_count,
                "new_content": updated_content,
            }

        candidate_files = list(self._candidate_files(self.workspace_root))
        references_by_file: Dict[Path, List[Dict[str, Any]]] = {}
        for reference in self.find_referencing_symbols(
            {
                "symbol_name": symbol_name,
                "file_path": self._relative(file_path),
                "max_results": 100000,
            }
        ).get("references", []):
            references_by_file.setdefault(
                self._resolve_path(reference["file"]), []
            ).append(reference)
        modified_files = 0
        replacement_count = 0

        for path in candidate_files:
            try:
                original_lines = path.read_text(encoding="utf-8").splitlines()
            except Exception:  # noqa: BLE001
                continue
            declared_in_file = [
                s
                for root in self._collect_symbols(path, include_bodies=False)
                for s in root.flatten()
            ]
            same_name_declarations = [
                symbol
                for symbol in declared_in_file
                if symbol.name == target_symbol.name
            ]
            updated_lines = original_lines[:]
            matches = 0

            if path == file_path:
                target_lines = original_lines[
                    target_symbol.start_line - 1 : target_symbol.end_line
                ]
                declaration_matches = sum(
                    len(pattern.findall(line)) for line in target_lines
                )
                if declaration_matches > 0:
                    renamed_lines = [
                        pattern.sub(new_name, line) for line in target_lines
                    ]
                    updated_lines[
                        target_symbol.start_line - 1 : target_symbol.end_line
                    ] = renamed_lines
                    matches += declaration_matches
            elif same_name_declarations:
                continue

            file_references = [
                reference
                for reference in references_by_file.get(path, [])
                if not (
                    path == file_path
                    and target_symbol.start_line
                    <= reference["line"]
                    <= target_symbol.end_line
                )
            ]
            for reference in sorted(
                file_references,
                key=lambda item: (item["line"], item["column"]),
                reverse=True,
            ):
                line_index = reference["line"] - 1
                updated_line = self._replace_occurrence_at_column(
                    updated_lines[line_index],
                    reference["column"],
                    target_symbol.name,
                    new_name,
                )
                if updated_line is None:
                    continue
                updated_lines[line_index] = updated_line
                matches += 1
            if matches == 0:
                continue

            path.write_text("\n".join(updated_lines), encoding="utf-8")
            modified_files += 1
            replacement_count += matches

        payload: Dict[str, Any] = {
            "success": True,
            "message": f"Renamed '{symbol_name}' to '{new_name}' in {modified_files} file(s)",
            "file": self._relative(file_path),
            "replacement_count": replacement_count,
        }
        if scope == "file":
            payload["new_content"] = file_path.read_text(encoding="utf-8")
        return payload

    def replace_symbol_body(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        symbol_name = optional_string(arguments, "name_path") or require_string(
            arguments, "symbol_name"
        )
        file_path = self._resolve_path(require_string(arguments, "file_path"))
        new_body = require_string(arguments, "new_body")

        symbols = [
            symbol
            for root in self._collect_symbols(file_path, include_bodies=False)
            for symbol in root.flatten()
        ]
        target = self._resolve_target_symbol(symbols, symbol_name)
        if target is None:
            raise ToolError(
                f"Symbol '{symbol_name}' not found in {self._relative(file_path)}"
            )

        original_lines = file_path.read_text(encoding="utf-8").splitlines()
        replacement_lines = new_body.splitlines()
        updated_lines = (
            original_lines[: target.start_line - 1]
            + replacement_lines
            + original_lines[target.end_line :]
        )
        updated_content = "\n".join(updated_lines)
        file_path.write_text(updated_content, encoding="utf-8")

        return {
            "success": True,
            "message": f"Replaced body of '{symbol_name}' in {self._relative(file_path)}",
            "file": self._relative(file_path),
            "affected_lines_start": target.start_line,
            "affected_lines_end": target.start_line + len(replacement_lines) - 1,
            "new_content": updated_content,
        }

    def insert_after_symbol(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        symbol_name = optional_string(arguments, "name_path") or require_string(
            arguments, "symbol_name"
        )
        file_path = self._resolve_path(require_string(arguments, "file_path"))
        content = require_string(arguments, "content")

        symbols = [
            symbol
            for root in self._collect_symbols(file_path, include_bodies=False)
            for symbol in root.flatten()
        ]
        target = self._resolve_target_symbol(symbols, symbol_name)
        if target is None:
            raise ToolError(
                f"Symbol '{symbol_name}' not found in {self._relative(file_path)}"
            )

        original_lines = file_path.read_text(encoding="utf-8").splitlines()
        inserted_lines = content.splitlines()
        updated_lines = (
            original_lines[: target.end_line]
            + inserted_lines
            + original_lines[target.end_line :]
        )
        updated_content = "\n".join(updated_lines)
        file_path.write_text(updated_content, encoding="utf-8")

        return {
            "success": True,
            "message": f"Inserted content after '{symbol_name}' in {self._relative(file_path)}",
            "file": self._relative(file_path),
            "affected_lines_start": target.end_line + 1,
            "affected_lines_end": target.end_line + len(inserted_lines),
            "new_content": updated_content,
        }

    def insert_before_symbol(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        symbol_name = optional_string(arguments, "name_path") or require_string(
            arguments, "symbol_name"
        )
        file_path = self._resolve_path(require_string(arguments, "file_path"))
        content = require_string(arguments, "content")

        symbols = [
            symbol
            for root in self._collect_symbols(file_path, include_bodies=False)
            for symbol in root.flatten()
        ]
        target = self._resolve_target_symbol(symbols, symbol_name)
        if target is None:
            raise ToolError(
                f"Symbol '{symbol_name}' not found in {self._relative(file_path)}"
            )

        original_lines = file_path.read_text(encoding="utf-8").splitlines()
        inserted_lines = content.splitlines()
        updated_lines = (
            original_lines[: target.start_line - 1]
            + inserted_lines
            + original_lines[target.start_line - 1 :]
        )
        updated_content = "\n".join(updated_lines)
        file_path.write_text(updated_content, encoding="utf-8")

        return {
            "success": True,
            "message": f"Inserted content before '{symbol_name}' in {self._relative(file_path)}",
            "file": self._relative(file_path),
            "affected_lines_start": target.start_line,
            "affected_lines_end": target.start_line + len(inserted_lines) - 1,
            "new_content": updated_content,
        }

    def search_for_pattern(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        pattern = (
            optional_string(arguments, "pattern")
            or optional_string(arguments, "substring_pattern")
            or ""
        )
        if not pattern:
            return {
                "results": [],
                "message": "Either 'pattern' or 'substring_pattern' is required",
            }
        file_glob = optional_string(arguments, "paths_include_glob") or optional_string(
            arguments, "file_glob"
        )
        max_results = optional_int(arguments, "max_results", 50)
        ctx_fallback = optional_int(arguments, "context_lines", 0)
        context_lines = max(
            optional_int(arguments, "context_lines_before", ctx_fallback),
            optional_int(arguments, "context_lines_after", ctx_fallback),
        )
        try:
            regex = re.compile(pattern)
        except re.error:
            return {
                "results": [],
                "message": f"No matches found for pattern: {pattern}",
            }
        extension_filter = (
            file_glob[2:] if file_glob and file_glob.startswith("*.") else None
        )
        results: List[Dict[str, Any]] = []
        for file_path in self._candidate_files(self.workspace_root):
            if extension_filter and file_path.suffix.lstrip(".") != extension_filter:
                continue
            lines = self._read_lines(file_path)
            if lines is None:
                continue
            for index, line in enumerate(lines):
                if len(results) >= max_results:
                    break
                match = regex.search(line)
                if not match:
                    continue
                payload: Dict[str, Any] = {
                    "file": self._relative(file_path),
                    "line": index + 1,
                    "column": match.start() + 1,
                    "matched_text": match.group(0),
                    "line_content": line.strip(),
                }
                if context_lines > 0:
                    payload["context_before"] = lines[
                        max(0, index - context_lines) : index
                    ]
                    payload["context_after"] = lines[
                        index + 1 : min(len(lines), index + 1 + context_lines)
                    ]
                results.append(payload)
        return (
            {"results": results, "count": len(results)}
            if results
            else {"results": [], "message": f"No matches found for pattern: {pattern}"}
        )

    def get_type_hierarchy(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        fqn = require_string(arguments, "fully_qualified_name")
        declarations = [
            declaration
            for path in self._candidate_files(self.workspace_root)
            if (declaration := self._parse_type_declaration(path))
        ]
        target = next(
            (
                declaration
                for declaration in declarations
                if declaration["qualified_name"] == fqn
            ),
            None,
        )
        if target is None:
            short_name = fqn.split(".")[-1]
            target = next(
                (
                    declaration
                    for declaration in declarations
                    if declaration["name"] == short_name
                ),
                None,
            )
        if target is None:
            return {
                "error": f"Class not found: {fqn}",
                "backend": "Workspace",
                "fully_qualified_name": fqn,
            }

        subtypes = [
            declaration
            for declaration in declarations
            if any(
                supertype in {target["name"], target["qualified_name"]}
                for supertype in declaration["supertypes"]
            )
        ]

        def resolve_decl(name: str) -> Optional[dict]:
            return next(
                (
                    declaration
                    for declaration in declarations
                    if declaration["name"] == name
                    or declaration["qualified_name"] == name
                ),
                None,
            )

        return {
            "class_name": target["name"],
            "fully_qualified_name": target["qualified_name"],
            "kind": target["kind"],
            "supertypes": [
                {
                    "name": supertype.split(".")[-1],
                    "qualified_name": (resolve_decl(supertype) or {}).get(
                        "qualified_name", ""
                    ),
                    "kind": (resolve_decl(supertype) or {}).get("kind", ""),
                }
                for supertype in target["supertypes"]
            ],
            "subtypes": [
                {
                    "name": declaration["name"],
                    "qualified_name": declaration["qualified_name"],
                }
                for declaration in subtypes
            ],
            "members": {
                "methods": [],
                "fields": [],
                "properties": target["properties"],
            },
            "type_parameters": [],
            "backend": "Workspace",
        }

    def execute_shell_command(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        command = require_string(arguments, "command")
        cwd = optional_string(arguments, "cwd")
        capture_stderr = optional_bool(arguments, "capture_stderr", True)
        work_dir = self._resolve_path(cwd) if cwd else self.workspace_root
        import subprocess as _sp

        import shlex as _shlex

        try:
            result = _sp.run(
                _shlex.split(command),
                shell=False,
                capture_output=True,
                text=True,
                cwd=str(work_dir),
                timeout=120,
                stdin=_sp.DEVNULL,
            )
            output: Dict[str, Any] = {
                "exitCode": result.returncode,
                "stdout": result.stdout,
            }
            if capture_stderr:
                output["stderr"] = result.stderr
            return output
        except _sp.TimeoutExpired:
            return {"exitCode": -1, "error": "Command timed out (120s)"}
        except Exception as e:
            raise ToolError(f"Shell command failed: {e}")

    def onboarding(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        self.memories_dir.mkdir(parents=True, exist_ok=True)
        created = []
        for mem in [
            "project_overview",
            "style_and_conventions",
            "suggested_commands",
            "task_completion",
        ]:
            path = self.memories_dir / f"{mem}.md"
            if not path.exists():
                path.write_text(
                    f"# {mem.replace('_', ' ').title()}\n\n(To be filled during onboarding)\n"
                )
                created.append(mem)
        return {"onboarding_performed": True, "created_memories": created}

    def prepare_for_new_conversation(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        return {
            "status": "ok",
            "message": "Session state cleared. Read memories to restore context.",
        }

    def remove_project_handler(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        project_path = require_string(arguments, "project_path")
        if self._multi_project:
            self._multi_project.remove_project(project_path)
            return {"removed": True, "path": project_path}
        return {"removed": False, "message": "Multi-project not available"}

    def restart_language_server(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        language = require_string(arguments, "language")
        if self._lsp_manager:
            client = self._lsp_manager._clients.get(language)
            if client:
                success = client.restart()
                return {"restarted": success, "language": language}
            self._lsp_manager._unavailable.discard(language)
            new_client = self._lsp_manager.ensure_started(language)
            return {"restarted": new_client is not None, "language": language}
        return {"restarted": False, "message": "LSP not available"}

    def list_queryable_projects(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        if self._multi_project:
            projects = self._multi_project.list_projects()
        else:
            projects = [
                {
                    "name": self.workspace_root.name,
                    "path": str(self.workspace_root),
                    "is_active": True,
                }
            ]
        return {"projects": projects, "count": len(projects)}

    def add_project(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        """Register an additional project for cross-project queries."""
        project_path = require_string(arguments, "project_path")
        resolved = Path(project_path).resolve()
        if not resolved.is_dir():
            raise ToolError(f"Directory not found: {project_path}")
        if self._multi_project:
            self._multi_project.add_project(str(resolved))
            return {"added": True, "name": resolved.name, "path": str(resolved)}
        raise ToolError("Multi-project support requires LSP backend")

    def query_project(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
        project_name = require_string(arguments, "project_name")
        tool_name = require_string(arguments, "tool_name")
        tool_params_json = require_string(arguments, "tool_params_json")

        READ_ONLY = {
            "find_symbol",
            "get_symbols_overview",
            "find_referencing_symbols",
            "search_for_pattern",
            "get_type_hierarchy",
            "read_file",
            "list_dir",
            "find_file",
        }
        if tool_name not in READ_ONLY:
            raise ToolError(
                f"Tool '{tool_name}' is not allowed for cross-project queries."
            )

        # Find target project
        if self._multi_project:
            target_manager = self._multi_project.get_manager_by_name(project_name)
            if target_manager:
                # Create a temporary server for the target project root
                target_root = None
                for root, mgr in self._multi_project._managers.items():
                    if mgr is target_manager:
                        target_root = root
                        break
                if target_root:
                    temp_server = WorkspaceMcpServer(
                        Path(target_root), root_source="query"
                    )
                    temp_server._lsp_manager = target_manager
                    tool = temp_server.tools.get(tool_name)
                    if tool:
                        import json as _json

                        return tool.handler(_json.loads(tool_params_json))

        # Fallback: current project only
        if project_name != self.workspace_root.name:
            available = (
                self._multi_project.list_projects() if self._multi_project else []
            )
            names = [p["name"] for p in available]
            raise ToolError(f"Project '{project_name}' not found. Available: {names}")

        tool = self.tools.get(tool_name)
        if not tool:
            raise ToolError(f"Tool '{tool_name}' not found.")
        import json as _json

        return tool.handler(_json.loads(tool_params_json))

    # ── LSP-backed symbol resolution (fallback to regex) ──

    def _lsp_document_symbols(
        self, file_path: Path, include_bodies: bool = False
    ) -> Optional[List[ParsedSymbol]]:
        """Try to get symbols via LSP. Returns None if LSP unavailable."""
        if not self._lsp_manager:
            return None
        client = self._lsp_manager.get_client(str(file_path))
        if not client:
            return None
        try:
            lsp_symbols = client.document_symbols(str(file_path.resolve()))
            if not lsp_symbols:
                return None
            # Read file lines once for body extraction
            lines = None
            if include_bodies:
                try:
                    lines = file_path.read_text(errors="replace").splitlines()
                except Exception:
                    pass
            return self._convert_lsp_symbols(lsp_symbols, file_path, "", lines)
        except Exception:
            return None

    def _convert_lsp_symbols(
        self,
        lsp_symbols: list,
        file_path: Path,
        parent_path: str = "",
        file_lines: Optional[List[str]] = None,
    ) -> List[ParsedSymbol]:
        """Convert LSP DocumentSymbol[] to ParsedSymbol[]."""
        from codelens_workspace.lsp_client import LSP_KIND_MAP

        result = []
        rel_path = self._relative(file_path)
        for sym in lsp_symbols:
            name = sym.get("name", "")
            kind_num = sym.get("kind", 0)
            kind = LSP_KIND_MAP.get(kind_num, "unknown")
            rng = sym.get("range", sym.get("location", {}).get("range", {}))
            start = rng.get("start", {})
            end = rng.get("end", {})
            start_line = start.get("line", 0)
            end_line = end.get("line", 0)
            name_path = f"{parent_path}/{name}" if parent_path else name

            # Extract body from file lines using LSP range
            body = None
            if file_lines and end_line >= start_line:
                body = "\n".join(file_lines[start_line : end_line + 1])

            children_lsp = sym.get("children", [])
            children = (
                self._convert_lsp_symbols(
                    children_lsp, file_path, name_path, file_lines
                )
                if children_lsp
                else []
            )

            parsed = ParsedSymbol(
                name=name,
                name_path=name_path,
                kind=kind,
                file_path=rel_path,
                line=start_line + 1,
                column=start.get("character", 0) + 1,
                signature=sym.get("detail", name),
                start_line=start_line + 1,
                end_line=end_line + 1,
                body=body,
                children=children,
            )
            result.append(parsed)
        return result

    def _lsp_find_references(
        self, symbol_name: str, file_path: Optional[str], max_results: int
    ) -> Optional[List[Dict[str, Any]]]:
        """Try LSP references. Returns None if unavailable."""
        if not self._lsp_manager or not file_path:
            return None
        resolved = self._resolve_path(file_path)
        client = self._lsp_manager.get_client(str(resolved))
        if not client:
            return None

        # Find symbol position first
        symbols = self._lsp_document_symbols(resolved)
        if not symbols:
            return None
        target = None
        bare = symbol_name.split("/")[-1] if "/" in symbol_name else symbol_name
        for s in (sym for root in (symbols or []) for sym in root.flatten()):
            if s.name == bare:
                target = s
                break
        if not target:
            return None

        try:
            from codelens_workspace.lsp_client import _uri_to_path

            refs = client.find_references(
                str(resolved.resolve()), target.line - 1, target.column - 1
            )
            results = []
            for ref in refs[:max_results]:
                loc_uri = ref.get("uri", "")
                loc_range = ref.get("range", {})
                loc_start = loc_range.get("start", {})
                ref_path = _uri_to_path(loc_uri)
                rel = (
                    str(Path(ref_path).relative_to(self.workspace_root))
                    if ref_path.startswith(str(self.workspace_root))
                    else ref_path
                )
                results.append(
                    {
                        "file": rel,
                        "line": loc_start.get("line", 0) + 1,
                        "column": loc_start.get("character", 0) + 1,
                        "symbol_name": bare,
                        "context": "",
                    }
                )
            return results
        except Exception:
            return None

    def _collect_symbols(
        self, target: Path, include_bodies: bool
    ) -> List[ParsedSymbol]:
        return [
            symbol
            for file_path in self._candidate_files(target)
            for symbol in self._parse_file(file_path, include_bodies)
        ]

    def _supported_profiles(self) -> List[Dict[str, Any]]:
        all_tools = set(self.tools)
        return [
            self._build_profile(
                "serena_baseline",
                "Serena-compatible baseline contract for symbolic retrieval, editing, onboarding, and memory workflows.",
                SERENA_BASELINE_TOOLS & all_tools,
            ),
            self._build_profile(
                "codelens_workspace",
                "Serena baseline plus standalone filesystem and workspace editing tools without JetBrains.",
                all_tools - JETBRAINS_ALIAS_TOOLS,
            ),
        ]

    def _build_profile(
        self, name: str, description: str, tools: set[str]
    ) -> Dict[str, Any]:
        ordered_tools = sorted(tools)
        return {
            "name": name,
            "description": description,
            "tool_count": len(ordered_tools),
            "tools": ordered_tools,
        }

    def _parse_type_declaration(self, file_path: Path) -> Optional[Dict[str, Any]]:
        lines = self._read_lines(file_path)
        if lines is None:
            return None
        package_name = next(
            (
                match.group(1)
                for line in lines
                if (match := PACKAGE_PATTERN.search(line))
            ),
            "",
        )
        declaration = next(
            (
                (line, match)
                for line in lines
                for pattern in CLASS_PATTERNS
                if (match := pattern.search(line))
            ),
            None,
        )
        if declaration is None:
            return None
        line, match = declaration
        name = match.group(2)
        kind = self._class_kind_for_declaration(line, match.group(1))
        return {
            "name": name,
            "qualified_name": f"{package_name}.{name}" if package_name else name,
            "kind": kind,
            "supertypes": self._extract_supertypes(line),
            "properties": self._extract_primary_properties(line),
        }

    def _class_kind_for_declaration(self, line: str, token: str) -> str:
        if "data class" in line:
            return "data_class"
        normalized = class_kind(token)
        return {
            "interface": "interface",
            "enum": "enum",
            "object": "object",
        }.get(normalized, "class")

    def _extract_supertypes(self, line: str) -> List[str]:
        supertypes: List[str] = []
        extends_match = EXTENDS_PATTERN.search(line)
        if extends_match:
            supertypes.append(self._normalize_type_name(extends_match.group(1)))
        implements_match = IMPLEMENTS_PATTERN.search(line)
        if implements_match:
            supertypes.extend(
                self._normalize_type_name(part)
                for part in implements_match.group(1).split(",")
                if part.strip()
            )
        if supertypes:
            return supertypes
        if ":" not in line:
            return []
        kotlin_clause = line.split(":", 1)[1].split("{", 1)[0].strip()
        return [
            self._normalize_type_name(part)
            for part in kotlin_clause.split(",")
            if part.strip()
        ]

    def _normalize_type_name(self, raw: str) -> str:
        trimmed = raw.strip().split(" where ", 1)[0].split("<", 1)[0]
        trimmed = trimmed.split("(", 1)[0]
        return trimmed.split(".")[-1]

    def _extract_primary_properties(self, line: str) -> List[str]:
        if "(" not in line or ")" not in line:
            return []
        parameter_block = line.split("(", 1)[1].rsplit(")", 1)[0]
        return [
            match.group(1)
            for part in parameter_block.split(",")
            if (match := PRIMARY_PROPERTY_PATTERN.search(part.strip()))
        ]

    def _resolve_target_symbol(
        self, symbols: List[ParsedSymbol], selector: str
    ) -> Optional[ParsedSymbol]:
        if self._is_name_path_selector(selector):
            normalized = selector.removeprefix("/")
            return next(
                (symbol for symbol in symbols if symbol.name_path == normalized), None
            )
        return next((symbol for symbol in symbols if symbol.name == selector), None)

    def _replace_occurrence_at_column(
        self, line: str, column: int, old_name: str, new_name: str
    ) -> Optional[str]:
        start_index = column - 1
        end_index = start_index + len(old_name)
        if start_index < 0 or end_index > len(line):
            return None
        if line[start_index:end_index] != old_name:
            return None
        return line[:start_index] + new_name + line[end_index:]

    def _is_code_occurrence(self, line: str, match_start: int) -> bool:
        trimmed = line.lstrip()
        if trimmed.startswith(("//", "#", "*", "/*")):
            return False
        prefix = line[:match_start]
        if "//" in prefix:
            return False
        return prefix.count('"') % 2 == 0 and prefix.count("'") % 2 == 0

    def _resolve_reference_scope(
        self, symbols: List[ParsedSymbol], target_symbol: ParsedSymbol
    ) -> range:
        owner_path = (
            target_symbol.name_path.rsplit("/", 1)[0]
            if "/" in target_symbol.name_path
            else ""
        )
        owner = (
            next((symbol for symbol in symbols if symbol.name_path == owner_path), None)
            if owner_path
            else None
        )
        scope_symbol = owner or target_symbol
        return range(scope_symbol.start_line, scope_symbol.end_line + 1)

    def _symbol_matcher(
        self, selector: str, exact_match: bool
    ) -> Callable[[ParsedSymbol], bool]:
        if self._is_name_path_selector(selector):
            return lambda symbol: self._matches_name_path_pattern(
                selector, symbol.name_path
            )
        if exact_match:
            return lambda symbol: symbol.name == selector
        lowered = selector.lower()
        return lambda symbol: lowered in symbol.name.lower()

    def _matches_name_path_pattern(self, pattern: str, name_path: str) -> bool:
        normalized = pattern.removeprefix("/")
        if pattern.startswith("/"):
            return name_path == normalized
        if "/" in normalized:
            return name_path == normalized or name_path.endswith(f"/{normalized}")
        return name_path.rsplit("/", 1)[-1] == normalized

    def _is_name_path_selector(self, selector: str) -> bool:
        return "/" in selector

    def _parse_file(self, file_path: Path, include_bodies: bool) -> List[ParsedSymbol]:
        lines = self._read_lines(file_path)
        if lines is None:
            return []
        roots: List[ParsedSymbol] = []
        stack: List[tuple[ParsedSymbol, int]] = []
        brace_depth = 0
        for index, line in enumerate(lines):
            declaration = self._parse_declaration(
                line, file_path, index, lines, include_bodies
            )
            if declaration is not None:
                symbol, opens_scope = declaration
                if stack:
                    symbol.name_path = f"{stack[-1][0].name_path}/{symbol.name}"
                if stack:
                    stack[-1][0].children.append(symbol)
                else:
                    roots.append(symbol)
                if opens_scope:
                    stack.append((symbol, brace_depth + 1))
            brace_depth += line.count("{") - line.count("}")
            while stack and stack[-1][1] > brace_depth:
                stack.pop()[0].end_line = index + 1
        for symbol, _ in stack:
            symbol.end_line = len(lines)
        return roots

    def _parse_declaration(
        self,
        line: str,
        file_path: Path,
        index: int,
        lines: List[str],
        include_bodies: bool,
    ) -> Optional[tuple[ParsedSymbol, bool]]:
        trimmed = line.lstrip()
        if any(trimmed.startswith(prefix) for prefix in STATEMENT_PREFIXES):
            return None

        for pattern in CLASS_PATTERNS:
            match = pattern.search(line)
            if match:
                token = match.group(1)
                name = match.group(2)
                return ParsedSymbol(
                    name,
                    name,
                    class_kind(token),
                    self._relative(file_path),
                    index + 1,
                    match.start() + 1,
                    line.strip(),
                    index + 1,
                    index + 1,
                    extract_body(lines, index) if include_bodies else None,
                    [],
                ), ("{" in line)
        for pattern in FUNCTION_PATTERNS:
            match = pattern.search(line)
            if match:
                name = match.group(1)
                if name not in RESERVED_WORDS:
                    return ParsedSymbol(
                        name,
                        name,
                        "function",
                        self._relative(file_path),
                        index + 1,
                        match.start() + 1,
                        line.strip(),
                        index + 1,
                        index + 1,
                        extract_body(lines, index) if include_bodies else None,
                        [],
                    ), ("{" in line)
        for pattern in PROPERTY_PATTERNS:
            match = pattern.search(line)
            if match:
                name = match.group(2)
                return (
                    ParsedSymbol(
                        name,
                        name,
                        "property",
                        self._relative(file_path),
                        index + 1,
                        match.start() + 1,
                        line.strip(),
                        index + 1,
                        index + 1,
                        line.strip() if include_bodies else None,
                        [],
                    ),
                    False,
                )
        return None

    def _declaration_name(self, line: str) -> Optional[str]:
        parsed = self._parse_declaration(
            line, self.workspace_root / "_", 0, [line], include_bodies=False
        )
        return parsed[0].name if parsed else None

    def _candidate_files(self, target: Path) -> Iterable[Path]:
        if not target.exists():
            raise ToolError(f"Path not found: {target}")
        if target.is_file():
            return (
                [target] if target.suffix.lstrip(".") in SEARCHABLE_EXTENSIONS else []
            )
        return [
            path
            for path in sorted(target.rglob("*"))
            if path.is_file() and path.suffix.lstrip(".") in SEARCHABLE_EXTENSIONS
        ]

    def _read_lines(self, file_path: Path) -> Optional[List[str]]:
        try:
            return file_path.read_text(encoding="utf-8").splitlines()
        except Exception:  # noqa: BLE001
            return None

    def _resolve_path(self, path_value: str) -> Path:
        path = Path(path_value)
        resolved = (
            path if path.is_absolute() else self.workspace_root / path
        ).resolve()
        if not str(resolved).startswith(str(self.workspace_root)):
            raise ToolError(f"Path escapes workspace root: {path_value}")
        return resolved

    def _relative(self, path: Path) -> str:
        return path.resolve().relative_to(self.workspace_root).as_posix()


def require_string(arguments: Dict[str, Any], key: str) -> str:
    value = arguments.get(key)
    if value is None or str(value).strip() == "":
        raise ToolError(f"Missing required parameter: {key}")
    return str(value)


def optional_string(arguments: Dict[str, Any], key: str) -> Optional[str]:
    value = arguments.get(key)
    return None if value is None else str(value)


def optional_int(arguments: Dict[str, Any], key: str, default: int) -> int:
    value = arguments.get(key)
    if value is None:
        return default
    return int(value)


def optional_bool(arguments: Dict[str, Any], key: str, default: bool) -> bool:
    value = arguments.get(key)
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        lowered = value.strip().lower()
        if lowered in {"true", "1", "yes"}:
            return True
        if lowered in {"false", "0", "no"}:
            return False
    return bool(value)


def class_kind(token: str) -> str:
    normalized = token.strip()
    if normalized == "interface":
        return "interface"
    if normalized in {"enum", "enum class"}:
        return "enum"
    if normalized == "object":
        return "object"
    if normalized == "annotation class":
        return "annotation"
    return "class"


def extract_body(lines: List[str], start_index: int) -> str:
    start_line = lines[start_index]
    if "{" not in start_line:
        return start_line.strip()
    depth = 0
    for line_index in range(start_index, len(lines)):
        line = lines[line_index]
        depth += line.count("{")
        depth -= line.count("}")
        if depth <= 0 and line_index > start_index:
            return "\n".join(lines[start_index : line_index + 1])
    return "\n".join(lines[start_index:])


def main() -> int:
    args = parse_args()
    workspace_root, root_source = resolve_workspace_root(args)
    server = WorkspaceMcpServer(workspace_root, root_source=root_source)
    try:
        server.run()
    except KeyboardInterrupt:
        return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
