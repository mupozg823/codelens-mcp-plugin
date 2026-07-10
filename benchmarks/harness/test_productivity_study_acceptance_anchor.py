#!/usr/bin/env python3
"""Adversarial anchor acceptance tests for productivity-study candidates."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_acceptance as acceptance
from test_productivity_study_acceptance import (
    STRICT_GUARD,
    write,
    write_anchor_candidate,
)

REL_PRESENT_GUARD = r"""import fs from "node:fs";
const text = fs.readFileSync("src/components/Fixture.tsx", "utf8");
const tags = text.match(/<a\b[^>]*>/gs) ?? [];
const bad = tags.some((tag) => /\sdownload/.test(tag) &&
  (!/target\s*=\s*["']_blank["']/.test(tag) || !/rel\s*=\s*["'][^"']+["']/.test(tag)));
process.exit(bad ? 1 : 0);
"""


def run(candidate: Path) -> tuple[bool, str | None]:
    return acceptance.run_evaluator_checks(
        candidate,
        (acceptance.SIGNATURE_ANCHOR_DOWNLOAD_ID,),
    )


def test_anchor_check_rejects_comment_dummy_anchor() -> None:
    dummy = (
        '/* <a href={sequenceSheet!.url} download target="_blank" '
        'rel="noopener noreferrer"> */\n'
        '// <a href={sequenceSheet!.url} download target="_blank" '
        'rel="noopener noreferrer">\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-comment-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", dummy)
        result = run(candidate)

    assert result[0] is False
    assert "required download anchor" in (result[1] or "")


def test_anchor_check_rejects_quoted_dummy_anchors() -> None:
    dummy = (
        'const single = \'<a href={sequenceSheet!.url} download target="_blank" '
        'rel="noopener noreferrer">\';\n'
        "const double = \"<a href={sequenceSheet!.url} download target='_blank' "
        "rel='noopener noreferrer'>" + '";\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-quote-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", dummy)
        result = run(candidate)

    assert result[0] is False
    assert "required download anchor" in (result[1] or "")


def test_anchor_check_rejects_template_dummy_anchor() -> None:
    dummy = (
        "const template = `<a href={sequenceSheet!.url} download "
        'target="_blank" rel="noopener noreferrer">`;\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-template-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", dummy)
        result = run(candidate)

    assert result[0] is False
    assert "required download anchor" in (result[1] or "")


def test_anchor_check_accepts_whitespace_varied_href_expressions() -> None:
    sequence = (
        '<a href = { sequenceSheet! . url } download target="_blank" '
        'rel="noopener noreferrer">sheet</a>\n'
    )
    gallery = (
        '<a href = { job . gif_path } download target="_blank" '
        'rel="noopener noreferrer">gif</a>\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-space-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", sequence)
        write(candidate, "src/components/gallery/UserGalleryPanels.tsx", gallery)
        result = run(candidate)

    assert result == (True, None)


def test_anchor_check_respects_quotes_and_braces_inside_opening_tag() -> None:
    sequence = (
        '<a title="1 > 0" data-meta={{ label: "x > y" }} '
        'href={sequenceSheet!.url} download target="_blank" '
        'rel="noopener noreferrer">sheet</a>\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-nested-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", sequence)
        result = run(candidate)

    assert result == (True, None)


def test_anchor_check_rejects_duplicate_live_surface_anchor() -> None:
    duplicate = (
        '<a href={sequenceSheet!.url} download target="_blank" '
        'rel="noopener noreferrer">one</a>\n'
    ) * 2
    with tempfile.TemporaryDirectory(prefix="study-anchor-duplicate-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", duplicate)
        result = run(candidate)

    assert result[0] is False
    assert "exactly one" in (result[1] or "")


def test_anchor_check_rejects_attribute_value_and_data_attribute_dummies() -> None:
    sequence = (
        '<a href={wrong.url} download data-target="_blank" '
        'data-rel="noopener noreferrer" '
        'title=\'href={sequenceSheet!.url} target="_blank" '
        'rel="noopener noreferrer"\'>wrong</a>\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-attrs-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", sequence)
        result = run(candidate)

    assert result[0] is False
    assert "exactly one" in (result[1] or "")


def test_anchor_check_rejects_data_href_as_real_href() -> None:
    sequence = (
        '<a data-href={sequenceSheet!.url} download target="_blank" '
        'rel="noopener noreferrer">wrong</a>\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-data-href-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", sequence)
        result = run(candidate)

    assert result[0] is False
    assert "exactly one" in (result[1] or "")


def test_anchor_check_rejects_duplicate_target_and_rel_attributes() -> None:
    sequence = (
        '<a href={sequenceSheet!.url} download target="_blank" target="_self" '
        'rel="noopener noreferrer" rel="nofollow">wrong</a>\n'
    )
    with tempfile.TemporaryDirectory(prefix="study-anchor-duplicates-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            STRICT_GUARD,
        )
        write(candidate, "src/components/film-v2/ResultCanvas.tsx", sequence)
        result = run(candidate)

    assert result[0] is False
    assert "exactly one" in (result[1] or "")


def test_anchor_check_rejects_guard_that_accepts_arbitrary_rel() -> None:
    with tempfile.TemporaryDirectory(prefix="study-anchor-rel-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_anchor_candidate(
            candidate,
            "node scripts/guards/anchor-download-target.mjs",
            REL_PRESENT_GUARD,
        )
        result = run(candidate)

    assert result[0] is False
    assert "rel=nofollow" in (result[1] or "")


def main() -> int:
    failures = 0
    for name, test in globals().copy().items():
        if not name.startswith("test_"):
            continue
        try:
            test()
            print(f"PASS  {name}")
        except AssertionError as error:
            failures += 1
            print(f"FAIL  {name}: {error}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
