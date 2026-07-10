"""Evaluator-owned structural checks for productivity-study candidates."""

from __future__ import annotations

import json
import re
import subprocess
import tempfile
from enum import StrEnum
from pathlib import Path
from typing import Final, Sequence, assert_never

import productivity_study_acceptance_syntax as syntax
from productivity_study_home import isolated_study_environment


class _CheckId(StrEnum):
    SIGNATURE_ANCHOR_DOWNLOAD = "signature-anchor-download-v1"
    SIGNATURE_SEQUENCE_TYPES_SPLIT = "signature-sequence-types-split-v1"
    CODELENS_SCIP_SPLIT = "codelens-scip-split-v1"


SIGNATURE_ANCHOR_DOWNLOAD_ID: Final = _CheckId.SIGNATURE_ANCHOR_DOWNLOAD.value
SIGNATURE_SEQUENCE_TYPES_SPLIT_ID: Final = _CheckId.SIGNATURE_SEQUENCE_TYPES_SPLIT.value
CODELENS_SCIP_SPLIT_ID: Final = _CheckId.CODELENS_SCIP_SPLIT.value
SUPPORTED_CHECK_IDS: Final = tuple(check_id.value for check_id in _CheckId)

_ANCHOR_SCRIPT: Final = "node scripts/guards/anchor-download-target.mjs"
_ANCHOR_GUARD: Final = "scripts/guards/anchor-download-target.mjs"
_ANCHOR_SURFACES: Final = (
    ("src/components/film-v2/ResultCanvas.tsx", "sequenceSheet!.url"),
    ("src/components/gallery/UserGalleryPanels.tsx", "job.gif_path"),
)
_TYPE_ROOT: Final = "src/lib/filmPlanner"
_TYPE_MODULE: Final = "@/src/lib/filmPlanner/billboardSequenceSheetTypes"
_TYPE_CONTRACTS: Final = tuple(
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
_TYPE_LEAVES: Final = (
    "billboardSequenceHandoffTypes.ts",
    "billboardSequenceKlingPrompt.ts",
    "billboardSequenceSheetRequest.ts",
    "billboardTakePlanContract.ts",
)
_SCIP_ROOT: Final = "crates/codelens-engine/src/scip_backend"
_SCIP_PRODUCTION: Final = ("mod.rs", "parse.rs", "navigation.rs", "call_graph.rs")
_SCIP_PARSE_HELPERS: Final = (
    "short_name",
    "is_definition",
    "parse_range",
    "is_function_like_symbol",
    "body_end_line",
)


def run_evaluator_checks(
    candidate: Path,
    check_ids: Sequence[str],
) -> tuple[bool, str | None]:
    """Run named evaluator checks, rejecting unknown identifiers."""
    for check_id in check_ids:
        try:
            parsed = _CheckId(check_id)
        except ValueError:
            return False, f"unsupported evaluator check: {check_id}"
        match parsed:
            case _CheckId.SIGNATURE_ANCHOR_DOWNLOAD:
                result = _check_signature_anchor_download(candidate)
            case _CheckId.SIGNATURE_SEQUENCE_TYPES_SPLIT:
                result = _check_signature_sequence_types(candidate)
            case _CheckId.CODELENS_SCIP_SPLIT:
                result = _check_codelens_scip_split(candidate)
            case unreachable:
                assert_never(unreachable)
        if not result[0]:
            return result
    return True, None


def _check_signature_anchor_download(candidate: Path) -> tuple[bool, str | None]:
    package, failure = _read_required(candidate, "package.json")
    if failure is not None:
        return False, failure
    try:
        payload = json.loads(package)
    except json.JSONDecodeError as error:
        return False, f"invalid package.json: {error.msg}"
    try:
        script = payload["scripts"]["guard:anchor-download"]
    except (KeyError, TypeError):
        return False, "anchor package script is missing"
    if script != _ANCHOR_SCRIPT:
        return False, "anchor package script must invoke the candidate guard exactly"

    for relative, href in _ANCHOR_SURFACES:
        source, failure = _read_required(candidate, relative)
        if failure is not None:
            return False, failure
        tags = [
            tag
            for tag in syntax.extract_jsx_anchor_opening_tags(source)
            if syntax.anchor_href_matches(tag, href)
            and re.search(r"\sdownload(?:\s|=|/|>)", tag)
        ]
        if len(tags) != 1:
            return (
                False,
                f"required download anchor must have exactly one match: {relative}",
            )
        tag = tags[0]
        if re.search(r'\btarget\s*=\s*["\']_blank["\']', tag) is None:
            return False, f"download anchor lacks target blank: {relative}"
        rel = re.search(r'\brel\s*=\s*["\']([^"\']*)["\']', tag)
        if rel is None or not {"noopener", "noreferrer"}.issubset(rel.group(1).split()):
            return False, f"download anchor lacks safe rel tokens: {relative}"
    return _mutation_check_anchor_guard(candidate)


def _mutation_check_anchor_guard(candidate: Path) -> tuple[bool, str | None]:
    guard_path = candidate / _ANCHOR_GUARD
    if not guard_path.is_file() or guard_path.is_symlink():
        return False, "candidate anchor guard is missing or not a regular file"
    fixtures = (
        ("missing target and rel", "", "", False),
        ("target-only", ' target="_blank"', "", False),
        ("rel=nofollow", ' target="_blank"', ' rel="nofollow"', False),
        ("noopener-only", ' target="_blank"', ' rel="noopener"', False),
        ("noreferrer-only", ' target="_blank"', ' rel="noreferrer"', False),
        ("safe rel without target", "", ' rel="noopener noreferrer"', False),
        ("fully safe", ' target="_blank"', ' rel="noopener noreferrer"', True),
    )
    with isolated_study_environment(candidate) as environment:
        for label, target, rel, should_pass in fixtures:
            content = f"<a href={{asset}} download{target}{rel}>asset</a>\n"
            with tempfile.TemporaryDirectory(
                prefix="codelens-guard-mutation-"
            ) as raw_tmp:
                fixture_root = Path(raw_tmp)
                fixture_root.chmod(0o700)
                fixture = fixture_root / "src/components/Fixture.tsx"
                fixture.parent.mkdir(parents=True)
                fixture.write_text(content, encoding="utf-8")
                try:
                    completed = subprocess.run(
                        ["node", str(guard_path.resolve())],
                        cwd=fixture_root,
                        env=environment,
                        check=False,
                        capture_output=True,
                        text=True,
                    )
                except OSError as error:
                    return False, f"cannot execute candidate anchor guard: {error}"
            passed = completed.returncode == 0
            if passed != should_pass:
                return False, f"candidate anchor guard mishandled {label} mutation"
    return True, None


def _check_signature_sequence_types(candidate: Path) -> tuple[bool, str | None]:
    types, failure = _read_required(
        candidate, f"{_TYPE_ROOT}/billboardSequenceSheetTypes.ts"
    )
    if failure is not None:
        return False, failure
    sheet, failure = _read_required(
        candidate, f"{_TYPE_ROOT}/billboardSequenceSheet.ts"
    )
    if failure is not None:
        return False, failure
    for contract in _TYPE_CONTRACTS:
        definition = rf"\bexport\s+(?:type|interface)\s+{re.escape(contract)}\b"
        if re.search(definition, types) is None:
            return False, f"dedicated type contract is missing: {contract}"
        if re.search(definition, sheet) is not None:
            return False, f"old sheet declaration remains: {contract}"
    public_reexport = (
        rf"export\s+type\s+\*\s+from\s+[\"\']{re.escape(_TYPE_MODULE)}[\"\']"
    )
    if re.search(public_reexport, sheet) is None:
        return False, "sheet module lacks the type-only public re-export"
    if re.search(r"(?m)^\s*import\s+(?!type\b)", types) is not None:
        return False, "dedicated types module contains a value import"
    if re.search(
        r"(?m)^\s*export\s+(?:const|let|var|function|class|enum|default|\{)", types
    ):
        return False, "dedicated types module contains a value export"
    reverse = r"from\s+[\"\']@/src/lib/filmPlanner/billboardSequenceSheet[\"\']"
    if re.search(reverse, types) is not None:
        return False, "dedicated types module imports the runtime sheet module"
    direct_import = rf"import\s+type\b[^;]*from\s+[\"\']{re.escape(_TYPE_MODULE)}[\"\']"
    for leaf in _TYPE_LEAVES:
        source, failure = _read_required(candidate, f"{_TYPE_ROOT}/{leaf}")
        if failure is not None:
            return False, failure
        if re.search(direct_import, source, flags=re.DOTALL) is None:
            return False, f"leaf module lacks direct type import: {leaf}"
        if re.search(reverse, source) is not None:
            return False, f"leaf module retains runtime sheet type import: {leaf}"
    return True, None


def _check_codelens_scip_split(candidate: Path) -> tuple[bool, str | None]:
    legacy = candidate / "crates/codelens-engine/src/scip_backend.rs"
    if legacy.exists() or legacy.is_symlink():
        return False, "legacy SCIP backend file remains"
    sources: dict[str, str] = {}
    for name in (*_SCIP_PRODUCTION, "tests.rs"):
        source, failure = _read_required(candidate, f"{_SCIP_ROOT}/{name}")
        if failure is not None:
            return False, failure
        sources[name] = source
    module = sources["mod.rs"]
    for name in ("call_graph", "navigation", "parse"):
        if re.search(rf"(?m)^\s*mod\s+{name}\s*;\s*$", module) is None:
            return False, f"private SCIP module wiring is missing: {name}"
    test_wiring = r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*mod\s+tests\s*;"
    if re.search(test_wiring, module) is None:
        return False, "SCIP test module wiring is missing"
    if re.search(r"\bpub\s+struct\s+ScipBackend\b", module) is None:
        return False, "ScipBackend is not public"
    for helper in _SCIP_PARSE_HELPERS:
        if (
            re.search(
                rf"\bpub\s*\(\s*super\s*\)\s+fn\s+{helper}\b", sources["parse.rs"]
            )
            is None
        ):
            return False, f"SCIP parse helper visibility is wrong: {helper}"
    if (
        re.search(
            r"\bimpl\s+PreciseBackend\s+for\s+ScipBackend\b", sources["navigation.rs"]
        )
        is None
    ):
        return False, "PreciseBackend implementation is not in navigation"
    for method in ("find_callers", "find_callees"):
        if re.search(rf"\bpub\s+fn\s+{method}\b", sources["call_graph.rs"]) is None:
            return False, f"SCIP call graph method is missing: {method}"
    if re.search(r"#\s*\[\s*test\s*\]", sources["tests.rs"]) is None:
        return False, "SCIP split has no wired unit tests"
    for name in _SCIP_PRODUCTION:
        if _pure_loc(sources[name]) > 250:
            return False, f"SCIP production module exceeds 250 pure LOC: {name}"
    return True, None


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
