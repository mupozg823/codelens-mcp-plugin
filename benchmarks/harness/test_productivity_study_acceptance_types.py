#!/usr/bin/env python3
"""Evasion tests for Signature type-split evaluator acceptance."""

from __future__ import annotations

import tempfile
from pathlib import Path

from productivity_study_acceptance import (
    SIGNATURE_SEQUENCE_TYPES_SPLIT_ID,
    run_evaluator_checks,
)
from test_productivity_study_acceptance import (
    TYPE_LEAVES,
    write_type_candidate,
)


TYPE_ROOT = Path("src/lib/filmPlanner")


def run(candidate: Path) -> tuple[bool, str | None]:
    return run_evaluator_checks(candidate, (SIGNATURE_SEQUENCE_TYPES_SPLIT_ID,))


def test_runtime_star_and_namespace_exports_are_rejected() -> None:
    for value_export in (
        'export * from "./runtime";\n',
        "export namespace Runtime { export const value = 1 }\n",
    ):
        with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_type_candidate(candidate)
            types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
            types_path.write_text(
                types_path.read_text(encoding="utf-8") + value_export,
                encoding="utf-8",
            )

            result = run(candidate)

        assert result[0] is False, value_export
        assert "value export" in (result[1] or "")


def test_modifier_prefixed_value_exports_are_rejected() -> None:
    for value_export in (
        "export async function leaked() {}\n",
        "export abstract class Leaked {}\n",
    ):
        with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_type_candidate(candidate)
            types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
            types_path.write_text(
                types_path.read_text(encoding="utf-8") + value_export,
                encoding="utf-8",
            )

            result = run(candidate)

        assert result[0] is False, value_export
        assert "value export" in (result[1] or "")


def test_relative_and_reexport_runtime_sheet_dependencies_are_rejected() -> None:
    for reverse_dependency in (
        'import type { LegacyType } from "./billboardSequenceSheet";\n',
        "export type { LegacyType } from "
        '\"@/src/lib/filmPlanner/./billboardSequenceSheet\";\n',
    ):
        with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_type_candidate(candidate)
            types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
            types_path.write_text(
                types_path.read_text(encoding="utf-8") + reverse_dependency,
                encoding="utf-8",
            )

            result = run(candidate)

        assert result[0] is False, reverse_dependency
        assert "runtime sheet module" in (result[1] or "")


def test_escaped_runtime_sheet_module_specifier_is_rejected() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + '\nimport type { LegacyType } from "./billboardSequenceSh\\u0065et";\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "runtime sheet module" in (result[1] or "")


def test_require_style_runtime_sheet_dependencies_are_rejected() -> None:
    cases = (
        ("billboardSequenceSheetTypes.ts", "value import"),
        (TYPE_LEAVES[0], "runtime sheet type import"),
    )
    for filename, failure in cases:
        with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_type_candidate(candidate)
            path = candidate / TYPE_ROOT / filename
            path.write_text(
                path.read_text(encoding="utf-8")
                + '\nimport type Legacy = require("./billboardSequenceSheet");\n',
                encoding="utf-8",
            )

            result = run(candidate)

        assert result[0] is False, filename
        assert failure in (result[1] or "")


def test_direct_require_runtime_sheet_dependency_is_rejected() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + '\nconst hidden = require("./billboardSequenceSheet");\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "value import" in (result[1] or "")


def test_parenthesized_require_runtime_sheet_dependency_is_rejected() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + "\ndeclare const require: (specifier: string) => unknown;\n"
            + '(require)("./billboardSequenceSheet");\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "value import" in (result[1] or "")


def test_eval_runtime_sheet_dependency_is_rejected() -> None:
    cases = (
        ("billboardSequenceSheetTypes.ts", "executable code"),
        (TYPE_LEAVES[0], "dynamic evaluation"),
    )
    for filename, failure in cases:
        with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_type_candidate(candidate)
            path = candidate / TYPE_ROOT / filename
            path.write_text(
                path.read_text(encoding="utf-8")
                + "\neval('import(\"./billboardSequenceSheet\")');\n",
                encoding="utf-8",
            )

            result = run(candidate)

        assert result[0] is False, filename
        assert failure in (result[1] or "")


def test_semicolonless_type_alias_cannot_hide_executable_code() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + "\ntype Bypass = unknown\n"
            + "eval('import(\"./billboardSequenceSheet\")');\n",
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "executable code" in (result[1] or "")


def test_leaf_runtime_sheet_type_reexport_is_rejected() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        leaf = candidate / TYPE_ROOT / TYPE_LEAVES[0]
        leaf.write_text(
            leaf.read_text(encoding="utf-8")
            + '\nexport type { BillboardSequenceSheetPlan as LegacyPlan } from '
            '"./billboardSequenceSheet";\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "runtime sheet type import" in (result[1] or "")


def test_leaf_runtime_sheet_side_effect_and_dynamic_imports_are_rejected() -> None:
    for runtime_import in (
        'import "./billboardSequenceSheet";\n',
        'import((("./billboardSequenceSheet")));\n',
        'import(`./billboardSequenceSheet`);\n',
        'import("./billboardSequence" + "Sheet");\n',
    ):
        with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_type_candidate(candidate)
            leaf = candidate / TYPE_ROOT / TYPE_LEAVES[0]
            leaf.write_text(
                leaf.read_text(encoding="utf-8") + "\n" + runtime_import,
                encoding="utf-8",
            )

            result = run(candidate)

        assert result[0] is False, runtime_import
        assert "runtime sheet type import" in (result[1] or "")


def main() -> int:
    tests = [value for name, value in globals().items() if name.startswith("test_")]
    failures = 0
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except AssertionError as error:
            failures += 1
            print(f"FAIL  {test.__name__}: {error}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
