#!/usr/bin/env python3
"""Focused tests for evaluator-owned productivity-study acceptance checks."""

from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_acceptance as acceptance


STRICT_GUARD = r"""import fs from "node:fs";
const text = fs.readFileSync("src/components/Fixture.tsx", "utf8");
const tags = text.match(/<a\b[^>]*>/gs) ?? [];
const bad = tags.some((tag) => /\sdownload(?:\s|=|>)/.test(tag) &&
  (!/target\s*=\s*["']_blank["']/.test(tag) ||
   !/rel\s*=\s*["'][^"']*\bnoopener\b[^"']*["']/.test(tag) ||
   !/rel\s*=\s*["'][^"']*\bnoreferrer\b[^"']*["']/.test(tag)));
process.exit(bad ? 1 : 0);
"""
TARGET_ONLY_GUARD = r"""import fs from "node:fs";
const text = fs.readFileSync("src/components/Fixture.tsx", "utf8");
const tags = text.match(/<a\b[^>]*>/gs) ?? [];
process.exit(tags.some((tag) => /\sdownload/.test(tag) &&
  !/target\s*=\s*["']_blank["']/.test(tag)) ? 1 : 0);
"""
SAFE_SEQUENCE_ANCHOR = (
    '<a href={sequenceSheet!.url} download target="_blank" '
    'rel="noopener noreferrer">sheet</a>\n'
)
SAFE_GALLERY_ANCHOR = (
    '<a href={job.gif_path} download target="_blank" '
    'rel="noopener noreferrer">gif</a>\n'
)
TYPE_CONTRACTS = tuple(
    (
        "BillboardSequenceSheetPresetId BillboardSequenceSheetLayout "
        "BillboardSequenceSheetDurationSec BillboardSequenceSheetQuality "
        "BillboardSequenceSheetOutputFormat BillboardSequenceSheetPreset "
        "BillboardSequenceCharacterInput BuildBillboardSequenceSheetPlanInput "
        "BillboardSequenceSheetTextPolicy BillboardSequenceDisplayText "
        "BillboardSequenceCropPlanItem BillboardSequenceImagePrompt "
        "CharacterAssetBibleEntry CharacterAssetBible "
        "BillboardSequenceSheetCell BillboardSequenceSheetPlan"
    ).split()
)
TYPE_LEAVES = (
    "billboardSequenceHandoffTypes.ts",
    "billboardSequenceKlingPrompt.ts",
    "billboardSequenceSheetRequest.ts",
    "billboardTakePlanContract.ts",
)


def write(root: Path, relative: str, content: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def write_anchor_candidate(root: Path, script: str, guard: str) -> None:
    write(
        root,
        "package.json",
        json.dumps({"scripts": {"guard:anchor-download": script}}),
    )
    write(root, "scripts/guards/anchor-download-target.mjs", guard)
    write(root, "src/components/film-v2/ResultCanvas.tsx", SAFE_SEQUENCE_ANCHOR)
    write(root, "src/components/gallery/UserGalleryPanels.tsx", SAFE_GALLERY_ANCHOR)


def write_type_candidate(root: Path) -> None:
    contracts = "\n".join(
        f"export interface {name} {{ readonly marker?: Marker }}"
        for name in TYPE_CONTRACTS
    )
    write(
        root,
        "src/lib/filmPlanner/billboardSequenceSheetTypes.ts",
        'import type { Marker } from "@/src/lib/filmPlanner/marker";\n' + contracts,
    )
    write(
        root,
        "src/lib/filmPlanner/billboardSequenceSheet.ts",
        'export type * from "@/src/lib/filmPlanner/billboardSequenceSheetTypes";\n'
        "import type { BillboardSequenceSheetPlan } from "
        '"@/src/lib/filmPlanner/billboardSequenceSheetTypes";\n',
    )
    for leaf in TYPE_LEAVES:
        write(
            root,
            f"src/lib/filmPlanner/{leaf}",
            "import type { BillboardSequenceSheetPlan } from "
            '"@/src/lib/filmPlanner/billboardSequenceSheetTypes";\n',
        )


def write_scip_candidate(root: Path, *, wire_tests: bool = True) -> None:
    test_wiring = "#[cfg(test)]\nmod tests;\n" if wire_tests else ""
    write(
        root,
        "crates/codelens-engine/src/scip_backend/mod.rs",
        "mod call_graph;\nmod navigation;\nmod parse;\n"
        f"{test_wiring}pub struct ScipBackend {{}}\n",
    )
    helpers = "\n".join(
        f"pub(super) fn {name}() {{}}"
        for name in (
            "short_name",
            "is_definition",
            "parse_range",
            "is_function_like_symbol",
            "body_end_line",
        )
    )
    write(root, "crates/codelens-engine/src/scip_backend/parse.rs", helpers)
    write(
        root,
        "crates/codelens-engine/src/scip_backend/navigation.rs",
        "impl PreciseBackend for ScipBackend {}\n",
    )
    write(
        root,
        "crates/codelens-engine/src/scip_backend/call_graph.rs",
        "impl ScipBackend {\n"
        "pub fn find_callees(&self) {}\n"
        "pub fn find_callers(&self) {}\n}\n",
    )
    write(
        root,
        "crates/codelens-engine/src/scip_backend/tests.rs",
        "#[test]\nfn split_works() {}\n",
    )


def run(candidate: Path, check_id: str) -> tuple[bool, str | None]:
    return acceptance.run_evaluator_checks(candidate, (check_id,))


def test_supported_ids_are_explicit_and_unknown_ids_fail_closed() -> None:
    expected = (
        "signature-anchor-download-v1",
        "signature-sequence-types-split-v1",
        "codelens-scip-split-v1",
    )
    with tempfile.TemporaryDirectory(prefix="study-acceptance-") as raw_tmp:
        result = run(Path(raw_tmp), "made-up-check")

    assert acceptance.SUPPORTED_CHECK_IDS == expected
    assert result[0] is False
    assert result[1] == "unsupported evaluator check: made-up-check"


def test_anchor_check_rejects_base_noop_and_accepts_target_shape() -> None:
    with tempfile.TemporaryDirectory(prefix="study-anchor-") as raw_tmp:
        candidate = Path(raw_tmp)
        write(candidate, "package.json", '{"scripts": {}}')
        assert run(candidate, acceptance.SIGNATURE_ANCHOR_DOWNLOAD_ID)[0] is False
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        result = run(candidate, acceptance.SIGNATURE_ANCHOR_DOWNLOAD_ID)

    assert result == (True, None)


def test_anchor_check_rejects_target_only_guard() -> None:
    with tempfile.TemporaryDirectory(prefix="study-anchor-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            TARGET_ONLY_GUARD,
        )
        result = run(candidate, acceptance.SIGNATURE_ANCHOR_DOWNLOAD_ID)

    assert result[0] is False
    assert "target-only" in (result[1] or "")


def test_anchor_check_rejects_script_true_evasion() -> None:
    with tempfile.TemporaryDirectory(prefix="study-anchor-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(candidate, "true", STRICT_GUARD)
        result = run(candidate, acceptance.SIGNATURE_ANCHOR_DOWNLOAD_ID)

    assert result[0] is False
    assert "package script" in (result[1] or "")


def test_type_split_rejects_base_noop_and_accepts_target_shape() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-") as raw_tmp:
        candidate = Path(raw_tmp)
        write(
            candidate,
            "src/lib/filmPlanner/billboardSequenceSheet.ts",
            "export interface BillboardSequenceSheetPlan {}\n",
        )
        assert run(candidate, acceptance.SIGNATURE_SEQUENCE_TYPES_SPLIT_ID)[0] is False
        write_type_candidate(candidate)
        result = run(candidate, acceptance.SIGNATURE_SEQUENCE_TYPES_SPLIT_ID)

    assert result == (True, None)


def test_type_split_rejects_dummy_file() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        write(
            candidate,
            "src/lib/filmPlanner/billboardSequenceSheetTypes.ts",
            "export interface Dummy {}\n",
        )
        result = run(candidate, acceptance.SIGNATURE_SEQUENCE_TYPES_SPLIT_ID)

    assert result[0] is False


def test_type_split_rejects_old_declarations() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        sheet = candidate / "src/lib/filmPlanner/billboardSequenceSheet.ts"
        sheet.write_text(
            sheet.read_text(encoding="utf-8")
            + "export interface BillboardSequenceSheetPlan {}\n",
            encoding="utf-8",
        )
        result = run(candidate, acceptance.SIGNATURE_SEQUENCE_TYPES_SPLIT_ID)

    assert result[0] is False
    assert "old sheet declaration" in (result[1] or "")


def test_type_split_rejects_value_or_reverse_import_evasions() -> None:
    with tempfile.TemporaryDirectory(prefix="study-types-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_type_candidate(candidate)
        types_path = candidate / "src/lib/filmPlanner/billboardSequenceSheetTypes.ts"
        types_path.write_text(
            types_path.read_text(encoding="utf-8")
            + 'import { runtime } from "@/src/lib/filmPlanner/billboardSequenceSheet";\n'
            + "export const leaked = runtime;\n",
            encoding="utf-8",
        )
        result = run(candidate, acceptance.SIGNATURE_SEQUENCE_TYPES_SPLIT_ID)

    assert result[0] is False


def test_scip_split_rejects_base_noop_and_accepts_target_shape() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-") as raw_tmp:
        candidate = Path(raw_tmp)
        write(
            candidate,
            "crates/codelens-engine/src/scip_backend.rs",
            "pub struct ScipBackend;\n",
        )
        assert run(candidate, acceptance.CODELENS_SCIP_SPLIT_ID)[0] is False
        (candidate / "crates/codelens-engine/src/scip_backend.rs").unlink()
        write_scip_candidate(candidate)
        result = run(candidate, acceptance.CODELENS_SCIP_SPLIT_ID)

    assert result == (True, None)


def test_scip_split_rejects_missing_test_wiring() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_scip_candidate(candidate, wire_tests=False)
        result = run(candidate, acceptance.CODELENS_SCIP_SPLIT_ID)

    assert result[0] is False
    assert "test module wiring" in (result[1] or "")


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
