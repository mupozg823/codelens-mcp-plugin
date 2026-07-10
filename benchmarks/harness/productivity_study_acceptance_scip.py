"""Evaluator-owned structural check for the Rust SCIP backend split."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Final


_SCIP_ROOT: Final = "crates/codelens-engine/src/scip_backend"
_SCIP_PRODUCTION: Final = ("mod.rs", "parse.rs", "navigation.rs", "call_graph.rs")
_SCIP_PARSE_HELPERS: Final = (
    "short_name",
    "is_definition",
    "parse_range",
    "is_function_like_symbol",
    "body_end_line",
)


def check_codelens_scip_split(candidate: Path) -> tuple[bool, str | None]:
    """Verify live Rust wiring before evaluator-owned SCIP tests are copied."""
    legacy = candidate / "crates/codelens-engine/src/scip_backend.rs"
    if legacy.exists() or legacy.is_symlink():
        return False, "legacy SCIP backend file remains"
    sources: dict[str, str] = {}
    for name in (*_SCIP_PRODUCTION, "tests.rs"):
        source, failure = _read_required(candidate, f"{_SCIP_ROOT}/{name}")
        if failure is not None:
            return False, failure
        sources[name] = source
    masked = {name: _mask_rust_noncode(source) for name, source in sources.items()}

    module = masked["mod.rs"]
    for name in ("call_graph", "navigation", "parse"):
        if not _has_top_level(module, rf"(?m)^[ \t]*mod\s+{name}\s*;"):
            return False, f"private SCIP module wiring is missing: {name}"
    if not _has_test_module_wiring(module):
        return False, "SCIP test module wiring is missing"
    if not _has_top_level(
        module, r"\bpub\s+struct\s+ScipBackend\b[^;{]*(?:\{|;)"
    ):
        return False, "ScipBackend is not public"

    parse = masked["parse.rs"]
    for helper in _SCIP_PARSE_HELPERS:
        pattern = rf"\bpub\s*\(\s*super\s*\)\s+fn\s+{helper}\b"
        if not _has_top_level(parse, pattern):
            return False, f"SCIP parse helper visibility is wrong: {helper}"
    if not _has_top_level(
        masked["navigation.rs"],
        r"\bimpl\s+PreciseBackend\s+for\s+ScipBackend\s*\{",
    ):
        return False, "PreciseBackend implementation is not in navigation"
    if not _has_inherent_call_graph_methods(masked["call_graph.rs"]):
        return False, "SCIP call graph method is missing"
    if not _has_live_test(masked["tests.rs"]):
        return False, "SCIP split has no wired unit tests"
    for name in _SCIP_PRODUCTION:
        if _pure_loc(sources[name]) > 250:
            return False, f"SCIP production module exceeds 250 pure LOC: {name}"
    return True, None


def _has_test_module_wiring(masked: str) -> bool:
    pattern = (
        r"(?m)^[ \t]*#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]"
        r"[ \t]*(?:\r?\n[ \t]*)?mod\s+tests\s*;"
    )
    for match in re.finditer(pattern, masked):
        module = masked.find("mod", match.start(), match.end())
        if (
            not _has_preceding_attribute(masked, match.start())
            and _at_top_level(masked, match.start())
            and _at_top_level(masked, module)
        ):
            return True
    return False


def _has_inherent_call_graph_methods(masked: str) -> bool:
    pattern = r"\bimpl\s+ScipBackend\s*\{"
    for match in re.finditer(pattern, masked):
        if (
            not _at_top_level(masked, match.start())
            or _has_preceding_attribute(masked, match.start())
        ):
            continue
        body_end = _matching_brace(masked, match.end() - 1)
        if body_end is None:
            continue
        body = masked[match.end() : body_end]
        if all(
            _has_top_level(body, rf"\bpub\s+fn\s+{name}\b")
            for name in ("find_callers", "find_callees")
        ):
            return True
    return False


def _has_top_level(masked: str, pattern: str) -> bool:
    return any(
        _at_top_level(masked, match.start())
        and not _has_preceding_attribute(masked, match.start())
        for match in re.finditer(pattern, masked)
    )


def _has_live_test(masked: str) -> bool:
    pattern = r"(?m)^[ \t]*#\s*\[\s*test\s*\][ \t]*(?:\r?\n[ \t]*)?fn\s+\w+"
    return any(
        not _has_preceding_attribute(masked, match.start())
        for match in re.finditer(pattern, masked)
    )


def _has_preceding_attribute(masked: str, position: int) -> bool:
    cursor = position
    while cursor > 0 and masked[cursor - 1].isspace():
        cursor -= 1
    if cursor == 0 or masked[cursor - 1] != "]":
        return False
    depth = 0
    for cursor in range(cursor - 1, -1, -1):
        if masked[cursor] == "]":
            depth += 1
        elif masked[cursor] == "[":
            depth -= 1
            if depth == 0:
                return cursor > 0 and masked[cursor - 1] == "#"
    return False


def _at_top_level(masked: str, position: int) -> bool:
    depth = 0
    for character in masked[:position]:
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
    return depth == 0


def _matching_brace(masked: str, opening: int) -> int | None:
    depth = 0
    for cursor in range(opening, len(masked)):
        if masked[cursor] == "{":
            depth += 1
        elif masked[cursor] == "}":
            depth -= 1
            if depth == 0:
                return cursor
    return None


def _mask_rust_noncode(source: str) -> str:
    masked = list(source)
    cursor = 0
    while cursor < len(source):
        start = cursor
        if source.startswith("//", cursor):
            end = source.find("\n", cursor + 2)
            cursor = len(source) if end < 0 else end
        elif source.startswith("/*", cursor):
            cursor = _block_comment_end(source, cursor)
        else:
            raw_end = _raw_string_end(source, cursor)
            if raw_end is not None:
                cursor = raw_end
            elif source[cursor] == '"':
                cursor = _quoted_end(source, cursor)
            elif source[cursor] == "'":
                character_end = _character_end(source, cursor)
                if character_end is None:
                    cursor += 1
                    continue
                cursor = character_end
            else:
                cursor += 1
                continue
        _mask_span(source, masked, start, cursor)
    return "".join(masked)


def _block_comment_end(source: str, start: int) -> int:
    cursor, depth = start + 2, 1
    while cursor < len(source):
        if source.startswith("/*", cursor):
            depth += 1
            cursor += 2
        elif source.startswith("*/", cursor):
            depth -= 1
            cursor += 2
            if depth == 0:
                return cursor
        else:
            cursor += 1
    return len(source)


def _raw_string_end(source: str, start: int) -> int | None:
    cursor = start + (2 if source.startswith("br", start) else 1)
    if source[start:cursor] not in {"r", "br"}:
        return None
    hashes = cursor
    while cursor < len(source) and source[cursor] == "#":
        cursor += 1
    if cursor >= len(source) or source[cursor] != '"':
        return None
    end = source.find('"' + source[hashes:cursor], cursor + 1)
    return len(source) if end < 0 else end + cursor - hashes + 1


def _quoted_end(source: str, start: int) -> int:
    cursor = start + 1
    while cursor < len(source):
        if source[cursor] == "\\":
            cursor += 2
        elif source[cursor] == '"':
            return cursor + 1
        else:
            cursor += 1
    return len(source)


def _character_end(source: str, start: int) -> int | None:
    cursor = start + 1
    while cursor < len(source) and cursor - start <= 20 and source[cursor] != "\n":
        if source[cursor] == "\\":
            cursor += 2
        elif source[cursor] == "'":
            return cursor + 1
        else:
            cursor += 1
    return None


def _mask_span(source: str, masked: list[str], start: int, end: int) -> None:
    for cursor in range(start, end):
        if source[cursor] != "\n":
            masked[cursor] = " "


def _read_required(candidate: Path, relative: str) -> tuple[str, str | None]:
    path = candidate / relative
    if not path.is_file() or path.is_symlink():
        return "", f"required evaluator file is missing or not regular: {relative}"
    try:
        return path.read_text(encoding="utf-8"), None
    except (OSError, UnicodeError) as error:
        return "", f"cannot read evaluator file {relative}: {error}"


def _pure_loc(source: str) -> int:
    return sum(
        1
        for line in source.splitlines()
        if line.strip() and not line.lstrip().startswith("//")
    )
