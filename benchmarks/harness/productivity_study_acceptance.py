"""Evaluator-owned structural checks for productivity-study candidates."""

from __future__ import annotations

import json
import subprocess
import tempfile
from enum import StrEnum
from pathlib import Path
from typing import Final, Sequence, assert_never

import productivity_study_acceptance_syntax as syntax
from productivity_study_acceptance_scip import check_codelens_scip_split
from productivity_study_acceptance_types import check_signature_sequence_types
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
                result = check_signature_sequence_types(candidate)
            case _CheckId.CODELENS_SCIP_SPLIT:
                result = check_codelens_scip_split(candidate)
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
            if syntax.anchor_has_required_attributes(tag, href)
        ]
        if len(tags) != 1:
            return (
                False,
                f"required download anchor must have exactly one match: {relative}",
            )
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


def _read_required(candidate: Path, relative: str) -> tuple[str, str | None]:
    path = candidate / relative
    if not path.is_file() or path.is_symlink():
        return "", f"required evaluator file is missing or not regular: {relative}"
    try:
        return path.read_text(encoding="utf-8"), None
    except (OSError, UnicodeError) as error:
        return "", f"cannot read evaluator file {relative}: {error}"
