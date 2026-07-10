"""Module-specifier extraction for evaluator-owned TypeScript acceptance checks."""

from __future__ import annotations

import re

from productivity_study_acceptance_code import mask_comments_and_strings


def module_specifier(statement: str) -> str | None:
    """Return the first live module specifier in a declaration-sized source slice."""
    specifiers = module_specifiers(statement)
    return specifiers[0] if specifiers else None


def module_specifiers(source: str) -> tuple[str, ...]:
    """Extract every live `from` and dynamic-import specifier."""
    masked = mask_comments_and_strings(source)
    specifiers: list[str] = []
    for match in re.finditer(r"\b(from|import)\b", masked):
        cursor = _literal_start(source, masked, match)
        if cursor is None:
            continue
        specifier = _quoted_value(source, cursor)
        if specifier is not None:
            specifiers.append(specifier)
    return tuple(specifiers)


def _literal_start(source: str, masked: str, match: re.Match[str]) -> int | None:
    cursor = _skip_ignored(source, masked, match.end())
    if match.group(1) == "import":
        if cursor < len(source) and source[cursor] in "'\"":
            return cursor
        if cursor >= len(source) or source[cursor] != "(":
            return None
        while cursor < len(source) and source[cursor] == "(":
            cursor = _skip_ignored(source, masked, cursor + 1)
    return cursor if cursor < len(source) and source[cursor] in "'\"" else None


def _skip_ignored(source: str, masked: str, cursor: int) -> int:
    while cursor < len(source):
        if source[cursor].isspace():
            cursor += 1
            continue
        if masked[cursor] == " " and source[cursor] not in "'\"":
            cursor += 1
            continue
        return cursor
    return cursor


def _quoted_value(source: str, cursor: int) -> str | None:
    quote = source[cursor]
    value: list[str] = []
    cursor += 1
    while cursor < len(source):
        if source[cursor] == "\\":
            escaped = _decode_string_escape(source, cursor)
            if escaped is None:
                return None
            character, cursor = escaped
            value.append(character)
            continue
        if source[cursor] == quote:
            return "".join(value)
        value.append(source[cursor])
        cursor += 1
    return None


def _decode_string_escape(source: str, cursor: int) -> tuple[str, int] | None:
    marker = cursor + 1
    if marker >= len(source):
        return None
    if source[marker] == "u":
        return _decode_unicode_escape(source, marker + 1)
    if source[marker] == "x":
        return _decode_hex_escape(source, marker + 1, 2)
    if source[marker] in "\n\r":
        if source[marker] == "\r" and marker + 1 < len(source) and source[marker + 1] == "\n":
            return "", marker + 2
        return "", marker + 1
    escaped = {
        "b": "\b",
        "f": "\f",
        "n": "\n",
        "r": "\r",
        "t": "\t",
        "v": "\v",
        "0": "\0",
    }.get(source[marker], source[marker])
    return escaped, marker + 1


def _decode_unicode_escape(source: str, cursor: int) -> tuple[str, int] | None:
    if cursor < len(source) and source[cursor] == "{":
        end = source.find("}", cursor + 1)
        if end < 0:
            return None
        return _decode_hex_escape(source, cursor + 1, end - cursor - 1, end + 1)
    return _decode_hex_escape(source, cursor, 4)


def _decode_hex_escape(
    source: str,
    cursor: int,
    length: int,
    next_cursor: int | None = None,
) -> tuple[str, int] | None:
    digits = source[cursor : cursor + length]
    if len(digits) != length or re.fullmatch(r"[0-9a-fA-F]+", digits) is None:
        return None
    value = int(digits, 16)
    if value > 0x10FFFF:
        return None
    return chr(value), next_cursor if next_cursor is not None else cursor + length
