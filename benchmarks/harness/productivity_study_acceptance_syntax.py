"""Small lexical helpers for evaluator-owned TypeScript acceptance checks."""

from __future__ import annotations

import re
from typing import Final


_BRACED_HREF: Final = re.compile(r"\bhref\s*=\s*\{(?P<expression>[^{}]*)\}", re.DOTALL)


def extract_jsx_anchor_opening_tags(source: str) -> tuple[str, ...]:
    """Return live JSX anchor opening tags outside JavaScript comments."""
    tags: list[str] = []
    cursor = 0
    while cursor < len(source):
        if source.startswith("//", cursor):
            line_end = source.find("\n", cursor + 2)
            cursor = len(source) if line_end < 0 else line_end + 1
            continue
        if source.startswith("/*", cursor):
            comment_end = source.find("*/", cursor + 2)
            cursor = len(source) if comment_end < 0 else comment_end + 2
            continue
        if source[cursor] in "'\"`":
            cursor = _after_quoted(source, cursor, source[cursor])
            continue
        if source.startswith("<a", cursor) and _is_tag_name_boundary(
            source, cursor + 2
        ):
            tag_end = _jsx_opening_tag_end(source, cursor + 2)
            if tag_end is None:
                break
            tags.append(source[cursor : tag_end + 1])
            cursor = tag_end + 1
            continue
        cursor += 1
    return tuple(tags)


def anchor_href_matches(tag: str, expected_expression: str) -> bool:
    """Return whether a tag has the expected braced href modulo whitespace."""
    return any(
        "".join(match.group("expression").split()) == expected_expression
        for match in _BRACED_HREF.finditer(tag)
    )


def _is_tag_name_boundary(source: str, cursor: int) -> bool:
    return cursor >= len(source) or source[cursor].isspace() or source[cursor] in "/>"


def _jsx_opening_tag_end(source: str, cursor: int) -> int | None:
    brace_depth = 0
    while cursor < len(source):
        if source.startswith("/*", cursor):
            comment_end = source.find("*/", cursor + 2)
            cursor = len(source) if comment_end < 0 else comment_end + 2
            continue
        if source.startswith("//", cursor):
            line_end = source.find("\n", cursor + 2)
            cursor = len(source) if line_end < 0 else line_end + 1
            continue
        if source[cursor] in "'\"`":
            cursor = _after_quoted(source, cursor, source[cursor])
            continue
        if source[cursor] == "{":
            brace_depth += 1
        elif source[cursor] == "}" and brace_depth > 0:
            brace_depth -= 1
        elif source[cursor] == ">" and brace_depth == 0:
            return cursor
        cursor += 1
    return None


def _after_quoted(source: str, cursor: int, quote: str) -> int:
    cursor += 1
    while cursor < len(source):
        if source[cursor] == "\\":
            cursor += 2
            continue
        if source[cursor] == quote:
            return cursor + 1
        cursor += 1
    return len(source)
