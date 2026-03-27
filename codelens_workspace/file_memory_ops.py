from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Dict, List, Optional


def list_memories(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    topic = optional_string(arguments, "topic")
    memories = self._list_memory_names(topic)
    return {
        "topic": topic,
        "count": len(memories),
        "memories": [
            {"name": name, "path": f".serena/memories/{name}.md"} for name in memories
        ],
    }


def read_memory(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    memory_name = normalize_memory_name(
        self, require_string(self, arguments, "memory_name")
    )
    path = self._memory_path(memory_name)
    if not path.is_file():
        raise self._tool_error(f"Memory not found: {memory_name}")
    content = path.read_text(encoding="utf-8")
    return {
        "memory_name": memory_name,
        "path": self._relative(path),
        "content": content,
        "line_count": len(content.splitlines()),
        "character_count": len(content),
    }


def write_memory(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    memory_name = normalize_memory_name(
        self, require_string(self, arguments, "memory_name")
    )
    content = require_string(self, arguments, "content")
    max_chars = optional_int(arguments, "max_chars", len(content))
    if max_chars <= 0:
        raise self._tool_error("max_chars must be greater than 0")
    path = self._memory_path(memory_name, create_parents=True)
    existed = path.exists()
    content_to_write = content[:max_chars]
    path.write_text(content_to_write, encoding="utf-8")
    return {
        "memory_name": memory_name,
        "path": self._relative(path),
        "written_characters": len(content_to_write),
        "truncated": len(content_to_write) != len(content),
        "created": not existed,
    }


def edit_memory(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    memory_name = normalize_memory_name(
        self, require_string(self, arguments, "memory_name")
    )
    path = self._memory_path(memory_name)
    if not path.is_file():
        raise self._tool_error(
            f"Memory not found: {memory_name}. Use write_memory to create new memories."
        )
    content = require_string(self, arguments, "content")
    max_chars = optional_int(arguments, "max_chars", len(content))
    old_content = path.read_text(encoding="utf-8")
    content_to_write = content[:max_chars]
    path.write_text(content_to_write, encoding="utf-8")
    return {
        "memory_name": memory_name,
        "path": self._relative(path),
        "old_characters": len(old_content),
        "new_characters": len(content_to_write),
        "truncated": len(content_to_write) != len(content),
    }


def rename_memory(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    old_name = normalize_memory_name(self, require_string(self, arguments, "old_name"))
    new_name = normalize_memory_name(self, require_string(self, arguments, "new_name"))
    old_path = self._memory_path(old_name)
    if not old_path.is_file():
        raise self._tool_error(f"Memory not found: {old_name}")
    new_path = self._memory_path(new_name, create_parents=True)
    if new_path.exists():
        raise self._tool_error(f"Target memory already exists: {new_name}")
    old_path.rename(new_path)
    return {
        "old_name": old_name,
        "new_name": new_name,
        "old_path": self._relative(old_path),
        "new_path": self._relative(new_path),
    }


def delete_memory(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    memory_name = normalize_memory_name(
        self, require_string(self, arguments, "memory_name")
    )
    path = self._memory_path(memory_name)
    if not path.is_file():
        raise self._tool_error(f"Memory not found: {memory_name}")
    path.unlink()
    return {"memory_name": memory_name, "path": self._relative(path), "deleted": True}


def read_file(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    path = self._resolve_path(require_string(self, arguments, "relative_path"))
    if not path.is_file():
        raise self._tool_error(f"File not found: {arguments['relative_path']}")
    lines = path.read_text(encoding="utf-8").splitlines()
    start = max(0, optional_int(arguments, "start_line", 0))
    end = min(len(lines), optional_int(arguments, "end_line", len(lines)))
    return {
        "content": "\n".join(lines[start:end]),
        "total_lines": len(lines),
        "file_path": self._relative(path),
    }


def list_dir(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    path = self._resolve_path(require_string(self, arguments, "relative_path"))
    recursive = optional_bool(arguments, "recursive", False)
    if not path.is_dir():
        raise self._tool_error(f"Directory not found: {arguments['relative_path']}")
    children = sorted(path.rglob("*") if recursive else path.iterdir())
    entries = [
        {
            "name": child.name,
            "type": "directory" if child.is_dir() else "file",
            "path": self._relative(child),
            "size": None if child.is_dir() else child.stat().st_size,
        }
        for child in children
    ]
    return {"entries": entries, "count": len(entries)}


def find_file(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    pattern = require_string(self, arguments, "wildcard_pattern")
    base_dir = self._resolve_path(optional_string(arguments, "relative_dir") or ".")
    if not base_dir.is_dir():
        raise self._tool_error(
            f"Directory not found: {optional_string(arguments, 'relative_dir') or '.'}"
        )
    regex = re.compile(wildcard_to_regex(pattern))
    files = [
        self._relative(path)
        for path in sorted(base_dir.rglob("*"))
        if path.is_file() and regex.search(path.name)
    ]
    return {"files": files, "count": len(files)}


def create_text_file(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    path = self._resolve_path(require_string(self, arguments, "relative_path"))
    content = require_string(self, arguments, "content")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return {"file_path": self._relative(path), "lines": len(content.splitlines())}


def delete_lines(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    path = self._resolve_path(require_string(self, arguments, "relative_path"))
    start_line = optional_int(arguments, "start_line", 1)
    end_line = optional_int(arguments, "end_line", 1)
    lines = path.read_text(encoding="utf-8").splitlines()
    self._validate_range(start_line, end_line, len(lines))
    new_lines = lines[: start_line - 1] + lines[end_line:]
    path.write_text(
        "\n".join(new_lines) + ("\n" if new_lines else ""), encoding="utf-8"
    )
    return {
        "deleted_lines": end_line - start_line + 1,
        "file_path": self._relative(path),
    }


def insert_at_line(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    path = self._resolve_path(require_string(self, arguments, "relative_path"))
    line_number = optional_int(arguments, "line_number", 1)
    content = require_string(self, arguments, "content")
    lines = path.read_text(encoding="utf-8").splitlines()
    if line_number < 1 or line_number > len(lines) + 1:
        raise self._tool_error(
            f"Invalid line number: {line_number} (file has {len(lines)} lines)"
        )
    new_lines = (
        lines[: line_number - 1] + content.splitlines() + lines[line_number - 1 :]
    )
    path.write_text("\n".join(new_lines) + "\n", encoding="utf-8")
    return {"inserted_at_line": line_number, "file_path": self._relative(path)}


def replace_lines(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    path = self._resolve_path(require_string(self, arguments, "relative_path"))
    start_line = optional_int(arguments, "start_line", 1)
    end_line = optional_int(arguments, "end_line", 1)
    content = require_string(self, arguments, "content")
    lines = path.read_text(encoding="utf-8").splitlines()
    self._validate_range(start_line, end_line, len(lines))
    replacement = content.splitlines()
    new_lines = lines[: start_line - 1] + replacement + lines[end_line:]
    path.write_text("\n".join(new_lines) + "\n", encoding="utf-8")
    return {
        "replaced_lines": end_line - start_line + 1,
        "file_path": self._relative(path),
    }


def replace_content(self, arguments: Dict[str, Any]) -> Dict[str, Any]:
    path = self._resolve_path(require_string(self, arguments, "relative_path"))
    find = optional_string(arguments, "needle") or optional_string(arguments, "find")
    replace_str = optional_string(arguments, "repl") or optional_string(
        arguments, "replace"
    )
    if not find:
        raise self._tool_error("Either 'find' or 'needle' is required")
    if replace_str is None:
        raise self._tool_error("Either 'replace' or 'repl' is required")
    mode = optional_string(arguments, "mode") or "literal"
    allow_multiple = optional_bool(arguments, "allow_multiple_occurrences", False)
    first_only = optional_bool(arguments, "first_only", not allow_multiple)
    content = path.read_text(encoding="utf-8")

    import re as _re

    if mode == "regex":
        pattern = _re.compile(find)
        if first_only:
            updated = pattern.sub(replace_str, content, count=1)
            replacements = 1 if updated != content else 0
        else:
            replacements = len(pattern.findall(content))
            updated = pattern.sub(replace_str, content)
    else:
        replacements = content.count(find)
        if first_only:
            replacements = 1 if find in content else 0
            updated = content.replace(find, replace_str, 1)
        else:
            updated = content.replace(find, replace_str)

    path.write_text(updated, encoding="utf-8")
    return {"file_path": self._relative(path), "replacements": replacements}


def _memory_path(self, memory_name: str, create_parents: bool = False) -> Path:
    path = (self.memories_dir / f"{memory_name}.md").resolve()
    if not str(path).startswith(str(self.memories_dir.resolve())):
        raise self._tool_error(f"Memory path escapes .serena/memories: {memory_name}")
    if create_parents:
        path.parent.mkdir(parents=True, exist_ok=True)
    return path


def _list_memory_names(self, topic: Optional[str]) -> List[str]:
    if not self.memories_dir.is_dir():
        return []
    normalized_topic = normalize_memory_name(self, topic) if topic else None
    names = [
        path.relative_to(self.memories_dir).as_posix()[:-3]
        for path in self.memories_dir.rglob("*.md")
        if path.is_file()
    ]
    if normalized_topic:
        names = [
            name
            for name in names
            if name == normalized_topic or name.startswith(f"{normalized_topic}/")
        ]
    return sorted(names)


def _validate_range(self, start_line: int, end_line: int, line_count: int) -> None:
    if start_line < 1 or end_line < start_line or end_line > line_count:
        raise self._tool_error(
            f"Invalid line range: {start_line}-{end_line} (file has {line_count} lines)"
        )


def require_string(self, arguments: Dict[str, Any], key: str) -> str:
    value = arguments.get(key)
    if value is None or str(value).strip() == "":
        raise self._tool_error(f"Missing required parameter: {key}")
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


def normalize_memory_name(self, memory_name: Optional[str]) -> str:
    if memory_name is None:
        raise self._tool_error("Memory name must not be empty")
    trimmed = memory_name.strip().replace("\\", "/")
    if not trimmed:
        raise self._tool_error("Memory name must not be empty")
    if trimmed.startswith("/"):
        raise self._tool_error("Memory name must be relative")
    without_extension = trimmed[:-3] if trimmed.endswith(".md") else trimmed
    without_slashes = without_extension.strip("/")
    segments = without_slashes.split("/")
    if not without_slashes or any(segment in {"", ".", ".."} for segment in segments):
        raise self._tool_error("Memory name must not contain path traversal segments")
    return without_slashes


def wildcard_to_regex(pattern: str) -> str:
    escaped = []
    for char in pattern:
        if char == ".":
            escaped.append(r"\.")
        elif char == "*":
            escaped.append(".*")
        elif char == "?":
            escaped.append(".")
        else:
            escaped.append(re.escape(char))
    return "".join(escaped)
