#!/usr/bin/env python3
"""Lexical-evasion tests for Signature type-split evaluator acceptance."""

from __future__ import annotations

import tempfile
from pathlib import Path

from productivity_study_acceptance import (
    SIGNATURE_SEQUENCE_TYPES_SPLIT_ID,
    run_evaluator_checks,
)
from test_productivity_study_acceptance import (
    TYPE_CONTRACTS,
    TYPE_LEAVES,
    write_type_candidate,
)


TYPE_ROOT = Path("src/lib/filmPlanner")


def run(candidate: Path) -> tuple[bool, str | None]:
    return run_evaluator_checks(candidate, (SIGNATURE_SEQUENCE_TYPES_SPLIT_ID,))


def test_comment_and_template_contract_dummies_do_not_count() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        declarations = "\n".join(
            f"export interface {name} {{}}" for name in TYPE_CONTRACTS
        )
        (candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts").write_text(
            'import type { Marker } from "@/src/lib/filmPlanner/marker";\n'
            f"/*\n{declarations}\n*/\n"
            f"const dummy = `\n{declarations}\n`;\n",
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "contract is missing" in (result[1] or "")


def test_comment_or_string_leaf_import_dummies_do_not_count() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        leaf = candidate / TYPE_ROOT / TYPE_LEAVES[0]
        leaf.write_text(
            '// import type { BillboardSequenceSheetPlan } from '
            '"@/src/lib/filmPlanner/billboardSequenceSheetTypes";\n'
            'const dummy = `import type { BillboardSequenceSheetPlan } from '
            '"@/src/lib/filmPlanner/billboardSequenceSheetTypes";`;\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "direct type import" in (result[1] or "")


def test_comment_only_public_reexport_does_not_count() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        sheet = candidate / TYPE_ROOT / "billboardSequenceSheet.ts"
        sheet.write_text(
            '// export type * from '
            '"@/src/lib/filmPlanner/billboardSequenceSheetTypes";\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "public re-export" in (result[1] or "")


def test_comment_module_specifier_dummy_does_not_count() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        sheet = candidate / TYPE_ROOT / "billboardSequenceSheet.ts"
        sheet.write_text(
            "export type * /* from "
            '\"@/src/lib/filmPlanner/billboardSequenceSheetTypes\" */ '
            'from "@/src/lib/filmPlanner/wrong";\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "public re-export" in (result[1] or "")


def test_comment_module_specifier_dummy_in_leaf_does_not_count() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        leaf = candidate / TYPE_ROOT / TYPE_LEAVES[0]
        leaf.write_text(
            "import type { BillboardSequenceSheetPlan } /* from "
            '\"@/src/lib/filmPlanner/billboardSequenceSheetTypes\" */ '
            'from "@/src/lib/filmPlanner/wrong";\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "direct type import" in (result[1] or "")


def test_comment_module_specifier_dummy_does_not_hide_reverse_import() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + "\nimport type { BillboardSequenceSheetPlan /* from "
            '\"@/src/lib/filmPlanner/billboardSequenceSheetTypes\" */ } '
            'from "@/src/lib/filmPlanner/billboardSequenceSheet";\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "runtime sheet module" in (result[1] or "")


def test_regex_literal_cannot_hide_value_export() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + "\nconst slash = /\\/\\//; export const leaked = 1;\n",
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "value export" in (result[1] or "")


def test_control_condition_regex_cannot_hide_value_export() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + "\nif (true) /\\/\\//.test(\"value\"); export const leaked = 1;\n",
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "value export" in (result[1] or "")


def test_else_and_do_regex_cannot_hide_value_export() -> None:
    for prefix in ("if (false) {} else", "do"):
        with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_type_candidate(candidate)
            types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
            types_path.write_text(
                types_path.read_text(encoding="utf-8")
                + f'\n{prefix} /\\/\\//.test("value"); export const leaked = 1;\n',
                encoding="utf-8",
            )

            result = run(candidate)

        assert result[0] is False, prefix
        assert "value export" in (result[1] or "")


def test_post_block_regex_cannot_hide_value_export() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + '\nif (true) {} /\\/\\//.test("value"); export const leaked = 1;\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result[0] is False
    assert "value export" in (result[1] or "")


def test_value_export_text_inside_comments_and_strings_is_ignored() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-evasion-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / TYPE_ROOT / "billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + '// export * from "./runtime";\n'
            + 'type Note = "export namespace Runtime";\n',
            encoding="utf-8",
        )

        result = run(candidate)

    assert result == (True, None)


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
