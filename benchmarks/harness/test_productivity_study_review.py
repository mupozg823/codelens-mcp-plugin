#!/usr/bin/env python3
"""Tests for blinded double-review scoring and packet deletion."""

from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from productivity_study_contract import Agent
import productivity_study_review as review


def make_record_dir(root: Path) -> Path:
    record = root / "run"
    record.mkdir()
    (record / "run-manifest.json").write_text(
        json.dumps({"blind_review": {"id": "opaque-id", "status": "pending"}}),
        encoding="utf-8",
    )
    (record / "blind-review-packet.json").write_text(
        json.dumps(
            {
                "review_id": "opaque-id",
                "task_id": "repo::review::001",
                "task_kind": "multi-file-impact-review",
                "prompt": "Review this result.",
                "rubric": ["States impact."],
                "response": "The impact is isolated.",
            }
        ),
        encoding="utf-8",
    )
    return record


def test_double_review_deletes_packet_and_keeps_only_outcomes() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-review-") as raw_tmp:
        record = make_record_dir(Path(raw_tmp))

        manifest = review.complete_blind_review(
            record,
            {Agent.CODEX: "pinned-codex", Agent.CLAUDE: "pinned-claude"},
            lambda _agent, _command, _workdir: '{"type":"result","result":"{\\"passed\\": true}"}',
        )

        assert (record / "blind-review-packet.json").exists() is False
        assert manifest["quality_status"] == "passed"
        assert len(manifest["blind_review"]["outcomes"]) == 2
        assert all(row["raw_evidence"]["raw_deleted"] for row in manifest["blind_review"]["outcomes"])
        assert "The impact is isolated." not in json.dumps(manifest)


def test_missing_or_disagreeing_reviewer_withholds_quality() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-review-") as raw_tmp:
        record = make_record_dir(Path(raw_tmp))

        manifest = review.complete_blind_review(
            record,
            {Agent.CODEX: "pinned-codex", Agent.CLAUDE: "pinned-claude"},
            lambda agent, _command, _workdir: '{"type":"result","result":"{\\"passed\\": %s}"}' % ("true" if agent is Agent.CODEX else "false"),
        )

        assert manifest["quality_status"] == "held"


def test_two_negative_reviews_fail_quality() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-review-") as raw_tmp:
        record = make_record_dir(Path(raw_tmp))

        manifest = review.complete_blind_review(
            record,
            {Agent.CODEX: "pinned-codex", Agent.CLAUDE: "pinned-claude"},
            lambda _agent, _command, _workdir: '{"type":"result","result":"{\\"passed\\": false}"}',
        )

        assert manifest["quality_status"] == "failed"


def test_review_commands_have_no_mcp_configuration_or_source_worktree() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-review-") as raw_tmp:
        workdir = Path(raw_tmp)
        codex = review.reviewer_command(Agent.CODEX, "pinned", "Score.", workdir)
        claude = review.reviewer_command(Agent.CLAUDE, "pinned", "Score.", workdir)

    assert "--config" not in codex
    assert "--mcp-config" not in claude
    assert "--strict-mcp-config" not in claude
    assert "--safe-mode" not in claude
    assert "--cd" in codex
    assert "read-only" in codex
    assert "plan" in claude


def main() -> int:
    tests = [
        test_double_review_deletes_packet_and_keeps_only_outcomes,
        test_missing_or_disagreeing_reviewer_withholds_quality,
        test_two_negative_reviews_fail_quality,
        test_review_commands_have_no_mcp_configuration_or_source_worktree,
    ]
    failures = 0
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except Exception as error:
            failures += 1
            print(f"FAIL  {test.__name__}: {error}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
