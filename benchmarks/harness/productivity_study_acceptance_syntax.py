"""Small lexical helpers for evaluator-owned TypeScript acceptance checks."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class JsxAttributeValueKind(StrEnum):
    """Lexical form used by a JSX attribute value."""

    BOOLEAN = "boolean"
    QUOTED = "quoted"
    BRACED = "braced"


@dataclass(frozen=True, slots=True)
class JsxAttribute:
    """One exactly named JSX attribute and its lexical value."""

    name: str
    kind: JsxAttributeValueKind
    value: str | None


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


def parse_jsx_anchor_attributes(tag: str) -> tuple[JsxAttribute, ...] | None:
    """Parse explicit attributes from one extracted JSX anchor opening tag."""
    if not tag.startswith("<a") or not _is_tag_name_boundary(tag, 2):
        return None
    attributes: list[JsxAttribute] = []
    cursor = 2
    while cursor < len(tag):
        while cursor < len(tag) and tag[cursor].isspace():
            cursor += 1
        if tag.startswith("/>", cursor):
            return tuple(attributes) if cursor + 2 == len(tag) else None
        if cursor < len(tag) and tag[cursor] == ">":
            return tuple(attributes) if cursor + 1 == len(tag) else None
        if cursor < len(tag) and tag[cursor] == "{":
            spread_end = _braced_end(tag, cursor)
            if spread_end is None:
                return None
            cursor = spread_end + 1
            continue
        name_start = cursor
        while cursor < len(tag) and _is_attribute_name_character(tag[cursor]):
            cursor += 1
        if cursor == name_start:
            return None
        name = tag[name_start:cursor]
        while cursor < len(tag) and tag[cursor].isspace():
            cursor += 1
        if cursor >= len(tag) or tag[cursor] != "=":
            attributes.append(JsxAttribute(name, JsxAttributeValueKind.BOOLEAN, None))
            continue
        cursor += 1
        while cursor < len(tag) and tag[cursor].isspace():
            cursor += 1
        if cursor >= len(tag):
            return None
        if tag[cursor] in "'\"":
            value_start = cursor + 1
            value_end = _after_quoted(tag, cursor, tag[cursor])
            if value_end > len(tag) or tag[value_end - 1] != tag[cursor]:
                return None
            attributes.append(
                JsxAttribute(
                    name,
                    JsxAttributeValueKind.QUOTED,
                    tag[value_start : value_end - 1],
                )
            )
            cursor = value_end
            continue
        if tag[cursor] == "{":
            value_start = cursor + 1
            value_end = _braced_end(tag, cursor)
            if value_end is None:
                return None
            attributes.append(
                JsxAttribute(
                    name,
                    JsxAttributeValueKind.BRACED,
                    tag[value_start:value_end],
                )
            )
            cursor = value_end + 1
            continue
        return None
    return None


def anchor_has_required_attributes(tag: str, expected_href: str) -> bool:
    """Match an anchor contract using exact, unique parsed attribute names."""
    attributes = parse_jsx_anchor_attributes(tag)
    if attributes is None:
        return False
    href = _unique_attribute(attributes, "href")
    download = _unique_attribute(attributes, "download")
    target = _unique_attribute(attributes, "target")
    rel = _unique_attribute(attributes, "rel")
    if href is None or download is None or target is None or rel is None:
        return False
    if href.kind is not JsxAttributeValueKind.BRACED:
        return False
    if "".join((href.value or "").split()) != expected_href:
        return False
    if target.kind is not JsxAttributeValueKind.QUOTED or target.value != "_blank":
        return False
    return (
        rel.kind is JsxAttributeValueKind.QUOTED
        and rel.value is not None
        and {"noopener", "noreferrer"}.issubset(rel.value.split())
    )


def _is_tag_name_boundary(source: str, cursor: int) -> bool:
    return cursor >= len(source) or source[cursor].isspace() or source[cursor] in "/>"


def _is_attribute_name_character(character: str) -> bool:
    return character.isalnum() or character in "_:$-."


def _unique_attribute(
    attributes: tuple[JsxAttribute, ...], name: str
) -> JsxAttribute | None:
    matching = tuple(attribute for attribute in attributes if attribute.name == name)
    return matching[0] if len(matching) == 1 else None


def _braced_end(source: str, cursor: int) -> int | None:
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
        elif source[cursor] == "}":
            brace_depth -= 1
            if brace_depth == 0:
                return cursor
        cursor += 1
    return None


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
