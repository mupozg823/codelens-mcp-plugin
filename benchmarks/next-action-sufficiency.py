#!/usr/bin/env python3
"""Classify what an agent did right after each CodeLens result in Claude Code transcripts.

Method from codegraph's explore-sufficiency study: the first tool call after an
exploration result shows whether the result was enough to act on.

  re_explore     another CodeLens call
  read_returned  Read (or a Bash cat/sed/head/tail/nl) of a file the CodeLens result named
  read_other     the same, of a file it did not name
  text_search    Grep or Glob, or a Bash rg/grep/find/kgrep/kfind/fd/ag/git grep
  proceed        anything else (Edit, Write, other Bash, Agent, LSP, ...)
  end            no further tool call in that transcript

A call issued in parallel with the CodeLens call (same assistant message) is not
a reaction to its result, so "next" is the first tool call after the result.
Old-layout transcripts interleave subagent turns with the main thread; there only
main-thread calls are classified, and subagent files under `subagents/` are read
whole.

Usage:
  python3 benchmarks/next-action-sufficiency.py [--root ~/.claude/projects]
      [--since 2026-04-01] [--until 2026-10-29] [--json out.json]
"""

import argparse
import bisect
import collections
import json
import mmap
import os
import re
import sys
import time

CATEGORIES = ["re_explore", "read_returned", "read_other", "text_search", "proceed", "end"]
EXTENSIONS = (
    "rs|ts|tsx|js|jsx|mjs|cjs|mts|cts|py|go|java|kt|kts|swift|c|h|cc|cpp|hpp|m|mm|rb|php|cs|"
    "scala|lua|dart|ex|exs|vue|svelte|astro|sql|sh|zsh|bash|toml|yaml|yml|json|md|html|css|scss"
)
PATH_RE = re.compile(r"(?<![\w.@+-])((?:/|\./)?(?:[\w@.+-]+/)*[\w@+-][\w@.+-]*\.(?:%s))(?![\w])" % EXTENSIONS)
# Binding and configuration calls are not exploration: what follows them says
# nothing about whether a result was sufficient.
SESSION_TOOLS = {"prepare_harness_session", "get_current_config", "get_capabilities", "set_profile", "set_preset"}
SEARCH_RE = re.compile(r"(?:^|[;&|(]\s*|\bxargs\s+)(?:rg|grep|egrep|kgrep|kfind|find|fd|ag|git\s+grep)\b")
READ_RE = re.compile(r"(?:^|[;&|(]\s*)(?:cat|sed|head|tail|nl|bat|less)\b")


def codelens_server(name):
    # mcp__codelens__search, mcp__codelens-lite__search, mcp__plugin_x_codelens__search
    if not name.startswith("mcp__"):
        return None
    server, _, tool = name[5:].rpartition("__")
    return (server, tool) if "codelens" in server else None


def result_text(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(c.get("text", "") for c in content if isinstance(c, dict))
    return ""


def is_error_result(block, text):
    if block.get("is_error"):
        return True
    head = text[:400].replace(" ", "")
    return '"success":false' in head


def mode_of(tool_input):
    if isinstance(tool_input, dict):
        mode = tool_input.get("mode")
        if isinstance(mode, str):
            return mode
    return None


def returned_paths(text):
    return {m.group(1)[2:] if m.group(1).startswith("./") else m.group(1) for m in PATH_RE.finditer(text)}


def target_names(tool_input):
    """Symbol names or queries the CodeLens call asked about."""
    names = set()
    if isinstance(tool_input, dict):
        nested = tool_input.get("target")
        for src in (tool_input, nested if isinstance(nested, dict) else {}):
            for key in ("name", "names", "symbol", "symbol_name", "symbol_names", "function_name", "query", "pattern"):
                value = src.get(key)
                for item in value if isinstance(value, list) else [value]:
                    if isinstance(item, str) and len(item) >= 3:
                        names.add(item.rsplit("/", 1)[-1])
    return names


def asks_same_target(next_use, names):
    if next_use is None or not names:
        return False
    haystack = json.dumps(next_use["input"], ensure_ascii=False)
    return any(name in haystack for name in names)


def read_hits(read_paths, paths):
    """True when a read path and a returned path name the same file.

    Either side may be relative (results are project-relative, Bash reads are
    relative to wherever the command cd'd), so a suffix match on a path
    boundary counts.
    """
    for a in read_paths:
        for b in paths:
            if a == b or a.endswith("/" + b.lstrip("/")) or b.endswith("/" + a.lstrip("/")):
                return True
    return False


def classify(next_use, paths):
    if next_use is None:
        return "end", None
    name, tool_input = next_use["name"], next_use["input"]
    if codelens_server(name):
        return "re_explore", name
    if name == "Read":
        fp = tool_input.get("file_path") if isinstance(tool_input, dict) else None
        return ("read_returned" if isinstance(fp, str) and read_hits([fp], paths) else "read_other"), name
    if name in ("Grep", "Glob"):
        return "text_search", name
    if name == "Bash" and isinstance(tool_input, dict):
        command = tool_input.get("command") or ""
        if SEARCH_RE.search(command):
            return "text_search", "Bash(search)"
        if READ_RE.search(command):
            read_paths = [m.group(1) for m in PATH_RE.finditer(command)]
            return ("read_returned" if read_hits(read_paths, paths) else "read_other"), "Bash(read)"
    return "proceed", name


def scan_file(path, since, until, rows, skipped):
    whole = "/subagents/" in path
    uses, use_lines, results = [], [], {}
    with open(path, encoding="utf-8", errors="replace") as fh:
        for idx, line in enumerate(fh):
            if '"tool_use"' not in line and '"tool_result"' not in line:
                continue
            try:
                obj = json.loads(line)
            except ValueError:
                continue
            msg = obj.get("message")
            if not isinstance(msg, dict) or not isinstance(msg.get("content"), list):
                continue
            side = bool(obj.get("isSidechain"))
            for block in msg["content"]:
                if not isinstance(block, dict):
                    continue
                if block.get("type") == "tool_use":
                    uses.append({"line": idx, "id": block.get("id"), "name": block.get("name") or "",
                                 "input": block.get("input"), "ts": obj.get("timestamp") or "", "side": side})
                    use_lines.append(idx)
                elif block.get("type") == "tool_result":
                    text = result_text(block.get("content"))
                    results[block.get("tool_use_id")] = (idx, text, is_error_result(block, text))
    for use in uses:
        cl = codelens_server(use["name"])
        if not cl:
            continue
        ts = use["ts"][:10]
        if (since and ts < since) or (until and ts >= until):
            continue
        if use["side"] and not whole:
            skipped["old_layout_sidechain"] += 1
            continue
        res = results.get(use["id"])
        if res is None:
            skipped["no_result"] += 1
            continue
        res_line, text, error = res
        nxt = None
        for j in range(bisect.bisect_right(use_lines, res_line), len(uses)):
            cand = uses[j]
            if whole or not cand["side"]:
                nxt = cand
                break
        category, next_name = classify(nxt, returned_paths(text))
        same = asks_same_target(nxt, target_names(use["input"]))
        rows.append({"server": cl[0], "tool": cl[1], "mode": mode_of(use["input"]), "month": use["ts"][:7],
                     "session": cl[1] in SESSION_TOOLS, "same_target": same, "file": path, "line": use["line"],
                     "error": error, "category": category, "next": next_name})


def table(title, groups, min_n):
    out = [f"### {title}", "", "| group | n | " + " | ".join(CATEGORIES) + " | same-target searches | error |",
           "| --- | ---: | " + " | ".join("---:" for _ in CATEGORIES) + " | ---: | ---: |"]
    for key, items in groups:
        n = len(items)
        if n < min_n:
            continue
        counts = collections.Counter(r["category"] for r in items)
        errors = sum(r["error"] for r in items)
        cells = " | ".join(f"{100 * counts[c] / n:.0f}%" for c in CATEGORIES)
        searches = [r for r in items if r["category"] == "text_search"]
        same = sum(r["same_target"] for r in searches)
        same_cell = f"{same}/{len(searches)}" if searches else "-"
        out.append(f"| {key} | {n} | {cells} | {same_cell} | {100 * errors / n:.0f}% |")
    return "\n".join(out) + "\n"


def grouped(rows, key):
    buckets = collections.defaultdict(list)
    for r in rows:
        buckets[key(r)].append(r)
    return sorted(buckets.items(), key=lambda kv: -len(kv[1]))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--root", default=os.path.expanduser("~/.claude/projects"))
    ap.add_argument("--since", help="first call date, YYYY-MM-DD")
    ap.add_argument("--until", help="exclusive end date, YYYY-MM-DD")
    ap.add_argument("--min-n", type=int, default=20, help="hide groups smaller than this")
    ap.add_argument("--json", help="write rows and counts here")
    args = ap.parse_args()

    started = time.time()
    rows, skipped = [], collections.Counter()
    files = scanned = 0
    for d, _, names in os.walk(args.root):
        for f in names:
            if not f.endswith(".jsonl"):
                continue
            files += 1
            p = os.path.join(d, f)
            try:
                if os.path.getsize(p) == 0:
                    continue
                with open(p, "rb") as fh, mmap.mmap(fh.fileno(), 0, access=mmap.ACCESS_READ) as m:
                    # Byte checks only, so the filter cannot depend on JSON spacing.
                    if m.find(b"mcp__") == -1 or m.find(b"codelens") == -1:
                        continue
                scanned += 1
                scan_file(p, args.since, args.until, rows, skipped)
            except OSError as exc:
                skipped[f"os_error:{exc.__class__.__name__}"] += 1

    real = [r for r in rows if not r["error"]]
    explore = [r for r in real if not r["session"]]
    print(f"transcripts {files}, candidates parsed {scanned}, classified CodeLens calls {len(rows)} "
          f"({len(rows) - len(real)} errors), skipped {dict(skipped)}, {time.time() - started:.0f}s\n")
    print(table("All calls", [("all", rows), ("successful", real), ("successful exploration", explore),
                              ("successful session/config", [r for r in real if r["session"]])], 1))
    print(table("By server (successful exploration)", grouped(explore, lambda r: r["server"]), 1))
    print(table("By tool and mode (successful)",
                grouped(real, lambda r: r["tool"] + (f" mode={r['mode']}" if r["mode"] else "")), args.min_n))
    print(table("By month (successful exploration)", sorted(grouped(explore, lambda r: r["month"])), 1))
    proceed = collections.Counter(r["next"] for r in explore if r["category"] == "proceed")
    print("Exploration then proceed, next tool: " + ", ".join(f"{k} {v}" for k, v in proceed.most_common(10)))
    if args.json:
        with open(args.json, "w") as fh:
            json.dump({"files": files, "scanned": scanned, "skipped": skipped, "rows": rows}, fh)


if __name__ == "__main__":
    sys.exit(main())
