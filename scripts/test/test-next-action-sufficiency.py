#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "benchmarks" / "next-action-sufficiency.py"


def use(tool_id: str, name: str, tool_input: dict, ts: str, side: bool = False) -> dict:
    return {"type": "tool_use", "id": tool_id, "name": name, "input": tool_input, "_ts": ts, "_side": side}


def assistant(blocks: list[dict]) -> dict:
    ts, side = blocks[0]["_ts"], blocks[0]["_side"]
    content = [{k: v for k, v in b.items() if not k.startswith("_")} for b in blocks]
    return {"type": "assistant", "timestamp": ts, "isSidechain": side, "message": {"content": content}}


def result(tool_id: str, text: str, ts: str, side: bool = False) -> dict:
    block = {"type": "tool_result", "tool_use_id": tool_id, "content": [{"type": "text", "text": text}]}
    return {"type": "user", "timestamp": ts, "isSidechain": side, "message": {"content": [block]}}


def run(root: Path) -> dict:
    out = root / "rows.json"
    proc = subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root), "--json", str(out)],
        capture_output=True,
        text=True,
        check=False,
    )
    assert proc.returncode == 0, f"stdout={proc.stdout} stderr={proc.stderr}"
    return json.loads(out.read_text(encoding="utf-8"))


def main() -> None:
    t = "2026-10-20T00:00:0{}Z".format
    events = [
        # refs names src/a.ts; a Grep issued in the same message is not a reaction to it,
        # the later Bash read of that file is.
        assistant([use("r1", "mcp__codelens__search", {"mode": "refs", "symbol_name": "fooBar"}, t(1)),
                   use("g0", "Grep", {"pattern": "unrelated"}, t(1))]),
        result("r1", '{"references":[{"file_path":"src/a.ts","line":3}]}', t(2)),
        result("g0", "", t(2)),
        assistant([use("b1", "Bash", {"command": "cd /repo && sed -n 1,40p src/a.ts"}, t(3))]),
        result("b1", "", t(4)),
        # symbol search, then a grep for the same name
        assistant([use("s1", "mcp__codelens__search", {"mode": "symbol", "names": ["fooBar", "bazQux"]}, t(5))]),
        result("s1", '{"symbols":[{"file_path":"src/b.ts"}]}', t(6)),
        assistant([use("g1", "Grep", {"pattern": "bazQux"}, t(7))]),
        result("g1", "", t(8)),
        # binding call, then a read of a file it never named
        assistant([use("p1", "mcp__codelens__prepare_harness_session", {"project": "/repo"}, t(9))]),
        result("p1", '{"success":true,"project":"/repo"}', t(9)),
        assistant([use("rd", "Read", {"file_path": "/repo/src/c.ts"}, t(9))]),
        result("rd", "", t(9)),
        # an old-layout subagent turn interleaved in the main file is skipped
        assistant([use("x1", "mcp__codelens__graph", {"mode": "callers", "function_name": "f"}, t(9), side=True)]),
        result("x1", "{}", t(9), side=True),
        # an error result, and nothing after it
        assistant([use("e1", "mcp__codelens-lite__graph", {"mode": "impact"}, t(9))]),
        result("e1", '{"success": false, "error": "response_size_limit"}', t(9)),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "proj").mkdir()
        (root / "proj" / "s.jsonl").write_text("\n".join(json.dumps(e) for e in events) + "\n", encoding="utf-8")
        data = run(root)

    rows = {(r["tool"], r["mode"]): r for r in data["rows"]}
    assert len(data["rows"]) == 4, data["rows"]
    assert data["skipped"] == {"old_layout_sidechain": 1}, data["skipped"]

    refs = rows[("search", "refs")]
    assert (refs["category"], refs["next"]) == ("read_returned", "Bash(read)"), refs
    assert refs["same_target"] is False and refs["session"] is False and refs["error"] is False, refs

    symbol = rows[("search", "symbol")]
    assert (symbol["category"], symbol["same_target"]) == ("text_search", True), symbol

    binding = rows[("prepare_harness_session", None)]
    assert (binding["category"], binding["session"]) == ("read_other", True), binding

    failed = rows[("graph", "impact")]
    assert (failed["server"], failed["category"], failed["error"]) == ("codelens-lite", "end", True), failed
    print("next-action-sufficiency: ok")


if __name__ == "__main__":
    main()
