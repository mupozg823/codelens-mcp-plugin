"""Evaluator-owned structural check for the Signature type-only split."""

from __future__ import annotations

import posixpath
import re
from pathlib import Path
from typing import Final

from productivity_study_acceptance_code import (
    actual_statements,
    mask_comments_and_strings,
    module_specifier,
)


_TYPE_ROOT: Final = "src/lib/filmPlanner"
_TYPE_MODULE: Final = "@/src/lib/filmPlanner/billboardSequenceSheetTypes"
_RUNTIME_SHEET_PATH: Final = "src/lib/filmPlanner/billboardSequenceSheet"
_MODULE_EXTENSIONS: Final = (".ts", ".tsx", ".js", ".jsx", ".mjs", ".mts", ".cts")
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


def check_signature_sequence_types(candidate: Path) -> tuple[bool, str | None]:
    """Verify the dedicated type module and every required direct consumer."""
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

    masked_types = mask_comments_and_strings(types)
    masked_sheet = mask_comments_and_strings(sheet)
    for contract in _TYPE_CONTRACTS:
        definition = rf"\bexport\s+(?:type|interface)\s+{re.escape(contract)}\b"
        if re.search(definition, masked_types) is None:
            return False, f"dedicated type contract is missing: {contract}"
        if re.search(definition, masked_sheet) is not None:
            return False, f"old sheet declaration remains: {contract}"

    sheet_exports = actual_statements(sheet, "export")
    if not any(_is_public_type_reexport(statement) for statement in sheet_exports):
        return False, "sheet module lacks the type-only public re-export"
    type_imports = actual_statements(types, "import")
    type_exports = actual_statements(types, "export")
    if any(not _is_type_only_import(statement) for statement in type_imports):
        return False, "dedicated types module contains a value import"
    if _has_value_export(masked_types):
        return False, "dedicated types module contains a value export"
    if any(
        _is_runtime_sheet_module(module_specifier(statement))
        for statement in (*type_imports, *type_exports)
    ):
        return False, "dedicated types module imports the runtime sheet module"

    for leaf in _TYPE_LEAVES:
        source, failure = _read_required(candidate, f"{_TYPE_ROOT}/{leaf}")
        if failure is not None:
            return False, failure
        imports = actual_statements(source, "import")
        if not any(
            _is_type_only_import(statement)
            and module_specifier(statement) == _TYPE_MODULE
            for statement in imports
        ):
            return False, f"leaf module lacks direct type import: {leaf}"
        exports = actual_statements(source, "export")
        if any(
            _is_runtime_sheet_module(module_specifier(statement))
            for statement in (*imports, *exports)
        ):
            return False, f"leaf module retains runtime sheet type import: {leaf}"
    return True, None


def _is_public_type_reexport(statement: str) -> bool:
    masked = mask_comments_and_strings(statement)
    return (
        re.search(r"\bexport\s+type\s+\*\s+from\b", masked) is not None
        and module_specifier(statement) == _TYPE_MODULE
    )


def _is_type_only_import(statement: str) -> bool:
    masked = mask_comments_and_strings(statement)
    return re.search(r"(?m)^\s*import\s+type\b", masked) is not None


def _has_value_export(masked_source: str) -> bool:
    for match in re.finditer(r"\bexport\b", masked_source):
        declaration = masked_source[match.end() :]
        if re.match(r"\s+(?:type|interface)\b", declaration) is None:
            return True
    return False


def _is_runtime_sheet_module(specifier: str | None) -> bool:
    if specifier is None:
        return False
    if specifier.startswith("@/"):
        normalized = posixpath.normpath(specifier[2:])
    elif specifier.startswith("."):
        normalized = posixpath.normpath(f"{_TYPE_ROOT}/{specifier}")
    else:
        return False
    for extension in _MODULE_EXTENSIONS:
        if normalized.endswith(extension):
            normalized = normalized[: -len(extension)]
            break
    return normalized == _RUNTIME_SHEET_PATH


def _read_required(candidate: Path, relative: str) -> tuple[str, str | None]:
    path = candidate / relative
    if not path.is_file() or path.is_symlink():
        return "", f"required evaluator file is missing or not regular: {relative}"
    try:
        return path.read_text(encoding="utf-8"), None
    except (OSError, UnicodeError) as error:
        return "", f"cannot read evaluator file {relative}: {error}"
