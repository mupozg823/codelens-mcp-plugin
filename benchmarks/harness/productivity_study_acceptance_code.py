"""Lexical helpers shared by evaluator-owned source acceptance checks."""

from __future__ import annotations

import re


def mask_comments_and_strings(source: str) -> str:
    """Blank comments and quoted literals while preserving offsets and newlines."""
    masked = list(source)
    cursor = 0
    while cursor < len(source):
        if source.startswith("//", cursor):
            cursor = _mask_until(source, masked, cursor, "\n")
            continue
        if source.startswith("/*", cursor):
            cursor = _mask_until(source, masked, cursor, "*/")
            continue
        if source[cursor] == "/" and _starts_regex_literal(masked, cursor):
            cursor = _mask_regex(source, masked, cursor)
            continue
        if source[cursor] == "`":
            cursor = _mask_template(source, masked, cursor)
            continue
        if source[cursor] in "'\"":
            cursor = _mask_quoted(source, masked, cursor, source[cursor])
            continue
        cursor += 1
    return "".join(masked)


def actual_statements(source: str, keyword: str) -> tuple[str, ...]:
    """Return semicolon-delimited statements beginning with a live keyword."""
    masked = mask_comments_and_strings(source)
    pattern = re.compile(rf"\b{re.escape(keyword)}\b")
    statements: list[str] = []
    for match in pattern.finditer(masked):
        start = match.start()
        end = masked.find(";", match.start())
        if end < 0:
            end = len(source) - 1
        statements.append(source[start : end + 1])
    return tuple(statements)


def contains_live_keyword(source: str, keyword: str) -> bool:
    """Return whether a word occurs outside comments and quoted literal text."""
    return re.search(rf"\b{re.escape(keyword)}\b", mask_comments_and_strings(source)) is not None


def contains_dynamic_import(source: str) -> bool:
    """Return whether source has a live dynamic-import expression."""
    masked = mask_comments_and_strings(source)
    for match in re.finditer(r"\bimport\b", masked):
        cursor = match.end()
        while cursor < len(masked) and masked[cursor].isspace():
            cursor += 1
        if cursor < len(masked) and masked[cursor] == "(":
            return True
    return False


def _mask_until(
    source: str,
    masked: list[str],
    cursor: int,
    terminator: str,
) -> int:
    end = source.find(terminator, cursor + 2)
    limit = len(source) if end < 0 else end + len(terminator)
    for index in range(cursor, limit):
        if source[index] != "\n":
            masked[index] = " "
    return limit


def _mask_quoted(
    source: str,
    masked: list[str],
    cursor: int,
    quote: str,
) -> int:
    masked[cursor] = " "
    cursor += 1
    while cursor < len(source):
        if source[cursor] == "\\":
            masked[cursor] = " "
            cursor += 1
            if cursor < len(source) and source[cursor] != "\n":
                masked[cursor] = " "
        elif source[cursor] == quote:
            masked[cursor] = " "
            return cursor + 1
        elif source[cursor] != "\n":
            masked[cursor] = " "
        cursor += 1
    return cursor


def _mask_template(source: str, masked: list[str], cursor: int) -> int:
    masked[cursor] = " "
    cursor += 1
    while cursor < len(source):
        if source[cursor] == "\\":
            masked[cursor] = " "
            cursor += 1
            if cursor < len(source) and source[cursor] != "\n":
                masked[cursor] = " "
        elif source[cursor] == "`":
            masked[cursor] = " "
            return cursor + 1
        elif source.startswith("${", cursor):
            masked[cursor] = " "
            masked[cursor + 1] = " "
            cursor = _mask_template_expression(source, masked, cursor + 2)
            continue
        elif source[cursor] != "\n":
            masked[cursor] = " "
        cursor += 1
    return cursor


def _mask_template_expression(source: str, masked: list[str], cursor: int) -> int:
    depth = 1
    while cursor < len(source):
        if source.startswith("//", cursor):
            cursor = _mask_until(source, masked, cursor, "\n")
            continue
        if source.startswith("/*", cursor):
            cursor = _mask_until(source, masked, cursor, "*/")
            continue
        if source[cursor] == "/" and _starts_regex_literal(masked, cursor):
            cursor = _mask_regex(source, masked, cursor)
            continue
        if source[cursor] == "`":
            cursor = _mask_template(source, masked, cursor)
            continue
        if source[cursor] in "'\"":
            cursor = _mask_quoted(source, masked, cursor, source[cursor])
            continue
        if source[cursor] == "{":
            depth += 1
        elif source[cursor] == "}":
            depth -= 1
            if depth == 0:
                masked[cursor] = " "
                return cursor + 1
        cursor += 1
    return cursor


def _starts_regex_literal(masked: list[str], cursor: int) -> bool:
    previous = _previous_code_character(masked, cursor)
    if previous is None or previous in "=([{,:;!?&|^~<>+-*%}":
        return True
    if _is_control_condition_end(masked, cursor):
        return True
    prefix = "".join(masked[:cursor]).rstrip()
    return re.search(
        r"\b(?:return|throw|case|delete|void|typeof|instanceof|in|of|yield|await|else|do|finally)$",
        prefix,
    ) is not None


def _previous_code_character(masked: list[str], cursor: int) -> str | None:
    cursor -= 1
    while cursor >= 0:
        if not masked[cursor].isspace():
            return masked[cursor]
        cursor -= 1
    return None


def _is_control_condition_end(masked: list[str], cursor: int) -> bool:
    close = cursor - 1
    while close >= 0 and masked[close].isspace():
        close -= 1
    if close < 0 or masked[close] != ")":
        return False
    depth = 0
    for index in range(close, -1, -1):
        if masked[index] == ")":
            depth += 1
        elif masked[index] == "(":
            depth -= 1
            if depth == 0:
                prefix = "".join(masked[:index]).rstrip()
                return re.search(
                    r"\b(?:if|while|for(?:\s+await)?|with|switch|catch)$",
                    prefix,
                ) is not None
    return False


def _mask_regex(source: str, masked: list[str], cursor: int) -> int:
    start = cursor
    in_character_class = False
    positions: list[int] = [cursor]
    cursor += 1
    while cursor < len(source):
        if source[cursor] == "\n":
            return start + 1
        positions.append(cursor)
        if source[cursor] == "\\":
            cursor += 1
            if cursor < len(source):
                positions.append(cursor)
                cursor += 1
            continue
        elif source[cursor] == "[":
            in_character_class = True
        elif source[cursor] == "]":
            in_character_class = False
        elif source[cursor] == "/" and not in_character_class:
            for index in positions:
                masked[index] = " "
            return cursor + 1
        cursor += 1
    return start + 1
